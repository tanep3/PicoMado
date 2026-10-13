//! Provisional bounded per-device queues. Control has priority over disposable
//! video frames; devices take turns when a shared dispatcher is used.

use std::collections::{BTreeMap, VecDeque};

/// These safety caps are unapproved measurement starting values.
pub const MAX_BYTES_PER_DEVICE: usize = 256 * 1024;
pub const CONTROL_RESERVE_BYTES: usize = 80 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketKind {
    Control,
    Video,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub device_id: String,
    /// Unique within a prepared dispatch batch; fragments share outbox_id.
    pub packet_id: u64,
    pub outbox_id: u64,
    pub finish_effect: bool,
    pub incarnation: u64,
    pub display_epoch: u64,
    pub kind: PacketKind,
    pub bytes: Vec<u8>,
}

#[derive(Default)]
struct DeviceQueue {
    control: VecDeque<Packet>,
    video: VecDeque<Packet>,
    control_bytes: usize,
    video_bytes: usize,
}

#[derive(Default)]
pub struct FairQueue {
    devices: BTreeMap<String, DeviceQueue>,
    rotation: VecDeque<String>,
}

impl FairQueue {
    pub fn contains(&self, packet_id: u64) -> bool {
        self.devices.values().any(|q| {
            q.control
                .iter()
                .chain(q.video.iter())
                .any(|p| p.packet_id == packet_id)
        })
    }
    pub fn contains_effect(&self, effect_id: u64) -> bool {
        self.devices.values().any(|q| {
            q.control
                .iter()
                .chain(q.video.iter())
                .any(|p| p.outbox_id == effect_id)
        })
    }
    pub fn retain_effects(&mut self, pending: &std::collections::BTreeSet<u64>) {
        for queue in self.devices.values_mut() {
            queue.control.retain(|p| pending.contains(&p.outbox_id));
            queue.video.retain(|p| pending.contains(&p.outbox_id));
            queue.control_bytes = queue.control.iter().map(|p| p.bytes.len()).sum();
            queue.video_bytes = queue.video.iter().map(|p| p.bytes.len()).sum();
        }
        self.rotation.retain(|id| {
            self.devices
                .get(id)
                .is_some_and(|q| !q.control.is_empty() || !q.video.is_empty())
        });
    }

    pub fn bytes(&self, device: &str) -> usize {
        self.devices
            .get(device)
            .map_or(0, |q| q.control_bytes + q.video_bytes)
    }

    pub fn push(&mut self, packet: Packet) -> Result<(), Packet> {
        let size = packet.bytes.len();
        if size == 0 || size > MAX_BYTES_PER_DEVICE || self.contains(packet.packet_id) {
            return Err(packet);
        }
        let id = packet.device_id.clone();
        let queue = self.devices.entry(id.clone()).or_default();
        match packet.kind {
            PacketKind::Control => {
                while queue.control_bytes + queue.video_bytes + size > MAX_BYTES_PER_DEVICE {
                    let Some(old) = queue.video.pop_front() else {
                        return Err(packet);
                    };
                    queue.video_bytes -= old.bytes.len();
                }
                queue.control_bytes += size;
                queue.control.push_back(packet);
            }
            PacketKind::Video => {
                if size > MAX_BYTES_PER_DEVICE - CONTROL_RESERVE_BYTES {
                    return Err(packet);
                }
                while queue.video_bytes + size > MAX_BYTES_PER_DEVICE - CONTROL_RESERVE_BYTES
                    || queue.control_bytes + queue.video_bytes + size > MAX_BYTES_PER_DEVICE
                {
                    let Some(old) = queue.video.pop_front() else {
                        return Err(packet);
                    };
                    queue.video_bytes -= old.bytes.len();
                }
                queue.video_bytes += size;
                queue.video.push_back(packet);
            }
        }
        if !self.rotation.contains(&id) {
            self.rotation.push_back(id);
        }
        Ok(())
    }

    pub fn drop_video_through(&mut self, device: &str, incarnation: u64, epoch: u64) {
        let mut empty = false;
        if let Some(queue) = self.devices.get_mut(device) {
            queue.video.retain(|packet| {
                let keep = packet.incarnation != incarnation || packet.display_epoch > epoch;
                if !keep {
                    queue.video_bytes -= packet.bytes.len();
                }
                keep
            });
            empty = queue.control.is_empty() && queue.video.is_empty();
        }
        if empty {
            self.rotation.retain(|id| id != device);
        }
    }

