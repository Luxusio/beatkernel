//! Actual native owner tests without opening devices, windows or worker threads.
use super::*;
use beatkernel::audio::{
    command_queue, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank,
};
use beatkernel::input::BindingMap;
use beatkernel::time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp};
use beatkernel_bms_runtime::{
    gauge::BmsGauge,
    play_result::CompletedPlayResult,
    step_gameplay::{StepGameplay, StepGameplayConfig},
    ui::motion::Easing,
};

fn motion(offset: [f32; 2]) -> ComponentMotion {
    ComponentMotion::new(
        UiTransform::default(),
        UiTransform::new(offset, [1.0, 1.0], 1.0).unwrap(),
        Duration::from_nanos(100),
        Easing::Linear,
    )
}

fn hit_regions(app: &Desktop) -> Vec<(u64, [i64; 4])> {
    app.hits
        .iter()
        .map(|(id, b)| (id.0, [b.x, b.y, b.width, b.height]))
        .collect()
}

fn menu_fixture(route: usize) -> (Desktop, ControlId, f64) {
    let mut app = tests::lifecycle_fixture();
    app.open_settings();
    let (control, x) = match route {
        0 => (ControlId(10), 25.0),
        1 => {
            app.open_records();
            (ControlId(50), 25.0)
        }
        2 => {
            app.open_local();
            (ControlId(33), 475.0)
        }
        3 => {
            app.open_local();
            app.local_setup.as_mut().unwrap().model.resize(2).unwrap();
            let player = app.local_setup.as_ref().unwrap().model.players()[1].id;
            let next = app
                .prepare_route(ScreenRoute::Devices { players: true })
                .unwrap();
            app.commit_route(next);
            let screen = app.navigator.active_id().unwrap();
            let catalog = DeviceCatalog::new(
                DeviceRequest::LinuxKeyboard,
                vec![beatkernel_bms_runtime::device_catalog::DeviceChoice {
                    id: "/dev/input/fixture".into(),
                    label: "FIXTURE KEYBOARD".into(),
                    detail: "METADATA ONLY".into(),
                    selectable: true,
                }],
            )
            .unwrap();
            app.picker = Some(PanelScope::new(
                screen,
                DevicePicker {
                    catalog,
                    player: Some(player),
                    first: 0,
                    selected: None,
                },
            ));
            (ControlId(22), 401.0)
        }
        _ => unreachable!(),
    };
    app.sync_ime();
    app.draw().unwrap();
    (app, control, x)
}

#[test]
fn native_remaining_four_menus_publish_only_accepted_pose_and_reuse_geometry() {
    for route in 0..4 {
        let (mut app, control, x) = menu_fixture(route);
        let screen = app.navigator.active_id().unwrap();
        app.publish_control_pose([960, 720]).unwrap();
        assert_eq!(app.hit_motion((x, 621.0), [960, 720]), Some(control));
        let node = app.control_node(control).unwrap();
        app.request_control_motion(screen, control, motion([100.0, 0.0]), Duration::ZERO)
            .unwrap();
        let key = UiComponentKey { screen, node };
        let identity = Arc::clone(app.scene.geometry_stamp().0);
        let revision = app.scene.geometry_stamp().1;
        app.tick_control_motion(Duration::from_nanos(50), [960, 720])
            .unwrap();
        assert_eq!(app.hit_motion((x, 621.0), [960, 720]), Some(control));
        assert!(app.publish_control_pose([0, 720]).is_err());
        assert_eq!(app.hit_motion((x, 621.0), [960, 720]), Some(control));
        app.publish_control_pose([960, 720]).unwrap();
        assert_ne!(app.hit_motion((x, 621.0), [960, 720]), Some(control));
        assert_eq!(app.hit_motion((x + 51.0, 621.0), [960, 720]), Some(control));
        assert_eq!(
            app.scene
                .component_transform(app.scene.component_id(key).unwrap())
                .unwrap()
                .offset(),
            [50.0, 0.0]
        );
        app.draw().unwrap();
        assert!(Arc::ptr_eq(&identity, app.scene.geometry_stamp().0));
        assert_eq!(revision, app.scene.geometry_stamp().1);
        let vertical = UiTransform::new([0.0, -50.0], [1.0, 1.0], 1.0).unwrap();
        app.request_control_motion(
            screen,
            control,
            ComponentMotion::new(vertical, vertical, Duration::ZERO, Easing::Linear),
            Duration::from_nanos(50),
        )
        .unwrap();
        app.publish_control_pose([960, 720]).unwrap();
        assert_ne!(
            app.hit_motion((x, 571.0), [960, 720]),
            Some(control),
            "footer motion must obey fixed parent clip"
        );
    }
}

