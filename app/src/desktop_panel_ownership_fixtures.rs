//! Connected Desktop owner regressions: real routes, retained views and completion fences.
use super::*;
use beatkernel_bms_runtime::{
    live_output_control::{OutputCapability, OutputReply},
    panel_scope::PanelPhase,
    settings::SettingsHost,
};

fn settings() -> Desktop {
    let mut app = tests::lifecycle_fixture();
    app.open_settings();
    app
}

fn keyboard_catalog() -> DeviceCatalog {
    DeviceCatalog::new(
        DeviceRequest::LinuxKeyboard,
        vec![
            beatkernel_bms_runtime::device_catalog::DeviceChoice {
                id: "/dev/input/ownership-fixture".into(),
                label: "OWNERSHIP KEYBOARD".into(),
                detail: "METADATA ONLY".into(),
                selectable: true,
            },
            beatkernel_bms_runtime::device_catalog::DeviceChoice {
                id: "/dev/input/ownership-primary".into(),
                label: "PRIMARY KEYBOARD".into(),
                detail: "METADATA ONLY".into(),
                selectable: true,
            },
        ],
    )
    .unwrap()
}

// Inject only the existing IO completion boundary. Desktop still owns admission,
// collection, route changes, cancellation and release; no alternate controller.
fn pending(app: &mut Desktop, result: Result<ProfileResult, String>) {
    let (owner, permit) = app.metadata_scope().unwrap();
    app.profile_io = Some(ProfileOperation {
        owner,
        permit,
        worker: Some(std::thread::spawn(move || result)),
    });
}

fn collect(app: &mut Desktop) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app
        .profile_io
        .as_ref()
        .unwrap()
        .worker
        .as_ref()
        .unwrap()
        .is_finished()
    {
        assert!(
            std::time::Instant::now() < deadline,
            "metadata fixture timed out"
        );
        std::thread::yield_now();
    }
    app.collect_profile();
    assert!(app.profile_io.is_none());
}

fn devices(app: &mut Desktop) {
    let player = app.local_setup.as_ref().unwrap().model.players()[1].id;
    pending(
        app,
        Ok(ProfileResult::Devices(keyboard_catalog(), Some(player))),
    );
    app.draw().unwrap();
    collect(app);
    assert_eq!(
        app.navigator.route(),
        ScreenRoute::Devices { players: true }
    );
}

#[test]
fn settings_lazy_mount_retains_exact_parent_and_reopen_rejects_old_completion() {
    let mut app = settings();
    let id = app.settings.as_ref().unwrap().id();
    let permit = app.settings.as_ref().unwrap().task_permit();
    assert!(app.settings_view.is_none());
    app.settings.as_mut().unwrap().profile = LineEditor::new("kept-profile", 4096).unwrap();
    app.draw().unwrap();
    assert_eq!(app.settings_view.as_ref().unwrap().id(), id);
    app.open_local();
    assert_eq!(permit.phase(), PanelPhase::Retained);
    assert_eq!(app.settings_view.as_ref().unwrap().id(), id);
    app.back();
    assert!(permit.is_active());
    assert_eq!(app.navigator.active_id(), Some(id));
    assert_eq!(
        app.settings.as_ref().unwrap().profile.value(),
        "kept-profile"
    );
    app.draw().unwrap();
    pending(&mut app, Ok(ProfileResult::Saved));
    app.draw().unwrap();
    // Pending UI refuses edits and navigation.
    app.back();
    assert_eq!(app.navigator.active_id(), Some(id));
    collect(&mut app);
    assert_eq!(
        app.settings.as_ref().unwrap().message.as_deref(),
        Some("PROFILE SAVED - APPLY IS SEPARATE")
    );
    pending(&mut app, Err("profile completion refused".into()));
    app.draw().unwrap();
    collect(&mut app);
    assert_eq!(
        app.settings.as_ref().unwrap().error.as_deref(),
        Some("profile completion refused")
    );
    assert_eq!(
        app.settings.as_ref().unwrap().profile.value(),
        "kept-profile"
    );
    assert_eq!(app.settings_view.as_ref().unwrap().id(), id);
    app.back();
    assert!(app.settings.is_none() && app.settings_view.is_none());
    assert!(permit.is_cancelled());
    assert!(app.hits.is_empty());
    app.open_settings();
    let fresh = app.settings.as_ref().unwrap().id();
    assert_ne!(fresh, id);
    assert!(app.settings_view.is_none());
    let before = app.settings.as_ref().unwrap().profile.clone();
    app.profile_io = Some(ProfileOperation {
        owner: id,
        permit,
        worker: Some(std::thread::spawn(|| Ok(ProfileResult::Saved))),
    });
    collect(&mut app);
    assert_eq!(app.settings.as_ref().unwrap().profile, before);
    assert!(app.settings.as_ref().unwrap().message.is_none());
    app.draw().unwrap();
    assert_eq!(app.settings_view.as_ref().unwrap().id(), fresh);
}

