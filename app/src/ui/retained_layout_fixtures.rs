use super::*;
use crate::ui::layout::{LayoutChange, LayoutGeometry, LayoutUpdate, MountedLayout, Node, NodeId};

fn region(b: Bounds) -> [i64; 4] {
    [b.x, b.y, b.width, b.height]
}

fn paint_control(
    id: NodeId,
    geometry: LayoutGeometry,
    scene: &mut Scene,
    hits: &mut Vec<(ControlId, Bounds)>,
) {
    let b = geometry.bounds;
    scene.rect(b.x, b.y, b.width, b.height, 0xabcdef);
    hits.push((ControlId(id.0 as u64), b));
}

#[test]
fn ancestor_clip_dependency_invalidates_motion_binding_even_when_ordinary_leaf_geometry_is_identical(
) {
    use crate::scene::{UiComponentKey, UiTransform};
    use crate::screen_lifecycle::ScreenInstanceId;
    let children = [Node::leaf([20, 10], 1u8).at(10, 10)];
    let mut layout = MountedLayout::mount(Node::layer([100, 50], &children)).unwrap();
    let mut nodes = RetainedNodes::new(100, 50).unwrap();
    nodes
        .static_layout_node(&layout, &[NodeId(1)], paint_control)
        .unwrap();
    let owner = ScreenInstanceId(81);
    let mut scene = Scene::with_capacity(100, 50, 8);
    let mut hits = Vec::new();
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    let key = UiComponentKey {
        screen: owner,
        node: NodeId(1),
    };
    let id = scene.component_id(key).unwrap();
    let pose = UiTransform::new([30.5, 0.0], [1.0, 1.0], 1.0).unwrap();
    scene.set_component_transforms(&[(id, pose)]).unwrap();
    assert_eq!(
        nodes.hit_components(&scene, owner, (50.5, 15.0)),
        Some(ControlId(1))
    );
    let geometry = layout.geometry(NodeId(1)).unwrap();
    let identities = nodes.identities();
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
    assert_eq!(
        region(layout.geometry(NodeId(1)).unwrap().bounds),
        region(geometry.bounds)
    );
    assert_eq!(
        region(layout.geometry(NodeId(1)).unwrap().clip),
        region(geometry.clip)
    );
    assert!(
        nodes.relayout(&layout).unwrap(),
        "true ancestor clip is a retained component dependency"
    );
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    assert_eq!(scene.component_id(key), Some(id));
    assert_eq!(scene.component_transform(id), Some(pose));
    assert_eq!(nodes.hit_components(&scene, owner, (50.5, 15.0)), None);
    assert_eq!(nodes.identities(), identities);
    let stamp = scene.geometry_stamp().1;
    let paints = nodes.paints();
    assert!(!nodes.relayout(&layout).unwrap());
    nodes
        .compose_components(&mut scene, &mut hits, owner, &[NodeId(1)])
        .unwrap();
    assert_eq!(scene.geometry_stamp().1, stamp);
    assert_eq!(nodes.paints(), paints);
}

fn composed(
    nodes: &RetainedNodes,
    width: u32,
    height: u32,
) -> (Vec<[f32; 4]>, Vec<(u64, [i64; 4])>) {
    let mut scene = Scene::new(width, height);
    let mut hits = Vec::new();
    nodes.compose(&mut scene, &mut hits).unwrap();
    (
        scene.rectangles().iter().map(|r| r.bounds).collect(),
        hits.iter().map(|(id, b)| (id.0, region(*b))).collect(),
    )
}

