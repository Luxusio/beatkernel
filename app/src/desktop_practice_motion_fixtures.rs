use super::*;
use beatkernel_bms_runtime::ui::motion::Easing;

fn practice_fixture() -> Desktop {
    let mut app = tests::lifecycle_fixture();
    app.open_settings();
    app.open_practice();
    // Opening installs the draft after route commitment. Establish its IME
    // target before composing: target changes invalidate painted_reactive.
    app.sync_ime();
    app.draw().unwrap();
    assert_eq!(app.navigator.route(), ScreenRoute::Practice);
    app
}

fn motion(offset: [f32; 2]) -> ComponentMotion {
    ComponentMotion::new(
        UiTransform::default(),
        UiTransform::new(offset, [1.0, 1.0], 1.0).unwrap(),
        Duration::from_nanos(100),
        Easing::Linear,
    )
}

fn request(app: &mut Desktop, control: u64, movement: ComponentMotion) -> UiComponentKey {
    let screen = app.navigator.active_id().unwrap();
    let node = app.control_node(ControlId(control)).unwrap();
    app.request_control_motion(screen, ControlId(control), movement, app.ui_motion.time)
        .unwrap();
    UiComponentKey { screen, node }
}

#[test]
fn desktop_practice_motion_publishes_initial_hits_and_retains_unpresented_pose() {
    let mut app = practice_fixture();
    app.publish_control_pose([960, 720]).unwrap();
    let screen = app.navigator.active_id().unwrap();
    assert_eq!(app.ui_motion.presented_screen, Some(screen));
    assert_eq!(
        app.hit_motion((30.0, 390.0), [960, 720]),
        Some(ControlId(71))
    );
    let key = request(&mut app, 71, motion([100.0, 0.0]));
    let geometry = Arc::clone(app.scene.geometry_stamp().0);
    let revision = app.scene.geometry_stamp().1;
    app.tick_control_motion(Duration::from_nanos(50), [960, 720])
        .unwrap();
    assert_eq!(
        app.hit_motion((30.0, 390.0), [960, 720]),
        Some(ControlId(71))
    );
    assert_eq!(app.hit_motion((210.0, 390.0), [960, 720]), None);
    assert!(app.publish_control_pose([0, 720]).is_err());
    assert_eq!(
        app.hit_motion((30.0, 390.0), [960, 720]),
        Some(ControlId(71))
    );
    app.publish_control_pose([960, 720]).unwrap();
    assert_eq!(app.hit_motion((30.0, 390.0), [960, 720]), None);
    assert_eq!(
        app.hit_motion((210.0, 390.0), [960, 720]),
        Some(ControlId(71))
    );
    assert_eq!(
        app.scene
            .component_transform(app.scene.component_id(key).unwrap())
            .unwrap()
            .offset(),
        [50.0, 0.0]
    );
    assert!(Arc::ptr_eq(&geometry, app.scene.geometry_stamp().0));
    assert_eq!(app.scene.geometry_stamp().1, revision);
    assert!(!app.practice.as_ref().unwrap().view.dirty());
    assert_eq!(
        app.hit_motion((30.0, 160.0), [960, 720]),
        Some(ControlId(70))
    );
}

