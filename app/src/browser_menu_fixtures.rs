//! Actual portable browser menu owner: lifetime, drafts and hostile correlation.
use crate::browser_menu::{
    decode_snapshot, encode_snapshot, BrowserMenu, MenuEffect, MenuToken, MAX_MENU_BYTES,
    MAX_MENU_FIELDS,
};
use crate::screen_lifecycle::{ScreenInstanceId, ScreenRoute};

fn token(menu: &BrowserMenu) -> MenuToken {
    menu.snapshot().token
}

fn paged_browser_local_fields(members: usize, sources: usize) -> Vec<String> {
    let mut fields = vec!["1".into(), "1".into(), members.to_string()];
    for index in 0..members {
        fields.push((index + 1).to_string());
        fields.push(String::new());
    }
    fields.push(sources.to_string());
    for index in 0..sources {
        fields.extend([
            (index + 1).to_string(),
            "hid".into(),
            format!("ACQUIRED SOURCE {}", index + 1),
            format!("Original acquired source {}", index + 1),
            "1".into(),
        ]);
    }
    fields
}

#[test]
fn actual_players_roster_shrink_normalizes_published_selection_and_first_visible_row() {
    use crate::browser_menu::BrowserMenuPresentation;
    let mut menu = BrowserMenu::new(107).unwrap();
    menu.navigate_with_fields(
        token(&menu),
        ScreenRoute::Settings,
        vec!["accepted settings".into()],
    )
    .unwrap();
    menu.navigate_with_fields(
        token(&menu),
        ScreenRoute::Players,
        paged_browser_local_fields(21, 0),
    )
    .unwrap();
    menu.select(token(&menu), 20).unwrap();
    let old = menu.snapshot();
    let mut presentation = BrowserMenuPresentation::new(old.clone()).unwrap();
    render_menu(&mut presentation);
    assert_eq!(presentation.hit((30.0, 125.0), [960, 720]), 20020);
    menu.navigate_with_fields(
        old.token,
        ScreenRoute::Players,
        paged_browser_local_fields(10, 0),
    )
    .unwrap();
    let admitted = menu.snapshot();
    assert_eq!(admitted.token.screen, old.token.screen);
    assert_eq!(admitted.selected, 9);
    assert!(admitted.token.revision > old.token.revision);
    assert!(menu.select(admitted.token, 10).is_err());
    assert_eq!(menu.snapshot(), admitted);
    presentation.apply(admitted.clone()).unwrap();
    let scene = render_menu(&mut presentation);
    assert_eq!(presentation.hit((30.0, 125.0), [960, 720]), 20000);
    assert_eq!(presentation.hit((30.0, 476.0), [960, 720]), 20009);
    menu_label(&scene, "10 PLAYERS  ROWS 1-10 OF 10");
    assert_eq!(
        menu_packet(&render_menu(&mut presentation)),
        menu_packet(&scene)
    );
    assert!(menu
        .navigate_with_fields(
            old.token,
            ScreenRoute::Players,
            paged_browser_local_fields(1, 0)
        )
        .is_err());
    assert_eq!(menu.snapshot(), admitted);
    assert!(menu
        .navigate_with_fields(
            admitted.token,
            ScreenRoute::Players,
            paged_browser_local_fields(0, 0)
        )
        .is_err());
    assert_eq!(
        menu.snapshot(),
        admitted,
        "empty Players roster cannot publish an invalid selection"
    );
    let mut malformed = paged_browser_local_fields(10, 1);
    *malformed.last_mut().unwrap() = "2".into();
    assert!(menu.set_fields(admitted.token, malformed.clone()).is_err());
    assert_eq!(menu.snapshot(), admitted);
    assert!(menu
        .navigate_with_fields(admitted.token, ScreenRoute::Players, malformed)
        .is_err());
    assert_eq!(menu.snapshot(), admitted);
    assert_eq!(
        menu_packet(&render_menu(&mut presentation)),
        menu_packet(&scene)
    );
}

#[test]
fn actual_device_inventory_shrink_and_empty_inventory_normalize_selection_before_compose() {
    use crate::browser_menu::BrowserMenuPresentation;
    let mut menu = BrowserMenu::new(109).unwrap();
    menu.navigate_with_fields(
        token(&menu),
        ScreenRoute::Settings,
        vec!["accepted settings".into()],
    )
    .unwrap();
    let route = ScreenRoute::Devices { players: false };
    menu.navigate_with_fields(token(&menu), route, paged_browser_local_fields(2, 20))
        .unwrap();
    menu.select(token(&menu), 15).unwrap();
    let old = menu.snapshot();
    let mut presentation = BrowserMenuPresentation::new(old.clone()).unwrap();
    render_menu(&mut presentation);
    assert_eq!(presentation.hit((30.0, 125.0), [960, 720]), 10010);
    menu.set_fields(old.token, paged_browser_local_fields(2, 5))
        .unwrap();
    let admitted = menu.snapshot();
    assert_eq!(admitted.token.screen, old.token.screen);
    assert_eq!(admitted.selected, 4);
    assert!(menu.select(admitted.token, 5).is_err());
    assert_eq!(menu.snapshot(), admitted);
    presentation.apply(admitted.clone()).unwrap();
    let scene = render_menu(&mut presentation);
    assert_eq!(presentation.hit((30.0, 125.0), [960, 720]), 10000);
    assert_eq!(presentation.hit((30.0, 281.0), [960, 720]), 10004);
    assert_eq!(
        menu_packet(&render_menu(&mut presentation)),
        menu_packet(&scene)
    );
    assert!(menu
        .set_fields(old.token, paged_browser_local_fields(2, 0))
        .is_err());
    assert_eq!(menu.snapshot(), admitted);
    let mut malformed = paged_browser_local_fields(2, 5);
    malformed[9] = "foreign source kind".into();
    assert!(menu.set_fields(admitted.token, malformed.clone()).is_err());
    assert_eq!(menu.snapshot(), admitted);
    assert!(menu
        .navigate_with_fields(admitted.token, route, malformed)
        .is_err());
    assert_eq!(menu.snapshot(), admitted);
    menu.navigate_with_fields(admitted.token, route, paged_browser_local_fields(2, 0))
        .unwrap();
    let empty = menu.snapshot();
    assert_eq!(empty.selected, 0);
    assert_eq!(empty.token.screen, admitted.token.screen);
    presentation.apply(empty.clone()).unwrap();
    let empty_scene = render_menu(&mut presentation);
    assert_eq!(presentation.hit((30.0, 125.0), [960, 720]), 0);
    assert_eq!(
        menu_packet(&render_menu(&mut presentation)),
        menu_packet(&empty_scene)
    );
    let mut invalid = paged_browser_local_fields(2, 1);
    invalid.pop();
    assert!(menu
        .navigate_with_fields(empty.token, route, invalid)
        .is_err());
    assert_eq!(menu.snapshot(), empty);
}

