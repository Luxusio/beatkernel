use super::*;
use beatkernel::time::Timestamp;
use beatkernel_bms_runtime::practice_control::*;

fn runtime_fixture() -> (Desktop, player::PlayerPublisher) {
    let (mut app, publisher) = fixture();
    publisher
        .advertise_practice_transactions(Some(PracticeTransactionCapability {
            practice: PracticeCapability {
                generation: 1,
                min_target: t(0),
                max_target: t(1000),
            },
            rate: PracticeRateState {
                revision: 7,
                rate: beatkernel::transport::Rate::NORMAL,
            },
        }))
        .unwrap();
    app.collect_game();
    (app, publisher)
}

// Copied audio DTOs check UI response interpretation only; actual PCM and native
// boundary qualification are exercised by the separate owner/audio fixtures.
fn copied_runtime_rate_reply(
    request: PracticeTransactionRequest,
    refused: bool,
) -> PracticeTransactionReply {
    use beatkernel::audio::{PracticeRateEvent, PracticeRateOutcome, PracticeRateRefusal};
    let PracticeTransactionRequest::Rate {
        id,
        generation,
        expected,
        requested_rate,
    } = request
    else {
        panic!("rate request required")
    };
    PracticeTransactionReply::Rate {
        id,
        generation,
        result: Ok(PracticeRateEvent {
            request_id: id,
            generation,
            iteration: 2,
            request_revision: expected.revision,
            applied_revision: expected.revision + u64::from(!refused),
            physical_frame: 300,
            playback_frame: 200,
            active_elapsed_nanos: 100_000_000,
            previous_rate: expected.rate,
            requested_rate,
            applied_rate: if refused {
                expected.rate
            } else {
                requested_rate
            },
            outcome: if refused {
                PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity)
            } else if requested_rate == expected.rate {
                PracticeRateOutcome::Noop
            } else {
                PracticeRateOutcome::Applied
            },
        }),
    }
}

fn replace_focused_rate_text(app: &mut Desktop, text: &str) {
    app.modifiers_changed(if cfg!(target_os = "macos") {
        ModifiersState::SUPER
    } else {
        ModifiersState::CONTROL
    });
    app.keyboard_input(
        PhysicalKey::Code(KeyCode::KeyA),
        &Key::Character("a".into()),
        Some("a"),
        false,
    );
    app.modifiers_changed(ModifiersState::empty());
    app.keyboard_input(
        PhysicalKey::Code(KeyCode::Digit1),
        &Key::Character(text.into()),
        Some(text),
        false,
    );
}

fn runtime_editor_geometry(
    app: &Desktop,
    control: ControlId,
    focused: bool,
) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    let rgba = |color: u32| {
        [
            ((color >> 16) & 255) as f32 / 255.0,
            ((color >> 8) & 255) as f32 / 255.0,
            (color & 255) as f32 / 255.0,
            1.0,
        ]
    };
    let bounds = app.hits.iter().find(|(id, _)| *id == control).unwrap().1;
    let field = app
        .scene
        .rectangles()
        .iter()
        .filter(|rectangle| {
            let [x, y, width, height] = rectangle.bounds;
            x >= bounds.x as f32
                && y >= bounds.y as f32
                && x + width <= (bounds.x + bounds.width) as f32
                && y + height <= (bounds.y + bounds.height) as f32
        })
        .map(|rectangle| (rectangle.bounds, rectangle.color, rectangle.uv))
        .collect::<Vec<_>>();
    let background = field
        .iter()
        .filter(|(rectangle, _, _)| {
            *rectangle == [bounds.x, bounds.y, bounds.width, bounds.height].map(|n| n as f32)
        })
        .collect::<Vec<_>>();
    assert_eq!(background.len(), 1, "one composed editor background");
    assert_eq!(
        background[0].1,
        rgba(if focused { 0x354e6c } else { 0x263d59 }),
        "Desktop must paint the actual focus of {control:?}"
    );
    let carets = field
        .iter()
        .filter(|(_, color, _)| *color == rgba(0x74e5c5))
        .map(|(rectangle, _, _)| *rectangle)
        .collect::<Vec<_>>();
    let expected = if focused {
        vec![[bounds.x + 20, bounds.y + 8, 2, 16].map(|n| n as f32)]
    } else {
        Vec::new()
    };
    assert_eq!(carets, expected, "Desktop editor caret for {control:?}");
    field
}

#[test]
fn desktop_runtime_rate_composition_tracks_optional_tab_focus_without_a_phantom_editor() {
    let (mut app, _publisher) = runtime_fixture();
    let owner = app.navigator.active_id().unwrap();
    let mut original_editors = None;
    let mut original_controls = None;
    for (step, focused) in [
        None,
        Some(RATE_NUMERATOR),
        Some(RATE_DENOMINATOR),
        Some(RATE_APPLY),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        if step > 0 {
            app.key(KeyCode::Tab, false);
        }
        app.draw_with_submission(false).unwrap();
        app.scene.status().unwrap();
        let draft = app.rate.as_ref().unwrap();
        assert_eq!(draft.focus, focused);
        assert_eq!(draft.view.id(), owner);
        assert!(!draft.view.dirty());
        assert_eq!(
            app.text_target().map(|target| target.field),
            match focused {
                Some(RATE_NUMERATOR) => Some(TextField::RateNumerator),
                Some(RATE_DENOMINATOR) => Some(TextField::RateDenominator),
                _ => None,
            }
        );
        let controls = [RATE_NUMERATOR, RATE_DENOMINATOR, RATE_APPLY].map(|control| {
            let matching = app
                .hits
                .iter()
                .filter(|(id, _)| *id == control)
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1);
            let bounds = matching[0].1;
            (
                draft.view.node_for_control(control).unwrap().unwrap(),
                [bounds.x, bounds.y, bounds.width, bounds.height],
            )
        });
        if let Some(original) = &original_controls {
            assert_eq!(&controls, original);
        } else {
            original_controls = Some(controls);
        }
        let editors = (
            runtime_editor_geometry(&app, RATE_NUMERATOR, focused == Some(RATE_NUMERATOR)),
            runtime_editor_geometry(&app, RATE_DENOMINATOR, focused == Some(RATE_DENOMINATOR)),
        );
        if focused.is_none() || focused == Some(RATE_APPLY) {
            if let Some(original) = &original_editors {
                assert_eq!(&editors, original);
            } else {
                original_editors = Some(editors.clone());
            }
        }
        app.draw_with_submission(false).unwrap();
        assert!(!app.rate.as_ref().unwrap().view.dirty());
        assert_eq!(
            runtime_editor_geometry(&app, RATE_NUMERATOR, focused == Some(RATE_NUMERATOR)),
            editors.0
        );
        assert_eq!(
            runtime_editor_geometry(&app, RATE_DENOMINATOR, focused == Some(RATE_DENOMINATOR)),
            editors.1
        );
    }
    assert!(app.window.is_none() && app.renderer.is_none());
}

