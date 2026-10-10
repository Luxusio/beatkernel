//! Actual remaining browser menu owners, submitted input poses and surface changes.
use crate::{
    browser_menu::{BrowserMenu, BrowserMenuPresentation, MenuToken},
    scene::{Scene, UiComponentKey, UiTransform},
    screen_lifecycle::{ScreenInstanceId, ScreenRoute},
    ui::{
        interaction::ControlId,
        motion::{ComponentMotion, Easing},
    },
};
use std::{sync::Arc, time::Duration};

const EXTENT: [u32; 2] = [960, 720];

fn token(menu: &BrowserMenu) -> MenuToken {
    menu.snapshot().token
}
fn settings_fields() -> Vec<String> {
    [
        "5", "5", "0", "output", "0", "48000", "8", "4", "8", "256", "8", "0", "",
    ]
    .map(str::to_owned)
    .to_vec()
}
fn local_fields() -> Vec<String> {
    [
        "1",
        "1",
        "2",
        "1",
        "",
        "2",
        "",
        "2",
        "1",
        "hid",
        "FIRST HID",
        "first source",
        "1",
        "2",
        "keyboard",
        "SECOND KEYBOARD",
        "second source",
        "1",
    ]
    .map(str::to_owned)
    .to_vec()
}
fn menu_for(route: ScreenRoute) -> BrowserMenu {
    let mut menu = BrowserMenu::new(603).unwrap();
    menu.navigate_with_fields(token(&menu), ScreenRoute::Settings, settings_fields())
        .unwrap();
    if route != ScreenRoute::Settings {
        let fields = match route {
            ScreenRoute::Records => vec!["records/first.bkr".into()],
            ScreenRoute::Players | ScreenRoute::Devices { .. } => local_fields(),
            _ => panic!("remaining menu fixture route"),
        };
        menu.navigate_with_fields(token(&menu), route, fields)
            .unwrap();
    }
    menu
}
fn compose(presentation: &mut BrowserMenuPresentation) -> Scene {
    let mut scene = Scene::new(960, 720);
    presentation.compose(&mut scene).unwrap();
    scene.status().unwrap();
    presentation.publish_presented(&scene, EXTENT).unwrap();
    scene
}
fn motion(offset: f32) -> ComponentMotion {
    ComponentMotion::new(
        UiTransform::default(),
        UiTransform::new([offset, 0.0], [1.0, 1.0], 1.0).unwrap(),
        Duration::from_nanos(100),
        Easing::Linear,
    )
}
fn key(presentation: &BrowserMenuPresentation, control: u64) -> UiComponentKey {
    UiComponentKey {
        screen: presentation.model.token.screen,
        node: presentation.control_node(ControlId(control)).unwrap(),
    }
}

#[test]
fn four_actual_menu_owners_move_only_uniforms_and_publish_hits_after_accepted_frame() {
    for (route, control, new_point, old_point) in [
        (ScreenRoute::Settings, 11, (180.0, 630.0), (370.0, 630.0)),
        (ScreenRoute::Records, 55, (740.0, 630.0), (920.0, 630.0)),
        (ScreenRoute::Players, 31, (160.0, 630.0), (304.0, 630.0)),
        (
            ScreenRoute::Devices { players: false },
            21,
            (180.0, 630.0),
            (370.0, 630.0),
        ),
    ] {
        let menu = menu_for(route);
        let mut presentation = BrowserMenuPresentation::new(menu.snapshot()).unwrap();
        let mut scene = compose(&mut presentation);
        assert_eq!(presentation.hit(old_point, EXTENT), control);
        assert_ne!(presentation.hit(new_point, EXTENT), control);
        let target = key(&presentation, control);
        presentation
            .request_motion(
                token(&menu),
                ControlId(control),
                motion(-100.0),
                Duration::ZERO,
                &mut scene,
            )
            .unwrap();
        let binding = scene.component_id(target).unwrap();
        let identity = Arc::clone(scene.geometry_stamp().0);
        let revision = scene.geometry_stamp().1;
        let pointer = scene.rectangles().as_ptr();
        let business = presentation.model.clone();
        assert!(presentation
            .tick_motion(Duration::from_nanos(50), EXTENT, &mut scene)
            .unwrap());
        assert_eq!(
            scene.component_transform(binding).unwrap().offset(),
            [-50.0, 0.0]
        );
        assert_eq!(presentation.model, business);
        assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
        assert_eq!(scene.geometry_stamp().1, revision);
        assert_eq!(scene.rectangles().as_ptr(), pointer);
        assert_eq!(presentation.hit(old_point, EXTENT), control);
        assert_ne!(presentation.hit(new_point, EXTENT), control);
        assert!(presentation.publish_presented(&scene, [0, 720]).is_err());
        assert_eq!(presentation.hit(old_point, EXTENT), control);
        presentation.publish_presented(&scene, EXTENT).unwrap();
        assert_eq!(presentation.hit(new_point, EXTENT), control);
        assert_ne!(presentation.hit(old_point, EXTENT), control);
        presentation.compose(&mut scene).unwrap();
        assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
        assert_eq!(scene.geometry_stamp().1, revision);
        assert_eq!(scene.component_id(target), Some(binding));
    }
}