#[test]
fn native_remaining_menu_refusal_suspension_and_back_retire_only_child() {
    for route in 0..4 {
        let (mut app, control, _) = menu_fixture(route);
        let screen = app.navigator.active_id().unwrap();
        let node = app.control_node(control).unwrap();
        app.request_control_motion(screen, control, motion([0.0, -100.0]), Duration::ZERO)
            .unwrap();
        app.tick_control_motion(Duration::from_nanos(25), [960, 720])
            .unwrap();
        let key = UiComponentKey { screen, node };
        let id = app.scene.component_id(key).unwrap();
        let pose = app.scene.component_transform(id);
        for (owner, target, time) in [
            (ScreenInstanceId(u64::MAX), control, 25),
            (screen, ControlId(u64::MAX), 25),
            (screen, control, 24),
        ] {
            assert!(app
                .request_control_motion(
                    owner,
                    target,
                    motion([900.0, 0.0]),
                    Duration::from_nanos(time)
                )
                .is_err());
        }
        assert_eq!(app.scene.component_transform(id), pose);
        assert_eq!(app.ui_motion.time, Duration::from_nanos(25));
        assert!(!app
            .tick_control_motion(Duration::from_nanos(25), [0, 720])
            .unwrap());
        app.tick_control_motion(Duration::from_nanos(1000), [960, 720])
            .unwrap();
        assert_eq!(app.scene.component_transform(id), pose);
        app.tick_control_motion(Duration::from_nanos(1025), [960, 720])
            .unwrap();
        assert_eq!(
            app.scene.component_transform(id).unwrap().offset(),
            [0.0, -50.0]
        );
        app.back();
        app.draw().unwrap();
        assert_ne!(app.navigator.active_id(), Some(screen));
        assert!(app.scene.component_id(key).is_none());
        assert!(app
            .ui_motion
            .owners
            .iter()
            .all(|owner| owner.screen != screen));
        assert!(app
            .request_control_motion(
                screen,
                control,
                motion([1.0, 0.0]),
                Duration::from_nanos(1025)
            )
            .is_err());
    }
}

#[test]
fn native_settings_editor_motion_keeps_accepted_ime_anchor_during_preedit_repaint() {
    let (mut app, _, _) = menu_fixture(0);
    let screen = app.navigator.active_id().unwrap();
    let control = ControlId(1000 + app.settings.as_ref().unwrap().selected as u64);
    let node = app.control_node(control).unwrap();
    app.publish_control_pose([960, 720]).unwrap();
    let initial = app.ime_cursor_area([960, 720]).unwrap();
    app.request_control_motion(screen, control, motion([-100.0, 0.0]), Duration::ZERO)
        .unwrap();
    app.tick_control_motion(Duration::from_nanos(50), [960, 720])
        .unwrap();
    assert_eq!(app.ime_cursor_area([960, 720]), Some(initial));
    app.publish_control_pose([960, 720]).unwrap();
    let moved = [initial[0] - 50, initial[1], initial[2], initial[3]];
    assert_eq!(app.ime_cursor_area([960, 720]), Some(moved));
    let key = UiComponentKey { screen, node };
    let value = app.settings.as_ref().unwrap().editor.value().to_owned();
    app.ime_event(Ime::Enabled);
    app.ime_event(Ime::Preedit("12".into(), Some((0, 2))));
    assert!(app.ime.preview.is_some());
    app.draw().unwrap();
    assert_eq!(app.settings.as_ref().unwrap().editor.value(), value);
    assert_eq!(
        app.scene
            .component_transform(app.scene.component_id(key).unwrap())
            .unwrap()
            .offset(),
        [-50.0, 0.0]
    );
    assert_eq!(app.ime_cursor_area([960, 720]), Some(moved));
}