#[test]
fn players_and_devices_mount_keep_parent_and_refused_selection_then_accept_once() {
    let mut app = settings();
    app.draw().unwrap();
    let settings_id = app.settings.as_ref().unwrap().id();
    let native = app.options.native.clone();
    app.open_local();
    let parent = app.local_setup.as_ref().unwrap().id();
    let parent_permit = app.local_setup.as_ref().unwrap().task_permit();
    assert!(app.players_view.is_none());
    app.local_setup.as_mut().unwrap().model.resize(2).unwrap();
    app.local_setup.as_mut().unwrap().selected = 1;
    app.draw().unwrap();
    assert_eq!(app.players_view.as_ref().unwrap().id(), parent);
    devices(&mut app);
    let child = app.picker.as_ref().unwrap().id();
    let child_permit = app.picker.as_ref().unwrap().task_permit();
    assert!(app.devices_view.is_none());
    assert_eq!(parent_permit.phase(), PanelPhase::Retained);
    assert_eq!(app.players_view.as_ref().unwrap().id(), parent);
    app.draw().unwrap();
    assert_eq!(app.devices_view.as_ref().unwrap().id(), child);
    app.use_device(); // No selected device: validation must leave both owners intact.
    assert_eq!(app.navigator.active_id(), Some(child));
    assert!(child_permit.is_active());
    assert_eq!(app.local_setup.as_ref().unwrap().selected, 1);
    pending(&mut app, Err("catalog refresh refused".into()));
    app.draw().unwrap();
    collect(&mut app);
    assert_eq!(app.navigator.active_id(), Some(child));
    assert_eq!(
        app.picker.as_ref().unwrap().catalog.choices()[0].id,
        "/dev/input/ownership-fixture"
    );
    assert_eq!(
        app.settings.as_ref().unwrap().error.as_deref(),
        Some("catalog refresh refused")
    );
    app.picker.as_mut().unwrap().selected = Some(0);
    app.use_device();
    assert_eq!(app.navigator.active_id(), Some(parent));
    assert!(app.picker.is_none() && app.devices_view.is_none());
    assert!(child_permit.is_cancelled() && parent_permit.is_active());
    assert_eq!(app.players_view.as_ref().unwrap().id(), parent);
    assert_eq!(app.local_setup.as_ref().unwrap().selected, 1);
    assert_eq!(app.options.native, native);
    devices(&mut app);
    assert_ne!(app.picker.as_ref().unwrap().id(), child);
    assert!(app.devices_view.is_none());
    app.back();
    let primary = app.local_setup.as_ref().unwrap().model.players()[0].id;
    app.local_setup
        .as_mut()
        .unwrap()
        .model
        .assign(primary, &keyboard_catalog(), 1)
        .unwrap();
    app.finish_local();
    assert_eq!(app.navigator.active_id(), Some(settings_id));
    assert!(app.local_setup.is_none() && app.players_view.is_none());
    assert!(parent_permit.is_cancelled());
    app.open_local();
    assert_ne!(app.local_setup.as_ref().unwrap().id(), parent);
    assert_eq!(app.local_setup.as_ref().unwrap().model.players().len(), 2);
    assert!(app.players_view.is_none());
}