#[test]
fn retained_reflow_repaints_only_dependent_packets_and_reuses_unchanged_frames() {
    let row = [Node::leaf([20, 10], 1u8), Node::leaf([30, 10], 2)];
    let branches = [
        Node::row([60, 10], 5, &row).at(10, 10),
        Node::leaf([10, 10], 3).at(80, 40),
    ];
    let mut layout = MountedLayout::mount(Node::layer([100, 60], &branches)).unwrap();
    let mut nodes = RetainedNodes::new(100, 60).unwrap();
    for id in [NodeId(2), NodeId(3), NodeId(4)] {
        nodes
            .static_layout_node(&layout, &[id], paint_control)
            .unwrap();
    }
    let initial = composed(&nodes, 100, 60);
    assert_eq!(
        initial.0,
        vec![
            [10.0, 10.0, 20.0, 10.0],
            [35.0, 10.0, 30.0, 10.0],
            [80.0, 40.0, 10.0, 10.0]
        ]
    );
    let identities = nodes.identities();
    let paints = nodes.paints();
    assert!(!nodes.relayout(&layout).unwrap());
    assert_eq!(composed(&nodes, 100, 60), initial);
    assert_eq!(nodes.identities(), identities);
    assert_eq!(nodes.paints(), paints);
    layout
        .update(&[LayoutUpdate {
            id: NodeId(2),
            change: LayoutChange::Size([25, 10]),
        }])
        .unwrap();
    assert!(nodes.relayout(&layout).unwrap());
    assert_eq!(nodes.identities(), identities);
    assert_eq!(
        nodes.paints(),
        vec![paints[0] + 1, paints[1] + 1, paints[2]]
    );
    assert_eq!(
        composed(&nodes, 100, 60).0,
        vec![
            [10.0, 10.0, 25.0, 10.0],
            [40.0, 10.0, 30.0, 10.0],
            [80.0, 40.0, 10.0, 10.0]
        ]
    );
    assert_eq!(nodes.hit((39.0, 15.0)), None);
    assert_eq!(nodes.hit((40.0, 15.0)), Some(ControlId(3)));
}

#[test]
fn inherited_clip_crops_paint_and_hits_in_identical_painter_order() {
    let children = [
        Node::leaf([40, 40], 1u8).at(0, 0),
        Node::leaf([20, 20], 2).at(20, 12),
    ];
    let mut layout = MountedLayout::mount(Node::layer([40, 40], &children)).unwrap();
    layout
        .update(&[LayoutUpdate {
            id: NodeId(0),
            change: LayoutChange::Clip(Some(Bounds {
                x: 10,
                y: 10,
                width: 25,
                height: 20,
            })),
        }])
        .unwrap();
    let mut nodes = RetainedNodes::new(40, 40).unwrap();
    nodes
        .static_layout_node(&layout, &[NodeId(1), NodeId(2)], paint_control)
        .unwrap();
    let (paint, hits) = composed(&nodes, 40, 40);
    assert_eq!(
        paint,
        vec![[10.0, 10.0, 25.0, 20.0], [20.0, 12.0, 15.0, 18.0]]
    );
    assert_eq!(hits, vec![(1, [10, 10, 25, 20]), (2, [20, 12, 15, 18])]);
    for (point, expected) in [
        ((10.0, 10.0), Some(ControlId(1))),
        ((20.0, 12.0), Some(ControlId(2))),
        ((34.999, 29.999), Some(ControlId(2))),
        ((35.0, 20.0), None),
        ((20.0, 30.0), None),
        ((9.999, 12.0), None),
    ] {
        assert_eq!(nodes.hit(point), expected);
    }
    let before = composed(&nodes, 40, 40);
    for clip in [
        Bounds {
            x: -1,
            y: 0,
            width: 1,
            height: 1,
        },
        Bounds {
            x: i64::MAX,
            y: 0,
            width: 1,
            height: 1,
        },
    ] {
        assert!(layout
            .update(&[LayoutUpdate {
                id: NodeId(0),
                change: LayoutChange::Clip(Some(clip))
            }])
            .is_err());
        assert!(!nodes.relayout(&layout).unwrap());
        assert_eq!(composed(&nodes, 40, 40), before);
    }
}