fn requested_menu_motion(offset: f32) -> crate::ui::motion::ComponentMotion {
    crate::ui::motion::ComponentMotion::new(
        crate::scene::UiTransform::default(),
        crate::scene::UiTransform::new([offset, 0.0], [1.0, 1.0], 1.0).unwrap(),
        std::time::Duration::from_nanos(100),
        crate::ui::motion::Easing::Linear,
    )
}

#[test]
fn actual_selection_parent_back_resume_zero_extent_and_removed_display_disposal() {
    use crate::{browser_menu::BrowserMenuPresentation, ui::interaction::ControlId};
    use std::time::Duration;
    let mut menu = BrowserMenu::new(103).unwrap();
    let mut presentation = BrowserMenuPresentation::new(menu.snapshot()).unwrap();
    let mut scene = render_menu(&mut presentation);
    let parent = token(&menu);
    let node = presentation.control_node(ControlId(5)).unwrap();
    let key = crate::scene::UiComponentKey {
        screen: parent.screen,
        node,
    };
    presentation
        .request_motion(
            parent,
            ControlId(5),
            requested_menu_motion(-200.0),
            Duration::ZERO,
            &mut scene,
        )
        .unwrap();
    presentation
        .tick_motion(Duration::from_nanos(25), [960, 720], &mut scene)
        .unwrap();
    presentation.publish_presented(&scene, [960, 720]).unwrap();
    let quarter = scene
        .component_transform(scene.component_id(key).unwrap())
        .unwrap();
    let settings_fields = [
        "5", "5", "0", "output", "0", "48000", "8", "4", "8", "256", "8", "0", "",
    ]
    .map(str::to_owned)
    .to_vec();
    menu.navigate_with_fields(parent, ScreenRoute::Settings, settings_fields.clone())
        .unwrap();
    presentation.apply(menu.snapshot()).unwrap();
    presentation.compose(&mut scene).unwrap();
    assert!(
        scene.component_id(key).is_none(),
        "inactive parent releases renderer slots"
    );
    presentation
        .tick_motion(Duration::from_nanos(1000), [960, 720], &mut scene)
        .unwrap();
    presentation.publish_presented(&scene, [960, 720]).unwrap();
    menu.back(token(&menu)).unwrap();
    presentation.apply(menu.snapshot()).unwrap();
    presentation.compose(&mut scene).unwrap();
    presentation
        .tick_motion(Duration::from_nanos(1000), [960, 720], &mut scene)
        .unwrap();
    let rebound = scene.component_id(key).unwrap();
    assert_eq!(scene.component_transform(rebound), Some(quarter));
    presentation
        .tick_motion(Duration::from_nanos(1025), [960, 720], &mut scene)
        .unwrap();
    assert_eq!(
        scene.component_transform(rebound).unwrap().offset(),
        [-100.0, 0.0]
    );
    presentation.publish_presented(&scene, [960, 720]).unwrap();
    assert!(!presentation
        .tick_motion(Duration::from_nanos(1025), [0, 720], &mut scene)
        .unwrap());
    assert!(!presentation
        .tick_motion(Duration::from_nanos(5000), [0, 0], &mut scene)
        .unwrap());
    presentation
        .tick_motion(Duration::from_nanos(5000), [960, 720], &mut scene)
        .unwrap();
    assert_eq!(
        scene.component_transform(rebound).unwrap().offset(),
        [-100.0, 0.0]
    );
    presentation
        .tick_motion(Duration::from_nanos(5025), [960, 720], &mut scene)
        .unwrap();
    assert_eq!(
        scene.component_transform(rebound).unwrap().offset(),
        [-150.0, 0.0]
    );
    menu.navigate_with_fields(token(&menu), ScreenRoute::Settings, settings_fields)
        .unwrap();
    presentation.apply(menu.snapshot()).unwrap();
    menu.navigate(token(&menu), ScreenRoute::Display).unwrap();
    presentation.apply(menu.snapshot()).unwrap();
    presentation.compose(&mut scene).unwrap();
    let display_token = token(&menu);
    let display_node = presentation.control_node(ControlId(41)).unwrap();
    presentation
        .request_motion(
            display_token,
            ControlId(41),
            requested_menu_motion(100.0),
            Duration::from_nanos(5025),
            &mut scene,
        )
        .unwrap();
    presentation
        .tick_motion(Duration::from_nanos(5050), [960, 720], &mut scene)
        .unwrap();
    presentation.publish_presented(&scene, [960, 720]).unwrap();
    // The moved BACK control retains its action row's fixed ancestor clip.
    assert_eq!(presentation.hit((395.0, 635.0), [960, 720]), 0);
    assert_eq!(presentation.hit((350.0, 635.0), [960, 720]), 41);
    menu.back(display_token).unwrap();
    presentation.apply(menu.snapshot()).unwrap();
    presentation.compose(&mut scene).unwrap();
    assert!(scene
        .component_id(crate::scene::UiComponentKey {
            screen: display_token.screen,
            node: display_node
        })
        .is_none());
    assert!(presentation
        .request_motion(
            display_token,
            ControlId(41),
            requested_menu_motion(100.0),
            Duration::from_nanos(5050),
            &mut scene
        )
        .is_err());
    presentation.dispose_motion();
    assert!(!presentation.motion_active());
    assert!(presentation
        .request_motion(
            token(&menu),
            ControlId(5),
            requested_menu_motion(1.0),
            Duration::from_nanos(5050),
            &mut scene
        )
        .is_err());
}

