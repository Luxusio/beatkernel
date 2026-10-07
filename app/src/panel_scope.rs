//! Typed panel data with cooperative task cancellation bounded by owner lifetime.
use crate::screen_lifecycle::{ScreenInstanceId, ScreenNavigator, ScreenPhase};
use std::{
    ops::{Deref, DerefMut},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

/// Panel lifetime derived from the coordinator's sole navigation stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PanelPhase {
    Active,
    Retained,
    Suspended,
    Disposed,
}
fn phase(token: &AtomicU8) -> PanelPhase {
    match token.load(Ordering::Acquire) {
        0 => PanelPhase::Active,
        1 => PanelPhase::Retained,
        2 => PanelPhase::Suspended,
        _ => PanelPhase::Disposed,
    }
}

/// A cloned task permit is cancelled when its initiating panel is dropped.
/// Check at task boundaries; it cannot interrupt an in-progress filesystem call.
/// Retained or suspended panels may finish existing work; the coordinator
/// separately checks active ownership before starting or presenting work.
#[derive(Clone, Debug)]
pub struct TaskPermit {
    phase: Arc<AtomicU8>,
}
impl TaskPermit {
    pub fn phase(&self) -> PanelPhase {
        phase(&self.phase)
    }
    pub fn is_active(&self) -> bool {
        self.phase() == PanelPhase::Active
    }
    pub fn is_cancelled(&self) -> bool {
        self.phase() == PanelPhase::Disposed
    }
}
/// Owns one typed screen draft and its cancellation token, without navigation state.
/// The coordinator disposes child scopes before parents and independently drains
/// native owners; dropping this data does not join or interrupt any worker.
pub struct PanelScope<T> {
    id: ScreenInstanceId,
    data: T,
    phase: Arc<AtomicU8>,
}
impl<T> PanelScope<T> {
    pub fn new(id: ScreenInstanceId, data: T) -> Self {
        Self {
            id,
            data,
            phase: Arc::new(AtomicU8::new(PanelPhase::Active as u8)),
        }
    }
    pub const fn id(&self) -> ScreenInstanceId {
        self.id
    }
    pub fn task_permit(&self) -> TaskPermit {
        TaskPermit {
            phase: Arc::clone(&self.phase),
        }
    }
    pub fn phase(&self) -> PanelPhase {
        phase(&self.phase)
    }
    pub fn is_active(&self) -> bool {
        self.phase() == PanelPhase::Active
    }
    /// Called synchronously by the coordinator after navigation or application
    /// suspension changes. Disposed scopes never re-enter the active lifetime.
    pub fn synchronize(&mut self, navigator: &ScreenNavigator) {
        if self.phase() == PanelPhase::Disposed {
            return;
        }
        let next = if navigator.phase() == ScreenPhase::Exiting || !navigator.retains(self.id) {
            PanelPhase::Disposed
        } else if navigator.phase() == ScreenPhase::Suspended {
            PanelPhase::Suspended
        } else if navigator.active_id() == Some(self.id) {
            PanelPhase::Active
        } else {
            PanelPhase::Retained
        };
        self.phase.store(next as u8, Ordering::Release);
    }
}
impl<T> Deref for PanelScope<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.data
    }
}
impl<T> DerefMut for PanelScope<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.data
    }
}
impl<T> Drop for PanelScope<T> {
    fn drop(&mut self) {
        self.phase
            .store(PanelPhase::Disposed as u8, Ordering::Release);
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::screen_lifecycle::ScreenRoute;
    #[test]
    fn retained_parent_child_back_and_suspend_share_actual_navigator_lifecycle() {
        let mut navigator = ScreenNavigator::default();
        let mut selection = PanelScope::new(navigator.active_id().unwrap(), "selection");
        let selection_permit = selection.task_permit();
        assert!(selection.is_active());
        assert!(selection_permit.is_active());
        navigator
            .navigate(ScreenRoute::Settings, false, true)
            .unwrap();
        selection.synchronize(&navigator);
        let mut settings = PanelScope::new(navigator.active_id().unwrap(), "settings");
        let settings_permit = settings.task_permit();
        assert_eq!(selection_permit.phase(), PanelPhase::Retained);
        assert!(!selection_permit.is_cancelled());
        navigator
            .navigate(ScreenRoute::Practice, false, true)
            .unwrap();
        selection.synchronize(&navigator);
        settings.synchronize(&navigator);
        let mut practice = PanelScope::new(navigator.active_id().unwrap(), "practice");
        let practice_permit = practice.task_permit();
        let clone = practice_permit.clone();
        assert_eq!(settings.phase(), PanelPhase::Retained);
        assert!(practice_permit.is_active());
        navigator.suspend();
        selection.synchronize(&navigator);
        settings.synchronize(&navigator);
        practice.synchronize(&navigator);
        for permit in [
            &selection_permit,
            &settings_permit,
            &practice_permit,
            &clone,
        ] {
            assert_eq!(permit.phase(), PanelPhase::Suspended);
            assert!(!permit.is_active());
            assert!(!permit.is_cancelled());
        }
        navigator.resume();
        selection.synchronize(&navigator);
        settings.synchronize(&navigator);
        practice.synchronize(&navigator);
        assert_eq!(selection.phase(), PanelPhase::Retained);
        assert_eq!(settings.phase(), PanelPhase::Retained);
        assert_eq!(practice.phase(), PanelPhase::Active);
        navigator.back(false, true).unwrap();
        practice.synchronize(&navigator);
        settings.synchronize(&navigator);
        assert_eq!(practice_permit.phase(), PanelPhase::Disposed);
        assert!(clone.is_cancelled());
        assert!(settings_permit.is_active());
        assert_eq!(*settings, "settings");
        drop(practice);
        assert!(practice_permit.is_cancelled());
        navigator.back(false, true).unwrap();
        settings.synchronize(&navigator);
        selection.synchronize(&navigator);
        assert!(settings_permit.is_cancelled());
        assert!(selection_permit.is_active());
        drop(selection);
        assert!(selection_permit.is_cancelled());
    }
    #[test]
    fn absent_or_closed_scopes_dispose_permanently_even_with_an_old_retaining_copy() {
        let mut navigator = ScreenNavigator::default();
        let retained_copy = navigator.clone();
        let mut panel = PanelScope::new(navigator.active_id().unwrap(), ());
        let permit = panel.task_permit();
        navigator
            .navigate(ScreenRoute::Closing, true, false)
            .unwrap();
        panel.synchronize(&navigator);
        assert_eq!(panel.phase(), PanelPhase::Disposed);
        panel.synchronize(&retained_copy);
        assert!(permit.is_cancelled());
        assert!(!permit.is_active());
        let mut absent = PanelScope::new(ScreenInstanceId(999), ());
        absent.synchronize(&retained_copy);
        assert_eq!(absent.phase(), PanelPhase::Disposed);
        let arbitrary = PanelScope::new(ScreenInstanceId(1000), ());
        assert_eq!(arbitrary.phase(), PanelPhase::Active);
        let permit = arbitrary.task_permit();
        drop(arbitrary);
        assert_eq!(permit.phase(), PanelPhase::Disposed);
    }
    #[test]
    fn typed_data_and_cloned_send_permits_share_only_their_owner_cancellation() {
        fn require_send<T: Send>() {}
        require_send::<TaskPermit>();
        let mut parent = PanelScope::new(ScreenInstanceId(1), vec!["parent"]);
        parent.push("retained");
        let parent_permit = parent.task_permit();
        let child = PanelScope::new(ScreenInstanceId(2), String::from("child"));
        let child_permit = child.task_permit();
        let clone = child_permit.clone();
        assert_eq!(child.id(), ScreenInstanceId(2));
        assert_eq!(child.as_str(), "child");
        assert!(!clone.is_cancelled());
        drop(child);
        assert!(child_permit.is_cancelled());
        assert!(clone.is_cancelled());
        assert!(!parent_permit.is_cancelled());
        assert_eq!(parent.as_slice(), &["parent", "retained"]);
        drop(parent);
        assert!(parent_permit.is_cancelled());
    }
}