#[test]
fn desktop_runtime_rate_keyboard_focus_edits_and_apply_keep_original_attempt_until_reply() {
    use beatkernel_bms_runtime::ui::rate::{APPLY_CONTROL, DENOMINATOR_CONTROL, NUMERATOR_CONTROL};
    let (mut app, publisher) = runtime_fixture();
    let owner = app.navigator.active_id().unwrap();
    let original = app.game.as_ref().unwrap().launch.clone();
    assert!(app.text_target().is_none());
    app.key(KeyCode::Tab, false);
    let numerator = app
        .text_target()
        .expect("NUMERATOR is the first rate focus");
    replace_focused_rate_text(&mut app, "6");
    assert_eq!(app.text_editor(numerator.field).unwrap().value(), "6");
    app.key(KeyCode::Tab, true);
    assert_eq!(
        app.text_target(),
        Some(numerator),
        "OS repetition cannot move focus"
    );
    app.key(KeyCode::Tab, false);
    let denominator = app
        .text_target()
        .expect("DENOMINATOR is the second rate focus");
    assert_ne!(denominator, numerator);
    replace_focused_rate_text(&mut app, "4");
    assert_eq!(app.text_editor(denominator.field).unwrap().value(), "4");
    app.key(KeyCode::Tab, false);
    assert!(
        app.text_target().is_none(),
        "Apply focus is not a text editor"
    );
    app.key(KeyCode::Enter, false);
    let request = publisher.take_practice_transaction(true).unwrap().unwrap();
    assert!(matches!(request,
        PracticeTransactionRequest::Rate { requested_rate, expected, .. }
            if requested_rate == beatkernel::transport::Rate::new(3, 2).unwrap()
                && expected.revision == 7
                && expected.rate == beatkernel::transport::Rate::NORMAL
    ));
    app.activate(APPLY_CONTROL);
    app.key(KeyCode::Enter, false);
    assert!(publisher.take_practice_transaction(true).unwrap().is_none());
    let game = app.game.as_ref().unwrap();
    assert!(!game.cancelling && !game.joined && game.prepared_retry.is_none());
    assert_eq!(game.launch.args(), original.args());
    assert_eq!(
        game.viewer
            .practice_transaction_capability()
            .unwrap()
            .unwrap()
            .rate
            .revision,
        7
    );
    publisher
        .commit_practice_transaction_reply(&copied_runtime_rate_reply(request, false))
        .unwrap();
    app.collect_game();
    assert!(!app.game.as_ref().unwrap().viewer.practice_pending());
    assert_eq!(app.navigator.active_id(), Some(owner));
    app.activate(NUMERATOR_CONTROL);
    assert_eq!(
        app.text_editor(app.text_target().unwrap().field)
            .unwrap()
            .value(),
        "6"
    );
    app.activate(DENOMINATOR_CONTROL);
    assert_eq!(
        app.text_editor(app.text_target().unwrap().field)
            .unwrap()
            .value(),
        "4"
    );
}

#[test]
fn desktop_runtime_rate_invalid_draft_is_retained_and_keyboard_recovers_without_new_route() {
    let (mut app, publisher) = runtime_fixture();
    let owner = app.navigator.active_id();
    app.key(KeyCode::Tab, false);
    replace_focused_rate_text(&mut app, "3");
    app.key(KeyCode::Tab, false);
    let target = app.text_target().unwrap();
    replace_focused_rate_text(&mut app, "0");
    app.key(KeyCode::Enter, false);
    assert!(publisher.take_practice_transaction(true).unwrap().is_none());
    assert!(!app.game.as_ref().unwrap().viewer.practice_pending());
    assert_eq!(app.text_target(), Some(target));
    assert_eq!(app.text_editor(target.field).unwrap().value(), "0");
    assert_eq!(app.navigator.active_id(), owner);
    replace_focused_rate_text(&mut app, "2");
    app.key(KeyCode::Enter, false);
    assert!(matches!(publisher.take_practice_transaction(true).unwrap(),
        Some(PracticeTransactionRequest::Rate { requested_rate, .. })
            if requested_rate == beatkernel::transport::Rate::new(3, 2).unwrap()
    ));
}