struct Domains;
impl ClockMapper for Domains {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (point.domain == target).then_some(point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn completion() -> (CompletedPlayResult, BmsGauge) {
    let source = beatkernel_bms::parse("#BPM 120\n", Default::default()).unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let prepared = beatkernel_bms_runtime::PreparedBms {
        compiled: source.compile().unwrap(),
        source,
        bank: SampleBank::new(format, PcmLimits::new(64, 256, 1).unwrap()).unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    };
    let config = StepGameplayConfig {
        host_origin: point(1, 0),
        output_origin: point(2, 0),
        preroll: beatkernel::time::Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 8,
        bgm_pending: 4,
        bgm_lookahead: beatkernel::time::Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 8,
    };
    let (mut owner, bank) = StepGameplay::new_section(
        prepared,
        config,
        BindingMap::from_bindings([]).unwrap(),
        Timestamp::ZERO,
        None,
    )
    .unwrap();
    owner.activate(point(1, 0)).unwrap();
    owner
        .advance_to(point(1, 1_000_000), &Domains, point(2, 0))
        .unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let first = mixer.render(&mut [0.0; 10]).unwrap();
    assert!(!owner
        .observe_completion(Some(first), Some(point(2, 10_000_000)))
        .unwrap());
    let second = mixer.render(&mut [0.0; 10]).unwrap();
    assert!(owner
        .observe_completion(Some(second), Some(point(2, 20_000_000)))
        .unwrap());
    (*owner.completed_result().unwrap(), owner.gauge().clone())
}
fn results_fixture(count: usize) -> Desktop {
    let (result, gauge) = completion();
    let (_, viewer) = player::channel();
    let mut game = Game {
        viewer,
        worker: None,
        snapshot: None,
        cancelling: false,
        joined: false,
        local_page: 0,
        local_comparisons: false,
        replay: false,
        launch: SessionLaunch::new(vec!["--chart".into(), "fixture.bms".into()]).unwrap(),
        prepared_retry: None,
        practice_bookmark: None,
        practice_loop: None,
        loop_enabled: false,
        completed_results: None,
        completed_results_error: None,
    };
    let ids = (0..count)
        .map(|index| PlayerId(70 + index as u32))
        .collect::<Vec<_>>();
    game.accept_snapshot(player::PlayerSnapshot {
        players: ids
            .iter()
            .map(|&player| player::LocalPlayerSnapshot {
                bms_score: None,
                player,
                chart: None,
                song_time: Some(Timestamp::ZERO),
                score: Default::default(),
                mine_damage: Default::default(),
                gauge: gauge.clone(),
                last_judge: None,
                recent_results: vec![],
                pressed_lanes: 0,
                note_progress: None,
                competition: None,
            })
            .collect(),
        completed_results: Some(ids.iter().map(|&id| (id, result)).collect()),
        status: player::PlayerStatus::Playing,
        ..Default::default()
    });
    game.owner_finished(true);
    let mut app = tests::lifecycle_fixture();
    app.game = Some(game);
    app.navigate(ScreenRoute::Play { replay: false }).unwrap();
    app.navigate(ScreenRoute::Results { replay: false })
        .unwrap();
    app.draw().unwrap();
    app
}
fn results_nodes(app: &Desktop, page: usize) -> Vec<NodeId> {
    app.game
        .as_ref()
        .unwrap()
        .completed_results
        .as_ref()
        .unwrap()
        .displayed_nodes(page, false)
        .unwrap()
}

#[test]
fn native_results_typed_commands_require_actual_readonly_completion_owner() {
    let mut app = results_fixture(1);
    let ordinary_hits = hit_regions(&app);
    let screen = app.navigator.active_id().unwrap();
    let (reply, received) = std::sync::mpsc::sync_channel(1);
    app.handle_ui_command(DesktopUiCommand::InspectScreen { reply });
    assert_eq!(received.recv().unwrap(), Some(screen));
    let (reply, received) = std::sync::mpsc::sync_channel(1);
    app.handle_ui_command(DesktopUiCommand::InspectResultsNodes { screen, reply });
    let nodes = received.recv().unwrap().unwrap();
    assert_eq!(nodes, results_nodes(&app, 0));
    let pose = UiTransform::new([25.0, 0.0], [1.0, 1.0], 1.0).unwrap();
    let (reply, received) = std::sync::mpsc::sync_channel(1);
    app.handle_ui_command(DesktopUiCommand::RequestNodeMotion {
        screen,
        node: nodes[0],
        motion: ComponentMotion::new(pose, pose, Duration::ZERO, Easing::Linear),
        reply,
    });
    received.recv().unwrap().unwrap();
    let key = UiComponentKey {
        screen,
        node: nodes[0],
    };
    assert_eq!(
        app.scene
            .component_transform(app.scene.component_id(key).unwrap()),
        Some(pose)
    );
    assert_eq!(
        hit_regions(&app),
        ordinary_hits,
        "readonly Results motion must preserve ordinary controls without inventing node controls"
    );
    app.publish_control_pose([960, 720]).unwrap();
    assert_eq!(app.hit_motion((30.0, 101.0), [960, 720]), None);
    let time = app.ui_motion.time;
    assert!(app
        .request_control_motion(screen, ControlId(10), motion([1.0, 0.0]), time)
        .is_err());
    for (owner, node) in [
        (ScreenInstanceId(u64::MAX), nodes[0]),
        (screen, NodeId(1023)),
    ] {
        assert!(app
            .request_node_motion(owner, node, motion([500.0, 0.0]), time)
            .is_err());
    }
    assert_eq!(
        app.scene
            .component_transform(app.scene.component_id(key).unwrap()),
        Some(pose)
    );
    let (mut menu, _, _) = menu_fixture(0);
    assert!(menu
        .request_node_motion(
            menu.navigator.active_id().unwrap(),
            nodes[0],
            motion([1.0, 0.0]),
            Duration::ZERO
        )
        .is_err());
}

#[test]
fn native_results_motion_only_draw_reuses_cache_and_rejects_regressed_time() {
    let mut app = results_fixture(2);
    let ordinary_hits = hit_regions(&app);
    let screen = app.navigator.active_id().unwrap();
    let node = results_nodes(&app, 0)[1];
    app.request_node_motion(screen, node, motion([0.0, 100.0]), Duration::ZERO)
        .unwrap();
    let key = UiComponentKey { screen, node };
    let identity = Arc::clone(app.scene.geometry_stamp().0);
    let revision = app.scene.geometry_stamp().1;
    app.tick_control_motion(Duration::from_nanos(50), [960, 720])
        .unwrap();
    let id = app.scene.component_id(key).unwrap();
    let pose = app.scene.component_transform(id);
    assert_eq!(pose.unwrap().offset(), [0.0, 50.0]);
    assert!(app
        .request_node_motion(screen, node, motion([900.0, 0.0]), Duration::from_nanos(49))
        .is_err());
    assert_eq!(app.ui_motion.time, Duration::from_nanos(50));
    app.draw().unwrap();
    app.draw().unwrap();
    assert!(Arc::ptr_eq(&identity, app.scene.geometry_stamp().0));
    assert_eq!(revision, app.scene.geometry_stamp().1);
    assert_eq!(
        app.scene
            .component_transform(app.scene.component_id(key).unwrap()),
        pose
    );
    assert_eq!(hit_regions(&app), ordinary_hits);
}

#[test]
fn native_results_partial_page_prunes_absent_cards_and_preserves_surviving_pose() {
    let mut app = results_fixture(5);
    let screen = app.navigator.active_id().unwrap();
    let first = results_nodes(&app, 0);
    let next = results_nodes(&app, 1);
    let removed = *first.iter().find(|node| !next.contains(node)).unwrap();
    let surviving = first[0];
    for node in [removed, surviving] {
        app.request_node_motion(screen, node, motion([100.0, 0.0]), Duration::ZERO)
            .unwrap();
    }
    app.tick_control_motion(Duration::from_nanos(25), [960, 720])
        .unwrap();
    app.change_local_page(true);
    app.draw().unwrap();
    assert_eq!(app.game.as_ref().unwrap().local_page, 1);
    assert!(app
        .scene
        .component_id(UiComponentKey {
            screen,
            node: removed
        })
        .is_none());
    assert!(app
        .request_node_motion(
            screen,
            removed,
            motion([1.0, 0.0]),
            Duration::from_nanos(25)
        )
        .is_err());
    let key = UiComponentKey {
        screen,
        node: surviving,
    };
    assert_eq!(
        app.scene
            .component_transform(app.scene.component_id(key).unwrap())
            .unwrap()
            .offset(),
        [25.0, 0.0]
    );
    assert!(!app
        .tick_control_motion(Duration::from_nanos(25), [0, 720])
        .unwrap());
    app.tick_control_motion(Duration::from_nanos(1000), [960, 720])
        .unwrap();
    assert_eq!(
        app.scene
            .component_transform(app.scene.component_id(key).unwrap())
            .unwrap()
            .offset(),
        [25.0, 0.0]
    );
    app.back();
    app.draw().unwrap();
    assert_ne!(app.navigator.active_id(), Some(screen));
    assert!(app
        .ui_motion
        .owners
        .iter()
        .all(|owner| owner.screen != screen));
    assert!(app
        .request_node_motion(
            screen,
            surviving,
            motion([1.0, 0.0]),
            Duration::from_nanos(1000)
        )
        .is_err());
}

#[test]
fn native_records_chooser_to_details_and_grade_pages_prune_absent_motion_nodes() {
    use beatkernel_bms_runtime::{
        competition::ScoreSummary,
        play_result::{PlayResultOutcome, PlayResultScope},
        result_archive::{ArchivedResult, ArchivedScore},
    };
    let (mut app, _, _) = menu_fixture(1);
    let screen = app.navigator.active_id().unwrap();
    let path = PathBuf::from("record11.bkr");
    let score = ScoreSummary {
        hits: 20,
        combo: 20,
        max_combo: 20,
        grades: (0..20).map(|grade| (grade, 1)).collect(),
        ..Default::default()
    };
    {
        let records = app.records.as_mut().unwrap();
        records.catalog = Some(RecordCatalog {
            entries: (0..25)
                .map(|index| PathBuf::from(format!("record{index}.bkr")))
                .collect(),
            truncated: false,
        });
        records.select(11);
        records.preview = Some(RecordPreview {
            path,
            records: 0,
            recorded_until: None,
            start: Timestamp::ZERO,
            end: None,
            historical: Some((
                PlayerId(9),
                ArchivedResult {
                    scope: PlayResultScope::FullSong,
                    outcome: PlayResultOutcome::BelowClearThreshold,
                    gauge: *BmsGauge::default().snapshot(),
                },
            )),
            historical_score: Some(Arc::new(ArchivedScore::from_summary(&score).unwrap())),
            bms_score: None,
            historical_bms_score: None,
            historical_comparison: None,
            archive_error: None,
            score: Default::default(),
        });
    }
    app.sync_ime();
    app.draw().unwrap();
    let chooser = [ControlId(66), ControlId(50010)];
    let keys = chooser.map(|control| UiComponentKey {
        screen,
        node: app.control_node(control).unwrap(),
    });
    for control in chooser {
        app.request_control_motion(screen, control, motion([10.0, 0.0]), Duration::ZERO)
            .unwrap();
    }
    app.tick_control_motion(Duration::from_nanos(25), [960, 720])
        .unwrap();
    app.toggle_record_details();
    app.draw().unwrap();
    assert!(app.records.as_ref().unwrap().details);
    assert_eq!(app.navigator.active_id(), Some(screen));
    for key in keys {
        assert!(
            app.scene.component_id(key).is_none(),
            "absent chooser nodes must retire before details composition"
        );
        assert!(app
            .ui_motion
            .owners
            .iter()
            .flat_map(|owner| owner.nodes.iter())
            .all(|(node, _)| *node != key.node));
    }
    assert!(
        app.hits
            .iter()
            .any(|(control, _)| *control == ControlId(66)),
        "ordinary detail Back remains available"
    );
    assert!(app
        .request_control_motion(
            screen,
            ControlId(50010),
            motion([1.0, 0.0]),
            Duration::from_nanos(25)
        )
        .is_err());
    let next = app.control_node(ControlId(68)).unwrap();
    app.request_control_motion(
        screen,
        ControlId(68),
        motion([10.0, 0.0]),
        Duration::from_nanos(25),
    )
    .unwrap();
    app.set_record_grade_page(usize::MAX);
    app.draw().unwrap();
    assert!(app
        .scene
        .component_id(UiComponentKey { screen, node: next })
        .is_none());
    assert!(app
        .request_control_motion(
            screen,
            ControlId(68),
            motion([1.0, 0.0]),
            Duration::from_nanos(25)
        )
        .is_err());
    assert!(app
        .hits
        .iter()
        .any(|(control, _)| *control == ControlId(66)));
    app.back();
    app.draw().unwrap();
    assert!(!app.records.as_ref().unwrap().details);
    assert_eq!(app.navigator.active_id(), Some(screen));
    assert!(app
        .hits
        .iter()
        .any(|(control, _)| *control == ControlId(50010)));
}
