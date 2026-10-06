use super::*;
use beatkernel_bms_runtime::{
    live_output_control::{OutputCapability, OutputReply},
    settings::SettingsHost,
    ui::live_audio::LiveAudioFrame,
};
fn cap(device: &str) -> OutputCapability {
    OutputCapability {
        host: SettingsHost::Linux,
        current_args: vec![
            "--alsa".into(),
            device.into(),
            "--buffer-frames".into(),
            "128".into(),
            "--period-frames".into(),
            "32".into(),
        ],
    }
}
fn prepared() -> (Desktop, player::PlayerPublisher) {
    let mut app = super::lifecycle_fixture();
    let next = app
        .prepare_route(ScreenRoute::Play { replay: false })
        .unwrap();
    app.commit_route(next);
    let (publisher, viewer) = player::channel();
    publisher
        .advertise_output(Some(cap("initial-device")))
        .unwrap();
    viewer.request_pause(true);
    let mut game = super::retry_fixture();
    game.viewer = viewer;
    game.snapshot = Some(player::PlayerSnapshot {
        status: player::PlayerStatus::Playing,
        pause: player::PauseState::Paused,
        ..Default::default()
    });
    app.game = Some(game);
    (app, publisher)
}
fn compose(app: &mut Desktop) {
    let draft = app.live_audio.as_ref().unwrap();
    let pending = app.game.as_ref().unwrap().viewer.output_pending();
    let view = app.live_audio_view.as_ref().unwrap();
    view.update(LiveAudioFrame {
        fields: draft.values.fields(),
        selected: draft.selected,
        editor: &draft.editor,
        message: draft.message.as_deref(),
        pending,
        hovered: None,
        armed: None,
    })
    .unwrap();
    app.scene.clear();
    app.hits.clear();
    view.compose(&mut app.scene, &mut app.hits).unwrap();
}
#[test]
fn actual_f2_gate_refuses_replay_terminal_cancel_and_unacknowledged_resume_intent_without_changing_play_instance()
 {
    for case in 0..9 {
        let (mut app, publisher) = prepared();
        let before = app.navigator.clone();
        match case {
            0 => app.game.as_mut().unwrap().replay = true,
            1 => app.game.as_mut().unwrap().joined = true,
            2 => app.game.as_mut().unwrap().cancelling = true,
            3 => {
                app.game
                    .as_mut()
                    .unwrap()
                    .snapshot
                    .as_mut()
                    .unwrap()
                    .cancelled = true
            }
            4 => app.game.as_ref().unwrap().viewer.request_pause(false),
            5 => publisher.advertise_output(None).unwrap(),
            6 => {
                app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause =
                    player::PauseState::Pausing
            }
            7 => {
                app.game.as_mut().unwrap().launch = SessionLaunch::new(vec![
                    "--chart".into(),
                    "pinned.bms".into(),
                    "--mp-host".into(),
                    "127.0.0.1:9000".into(),
                ])
                .unwrap();
            }
            _ => {
                app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause =
                    player::PauseState::Running
            }
        }
        if case == 8 {
            app.failure = Some("existing owner failure".into());
        }
        let launch = app.game.as_ref().unwrap().launch.args().to_vec();
        let failure = app.failure.clone();
        app.key(KeyCode::F2, false);
        assert_eq!(app.navigator, before);
        assert!(app.live_audio.is_none());
        assert_eq!(app.game.as_ref().unwrap().launch.args(), launch);
        assert_eq!(app.failure, failure);
    }
    let (mut app, _) = prepared();
    let play = app.navigator.active_id().unwrap();
    let native = app.options.native.clone();
    app.key(KeyCode::F2, true);
    assert_eq!(app.navigator.active_id(), Some(play));
    app.key(KeyCode::F2, false);
    assert_eq!(app.navigator.route(), ScreenRoute::LiveAudio);
    assert!(app.navigator.retains(play));
    app.activate(ControlId(91));
    assert_eq!(app.navigator.active_id(), Some(play));
    assert!(app.live_audio.is_none());
    assert!(app.live_audio_view.is_none());
    assert!(app.game.as_ref().unwrap().viewer.pause_requested());
    assert_eq!(app.options.native, native);
}
#[test]
fn rejected_live_output_paste_and_ime_commit_report_local_errors_without_mutating_settings() {
    for ime in [false, true] {
        let (mut app, _) = prepared();
        app.open_live_audio();
        app.live_audio.as_mut().unwrap().editor.select_all();
        let before = app.live_audio.as_ref().unwrap().editor.clone();
        let values = app.live_audio.as_ref().unwrap().values.native_args();
        if ime {
            app.sync_ime();
            app.ime_event(Ime::Enabled);
            app.ime_event(Ime::Commit("invalid\nendpoint".into()));
        } else {
            let target = app.text_target().unwrap();
            app.pending_clipboard = Some(PendingClipboard {
                target,
                edit: ClipboardEdit::prepare(&before, ClipboardAction::Paste)
                    .unwrap()
                    .unwrap(),
            });
            app.finish_clipboard(Ok(Some("invalid\nendpoint".into())));
        }
        let draft = app.live_audio.as_ref().unwrap();
        assert!(
            draft
                .message
                .as_ref()
                .is_some_and(|message| !message.is_empty())
        );
        assert_eq!(draft.editor, before);
        assert_eq!(draft.values.native_args(), values);
        assert!(app.failure.is_none());
        if ime {
            app.ime_event(Ime::Commit("valid-endpoint".into()));
        } else {
            let target = app.text_target().unwrap();
            app.pending_clipboard = Some(PendingClipboard {
                target,
                edit: ClipboardEdit::prepare(&before, ClipboardAction::Paste)
                    .unwrap()
                    .unwrap(),
            });
            app.finish_clipboard(Ok(Some("valid-endpoint".into())));
        }
        let draft = app.live_audio.as_ref().unwrap();
        assert_eq!(draft.editor.value(), "valid-endpoint");
        assert!(draft.message.is_none());
    }
}
#[test]
fn field_ime_edits_use_child_scope_pending_disables_hits_and_back_cancels_composition_and_pointer_arm()
 {
    let (mut app, publisher) = prepared();
    let native = app.options.native.clone();
    let launch = app.game.as_ref().unwrap().launch.args().to_vec();
    app.open_live_audio();
    app.sync_ime();
    assert!(matches!(
        app.text_target().unwrap().field,
        TextField::LiveOutput(0)
    ));
    app.live_audio.as_mut().unwrap().editor.select_all();
    app.ime_event(Ime::Enabled);
    app.ime_event(Ime::Preedit("별é".into(), Some((0, 5))));
    app.ime_event(Ime::Commit("edited-device".into()));
    assert_eq!(
        app.live_audio.as_ref().unwrap().editor.value(),
        "edited-device"
    );
    app.apply_live_audio();
    let id = app.live_audio.as_ref().unwrap().request.unwrap();
    assert_eq!(publisher.take_output_request().unwrap().unwrap().id, id);
    assert_eq!(app.game.as_ref().unwrap().pause_target(), None);
    let editor = app.live_audio.as_ref().unwrap().editor.clone();
    app.live_audio_key(KeyCode::Backspace, false);
    app.ime_event(Ime::Commit("ignored while pending".into()));
    assert_eq!(app.live_audio.as_ref().unwrap().editor, editor);
    assert!(app.text_target().is_none());
    compose(&mut app);
    assert_eq!(
        app.hits.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        [ControlId(91)]
    );
    app.gesture.press(Some(ControlId(91)));
    app.activate(ControlId(91));
    assert!(!app.gesture.is_armed(ControlId(91)));
    assert!(app.live_audio.is_none());
    assert!(app.game.as_ref().unwrap().viewer.pause_requested());
    assert_eq!(app.options.native, native);
    assert_eq!(app.game.as_ref().unwrap().launch.args(), launch);
    publisher
        .reply_output(&OutputReply {
            id,
            result: Err("owner refused settings".into()),
        })
        .unwrap();
    app.collect_game();
    assert!(!app.game.as_ref().unwrap().viewer.output_pending());
}
#[test]
fn delayed_correlated_reply_after_back_refreshes_clean_draft_without_overwriting_new_child_message()
{
    let (mut app, publisher) = prepared();
    let play = app.navigator.active_id().unwrap();
    app.open_live_audio();
    let old = app.navigator.active_id().unwrap();
    app.apply_live_audio();
    let id = publisher.take_output_request().unwrap().unwrap().id;
    app.back();
    assert_eq!(app.navigator.active_id(), Some(play));
    app.open_live_audio();
    let new = app.navigator.active_id().unwrap();
    assert_ne!(old, new);
    app.live_audio.as_mut().unwrap().message = Some("new child message".into());
    publisher
        .reply_output(&OutputReply {
            id,
            result: Ok(cap("actual-applied-device")),
        })
        .unwrap();
    app.collect_game();
    assert_eq!(
        app.live_audio.as_ref().unwrap().values.native_args(),
        cap("actual-applied-device").current_args
    );
    assert_eq!(
        app.live_audio.as_ref().unwrap().message.as_deref(),
        Some("new child message")
    );
    assert_eq!(app.live_audio.as_ref().unwrap().request, None);
    assert_eq!(app.navigator.active_id(), Some(new));
    assert!(app.navigator.retains(play));
    assert_eq!(
        app.game
            .as_ref()
            .unwrap()
            .viewer
            .output_capability()
            .unwrap(),
        Some(cap("actual-applied-device"))
    );
    assert!(!app.game.as_ref().unwrap().viewer.output_pending());
    assert!(app.game.as_ref().unwrap().viewer.pause_requested());
}
#[test]
fn focus_loss_close_and_joined_terminal_route_dispose_or_suspend_child_without_restarting_native_owner()
 {
    let (mut app, _) = prepared();
    app.open_live_audio();
    let child = app.navigator.active_id().unwrap();
    let play = app
        .navigator
        .stack()
        .iter()
        .find(|entry| entry.route == (ScreenRoute::Play { replay: false }))
        .unwrap()
        .id;
    app.active = false;
    app.sync_ime();
    let before = app.live_audio.as_ref().unwrap().editor.clone();
    app.ime_event(Ime::Commit("inactive".into()));
    assert_eq!(app.live_audio.as_ref().unwrap().editor, before);
    assert!(app.navigator.retains(child));
    app.active = true;
    app.game.as_mut().unwrap().joined = true;
    app.game.as_mut().unwrap().snapshot.as_mut().unwrap().status = player::PlayerStatus::Finished;
    app.collect_game();
    assert_eq!(
        app.navigator.route(),
        ScreenRoute::Results { replay: false }
    );
    assert!(app.navigator.retains(play));
    assert!(app.live_audio.is_none());
    assert!(app.live_audio_view.is_none());
    assert!(app.game.as_ref().unwrap().worker.is_none());
    let (mut app, _) = prepared();
    app.open_live_audio();
    app.request_close();
    assert_eq!(app.navigator.route(), ScreenRoute::Closing);
    assert!(app.live_audio.is_none());
    assert!(app.game.as_ref().unwrap().cancelling);
    assert!(app.game.as_ref().unwrap().worker.is_none());
}

