//! Actual portable browser Results owner; no WASM, GPU or mock scheduler.
use crate::{
    browser_results_motion::BrowserResultsMotion,
    local_players::PlayerId,
    scene::{Scene, UiComponentKey, UiTransform},
    screen_lifecycle::ScreenInstanceId,
    ui::{
        layout::NodeId,
        motion::{ComponentMotion, Easing},
        results::{FrozenResultsView, ResultsView},
    },
};
use beatkernel::{
    audio::{command_queue, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank},
    input::BindingMap,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
};
use std::{sync::Arc, time::Duration};

struct Domains;
impl ClockMapper for Domains {
    fn map(&self, p: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (p.domain == target).then_some(p.timestamp)
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
fn view(count: u32) -> FrozenResultsView {
    // Completion comes from the real gameplay owner and two observed output drains.
    let source = beatkernel_bms::parse("#BPM 120\n", Default::default()).unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let prepared = crate::PreparedBms {
        compiled: source.compile().unwrap(),
        source,
        bank: SampleBank::new(format, PcmLimits::new(64, 256, 1).unwrap()).unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    };
    let config = crate::step_gameplay::StepGameplayConfig {
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
    let (mut owner, bank) = crate::step_gameplay::StepGameplay::new_section(
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
    // Retain the connected output producer until both observed drains finish.
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
    let result = *owner.completed_result().unwrap();
    let roster: Vec<_> = (1..=count).map(PlayerId).collect();
    let rows: Vec<_> = roster.iter().map(|player| (*player, result)).collect();
    let original = ResultsView::new(&rows, &roster).unwrap();
    FrozenResultsView::from_model(original.export_visual().unwrap()).unwrap()
}
fn motion() -> ComponentMotion {
    ComponentMotion::new(
        UiTransform::default(),
        UiTransform::new([40.0, 10.0], [1.25, 0.75], 0.5).unwrap(),
        Duration::from_secs(1),
        Easing::Linear,
    )
}
fn id(scene: &Scene, owner: &BrowserResultsMotion, node: NodeId) -> crate::scene::UiComponentId {
    scene
        .component_id(UiComponentKey {
            screen: owner.screen(),
            node,
        })
        .unwrap()
}

#[test]
fn full_u64_pair_authentication_and_refusal_are_atomic() {
    let pair = ((1_u64 << 63) + 9, u64::MAX);
    let view = view(1);
    let node = view.displayed_nodes(0, false).unwrap()[0];
    let mut owner = BrowserResultsMotion::new(pair, ScreenInstanceId(9001)).unwrap();
    assert_eq!(owner.identity(), pair);
    let mut scene = Scene::new(960, 720);
    owner
        .request(
            pair,
            node,
            motion(),
            Duration::ZERO,
            &view,
            0,
            false,
            &mut scene,
        )
        .unwrap();
    owner
        .tick(Duration::from_millis(200), [960, 720], &mut scene)
        .unwrap();
    let binding = id(&scene, &owner, node);
    let pose = scene.component_transform(binding);
    let identity = scene.geometry_stamp().0.clone();
    let revision = scene.geometry_stamp().1;
    let nodes = owner.nodes();
    for (identity_pair, target, now, page) in [
        ((pair.0, pair.1 - 1), node, Duration::from_millis(200), 0),
        ((pair.0 - 1, pair.1), node, Duration::from_millis(200), 0),
        (pair, NodeId(usize::MAX), Duration::from_millis(200), 0),
        (pair, node, Duration::from_millis(199), 0),
        (pair, node, Duration::from_millis(200), usize::MAX),
    ] {
        assert!(owner
            .request(
                identity_pair,
                target,
                motion(),
                now,
                &view,
                page,
                false,
                &mut scene
            )
            .is_err());
        assert_eq!(owner.nodes(), nodes);
        assert!(owner.active());
        assert_eq!(scene.component_transform(binding), pose);
        assert!(Arc::ptr_eq(scene.geometry_stamp().0, &identity));
        assert_eq!(scene.geometry_stamp().1, revision);
    }
    assert!(BrowserResultsMotion::new((0, 1), ScreenInstanceId(1)).is_err());
    assert!(BrowserResultsMotion::new((1, 0), ScreenInstanceId(1)).is_err());
    assert!(BrowserResultsMotion::new((1, 1), ScreenInstanceId(0)).is_err());
}

#[test]
fn motion_ticks_preserve_geometry_identity_and_rebind_preserves_pose() {
    let pair = (7, 9);
    let view = view(1);
    let node = view.displayed_nodes(0, false).unwrap()[0];
    let mut owner = BrowserResultsMotion::new(pair, ScreenInstanceId(9002)).unwrap();
    let mut scene = Scene::new(960, 720);
    owner
        .request(
            pair,
            node,
            motion(),
            Duration::ZERO,
            &view,
            0,
            false,
            &mut scene,
        )
        .unwrap();
    let binding = id(&scene, &owner, node);
    let identity = scene.geometry_stamp().0.clone();
    let revision = scene.geometry_stamp().1;
    for ms in [250, 500, 750, 1000] {
        owner
            .tick(Duration::from_millis(ms), [960, 720], &mut scene)
            .unwrap();
        assert!(Arc::ptr_eq(scene.geometry_stamp().0, &identity));
        assert_eq!(scene.geometry_stamp().1, revision);
        assert_eq!(id(&scene, &owner, node), binding);
    }
    assert!(!owner.active());
    let pose = scene.component_transform(binding).unwrap();
    assert_eq!(pose.offset(), [40.0, 10.0]);
    let mut rebuilt = Scene::new(960, 720);
    view.compose_components_mode(&mut rebuilt, owner.screen(), 0, false, &owner.nodes())
        .unwrap();
    owner.restore(&mut rebuilt).unwrap();
    assert_eq!(
        rebuilt.component_transform(id(&rebuilt, &owner, node)),
        Some(pose)
    );
}

#[test]
fn page_pruning_cancels_absent_cards_and_preserves_surviving_scope() {
    let pair = (7, 9);
    let view = view(5);
    let first = view.displayed_nodes(0, false).unwrap();
    let last = view.displayed_nodes(1, false).unwrap();
    let absent = *first.iter().find(|node| !last.contains(node)).unwrap();
    let scope = first[0];
    assert!(last.contains(&scope));
    let mut owner = BrowserResultsMotion::new(pair, ScreenInstanceId(9003)).unwrap();
    let mut scene = Scene::new(960, 720);
    for node in [scope, absent] {
        owner
            .request(
                pair,
                node,
                motion(),
                Duration::ZERO,
                &view,
                0,
                false,
                &mut scene,
            )
            .unwrap();
    }
    owner
        .tick(Duration::from_millis(250), [960, 720], &mut scene)
        .unwrap();
    let saved = scene
        .component_transform(id(&scene, &owner, scope))
        .unwrap();
    assert!(owner.prune(&view, 1, false).unwrap());
    assert!(owner.nodes().contains(&scope));
    assert!(!owner.nodes().contains(&absent));
    assert!(owner.active());
    scene.clear();
    view.compose_components_mode(&mut scene, owner.screen(), 1, false, &owner.nodes())
        .unwrap();
    owner.restore(&mut scene).unwrap();
    assert_eq!(
        scene.component_transform(id(&scene, &owner, scope)),
        Some(saved)
    );
    let nodes = owner.nodes();
    assert!(owner
        .request(
            pair,
            absent,
            motion(),
            Duration::from_millis(250),
            &view,
            1,
            false,
            &mut scene
        )
        .is_err());
    assert_eq!(owner.nodes(), nodes);
    owner
        .tick(Duration::from_millis(1000), [960, 720], &mut scene)
        .unwrap();
    assert!(!owner.active());
}

#[test]
fn suspension_excludes_hidden_elapsed_time_and_disposal_rejects_reuse() {
    let pair = (7, 9);
    let view = view(1);
    let node = view.displayed_nodes(0, false).unwrap()[0];
    let mut owner = BrowserResultsMotion::new(pair, ScreenInstanceId(9004)).unwrap();
    let mut scene = Scene::new(960, 720);
    owner
        .request(
            pair,
            node,
            motion(),
            Duration::ZERO,
            &view,
            0,
            false,
            &mut scene,
        )
        .unwrap();
    owner
        .tick(Duration::from_millis(200), [960, 720], &mut scene)
        .unwrap();
    let binding = id(&scene, &owner, node);
    let saved = scene.component_transform(binding);
    owner.suspend(Duration::from_millis(200)).unwrap();
    owner
        .tick(Duration::from_millis(5000), [0, 0], &mut scene)
        .unwrap();
    assert_eq!(scene.component_transform(binding), saved);
    assert!(owner
        .request(
            pair,
            node,
            motion(),
            Duration::from_millis(5000),
            &view,
            0,
            false,
            &mut scene
        )
        .is_err());
    owner.resume(Duration::from_millis(5000)).unwrap();
    owner
        .tick(Duration::from_millis(5100), [960, 720], &mut scene)
        .unwrap();
    assert!(owner.active());
    assert_eq!(
        scene.component_transform(binding).unwrap().offset(),
        [12.0, 3.0]
    );
    owner.dispose();
    assert!(!owner.active());
    assert!(owner.validate_time(Duration::from_millis(5100)).is_err());
    assert!(owner
        .request(
            pair,
            node,
            motion(),
            Duration::from_millis(5100),
            &view,
            0,
            false,
            &mut scene
        )
        .is_err());
}

#[test]
fn rejected_scene_refuses_without_partial_owner_or_geometry_publish() {
    let pair = (7, 9);
    let view = view(1);
    let node = view.displayed_nodes(0, false).unwrap()[0];
    let mut owner = BrowserResultsMotion::new(pair, ScreenInstanceId(9005)).unwrap();
    let mut scene = Scene::new(960, 720);
    // A pre-existing invalid scene must refuse the cold prefix capture, before publication.
    scene.reject("deliberate rejected renderer candidate".into());
    let identity = scene.geometry_stamp().0.clone();
    let revision = scene.geometry_stamp().1;
    assert!(owner
        .request(
            pair,
            node,
            motion(),
            Duration::ZERO,
            &view,
            0,
            false,
            &mut scene
        )
        .is_err());
    assert!(owner.nodes().is_empty());
    assert!(!owner.active());
    assert!(Arc::ptr_eq(scene.geometry_stamp().0, &identity));
    assert_eq!(scene.geometry_stamp().1, revision);
    assert!(scene.status().is_err());
}