#[test]
fn desktop_practice_motion_ime_keeps_unanimated_editor_and_cold_admission_anchor() {
    let mut app = practice_fixture();
    app.publish_control_pose([960, 720]).unwrap();
    assert_eq!(app.ime_cursor_area([960, 720]), Some([24, 150, 906, 40]));
    request(&mut app, 71, motion([100.0, 0.0]));
    app.tick_control_motion(Duration::from_nanos(50), [960, 720])
        .unwrap();
    app.publish_control_pose([960, 720]).unwrap();
    let editor = app
        .ui_motion
        .presented_hits
        .iter()
        .find(|hit| hit.control == ControlId(70))
        .unwrap();
    assert_eq!(
        editor.key, None,
        "unbound editor must remain truly unanimated"
    );
    assert_eq!(app.ime_cursor_area([960, 720]), Some([24, 150, 906, 40]));
    // Immediate nonidentity cold admission changes live geometry but has not
    // painted it. The accepted unanimated snapshot remains authoritative.
    let pose = UiTransform::new([10.25, 20.5], [0.5, 0.5], 1.0).unwrap();
    request(
        &mut app,
        70,
        ComponentMotion::new(pose, pose, Duration::ZERO, Easing::Linear),
    );
    assert_eq!(app.ime_cursor_area([960, 720]), Some([24, 150, 906, 40]));
    app.publish_control_pose([960, 720]).unwrap();
    assert_eq!(app.ime_cursor_area([960, 720]), Some([34, 170, 454, 21]));
    // Accepted frame used a 1440x1080 fitted viewport with x=240 bars.
    app.publish_control_pose([1920, 1080]).unwrap();
    assert_eq!(app.ime_cursor_area([800, 600]), Some([291, 255, 680, 31]));
    assert_eq!(app.ime_cursor_area([0, 600]), None);
}

#[test]
fn desktop_practice_motion_ime_unpublished_ticks_repaint_and_zero_opacity() {
    let mut app = practice_fixture();
    app.publish_control_pose([960, 720]).unwrap();
    let key = request(&mut app, 70, motion([0.0, 100.0]));
    app.tick_control_motion(Duration::from_nanos(25), [960, 720])
        .unwrap();
    app.publish_control_pose([960, 720]).unwrap();
    assert_eq!(app.ime_cursor_area([960, 720]), Some([24, 175, 906, 40]));
    app.tick_control_motion(Duration::from_nanos(50), [960, 720])
        .unwrap();
    assert_eq!(app.ime_cursor_area([960, 720]), Some([24, 175, 906, 40]));
    let previous_id = app.scene.component_id(key).unwrap();
    let previous_geometry = Arc::clone(app.scene.geometry_stamp().0);
    let previous_revision = app.scene.geometry_stamp().1;
    app.ime_event(Ime::Enabled);
    app.ime_event(Ime::Preedit("12".into(), Some((0, 2))));
    assert!(app.ime.enabled && app.ime.composing);
    assert!(app.ime.preview.is_some());
    let preview = app
        .ime_editor(
            ImeField::PracticeStart,
            &app.practice.as_ref().unwrap().editor,
        )
        .visible_line(40);
    assert!(preview.composition.is_some());
    assert_eq!(app.practice.as_ref().unwrap().editor.value(), "0:00");
    app.draw().unwrap();
    let rebound = app.scene.component_id(key).unwrap();
    // Ordinary same-key recomposition deliberately preserves binding identity.
    // Cold recomposition can replace the Scene identity and restart its epoch;
    // use the same complete stamp comparison as the actual renderer.
    assert_eq!(rebound, previous_id);
    assert!(
        !Arc::ptr_eq(&previous_geometry, app.scene.geometry_stamp().0)
            || app.scene.geometry_stamp().1 != previous_revision
    );
    assert!(!app.practice.as_ref().unwrap().view.dirty());
    assert_eq!(
        app.scene.component_transform(rebound).unwrap().offset(),
        [0.0, 50.0]
    );
    assert_eq!(app.ime_cursor_area([960, 720]), Some([24, 175, 906, 40]));
    app.publish_control_pose([960, 720]).unwrap();
    assert_eq!(app.ime_cursor_area([960, 720]), Some([24, 200, 906, 40]));
    let hidden = UiTransform::new([0.0, 50.0], [1.0, 1.0], 0.0).unwrap();
    request(
        &mut app,
        70,
        ComponentMotion::new(hidden, hidden, Duration::ZERO, Easing::Linear),
    );
    assert_eq!(app.ime_cursor_area([960, 720]), Some([24, 200, 906, 40]));
    app.publish_control_pose([960, 720]).unwrap();
    assert_eq!(app.ime_cursor_area([960, 720]), None);
    assert_eq!(app.hit_motion((30.0, 210.0), [960, 720]), None);
}

