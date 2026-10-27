//! Optional LCD phrase example. The PNG is rendered before compilation.
use crate::content::{ButtonAction, ContentError, ContentPlugin, PluginContext, PluginPlan, PluginPosition};
use crate::{Candidate, Capability, Event, Plugin, WorldQuery};
use std::collections::BTreeSet;
use std::sync::LazyLock;

static FRAME: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let image = image::load_from_memory(include_bytes!("../assets/word-card.png"))
        .expect("word card PNG").to_rgb8();
    assert_eq!(image.dimensions(), (160, 80), "word card must be 160x80");
    let mut frame = Vec::with_capacity(160 * 80 * 2);
    for pixel in image.pixels() {
        let [r, g, b] = pixel.0;
        let color = ((u16::from(r) >> 3) << 11) | ((u16::from(g) >> 2) << 5) | (u16::from(b) >> 3);
        frame.extend_from_slice(&color.to_be_bytes());
    }
    frame
});

pub struct WordRules;
impl Plugin for WordRules {
    fn id(&self) -> &str { "my-words" }
    fn supports(&self, caps: &BTreeSet<Capability>) -> bool { caps.contains("lcd") }
    fn candidates<'a>(&'a self, _: &'a dyn WorldQuery, _: &'a Event) -> Box<dyn Iterator<Item = Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}

#[derive(Clone, Default)]
pub struct WordContent;
impl ContentPlugin for WordContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> { Box::new(self.clone()) }
    fn id(&self) -> &'static str { "my-words" }
    fn publish(&mut self, _: &PluginContext, _: &[u8], _: &str, _: &str) -> Result<(), ContentError> { Err(ContentError::Unsupported) }
    fn position(&self, _: &PluginContext) -> Option<PluginPosition> { None }
    fn plan(&mut self, _: &PluginContext, _: ButtonAction) -> Result<PluginPlan, ContentError> { Err(ContentError::Unsupported) }
    fn current_frame(&self, _: &PluginContext) -> Option<Vec<u8>> { Some(FRAME.clone()) }
    fn session_reset(&mut self, _: &str, _: u64) {}
    fn delete_device(&mut self, _: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::App;
    use crate::Effect;
    use std::sync::Arc;
    use crate::fetch::SystemClock;

    #[test]
    fn registered_phrase_reaches_server_send_effect() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open_with_plugin_set(&temp.path().join("state.db"), temp.path(),
            Arc::new(SystemClock), crate::plugins::builtins()).unwrap();
        let token = app.hello("desk", BTreeSet::from(["lcd".into(), "buttons4".into()]), 1).unwrap();
        for _ in 0..5 { app.button(&token, ButtonAction::Next).unwrap(); }
        assert_eq!(app.view().devices[0].selected.as_deref(), Some("my-words"));
        let frames: Vec<_> = app.pending_effects().into_iter().filter_map(|(_, effect)| match effect {
            Effect::SendFrame { asset_handle, .. } => app.frame_asset(&asset_handle),
            _ => None,
        }).collect();
        assert!(frames.iter().any(|frame| frame.as_ref() == &*FRAME));
        assert_eq!(FRAME.len(), 25_600);
    }
}