#[test]
fn actual_selection_motion_ticks_reuse_packets_and_publish_hits_only_after_accepted_frame() {
    use crate::{browser_menu::BrowserMenuPresentation, ui::interaction::ControlId};
    use std::{sync::Arc, time::Duration};
    let menu = BrowserMenu::new(97).unwrap();
    let mut presentation = BrowserMenuPresentation::new(menu.snapshot()).unwrap();
    let mut scene = render_menu(&mut presentation);
    let node = presentation.control_node(ControlId(5)).unwrap();
    assert!(node.0 < 1024);
    assert_eq!(presentation.hit((760.0, 35.0), [960, 720]), 5);
    presentation
        .request_motion(
            token(&menu),
            ControlId(5),
            requested_menu_motion(-200.0),
            Duration::ZERO,
            &mut scene,
        )
        .unwrap();
    let key = crate::scene::UiComponentKey {
        screen: token(&menu).screen,
        node,
    };
    let id = scene.component_id(key).unwrap();
    let packet = menu_packet(&scene);
    let pointer = scene.rectangles().as_ptr();
    let identity = Arc::clone(scene.geometry_stamp().0);
    let epoch = scene.geometry_stamp().1;
    let model = presentation.model.clone();
    assert!(presentation
        .tick_motion(Duration::from_nanos(50), [960, 720], &mut scene)
        .unwrap());
    assert_eq!(
        scene.component_transform(id).unwrap().offset(),
        [-100.0, 0.0]
    );
    assert_eq!(
        presentation.model, model,
        "animation needs no business revision"
    );
    assert_eq!(menu_packet(&scene), packet);
    assert_eq!(scene.rectangles().as_ptr(), pointer);
    assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
    assert_eq!(scene.geometry_stamp().1, epoch);
    // This unaccepted draw models a surface retry: the actual presentation
    // owner must keep inverse input at the earlier successfully painted pose.
    assert_eq!(presentation.hit((660.0, 35.0), [960, 720]), 0);
    assert_eq!(presentation.hit((920.0, 35.0), [960, 720]), 5);
    assert!(presentation.publish_presented(&scene, [0, 720]).is_err());
    assert_eq!(presentation.hit((660.0, 35.0), [960, 720]), 0);
    presentation.publish_presented(&scene, [960, 720]).unwrap();
    assert_eq!(presentation.hit((660.0, 35.0), [960, 720]), 5);
    assert_eq!(presentation.hit((920.0, 35.0), [960, 720]), 0);
    assert!(presentation.motion_active());
    assert!(presentation
        .tick_motion(Duration::from_nanos(100), [960, 720], &mut scene)
        .unwrap());
    assert!(!presentation.motion_active());
    presentation.publish_presented(&scene, [960, 720]).unwrap();
    assert_eq!(presentation.hit((560.0, 35.0), [960, 720]), 5);
    assert!(!presentation
        .tick_motion(Duration::from_nanos(101), [960, 720], &mut scene)
        .unwrap());
}

#[test]
fn actual_menu_stale_controls_invalid_transforms_and_regressing_time_refuse_atomically() {
    use crate::{
        browser_menu::BrowserMenuPresentation, scene::UiTransform, ui::interaction::ControlId,
    };
    use std::time::Duration;
    let menu = BrowserMenu::new(101).unwrap();
    let mut presentation = BrowserMenuPresentation::new(menu.snapshot()).unwrap();
    let mut scene = render_menu(&mut presentation);
    presentation
        .request_motion(
            token(&menu),
            ControlId(5),
            requested_menu_motion(-200.0),
            Duration::ZERO,
            &mut scene,
        )
        .unwrap();
    presentation
        .tick_motion(Duration::from_nanos(25), [960, 720], &mut scene)
        .unwrap();
    presentation.publish_presented(&scene, [960, 720]).unwrap();
    let node = presentation.control_node(ControlId(5)).unwrap();
    let id = scene
        .component_id(crate::scene::UiComponentKey {
            screen: token(&menu).screen,
            node,
        })
        .unwrap();
    let pose = scene.component_transform(id);
    let packet = menu_packet(&scene);
    let epoch = scene.geometry_stamp().1;
    for bad in [
        MenuToken {
            generation: 1,
            ..token(&menu)
        },
        MenuToken {
            screen: ScreenInstanceId(999),
            ..token(&menu)
        },
        MenuToken {
            revision: token(&menu).revision + 1,
            ..token(&menu)
        },
    ] {
        assert!(presentation
            .request_motion(
                bad,
                ControlId(5),
                requested_menu_motion(80.0),
                Duration::from_nanos(25),
                &mut scene
            )
            .is_err());
    }
    assert!(presentation
        .request_motion(
            token(&menu),
            ControlId(u64::MAX),
            requested_menu_motion(80.0),
            Duration::from_nanos(25),
            &mut scene
        )
        .is_err());
    assert!(presentation
        .request_motion(
            token(&menu),
            ControlId(5),
            requested_menu_motion(80.0),
            Duration::from_nanos(24),
            &mut scene
        )
        .is_err());
    assert!(presentation
        .tick_motion(Duration::from_nanos(24), [960, 720], &mut scene)
        .is_err());
    for (offset, scale, opacity) in [
        ([f32::NAN, 0.0], [1.0, 1.0], 1.0),
        ([0.0, 0.0], [0.0, 1.0], 1.0),
        ([0.0, 0.0], [1.0, 1.0], 2.0),
    ] {
        assert!(UiTransform::new(offset, scale, opacity).is_err());
    }
    assert_eq!(scene.component_transform(id), pose);
    assert_eq!(menu_packet(&scene), packet);
    assert_eq!(scene.geometry_stamp().1, epoch);
    assert_eq!(presentation.hit((710.0, 35.0), [960, 720]), 5);
    presentation
        .tick_motion(Duration::from_nanos(50), [960, 720], &mut scene)
        .unwrap();
    assert_eq!(
        scene.component_transform(id).unwrap().offset(),
        [-100.0, 0.0]
    );
}

#[test]
fn menu_navigation_retains_parent_draft_and_back_discards_child_editor() {
    let mut menu = BrowserMenu::new(17).unwrap();
    let selection = menu.snapshot();
    assert_eq!(selection.route, ScreenRoute::Selection);
    menu.navigate(token(&menu), ScreenRoute::Settings).unwrap();
    menu.set_fields(token(&menu), vec!["accepted parent draft".into()])
        .unwrap();
    let settings = menu.snapshot();
    menu.navigate(token(&menu), ScreenRoute::Practice).unwrap();
    menu.set_fields(
        token(&menu),
        vec!["12.345678901".into(), "604800.000000001".into()],
    )
    .unwrap();
    assert_ne!(menu.snapshot().token.screen, settings.token.screen);
    menu.back(token(&menu)).unwrap();
    let returned = menu.snapshot();
    assert_eq!(returned.route, ScreenRoute::Settings);
    assert_eq!(returned.token.screen, settings.token.screen);
    assert_eq!(
        returned.fields, settings.fields,
        "Back never applies a child editor draft"
    );
    assert!(returned.token.revision > settings.token.revision);
    menu.back(token(&menu)).unwrap();
    assert_eq!(menu.snapshot().route, ScreenRoute::Selection);
    assert_eq!(menu.snapshot().token.screen, selection.token.screen);
}

#[test]
fn every_supported_menu_route_uses_shared_navigation_and_preserves_parent() {
    for route in [
        ScreenRoute::Practice,
        ScreenRoute::Records,
        ScreenRoute::Players,
        ScreenRoute::Devices { players: false },
        ScreenRoute::Display,
    ] {
        let mut menu = BrowserMenu::new(1).unwrap();
        menu.navigate(token(&menu), ScreenRoute::Settings).unwrap();
        let parent = menu.snapshot();
        menu.navigate(token(&menu), route).unwrap();
        let child = menu.snapshot();
        assert_eq!(child.route, route);
        assert_ne!(child.token.screen, parent.token.screen);
        menu.back(token(&menu)).unwrap();
        assert_eq!(menu.snapshot().token.screen, parent.token.screen);
        assert_eq!(menu.snapshot().fields, parent.fields);
        assert!(
            menu.navigate(child.token, route).is_err(),
            "retired child cannot navigate its parent"
        );
    }
}