#[test]
fn desktop_runtime_rate_reverse_focus_and_ime_keep_play_shortcuts_and_committed_draft_separate() {
    let (mut app, publisher) = runtime_fixture();
    app.key(KeyCode::Tab, false);
    let numerator = app.text_target().unwrap();
    assert_eq!(numerator.field, TextField::RateNumerator);
    app.modifiers_changed(ModifiersState::SHIFT);
    app.key(KeyCode::Tab, false);
    assert!(app.text_target().is_none());
    app.modifiers_changed(ModifiersState::empty());
    app.key(KeyCode::Tab, false);
    assert_eq!(app.text_target(), Some(numerator));
    app.key(KeyCode::Tab, false);
    assert_eq!(app.text_target().unwrap().field, TextField::RateDenominator);
    app.modifiers_changed(ModifiersState::SHIFT);
    app.key(KeyCode::Tab, false);
    assert_eq!(app.text_target(), Some(numerator));
    app.modifiers_changed(ModifiersState::empty());
    replace_focused_rate_text(&mut app, "6");
    app.text_editor_mut(numerator.field).unwrap().select_all();
    app.ime_event(Ime::Enabled);
    app.ime_event(Ime::Preedit("9".into(), Some((1, 1))));
    assert!(app.ime_owns_keyboard());
    assert_eq!(app.text_editor(numerator.field).unwrap().value(), "6");
    app.key(KeyCode::Tab, false);
    app.key(KeyCode::Enter, false);
    assert_eq!(app.text_target(), Some(numerator));
    assert!(publisher.take_practice_transaction(true).unwrap().is_none());
    app.ime_event(Ime::Commit("9".into()));
    app.ime_event(Ime::Disabled);
    assert_eq!(app.text_editor(numerator.field).unwrap().value(), "9");
    assert!(!app.game.as_ref().unwrap().viewer.practice_pending());
    app.key(KeyCode::F9, false);
    assert!(
        app.game.as_ref().unwrap().viewer.pause_requested(),
        "focused rate editing must preserve F9"
    );
    app.key(KeyCode::Escape, false);
    assert!(
        app.game.as_ref().unwrap().cancelling,
        "focused rate editing must preserve stop"
    );
}

#[test]
fn desktop_runtime_rate_paused_request_allows_f9_resume_through_both_guards_only() {
    let (mut app, publisher) = runtime_fixture();
    app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause = player::PauseState::Paused;
    let viewer = &app.game.as_ref().unwrap().viewer;
    viewer.request_pause(true);
    viewer
        .request_practice_rate(beatkernel::transport::Rate::new(3, 2).unwrap())
        .unwrap();
    assert!(publisher
        .take_practice_transaction(false)
        .unwrap()
        .is_none());
    assert_eq!(app.game.as_ref().unwrap().pause_target(), Some(false));
    app.key(KeyCode::F9, false);
    assert!(!app.game.as_ref().unwrap().viewer.pause_requested());
    assert!(publisher
        .take_practice_transaction(false)
        .unwrap()
        .is_none());
    app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause = player::PauseState::Running;
    assert_eq!(
        app.game.as_ref().unwrap().pause_target(),
        None,
        "pending rate allows resume only"
    );
    app.key(KeyCode::F9, false);
    assert!(!app.game.as_ref().unwrap().viewer.pause_requested());
    assert!(matches!(
        publisher.take_practice_transaction(true).unwrap(),
        Some(PracticeTransactionRequest::Rate { .. })
    ));
}

#[test]
fn desktop_runtime_rate_audio_refusal_is_not_applied_and_does_not_clear_loop_or_score() {
    let (mut app, publisher) = runtime_fixture();
    let game = app.game.as_mut().unwrap();
    game.loop_enabled = true;
    game.snapshot.as_mut().unwrap().score.hits = 37;
    game.snapshot.as_mut().unwrap().score.combo = 9;
    game.snapshot.as_mut().unwrap().pressed_lanes = 5;
    let before_score = game.snapshot.as_ref().unwrap().score.clone();
    game.viewer
        .request_practice_rate(beatkernel::transport::Rate::new(3, 2).unwrap())
        .unwrap();
    let request = publisher.take_practice_transaction(true).unwrap().unwrap();
    publisher
        .commit_practice_transaction_reply(&copied_runtime_rate_reply(request, true))
        .unwrap();
    app.collect_game();
    let game = app.game.as_ref().unwrap();
    assert!(game.loop_enabled);
    assert!(!game.cancelling && game.prepared_retry.is_none());
    assert_eq!(game.snapshot.as_ref().unwrap().score, before_score);
    assert_eq!(game.snapshot.as_ref().unwrap().pressed_lanes, 5);
    assert_eq!(
        game.viewer
            .practice_transaction_capability()
            .unwrap()
            .unwrap()
            .rate,
        PracticeRateState {
            revision: 7,
            rate: beatkernel::transport::Rate::NORMAL
        }
    );
    assert!(
        game.viewer
            .take_practice_transaction_response()
            .unwrap()
            .is_none(),
        "desktop consumes the correlated response once"
    );
    let failure = app.failure.clone();
    assert!(
        failure.is_some(),
        "an audio-side refusal needs an error presentation"
    );
    app.collect_game();
    assert_eq!(app.failure, failure);
}