#[test]
fn remaining_owners_reject_foreign_stale_absent_pending_and_regressed_requests_atomically() {
    for (route, control) in [
        (ScreenRoute::Settings, 11),
        (ScreenRoute::Records, 55),
        (ScreenRoute::Players, 31),
        (ScreenRoute::Devices { players: false }, 21),
    ] {
        let menu = menu_for(route);
        let current = token(&menu);
        let mut presentation = BrowserMenuPresentation::new(menu.snapshot()).unwrap();
        let mut scene = compose(&mut presentation);
        presentation
            .request_motion(
                current,
                ControlId(control),
                motion(-100.0),
                Duration::ZERO,
                &mut scene,
            )
            .unwrap();
        presentation
            .tick_motion(Duration::from_nanos(25), EXTENT, &mut scene)
            .unwrap();
        let binding = scene.component_id(key(&presentation, control)).unwrap();
        let pose = scene.component_transform(binding);
        let identity = Arc::clone(scene.geometry_stamp().0);
        let revision = scene.geometry_stamp().1;
        for bad in [
            MenuToken {
                generation: current.generation + 1,
                ..current
            },
            MenuToken {
                screen: ScreenInstanceId(9999),
                ..current
            },
            MenuToken {
                revision: current.revision + 1,
                ..current
            },
        ] {
            assert!(presentation
                .request_motion(
                    bad,
                    ControlId(control),
                    motion(10.0),
                    Duration::from_nanos(25),
                    &mut scene
                )
                .is_err());
        }
        assert!(presentation
            .request_motion(
                current,
                ControlId(u64::MAX),
                motion(10.0),
                Duration::from_nanos(25),
                &mut scene
            )
            .is_err());
        assert!(presentation
            .request_motion(
                current,
                ControlId(control),
                motion(10.0),
                Duration::from_nanos(24),
                &mut scene
            )
            .is_err());
        assert!(presentation
            .tick_motion(Duration::from_nanos(24), EXTENT, &mut scene)
            .is_err());
        assert_eq!(scene.component_transform(binding), pose);
        assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
        assert_eq!(scene.geometry_stamp().1, revision);
        let mut pending = menu.snapshot();
        pending.pending = true;
        presentation.apply(pending).unwrap();
        assert!(presentation
            .request_motion(
                current,
                ControlId(control),
                motion(10.0),
                Duration::from_nanos(25),
                &mut scene
            )
            .is_err());
        assert_eq!(scene.component_transform(binding), pose);
    }
}

#[test]
fn settings_editor_repaint_retains_node_pose_and_requires_new_submission() {
    let mut menu = menu_for(ScreenRoute::Settings);
    let mut presentation = BrowserMenuPresentation::new(menu.snapshot()).unwrap();
    let mut scene = compose(&mut presentation);
    let target = key(&presentation, 1000);
    presentation
        .request_motion(
            token(&menu),
            ControlId(1000),
            motion(101.0),
            Duration::ZERO,
            &mut scene,
        )
        .unwrap();
    presentation
        .tick_motion(Duration::from_nanos(50), EXTENT, &mut scene)
        .unwrap();
    presentation.publish_presented(&scene, EXTENT).unwrap();
    assert_eq!(presentation.hit((300.0, 130.0), EXTENT), 0);
    assert_eq!(presentation.hit((350.0, 130.0), EXTENT), 1000);
    let pose = scene
        .component_transform(scene.component_id(target).unwrap())
        .unwrap();
    menu.edit(token(&menu), 0, "UPDATED EDITOR VALUE".into())
        .unwrap();
    presentation.apply(menu.snapshot()).unwrap();
    assert_eq!(presentation.hit((350.0, 130.0), EXTENT), 0);
    presentation.compose(&mut scene).unwrap();
    assert_eq!(key(&presentation, 1000), target);
    assert_eq!(
        scene.component_transform(scene.component_id(target).unwrap()),
        Some(pose)
    );
    assert_eq!(presentation.model.fields[0], "UPDATED EDITOR VALUE");
    presentation.publish_presented(&scene, EXTENT).unwrap();
    assert_eq!(presentation.hit((350.0, 130.0), EXTENT), 1000);
    presentation
        .tick_motion(Duration::from_nanos(75), EXTENT, &mut scene)
        .unwrap();
    assert_eq!(presentation.hit((350.0, 130.0), EXTENT), 1000);
    presentation.publish_presented(&scene, EXTENT).unwrap();
    assert_eq!(presentation.hit((350.0, 130.0), EXTENT), 0);
}

