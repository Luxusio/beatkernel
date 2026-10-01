//! Pure screen routing and lifecycle admission; owners and draft cleanup are external.

/// The sole active screen, including mode-specific retained play/results state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenRoute {
    Selection,
    Settings,
    Display,
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
            Self::Display | Self::Records | Self::Players | Self::Devices { players: false } => {
                Some(Self::Settings)
            }
            Self::Devices { players: true } => Some(Self::Players),
            Self::Results { replay } => Some(Self::Play { replay }),
            Self::Selection | Self::Settings | Self::Play { .. } | Self::Closing => None,
        }
    }
    /// Whether this active route retains the given screen's data.
    /// Settings children include Settings; a player device picker also includes
    /// Players. Results retains Play. Closing retains no normal screen data.
    pub fn contains(self, kind: ScreenKind) -> bool {
        let direct = match self {
            Self::Selection => Some(ScreenKind::Selection),
            Self::Settings => Some(ScreenKind::Settings),
            Self::Display => Some(ScreenKind::Display),
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenNavigator {
    route: ScreenRoute,
    phase: ScreenPhase,
}
impl Default for ScreenNavigator {
    fn default() -> Self {
        Self {
            route: ScreenRoute::Selection,
            phase: ScreenPhase::Active,
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
            self.route = to;
            self.phase = ScreenPhase::Exiting;
            return Ok(Some(transition));
        }
        if self.route == ScreenRoute::Closing {
            return Err("closing screen cannot navigate back to an active route".into());
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
                | ScreenRoute::Records
                | ScreenRoute::Players
                | ScreenRoute::Devices { players: false },
            )
            | (
                ScreenRoute::Display
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
            ScreenRoute::Records,
            ScreenRoute::Players,
            ScreenRoute::Devices { players: false },
        ] {
            assert_eq!(child.parent(), Some(ScreenRoute::Settings));
            assert!(child.contains(ScreenKind::Settings));
            assert!(!child.contains(ScreenKind::Selection));
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
            ScreenRoute::Settings,
            ScreenRoute::Play { replay: false },
            ScreenRoute::Closing,
        ] {
            assert_eq!(root.parent(), None);
        }
        for kind in [
            ScreenKind::Selection,
            ScreenKind::Settings,
            ScreenKind::Display,
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
            ScreenRoute::Records,
            ScreenRoute::Players,
            ScreenRoute::Devices { players: false },
        ] {
            navigator.navigate(child, false, false).unwrap();
            let before = navigator;
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
        let before = navigator;
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
        let before = navigator;
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
            let before = navigator;
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
            let before = navigator;
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
}