#[test]
fn desktop_practice_motion_ime_parent_viewport_clips_and_no_missing_key_fallback() {
    let mut app = practice_fixture();
    let pose = UiTransform::new([900.25, 0.5], [1.0, 1.0], 1.0).unwrap();
    let key = request(
        &mut app,
        70,
        ComponentMotion::new(pose, pose, Duration::ZERO, Easing::Linear),
    );
    app.publish_control_pose([960, 720]).unwrap();
    assert_eq!(app.ime_cursor_area([960, 720]), Some([924, 150, 6, 41]));
    assert_eq!(
        app.hit_motion((925.0, 160.0), [960, 720]),
        Some(ControlId(70))
    );
    // The staged key is explicit: a fresh snapshot without its live binding
    // must refuse rather than publishing the raw, unanimated editor rectangle.
    app.scene.dispose_components(key.screen);
    app.publish_control_pose([960, 720]).unwrap();
    assert_eq!(app.ime_cursor_area([960, 720]), None);
}

#[test]
fn desktop_practice_motion_refusals_zero_extent_and_removed_owner_are_atomic() {
    let mut app = practice_fixture();
    let key = request(&mut app, 71, motion([100.0, 0.0]));
    app.tick_control_motion(Duration::from_nanos(25), [960, 720])
        .unwrap();
    app.publish_control_pose([960, 720]).unwrap();
    let id = app.scene.component_id(key).unwrap();
    let pose = app.scene.component_transform(id);
    for (screen, control, now) in [
        (ScreenInstanceId(u64::MAX), ControlId(71), 25),
        (key.screen, ControlId(u64::MAX), 25),
        (key.screen, ControlId(71), 24),
    ] {
        assert!(app
            .request_control_motion(
                screen,
                control,
                motion([500.0, 0.0]),
                Duration::from_nanos(now)
            )
            .is_err());
    }
    assert_eq!(app.scene.component_transform(id), pose);
    assert_eq!(app.ui_motion.time, Duration::from_nanos(25));
    assert!(!app
        .tick_control_motion(Duration::from_nanos(25), [0, 720])
        .unwrap());
    assert!(!app
        .tick_control_motion(Duration::from_nanos(1000), [0, 0])
        .unwrap());
    app.tick_control_motion(Duration::from_nanos(1000), [960, 720])
        .unwrap();
    assert_eq!(app.scene.component_transform(id), pose);
    app.tick_control_motion(Duration::from_nanos(1025), [960, 720])
        .unwrap();
    assert_eq!(
        app.scene.component_transform(id).unwrap().offset(),
        [50.0, 0.0]
    );
    app.back();
    app.draw().unwrap();
    assert_eq!(app.navigator.route(), ScreenRoute::Settings);
    assert!(app.scene.component_id(key).is_none());
    assert!(app
        .ui_motion
        .owners
        .iter()
        .all(|owner| owner.screen != key.screen));
    assert!(app
        .request_control_motion(
            key.screen,
            ControlId(71),
            motion([1.0, 0.0]),
            Duration::from_nanos(1025)
        )
        .is_err());
    app.open_practice();
    app.draw().unwrap();
    assert_ne!(app.navigator.active_id(), Some(key.screen));
    app.request_close();
    assert!(app.ui_motion.disposed);
    assert!(app.ui_motion.owners.is_empty());
    assert!(app
        .tick_control_motion(Duration::from_nanos(1050), [960, 720])
        .is_err());
}

