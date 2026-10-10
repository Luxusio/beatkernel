#![cfg(feature = "graphics")]

use beatkernel_bms_runtime::{
    local_players::PlayerId,
    local_setup::{BrowserLocalProjection, BrowserPlayerMember, LocalSetup},
    scene::{Scene, UiComponentKey, UiTransform},
    screen_lifecycle::ScreenInstanceId,
    settings::{NativeSettings, SettingsHost},
    ui::{
        interaction::ControlId,
        layout::NodeId,
        players::{BrowserPlayersFrame, PlayersFrame, PlayersView},
    },
};
use std::sync::Arc;

fn model(count: usize) -> LocalSetup {
    let settings = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
    let mut model = LocalSetup::from_settings(&settings, SettingsHost::Linux).unwrap();
    model.resize(count).unwrap();
    model
}

fn frame(model: &LocalSetup) -> PlayersFrame<'_> {
    PlayersFrame {
        model,
        selected: 0,
        first: 0,
        pending: false,
        error: None,
        message: None,
        hovered: None,
        armed: None,
    }
}

fn browser_frame<'a>(model: &'a BrowserLocalProjection<'a>) -> BrowserPlayersFrame<'a> {
    BrowserPlayersFrame {
        model,
        selected: 0,
        first: 0,
        pending: false,
        error: None,
        message: None,
        hovered: None,
        armed: None,
    }
}

#[test]
fn native_solo_and_multiplayer_actions_match_actual_enabled_hits() {
    let view = PlayersView::new(ScreenInstanceId(301), 960, 720).unwrap();
    let solo = model(1);
    view.update(frame(&solo)).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    for control in [30, 31, 33, 20000] {
        assert!(view.node_for_control(ControlId(control)).unwrap().is_some());
    }
    for control in [32, 34, 35, 36, 37, 20001, 99999] {
        assert_eq!(view.node_for_control(ControlId(control)).unwrap(), None);
    }
    assert_eq!(view.hit((484.0, 630.0)), Some(ControlId(33)));

    let multi = model(2);
    view.update(frame(&multi)).unwrap();
    view.compose(&mut scene, &mut hits).unwrap();
    for control in [32, 34, 35, 20000, 20001] {
        assert!(view.node_for_control(ControlId(control)).unwrap().is_some());
    }
    assert_eq!(view.node_for_control(ControlId(20002)).unwrap(), None);
}

#[test]
fn browser_assignment_capability_and_pending_state_control_motion_targets() {
    let players = [
        BrowserPlayerMember {
            id: PlayerId(1),
            source: None,
        },
        BrowserPlayerMember {
            id: PlayerId(2),
            source: None,
        },
    ];
    let mut projection = BrowserLocalProjection {
        players: &players,
        sources: &[],
        can_assign: false,
    };
    let owner = ScreenInstanceId(302);
    let view = PlayersView::new(owner, 960, 720).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.update_browser(browser_frame(&projection)).unwrap();
    view.compose_components(&mut scene, &mut hits, owner, &[])
        .unwrap();
    assert_eq!(view.node_for_control(ControlId(34)).unwrap(), None);
    assert_eq!(view.node_for_control(ControlId(35)).unwrap(), None);
    assert!(view.node_for_control(ControlId(33)).unwrap().is_some());

    projection.can_assign = true;
    view.update_browser(browser_frame(&projection)).unwrap();
    view.compose_components(&mut scene, &mut hits, owner, &[])
        .unwrap();
    assert!(view.node_for_control(ControlId(34)).unwrap().is_some());
    assert!(view.node_for_control(ControlId(35)).unwrap().is_some());
    assert_eq!(
        view.hit_components(&scene, owner, (634.0, 630.0)),
        Some(ControlId(34))
    );

    let mut pending = browser_frame(&projection);
    pending.pending = true;
    view.update_browser(pending).unwrap();
    view.compose_components(&mut scene, &mut hits, owner, &[])
        .unwrap();
    assert!(hits.is_empty());
    for control in [30, 31, 32, 33, 34, 35, 20000, 20001] {
        assert_eq!(view.node_for_control(ControlId(control)).unwrap(), None);
    }
    assert_eq!(view.hit_components(&scene, owner, (634.0, 630.0)), None);
}

