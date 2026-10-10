//! Opt-in connected native acceptance on an actual X11 software-rendered window.
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

#[cfg(target_os = "linux")]
fn capture_stage(name: &str, extent: [u32; 2]) -> image::RgbaImage {
    let directory = std::path::PathBuf::from(
        std::env::var_os("BEATKERNEL_TEST_NATIVE_UI_ARTIFACT_DIR")
            .expect("set the isolated acceptance PNG artifact directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{name}.png"));
    let helper = std::env::var_os("BEATKERNEL_TEST_NATIVE_UI_CAPTURE")
        .expect("set a capture executable for this owned X11 window");
    // Acquisition belongs to the external acceptance runner. This fixture does
    // not substitute Scene geometry or offscreen output for the window pixels.
    std::thread::sleep(Duration::from_millis(50));
    let status = std::process::Command::new(helper)
        .arg(&path)
        .status()
        .expect("execute direct X11 capture helper");
    assert!(
        status.success(),
        "capture helper failed for {}",
        path.display()
    );
    let image = image::open(&path)
        .expect("decode actual window PNG")
        .to_rgba8();
    assert!(image.width() >= extent[0] && image.height() >= extent[1]);
    assert!(
        image
            .pixels()
            .any(|pixel| pixel.0[0] != 0 || pixel.0[1] != 0 || pixel.0[2] != 0),
        "actual window capture must not be black"
    );
    eprintln!("actual native PNG: {}", path.display());
    // The owned display may be larger than this undecorated window at (0,0).
    image::imageops::crop_imm(&image, 0, 0, extent[0], extent[1]).to_image()
}

