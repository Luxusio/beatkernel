//! Deferred retained Play ancestry and exact live child lifecycle admission.
use super::*;
#[test]
fn live_audio_back_retains_same_play_instance_and_reopening_never_reuses_child_identity() {
    let mut n = ScreenNavigator::default();
    n.navigate(ScreenRoute::Play { replay: false }, false, true)
        .unwrap();
    let play = n.active_id().unwrap();
    n.navigate(ScreenRoute::LiveAudio, false, false).unwrap();
    let child = n.active_id().unwrap();
    assert_ne!(child, play);
    assert!(n.retains(play));
    assert!(n.route().contains(ScreenKind::Play));
    assert!(n.route().contains(ScreenKind::LiveAudio));
    n.navigate(ScreenRoute::Play { replay: false }, false, false)
        .unwrap();
    assert_eq!(n.active_id(), Some(play));
    assert!(!n.retains(child));
    n.navigate(ScreenRoute::LiveAudio, false, false).unwrap();
    assert_ne!(n.active_id(), Some(child));
    assert!(n.retains(play));
    assert!(!n.accepts(child));
}
#[test]
fn replay_bad_routes_metadata_and_suspended_entry_refuse_without_allocating_new_screen_identity() {
    for replay in [false, true] {
        let mut n = ScreenNavigator::default();
        n.navigate(ScreenRoute::Play { replay }, false, true)
            .unwrap();
        let before = n.clone();
        if replay {
            assert!(n.navigate(ScreenRoute::LiveAudio, false, false).is_err());
            assert_eq!(n, before);
        } else {
            assert!(n.navigate(ScreenRoute::LiveAudio, true, false).is_err());
            assert_eq!(n, before);
            n.suspend();
            let suspended = n.clone();
            assert!(n.navigate(ScreenRoute::LiveAudio, false, false).is_err());
            assert_eq!(n, suspended);
        }
    }
    let mut n = ScreenNavigator::default();
    let before = n.clone();
    assert!(n.navigate(ScreenRoute::LiveAudio, false, true).is_err());
    assert_eq!(n, before);
}
#[test]
fn joined_result_or_close_disposes_live_child_while_normal_result_keeps_original_play_parent() {
    let mut n = ScreenNavigator::default();
    n.navigate(ScreenRoute::Play { replay: false }, false, true)
        .unwrap();
    let play = n.active_id().unwrap();
    n.navigate(ScreenRoute::LiveAudio, false, false).unwrap();
    let child = n.active_id().unwrap();
    let before = n.clone();
    assert!(
        n.navigate(ScreenRoute::Results { replay: false }, false, false)
            .is_err()
    );
    assert_eq!(n, before);
    n.navigate(ScreenRoute::Results { replay: false }, false, true)
        .unwrap();
    assert!(n.retains(play));
    assert!(!n.retains(child));
    assert!(!n.route().contains(ScreenKind::LiveAudio));
    n.navigate(ScreenRoute::Closing, false, false).unwrap();
    assert!(!n.retains(play));
    assert_eq!(n.phase(), ScreenPhase::Exiting);
    n.resume();
    assert_eq!(n.phase(), ScreenPhase::Exiting);
}