#[test]
fn actual_players_parent_and_devices_child_back_reopen_retire_only_child_tracks() {
    let mut menu = menu_for(ScreenRoute::Players);
    let parent = token(&menu);
    let mut presentation = BrowserMenuPresentation::new(menu.snapshot()).unwrap();
    let mut scene = compose(&mut presentation);
    let parent_key = key(&presentation, 31);
    presentation
        .request_motion(
            parent,
            ControlId(31),
            motion(-100.0),
            Duration::ZERO,
            &mut scene,
        )
        .unwrap();
    presentation
        .tick_motion(Duration::from_nanos(25), EXTENT, &mut scene)
        .unwrap();
    menu.navigate_with_fields(
        token(&menu),
        ScreenRoute::Devices { players: true },
        local_fields(),
    )
    .unwrap();
    let child = token(&menu);
    presentation.apply(menu.snapshot()).unwrap();
    presentation.compose(&mut scene).unwrap();
    presentation.publish_presented(&scene, EXTENT).unwrap();
    let child_key = key(&presentation, 21);
    presentation
        .request_motion(
            child,
            ControlId(21),
            motion(-100.0),
            Duration::from_nanos(25),
            &mut scene,
        )
        .unwrap();
    presentation
        .tick_motion(Duration::from_nanos(50), EXTENT, &mut scene)
        .unwrap();
    menu.back(token(&menu)).unwrap();
    presentation.apply(menu.snapshot()).unwrap();
    presentation.compose(&mut scene).unwrap();
    presentation
        .tick_motion(Duration::from_nanos(1000), EXTENT, &mut scene)
        .unwrap();
    assert!(scene.component_id(child_key).is_none());
    assert_eq!(
        scene
            .component_transform(scene.component_id(parent_key).unwrap())
            .unwrap()
            .offset(),
        [-25.0, 0.0]
    );
    assert!(presentation
        .request_motion(
            child,
            ControlId(21),
            motion(1.0),
            Duration::from_nanos(1000),
            &mut scene
        )
        .is_err());
    presentation
        .tick_motion(Duration::from_nanos(1025), EXTENT, &mut scene)
        .unwrap();
    assert_eq!(
        scene
            .component_transform(scene.component_id(parent_key).unwrap())
            .unwrap()
            .offset(),
        [-50.0, 0.0]
    );
    menu.navigate_with_fields(
        token(&menu),
        ScreenRoute::Devices { players: true },
        local_fields(),
    )
    .unwrap();
    assert_ne!(token(&menu).screen, child.screen);
    presentation.apply(menu.snapshot()).unwrap();
    presentation.compose(&mut scene).unwrap();
    presentation.publish_presented(&scene, EXTENT).unwrap();
    assert_eq!(presentation.hit((370.0, 630.0), EXTENT), 21);
    assert!(scene.component_id(child_key).is_none());
}