#[test]
fn players_failed_done_preserves_view_model_and_exact_settings_parent() {
    fn refused(_: &[String]) -> Result<(), Box<dyn Error>> {
        Err("fixture validation refused".into())
    }
    let mut app = settings();
    let parent = app.settings.as_ref().unwrap().id();
    let values = app.settings.as_ref().unwrap().values.native_args();
    app.open_local();
    app.local_setup.as_mut().unwrap().model.resize(2).unwrap();
    app.draw().unwrap();
    let child = app.local_setup.as_ref().unwrap().id();
    let permit = app.local_setup.as_ref().unwrap().task_permit();
    app.validate = refused;
    app.finish_local();
    assert_eq!(app.navigator.active_id(), Some(child));
    assert_eq!(app.players_view.as_ref().unwrap().id(), child);
    assert_eq!(app.local_setup.as_ref().unwrap().model.players().len(), 2);
    assert!(permit.is_active());
    assert_eq!(app.settings.as_ref().unwrap().id(), parent);
    assert_eq!(app.settings.as_ref().unwrap().values.native_args(), values);
    assert!(app.settings.as_ref().unwrap().error.is_some());
    app.back();
    assert!(permit.is_cancelled());
    app.open_local();
    assert_ne!(app.local_setup.as_ref().unwrap().id(), child);
    assert_eq!(app.local_setup.as_ref().unwrap().model.players().len(), 1);
}

#[test]
fn records_pending_success_refusal_and_fresh_reopen_preserve_settings_draft() {
    let mut app = settings();
    app.settings.as_mut().unwrap().profile = LineEditor::new("records-parent", 4096).unwrap();
    app.draw().unwrap();
    let parent = app.settings.as_ref().unwrap().id();
    let parent_permit = app.settings.as_ref().unwrap().task_permit();
    app.open_records();
    let child = app.records.as_ref().unwrap().id();
    let permit = app.records.as_ref().unwrap().task_permit();
    assert!(app.records_view.is_none());
    app.records.as_mut().unwrap().directory = LineEditor::new("edited-record-dir", 4096).unwrap();
    app.draw().unwrap();
    assert_eq!(app.records_view.as_ref().unwrap().id(), child);
    pending(
        &mut app,
        Ok(ProfileResult::Records(RecordCatalog {
            entries: Vec::new(),
            truncated: false,
        })),
    );
    app.draw().unwrap();
    app.back();
    assert_eq!(app.navigator.active_id(), Some(child));
    collect(&mut app);
    assert!(app.records.as_ref().unwrap().catalog.is_some());
    assert_eq!(
        app.records.as_ref().unwrap().directory.value(),
        "edited-record-dir"
    );
    pending(&mut app, Err("record query refused".into()));
    collect(&mut app);
    assert_eq!(
        app.records.as_ref().unwrap().error.as_deref(),
        Some("record query refused")
    );
    assert_eq!(app.records.as_ref().unwrap().id(), child);
    assert_eq!(app.records_view.as_ref().unwrap().id(), child);
    app.back();
    assert!(permit.is_cancelled() && parent_permit.is_active());
    assert!(app.records.is_none() && app.records_view.is_none());
    assert!(app.hits.is_empty());
    assert_eq!(app.settings.as_ref().unwrap().id(), parent);
    assert_eq!(app.settings_view.as_ref().unwrap().id(), parent);
    assert_eq!(
        app.settings.as_ref().unwrap().profile.value(),
        "records-parent"
    );
    app.open_records();
    assert_ne!(app.records.as_ref().unwrap().id(), child);
    assert_ne!(
        app.records.as_ref().unwrap().directory.value(),
        "edited-record-dir"
    );
    assert!(app.records_view.is_none());
}

