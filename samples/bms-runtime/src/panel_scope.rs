//! Typed panel data with cooperative task cancellation bounded by owner lifetime.
use crate::screen_lifecycle::ScreenInstanceId;
use std::{
    ops::{Deref, DerefMut},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// A cloned task permit is cancelled when its initiating panel is dropped.
/// Check at task boundaries; it cannot interrupt an in-progress filesystem call.
#[derive(Clone, Debug)]
pub struct TaskPermit {
    cancelled: Arc<AtomicBool>,
}
impl TaskPermit {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}
/// Owns one typed screen draft and its cancellation token, without navigation state.
/// The coordinator disposes child scopes before parents and independently drains
/// native owners; dropping this data does not join or interrupt any worker.
pub struct PanelScope<T> {
    id: ScreenInstanceId,
    data: T,
    cancelled: Arc<AtomicBool>,
}
impl<T> PanelScope<T> {
    pub fn new(id: ScreenInstanceId, data: T) -> Self {
        Self {
            id,
            data,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
    pub const fn id(&self) -> ScreenInstanceId {
        self.id
    }
    pub fn task_permit(&self) -> TaskPermit {
        TaskPermit {
            cancelled: Arc::clone(&self.cancelled),
        }
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
        self.cancelled.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
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