#[test]
fn menu_token_refusals_are_atomic_and_never_coerce_full_u64_identity() {
    assert!(BrowserMenu::new(0).is_err());
    let mut menu = BrowserMenu::new(u64::MAX).unwrap();
    let initial = menu.snapshot();
    assert_eq!(initial.token.generation, u64::MAX);
    let stale = initial.token;
    menu.navigate(token(&menu), ScreenRoute::Settings).unwrap();
    let current = menu.snapshot();
    for wrong in [
        stale,
        MenuToken {
            generation: 1,
            ..current.token
        },
        MenuToken {
            screen: ScreenInstanceId(u64::MAX),
            ..current.token
        },
        MenuToken {
            revision: current.token.revision.saturating_add(1),
            ..current.token
        },
    ] {
        assert!(menu.set_fields(wrong, vec!["forged draft".into()]).is_err());
        assert!(menu.back(wrong).is_err());
        assert!(menu.accept_action(wrong, 1, 1).is_err());
        assert_eq!(menu.snapshot(), current);
    }
}

#[test]
fn menu_budgets_and_suspend_dispose_refuse_before_mutation() {
    let mut menu = BrowserMenu::new(3).unwrap();
    menu.navigate(token(&menu), ScreenRoute::Settings).unwrap();
    let original = menu.snapshot();
    for fields in [
        vec!["x".repeat(4097)],
        vec![String::new(); MAX_MENU_FIELDS + 1],
        vec!["x".repeat(4096); MAX_MENU_BYTES / 4096 + 1],
    ] {
        assert!(menu.set_fields(token(&menu), fields).is_err());
        assert_eq!(menu.snapshot(), original);
    }
    menu.suspend();
    let suspended = menu.snapshot();
    assert!(menu
        .set_fields(token(&menu), vec!["hidden edit".into()])
        .is_err());
    assert!(menu.accept_action(token(&menu), 1, 1).is_err());
    assert_eq!(menu.snapshot(), suspended);
    menu.resume();
    menu.navigate(token(&menu), ScreenRoute::Practice).unwrap();
    menu.dispose();
    let disposed = menu.snapshot();
    assert!(menu.back(token(&menu)).is_err());
    assert!(menu.accept_action(token(&menu), 1, 1).is_err());
    menu.resume();
    assert_eq!(
        menu.snapshot(),
        disposed,
        "resume cannot resurrect disposed local views"
    );
}

#[test]
fn rejected_navigation_does_not_consume_action_identity_or_change_editor() {
    let mut menu = BrowserMenu::new(11).unwrap();
    let original = menu.snapshot();
    assert!(menu.navigate(token(&menu), ScreenRoute::Practice).is_err());
    assert_eq!(menu.snapshot(), original);
    assert!(menu.accept_action(token(&menu), 0, 1).is_err());
    assert_eq!(menu.snapshot(), original);
}

#[test]
fn semantic_actions_use_original_view_controls_and_ack_exactly_once() {
    let mut menu = BrowserMenu::new(5).unwrap();
    menu.accept_action(token(&menu), 1, 5).unwrap();
    assert_eq!(menu.snapshot().route, ScreenRoute::Settings);
    let parent_fields = (0..13)
        .map(|index| format!("original parent field {index}"))
        .collect::<Vec<_>>();
    menu.set_fields(token(&menu), parent_fields.clone())
        .unwrap();
    let settings = menu.snapshot();
    assert!(menu.accept_action(token(&menu), 1, 74).is_err());
    assert_eq!(
        menu.snapshot(),
        settings,
        "duplicate ACK cannot run a second action"
    );
    assert!(menu.accept_action(token(&menu), 2, u64::MAX).is_err());
    assert_eq!(menu.snapshot(), settings);
    menu.accept_action(token(&menu), 2, 74).unwrap();
    assert_eq!(menu.snapshot().route, ScreenRoute::Practice);
    menu.set_fields(
        token(&menu),
        vec!["1.000000001".into(), "2.000000002".into()],
    )
    .unwrap();
    let changed = menu.snapshot();
    assert_eq!(
        menu.accept_action(token(&menu), 3, 71).unwrap(),
        MenuEffect::None
    );
    let adopted = menu.snapshot();
    assert_eq!(adopted.route, ScreenRoute::Settings);
    assert_eq!(adopted.token.screen, settings.token.screen);
    assert!(adopted.token.revision > changed.token.revision);
    let mut expected_parent = parent_fields;
    expected_parent[11] = "1.000000001".into();
    expected_parent[12] = "2.000000002".into();
    assert_eq!(adopted.fields, expected_parent);
    assert_eq!(
        menu.accept_action(token(&menu), 4, 10).unwrap(),
        MenuEffect::Apply
    );
    menu.navigate(token(&menu), ScreenRoute::Practice).unwrap();
    assert_eq!(menu.snapshot().fields, changed.fields);
    menu.accept_action(token(&menu), 5, 72).unwrap();
    assert_eq!(menu.snapshot().route, ScreenRoute::Settings);
    assert!(menu.accept_action(changed.token, 6, 71).is_err());
    let current = menu.snapshot();
    assert_eq!(
        menu.accept_action(token(&menu), 6, 13).unwrap(),
        MenuEffect::Load
    );
    assert_eq!(
        menu.snapshot().fields,
        current.fields,
        "Load intent is not a fake I/O success"
    );
}

#[test]
fn menu_wire_preserves_wide_identity_utf8_and_every_supported_route() {
    let mut menu = BrowserMenu::new(u64::MAX).unwrap();
    menu.navigate(token(&menu), ScreenRoute::Settings).unwrap();
    menu.set_fields(token(&menu), vec!["曲🎵".into(), "604800.000000001".into()])
        .unwrap();
    let snapshot = menu.snapshot();
    let bytes = encode_snapshot(&snapshot).unwrap();
    assert_eq!(&bytes[..4], b"BKMN");
    assert_eq!(&bytes[4..12], &u64::MAX.to_le_bytes());
    assert_eq!(&bytes[12..20], &snapshot.token.screen.0.to_le_bytes());
    assert_eq!(&bytes[20..28], &snapshot.token.revision.to_le_bytes());
    assert_eq!(&bytes[28..32], &2u32.to_le_bytes());
    assert_eq!(&bytes[36..40], &2u32.to_le_bytes());
    assert_eq!(&bytes[40..44], &[0, 0, 0, 0]);
    assert_eq!(decode_snapshot(&bytes).unwrap(), snapshot);
    for route in [
        ScreenRoute::Selection,
        ScreenRoute::Settings,
        ScreenRoute::Practice,
        ScreenRoute::Records,
        ScreenRoute::Players,
        ScreenRoute::Devices { players: false },
        ScreenRoute::Devices { players: true },
        ScreenRoute::Display,
        ScreenRoute::Results { replay: false },
        ScreenRoute::Results { replay: true },
    ] {
        let mut dto = snapshot.clone();
        dto.route = route;
        assert_eq!(
            decode_snapshot(&encode_snapshot(&dto).unwrap()).unwrap(),
            dto
        );
    }
}

