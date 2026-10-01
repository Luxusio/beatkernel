//! Pure screen routing and lifecycle admission; owners and draft cleanup are external.

/// The sole active screen, including mode-specific retained play/results state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenRoute {
    Selection,
    Settings,
    Display,
    Practice,
    Records,
    Players,
    Devices { players: bool },
    Play { replay: bool },
    Results { replay: bool },
    Closing,
}
/// Screen data retained by a route or its parent hierarchy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenKind {
    Selection,
    Settings,
    Display,
    Practice,
    Records,
    Players,
    Devices,
    Play,
}
impl ScreenRoute {
    /// Immediate retained-data ancestor; roots and terminal close have none.
    /// This does not automatically permit Back: Results retains Play data, but
    /// retry requires its explicit edge and a joined game owner.
    pub const fn parent(self) -> Option<Self> {
        match self {
            Self::Display
            | Self::Practice
            | Self::Records
            | Self::Players
            | Self::Devices { players: false } => Some(Self::Settings),
            Self::Settings => Some(Self::Selection),
            Self::Devices { players: true } => Some(Self::Players),
            Self::Results { replay } => Some(Self::Play { replay }),
            Self::Selection | Self::Play { .. } | Self::Closing => None,
        }
    }
    /// Whether this active route retains the given screen's data.
    /// Settings retains Selection; its children include Settings and Selection.
    /// A player device picker also includes
    /// Players. Results retains Play. Closing retains no normal screen data.
    pub fn contains(self, kind: ScreenKind) -> bool {
        let direct = match self {
            Self::Selection => Some(ScreenKind::Selection),
            Self::Settings => Some(ScreenKind::Settings),
            Self::Display => Some(ScreenKind::Display),
            Self::Practice => Some(ScreenKind::Practice),
            Self::Records => Some(ScreenKind::Records),
            Self::Players => Some(ScreenKind::Players),
            Self::Devices { .. } => Some(ScreenKind::Devices),
            Self::Play { .. } | Self::Results { .. } => Some(ScreenKind::Play),
            Self::Closing => None,
        };
        direct == Some(kind) || self.parent().is_some_and(|parent| parent.contains(kind))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenPhase {
    Active,
    Suspended,
    Exiting,
}
/// An admitted transition; the coordinator owns cleanup outside destination ancestry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenTransition {
    pub from: ScreenRoute,
    pub to: ScreenRoute,
}
/// Non-reused identity allocated by a navigator when a screen is entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScreenInstanceId(pub u64);
/// A retained screen instance; data scopes are owned by the desktop coordinator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenEntry {
    pub route: ScreenRoute,
    pub id: ScreenInstanceId,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenNavigator {
    route: ScreenRoute,
    phase: ScreenPhase,
    stack: Vec<ScreenEntry>,
    next_id: Option<u64>,
}
impl Default for ScreenNavigator {
    fn default() -> Self {
        Self {
            route: ScreenRoute::Selection,
            phase: ScreenPhase::Active,
            stack: vec![ScreenEntry {
                route: ScreenRoute::Selection,
                id: ScreenInstanceId(1),
            }],
            next_id: Some(2),
        }
    }
}
impl ScreenNavigator {
    pub const fn route(&self) -> ScreenRoute {
        self.route
    }
    pub const fn phase(&self) -> ScreenPhase {
        self.phase
    }
    pub fn active_id(&self) -> Option<ScreenInstanceId> {
        self.stack.last().map(|entry| entry.id)
    }
    /// Result delivery requires both the active instance and an active lifecycle.
    pub fn accepts(&self, id: ScreenInstanceId) -> bool {
        self.phase == ScreenPhase::Active && self.active_id() == Some(id)
    }
    pub fn retains(&self, id: ScreenInstanceId) -> bool {
        self.stack.iter().any(|entry| entry.id == id)
    }
    /// Retained ancestors first, active entry last; at most four entries.
    pub fn stack(&self) -> &[ScreenEntry] {
        &self.stack
    }
    /// Back resumes menu parents, but Results returns to fresh Selection.
    /// A running Play has no Back edge: its owner must join into Results first.
    pub fn back_target(&self) -> Option<ScreenRoute> {
        match self.route {
            ScreenRoute::Results { .. } => Some(ScreenRoute::Selection),
            route => route.parent(),
        }
    }
    pub fn back(
        &mut self,
        pending_metadata: bool,
        owner_joined: bool,
    ) -> Result<Option<ScreenTransition>, String> {
        let target = self.back_target().ok_or("screen has no Back target")?;
        self.navigate(target, pending_metadata, owner_joined)
    }
    /// Admits only explicit edges. Every rejection leaves route and phase intact.
    /// Pending metadata fences even same-route requests. Close always bypasses
    /// that fence and owner joins; the coordinator must still drain resources.
    pub fn navigate(
        &mut self,
        to: ScreenRoute,
        pending_metadata: bool,
        owner_joined: bool,
    ) -> Result<Option<ScreenTransition>, String> {
        if to == ScreenRoute::Closing {
            if self.route == to {
                return Ok(None);
            }
            let transition = ScreenTransition {
                from: self.route,
                to,
            };
            self.stack.clear();
            self.route = to;
            self.phase = ScreenPhase::Exiting;
            return Ok(Some(transition));
        }
        if self.route == ScreenRoute::Closing {
            return Err("closing screen cannot navigate back to an active route".into());
        }
        if self.phase == ScreenPhase::Suspended {
            return Err("screen navigation waits for resume".into());
        }
        if pending_metadata {
            return Err("screen navigation waits for pending metadata".into());
        }
        if self.route == to {
            return Ok(None);
        }
        let allowed = match (self.route, to) {
            (ScreenRoute::Selection, ScreenRoute::Settings | ScreenRoute::Play { .. })
            | (
                ScreenRoute::Settings,
                ScreenRoute::Selection
                | ScreenRoute::Display
                | ScreenRoute::Practice
                | ScreenRoute::Records
                | ScreenRoute::Players
                | ScreenRoute::Devices { players: false },
            )
            | (
                ScreenRoute::Display
                | ScreenRoute::Practice
                | ScreenRoute::Records
                | ScreenRoute::Players
                | ScreenRoute::Devices { players: false },
                ScreenRoute::Settings,
            )
            | (ScreenRoute::Players, ScreenRoute::Devices { players: true })
            | (ScreenRoute::Devices { players: true }, ScreenRoute::Players)
            | (ScreenRoute::Records, ScreenRoute::Play { replay: true }) => true,
            (ScreenRoute::Play { replay: from }, ScreenRoute::Results { replay: to })
            | (ScreenRoute::Results { replay: from }, ScreenRoute::Play { replay: to }) => {
                if !owner_joined {
                    return Err("screen navigation waits for the game owner to join".into());
                }
                from == to
            }
            (ScreenRoute::Results { .. }, ScreenRoute::Selection) => {
                if !owner_joined {
                    return Err("screen navigation waits for the game owner to join".into());
                }
                true
            }
            _ => false,
        };
        if !allowed {
            return Err("illegal screen transition".into());
        }
        // Parent return resumes the original instance; retry is a fresh root,
        // even though Results retains old Play data for final presentation.
        let reset = matches!(
            (self.route, to),
            (
                ScreenRoute::Selection | ScreenRoute::Records,
                ScreenRoute::Play { .. }
            ) | (
                ScreenRoute::Results { .. },
                ScreenRoute::Play { .. } | ScreenRoute::Selection
            )
        );
        let retained = if reset {
            None
        } else {
            self.stack.iter().position(|entry| entry.route == to)
        };
        if let Some(index) = retained {
            self.stack.truncate(index + 1);
        } else {
            if !reset && self.stack.len() == 4 {
                return Err("screen stack exceeds four entries".into());
            }
            let id = self
                .next_id
                .ok_or("screen instance identity space exhausted")?;
            // Reservation is the final fallible step, before stack/allocator mutation.
            if !reset {
                self.stack
                    .try_reserve(1)
                    .map_err(|_| "screen stack allocation failed")?;
            }
            if reset {
                self.stack.clear();
            }
            self.stack.push(ScreenEntry {
                route: to,
                id: ScreenInstanceId(id),
            });
            self.next_id = id.checked_add(1);
        }
        let transition = ScreenTransition {
            from: self.route,
            to,
        };
        self.route = to;
        Ok(Some(transition))
    }
    /// Retains the route and draft/native owners; terminal close cannot suspend.
    pub fn suspend(&mut self) {
        if self.phase != ScreenPhase::Exiting {
            self.phase = ScreenPhase::Suspended;
        }
    }
    /// Resumes the retained route; terminal close cannot become active again.
    pub fn resume(&mut self) {
        if self.phase != ScreenPhase::Exiting {
            self.phase = ScreenPhase::Active;
        }
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn immediate_parents_and_retained_ancestry_are_exact() {
        for child in [
            ScreenRoute::Display,
            ScreenRoute::Practice,
            ScreenRoute::Records,
            ScreenRoute::Players,
            ScreenRoute::Devices { players: false },
        ] {
            assert_eq!(child.parent(), Some(ScreenRoute::Settings));
            assert!(child.contains(ScreenKind::Settings));
            assert!(child.contains(ScreenKind::Selection));
        }
        let picker = ScreenRoute::Devices { players: true };
        assert_eq!(picker.parent(), Some(ScreenRoute::Players));
        assert!(picker.contains(ScreenKind::Devices));
        assert!(picker.contains(ScreenKind::Players));
        assert!(picker.contains(ScreenKind::Settings));
        assert!(!picker.contains(ScreenKind::Records));
        assert!(!ScreenRoute::Devices { players: false }.contains(ScreenKind::Players));
        for replay in [false, true] {
            assert_eq!(
                ScreenRoute::Results { replay }.parent(),
                Some(ScreenRoute::Play { replay })
            );
            assert!(ScreenRoute::Results { replay }.contains(ScreenKind::Play));
        }
        for root in [
            ScreenRoute::Selection,
            ScreenRoute::Play { replay: false },
            ScreenRoute::Closing,
        ] {
            assert_eq!(root.parent(), None);
        }
        for kind in [
            ScreenKind::Selection,
            ScreenKind::Settings,
            ScreenKind::Display,
            ScreenKind::Practice,
            ScreenKind::Records,
            ScreenKind::Players,
            ScreenKind::Devices,
            ScreenKind::Play,
        ] {
            assert!(!ScreenRoute::Closing.contains(kind));
        }
    }
    #[test]
    fn legal_navigation_retains_parents_and_rejects_sibling_modal_replacement() {
        let mut navigator = ScreenNavigator::default();
        assert_eq!(navigator.route(), ScreenRoute::Selection);
        assert_eq!(navigator.phase(), ScreenPhase::Active);
        assert_eq!(
            navigator
                .navigate(ScreenRoute::Settings, false, false)
                .unwrap(),
            Some(ScreenTransition {
                from: ScreenRoute::Selection,
                to: ScreenRoute::Settings
            })
        );
        for child in [
            ScreenRoute::Display,
            ScreenRoute::Practice,
            ScreenRoute::Records,
            ScreenRoute::Players,
            ScreenRoute::Devices { players: false },
        ] {
            navigator.navigate(child, false, false).unwrap();
            let before = navigator.clone();
            let sibling = if child == ScreenRoute::Display {
                ScreenRoute::Records
            } else {
                ScreenRoute::Display
            };
            assert!(navigator.navigate(sibling, false, true).is_err());
            assert_eq!(navigator, before);
            navigator
                .navigate(ScreenRoute::Settings, false, false)
                .unwrap();
        }
        navigator
            .navigate(ScreenRoute::Players, false, false)
            .unwrap();
        navigator
            .navigate(ScreenRoute::Devices { players: true }, false, false)
            .unwrap();
        let before = navigator.clone();
        assert!(
            navigator
                .navigate(ScreenRoute::Settings, false, true)
                .is_err()
        );
        assert_eq!(navigator, before);
        navigator
            .navigate(ScreenRoute::Players, false, false)
            .unwrap();
        navigator
            .navigate(ScreenRoute::Settings, false, false)
            .unwrap();
        navigator
            .navigate(ScreenRoute::Records, false, false)
            .unwrap();
        assert!(
            navigator
                .navigate(ScreenRoute::Play { replay: false }, false, true)
                .is_err()
        );
        navigator
            .navigate(ScreenRoute::Play { replay: true }, false, false)
            .unwrap();
    }
    #[test]
    fn metadata_and_owner_join_fences_are_atomic_including_same_route_and_wrong_mode() {
        let mut navigator = ScreenNavigator::default();
        let before = navigator.clone();
        assert!(
            navigator
                .navigate(ScreenRoute::Selection, true, true)
                .is_err()
        );
        assert!(
            navigator
                .navigate(ScreenRoute::Settings, true, true)
                .is_err()
        );
        assert_eq!(navigator, before);
        assert_eq!(
            navigator
                .navigate(ScreenRoute::Selection, false, false)
                .unwrap(),
            None
        );
        for replay in [false, true] {
            navigator
                .navigate(ScreenRoute::Play { replay }, false, false)
                .unwrap();
            let before = navigator.clone();
            assert!(
                navigator
                    .navigate(ScreenRoute::Results { replay }, false, false)
                    .is_err()
            );
            assert!(
                navigator
                    .navigate(ScreenRoute::Results { replay: !replay }, false, true)
                    .is_err()
            );
            assert!(
                navigator
                    .navigate(ScreenRoute::Selection, false, true)
                    .is_err()
            );
            assert_eq!(navigator, before);
            navigator
                .navigate(ScreenRoute::Results { replay }, false, true)
                .unwrap();
            let before = navigator.clone();
            assert!(
                navigator
                    .navigate(ScreenRoute::Selection, false, false)
                    .is_err()
            );
            assert!(
                navigator
                    .navigate(ScreenRoute::Play { replay }, false, false)
                    .is_err()
            );
            assert!(
                navigator
                    .navigate(ScreenRoute::Play { replay: !replay }, false, true)
                    .is_err()
            );
            assert_eq!(navigator, before);
            navigator
                .navigate(ScreenRoute::Play { replay }, false, true)
                .unwrap();
            navigator
                .navigate(ScreenRoute::Results { replay }, false, true)
                .unwrap();
            navigator
                .navigate(ScreenRoute::Selection, false, true)
                .unwrap();
        }
    }
    #[test]
    fn suspend_resume_preserve_route_and_close_is_terminal_even_with_pending_owners() {
        let mut navigator = ScreenNavigator::default();
        navigator
            .navigate(ScreenRoute::Play { replay: true }, false, false)
            .unwrap();
        navigator.suspend();
        navigator.suspend();
        assert_eq!(navigator.route(), ScreenRoute::Play { replay: true });
        assert_eq!(navigator.phase(), ScreenPhase::Suspended);
        navigator.resume();
        assert_eq!(navigator.phase(), ScreenPhase::Active);
        navigator.suspend();
        let transition = navigator
            .navigate(ScreenRoute::Closing, true, false)
            .unwrap()
            .unwrap();
        assert_eq!(transition.from, ScreenRoute::Play { replay: true });
        assert_eq!(navigator.phase(), ScreenPhase::Exiting);
        navigator.suspend();
        navigator.resume();
        assert_eq!(navigator.phase(), ScreenPhase::Exiting);
        assert!(
            navigator
                .navigate(ScreenRoute::Selection, false, true)
                .is_err()
        );
        assert_eq!(
            navigator
                .navigate(ScreenRoute::Closing, true, false)
                .unwrap(),
            None
        );
        assert_eq!(navigator.route(), ScreenRoute::Closing);
    }
    #[test]
    fn retained_stack_resumes_exact_parent_ids_and_drops_only_child_entries() {
        let mut navigator = ScreenNavigator::default();
        let selection = navigator.active_id().unwrap();
        navigator
            .navigate(ScreenRoute::Settings, false, false)
            .unwrap();
        let settings = navigator.active_id().unwrap();
        assert!(navigator.retains(selection));
        assert!(!navigator.accepts(selection));
        navigator
            .navigate(ScreenRoute::Players, false, false)
            .unwrap();
        let players = navigator.active_id().unwrap();
        navigator
            .navigate(ScreenRoute::Devices { players: true }, false, false)
            .unwrap();
        let devices = navigator.active_id().unwrap();
        assert_eq!(navigator.stack().len(), 4);
        assert_eq!(
            navigator
                .stack()
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            vec![selection, settings, players, devices]
        );
        assert!(navigator.accepts(devices));
        navigator.back(false, false).unwrap();
        assert_eq!(navigator.active_id(), Some(players));
        assert!(!navigator.retains(devices));
        navigator.back(false, false).unwrap();
        assert_eq!(navigator.active_id(), Some(settings));
        navigator
            .navigate(ScreenRoute::Records, false, false)
            .unwrap();
        let records = navigator.active_id().unwrap();
        assert!(records.0 > devices.0);
        navigator.back(false, false).unwrap();
        navigator.back(false, false).unwrap();
        assert_eq!(navigator.active_id(), Some(selection));
        assert_eq!(navigator.stack().len(), 1);
        assert!(!navigator.accepts(records));
    }
    #[test]
    fn retry_and_results_back_create_fresh_roots_instead_of_resuming_completed_play() {
        let mut navigator = ScreenNavigator::default();
        navigator
            .navigate(ScreenRoute::Settings, false, false)
            .unwrap();
        navigator
            .navigate(ScreenRoute::Records, false, false)
            .unwrap();
        let menu_ids = navigator
            .stack()
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        navigator
            .navigate(ScreenRoute::Play { replay: true }, false, false)
            .unwrap();
        let first_play = navigator.active_id().unwrap();
        assert_eq!(navigator.stack().len(), 1);
        assert!(menu_ids.iter().all(|id| !navigator.retains(*id)));
        navigator
            .navigate(ScreenRoute::Results { replay: true }, false, true)
            .unwrap();
        let results = navigator.active_id().unwrap();
        assert!(navigator.retains(first_play));
        assert_eq!(navigator.back_target(), Some(ScreenRoute::Selection));
        let unchanged = navigator.clone();
        assert!(
            navigator
                .navigate(ScreenRoute::Play { replay: true }, false, false)
                .is_err()
        );
        assert_eq!(navigator, unchanged);
        navigator
            .navigate(ScreenRoute::Play { replay: true }, false, true)
            .unwrap();
        let retry = navigator.active_id().unwrap();
        assert!(retry.0 > results.0);
        assert!(!navigator.retains(first_play));
        assert!(!navigator.retains(results));
        assert_eq!(navigator.stack().len(), 1);
        navigator
            .navigate(ScreenRoute::Results { replay: true }, false, true)
            .unwrap();
        navigator.back(false, true).unwrap();
        assert_eq!(navigator.route(), ScreenRoute::Selection);
        assert!(navigator.active_id().unwrap().0 > retry.0);
        assert_eq!(navigator.stack().len(), 1);
    }
    #[test]
    fn exhaustion_suspend_and_failed_preflight_preserve_identity_and_close_always_clears() {
        let mut navigator = ScreenNavigator::default();
        let selection = navigator.active_id().unwrap();
        navigator.suspend();
        let before = navigator.clone();
        assert!(!navigator.accepts(selection));
        assert!(navigator.retains(selection));
        assert!(
            navigator
                .navigate(ScreenRoute::Settings, false, true)
                .is_err()
        );
        assert!(
            navigator
                .navigate(ScreenRoute::Selection, false, true)
                .is_err()
        );
        assert_eq!(navigator, before);
        navigator.resume();
        assert!(navigator.accepts(selection));
        let mut preflight = navigator.clone();
        preflight
            .navigate(ScreenRoute::Settings, false, false)
            .unwrap();
        assert_eq!(navigator.active_id(), Some(selection));
        assert_eq!(navigator.stack().len(), 1);
        // Failed external preparation discards the speculative navigator.
        drop(preflight);
        navigator.next_id = Some(u64::MAX);
        navigator
            .navigate(ScreenRoute::Settings, false, false)
            .unwrap();
        assert_eq!(navigator.active_id(), Some(ScreenInstanceId(u64::MAX)));
        let before = navigator.clone();
        assert!(
            navigator
                .navigate(ScreenRoute::Records, false, false)
                .is_err()
        );
        assert_eq!(navigator, before);
        navigator.back(false, false).unwrap();
        assert_eq!(navigator.active_id(), Some(selection));
        let before = navigator.clone();
        assert!(
            navigator
                .navigate(ScreenRoute::Settings, false, false)
                .is_err()
        );
        assert_eq!(navigator, before);
        assert_eq!(
            navigator
                .navigate(ScreenRoute::Selection, false, false)
                .unwrap(),
            None
        );
        navigator.suspend();
        navigator
            .navigate(ScreenRoute::Closing, true, false)
            .unwrap();
        assert!(navigator.stack().is_empty());
        assert_eq!(navigator.active_id(), None);
        assert!(!navigator.retains(selection));
        navigator.resume();
        navigator.suspend();
        assert_eq!(navigator.phase(), ScreenPhase::Exiting);
        assert!(
            navigator
                .navigate(ScreenRoute::Selection, false, true)
                .is_err()
        );
    }
}
