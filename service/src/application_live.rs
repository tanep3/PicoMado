// Transient projections and guarded view merge; no storage or plugin-ID rules.

impl App {
    pub(crate) fn transient_input_snapshot(
        &self,
        message: &crate::protocol::WireMessage,
    ) -> Option<Self> {
        use crate::protocol::WireMessage as W;
        let unix = self.clock.unix_seconds();
        let today = self.clock.today_jst();
        let eligible = match message {
            W::ReceiptAck { .. } | W::RenderedAck { .. } | W::StreamStatus { .. } => true,
            W::EnterSub { .. } | W::Up { .. } | W::Down { .. } | W::Power { .. } => {
                // The bound transport session is checked by the unchanged
                // message processor; choose a path only, without applying Rules.
                false
            }
            _ => false,
        };
        if !eligible {
            return None;
        }
        let mut next = self.transient_snapshot();
        next.clock = Arc::new(InputClock { unix, today });
        Some(next)
    }
    pub(crate) fn transient_button_snapshot(
        &self,
        token: &SessionToken,
        message: &crate::protocol::WireMessage,
    ) -> Option<Self> {
        use crate::protocol::WireMessage as W;
        if let Some(snapshot) = self.transient_input_snapshot(message) {
            return Some(snapshot);
        }
        let action = match message {
            W::EnterSub { .. } => ButtonAction::Sub,
            W::Up { .. } => ButtonAction::Up,
            W::Down { .. } => ButtonAction::Down,
            W::Power { .. } => ButtonAction::Power,
            _ => return None,
        };
        let d = self.runtime.world.devices.get(&token.device_id)?;
        if !session_current(d, token)
            || !matches!(
                (&d.mode, action),
                (Mode::Normal, ButtonAction::Sub) | (Mode::Sub, _)
            )
        {
            return None;
        }
        let today = self.clock.today_jst();
        let unix = self.clock.unix_seconds();
        let ctx = PluginContext {
            token: token.clone(),
            settings_revision: d.settings_revision,
            mode: d.mode.clone(),
        };
        if !self
            .contents
            .get(d.selected.as_ref()?)?
            .transient_input_ready(&ctx, &today)
        {
            return None;
        }
        let mut next = self.transient_snapshot();
        next.clock = Arc::new(InputClock { unix, today });
        Some(next)
    }
    pub(crate) fn transient_snapshot(&self) -> Self {
        let mut s = self.published_snapshot();
        s.transient_only = true;
        s
    }
    pub(crate) fn finish_transient_snapshot(&mut self, previous: &Self) {
        self.clock = previous.clock.clone();
        self.transient_only = false;
        self.transient_changed = false;
    }
    pub(crate) fn transient_was_changed(&self) -> bool {
        self.transient_changed
    }
    pub(crate) fn notify_published(&self) {
        self.dispatch.notify_published();
    }
    pub(crate) fn complete_transient_effect(&mut self, id: u64) -> Result<bool, AppError> {
        if !self.runtime.outbox.contains_key(&id) {
            return Ok(false);
        }
        if matches!(self.runtime.outbox.get(&id), Some(Effect::Fetch(_))) {
            return Err(AppError::Conflict("fetch completion requires persistence"));
        }
        self.complete_effect(id)
    }
    pub(crate) fn published_video_receiver(
        &self,
        token: &SessionToken,
    ) -> Option<tokio::sync::watch::Receiver<Option<Arc<crate::lcd_stream::VideoFrame>>>> {
        if !self.supports_lcd_push(token) {
            return None;
        }
        let slot = self.video_slots.get(&token.device_id)?.lock().ok()?;
        (slot.session == *token).then(|| slot.latest.subscribe())
    }
    pub(crate) fn merge_transient_since(
        &mut self,
        base: &Self,
        current: &Self,
    ) -> Result<(), AppError> {
        let live_changed = current.runtime.world.devices != base.runtime.world.devices
            || current.runtime.outbox != base.runtime.outbox;
        let mut accepted = BTreeSet::new();
        let mut refresh = Vec::new();
        for (id, before) in &base.runtime.world.devices {
            let Some(now) = current.runtime.world.devices.get(id) else {
                continue;
            };
            let Some(ours) = self.runtime.world.devices.get_mut(id) else {
                continue;
            };
            // A durable invalidation wins even if concurrent ephemeral input
            // happened to allocate the same display epoch in its private view.
            let unchanged = ours.incarnation == before.incarnation
                && ours.session_epoch == before.session_epoch
                && ours.settings_revision == before.settings_revision
                && ours.selected == before.selected
                && self.input_fences.get(id) == base.input_fences.get(id)
                && ours.enabled == before.enabled;
            if !unchanged {
                continue;
            }
            accepted.insert(id.clone());
            if now == before {
                continue;
            }
            let content_refresh = ours.display_epoch != before.display_epoch;
            ours.mode = now.mode.clone();
            ours.owner = now.owner.clone();
            ours.online = now.online;
            ours.screen_owner_epoch = now.screen_owner_epoch;
            ours.display_epoch = now.display_epoch;
            ours.pending_frame_id = now.pending_frame_id;
            ours.last_sent_frame = now.last_sent_frame;
            ours.last_received_frame = now.last_received_frame;
            ours.last_rendered_frame = now.last_rendered_frame;
            if content_refresh && ours.mode == Mode::Normal && ours.pending_frame_id.is_none() {
                refresh.push(crate::session_token(id, ours));
            }
            let ctx = PluginContext {
                token: crate::session_token(id, ours),
                settings_revision: ours.settings_revision,
                mode: ours.mode.clone(),
            };
            if let Some(plugin) = &ours.selected {
                if let Some(view) = current
                    .contents
                    .get(plugin)
                    .and_then(|p| p.transient_view(&ctx))
                {
                    self.contents
                        .get_mut(plugin)
                        .ok_or(AppError::NotFound)?
                        .merge_transient_view(&ctx, view.as_ref())
                        .map_err(|_| AppError::Conflict("transient plugin view merge failed"))?;
                }
            }
            if let Some(time) = current.timed_frames.get(id) {
                self.timed_frames.insert(id.clone(), time.clone());
            }
        }
        // IDs are globally reserved. Respect completed old effects without
        // deleting a different effect newly created by the durable owner.
        self.runtime.outbox.retain(|id, _| {
            !base.runtime.outbox.contains_key(id) || current.runtime.outbox.contains_key(id)
        });
        for (id, effect) in &current.runtime.outbox {
            if !base.runtime.outbox.contains_key(id) && accepted.contains(effect_device(effect)) {
                self.runtime.outbox.insert(*id, effect.clone());
            }
        }
        self.frame_assets.extend(
            current
                .frame_assets
                .iter()
                .map(|(id, pixels)| (id.clone(), pixels.clone())),
        );
        self.runtime.prune_stale_effects();
        self.frame_assets.retain(|handle, _| {
            self.runtime.outbox.values().any(
                |effect| matches!(effect,Effect::SendFrame{asset_handle,..}if asset_handle==handle),
            )
        });
        for token in refresh {
            let was_transient = self.transient_only;
            self.transient_only = true;
            let queued = self.queue_selected_cached_frame(&token);
            self.transient_only = was_transient;
            queued?;
        }
        self.runtime.next_effect_id = self.runtime.next_effect_id.max(
            *self
                .effect_counter
                .lock()
                .map_err(|_| AppError::Conflict("effect allocator unavailable"))?,
        );
        self.runtime.world.next_frame_id = self.runtime.world.next_frame_id.max(
            *self
                .frame_counter
                .lock()
                .map_err(|_| AppError::Conflict("frame allocator unavailable"))?,
        );
        if live_changed {
            self.runtime.world.revision = self
                .runtime
                .world
                .revision
                .max(current.runtime.world.revision)
                .checked_add(1)
                .ok_or(AppError::Conflict("revision exhausted"))?;
        }
        #[cfg(feature = "boundary-observation")]
        {
            for (id, cause) in &current.frame_causes {
                if !self.frame_causes.iter().any(|(existing, _)| existing == id) {
                    if self.frame_causes.len() == 256 {
                        self.frame_causes.pop_front();
                    }
                    self.frame_causes.push_back((*id, *cause));
                }
            }
            for (key, cause) in &current.view_causes {
                if !accepted.iter().any(|id| {
                    self.runtime.world.devices.get(id).is_some_and(|d| {
                        (
                            d.incarnation,
                            d.session_epoch,
                            d.display_epoch,
                            d.screen_owner_epoch,
                        ) == *key
                    })
                }) {
                    continue;
                }
                self.view_causes.retain(|(existing, _)| existing != key);
                if self.view_causes.len() == 256 {
                    self.view_causes.pop_front();
                }
                self.view_causes.push_back((*key, *cause));
            }
        }
        Ok(())
    }
}
fn effect_device(effect: &Effect) -> &str {
    match effect {
        Effect::CloseSession(t)
        | Effect::EnterStandby { token: t }
        | Effect::SyncSettings { token: t, .. } => &t.device_id,
        Effect::ShowSelected { token: t, .. }
        | Effect::ShowLogo { token: t }
        | Effect::ResetPluginView { token: t, .. } => &t.device_id,
        Effect::DropQueuedFrames { device_id, .. } => device_id,
        Effect::Fetch(t) => &t.device_id,
        Effect::SendFrame { token, .. } => &token.device_id,
    }
}