#[test]
fn menu_wire_hostile_last_field_and_header_refuse_without_owner_mutation() {
    let mut menu = BrowserMenu::new(7).unwrap();
    menu.navigate(token(&menu), ScreenRoute::Settings).unwrap();
    menu.set_fields(token(&menu), vec!["unchanged draft".into()])
        .unwrap();
    let snapshot = menu.snapshot();
    let bytes = encode_snapshot(&snapshot).unwrap();
    for extent in 0..bytes.len() {
        assert!(decode_snapshot(&bytes[..extent]).is_err());
    }
    let mut malformed = Vec::new();
    for index in [0, 4, 12, 20, 28, 36, 40, 41, 42, 43] {
        let mut bad = bytes.clone();
        if [4, 12, 20].contains(&index) {
            bad[index..index + 8].fill(0);
        } else {
            bad[index] = 255;
        }
        malformed.push(bad);
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    malformed.push(trailing);
    let mut invalid_utf8 = bytes.clone();
    invalid_utf8[48] = 255;
    malformed.push(invalid_utf8);
    let mut control = bytes.clone();
    control[48] = 0;
    malformed.push(control);
    malformed.push(vec![0; MAX_MENU_BYTES + 1]);
    for bad in malformed {
        assert!(decode_snapshot(&bad).is_err());
        assert_eq!(
            menu.snapshot(),
            snapshot,
            "hostile DTO never changes business owner"
        );
    }
}

#[test]
fn unchanged_draft_is_revision_stable_and_nested_device_back_keeps_players_identity() {
    let mut menu = BrowserMenu::new(19).unwrap();
    menu.navigate(token(&menu), ScreenRoute::Settings).unwrap();
    menu.navigate(token(&menu), ScreenRoute::Players).unwrap();
    menu.set_fields(token(&menu), acquired_browser_member_fields())
        .unwrap();
    let players = menu.snapshot();
    menu.set_fields(token(&menu), players.fields.clone())
        .unwrap();
    assert_eq!(
        menu.snapshot(),
        players,
        "unchanged state does not rebuild its declaration or stale its action token"
    );
    menu.accept_action(token(&menu), 1, 34).unwrap();
    assert_eq!(
        menu.snapshot().route,
        ScreenRoute::Devices { players: true }
    );
    let devices = menu.snapshot();
    menu.accept_action(token(&menu), 2, 21).unwrap();
    assert_eq!(menu.snapshot().token.screen, players.token.screen);
    assert_eq!(menu.snapshot().fields, players.fields);
    assert!(menu.accept_action(devices.token, 3, 20).is_err());
    assert_eq!(
        menu.snapshot().fields,
        players.fields,
        "retired device picker cannot replace an assignment"
    );
}

#[test]
fn menu_navigation_cannot_manufacture_gameplay_completion_or_output_ownership() {
    let mut menu = BrowserMenu::new(37).unwrap();
    let original = menu.snapshot();
    for route in [
        ScreenRoute::Play { replay: false },
        ScreenRoute::Play { replay: true },
        ScreenRoute::Results { replay: false },
        ScreenRoute::Results { replay: true },
        ScreenRoute::LiveAudio,
        ScreenRoute::Closing,
    ] {
        assert!(menu.navigate(token(&menu), route).is_err());
        assert_eq!(menu.snapshot(), original);
    }
    assert_eq!(
        menu.accept_action(token(&menu), 1, 1).unwrap(),
        MenuEffect::Start
    );
    assert_eq!(
        menu.snapshot(),
        original,
        "Start emits intent; only the actual game owner can prepare and complete a session"
    );
}

fn genuine_record() -> crate::record_model::FrozenRecordPreview {
    crate::record_model::FrozenRecordPreview::from_record(
        &crate::record_model::visual_fixtures::record(),
    )
    .unwrap()
}
fn prefix_only() -> crate::record_model::FrozenRecordPreview {
    let mut record = genuine_record();
    record.historical = None;
    record.historical_score = None;
    record.historical_comparison = None;
    record.historical_bms_score = None;
    record
}
fn render_menu(
    presentation: &mut crate::browser_menu::BrowserMenuPresentation,
) -> crate::scene::Scene {
    let mut scene = crate::scene::Scene::new(960, 720);
    presentation.compose(&mut scene).unwrap();
    scene.status().unwrap();
    presentation.publish_presented(&scene, [960, 720]).unwrap();
    scene
}
fn menu_packet(scene: &crate::scene::Scene) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    scene
        .rectangles()
        .iter()
        .map(|r| (r.bounds, r.color, r.uv))
        .collect()
}
fn menu_label(scene: &crate::scene::Scene, value: &str) {
    let expected: Vec<_> = value.chars().map(crate::font::glyph_uv).collect();
    let actual: Vec<_> = scene.rectangles().iter().map(|r| r.uv).collect();
    assert!(
        actual
            .windows(expected.len())
            .any(|window| window == expected),
        "missing actual menu label {value}"
    );
}
fn records_menu() -> BrowserMenu {
    let record = genuine_record();
    let mut menu = BrowserMenu::new(u64::MAX).unwrap();
    menu.navigate(token(&menu), ScreenRoute::Settings).unwrap();
    menu.set_fields(
        token(&menu),
        [
            "5", "5", "0", "output", "0", "48000", "8", "4", "8", "256", "8", "0", "",
        ]
        .map(str::to_owned)
        .to_vec(),
    )
    .unwrap();
    menu.navigate(token(&menu), ScreenRoute::Records).unwrap();
    menu.set_fields(token(&menu), vec![record.path.to_str().unwrap().into()])
        .unwrap();
    menu
}