#[test]
fn failed_candidate_paint_keeps_all_previous_packets_hits_and_identity_then_retries() {
    let children = [Node::leaf([10, 10], 1u8), Node::leaf([10, 10], 2)];
    let mut layout = MountedLayout::mount(Node::row([40, 20], 5, &children)).unwrap();
    let mut nodes = RetainedNodes::new(40, 20).unwrap();
    let refuse = Rc::new(Cell::new(false));
    nodes
        .static_layout_node(&layout, &[NodeId(1)], paint_control)
        .unwrap();
    let fault = Rc::clone(&refuse);
    nodes
        .static_layout_node(&layout, &[NodeId(2)], move |id, geometry, scene, hits| {
            paint_control(id, geometry, scene, hits);
            if fault.get() {
                for _ in 0..crate::scene::MAX_RECTANGLES {
                    scene.rect(geometry.bounds.x, 0, 1, 1, 0);
                }
            }
        })
        .unwrap();
    let before = composed(&nodes, 40, 20);
    let identities = nodes.identities();
    let paints = nodes.paints();
    layout
        .update(&[LayoutUpdate {
            id: NodeId(1),
            change: LayoutChange::Size([15, 10]),
        }])
        .unwrap();
    refuse.set(true);
    assert!(nodes.relayout(&layout).is_err());
    assert_eq!(nodes.identities(), identities);
    assert_eq!(nodes.paints(), paints);
    assert!(!nodes.dirty());
    assert_eq!(composed(&nodes, 40, 20), before);
    assert_eq!(nodes.hit((16.0, 5.0)), Some(ControlId(2)));
    refuse.set(false);
    assert!(nodes.relayout(&layout).unwrap());
    assert_eq!(
        composed(&nodes, 40, 20).0,
        vec![[0.0, 0.0, 15.0, 10.0], [20.0, 0.0, 10.0, 10.0]]
    );
}

#[test]
fn zero_extent_suspends_paint_and_input_then_restores_same_mounted_packets() {
    let mut layout = MountedLayout::mount(Node::leaf([30, 20], 1u8)).unwrap();
    let mut nodes = RetainedNodes::new(30, 20).unwrap();
    nodes
        .static_layout_node(&layout, &[NodeId(0)], paint_control)
        .unwrap();
    let before = composed(&nodes, 30, 20);
    let identities = nodes.identities();
    layout.resize([0, 20]).unwrap();
    assert!(nodes.relayout(&layout).unwrap());
    assert_eq!(nodes.hit((1.0, 1.0)), None);
    assert_eq!(composed(&nodes, 30, 20), (vec![], vec![]));
    layout.resize([30, 20]).unwrap();
    assert!(nodes.relayout(&layout).unwrap());
    assert_eq!(nodes.identities(), identities);
    assert_eq!(composed(&nodes, 30, 20), before);
    assert!(!nodes.relayout(&layout).unwrap());
}

#[test]
fn reactive_updates_after_reflow_use_current_geometry_without_replacing_packets() {
    use floem_reactive::SignalUpdate;
    let scope = Scope::new();
    let enabled = scope.create_rw_signal(true);
    let memo = scope.create_memo(move |_| enabled.get());
    let mut layout = MountedLayout::mount(Node::leaf([30, 20], 1u8)).unwrap();
    let mut nodes = RetainedNodes::new(30, 20).unwrap();
    nodes
        .bind_layout(
            scope,
            memo,
            &layout,
            &[NodeId(0)],
            |enabled, id, geometry, scene, hits| {
                let b = geometry.bounds;
                scene.rect(
                    b.x,
                    b.y,
                    b.width,
                    b.height,
                    if *enabled { 0xffffff } else { 0 },
                );
                if *enabled {
                    hits.push((ControlId(id.0 as u64), b));
                }
            },
        )
        .unwrap();
    let identities = nodes.identities();
    layout.resize([45, 25]).unwrap();
    nodes.relayout(&layout).unwrap();
    assert_eq!(composed(&nodes, 45, 25).0, vec![[0.0, 0.0, 45.0, 25.0]]);
    enabled.set(false);
    assert_eq!(
        composed(&nodes, 45, 25),
        (vec![[0.0, 0.0, 45.0, 25.0]], vec![])
    );
    enabled.set(true);
    assert_eq!(composed(&nodes, 45, 25).1, vec![(0, [0, 0, 45, 25])]);
    assert_eq!(nodes.identities(), identities);
    scope.dispose();
}

