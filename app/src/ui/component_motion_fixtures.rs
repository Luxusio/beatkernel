//! Real component slots and retained packets driven by explicit lifecycle time.
use super::*;
use crate::{
    scene::{ClipRect, Scene, UiComponentKey, UiTransform},
    screen_lifecycle::ScreenInstanceId,
    ui::{
        interaction::{Bounds, ControlId},
        layout::{LayoutChange, LayoutGeometry, LayoutUpdate, MountedLayout, Node, NodeId},
        retained::RetainedNodes,
    },
};

#[test]
fn moving_source_clip_and_fixed_ancestor_clip_admit_the_same_half_open_edges() {
    let owner = ScreenInstanceId(91);
    let children = [Node::leaf([40, 20], 1u8).at(10, 10)];
    let mut layout = MountedLayout::mount(Node::layer([100, 60], &children)).unwrap();
    layout
        .update(&[
            LayoutUpdate {
                id: NodeId(0),
                change: LayoutChange::Clip(Some(Bounds {
                    x: 0,
                    y: 0,
                    width: 55,
                    height: 45,
                })),
            },
            LayoutUpdate {
                id: NodeId(1),
                change: LayoutChange::Clip(Some(Bounds {
                    x: 5,
                    y: 5,
                    width: 20,
                    height: 10,
                })),
            },
        ])
        .unwrap();
    let mut retained = RetainedNodes::new(100, 60).unwrap();
    retained
        .static_layout_node(&layout, &[NodeId(1)], paint)
        .unwrap();
    let mut scene = Scene::with_capacity(100, 60, 8);
    let mut hits = Vec::new();
    retained
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    let key = UiComponentKey {
        screen: owner,
        node: NodeId(1),
    };
    let id = scene.component_id(key).unwrap();
    let epoch = scene.geometry_stamp().1;
    let packet = scene.rectangles().as_ptr();
    let transform = UiTransform::new([20.0, 5.0], [2.0, 2.0], 0.5).unwrap();
    scene.set_component_transforms(&[(id, transform)]).unwrap();
    let uniform = scene.component_gpu_uniform(scene.batches()[0].component as usize);
    assert_eq!(&uniform[4..7], &[10.0, 10.0, 0.5]);
    assert_eq!(&uniform[8..12], &[0.0, 0.0, 55.0, 45.0]);
    assert_eq!(&uniform[12..16], &[15.0, 15.0, 35.0, 25.0]);
    // Source edges move to [40,80) x [25,45); the fixed ancestor
    // cuts the painted/hittable part to [40,55) x [25,45).
    for point in [(40.0, 25.0), (54.5, 44.5)] {
        let local = scene.project_component_point(id, point).unwrap();
        assert!(local.0 >= 15.0 && local.0 < 35.0);
        assert!(local.1 >= 15.0 && local.1 < 25.0);
        assert_eq!(
            retained.hit_components(&scene, owner, point),
            Some(ControlId(1))
        );
    }
    for point in [(39.5, 30.0), (55.0, 30.0), (45.0, 24.5), (45.0, 45.0)] {
        assert_eq!(scene.project_component_point(id, point), None);
        assert_eq!(retained.hit_components(&scene, owner, point), None);
    }
    scene
        .set_component_transforms(&[(id, UiTransform::new([20.0, 5.0], [2.0, 2.0], 0.0).unwrap())])
        .unwrap();
    assert_eq!(retained.hit_components(&scene, owner, (45.0, 30.0)), None);
    assert_eq!(scene.geometry_stamp().1, epoch);
    assert_eq!(scene.rectangles().as_ptr(), packet);
}

fn component(
    scene: &mut Scene,
    screen: ScreenInstanceId,
    node: NodeId,
    x: i64,
) -> (UiComponentKey, crate::scene::UiComponentId) {
    let first = scene.rectangles().len() as u32;
    scene.rect(x, 10, 10, 10, 0xabcdef);
    let key = UiComponentKey { screen, node };
    let id = scene
        .bind_component(
            key,
            &[first..first + 1],
            ClipRect::new([x, 10, 10, 10]).unwrap(),
            Some(ClipRect::new([0, 0, 200, 100]).unwrap()),
        )
        .unwrap();
    (key, id)
}

fn motion(duration: u64) -> ComponentMotion {
    ComponentMotion::new(
        UiTransform::default(),
        UiTransform::new([8.0, -4.0], [2.0, 1.0], 0.5).unwrap(),
        Duration::from_nanos(duration),
        Easing::Linear,
    )
}