#[test]
fn remaining_menu_suspension_zero_extent_resume_and_dispose_obey_presentation_time() {
    let menu = menu_for(ScreenRoute::Settings);
    let current = token(&menu);
    let mut presentation = BrowserMenuPresentation::new(menu.snapshot()).unwrap();
    let mut scene = compose(&mut presentation);
    let target = key(&presentation, 11);
    presentation
        .request_motion(
            current,
            ControlId(11),
            motion(-100.0),
            Duration::ZERO,
            &mut scene,
        )
        .unwrap();
    presentation
        .tick_motion(Duration::from_nanos(25), EXTENT, &mut scene)
        .unwrap();
    presentation.publish_presented(&scene, EXTENT).unwrap();
    presentation
        .suspend_motion(Duration::from_nanos(25))
        .unwrap();
    assert!(!presentation.motion_active());
    assert_eq!(presentation.hit((370.0, 630.0), EXTENT), 0);
    assert!(presentation
        .request_motion(
            current,
            ControlId(11),
            motion(1.0),
            Duration::from_nanos(25),
            &mut scene
        )
        .is_err());
    presentation
        .tick_motion(Duration::from_nanos(1000), EXTENT, &mut scene)
        .unwrap();
    presentation
        .resume_motion(Duration::from_nanos(1000))
        .unwrap();
    presentation
        .tick_motion(Duration::from_nanos(1000), EXTENT, &mut scene)
        .unwrap();
    let binding = scene.component_id(target).unwrap();
    assert_eq!(
        scene.component_transform(binding).unwrap().offset(),
        [-25.0, 0.0]
    );
    presentation
        .tick_motion(Duration::from_nanos(1025), EXTENT, &mut scene)
        .unwrap();
    assert_eq!(
        scene.component_transform(binding).unwrap().offset(),
        [-50.0, 0.0]
    );
    presentation
        .tick_motion(Duration::from_nanos(1025), [0, 720], &mut scene)
        .unwrap();
    presentation
        .tick_motion(Duration::from_nanos(2000), [0, 0], &mut scene)
        .unwrap();
    presentation
        .tick_motion(Duration::from_nanos(2000), EXTENT, &mut scene)
        .unwrap();
    assert_eq!(
        scene.component_transform(binding).unwrap().offset(),
        [-50.0, 0.0]
    );
    presentation
        .tick_motion(Duration::from_nanos(2025), EXTENT, &mut scene)
        .unwrap();
    assert_eq!(
        scene.component_transform(binding).unwrap().offset(),
        [-75.0, 0.0]
    );
    presentation.dispose_motion();
    assert!(!presentation.motion_active());
    assert_eq!(presentation.hit((180.0, 630.0), EXTENT), 0);
    assert!(presentation
        .request_motion(
            current,
            ControlId(11),
            motion(1.0),
            Duration::from_nanos(2025),
            &mut scene
        )
        .is_err());
}

#[test]
fn records_surface_and_grade_page_changes_prune_absent_mounted_targets() {
    let record = crate::record_model::FrozenRecordPreview::from_record(
        &crate::record_model::visual_fixtures::record(),
    )
    .unwrap();
    let mut menu = menu_for(ScreenRoute::Records);
    menu.set_fields(token(&menu), vec![record.path.to_str().unwrap().into()])
        .unwrap();
    let current = token(&menu);
    let mut presentation = BrowserMenuPresentation::new(menu.snapshot()).unwrap();
    presentation.set_record_preview(current, record).unwrap();
    let mut scene = compose(&mut presentation);
    let row_key = key(&presentation, 50000);
    let chooser_details_key = key(&presentation, 66);
    presentation
        .request_motion(
            current,
            ControlId(50000),
            motion(10.0),
            Duration::ZERO,
            &mut scene,
        )
        .unwrap();
    presentation
        .request_motion(
            current,
            ControlId(66),
            motion(10.0),
            Duration::ZERO,
            &mut scene,
        )
        .unwrap();
    presentation
        .tick_motion(Duration::from_nanos(25), EXTENT, &mut scene)
        .unwrap();
    presentation.set_record_details(true, 0).unwrap();
    presentation.compose(&mut scene).unwrap();
    assert!(scene.component_id(row_key).is_none());
    assert!(scene.component_id(chooser_details_key).is_none());
    assert_ne!(key(&presentation, 66), chooser_details_key);
    assert!(!presentation.motion_active());
    assert!(presentation
        .request_motion(
            current,
            ControlId(50000),
            motion(10.0),
            Duration::from_nanos(25),
            &mut scene
        )
        .is_err());
    assert!(presentation.control_node(ControlId(67)).is_err());
    let next_key = key(&presentation, 68);
    presentation
        .request_motion(
            current,
            ControlId(68),
            motion(10.0),
            Duration::from_nanos(25),
            &mut scene,
        )
        .unwrap();
    presentation
        .tick_motion(Duration::from_nanos(50), EXTENT, &mut scene)
        .unwrap();
    presentation.set_record_details(true, 1).unwrap();
    presentation.compose(&mut scene).unwrap();
    assert!(scene.component_id(next_key).is_none());
    assert!(presentation.control_node(ControlId(68)).is_err());
    assert!(presentation.control_node(ControlId(67)).is_ok());
    assert!(!presentation.motion_active());
    assert!(presentation
        .request_motion(
            current,
            ControlId(68),
            motion(10.0),
            Duration::from_nanos(50),
            &mut scene
        )
        .is_err());
    presentation.publish_presented(&scene, EXTENT).unwrap();
    assert_eq!(presentation.hit((450.0, 580.0), EXTENT), 67);
    presentation.set_record_details(false, 0).unwrap();
    presentation.compose(&mut scene).unwrap();
    assert_eq!(key(&presentation, 66), chooser_details_key);
    assert!(scene.component_id(next_key).is_none());
    assert!(presentation.control_node(ControlId(68)).is_err());
    presentation.publish_presented(&scene, EXTENT).unwrap();
    assert_eq!(presentation.hit((30.0, 175.0), EXTENT), 50000);
}