#[test]
fn old_child_success_does_not_replace_an_independently_edited_reopened_draft() {
    let (mut app, publisher) = prepared();
    app.open_live_audio();
    app.apply_live_audio();
    let id = publisher.take_output_request().unwrap().unwrap().id;
    app.back();
    app.open_live_audio();
    let child = app.navigator.active_id();
    let draft = app.live_audio.as_mut().unwrap();
    draft.editor.select_all();
    draft.edit(None, Some("independent-new-draft"));
    draft.message = Some("new child notice".into());
    let edited = draft.values.native_args();
    let editor = draft.editor.clone();
    publisher
        .reply_output(&OutputReply {
            id,
            result: Ok(cap("actual-applied-device")),
        })
        .unwrap();
    app.collect_game();
    let draft = app.live_audio.as_ref().unwrap();
    assert_eq!(draft.values.native_args(), edited);
    assert_eq!(draft.editor, editor);
    assert_eq!(draft.message.as_deref(), Some("new child notice"));
    assert_eq!(draft.request, None);
    assert_eq!(app.navigator.active_id(), child);
    assert_eq!(
        app.game
            .as_ref()
            .unwrap()
            .viewer
            .output_capability()
            .unwrap(),
        Some(cap("actual-applied-device"))
    );
}

#[test]
fn old_child_refusal_does_not_change_clean_reopened_values_or_notice() {
    let (mut app, publisher) = prepared();
    app.open_live_audio();
    app.apply_live_audio();
    let id = publisher.take_output_request().unwrap().unwrap().id;
    app.back();
    app.open_live_audio();
    let draft = app.live_audio.as_mut().unwrap();
    draft.message = Some("new child notice".into());
    let values = draft.values.native_args();
    let editor = draft.editor.clone();
    publisher
        .reply_output(&OutputReply {
            id,
            result: Err("old request refused".into()),
        })
        .unwrap();
    app.collect_game();
    let draft = app.live_audio.as_ref().unwrap();
    assert_eq!(draft.values.native_args(), values);
    assert_eq!(draft.editor, editor);
    assert_eq!(draft.message.as_deref(), Some("new child notice"));
    assert!(app.game.as_ref().unwrap().viewer.pause_requested());
    assert!(!app.game.as_ref().unwrap().viewer.output_pending());
}