struct CountedLayoutModel {
    generation: u32,
    text: String,
    clones: Rc<Cell<usize>>,
}
impl Clone for CountedLayoutModel {
    fn clone(&self) -> Self {
        self.clones.set(self.clones.get() + 1);
        Self {
            generation: self.generation,
            text: self.text.clone(),
            clones: Rc::clone(&self.clones),
        }
    }
}
impl PartialEq for CountedLayoutModel {
    fn eq(&self, other: &Self) -> bool {
        self.generation == other.generation && self.text == other.text
    }
}

#[test]
fn borrowed_layout_snapshot_clones_once_per_update_independent_of_painted_node_count() {
    use crate::scene::{UiComponentKey, UiTransform};
    use crate::screen_lifecycle::ScreenInstanceId;
    use floem_reactive::SignalUpdate;
    let mut setup_clone_counts = Vec::new();
    for count in [1, 16] {
        let scope = Scope::new();
        let generation = scope.create_rw_signal(1u32);
        let clones = Rc::new(Cell::new(0));
        let memo_clones = Rc::clone(&clones);
        let memo = scope.create_memo(move |_| CountedLayoutModel {
            generation: generation.get(),
            text: "non-Copy owned layout text".repeat(16),
            clones: Rc::clone(&memo_clones),
        });
        let children: Vec<_> = (0..count)
            .map(|index| Node::leaf([10, 10], index as u8).at(index * 12, 10))
            .collect();
        let mut layout = MountedLayout::mount(Node::layer([240, 100], &children)).unwrap();
        let ids: Vec<_> = layout.leaves().iter().map(|leaf| leaf.id).collect();
        let observed = Rc::new(RefCell::new(Vec::new()));
        let captured = Rc::clone(&observed);
        let mut nodes = RetainedNodes::new(240, 100).unwrap();
        nodes
            .bind_layout(
                scope,
                memo,
                &layout,
                &ids,
                move |model, id, geometry, scene, hits| {
                    captured.borrow_mut().push((
                        model.generation,
                        model as *const CountedLayoutModel as usize,
                    ));
                    paint_control(id, geometry, scene, hits);
                },
            )
            .unwrap();
        setup_clone_counts.push(clones.get());
        assert!(
            clones.get() > 0,
            "memo admission should create its owned model snapshot"
        );
        observed.borrow_mut().clear();
        let before = clones.get();
        generation.set(2);
        assert_eq!(
            clones.get() - before,
            1,
            "one memo publication must not clone per leaf"
        );
        let update = observed.borrow();
        assert_eq!(update.len(), count as usize);
        assert!(update.iter().all(|entry| *entry == update[0]));
        assert_eq!(update[0].0, 2);
        drop(update);

        let before_geometry = clones.get();
        composed(&nodes, 240, 100);
        assert!(!nodes.dirty());
        // Resize is a valid geometry-only dependency change for every node.
        layout.resize([300, 120]).unwrap();
        assert!(nodes.relayout(&layout).unwrap());
        observed.borrow_mut().clear();
        let owner = ScreenInstanceId(151 + count as u64);
        let mut scene = Scene::new(300, 120);
        let mut hits = Vec::new();
        nodes
            .compose_components(&mut scene, &mut hits, owner, &ids)
            .unwrap();
        assert_eq!(observed.borrow().len(), count as usize);
        let model_addresses = observed.borrow();
        assert!(model_addresses
            .iter()
            .all(|entry| *entry == model_addresses[0]));
        drop(model_addresses);
        let identity = scene.geometry_stamp().0.clone();
        let revision = scene.geometry_stamp().1;
        for id in &ids {
            let component = scene
                .component_id(UiComponentKey {
                    screen: owner,
                    node: *id,
                })
                .unwrap();
            scene
                .set_component_transforms(&[(
                    component,
                    UiTransform::new([0.5, 1.0], [1.0, 1.0], 1.0).unwrap(),
                )])
                .unwrap();
        }
        nodes
            .compose_components(&mut scene, &mut hits, owner, &ids)
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(&identity, scene.geometry_stamp().0));
        assert_eq!(scene.geometry_stamp().1, revision);
        assert_eq!(
            clones.get(),
            before_geometry,
            "geometry/capture/motion must borrow the admitted model"
        );
        scope.dispose();
    }
    assert_eq!(
        setup_clone_counts[0], setup_clone_counts[1],
        "mount clones must not scale with leaf count"
    );
}