#[cfg(target_os = "linux")]
fn present_until(app: &mut Desktop, minimum: u64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.renderer.as_ref().unwrap().presentation_count() < minimum {
        assert!(Instant::now() < deadline, "native frame was not presented");
        app.draw().unwrap();
        std::thread::sleep(Duration::from_millis(16));
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires BEATKERNEL_TEST_NATIVE_UI_WINDOW=1, owned X11 display/GPU and direct PNG capture helper"]
#[allow(deprecated)]
fn remaining_menus_and_genuine_results_present_actual_native_window_motion() {
    use winit::platform::x11::EventLoopBuilderExtX11;
    assert_eq!(
        std::env::var("BEATKERNEL_TEST_NATIVE_UI_WINDOW").as_deref(),
        Ok("1")
    );
    let event_loop = EventLoop::<DesktopUiCommand>::with_user_event()
        .with_x11()
        .with_any_thread(true)
        .build()
        .unwrap();
    let window = Arc::new(
        event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("BeatKernel remaining components acceptance")
                    .with_decorations(false)
                    .with_position(winit::dpi::LogicalPosition::new(0.0, 0.0))
                    .with_inner_size(LogicalSize::new(960.0, 720.0)),
            )
            .unwrap(),
    );
    let size = window.inner_size();
    let extent = [size.width, size.height];
    assert_eq!(
        extent,
        [960, 720],
        "runner must supply a scale-one isolated display"
    );
    let instance = graphics::instance(BackendChoice::Auto).unwrap();
    let surface = instance.create_surface(window.clone()).unwrap();
    let mut renderer =
        pollster::block_on(Renderer::new(surface, &instance, Presentation::Fifo)).unwrap();
    renderer.resize(size.width, size.height).unwrap();
    let description = renderer.description();
    let mut renderer = Some(renderer);
    let mut instance = Some(instance);
    let mut total_motion_frames = 0;

    for route in 0..5 {
        let (mut app, menu_target) = if route < 4 {
            let (app, control, x) = menu_fixture(route);
            (app, Some((control, x)))
        } else {
            (results_fixture(5), None)
        };
        app.window = Some(window.clone());
        app.renderer = renderer.take();
        app.instance = instance.take();
        let before = app.renderer.as_ref().unwrap().presentation_count();
        present_until(&mut app, before + 1);
        let baseline = capture_stage(&format!("surface-{route}-baseline"), extent);
        let baseline_hits = app.hits.clone();
        let (reply, received) = std::sync::mpsc::sync_channel(1);
        app.handle_ui_command(DesktopUiCommand::InspectScreen { reply });
        let screen = received.recv().unwrap().unwrap();
        // Footer controls stay inside the fixed footer clip when moving along
        // its horizontal axis. Results cards have their own vertical source.
        let offset = if menu_target.is_some() {
            [80.0, 0.0]
        } else {
            [0.0, -80.0]
        };
        let destination = UiTransform::new(offset, [1.0, 1.0], 1.0).unwrap();
        let movement = ComponentMotion::new(
            UiTransform::default(),
            destination,
            Duration::from_millis(250),
            Easing::Linear,
        );
        let (reply, received) = std::sync::mpsc::sync_channel(1);
        let node = if let Some((control, x)) = menu_target {
            assert_eq!(app.hit_motion((x, 621.0), extent), Some(control));
            let node = app.control_node(control).unwrap();
            app.handle_ui_command(DesktopUiCommand::RequestMotion {
                screen,
                control,
                motion: movement,
                reply,
            });
            node
        } else {
            let nodes = results_nodes(&app, 0);
            assert_eq!(nodes.len(), 6);
            app.handle_ui_command(DesktopUiCommand::RequestNodeMotion {
                screen,
                node: nodes[1],
                motion: movement,
                reply,
            });
            nodes[1]
        };
        received.recv().unwrap().unwrap();
        let key = UiComponentKey { screen, node };
        let geometry = Arc::clone(app.scene.geometry_stamp().0);
        let revision = app.scene.geometry_stamp().1;
        let first = app.renderer.as_ref().unwrap().presentation_count();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut frames = 0;
        loop {
            assert!(
                Instant::now() < deadline,
                "surface {route} motion did not finish"
            );
            let previous = app.renderer.as_ref().unwrap().presentation_count();
            app.draw().unwrap();
            let current = app.renderer.as_ref().unwrap().presentation_count();
            if current > previous {
                frames += 1;
                assert_eq!(app.ui_motion.presented_screen, Some(screen));
            }
            assert!(Arc::ptr_eq(&geometry, app.scene.geometry_stamp().0));
            assert_eq!(app.scene.geometry_stamp().1, revision);
            if !app.control_motion_active() && current > previous {
                break;
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        assert!(frames >= 2);
        total_motion_frames += frames;
        assert!(app.renderer.as_ref().unwrap().presentation_count() >= first + 2);
        assert_eq!(
            app.scene
                .component_transform(app.scene.component_id(key).unwrap()),
            Some(destination)
        );
        let moved = capture_stage(&format!("surface-{route}-moved"), extent);
        assert_ne!(
            baseline.as_raw(),
            moved.as_raw(),
            "surface {route} motion must change actual window pixels"
        );
        // Presentation changes occur below the ordinary heading/status region.
        for y in 0..100 {
            for x in 0..extent[0] {
                assert_eq!(
                    baseline.get_pixel(x, y),
                    moved.get_pixel(x, y),
                    "ordinary header changed on surface {route}"
                );
            }
        }
        if let Some((control, x)) = menu_target {
            assert_ne!(app.hit_motion((x, 621.0), extent), Some(control));
            assert_eq!(app.hit_motion((x + 80.0, 621.0), extent), Some(control));
        } else {
            assert_eq!(app.hits.len(), baseline_hits.len());
            for ((control, bounds), (old_control, old_bounds)) in
                app.hits.iter().zip(&baseline_hits)
            {
                assert_eq!(control, old_control);
                assert_eq!(
                    [bounds.x, bounds.y, bounds.width, bounds.height],
                    [
                        old_bounds.x,
                        old_bounds.y,
                        old_bounds.width,
                        old_bounds.height
                    ]
                );
            }
            assert_eq!(
                app.hit_motion((800.0, 80.0), extent),
                Some(ControlId(3)),
                "ordinary Results RETURN action must stay available"
            );
            assert_eq!(app.hit_motion((30.0, 150.0), extent), None);
            // Score-card motion must preserve the full ordinary Results footer.
            for y in 600..720 {
                for x in 0..extent[0] {
                    assert_eq!(baseline.get_pixel(x, y), moved.get_pixel(x, y));
                }
            }
            app.change_local_page(true);
            let count = app.renderer.as_ref().unwrap().presentation_count();
            present_until(&mut app, count + 1);
            assert_eq!(app.game.as_ref().unwrap().local_page, 1);
            assert_eq!(results_nodes(&app, 1).len(), 3);
            capture_stage("results-partial-page", extent);
        }
        app.back();
        let count = app.renderer.as_ref().unwrap().presentation_count();
        present_until(&mut app, count + 1);
        assert_ne!(app.navigator.active_id(), Some(screen));
        assert!(app.scene.component_id(key).is_none());
        assert!(app
            .ui_motion
            .owners
            .iter()
            .all(|owner| owner.screen != screen));
        eprintln!("actual native surface {route}: {description}; frames={frames}; geometry_revision={revision}; header preserved; owner retired");
        renderer = app.renderer.take();
        instance = app.instance.take();
        app.window = None;
        app.request_close();
        assert!(app.ui_motion.disposed);
    }
    assert!(total_motion_frames >= 10);
    drop(renderer);
    drop(instance);
    drop(window);
    drop(event_loop);
}