#[test]
fn all_sixty_four_scheduler_bindings_rebind_atomically_without_resetting_elapsed_pose() {
    let owner = ScreenInstanceId(92);
    let mut scene = Scene::with_capacity(200, 100, 128);
    let mut scheduler = MotionScheduler::new(owner, 64).unwrap();
    let mut original = Vec::new();
    for index in 0..64 {
        let (key, id) = component(&mut scene, owner, NodeId(index), 10);
        scheduler
            .schedule(key, id, motion(100), Duration::ZERO)
            .unwrap();
        original.push((key, id));
    }
    scheduler
        .tick(owner, Duration::from_nanos(25), &mut scene)
        .unwrap();
    let pose = scene.component_transform(original[0].1).unwrap();
    scheduler.suspend(Duration::from_nanos(25)).unwrap();
    assert_eq!(scene.dispose_components(owner), 64);
    // Another screen can consume the entire renderer binding budget while
    // the retained parent scheduler owns its tracks but no live slots.
    let other = ScreenInstanceId(93);
    for index in 0..64 {
        let (_, id) = component(&mut scene, other, NodeId(index), 10);
        assert!(scene.component_transform(id).is_some());
    }
    assert_eq!(scene.dispose_components(other), 64);
    let clip = ClipRect::new([0, 0, 200, 100]).unwrap();
    let mut rebound = Vec::new();
    for (index, (key, old_id)) in original.iter().enumerate() {
        let id = scene
            .bind_component(
                *key,
                &[index as u32..index as u32 + 1],
                ClipRect::new([10, 10, 10, 10]).unwrap(),
                Some(clip),
            )
            .unwrap();
        assert_ne!(id, *old_id);
        rebound.push(id);
    }
    scheduler.rebind(&scene).unwrap();
    assert_eq!(scheduler.active_count(), 64);
    scene
        .set_component_transforms(&rebound.iter().map(|id| (*id, pose)).collect::<Vec<_>>())
        .unwrap();
    scheduler.resume(Duration::from_nanos(1000)).unwrap();
    assert!(!scheduler
        .tick(owner, Duration::from_nanos(1000), &mut scene)
        .unwrap());
    for id in &rebound {
        assert_eq!(scene.component_transform(*id), Some(pose));
    }
    scheduler
        .tick(owner, Duration::from_nanos(1025), &mut scene)
        .unwrap();
    for id in &rebound {
        assert_eq!(
            scene.component_transform(*id).unwrap().offset(),
            [4.0, -2.0]
        );
    }
    // A late missing key cannot partially change earlier admitted IDs.
    scene.dispose_components(owner);
    for (index, (key, _)) in original[..63].iter().enumerate() {
        scene
            .bind_component(
                *key,
                &[index as u32..index as u32 + 1],
                ClipRect::new([10, 10, 10, 10]).unwrap(),
                Some(clip),
            )
            .unwrap();
    }
    assert!(scheduler.rebind(&scene).is_err());
    assert_eq!(scheduler.active_count(), 64);
    assert!(scheduler
        .tick(owner, Duration::from_nanos(1050), &mut scene)
        .is_err());
    for (key, _) in &original[..63] {
        assert_eq!(
            scene.component_transform(scene.component_id(*key).unwrap()),
            Some(UiTransform::default())
        );
    }
}

#[test]
fn explicit_scheduler_freezes_suspended_elapsed_and_cancel_dispose_fence_every_later_tick() {
    let owner = ScreenInstanceId(41);
    let mut scene = Scene::with_capacity(200, 100, 8);
    let (key, id) = component(&mut scene, owner, NodeId(7), 10);
    let mut scheduler = MotionScheduler::new(owner, 2).unwrap();
    scheduler
        .schedule(key, id, motion(100), Duration::ZERO)
        .unwrap();
    assert_eq!(scheduler.active_count(), 1);
    assert!(scheduler
        .tick(owner, Duration::from_nanos(25), &mut scene)
        .unwrap());
    let quarter = scene.component_transform(id).unwrap();
    assert_eq!(quarter.offset(), [2.0, -1.0]);
    assert_eq!(quarter.opacity(), 0.875);
    scheduler.suspend(Duration::from_nanos(25)).unwrap();
    assert!(scheduler.resume(Duration::from_nanos(24)).is_err());
    assert!(scheduler
        .schedule(key, id, motion(1), Duration::from_nanos(25))
        .is_err());
    assert!(!scheduler
        .tick(owner, Duration::from_nanos(1000), &mut scene)
        .unwrap());
    assert_eq!(scene.component_transform(id), Some(quarter));
    scheduler.resume(Duration::from_nanos(1000)).unwrap();
    assert!(scheduler
        .tick(owner, Duration::from_nanos(1025), &mut scene)
        .unwrap());
    assert_eq!(scene.component_transform(id).unwrap().offset(), [4.0, -2.0]);
    assert!(scheduler.cancel(NodeId(7)));
    assert!(!scheduler.cancel(NodeId(7)));
    let cancelled = scene.component_transform(id);
    assert!(!scheduler
        .tick(owner, Duration::from_nanos(1100), &mut scene)
        .unwrap());
    assert_eq!(scene.component_transform(id), cancelled);
    scheduler
        .schedule(key, id, motion(100), Duration::from_nanos(1100))
        .unwrap();
    assert_eq!(scheduler.dispose(), 1);
    assert!(scheduler.disposed());
    assert_eq!(scheduler.active_count(), 0);
    assert_eq!(scheduler.dispose(), 0);
    assert!(scheduler
        .tick(owner, Duration::from_nanos(1200), &mut scene)
        .is_err());
    assert!(scheduler
        .schedule(key, id, motion(1), Duration::from_nanos(1200))
        .is_err());
    assert_eq!(scene.component_transform(id), cancelled);
}