#[test]
fn reentrant_model_publication_keeps_current_packet_snapshot_and_releases_model_borrow() {
    use floem_reactive::SignalUpdate;
    let scope = Scope::new();
    let generation = scope.create_rw_signal(1u32);
    let clones = Rc::new(Cell::new(0));
    let memo_clones = Rc::clone(&clones);
    let memo = scope.create_memo(move |_| CountedLayoutModel {
        generation: generation.get(),
        text: format!("generation {}", generation.get()),
        clones: Rc::clone(&memo_clones),
    });
    let children = [
        Node::leaf([10, 10], 1u8).at(0, 0),
        Node::leaf([10, 10], 2u8).at(20, 0),
    ];
    let layout = MountedLayout::mount(Node::layer([60, 30], &children)).unwrap();
    let ids = [NodeId(1), NodeId(2)];
    let publish = Rc::new(Cell::new(false));
    let callback_publish = Rc::clone(&publish);
    let observed = Rc::new(RefCell::new(Vec::new()));
    let callback_observed = Rc::clone(&observed);
    let mut nodes = RetainedNodes::new(60, 30).unwrap();
    nodes
        .bind_layout(
            scope,
            memo,
            &layout,
            &ids,
            move |model, id, geometry, scene, hits| {
                callback_observed.borrow_mut().push((
                    id,
                    model.generation,
                    model as *const CountedLayoutModel as usize,
                ));
                scene.rect(
                    geometry.bounds.x,
                    geometry.bounds.y,
                    geometry.bounds.width,
                    geometry.bounds.height,
                    model.generation,
                );
                hits.push((
                    ControlId(100 * u64::from(model.generation) + id.0 as u64),
                    geometry.bounds,
                ));
                if callback_publish.replace(false) {
                    generation.set(2);
                }
            },
        )
        .unwrap();
    // Capture the actual packet painter after releasing its packet borrow.
    // This tests reentrant model publication, not nested Scene composition.
    let binding = Rc::clone(nodes.packets[0].borrow().layout.as_ref().unwrap());
    let dependencies = binding.geometries.borrow().clone();
    observed.borrow_mut().clear();
    publish.set(true);
    let before = clones.get();
    let current = (binding.paint)(&dependencies, [60, 30], &ids);
    current.geometry.as_ref().unwrap();
    assert_eq!(
        current.hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
        vec![101, 102],
        "all nodes of an entered packet must retain the old snapshot"
    );
    assert_eq!(
        clones.get() - before,
        1,
        "the reentrant memo publication clones once"
    );
    let calls = observed.borrow();
    let old: Vec<_> = calls
        .iter()
        .filter(|(_, revision, _)| *revision == 1)
        .collect();
    let new: Vec<_> = calls
        .iter()
        .filter(|(_, revision, _)| *revision == 2)
        .collect();
    assert_eq!(old.len(), 2);
    assert_eq!(new.len(), 2);
    assert_eq!(old[0].2, old[1].2);
    assert_eq!(new[0].2, new[1].2);
    assert_ne!(
        old[0].2, new[0].2,
        "old snapshot stays alive while the new one is published"
    );
    drop(calls);
    assert_eq!(
        composed(&nodes, 60, 30).1,
        vec![(201, [0, 0, 10, 10]), (202, [20, 0, 10, 10])]
    );
    let following = (binding.paint)(&dependencies, [60, 30], &ids);
    assert_eq!(
        following
            .hits
            .iter()
            .map(|(id, _)| id.0)
            .collect::<Vec<_>>(),
        vec![201, 202]
    );
    scope.dispose();
}

