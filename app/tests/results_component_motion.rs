#![cfg(feature = "graphics")]

use beatkernel::{
    audio::{command_queue, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank},
    input::BindingMap,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms_runtime::{
    competition::{OpponentKind, ScoreSummary},
    competition_presentation::{CompetitionSnapshot, GhostSnapshot},
    local_players::PlayerId,
    play_result::CompletedPlayResult,
    scene::{Scene, UiComponentKey, UiTransform},
    screen_lifecycle::ScreenInstanceId,
    step_gameplay::{StepGameplay, StepGameplayConfig},
    ui::{
        layout::{LayoutChange, LayoutUpdate, NodeId},
        results::{FrozenResultsView, ResultDetails, ResultsView},
    },
    PreparedBms,
};
use std::sync::Arc;

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

// Obtain immutable completion through a real gameplay owner and two observed
// mixer drains. Frozen presentation data never substitutes for this evidence.
fn completed() -> CompletedPlayResult {
    let source = beatkernel_bms::parse("#BPM 120\n", Default::default()).unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let prepared = PreparedBms {
        compiled: source.compile().unwrap(),
        source,
        bank: SampleBank::new(format, PcmLimits::new(64, 256, 1).unwrap()).unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    };
    let config = StepGameplayConfig {
        host_origin: point(1, 0),
        output_origin: point(2, 0),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 8,
        bgm_pending: 4,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
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
    assert!(owner.completed_result().is_none());
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
    *owner.completed_result().unwrap()
}

enum View {
    Genuine(ResultsView),
    Frozen(FrozenResultsView),
}
impl View {
    fn nodes(&self, page: usize, comparisons: bool) -> Result<Vec<NodeId>, String> {
        match self {
            Self::Genuine(v) => v.displayed_nodes(page, comparisons),
            Self::Frozen(v) => v.displayed_nodes(page, comparisons),
        }
    }
    fn compose(
        &self,
        scene: &mut Scene,
        owner: ScreenInstanceId,
        page: usize,
        comparisons: bool,
        nodes: &[NodeId],
    ) -> Result<(), String> {
        match self {
            Self::Genuine(v) => v.compose_components_mode(scene, owner, page, comparisons, nodes),
            Self::Frozen(v) => v.compose_components_mode(scene, owner, page, comparisons, nodes),
        }
    }
    fn resize(&mut self, width: u32, height: u32) {
        match self {
            Self::Genuine(v) => v.resize(width, height),
            Self::Frozen(v) => v.resize(width, height),
        }
        .unwrap();
    }
    fn update(&mut self, updates: &[LayoutUpdate]) {
        match self {
            Self::Genuine(v) => v.update_layout(updates),
            Self::Frozen(v) => v.update_layout(updates),
        }
        .unwrap();
    }
}

fn views(count: usize, comparisons: bool) -> [View; 2] {
    let roster: Vec<_> = (1..=count).map(|id| PlayerId(id as u32)).collect();
    let result = completed();
    let rows: Vec<_> = roster.iter().map(|id| (*id, result)).collect();
    let genuine = if comparisons {
        let score = ScoreSummary::default();
        let comparison = CompetitionSnapshot {
            ghosts: (0..if count == 1 { 5 } else { 1 })
                .map(|index| GhostSnapshot {
                    kind: OpponentKind::Own,
                    label: format!("past-{index}.bkr"),
                    hits: 0,
                    misses: 0,
                    combo: 0,
                    max_combo: 0,
                    recorded_until: Some(Timestamp::ZERO),
                })
                .collect(),
            network: None,
        };
        let details: Vec<_> = roster
            .iter()
            .map(|player| ResultDetails {
                player: *player,
                score: &score,
                competition: Some(&comparison),
            })
            .collect();
        ResultsView::new_with_details(&rows, &roster, &details).unwrap()
    } else {
        ResultsView::new(&rows, &roster).unwrap()
    };
    let frozen = FrozenResultsView::from_model(genuine.export_visual().unwrap()).unwrap();
    [View::Genuine(genuine), View::Frozen(frozen)]
}

#[test]
fn comparison_only_cards_are_not_admitted_in_the_score_mode() {
    for view in views(1, true) {
        let score_nodes = view.nodes(0, false).unwrap();
        let comparison_nodes = view.nodes(0, true).unwrap();
        assert_eq!(score_nodes.len(), 3);
        assert_eq!(comparison_nodes.len(), 6);
        assert!(view.nodes(1, false).is_err());
        assert_eq!(view.nodes(1, true).unwrap().len(), 3);
        let owner = ScreenInstanceId(506);
        let mut scene = Scene::new(960, 720);
        view.compose(&mut scene, owner, 0, true, &[comparison_nodes[2]])
            .unwrap();
        let binding = scene
            .component_id(UiComponentKey {
                screen: owner,
                node: comparison_nodes[2],
            })
            .unwrap();
        view.compose(&mut scene, owner, 0, false, &[score_nodes[1]])
            .unwrap();
        assert!(scene
            .set_component_transforms(&[(binding, UiTransform::default())])
            .is_err());
        assert!(view
            .compose(&mut scene, owner, 0, false, &[comparison_nodes[2]])
            .is_err());
    }
}

#[test]
fn genuine_and_frozen_targets_follow_partial_pages_and_comparison_fallback() {
    for comparisons in [false, true] {
        for view in views(5, comparisons) {
            for mode in [false, true] {
                let full = view.nodes(0, mode).unwrap();
                let partial = view.nodes(1, mode).unwrap();
                assert_eq!(full.len(), 6);
                assert_eq!(partial.len(), 3);
                assert_eq!(partial, vec![full[0], full[1], full[5]]);
                assert!(view.nodes(2, mode).is_err());
                assert!(view.nodes(usize::MAX, mode).is_err());
            }
        }
    }
}

#[test]
fn motion_ticks_reuse_geometry_and_preserve_readonly_result_identity() {
    for view in views(1, false) {
        let owner = ScreenInstanceId(501);
        let nodes = view.nodes(0, false).unwrap();
        let card = nodes[1];
        let mut scene = Scene::new(960, 720);
        scene.rect(1, 1, 10, 10, 0xffffff); // Ordinary caller header precedes Results.
        view.compose(&mut scene, owner, 0, false, &[card]).unwrap();
        let key = UiComponentKey {
            screen: owner,
            node: card,
        };
        let component = scene.component_id(key).unwrap();
        let identity = scene.geometry_stamp().0.clone();
        let revision = scene.geometry_stamp().1;
        for offset in [0.5, 35.0, -12.0] {
            let transform = UiTransform::new([offset, 0.0], [1.0, 1.0], 0.75).unwrap();
            scene
                .set_component_transforms(&[(component, transform)])
                .unwrap();
            view.compose(&mut scene, owner, 0, false, &[card]).unwrap();
            assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
            assert_eq!(revision, scene.geometry_stamp().1);
            assert_eq!(scene.component_transform(component), Some(transform));
            assert_eq!(view.nodes(0, false).unwrap(), nodes);
        }
    }
}

#[test]
fn invalid_targets_refuse_without_changing_geometry_or_existing_pose() {
    for view in views(5, false) {
        let owner = ScreenInstanceId(502);
        let full = view.nodes(0, false).unwrap();
        let mut scene = Scene::new(960, 720);
        view.compose(&mut scene, owner, 1, false, &[full[1]])
            .unwrap();
        let component = scene
            .component_id(UiComponentKey {
                screen: owner,
                node: full[1],
            })
            .unwrap();
        let transform = UiTransform::new([25.0, 0.0], [1.0, 1.0], 1.0).unwrap();
        scene
            .set_component_transforms(&[(component, transform)])
            .unwrap();
        let identity = scene.geometry_stamp().0.clone();
        let revision = scene.geometry_stamp().1;
        for (bad_owner, page, targets) in [
            (owner, 1, vec![full[1], full[1]]),
            (owner, 1, vec![full[2]]), // Slot absent on the partial page.
            (owner, 1, vec![NodeId(usize::MAX)]),
            (owner, usize::MAX, vec![full[1]]),
            (ScreenInstanceId(0), 1, vec![full[1]]),
            (ScreenInstanceId(999), 1, vec![full[1]]),
        ] {
            assert!(view
                .compose(&mut scene, bad_owner, page, false, &targets)
                .is_err());
            assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
            assert_eq!(revision, scene.geometry_stamp().1);
            assert_eq!(scene.component_transform(component), Some(transform));
        }
    }
}

#[test]
fn page_replacement_retires_absent_card_bindings() {
    for view in views(5, false) {
        let owner = ScreenInstanceId(503);
        let nodes = view.nodes(0, false).unwrap();
        let mut scene = Scene::new(960, 720);
        view.compose(&mut scene, owner, 0, false, &nodes).unwrap();
        let absent = scene
            .component_id(UiComponentKey {
                screen: owner,
                node: nodes[2],
            })
            .unwrap();
        let partial = view.nodes(1, false).unwrap();
        view.compose(&mut scene, owner, 1, false, &partial).unwrap();
        assert!(scene
            .component_id(UiComponentKey {
                screen: owner,
                node: nodes[2]
            })
            .is_none());
        assert!(scene
            .set_component_transforms(&[(absent, UiTransform::default())])
            .is_err());
        for node in partial {
            assert!(scene
                .component_id(UiComponentKey {
                    screen: owner,
                    node
                })
                .is_some());
        }
    }
}

#[test]
fn local_card_source_can_move_back_inside_fixed_ancestor_clip() {
    for mut view in views(2, false) {
        let owner = ScreenInstanceId(504);
        let nodes = view.nodes(0, false).unwrap();
        let card = nodes[2];
        // Column-flow children cannot set an origin. Growing the preceding
        // card moves this card to y=570, beyond the fixed parent y=140..570.
        view.update(&[LayoutUpdate {
            id: nodes[1],
            change: LayoutChange::Size([912, 420]),
        }]);
        let mut scene = Scene::new(960, 720);
        view.compose(&mut scene, owner, 0, false, &[card]).unwrap();
        let key = UiComponentKey {
            screen: owner,
            node: card,
        };
        let component = scene.component_id(key).unwrap();
        assert_eq!(
            scene
                .presented_pose()
                .visible_rect(Some(key), [24.0, 590.0, 34.0, 600.0]),
            None
        );
        scene
            .set_component_transforms(&[(
                component,
                UiTransform::new([0.0, -100.0], [1.0, 1.0], 1.0).unwrap(),
            )])
            .unwrap();
        assert_eq!(
            scene
                .presented_pose()
                .visible_rect(Some(key), [24.0, 590.0, 34.0, 600.0]),
            Some([24.0, 490.0, 34.0, 500.0])
        );
        // A source rectangle outside the node remains excluded after motion.
        assert_eq!(
            scene
                .presented_pose()
                .visible_rect(Some(key), [24.0, 700.0, 34.0, 710.0]),
            None
        );
    }
}

#[test]
fn resize_rebinds_component_geometry_and_suspension_hides_targets() {
    for mut view in views(1, false) {
        let owner = ScreenInstanceId(505);
        let nodes = view.nodes(0, false).unwrap();
        let mut scene = Scene::new(960, 720);
        view.compose(&mut scene, owner, 0, false, &[nodes[1]])
            .unwrap();
        view.resize(480, 360);
        let mut resized = Scene::new(480, 360);
        view.compose(&mut resized, owner, 0, false, &[nodes[1]])
            .unwrap();
        assert_eq!(resized.logical_extent(), [480, 360]);
        assert!(resized
            .component_id(UiComponentKey {
                screen: owner,
                node: nodes[1]
            })
            .is_some());
        assert_eq!(view.nodes(0, false).unwrap(), nodes);
        view.resize(0, 0);
        assert!(view.nodes(0, false).unwrap().is_empty());
        view.resize(960, 720);
        assert_eq!(view.nodes(0, false).unwrap(), nodes);
    }
}
