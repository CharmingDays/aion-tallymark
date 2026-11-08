//! Desktop popup edge detection; independent of Tauri and the current target mode.
use std::collections::HashSet;

#[derive(Default)]
pub struct BossPopup {
    seen: HashSet<(i64, i32)>,
}

impl BossPopup {
    /// Observe even when disabled/visible: hiding or enabling later must not
    /// replay a boss that was already detected. A new encounter can reuse an ID.
    pub fn observe(&mut self, keys: &[(i64, i32)], enabled: bool, hidden: bool) -> bool {
        let current: HashSet<_> = keys.iter().copied().collect();
        let fresh = current.iter().any(|key| !self.seen.contains(key));
        self.seen = current;
        enabled && hidden && fresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn once_per_boss_not_per_snapshot_or_hide() {
        let mut gate = BossPopup::default();
        assert!(gate.observe(&[(10, 1)], true, true));
        assert!(!gate.observe(&[(10, 1)], true, true));
        assert!(!gate.observe(&[(10, 1)], true, false));
        assert!(!gate.observe(&[(10, 1)], true, true));
        assert!(gate.observe(&[(10, 1), (10, 2)], true, true));
        assert!(!gate.observe(&[(10, 1), (10, 2)], true, true));
    }

    #[test]
    fn visible_or_disabled_detections_are_not_replayed() {
        let mut gate = BossPopup::default();
        assert!(!gate.observe(&[(10, 1)], false, true));
        assert!(!gate.observe(&[(10, 1)], true, true));
        assert!(!gate.observe(&[(10, 2)], true, false));
        assert!(!gate.observe(&[(10, 2)], true, true));
        assert!(!gate.observe(&[], true, true));
        assert!(gate.observe(&[(20, 1)], true, true));
    }
}