#[test]
fn desktop_practice_motion_back_resumes_retained_selection_ancestor() {
    let mut app = tests::lifecycle_fixture();
    app.draw().unwrap();
    let key = request(&mut app, 5, motion([-200.0, 0.0]));
    app.tick_control_motion(Duration::from_nanos(25), [960, 720])
        .unwrap();
    let quarter = app
        .scene
        .component_transform(app.scene.component_id(key).unwrap());
    app.open_settings();
    app.open_practice();
    app.draw().unwrap();
    request(&mut app, 71, motion([100.0, 0.0]));
    app.tick_control_motion(Duration::from_nanos(1000), [960, 720])
        .unwrap();
    app.back();
    app.back();
    app.draw().unwrap();
    assert_eq!(app.navigator.active_id(), Some(key.screen));
    app.tick_control_motion(Duration::from_nanos(1000), [960, 720])
        .unwrap();
    assert_eq!(
        app.scene
            .component_transform(app.scene.component_id(key).unwrap()),
        quarter
    );
    app.tick_control_motion(Duration::from_nanos(1025), [960, 720])
        .unwrap();
    assert_eq!(
        app.scene
            .component_transform(app.scene.component_id(key).unwrap())
            .unwrap()
            .offset(),
        [-100.0, 0.0]
    );
}