#[test]
fn desktop_runtime_f5_is_one_original_restart_and_refusal_keeps_pinned_owner_and_loop() {
    use beatkernel::audio::{
        PracticeBoundaryKind, PracticeRateOutcome, PracticeRateRefusal, PracticeReceipt,
        PracticeRestartEvent,
    };
    let (mut app, publisher) = runtime_fixture();
    let original = SessionLaunch::new(vec![
        "--chart".into(),
        "pinned.bms".into(),
        "--start-ns".into(),
        "123".into(),
        "--end-ns".into(),
        "999".into(),
        "--speed".into(),
        "6/4".into(),
        "--record-replay".into(),
        "records/pinned.bkr".into(),
    ])
    .unwrap();
    let current = original
        .retry_from(PracticeStart::from_nanoseconds(789).unwrap())
        .unwrap();
    app.game.as_mut().unwrap().launch = current.clone();
    app.game.as_mut().unwrap().loop_enabled = true;
    app.key(KeyCode::F5, true);
    assert!(publisher.take_practice_transaction(true).unwrap().is_none());
    for refused in [true, false] {
        app.key(KeyCode::F5, false);
        let request = publisher.take_practice_transaction(true).unwrap().unwrap();
        let PracticeTransactionRequest::RestartOriginal {
            id,
            generation,
            expected,
        } = request
        else {
            panic!("F5 must be one compound original restart, not a region scrub")
        };
        app.key(KeyCode::F5, false);
        assert!(publisher.take_practice_transaction(true).unwrap().is_none());
        assert!(publisher.take_practice_request().unwrap().is_none());
        let game = app.game.as_ref().unwrap();
        assert!(!game.cancelling && !game.joined && game.prepared_retry.is_none());
        assert!(game.loop_enabled);
        assert_eq!(game.launch.args(), current.args());
        let target = if refused { t(20) } else { t(123) };
        let reply = PracticeTransactionReply::RestartOriginal {
            id,
            generation,
            result: Ok(PracticeRestartEvent {
                boundary: PracticeReceipt {
                    request_id: id,
                    generation: generation + u64::from(!refused),
                    iteration: 0,
                    physical_frame: 300,
                    playback_frame: 200,
                    requested_song_time: target,
                    applied_song_time: target,
                    correction_nanos: 0,
                    kind: if refused {
                        PracticeBoundaryKind::ControlRejectedRevision
                    } else {
                        PracticeBoundaryKind::Requested
                    },
                },
                request_revision: expected.revision,
                applied_revision: expected.revision + u64::from(!refused),
                previous_rate: expected.rate,
                requested_rate: beatkernel::transport::Rate::new(3, 2).unwrap(),
                applied_rate: if refused {
                    expected.rate
                } else {
                    beatkernel::transport::Rate::new(3, 2).unwrap()
                },
                outcome: if refused {
                    PracticeRateOutcome::Refused(PracticeRateRefusal::Ended)
                } else {
                    PracticeRateOutcome::Applied
                },
            }),
        };
        if refused {
            // A refused compound control is an unchanged-state marker. A
            // Requested boundary would falsely claim an applied new attempt.
            let mut wrong_marker = reply.clone();
            if let PracticeTransactionReply::RestartOriginal {
                result: Ok(event), ..
            } = &mut wrong_marker
            {
                event.boundary.kind = PracticeBoundaryKind::Requested;
            }
            assert!(publisher
                .commit_practice_transaction_reply(&wrong_marker)
                .is_err());
            let viewer = &app.game.as_ref().unwrap().viewer;
            assert_eq!(
                viewer.pending_practice_transaction().unwrap(),
                Some(request)
            );
            assert_eq!(
                viewer
                    .practice_transaction_capability()
                    .unwrap()
                    .unwrap()
                    .rate,
                expected
            );
            assert!(viewer
                .take_practice_transaction_response()
                .unwrap()
                .is_none());
        }
        publisher.commit_practice_transaction_reply(&reply).unwrap();
        app.collect_game();
        let game = app.game.as_ref().unwrap();
        assert_eq!(game.loop_enabled, refused);
        assert!(!game.cancelling && game.prepared_retry.is_none());
        assert_eq!(game.launch.original_args(), original.original_args());
        assert_eq!(
            game.launch.args(),
            current.args(),
            "UI cannot invent unpublished launch evidence"
        );
    }
}