#[test]
fn live_audio_navigation_keeps_the_running_worker_until_explicit_close() {
    let (mut app, publisher) = prepared();
    let play = app.navigator.active_id().unwrap();
    let (started, ready) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        player::with_publisher(publisher, || {
            started.send(()).unwrap();
            while !player::cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Ok(())
        })
    });
    app.game.as_mut().unwrap().worker = Some(worker);
    ready
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();

    for _ in 0..2 {
        app.key(KeyCode::F2, false);
        assert_eq!(app.navigator.route(), ScreenRoute::LiveAudio);
        let game = app.game.as_ref().unwrap();
        assert!(!game.cancelling);
        assert!(!game.joined);
        assert!(!game.worker.as_ref().unwrap().is_finished());
        app.activate(ControlId(91));
        assert_eq!(app.navigator.active_id(), Some(play));
        assert!(
            !app.game
                .as_ref()
                .unwrap()
                .worker
                .as_ref()
                .unwrap()
                .is_finished()
        );
        assert!(app.game.as_ref().unwrap().viewer.pause_requested());
    }

    app.request_close();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
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
            std::time::Instant::now() < deadline,
            "cancelled worker did not exit"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.collect_game();
    assert_eq!(app.navigator.route(), ScreenRoute::Closing);
    assert!(app.game.as_ref().unwrap().joined);
    assert!(app.game.as_ref().unwrap().worker.is_none());
}