    pub fn pop_for(&mut self, device: &str) -> Option<Packet> {
        let queue = self.devices.get_mut(device)?;
        let packet = if let Some(packet) = queue.control.pop_front() {
            queue.control_bytes -= packet.bytes.len();
            packet
        } else {
            let packet = queue.video.pop_front()?;
            queue.video_bytes -= packet.bytes.len();
            packet
        };
        if queue.control.is_empty() && queue.video.is_empty() {
            self.rotation.retain(|id| id != device);
        }
        Some(packet)
    }

    pub fn pop_next(&mut self) -> Option<Packet> {
        let id = self.rotation.pop_front()?;
        let packet = self.pop_for(&id)?;
        if self.bytes(&id) > 0 {
            self.rotation.push_back(id);
        }
        Some(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(device: &str, id: u64, kind: PacketKind, size: usize) -> Packet {
        Packet {
            device_id: device.into(),
            packet_id: id,
            outbox_id: id,
            finish_effect: true,
            incarnation: 1,
            display_epoch: 1,
            kind,
            bytes: vec![0; size],
        }
    }

    #[test]
    fn controls_preempt_video_and_slow_devices_do_not_starve_others() {
        let mut queue = FairQueue::default();
        queue.push(packet("a", 1, PacketKind::Video, 100)).unwrap();
        queue.push(packet("b", 2, PacketKind::Video, 100)).unwrap();
        queue.push(packet("a", 3, PacketKind::Control, 10)).unwrap();
        assert_eq!(queue.pop_next().unwrap().outbox_id, 3);
        assert_eq!(queue.pop_next().unwrap().outbox_id, 2);
        assert_eq!(queue.pop_next().unwrap().outbox_id, 1);
    }

    #[test]
    fn bounded_video_drops_stale_and_preserves_control_reserve() {
        let mut queue = FairQueue::default();
        let video_cap = MAX_BYTES_PER_DEVICE - CONTROL_RESERVE_BYTES;
        queue
            .push(packet("a", 1, PacketKind::Video, video_cap))
            .unwrap();
        queue
            .push(packet("a", 2, PacketKind::Video, video_cap))
            .unwrap();
        assert!(!queue.contains(1));
        assert!(queue.contains(2));
        queue
            .push(packet("a", 3, PacketKind::Control, CONTROL_RESERVE_BYTES))
            .unwrap();
        assert_eq!(queue.bytes("a"), MAX_BYTES_PER_DEVICE);
        assert!(queue
            .push(packet("a", 4, PacketKind::Video, video_cap + 1))
            .is_err());
        queue.drop_video_through("a", 1, 1);
        assert_eq!(queue.bytes("a"), CONTROL_RESERVE_BYTES);
        assert_eq!(queue.pop_for("a").unwrap().outbox_id, 3);
    }

    #[test]
    fn dropping_a_device_queue_keeps_other_devices_schedulable() {
        let mut queue = FairQueue::default();
        queue.push(packet("a", 1, PacketKind::Video, 100)).unwrap();
        queue.push(packet("b", 2, PacketKind::Control, 10)).unwrap();
        queue.drop_video_through("a", 1, 1);
        assert_eq!(queue.pop_next().unwrap().outbox_id, 2);
    }
    #[test]
    fn sustained_non_reading_device_stays_bounded_and_other_device_advances() {
        let mut queue = FairQueue::default();
        for id in 1..=1000 {
            queue
                .push(packet("slow", id, PacketKind::Video, 4096))
                .unwrap();
            assert!(queue.bytes("slow") <= MAX_BYTES_PER_DEVICE);
            if id % 25 == 0 {
                queue
                    .push(packet("fast", 1000 + id, PacketKind::Control, 100))
                    .unwrap();
            }
        }
        assert!(queue.bytes("fast") <= MAX_BYTES_PER_DEVICE);
        assert_eq!(queue.pop_next().unwrap().device_id, "slow");
        assert_eq!(queue.pop_next().unwrap().device_id, "fast");
        assert!(queue.bytes("slow") <= MAX_BYTES_PER_DEVICE);
    }
}