#[test]
fn cold_scheduler_refusal_preserves_tracks_and_atomic_tick_never_publishes_a_valid_prefix() {
    let owner = ScreenInstanceId(42);
    let mut scene = Scene::with_capacity(200, 100, 8);
    let (first_key, first_id) = component(&mut scene, owner, NodeId(1), 10);
    let (second_key, second_id) = component(&mut scene, ScreenInstanceId(99), NodeId(2), 30);
    let mut scheduler = MotionScheduler::new(owner, 1).unwrap();
    scheduler
        .schedule(first_key, first_id, motion(100), Duration::ZERO)
        .unwrap();
    assert!(scheduler
        .schedule(second_key, second_id, motion(100), Duration::ZERO)
        .is_err());
    assert_eq!(scheduler.active_count(), 1);
    assert!(scheduler
        .tick(ScreenInstanceId(43), Duration::from_nanos(25), &mut scene)
        .is_err());
    assert_eq!(
        scene.component_transform(first_id),
        Some(UiTransform::default())
    );
    assert!(scheduler
        .tick(owner, Duration::from_nanos(25), &mut scene)
        .unwrap());
    let accepted = scene.component_transform(first_id);
    assert!(scheduler
        .tick(owner, Duration::from_nanos(24), &mut scene)
        .is_err());
    assert!(scheduler
        .schedule(first_key, first_id, motion(1), Duration::from_nanos(24))
        .is_err());
    assert_eq!(scene.component_transform(first_id), accepted);
    assert_eq!(scheduler.active_count(), 1);
    // A capacity refusal cannot replace the already admitted motion.
    let (third_key, third_id) = component(&mut scene, owner, NodeId(3), 50);
    assert!(scheduler
        .schedule(third_key, third_id, motion(1), Duration::from_nanos(25))
        .is_err());
    assert!(scheduler
        .tick(owner, Duration::from_nanos(50), &mut scene)
        .unwrap());
    assert_eq!(
        scene.component_transform(first_id).unwrap().offset(),
        [4.0, -2.0]
    );

    let mut stale_scene = Scene::with_capacity(200, 100, 8);
    let (good_key, good_id) = component(&mut stale_scene, owner, NodeId(4), 10);
    let (stale_key, stale_id) = component(&mut stale_scene, owner, NodeId(5), 30);
    let mut batch = MotionScheduler::new(owner, 2).unwrap();
    batch
        .schedule(good_key, good_id, motion(10), Duration::ZERO)
        .unwrap();
    // Schedule cannot inspect Scene; destruction before tick must refuse the
    // whole batch, including an earlier valid track that would now complete.
    batch
        .schedule(stale_key, stale_id, motion(10), Duration::ZERO)
        .unwrap();
    stale_scene.dispose_components(owner);
    let clip = ClipRect::new([0, 0, 200, 100]).unwrap();
    let good_id = stale_scene
        .bind_component(
            good_key,
            &[0..1],
            ClipRect::new([10, 10, 10, 10]).unwrap(),
            Some(clip),
        )
        .unwrap();
    batch
        .schedule(good_key, good_id, motion(10), Duration::ZERO)
        .unwrap();
    assert!(batch
        .tick(owner, Duration::from_nanos(10), &mut stale_scene)
        .is_err());
    assert_eq!(
        stale_scene.component_transform(good_id),
        Some(UiTransform::default())
    );
    assert_eq!(batch.active_count(), 2);
}

#[test]
fn same_node_replacement_and_zero_duration_complete_exactly_without_duplicate_tracks() {
    let owner = ScreenInstanceId(43);
    let mut scene = Scene::with_capacity(200, 100, 4);
    let (key, id) = component(&mut scene, owner, NodeId(1), 10);
    let mut scheduler = MotionScheduler::new(owner, 1).unwrap();
    scheduler
        .schedule(key, id, motion(100), Duration::ZERO)
        .unwrap();
    scheduler
        .tick(owner, Duration::from_nanos(25), &mut scene)
        .unwrap();
    let quarter = scene.component_transform(id).unwrap();
    let destination = UiTransform::new([10.0, 3.0], [1.0, 1.0], 1.0).unwrap();
    scheduler
        .schedule(
            key,
            id,
            ComponentMotion::new(
                quarter,
                destination,
                Duration::from_nanos(100),
                Easing::Linear,
            ),
            Duration::from_nanos(25),
        )
        .unwrap();
    assert_eq!(scheduler.active_count(), 1);
    assert!(!scheduler
        .tick(owner, Duration::from_nanos(25), &mut scene)
        .unwrap());
    scheduler
        .tick(owner, Duration::from_nanos(75), &mut scene)
        .unwrap();
    let halfway = scene.component_transform(id).unwrap();
    assert_eq!(halfway.offset(), [6.0, 1.0]);
    assert_eq!(halfway.opacity(), 0.9375);
    scheduler
        .schedule(
            key,
            id,
            ComponentMotion::new(halfway, destination, Duration::ZERO, Easing::EaseOut),
            Duration::from_nanos(75),
        )
        .unwrap();
    assert!(scheduler
        .tick(owner, Duration::from_nanos(75), &mut scene)
        .unwrap());
    assert_eq!(scene.component_transform(id), Some(destination));
    assert_eq!(scheduler.active_count(), 0);
    assert!(!scheduler
        .tick(owner, Duration::from_nanos(76), &mut scene)
        .unwrap());
}