#[test]
fn desktop_runtime_rate_pending_back_and_escape_never_fabricate_applied_completion() {
    let (mut app, publisher) = runtime_fixture();
    let owner = app.navigator.active_id();
    app.game
        .as_ref()
        .unwrap()
        .viewer
        .request_practice_rate(beatkernel::transport::Rate::new(3, 2).unwrap())
        .unwrap();
    let request = publisher.take_practice_transaction(true).unwrap().unwrap();
    assert!(app.navigate(ScreenRoute::Selection).is_err());
    assert_eq!(app.navigator.active_id(), owner);
    assert!(app.game.as_ref().unwrap().viewer.practice_pending());
    assert!(app
        .game
        .as_ref()
        .unwrap()
        .viewer
        .take_practice_transaction_response()
        .unwrap()
        .is_none());
    app.key(KeyCode::Escape, false);
    assert!(app.game.as_ref().unwrap().cancelling);
    assert!(publisher.take_practice_transaction(true).unwrap().is_none());
    assert!(publisher
        .commit_practice_transaction_reply(&copied_runtime_rate_reply(request, false))
        .is_err());
    app.collect_game();
    let game = app.game.as_ref().unwrap();
    assert!(game.cancelling && game.prepared_retry.is_none());
    assert!(game
        .viewer
        .practice_transaction_capability()
        .unwrap()
        .is_none());
    assert!(!game.viewer.practice_pending());
}
fn t(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn fixture() -> (Desktop, player::PlayerPublisher) {
    let mut app = tests::lifecycle_fixture();
    let next = app
        .prepare_route(ScreenRoute::Play { replay: false })
        .unwrap();
    app.commit_route(next);
    let (publisher, viewer) = player::channel();
    let _ = viewer.take_latest();
    publisher
        .advertise_practice(Some(PracticeCapability {
            generation: 1,
            min_target: t(0),
            max_target: t(1000),
        }))
        .unwrap();
    app.game = Some(Game {
        viewer,
        worker: None,
        snapshot: Some(player::PlayerSnapshot {
            status: player::PlayerStatus::Playing,
            song_time: Some(t(10)),
            pause: player::PauseState::Running,
            ..Default::default()
        }),
        completed_results: None,
        completed_results_error: None,
        cancelling: false,
        joined: false,
        local_page: 0,
        local_comparisons: false,
        replay: false,
        launch: SessionLaunch::new(vec!["--chart".into(), "pinned.bms".into()]).unwrap(),
        prepared_retry: None,
        practice_bookmark: None,
        practice_loop: None,
        loop_enabled: false,
    });
    (app, publisher)
}
fn ack(publisher: &player::PlayerPublisher, request: PracticeRequest, generation: u64) {
    let target = match request.action {
        PracticeAction::Loop { start, .. } => start,
        PracticeAction::Scrub { target } => target,
        PracticeAction::DisableLoop => t(20),
    };
    publisher
        .commit_practice_reply(&PracticeReply {
            id: request.id,
            generation: request.generation,
            result: Ok(PracticeApplied {
                generation,
                physical_frame: 100,
                playback_frame: 80,
                requested_target: target,
                applied_target: target,
            }),
        })
        .unwrap();
}
#[test]
fn desktop_practice_control_f8_f11_actual_dispatch_retains_owner_until_ack() {
    let (mut app, publisher) = fixture();
    app.key(KeyCode::F7, false);
    app.game
        .as_mut()
        .unwrap()
        .snapshot
        .as_mut()
        .unwrap()
        .song_time = Some(t(20));
    app.key(KeyCode::F10, false);
    app.key(KeyCode::F11, false);
    let request = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(
        request.action,
        PracticeAction::Loop {
            start: t(10),
            end: t(20)
        }
    );
    let game = app.game.as_ref().unwrap();
    assert!(!game.cancelling && !game.joined && game.prepared_retry.is_none());
    assert!(!game.loop_enabled); // Admission is not applied boundary.
    assert!(game.pause_target().is_none());
    app.key(KeyCode::F8, false);
    assert!(publisher.take_practice_request().unwrap().is_none());
    ack(&publisher, request, 2);
    app.collect_game();
    assert!(app.game.as_ref().unwrap().loop_enabled);
    app.key(KeyCode::F11, false);
    let request = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(request.action, PracticeAction::DisableLoop);
    ack(&publisher, request, 2);
    app.collect_game();
    assert!(!app.game.as_ref().unwrap().loop_enabled);
    app.key(KeyCode::F8, false);
    let request = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(request.action, PracticeAction::Scrub { target: t(10) });
    publisher
        .reply_practice(&PracticeReply {
            id: request.id,
            generation: request.generation,
            result: Err("queue full".into()),
        })
        .unwrap();
    app.collect_game();
    assert!(app.failure.as_ref().unwrap().contains("queue full"));
    assert!(!app.game.as_ref().unwrap().cancelling);
    app.key(KeyCode::F5, false);
    let request = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(request.action, PracticeAction::Scrub { target: t(0) });
    let game = app.game.as_ref().unwrap();
    assert!(!game.cancelling && !game.joined && game.prepared_retry.is_none());
}

#[test]
fn desktop_practice_control_f5_uses_pinned_start_and_applied_ack_only() {
    let (mut app, publisher) = fixture();
    let original = SessionLaunch::new(vec![
        "--chart".into(),
        "pinned.bms".into(),
        "--start-ns".into(),
        "123".into(),
        "--end-ns".into(),
        "999".into(),
        "--speed".into(),
        "6/4".into(),
        "--record-replay".into(),
        "records/pinned.bkr".into(),
    ])
    .unwrap();
    let current = original
        .retry_from(PracticeStart::from_nanoseconds(789).unwrap())
        .unwrap();
    assert!(current
        .args()
        .chunks_exact(2)
        .any(|pair| pair == ["--start-ns", "789"]));
    assert!(current
        .args()
        .chunks_exact(2)
        .any(|pair| pair == ["--speed", "6/4"]));
    assert!(current
        .args()
        .chunks_exact(2)
        .any(|pair| pair == ["--record-replay", "records/pinned.retry1.bkr"]));
    app.game.as_mut().unwrap().launch = current.clone();
    app.game
        .as_mut()
        .unwrap()
        .snapshot
        .as_mut()
        .unwrap()
        .song_time = Some(t(456));
    app.key(KeyCode::F7, false);
    app.game
        .as_mut()
        .unwrap()
        .snapshot
        .as_mut()
        .unwrap()
        .song_time = Some(t(800));
    app.key(KeyCode::F10, false);
    app.key(KeyCode::F11, false);
    let looping = publisher.take_practice_request().unwrap().unwrap();
    ack(&publisher, looping, 2);
    app.collect_game();
    assert!(app.game.as_ref().unwrap().loop_enabled);

    app.key(KeyCode::F5, true); // OS key repetition cannot create an attempt.
    assert!(publisher.take_practice_request().unwrap().is_none());
    app.key(KeyCode::F5, false);
    let refused = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(refused.action, PracticeAction::Scrub { target: t(123) });
    app.key(KeyCode::F5, false);
    assert!(publisher.take_practice_request().unwrap().is_none());
    let unchanged = |app: &Desktop| {
        let game = app.game.as_ref().unwrap();
        assert!(!game.cancelling && !game.joined && game.prepared_retry.is_none());
        assert_eq!(game.launch.args(), current.args());
        assert_eq!(game.launch.original_args(), original.original_args());
        assert_eq!(game.launch.attempt(), 1);
        assert_eq!(game.practice_bookmark.unwrap().nanoseconds(), 456);
    };
    unchanged(&app);
    assert!(app.game.as_ref().unwrap().loop_enabled);
    publisher
        .reply_practice(&PracticeReply {
            id: refused.id,
            generation: refused.generation,
            result: Err("qualified boundary refused".into()),
        })
        .unwrap();
    app.collect_game();
    unchanged(&app);
    assert!(app.game.as_ref().unwrap().loop_enabled);
    assert!(app
        .failure
        .as_ref()
        .unwrap()
        .contains("qualified boundary refused"));

    app.key(KeyCode::F5, false);
    let applied = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(applied.action, PracticeAction::Scrub { target: t(123) });
    assert_ne!(applied.id, refused.id);
    assert!(app.game.as_ref().unwrap().loop_enabled);
    // Channel acceptance is UI evidence, not an injected native boundary.
    ack(&publisher, applied, 3);
    app.collect_game();
    unchanged(&app);
    assert!(!app.game.as_ref().unwrap().loop_enabled);
    app.key(KeyCode::F8, false);
    let bookmark = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(bookmark.action, PracticeAction::Scrub { target: t(456) });
}

#[test]
fn desktop_practice_control_f5_paused_or_output_pending_keeps_owner() {
    use beatkernel_bms_runtime::{live_output_control::OutputCapability, settings::SettingsHost};
    let (mut app, publisher) = fixture();
    let initial = app.game.as_ref().unwrap().launch.clone();
    for pause in [player::PauseState::Pausing, player::PauseState::Paused] {
        app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause = pause;
        app.key(KeyCode::F5, false);
        assert!(publisher.take_practice_request().unwrap().is_none());
        let game = app.game.as_ref().unwrap();
        assert!(!game.cancelling && game.prepared_retry.is_none());
        assert_eq!(game.launch.args(), initial.args());
    }
    app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause = player::PauseState::Running;
    let args = vec![
        "--alsa".into(),
        "fixture-device".into(),
        "--buffer-frames".into(),
        "128".into(),
        "--period-frames".into(),
        "32".into(),
    ];
    publisher
        .advertise_output(Some(OutputCapability {
            host: SettingsHost::Linux,
            current_args: args.clone(),
        }))
        .unwrap();
    app.game
        .as_ref()
        .unwrap()
        .viewer
        .request_output(args)
        .unwrap();
    app.key(KeyCode::F5, false);
    assert!(publisher.take_practice_request().unwrap().is_none());
    let game = app.game.as_ref().unwrap();
    assert!(game.viewer.output_pending());
    assert!(!game.cancelling && game.prepared_retry.is_none());
    assert_eq!(game.launch.args(), initial.args());
}
#[test]
fn desktop_practice_control_scrub_clears_loop_only_after_successful_ack() {
    let (mut app, publisher) = fixture();
    app.key(KeyCode::F7, false);
    app.game
        .as_mut()
        .unwrap()
        .snapshot
        .as_mut()
        .unwrap()
        .song_time = Some(t(20));
    app.key(KeyCode::F10, false);
    app.key(KeyCode::F11, false);
    let request = publisher.take_practice_request().unwrap().unwrap();
    ack(&publisher, request, 2);
    app.collect_game();
    assert!(app.game.as_ref().unwrap().loop_enabled);
    app.key(KeyCode::F8, false);
    let refused = publisher.take_practice_request().unwrap().unwrap();
    assert!(app.game.as_ref().unwrap().loop_enabled);
    publisher
        .reply_practice(&PracticeReply {
            id: refused.id,
            generation: refused.generation,
            result: Err("cold scrub refusal".into()),
        })
        .unwrap();
    app.collect_game();
    assert!(app.game.as_ref().unwrap().loop_enabled);
    app.key(KeyCode::F8, false);
    let applied = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(applied.action, PracticeAction::Scrub { target: t(10) });
    assert!(app.game.as_ref().unwrap().loop_enabled);
    ack(&publisher, applied, 3);
    app.collect_game();
    assert!(!app.game.as_ref().unwrap().loop_enabled);
    assert!(!app.game.as_ref().unwrap().cancelling);
    app.key(KeyCode::F11, false);
    let next = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(
        next.action,
        PracticeAction::Loop {
            start: t(10),
            end: t(20)
        }
    );
}

#[test]
fn desktop_practice_control_watch_network_pause_and_revocation_never_legacy_restart() {
    let (mut app, publisher) = fixture();
    app.key(KeyCode::F7, false);
    app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause = player::PauseState::Pausing;
    app.key(KeyCode::F8, false);
    assert!(publisher.take_practice_request().unwrap().is_none());
    assert!(!app.game.as_ref().unwrap().cancelling);
    app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause = player::PauseState::Running;
    app.game.as_mut().unwrap().launch = SessionLaunch::new(vec![
        "--chart".into(),
        "pinned.bms".into(),
        "--mp-host".into(),
        "localhost:8000".into(),
    ])
    .unwrap();
    app.key(KeyCode::F8, false);
    assert!(publisher.take_practice_request().unwrap().is_none());
    app.game.as_mut().unwrap().launch =
        SessionLaunch::new(vec!["--chart".into(), "pinned.bms".into()]).unwrap();
    publisher.advertise_practice(None).unwrap();
    app.key(KeyCode::F8, false);
    let game = app.game.as_ref().unwrap();
    assert!(!game.cancelling && game.prepared_retry.is_none());
    assert!(!game.practice_restart_available());
    app.game.as_mut().unwrap().replay = true;
    assert!(!app.game.as_ref().unwrap().practice_restart_available());
}

#[test]
fn desktop_practice_control_f5_and_joined_retry_follow_actual_boundary_lineage() {
    use beatkernel_bms_runtime::{
        native_judge::NativeJudgeConfig,
        play_policy::{GaugeSelection, OriginalGaugeContext},
        practice_session::{prepare_attempt, PracticeAttemptConfig},
    };
    let source =
        beatkernel_bms::parse("#BPM 60\n#WAV01 key.wav\n#00011:0101\n", Default::default())
            .unwrap();
    let domain = beatkernel::time::ClockDomainId(1);
    let policy = NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        output: domain,
        end: None,
    }
    .resolve_play_policy(
        &OriginalGaugeContext::from_source(&source),
        GaugeSelection::BeatKernel,
    )
    .unwrap();
    let initial = SessionLaunch::new(vec![
        "--chart".into(),
        "pinned.bms".into(),
        "--record-replay".into(),
        "records/game.bkr".into(),
    ])
    .unwrap();
    for dispatch_f5 in [true, false] {
        let (mut app, publisher) = fixture();
        app.game.as_mut().unwrap().launch = initial.clone();
        if !dispatch_f5 {
            // This preflight is selected before the native owner advances. Final
            // joined handling must rebase it onto actual published boundaries.
            app.game.as_mut().unwrap().prepared_retry = Some(initial.retry().unwrap());
        }
        let mut current = initial.clone();
        player::with_publisher(publisher, || {
            player::pin_native_launch(initial.clone()).unwrap();
            player::publish_chart(&source, &source.source.compile().unwrap()).unwrap();
            player::prepare_native_play_policies(&[(
                beatkernel_bms_runtime::local_players::PlayerId(1),
                &policy,
            )])
            .unwrap();
            for generation in 1..=3 {
                let attempt = prepare_attempt(
                    &source,
                    &policy,
                    &current,
                    PracticeAttemptConfig {
                        start: t(1_000_000_000),
                        end: None,
                        domain,
                        chart_seed: 0,
                        capture_limits: beatkernel_bms_runtime::native_judge::capture_limits(
                            true, 16384, 128,
                        )
                        .unwrap(),
                    },
                )
                .unwrap();
                let staged = player::prepare_practice_presentation(
                    generation,
                    &[(beatkernel_bms_runtime::local_players::PlayerId(1), &attempt)],
                )
                .unwrap();
                player::commit_practice_presentation(staged, generation + 1).unwrap();
                current = attempt.next_launch;
            }
            // No frame collection happened during those real presentation commits.
            if dispatch_f5 {
                app.key(KeyCode::F5, false);
                let game = app.game.as_ref().unwrap();
                assert_eq!(game.launch.attempt(), 3);
                assert!(game.prepared_retry.is_none() && !game.cancelling);
                let request = player::take_practice_request().unwrap().unwrap();
                assert_eq!(request.action, PracticeAction::Scrub { target: t(0) });
            }
            Ok(())
        })
        .unwrap();

        // Exercise the same final-join handoff over actual published native launch
        // evidence. This component fixture makes no physical/joined-worker claim.
        let game = app.game.as_mut().unwrap();
        game.sync_native_launch().unwrap();
        assert_eq!(game.launch.attempt(), 3);
        if dispatch_f5 {
            game.prepared_retry = Some(initial.retry().unwrap());
        }
        game.rebase_joined_retry().unwrap();
        let retry = game.owner_finished(true).unwrap();
        assert_eq!(retry.attempt(), 4);
        assert_eq!(retry.args()[3], "records/game.retry4.bkr");
        assert_eq!(retry.original_args(), initial.original_args());
    }
}