#[test]
fn genuine_prefix_wire_preserves_original_timing_classes_and_wide_token_without_live_restoration() {
    use crate::browser_menu::{decode_record_preview, encode_record_preview};
    let value = prefix_only();
    let menu = records_menu();
    let bytes = encode_record_preview(token(&menu), &value, None).unwrap();
    assert_eq!(&bytes[..4], b"BKRP");
    assert_eq!(&bytes[4..12], &u64::MAX.to_le_bytes());
    let (decoded_token, decoded) = decode_record_preview(&bytes).unwrap();
    assert_eq!(decoded_token, token(&menu));
    assert_eq!(decoded.path, value.path);
    assert_eq!(decoded.records, value.records);
    assert_eq!(decoded.recorded_until, value.recorded_until);
    assert_eq!(decoded.start, value.start);
    assert_eq!(decoded.end, value.end);
    assert_eq!(decoded.score, value.score);
    assert_eq!(decoded.score.timing.count, 1);
    assert_eq!(decoded.score.timing.sum, 2);
    assert_eq!(decoded.score.timing.absolute_sum, 2);
    assert_eq!(decoded.score.timing.last, Some(2));
    assert_eq!(decoded.score.timing.min, Some(2));
    assert_eq!(decoded.score.timing.max, Some(2));
    assert_eq!(decoded.bms_score, value.bms_score);
    assert!(decoded.historical.is_none());
    assert!(decoded.historical_score.is_none());
    assert!(decoded.historical_comparison.is_none());
    assert_eq!(
        encode_record_preview(decoded_token, &decoded, None).unwrap(),
        bytes
    );
    assert!(
        encode_record_preview(token(&menu), &genuine_record(), None).is_err(),
        "associated history cannot be encoded without its genuine archive"
    );
}

#[test]
fn genuine_capture_and_original_archive_body_roundtrip_without_inventing_historical_completion() {
    use crate::browser_menu::{decode_record_preview, encode_record_preview};
    let (record, archive) = crate::record_model::visual_fixtures::record_with_archive();
    let frozen = crate::record_model::FrozenRecordPreview::from_record(&record).unwrap();
    let menu = records_menu();
    let bytes = encode_record_preview(token(&menu), &frozen, Some(&archive)).unwrap();
    let (decoded_token, decoded) = decode_record_preview(&bytes).unwrap();
    assert_eq!(decoded_token, token(&menu));
    assert_eq!(decoded.score, frozen.score);
    assert_eq!(decoded.bms_score, frozen.bms_score);
    assert_eq!(decoded.historical, frozen.historical);
    assert_eq!(decoded.historical_score, frozen.historical_score);
    assert_eq!(decoded.historical_bms_score, frozen.historical_bms_score);
    assert_eq!(decoded.historical_comparison, frozen.historical_comparison);
    assert_eq!(
        decoded.historical.unwrap().0,
        crate::local_players::PlayerId(u32::MAX)
    );
    assert_eq!(
        encode_record_preview(decoded_token, &decoded, Some(&archive)).unwrap(),
        bytes
    );
    let body = crate::result_archive::encode_archive(&archive).unwrap();
    assert!(bytes.ends_with(&body));
    let body_at = bytes.len() - body.len();
    let mut hostile_body = bytes.clone();
    hostile_body[body_at] = 0;
    assert!(decode_record_preview(&hostile_body).is_err());
    let mut hostile_length = bytes.clone();
    hostile_length[body_at - 4..body_at].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_record_preview(&hostile_length).is_err());
    for end in body_at..bytes.len() {
        assert!(decode_record_preview(&bytes[..end]).is_err());
    }
    let mut mismatch = frozen.clone();
    mismatch.historical_score = None;
    mismatch.historical_bms_score = None;
    assert!(encode_record_preview(token(&menu), &mismatch, Some(&archive)).is_err());
}