#[test]
fn desktop_practice_motion_preserves_tab_focus_preedit_and_all_four_actions() {
    let mut app = practice_fixture();
    request(&mut app, 71, motion([100.0, 0.0]));
    app.practice_key(KeyCode::Tab, false);
    assert!(app.practice.as_ref().unwrap().end_focused);
    app.practice_key(KeyCode::Tab, true);
    assert!(app.practice.as_ref().unwrap().end_focused);
    app.activate(ControlId(70));
    assert!(!app.practice.as_ref().unwrap().end_focused);
    app.ime_event(Ime::Enabled);
    app.ime_event(Ime::Preedit("1".into(), Some((0, 1))));
    assert!(app.ime.preview.is_some());
    assert_eq!(app.practice.as_ref().unwrap().editor.value(), "0:00");
    app.ime_event(Ime::Disabled);
    app.practice.as_mut().unwrap().editor = LineEditor::new("20:00:00", 64).unwrap();
    app.practice.as_mut().unwrap().end_editor = LineEditor::new("21:00:00", 64).unwrap();
    app.activate(ControlId(76));
    assert_eq!(app.practice.as_ref().unwrap().editor.value(), "20:00:00");
    assert_eq!(app.practice.as_ref().unwrap().end_editor.value(), "");
    app.activate(ControlId(73));
    assert_eq!(app.practice.as_ref().unwrap().editor.value(), "0:00");
    assert!(!app.practice.as_ref().unwrap().end_focused);
    app.practice.as_mut().unwrap().editor = LineEditor::new("1.000000001", 64).unwrap();
    app.activate(ControlId(71));
    assert_eq!(app.navigator.route(), ScreenRoute::Settings);
    assert_eq!(
        PracticeStart::from_settings(&app.settings.as_ref().unwrap().values)
            .unwrap()
            .nanoseconds(),
        1_000_000_001
    );
    let committed = app.settings.as_ref().unwrap().values.native_args();
    app.open_practice();
    app.draw().unwrap();
    request(&mut app, 72, motion([100.0, 0.0]));
    app.practice.as_mut().unwrap().editor = LineEditor::new("9", 64).unwrap();
    app.activate(ControlId(72));
    assert_eq!(app.navigator.route(), ScreenRoute::Settings);
    assert_eq!(
        app.settings.as_ref().unwrap().values.native_args(),
        committed
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires BEATKERNEL_TEST_NATIVE_UI_WINDOW=1 and an actual X11 display/GPU surface"]
#[allow(deprecated)]
fn desktop_practice_motion_actual_x11_window_presents_and_hits_accepted_pose() {
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
                    .with_title("BeatKernel actual Practice component motion fixture")
                    .with_inner_size(LogicalSize::new(960.0, 720.0)),
            )
            .unwrap(),
    );
    let mut app = practice_fixture();
    let instance = graphics::instance(app.active_backend).unwrap();
    let surface = instance.create_surface(window.clone()).unwrap();
    let mut renderer =
        pollster::block_on(Renderer::new(surface, &instance, app.options.presentation)).unwrap();
    let size = window.inner_size();
    renderer.resize(size.width, size.height).unwrap();
    let description = renderer.description();
    app.window = Some(window.clone());
    app.renderer = Some(renderer);
    app.instance = Some(instance);
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.renderer.as_ref().unwrap().presentation_count() == 0 {
        assert!(
            Instant::now() < deadline,
            "initial Practice frame not presented"
        );
        app.draw().unwrap();
        std::thread::sleep(Duration::from_millis(16));
    }
    let extent = [size.width, size.height];
    let physical = |x: f64, y: f64| {
        (
            x * f64::from(size.width) / 960.0,
            y * f64::from(size.height) / 720.0,
        )
    };
    assert_eq!(
        app.hit_motion(physical(30.0, 390.0), extent),
        Some(ControlId(71))
    );
    let (reply, received) = std::sync::mpsc::sync_channel(1);
    app.handle_ui_command(DesktopUiCommand::InspectScreen { reply });
    let screen = received.recv().unwrap().unwrap();
    let destination = UiTransform::new([100.0, 0.0], [1.0, 1.0], 1.0).unwrap();
    let (reply, received) = std::sync::mpsc::sync_channel(1);
    app.handle_ui_command(DesktopUiCommand::RequestMotion {
        screen,
        control: ControlId(71),
        motion: ComponentMotion::new(
            UiTransform::default(),
            destination,
            Duration::from_millis(400),
            Easing::Linear,
        ),
        reply,
    });
    received.recv().unwrap().unwrap();
    let key = UiComponentKey {
        screen,
        node: app.control_node(ControlId(71)).unwrap(),
    };
    let geometry = Arc::clone(app.scene.geometry_stamp().0);
    let revision = app.scene.geometry_stamp().1;
    let first = app.renderer.as_ref().unwrap().presentation_count();
    let mut samples = 0;
    loop {
        assert!(
            Instant::now() < deadline,
            "Practice motion did not present completion"
        );
        let before = app.renderer.as_ref().unwrap().presentation_count();
        app.draw().unwrap();
        let after = app.renderer.as_ref().unwrap().presentation_count();
        if after > before {
            samples += 1;
            assert_eq!(app.ui_motion.presented_screen, Some(screen));
        }
        assert!(Arc::ptr_eq(&geometry, app.scene.geometry_stamp().0));
        assert_eq!(app.scene.geometry_stamp().1, revision);
        if !app.control_motion_active() && after > before {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    let last = app.renderer.as_ref().unwrap().presentation_count();
    assert!(samples >= 2);
    assert!(last >= first + 2);
    assert_eq!(
        app.scene
            .component_transform(app.scene.component_id(key).unwrap()),
        Some(destination)
    );
    assert_eq!(app.hit_motion(physical(30.0, 390.0), extent), None);
    assert_eq!(
        app.hit_motion(physical(210.0, 390.0), extent),
        Some(ControlId(71))
    );
    eprintln!("actual Practice native motion: {description}; extent={extent:?}; frames={samples}; counters={first}..{last}; geometry_epoch={revision}; old_hit=None; moved_hit=71");
    if let Ok(hold) = std::env::var("BEATKERNEL_TEST_NATIVE_UI_HOLD_MS") {
        std::thread::sleep(Duration::from_millis(
            hold.parse::<u64>().unwrap().min(5000),
        ));
    }
    app.request_close();
    assert!(app.ui_motion.disposed);
    assert!(app.ui_motion.owners.is_empty());
    assert!(app.scene.component_id(key).is_none());
    drop(app);
    drop(window);
    drop(event_loop);
}