#[test]
fn sixty_four_active_tracks_bound_animation_without_bounding_unanimated_layout_nodes() {
    let owner = ScreenInstanceId(44);
    let children: Vec<_> = (0..100u16).map(|index| Node::leaf([1, 1], index)).collect();
    let layout = MountedLayout::mount(Node::column([2, 100], 0, &children)).unwrap();
    let mut nodes = RetainedNodes::new(2, 100).unwrap();
    for leaf in layout.leaves() {
        nodes
            .static_layout_node(&layout, &[leaf.id], paint)
            .unwrap();
    }
    let mut scene = Scene::with_capacity(2, 100, 128);
    let mut hits = Vec::new();
    let animated: Vec<_> = layout.leaves()[..64].iter().map(|leaf| leaf.id).collect();
    nodes
        .compose_components(&mut scene, &mut hits, owner, &animated)
        .unwrap();
    assert_eq!(scene.rectangles().len(), 100);
    assert_eq!(hits.len(), 100);
    let identities = nodes.identities();
    let paints = nodes.paints();
    let stamp = scene.geometry_stamp().1;
    let too_many: Vec<_> = layout.leaves()[..65].iter().map(|leaf| leaf.id).collect();
    assert!(nodes
        .compose_components(&mut scene, &mut hits, owner, &too_many)
        .is_err());
    assert_eq!(scene.geometry_stamp().1, stamp);
    assert_eq!(scene.rectangles().len(), 100);
    let mut scheduler = MotionScheduler::new(owner, 64).unwrap();
    for node in animated {
        let key = UiComponentKey {
            screen: owner,
            node,
        };
        scheduler
            .schedule(
                key,
                scene.component_id(key).unwrap(),
                motion(100),
                Duration::ZERO,
            )
            .unwrap();
    }
    assert_eq!(scheduler.active_count(), 64);
    assert!(scheduler
        .tick(owner, Duration::from_nanos(25), &mut scene)
        .unwrap());
    assert_eq!(scene.geometry_stamp().1, stamp);
    assert_eq!(nodes.identities(), identities);
    assert_eq!(nodes.paints(), paints);
    assert_eq!(scene.rectangles().len(), 100);
    assert_eq!(hits.len(), 100);
    assert!(scheduler
        .tick(owner, Duration::from_nanos(100), &mut scene)
        .unwrap());
    assert_eq!(scheduler.active_count(), 0);
    assert!(!scheduler
        .tick(owner, Duration::from_nanos(101), &mut scene)
        .unwrap());
    for cap in [0, 65, usize::MAX] {
        assert!(MotionScheduler::new(owner, cap).is_err());
    }
    assert!(MotionScheduler::new(ScreenInstanceId(0), 1).is_err());
}

fn paint(
    id: NodeId,
    geometry: LayoutGeometry,
    scene: &mut Scene,
    hits: &mut Vec<(ControlId, Bounds)>,
) {
    let bounds = geometry.bounds;
    scene.rect(bounds.x, bounds.y, bounds.width, bounds.height, 0xabcdef);
    hits.push((ControlId(id.0 as u64), bounds));
}

#[test]
fn retained_component_parts_keep_painter_order_inverse_hit_clips_and_unchanged_packets() {
    let owner = ScreenInstanceId(45);
    let children = [
        Node::leaf([20, 10], 1u8).at(10, 10),
        Node::leaf([20, 10], 2).at(35, 10),
    ];
    let layout = MountedLayout::mount(Node::layer([80, 40], &children)).unwrap();
    let mut nodes = RetainedNodes::new(80, 40).unwrap();
    nodes
        .static_layout_node(&layout, &[NodeId(1), NodeId(2)], paint)
        .unwrap();
    let identities = nodes.identities();
    let paints = nodes.paints();
    let mut scene = Scene::with_capacity(80, 40, 8);
    let mut hits = Vec::new();
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    let key = UiComponentKey {
        screen: owner,
        node: NodeId(1),
    };
    let id = scene.component_id(key).unwrap();
    let stamp = scene.geometry_stamp().1;
    scene
        .set_component_transforms(&[(id, UiTransform::new([12.5, 0.0], [1.0, 1.0], 0.5).unwrap())])
        .unwrap();
    assert_eq!(
        nodes.hit_components(&scene, owner, (24.0, 12.0)),
        Some(ControlId(1))
    );
    assert_eq!(
        nodes.hit_components(&scene, owner, (40.0, 12.0)),
        Some(ControlId(2)),
        "later unanimated component wins overlapping painter order"
    );
    assert_eq!(nodes.hit_components(&scene, owner, (10.0, 12.0)), None);
    assert_eq!(
        nodes.hit_components(&scene, ScreenInstanceId(46), (24.0, 12.0)),
        None
    );
    assert_eq!(scene.geometry_stamp().1, stamp);
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    assert_eq!(scene.geometry_stamp().1, stamp);
    assert_eq!(nodes.identities(), identities);
    assert_eq!(nodes.paints(), paints);
    assert_eq!(scene.component_id(key), Some(id));
    assert_eq!(scene.rectangles()[0].bounds, [10.0, 10.0, 20.0, 10.0]);
    assert_eq!(scene.rectangles()[1].bounds, [35.0, 10.0, 20.0, 10.0]);
    scene
        .set_component_transforms(&[(id, UiTransform::new([12.5, 0.0], [1.0, 1.0], 0.0).unwrap())])
        .unwrap();
    assert_eq!(nodes.hit_components(&scene, owner, (24.0, 12.0)), None);
}