#[test]
fn actual_display_preserves_default_controls_and_resize_round_trip_geometry() {
    use crate::screen_lifecycle::ScreenInstanceId;
    use crate::ui::display::{DisplayFrame, DisplayView};
    use crate::ui::text_input::LineEditor;
    let mut view = DisplayView::new(ScreenInstanceId(71), 960, 720).unwrap();
    let editors = ["auto", "fifo", "120", "2000"].map(|value| LineEditor::new(value, 32).unwrap());
    let update = || DisplayFrame {
        editors: &editors,
        selected: 0,
        error: None,
        pending: false,
        hovered: None,
        armed: None,
    };
    view.update(update()).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    let original = scene
        .rectangles()
        .iter()
        .map(|r| (r.bounds, r.color, r.uv))
        .collect::<Vec<_>>();
    assert_eq!(
        hits.iter()
            .map(|(id, b)| (id.0, region(*b)))
            .collect::<Vec<_>>(),
        vec![
            (40000, [280, 130, 650, 34]),
            (40001, [280, 205, 650, 34]),
            (40002, [280, 280, 650, 34]),
            (40003, [280, 355, 650, 34]),
            (40, [24, 620, 170, 34]),
            (41, [212, 620, 170, 34]),
        ]
    );
    assert!(!view.dirty());
    view.update(update()).unwrap();
    assert!(!view.resize(960, 720).unwrap());
    assert!(!view.dirty());
    assert!(view.resize(1200, 800).unwrap());
    view.compose(&mut Scene::new(1200, 800), &mut hits).unwrap();
    assert_eq!(
        hits.iter()
            .map(|(id, b)| (id.0, region(*b)))
            .collect::<Vec<_>>(),
        vec![
            (40000, [286, 144, 650, 34]),
            (40001, [286, 219, 650, 34]),
            (40002, [286, 294, 650, 34]),
            (40003, [286, 369, 650, 34]),
            (40, [30, 688, 170, 34]),
            (41, [218, 688, 170, 34]),
        ]
    );
    assert_eq!(view.id(), ScreenInstanceId(71));
    assert!(view.resize(480, 360).unwrap());
    let mut narrow = Scene::new(480, 360);
    view.compose(&mut narrow, &mut hits).unwrap();
    assert_eq!(
        hits.iter()
            .map(|(id, b)| (id.0, region(*b)))
            .collect::<Vec<_>>(),
        vec![
            (40000, [268, 65, 212, 34]),
            (40001, [268, 140, 212, 34]),
            (40002, [268, 215, 212, 34]),
            (40003, [268, 290, 212, 34]),
            (40, [12, 310, 170, 34]),
            (41, [200, 310, 170, 34]),
        ]
    );
    assert!(narrow.rectangles().iter().all(|r| r.bounds[0] >= 0.0
        && r.bounds[1] >= 0.0
        && r.bounds[0] + r.bounds[2] <= 480.0
        && r.bounds[1] + r.bounds[3] <= 360.0));
    assert!(view.resize(0, 800).unwrap());
    let mut suspended = Scene::new(1200, 800);
    view.compose(&mut suspended, &mut hits).unwrap();
    assert!(suspended.rectangles().is_empty());
    assert!(hits.is_empty());
    assert!(view.resize(960, 720).unwrap());
    view.compose(&mut scene, &mut hits).unwrap();
    assert_eq!(
        scene
            .rectangles()
            .iter()
            .map(|r| (r.bounds, r.color, r.uv))
            .collect::<Vec<_>>(),
        original
    );
    assert_eq!(hits.len(), 6);
}