#[test]
fn roster_page_targets_follow_present_members_and_navigation_actions() {
    let owner = ScreenInstanceId(305);
    let view = PlayersView::new(owner, 960, 720).unwrap();
    let roster = model(12);
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.update(frame(&roster)).unwrap();
    view.compose_components(&mut scene, &mut hits, owner, &[])
        .unwrap();
    assert_eq!(view.node_for_control(ControlId(36)).unwrap(), None);
    assert!(view.node_for_control(ControlId(37)).unwrap().is_some());
    assert!(view.node_for_control(ControlId(20009)).unwrap().is_some());
    assert_eq!(view.node_for_control(ControlId(20010)).unwrap(), None);

    let mut next = frame(&roster);
    next.first = 10;
    next.selected = 10;
    view.update(next).unwrap();
    view.compose_components(&mut scene, &mut hits, owner, &[])
        .unwrap();
    assert!(view.node_for_control(ControlId(36)).unwrap().is_some());
    assert_eq!(view.node_for_control(ControlId(37)).unwrap(), None);
    assert_eq!(view.node_for_control(ControlId(20000)).unwrap(), None);
    assert!(view.node_for_control(ControlId(20010)).unwrap().is_some());
    assert!(view.node_for_control(ControlId(20011)).unwrap().is_some());
    assert_eq!(view.node_for_control(ControlId(20012)).unwrap(), None);
    assert_eq!(
        view.hit_components(&scene, owner, (30.0, 130.0)),
        Some(ControlId(20010))
    );
    assert_eq!(view.hit_components(&scene, owner, (30.0, 220.0)), None);
}

#[test]
fn translated_action_uses_literal_visual_position_without_rebuilding_geometry() {
    let owner = ScreenInstanceId(303);
    let view = PlayersView::new(owner, 960, 720).unwrap();
    let roster = model(2);
    view.update(frame(&roster)).unwrap();
    let node = view.node_for_control(ControlId(33)).unwrap().unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose_components(&mut scene, &mut hits, owner, &[node])
        .unwrap();
    let key = UiComponentKey {
        screen: owner,
        node,
    };
    let binding = scene.component_id(key).unwrap();
    let (identity, revision) = scene.geometry_stamp();
    let identity = identity.clone();
    assert_eq!(
        view.hit_components(&scene, owner, (604.0, 630.0)),
        Some(ControlId(33))
    );
    // The action row's inherited clip stays at y=620..654. Translating
    // completely above it hides the button rather than expanding that clip.
    scene
        .set_component_transforms(&[(
            binding,
            UiTransform::new([0.0, -50.0], [1.0, 1.0], 1.0).unwrap(),
        )])
        .unwrap();
    assert_eq!(view.hit_components(&scene, owner, (484.0, 580.0)), None);
    assert_eq!(view.hit_components(&scene, owner, (484.0, 630.0)), None);
    scene
        .set_component_transforms(&[(
            binding,
            UiTransform::new([-50.0, 0.0], [1.0, 1.0], 1.0).unwrap(),
        )])
        .unwrap();
    assert_eq!(
        view.hit_components(&scene, owner, (434.0, 630.0)),
        Some(ControlId(33))
    );
    assert_eq!(view.hit_components(&scene, owner, (604.0, 630.0)), None);
    assert_eq!(
        view.hit_components(&scene, ScreenInstanceId(999), (434.0, 630.0)),
        None
    );
    view.compose_components(&mut scene, &mut hits, owner, &[node])
        .unwrap();
    assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
    assert_eq!(revision, scene.geometry_stamp().1);
    assert_eq!(scene.component_id(key), Some(binding));
    assert_eq!(
        view.hit_components(&scene, owner, (434.0, 630.0)),
        Some(ControlId(33))
    );
}

#[test]
fn duplicate_absent_and_zero_owner_requests_preserve_published_scene_and_hits() {
    let owner = ScreenInstanceId(304);
    let view = PlayersView::new(owner, 960, 720).unwrap();
    let roster = model(2);
    view.update(frame(&roster)).unwrap();
    let node = view.node_for_control(ControlId(33)).unwrap().unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose_components(&mut scene, &mut hits, owner, &[node])
        .unwrap();
    let binding = scene
        .component_id(UiComponentKey {
            screen: owner,
            node,
        })
        .unwrap();
    let (identity, revision) = scene.geometry_stamp();
    let identity = identity.clone();
    let numeric_hits = |hits: &[(ControlId, beatkernel_bms_runtime::ui::interaction::Bounds)]| {
        hits.iter()
            .map(|(id, bounds)| (*id, bounds.x, bounds.y, bounds.width, bounds.height))
            .collect::<Vec<_>>()
    };
    let original_hits = numeric_hits(&hits);
    for (request_owner, targets) in [
        (owner, vec![node, node]),
        (owner, vec![NodeId(usize::MAX)]),
        (ScreenInstanceId(0), vec![node]),
    ] {
        assert!(view
            .compose_components(&mut scene, &mut hits, request_owner, &targets)
            .is_err());
        assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
        assert_eq!(revision, scene.geometry_stamp().1);
        assert_eq!(numeric_hits(&hits), original_hits);
        assert_eq!(
            scene.component_id(UiComponentKey {
                screen: owner,
                node
            }),
            Some(binding)
        );
        assert_eq!(
            view.hit_components(&scene, owner, (484.0, 630.0)),
            Some(ControlId(33))
        );
    }
}