#[test]
fn repeated_packet_parts_bind_one_node_but_missing_duplicate_and_ambiguous_geometry_refuse() {
    let owner = ScreenInstanceId(47);
    let children = [Node::leaf([10, 10], 1u8).at(5, 5)];
    let layout = MountedLayout::mount(Node::layer([40, 40], &children)).unwrap();
    let mut nodes = RetainedNodes::new(40, 40).unwrap();
    for _ in 0..2 {
        nodes
            .static_layout_node(&layout, &[NodeId(1)], paint)
            .unwrap();
    }
    let mut scene = Scene::with_capacity(40, 40, 8);
    let mut hits = Vec::new();
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    assert_eq!(scene.rectangles().len(), 2);
    assert!(scene
        .component_id(UiComponentKey {
            screen: owner,
            node: NodeId(1)
        })
        .is_some());
    let stamp = scene.geometry_stamp().1;
    let accepted_hits: Vec<_> = hits
        .iter()
        .map(|(id, b)| (*id, [b.x, b.y, b.width, b.height]))
        .collect();
    for targets in [vec![NodeId(1), NodeId(1)], vec![NodeId(999)]] {
        assert!(nodes
            .compose_components(&mut scene, &mut hits, owner, &targets)
            .is_err());
        assert_eq!(scene.geometry_stamp().1, stamp);
        assert_eq!(scene.rectangles().len(), 2);
        assert_eq!(
            hits.iter()
                .map(|(id, b)| (*id, [b.x, b.y, b.width, b.height]))
                .collect::<Vec<_>>(),
            accepted_hits
        );
    }
    let other_children = [Node::leaf([10, 10], 1u8).at(20, 5)];
    let other = MountedLayout::mount(Node::layer([40, 40], &other_children)).unwrap();
    let mut ambiguous = RetainedNodes::new(40, 40).unwrap();
    ambiguous
        .static_layout_node(&layout, &[NodeId(1)], paint)
        .unwrap();
    ambiguous
        .static_layout_node(&other, &[NodeId(1)], paint)
        .unwrap();
    assert!(ambiguous
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .is_err());
    assert_eq!(scene.geometry_stamp().1, stamp);
    assert_eq!(scene.rectangles().len(), 2);
    assert_eq!(
        hits.iter()
            .map(|(id, b)| (*id, [b.x, b.y, b.width, b.height]))
            .collect::<Vec<_>>(),
        accepted_hits
    );
}

#[test]
fn animated_leaf_moves_beyond_its_old_box_inside_the_actual_parent_without_repainting() {
    let owner = ScreenInstanceId(82);
    let children = [Node::leaf([20, 10], 1u8).at(10, 10)];
    let layout = MountedLayout::mount(Node::layer([100, 50], &children)).unwrap();
    let mut nodes = RetainedNodes::new(100, 50).unwrap();
    nodes
        .static_layout_node(&layout, &[NodeId(1)], paint)
        .unwrap();
    let identities = nodes.identities();
    let paints = nodes.paints();
    let mut scene = Scene::with_capacity(100, 50, 4);
    let mut hits = Vec::new();
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    let id = scene
        .component_id(UiComponentKey {
            screen: owner,
            node: NodeId(1),
        })
        .unwrap();
    let stamp = scene.geometry_stamp().1;
    let pointer = scene.rectangles().as_ptr();
    scene
        .set_component_transforms(&[(id, UiTransform::new([30.5, 0.0], [1.0, 1.0], 0.75).unwrap())])
        .unwrap();
    assert_eq!(
        scene.project_component_point(id, (50.5, 15.0)),
        Some((20.0, 15.0))
    );
    assert_eq!(
        nodes.hit_components(&scene, owner, (50.5, 15.0)),
        Some(ControlId(1))
    );
    assert_eq!(nodes.hit_components(&scene, owner, (60.5, 15.0)), None);
    assert_eq!(nodes.hit_components(&scene, owner, (20.0, 15.0)), None);
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    assert_eq!(scene.geometry_stamp().1, stamp);
    assert_eq!(scene.rectangles().as_ptr(), pointer);
    assert_eq!(nodes.identities(), identities);
    assert_eq!(nodes.paints(), paints);
    assert_eq!(scene.rectangles()[0].bounds, [10.0, 10.0, 20.0, 10.0]);
}