#[test]
fn record_wire_truncation_hostile_extents_counts_timing_and_tokens_refuse_without_menu_effects() {
    use crate::browser_menu::{decode_record_preview, encode_record_preview};
    let record = prefix_only();
    let menu = records_menu();
    let original = menu.snapshot();
    let bytes = encode_record_preview(original.token, &record, None).unwrap();
    for end in 0..bytes.len() {
        assert!(decode_record_preview(&bytes[..end]).is_err());
    }
    // The fixed token and counted UTF-8 path precede this genuine specimen's
    // prefix extent. Its end is absent and its actual recorded-until is present.
    let prefix = 32 + record.path.to_str().unwrap().len();
    let grade_count = prefix + 8 + 8 + 1 + 9 + 32;
    let timing = grade_count + 4 + record.score.grades.len() * 12;
    let mut hostile = Vec::new();
    let mut magic = bytes.clone();
    magic[0] = 0;
    hostile.push(magic);
    for index in [4, 12, 20] {
        let mut bad = bytes.clone();
        bad[index..index + 8].fill(0);
        hostile.push(bad);
    }
    let mut path_len = bytes.clone();
    path_len[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
    hostile.push(path_len);
    for first in [0u8, 255] {
        let mut bad = bytes.clone();
        bad[32] = first;
        hostile.push(bad);
    }
    let mut count = bytes.clone();
    count[prefix..prefix + 8].copy_from_slice(&1_000_001u64.to_le_bytes());
    hostile.push(count);
    let mut option = bytes.clone();
    option[prefix + 16] = 255;
    hostile.push(option);
    let mut grades = bytes.clone();
    grades[grade_count..grade_count + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    hostile.push(grades);
    let mut timing_count = bytes.clone();
    timing_count[timing..timing + 8].copy_from_slice(&2u64.to_le_bytes());
    hostile.push(timing_count);
    let mut trailing = bytes.clone();
    trailing.push(0);
    hostile.push(trailing);
    hostile.push(vec![0; MAX_MENU_BYTES + 1]);
    for bad in hostile {
        assert!(decode_record_preview(&bad).is_err());
        assert_eq!(menu.snapshot(), original);
    }
    let mut too_many = record.clone();
    too_many.records = 1_000_001;
    assert!(encode_record_preview(original.token, &too_many, None).is_err());
    let mut path = record.clone();
    path.path = "x".repeat(4097).into();
    assert!(encode_record_preview(original.token, &path, None).is_err());
}

#[test]
fn touch_revisions_and_renderer_preview_correlation_refuse_stale_path_and_navigation_atomically() {
    let mut menu = records_menu();
    let mut presentation =
        crate::browser_menu::BrowserMenuPresentation::new(menu.snapshot()).unwrap();
    let record = genuine_record();
    presentation
        .set_record_preview(token(&menu), record.clone())
        .unwrap();
    let original = render_menu(&mut presentation);
    menu_label(&original, "PREFIX EX 2");
    let stale = token(&menu);
    menu.touch(stale).unwrap();
    assert_eq!(token(&menu).revision, stale.revision + 1);
    assert_eq!(token(&menu).screen, stale.screen);
    let touched = menu.snapshot();
    assert!(menu.touch(stale).is_err());
    assert_eq!(menu.snapshot(), touched);
    presentation.apply(touched).unwrap();
    assert!(presentation
        .set_record_preview(stale, record.clone())
        .is_err());
    assert_eq!(
        menu_packet(&render_menu(&mut presentation)),
        menu_packet(&original)
    );
    let mut foreign = record.clone();
    foreign.path = "records/foreign.bkr".into();
    assert!(presentation
        .set_record_preview(token(&menu), foreign)
        .is_err());
    assert_eq!(
        menu_packet(&render_menu(&mut presentation)),
        menu_packet(&original)
    );
    presentation
        .set_record_preview(token(&menu), record)
        .unwrap();
    menu.back(token(&menu)).unwrap();
    presentation.apply(menu.snapshot()).unwrap();
    let parent = render_menu(&mut presentation);
    assert_eq!(presentation.model.route, ScreenRoute::Settings);
    assert_ne!(menu_packet(&parent), menu_packet(&original));
    assert!(presentation
        .set_record_preview(stale, genuine_record())
        .is_err());
}

#[test]
fn genuine_stored_records_pages_opponent_gates_and_cached_renderer_lifecycle_match_shared_views() {
    let menu = records_menu();
    let mut presentation =
        crate::browser_menu::BrowserMenuPresentation::new(menu.snapshot()).unwrap();
    let record = genuine_record();
    presentation
        .set_record_preview(token(&menu), record.clone())
        .unwrap();
    presentation.set_opponents(8, 1, 0).unwrap();
    let catalog = render_menu(&mut presentation);
    assert_eq!(presentation.hit((310.0, 625.0), [960, 720]), 0);
    assert_eq!(presentation.hit((450.0, 580.0), [960, 720]), 60);
    assert_eq!(presentation.hit((600.0, 580.0), [960, 720]), 0);
    assert_eq!(presentation.hit((760.0, 580.0), [960, 720]), 59);
    assert!(presentation.set_opponents(9, 1, 0).is_err());
    assert!(presentation.set_opponents(8, u32::MAX, 1).is_err());
    assert_eq!(
        menu_packet(&render_menu(&mut presentation)),
        menu_packet(&catalog)
    );
    for page in [0, 1] {
        presentation.set_record_details(true, page).unwrap();
        let scene = render_menu(&mut presentation);
        assert!(presentation.record_details());
        assert_eq!(presentation.record_page(), page);
        assert_eq!(presentation.hit((755.0, 621.0), [960, 720]), 66);
        assert_eq!(presentation.hit((30.0, 180.0), [960, 720]), 0);
        if page == 0 {
            menu_label(&scene, "STORED HITS 1 MISSES 0");
            menu_label(&scene, "TIMING SUM 2 NS");
        } else {
            menu_label(&scene, "SAVED REPLAY OPERATION PREFIX");
        }
        presentation
            .set_record_preview(token(&menu), record.clone())
            .unwrap();
        assert_eq!(presentation.record_page(), page);
        assert_eq!(
            menu_packet(&render_menu(&mut presentation)),
            menu_packet(&scene)
        );
        assert!(presentation.set_record_details(true, 2).is_err());
        assert_eq!(presentation.record_page(), page);
        assert_eq!(
            menu_packet(&render_menu(&mut presentation)),
            menu_packet(&scene)
        );
    }
    presentation.set_record_details(false, 0).unwrap();
    assert_eq!(
        menu_packet(&render_menu(&mut presentation)),
        menu_packet(&catalog)
    );
    assert_eq!(presentation.hit((755.0, 621.0), [960, 720]), 55);
}

#[test]
fn practice_done_rejects_invalid_section_atomically_then_adopts_parent_without_apply_side_effect() {
    let mut menu = BrowserMenu::new(73).unwrap();
    menu.navigate(token(&menu), ScreenRoute::Settings).unwrap();
    let original = [
        "5", "5", "0", "output", "0", "48000", "8", "4", "8", "256", "8", "0", "",
    ]
    .map(str::to_owned)
    .to_vec();
    menu.set_fields(token(&menu), original.clone()).unwrap();
    let parent = menu.snapshot();
    menu.navigate(token(&menu), ScreenRoute::Practice).unwrap();
    assert_eq!(menu.fields(), ["0", ""]);
    for fields in [
        vec!["invalid".into(), "".into()],
        vec!["2".into(), "1".into()],
    ] {
        menu.set_fields(token(&menu), fields).unwrap();
        let bad = menu.snapshot();
        assert!(menu.accept_action(token(&menu), 1, 71).is_err());
        assert_eq!(menu.snapshot(), bad);
    }
    menu.set_fields(
        token(&menu),
        vec!["1.000000001".into(), "604800.000000001".into()],
    )
    .unwrap();
    let child = menu.snapshot();
    assert_eq!(
        menu.accept_action(token(&menu), 1, 71).unwrap(),
        MenuEffect::None
    );
    let returned = menu.snapshot();
    assert_eq!(returned.route, ScreenRoute::Settings);
    assert_eq!(returned.token.screen, parent.token.screen);
    assert!(returned.token.revision > child.token.revision);
    assert_eq!(&returned.fields[..11], &original[..11]);
    assert_eq!(&returned.fields[11..], &["1.000000001", "604800.000000001"]);
    assert!(menu.accept_action(child.token, 2, 71).is_err());
    assert_eq!(
        menu.accept_action(token(&menu), 2, 10).unwrap(),
        MenuEffect::Apply
    );
    menu.navigate(token(&menu), ScreenRoute::Practice).unwrap();
    let before_back = menu.snapshot();
    menu.set_fields(token(&menu), vec!["12".into(), "13".into()])
        .unwrap();
    menu.back(token(&menu)).unwrap();
    assert_eq!(menu.snapshot().fields, returned.fields);
    assert!(menu.accept_action(before_back.token, 3, 71).is_err());
}

fn acquired_browser_member_fields() -> Vec<String> {
    [
        "1",
        "1",
        "2",
        "4294967295",
        "kbd:41",
        "7",
        "touch:2",
        "2",
        "kbd:41",
        "keyboard",
        "PRIMARY KEYS",
        "Original acquired keyboard",
        "1",
        "touch:2",
        "touch",
        "TOUCHSCREEN",
        "Original acquired touch source",
        "1",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[test]
fn malformed_destination_member_metadata_keeps_parent_backstack_action_space_and_next_instance() {
    let mut menu = BrowserMenu::new(83).unwrap();
    let mut pristine = BrowserMenu::new(83).unwrap();
    let parent_fields = [
        "5", "5", "0", "output", "0", "48000", "8", "4", "8", "256", "8", "0", "",
    ]
    .map(str::to_owned)
    .to_vec();
    for owner in [&mut menu, &mut pristine] {
        owner
            .navigate_with_fields(token(owner), ScreenRoute::Settings, parent_fields.clone())
            .unwrap();
    }
    let parent = menu.snapshot();
    let valid = acquired_browser_member_fields();
    let mut malformed = Vec::new();
    for (field, value) in [
        (0, "2"),
        (1, "-1"),
        (2, "65"),
        (3, "0"),
        (5, "4294967295"),
        (6, "kbd:41"),
        (7, "999999999"),
        (13, "kbd:41"),
        (14, "native-path"),
        (17, "2"),
        (10, "invalid\nlabel"),
    ] {
        let mut bad = valid.clone();
        bad[field] = value.into();
        malformed.push(bad);
    }
    let mut truncated = valid.clone();
    truncated.pop();
    malformed.push(truncated);
    let mut trailing = valid.clone();
    trailing.push("foreign trailing metadata".into());
    malformed.push(trailing);
    for fields in malformed {
        assert!(menu
            .navigate_with_fields(parent.token, ScreenRoute::Players, fields)
            .is_err());
        assert_eq!(menu.snapshot(), parent);
    }
    menu.navigate_with_fields(parent.token, ScreenRoute::Players, valid.clone())
        .unwrap();
    pristine
        .navigate_with_fields(token(&pristine), ScreenRoute::Players, valid.clone())
        .unwrap();
    assert_eq!(
        menu.snapshot(),
        pristine.snapshot(),
        "failed destinations must not consume the next screen identity"
    );
    assert_eq!(menu.fields(), valid);
    menu.accept_action(token(&menu), 1, 34).unwrap();
    pristine.accept_action(token(&pristine), 1, 34).unwrap();
    assert_eq!(menu.snapshot(), pristine.snapshot());
    for expected in [
        ScreenRoute::Players,
        ScreenRoute::Settings,
        ScreenRoute::Selection,
    ] {
        menu.back(token(&menu)).unwrap();
        pristine.back(token(&pristine)).unwrap();
        assert_eq!(menu.snapshot(), pristine.snapshot());
        assert_eq!(menu.route(), expected);
        if expected == ScreenRoute::Settings {
            assert_eq!(menu.fields(), parent_fields);
        }
    }
}

#[test]
fn destination_shape_capacity_unsupported_route_and_stale_tokens_refuse_before_navigation_commit() {
    let mut menu = BrowserMenu::new(u64::MAX).unwrap();
    let mut pristine = BrowserMenu::new(u64::MAX).unwrap();
    let original = menu.snapshot();
    // A well-formed Players payload still cannot bypass the actual parent edge.
    assert!(menu
        .navigate_with_fields(
            original.token,
            ScreenRoute::Players,
            acquired_browser_member_fields()
        )
        .is_err());
    assert_eq!(menu.snapshot(), original);
    for owner in [&mut menu, &mut pristine] {
        owner
            .navigate_with_fields(
                token(owner),
                ScreenRoute::Settings,
                vec!["accepted parent draft".into()],
            )
            .unwrap();
    }
    let parent = menu.snapshot();
    let cases = [
        (ScreenRoute::Practice, vec!["1".into()]),
        (ScreenRoute::Display, vec!["1".into(); 3]),
        (ScreenRoute::Records, vec!["record.bkr".into(); 257]),
        (
            ScreenRoute::Settings,
            vec![String::new(); crate::settings::MAX_FIELDS + 1],
        ),
        (ScreenRoute::Practice, vec!["x".repeat(4097), "".into()]),
        (ScreenRoute::Play { replay: false }, vec![]),
        (ScreenRoute::Results { replay: true }, vec![]),
        (ScreenRoute::Closing, vec![]),
    ];
    for (route, values) in cases {
        assert!(menu
            .navigate_with_fields(parent.token, route, values)
            .is_err());
        assert_eq!(menu.snapshot(), parent);
    }
    for wrong in [
        original.token,
        MenuToken {
            generation: 1,
            ..parent.token
        },
        MenuToken {
            screen: ScreenInstanceId(u64::MAX),
            ..parent.token
        },
        MenuToken {
            revision: parent.token.revision + 1,
            ..parent.token
        },
    ] {
        assert!(menu
            .navigate_with_fields(wrong, ScreenRoute::Display, vec!["1".into(); 4])
            .is_err());
        assert_eq!(menu.snapshot(), parent);
    }
    menu.navigate_with_fields(parent.token, ScreenRoute::Display, vec!["1".into(); 4])
        .unwrap();
    pristine
        .navigate_with_fields(token(&pristine), ScreenRoute::Display, vec!["1".into(); 4])
        .unwrap();
    assert_eq!(menu.snapshot(), pristine.snapshot());
    menu.back(token(&menu)).unwrap();
    pristine.back(token(&pristine)).unwrap();
    assert_eq!(menu.snapshot(), pristine.snapshot());
    assert_eq!(menu.fields(), parent.fields);
    assert_eq!(
        menu.accept_action(token(&menu), 1, 13).unwrap(),
        MenuEffect::Load
    );
    assert_eq!(
        pristine.accept_action(token(&pristine), 1, 13).unwrap(),
        MenuEffect::Load
    );
    assert_eq!(menu.snapshot(), pristine.snapshot());
}

#[test]
fn nested_device_destination_failure_keeps_original_players_and_four_entry_backstack() {
    let mut menu = BrowserMenu::new(89).unwrap();
    let mut pristine = BrowserMenu::new(89).unwrap();
    let fields = acquired_browser_member_fields();
    for owner in [&mut menu, &mut pristine] {
        owner
            .navigate_with_fields(
                token(owner),
                ScreenRoute::Settings,
                vec!["kept settings".into()],
            )
            .unwrap();
        owner
            .navigate_with_fields(token(owner), ScreenRoute::Players, fields.clone())
            .unwrap();
    }
    let players = menu.snapshot();
    let mut bad_source = fields.clone();
    bad_source[14] = "unsupported source kind".into();
    for invalid in [
        bad_source,
        vec!["1".into(), "1".into(), "0".into(), "999999999".into()],
    ] {
        assert!(menu
            .navigate_with_fields(
                players.token,
                ScreenRoute::Devices { players: true },
                invalid
            )
            .is_err());
        assert_eq!(menu.snapshot(), players);
    }
    menu.navigate_with_fields(
        players.token,
        ScreenRoute::Devices { players: true },
        fields.clone(),
    )
    .unwrap();
    pristine
        .navigate_with_fields(
            token(&pristine),
            ScreenRoute::Devices { players: true },
            fields,
        )
        .unwrap();
    assert_eq!(menu.snapshot(), pristine.snapshot());
    let devices = menu.snapshot();
    menu.back(token(&menu)).unwrap();
    pristine.back(token(&pristine)).unwrap();
    assert_eq!(menu.snapshot(), pristine.snapshot());
    assert_eq!(menu.snapshot().token.screen, players.token.screen);
    assert_eq!(menu.fields(), players.fields);
    assert!(menu
        .navigate_with_fields(
            devices.token,
            ScreenRoute::Devices { players: true },
            acquired_browser_member_fields()
        )
        .is_err());
    assert_eq!(menu.snapshot(), pristine.snapshot());
}