fn capability(device: &str) -> OutputCapability {
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

fn live_audio() -> (Desktop, player::PlayerPublisher) {
    let mut app = tests::lifecycle_fixture();
    let next = app
        .prepare_route(ScreenRoute::Play { replay: false })
        .unwrap();
    app.commit_route(next);
    let (publisher, viewer) = player::channel();
    publisher
        .advertise_output(Some(capability("initial-device")))
        .unwrap();
    viewer.request_pause(true);
    // A new channel contains its default startup snapshot. Consume that before
    // installing the acknowledged paused owner, so later real collect_game()
    // calls cannot overwrite the fixture's Playing/Paused acknowledgement.
    assert!(viewer.take_latest().is_some());
    let mut game = tests::retry_fixture();
    game.viewer = viewer;
    game.snapshot = Some(player::PlayerSnapshot {
        status: player::PlayerStatus::Playing,
        pause: player::PauseState::Paused,
        ..Default::default()
    });
    app.game = Some(game);
    (app, publisher)
}

#[test]
fn live_audio_real_channel_draw_pending_replies_suspend_and_fresh_identity() {
    let (mut app, publisher) = live_audio();
    let play = app.navigator.active_id().unwrap();
    let native = app.options.native.clone();
    let launch = app.game.as_ref().unwrap().launch.args().to_vec();
    app.open_live_audio();
    let child = app.live_audio.as_ref().unwrap().id();
    let permit = app.live_audio.as_ref().unwrap().task_permit();
    // This panel intentionally constructs its retained view on open.
    assert_eq!(app.live_audio_view.as_ref().unwrap().id(), child);
    app.draw().unwrap();
    assert_eq!(app.painted_reactive, Some(child));
    app.navigator.suspend();
    app.synchronize_panel_lifecycles();
    assert_eq!(permit.phase(), PanelPhase::Suspended);
    assert!(!permit.is_cancelled());
    app.navigator.resume();
    app.synchronize_panel_lifecycles();
    assert!(permit.is_active());
    for accepted in [false, true] {
        app.live_audio.as_mut().unwrap().editor.select_all();
        app.live_audio
            .as_mut()
            .unwrap()
            .edit(None, Some("edited-device"));
        let values = app.live_audio.as_ref().unwrap().values.native_args();
        app.apply_live_audio();
        let request = publisher.take_output_request().unwrap().unwrap();
        app.draw().unwrap();
        assert_eq!(
            app.hits.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [ControlId(91)]
        );
        publisher
            .reply_output(&OutputReply {
                id: request.id,
                result: if accepted {
                    Ok(capability("canonical-device"))
                } else {
                    Err("live owner refused".into())
                },
            })
            .unwrap();
        app.collect_game();
        assert_eq!(app.live_audio.as_ref().unwrap().id(), child);
        assert_eq!(app.live_audio.as_ref().unwrap().request, None);
        assert_eq!(
            app.live_audio.as_ref().unwrap().values.native_args(),
            if accepted {
                capability("canonical-device").current_args
            } else {
                values
            }
        );
        assert_eq!(app.options.native, native);
        assert_eq!(app.game.as_ref().unwrap().launch.args(), launch);
    }
    app.back();
    assert!(permit.is_cancelled());
    assert!(app.live_audio.is_none() && app.live_audio_view.is_none());
    assert_eq!(app.navigator.active_id(), Some(play));
    assert!(app.hits.is_empty());
    app.open_live_audio();
    assert_ne!(app.live_audio.as_ref().unwrap().id(), child);
    assert_eq!(
        app.live_audio.as_ref().unwrap().editor.value(),
        "canonical-device"
    );
}

#[test]
fn closing_retires_all_five_drafts_views_and_permits_without_stale_draw_or_completion() {
    for route in 0..5 {
        let mut app = if route == 4 {
            live_audio().0
        } else {
            settings()
        };
        if route == 1 || route == 2 {
            app.open_local();
            app.local_setup.as_mut().unwrap().model.resize(2).unwrap();
            if route == 2 {
                devices(&mut app);
            }
        } else if route == 3 {
            app.open_records();
        } else if route == 4 {
            app.open_live_audio();
        }
        app.draw().unwrap();
        let id = app.navigator.active_id().unwrap();
        let permit = match route {
            0 => app.settings.as_ref().unwrap().task_permit(),
            1 => app.local_setup.as_ref().unwrap().task_permit(),
            2 => app.picker.as_ref().unwrap().task_permit(),
            3 => app.records.as_ref().unwrap().task_permit(),
            _ => app.live_audio.as_ref().unwrap().task_permit(),
        };
        let native = app.options.native.clone();
        if route != 4 {
            pending(&mut app, Err("late completion must be discarded".into()));
        }
        app.request_close();
        assert_eq!(app.navigator.route(), ScreenRoute::Closing);
        assert!(permit.is_cancelled());
        assert!(!app.navigator.retains(id) && !app.navigator.accepts(id));
        assert!(app.settings.is_none() && app.settings_view.is_none());
        assert!(app.local_setup.is_none() && app.players_view.is_none());
        assert!(app.picker.is_none() && app.devices_view.is_none());
        assert!(app.records.is_none() && app.records_view.is_none());
        assert!(app.live_audio.is_none() && app.live_audio_view.is_none());
        assert!(app.hits.is_empty() && app.painted_reactive.is_none());
        if route != 4 {
            collect(&mut app);
        }
        app.draw().unwrap();
        assert!(app.hits.is_empty() && app.painted_reactive.is_none());
        assert_eq!(app.options.native, native);
        assert!(app.failure.is_none());
    }
}