#[test]
fn desktop_practice_control_spawned_worker_keeps_tls_and_joined_f5_lineage_after_explicit_cancel() {
    use beatkernel_bms_runtime::{
        native_judge::NativeJudgeConfig,
        play_policy::{GaugeSelection, OriginalGaugeContext},
        practice_session::{prepare_attempt, PracticeAttemptConfig},
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    const WAIT: Duration = Duration::from_secs(5);
    static JOINED_VALIDATIONS: AtomicUsize = AtomicUsize::new(0);
    fn validate_joined(args: &[String]) -> Result<(), Box<dyn Error>> {
        assert!(args
            .chunks_exact(2)
            .any(|pair| pair == ["--record-replay", "records/thread.retry4.bkr"]));
        JOINED_VALIDATIONS.fetch_add(1, Ordering::SeqCst);
        Err("joined retry deliberately refused after validation".into())
    }
    enum Command {
        ApplyRequest,
        StageBeforeCancel,
        ApplyCancelled,
    }
    #[derive(Debug)]
    struct Step {
        worker: thread::ThreadId,
        attempt: u32,
        request: Option<u64>,
        visual_refused: bool,
    }
    let (commands, owner_commands) = mpsc::sync_channel(1);
    let (owner_steps, steps) = mpsc::sync_channel(1);
    let initial = SessionLaunch::new(vec![
        "--chart".into(),
        "pinned-thread.bms".into(),
        "--record-replay".into(),
        "records/thread.bkr".into(),
    ])
    .unwrap();
    let expected_initial = initial.clone();
    let game = spawn_game_with(
        move |args| {
            let worker = thread::current().id();
            assert_eq!(thread::current().name(), Some("bms-game"));
            assert!(player::attached());
            let mut current = player::native_launch(args)?;
            assert_eq!(current.args(), expected_initial.args());
            assert_eq!(current.original_args(), expected_initial.original_args());
            assert_eq!(current.attempt(), 0);
            let source =
                beatkernel_bms::parse("#BPM 60\n#WAV01 key.wav\n#00011:0101\n", Default::default())
                    .unwrap();
            let domain = beatkernel::time::ClockDomainId(1);
            let policy = NativeJudgeConfig {
                early: 0,
                late: 0,
                offset: 0,
                preroll: 0,
                output: domain,
                end: None,
            }
            .resolve_play_policy(
                &OriginalGaugeContext::from_source(&source),
                GaugeSelection::BeatKernel,
            )
            .unwrap();
            let member = beatkernel_bms_runtime::local_players::PlayerId(1);
            player::publish_chart(&source, &source.source.compile().unwrap()).unwrap();
            player::prepare_native_play_policies(&[(member, &policy)]).unwrap();
            player::publish_pause(player::PauseState::Running);
            let prepare = |launch: &SessionLaunch, generation: u64| {
                let attempt = prepare_attempt(
                    &source,
                    &policy,
                    launch,
                    PracticeAttemptConfig {
                        start: t(1_000_000_000),
                        end: None,
                        domain,
                        chart_seed: 0,
                        capture_limits: beatkernel_bms_runtime::native_judge::capture_limits(
                            true, 16384, 128,
                        )
                        .unwrap(),
                    },
                )
                .unwrap();
                let presentation =
                    player::prepare_practice_presentation(generation, &[(member, &attempt)])
                        .unwrap();
                (attempt, presentation)
            };
            // These are genuine TLS presentation/identity effects. This injected
            // owner does not claim to qualify an audio or native output boundary.
            let (attempt, presentation) = prepare(&current, 1);
            player::commit_practice_presentation(presentation, 2).unwrap();
            current = attempt.next_launch;
            player::advertise_practice(Some(PracticeCapability {
                generation: 2,
                min_target: t(0),
                max_target: t(4_000_000_000),
            }))
            .unwrap();
            owner_steps
                .send(Step {
                    worker,
                    attempt: 1,
                    request: None,
                    visual_refused: false,
                })
                .unwrap();
            assert!(matches!(
                owner_commands.recv_timeout(WAIT).unwrap(),
                Command::ApplyRequest
            ));
            let request = player::take_practice_request().unwrap().unwrap();
            assert_eq!(request.generation, 2);
            assert_eq!(
                request.action,
                PracticeAction::Scrub {
                    target: t(1_000_000_000)
                }
            );
            assert!(!player::cancelled());
            let (attempt, presentation) = prepare(&current, 2);
            player::commit_practice_presentation(presentation, 3).unwrap();
            current = attempt.next_launch;
            player::commit_practice_reply(&PracticeReply {
                id: request.id,
                generation: request.generation,
                result: Ok(PracticeApplied {
                    generation: 3,
                    physical_frame: 100,
                    playback_frame: 80,
                    requested_target: t(1_000_000_000),
                    applied_target: t(1_000_000_000),
                }),
            })
            .unwrap();
            owner_steps
                .send(Step {
                    worker,
                    attempt: 2,
                    request: Some(request.id),
                    visual_refused: false,
                })
                .unwrap();
            assert!(matches!(
                owner_commands.recv_timeout(WAIT).unwrap(),
                Command::StageBeforeCancel
            ));
            let (attempt, mut presentation) = prepare(&current, 3);
            owner_steps
                .send(Step {
                    worker,
                    attempt: current.attempt(),
                    request: None,
                    visual_refused: false,
                })
                .unwrap();
            assert!(matches!(
                owner_commands.recv_timeout(WAIT).unwrap(),
                Command::ApplyCancelled
            ));
            assert!(player::cancelled());
            // The native pump would supply actual boundary qualification here.
            // Applied identity must survive observer refusal after cancellation.
            player::apply_practice_identity(&mut presentation, 4);
            assert!(player::commit_practice_presentation(presentation, 4).is_err());
            current = attempt.next_launch;
            assert_eq!(player::native_launch(current.args()).unwrap().attempt(), 3);
            owner_steps
                .send(Step {
                    worker,
                    attempt: 3,
                    request: None,
                    visual_refused: true,
                })
                .unwrap();
            Ok(())
        },
        initial.clone(),
        0,
        false,
        false,
    )
    .unwrap();
    let mut app = tests::lifecycle_fixture();
    let next = app
        .prepare_route(ScreenRoute::Play { replay: false })
        .unwrap();
    app.commit_route(next);
    app.game = Some(game);
    let ready = steps.recv_timeout(WAIT).unwrap();
    assert_ne!(ready.worker, thread::current().id());
    assert_eq!(ready.attempt, 1);
    app.collect_game();
    assert_eq!(app.game.as_ref().unwrap().launch.attempt(), 1);
    assert_eq!(
        app.game.as_ref().unwrap().snapshot.as_ref().unwrap().status,
        player::PlayerStatus::Playing
    );
    app.key(KeyCode::F7, false);
    app.key(KeyCode::F8, false);
    let request = app
        .game
        .as_ref()
        .unwrap()
        .viewer
        .pending_practice_request()
        .unwrap()
        .unwrap();
    assert!(!app.game.as_ref().unwrap().cancelling);
    assert!(app.game.as_ref().unwrap().worker.is_some());
    commands.send(Command::ApplyRequest).unwrap();
    let applied = steps.recv_timeout(WAIT).unwrap();
    assert_eq!(applied.worker, ready.worker);
    assert_eq!(applied.request, Some(request.id));
    assert_eq!(applied.attempt, 2);
    app.collect_game();
    let game = app.game.as_ref().unwrap();
    assert_eq!(game.launch.attempt(), 2);
    assert!(!game.viewer.practice_pending());
    assert!(!game.cancelling && !game.joined && game.prepared_retry.is_none());
    commands.send(Command::StageBeforeCancel).unwrap();
    assert_eq!(steps.recv_timeout(WAIT).unwrap().worker, ready.worker);
    app.key(KeyCode::F5, false);
    let game = app.game.as_ref().unwrap();
    assert!(!game.cancelling && game.prepared_retry.is_none());
    assert_eq!(
        game.viewer
            .pending_practice_request()
            .unwrap()
            .unwrap()
            .action,
        PracticeAction::Scrub { target: t(0) }
    );
    app.game.as_mut().unwrap().cancel();
    // Keep the joined owner for inspection; avoid creating the next native owner.
    app.occluded = true;
    app.validate = validate_joined;
    let before = JOINED_VALIDATIONS.load(Ordering::SeqCst);
    commands.send(Command::ApplyCancelled).unwrap();
    let final_step = steps.recv_timeout(WAIT).unwrap();
    assert_eq!(final_step.worker, ready.worker);
    assert_eq!(final_step.attempt, 3);
    assert!(final_step.visual_refused);
    let deadline = Instant::now() + WAIT;
    while !app
        .game
        .as_ref()
        .unwrap()
        .worker
        .as_ref()
        .unwrap()
        .is_finished()
    {
        assert!(
            Instant::now() < deadline,
            "spawned game worker failed to finish"
        );
        thread::yield_now();
    }
    app.collect_game(); // Actual production join and canonical lineage sync.
    let game = app.game.as_ref().unwrap();
    assert!(game.joined && game.worker.is_none());
    assert_eq!(game.launch.attempt(), 3);
    assert_eq!(game.launch.original_args(), initial.original_args());
    assert_eq!(JOINED_VALIDATIONS.load(Ordering::SeqCst), before);
    app.occluded = false;
    app.key(KeyCode::F5, false); // Joined owner takes the fresh retry preflight.
    assert_eq!(JOINED_VALIDATIONS.load(Ordering::SeqCst), before + 1);
    assert!(app
        .failure
        .as_ref()
        .unwrap()
        .contains("joined retry deliberately refused"));
    let game = app.game.as_ref().unwrap();
    assert!(game.joined && game.worker.is_none() && game.prepared_retry.is_none());
    assert!(game.viewer.practice_capability().unwrap().is_none());
}