#[test]
fn initially_ancestor_and_viewport_cropped_source_remains_revealable_by_component_motion() {
    for viewport_crop in [false, true] {
        let width = if viewport_crop { 50 } else { 100 };
        let x = if viewport_crop { 40 } else { 50 };
        let children = [Node::leaf([20, 10], 1u8).at(x, 10)];
        let mut layout =
            MountedLayout::mount(Node::layer([width, 50], &children).clipped()).unwrap();
        if !viewport_crop {
            layout
                .update(&[LayoutUpdate {
                    id: NodeId(0),
                    change: LayoutChange::Clip(Some(Bounds {
                        x: 0,
                        y: 0,
                        width: 40,
                        height: 50,
                    })),
                }])
                .unwrap();
        }
        let mut nodes = RetainedNodes::new(width as u32, 50).unwrap();
        nodes
            .static_layout_node(&layout, &[NodeId(1)], paint)
            .unwrap();
        let mut ordinary = Scene::with_capacity(width as u32, 50, 4);
        let mut hits = Vec::new();
        nodes.compose(&mut ordinary, &mut hits).unwrap();
        if viewport_crop {
            assert_eq!(ordinary.rectangles()[0].bounds, [40.0, 10.0, 10.0, 10.0]);
        } else {
            assert!(ordinary.rectangles().is_empty());
            assert!(hits.is_empty());
        }
        let owner = ScreenInstanceId(83);
        let mut scene = Scene::with_capacity(width as u32, 50, 4);
        nodes
            .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
            .unwrap();
        assert_eq!(
            scene.rectangles()[0].bounds,
            [x as f32, 10.0, 20.0, 10.0],
            "cold animated composition must preserve source that the next uniform can reveal"
        );
        let id = scene
            .component_id(UiComponentKey {
                screen: owner,
                node: NodeId(1),
            })
            .unwrap();
        let stamp = scene.geometry_stamp().1;
        let offset = if viewport_crop { -20.0 } else { -30.0 };
        scene
            .set_component_transforms(&[(
                id,
                UiTransform::new([offset, 0.0], [1.0, 1.0], 1.0).unwrap(),
            )])
            .unwrap();
        assert_eq!(
            nodes.hit_components(&scene, owner, (35.0, 15.0)),
            Some(ControlId(1))
        );
        assert_eq!(
            scene.project_component_point(id, (35.0, 15.0)),
            Some((35.0 - f64::from(offset), 15.0))
        );
        assert_eq!(nodes.hit_components(&scene, owner, (40.0, 15.0)), None);
        assert_eq!(scene.geometry_stamp().1, stamp);
        let mut unanimated = Scene::with_capacity(width as u32, 50, 4);
        nodes.compose(&mut unanimated, &mut hits).unwrap();
        assert_eq!(
            unanimated
                .rectangles()
                .iter()
                .map(|r| r.bounds)
                .collect::<Vec<_>>(),
            ordinary
                .rectangles()
                .iter()
                .map(|r| r.bounds)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn repeated_packet_node_with_different_inherited_clip_refuses_without_partial_scene_or_hits() {
    let owner = ScreenInstanceId(84);
    let children = [Node::leaf([20, 10], 1u8).at(10, 10)];
    let first = MountedLayout::mount(Node::layer([100, 50], &children)).unwrap();
    let mut second = first.clone();
    second
        .update(&[LayoutUpdate {
            id: NodeId(0),
            change: LayoutChange::Clip(Some(Bounds {
                x: 0,
                y: 0,
                width: 40,
                height: 50,
            })),
        }])
        .unwrap();
    let shape = |b: Bounds| [b.x, b.y, b.width, b.height];
    assert_eq!(
        shape(first.geometry(NodeId(1)).unwrap().bounds),
        shape(second.geometry(NodeId(1)).unwrap().bounds)
    );
    assert_eq!(
        shape(first.geometry(NodeId(1)).unwrap().clip),
        shape(second.geometry(NodeId(1)).unwrap().clip)
    );
    let mut accepted = RetainedNodes::new(100, 50).unwrap();
    accepted
        .static_layout_node(&first, &[NodeId(1)], paint)
        .unwrap();
    let mut scene = Scene::with_capacity(100, 50, 8);
    let mut hits = Vec::new();
    accepted
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    let before = scene
        .rectangles()
        .iter()
        .map(|r| (r.bounds, r.color, r.uv))
        .collect::<Vec<_>>();
    let before_hits = hits
        .iter()
        .map(|(id, b)| (*id, shape(*b)))
        .collect::<Vec<_>>();
    let stamp = scene.geometry_stamp().1;
    let id = scene.component_id(UiComponentKey {
        screen: owner,
        node: NodeId(1),
    });
    let mut ambiguous = RetainedNodes::new(100, 50).unwrap();
    ambiguous
        .static_layout_node(&first, &[NodeId(1)], paint)
        .unwrap();
    ambiguous
        .static_layout_node(&second, &[NodeId(1)], paint)
        .unwrap();
    assert!(ambiguous
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .is_err());
    assert_eq!(
        scene
            .rectangles()
            .iter()
            .map(|r| (r.bounds, r.color, r.uv))
            .collect::<Vec<_>>(),
        before
    );
    assert_eq!(
        hits.iter()
            .map(|(id, b)| (*id, shape(*b)))
            .collect::<Vec<_>>(),
        before_hits
    );
    assert_eq!(scene.geometry_stamp().1, stamp);
    assert_eq!(
        scene.component_id(UiComponentKey {
            screen: owner,
            node: NodeId(1)
        }),
        id
    );
}

#[test]
fn interior_node_local_clip_scales_about_node_bounds_and_warm_ticks_only_change_uniforms() {
    let owner = ScreenInstanceId(85);
    let children = [Node::leaf([30, 20], 1u8).at(10, 20)];
    let mut layout = MountedLayout::mount(Node::layer([120, 100], &children)).unwrap();
    layout
        .update(&[LayoutUpdate {
            id: NodeId(1),
            change: LayoutChange::Clip(Some(Bounds {
                x: 5,
                y: 5,
                width: 10,
                height: 10,
            })),
        }])
        .unwrap();
    let clips = layout.component_clips(NodeId(1)).unwrap();
    assert_eq!([clips.source.x, clips.source.y], [15, 25]);
    let mut nodes = RetainedNodes::new(120, 100).unwrap();
    nodes
        .static_layout_node(&layout, &[NodeId(1)], paint)
        .unwrap();
    let mut scene = Scene::with_capacity(120, 100, 4);
    let mut hits = Vec::new();
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    let key = UiComponentKey {
        screen: owner,
        node: NodeId(1),
    };
    let id = scene.component_id(key).unwrap();
    let slot = scene.batches()[0].component as usize;
    let stamp = scene.geometry_stamp().1;
    let pointer = scene.rectangles().as_ptr();
    let identities = nodes.identities();
    let paints = nodes.paints();
    let destination = UiTransform::new([30.0, 10.0], [2.0, 3.0], 0.5).unwrap();
    let mut scheduler = MotionScheduler::new(owner, 1).unwrap();
    scheduler
        .schedule(
            key,
            id,
            ComponentMotion::new(
                UiTransform::default(),
                destination,
                Duration::from_nanos(100),
                Easing::Linear,
            ),
            Duration::ZERO,
        )
        .unwrap();
    for elapsed in [25, 50, 75, 100] {
        assert!(scheduler
            .tick(owner, Duration::from_nanos(elapsed), &mut scene)
            .unwrap());
        nodes
            .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
            .unwrap();
        assert_eq!(scene.geometry_stamp().1, stamp);
        assert_eq!(scene.rectangles().as_ptr(), pointer);
        assert_eq!(nodes.identities(), identities);
        assert_eq!(nodes.paints(), paints);
        assert_eq!(scene.component_id(key), Some(id));
    }
    // Node origin (10,20), rather than source-clip origin (15,25), gives
    // destination local-clip edges x=[50,70), y=[45,75).
    let uniform = scene.component_gpu_uniform(slot);
    assert_eq!(&uniform[..7], &[30.0, 10.0, 2.0, 3.0, 10.0, 20.0, 0.5]);
    assert_eq!(&uniform[8..12], &[0.0, 0.0, 120.0, 100.0]);
    assert_eq!(&uniform[12..16], &[15.0, 25.0, 25.0, 35.0]);
    assert_eq!(
        scene.project_component_point(id, (50.0, 45.0)),
        Some((15.0, 25.0))
    );
    assert_eq!(
        scene.project_component_point(id, (60.0, 60.0)),
        Some((20.0, 30.0))
    );
    for point in [(50.0, 45.0), (60.0, 60.0), (69.5, 74.5)] {
        assert_eq!(
            nodes.hit_components(&scene, owner, point),
            Some(ControlId(1))
        );
    }
    for point in [
        (49.5, 60.0),
        (60.0, 44.5),
        (70.0, 60.0),
        (60.0, 75.0),
        (20.0, 30.0),
    ] {
        assert_eq!(scene.project_component_point(id, point), None);
        assert_eq!(nodes.hit_components(&scene, owner, point), None);
    }
    assert_eq!(scene.rectangles()[0].bounds, [15.0, 25.0, 10.0, 10.0]);
    assert_eq!(scene.component_transform(id), Some(destination));
}

#[test]
fn ancestor_clip_dependency_rebinds_paint_and_hits_even_when_leaf_geometry_is_identical() {
    let owner = ScreenInstanceId(86);
    let children = [Node::leaf([20, 10], 1u8).at(10, 10)];
    let mut layout = MountedLayout::mount(Node::layer([100, 50], &children)).unwrap();
    let shape = |b: Bounds| [b.x, b.y, b.width, b.height];
    let before_leaf = layout.geometry(NodeId(1)).unwrap();
    let mut nodes = RetainedNodes::new(100, 50).unwrap();
    nodes
        .static_layout_node(&layout, &[NodeId(1)], paint)
        .unwrap();
    let mut scene = Scene::with_capacity(100, 50, 4);
    let mut hits = Vec::new();
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    let key = UiComponentKey {
        screen: owner,
        node: NodeId(1),
    };
    let id = scene.component_id(key).unwrap();
    let transform = UiTransform::new([30.0, 0.0], [1.0, 1.0], 1.0).unwrap();
    scene.set_component_transforms(&[(id, transform)]).unwrap();
    assert_eq!(
        nodes.hit_components(&scene, owner, (55.0, 15.0)),
        Some(ControlId(1))
    );
    let paints = nodes.paints();
    layout
        .update(&[LayoutUpdate {
            id: NodeId(0),
            change: LayoutChange::Clip(Some(Bounds {
                x: 0,
                y: 0,
                width: 50,
                height: 50,
            })),
        }])
        .unwrap();
    let after_leaf = layout.geometry(NodeId(1)).unwrap();
    assert_eq!(shape(before_leaf.bounds), shape(after_leaf.bounds));
    assert_eq!(shape(before_leaf.clip), shape(after_leaf.clip));
    assert_eq!(
        shape(layout.component_clips(NodeId(1)).unwrap().inherited),
        [0, 0, 50, 50]
    );
    assert!(
        nodes.relayout(&layout).unwrap(),
        "inherited clip must invalidate its retained binding"
    );
    assert_ne!(nodes.paints(), paints);
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    let rebound = scene.component_id(key).unwrap();
    assert_eq!(scene.component_transform(rebound), Some(transform));
    let uniform = scene.component_gpu_uniform(scene.batches()[0].component as usize);
    assert_eq!(&uniform[8..12], &[0.0, 0.0, 50.0, 50.0]);
    assert_eq!(&uniform[12..16], &[10.0, 10.0, 30.0, 20.0]);
    assert_eq!(
        nodes.hit_components(&scene, owner, (49.5, 15.0)),
        Some(ControlId(1))
    );
    for point in [(50.0, 15.0), (55.0, 15.0)] {
        assert_eq!(scene.project_component_point(rebound, point), None);
        assert_eq!(nodes.hit_components(&scene, owner, point), None);
    }
    assert_eq!(scene.rectangles()[0].bounds, [10.0, 10.0, 20.0, 10.0]);
    assert!(!nodes.relayout(&layout).unwrap());
}

#[test]
fn invalid_layout_batch_preserves_published_component_clips_transform_and_cached_geometry() {
    let owner = ScreenInstanceId(87);
    let children = [Node::leaf([20, 10], 1u8).at(10, 10)];
    let mut layout = MountedLayout::mount(Node::layer([100, 50], &children)).unwrap();
    let mut nodes = RetainedNodes::new(100, 50).unwrap();
    nodes
        .static_layout_node(&layout, &[NodeId(1)], paint)
        .unwrap();
    let mut scene = Scene::with_capacity(100, 50, 4);
    let mut hits = Vec::new();
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    let key = UiComponentKey {
        screen: owner,
        node: NodeId(1),
    };
    let id = scene.component_id(key).unwrap();
    let transform = UiTransform::new([30.0, 0.0], [1.0, 1.0], 0.75).unwrap();
    scene.set_component_transforms(&[(id, transform)]).unwrap();
    let slot = scene.batches()[0].component as usize;
    let uniform = scene.component_gpu_uniform(slot);
    let stamp = scene.geometry_stamp().1;
    let pointer = scene.rectangles().as_ptr();
    let paints = nodes.paints();
    let identities = nodes.identities();
    let revision = layout.revision();
    let shape = |b: Bounds| [b.x, b.y, b.width, b.height];
    let clips = layout.component_clips(NodeId(1)).unwrap();
    let before_hits = hits
        .iter()
        .map(|(id, bounds)| (*id, shape(*bounds)))
        .collect::<Vec<_>>();
    assert!(layout
        .update(&[
            LayoutUpdate {
                id: NodeId(0),
                change: LayoutChange::Clip(Some(Bounds {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 50
                }))
            },
            LayoutUpdate {
                id: NodeId(1),
                change: LayoutChange::Size([-1, 10])
            },
        ])
        .is_err());
    assert_eq!(layout.revision(), revision);
    let refused = layout.component_clips(NodeId(1)).unwrap();
    assert_eq!(shape(refused.source), shape(clips.source));
    assert_eq!(shape(refused.inherited), shape(clips.inherited));
    assert!(!nodes.relayout(&layout).unwrap());
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    assert_eq!(scene.component_id(key), Some(id));
    assert_eq!(scene.component_transform(id), Some(transform));
    assert_eq!(scene.component_gpu_uniform(slot), uniform);
    assert_eq!(scene.geometry_stamp().1, stamp);
    assert_eq!(scene.rectangles().as_ptr(), pointer);
    assert_eq!(nodes.paints(), paints);
    assert_eq!(nodes.identities(), identities);
    assert_eq!(
        hits.iter()
            .map(|(id, bounds)| (*id, shape(*bounds)))
            .collect::<Vec<_>>(),
        before_hits
    );
    assert_eq!(
        nodes.hit_components(&scene, owner, (55.0, 15.0)),
        Some(ControlId(1))
    );
}
