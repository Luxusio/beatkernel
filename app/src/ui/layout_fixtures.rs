use super::*;

fn leaf(width: i64, height: i64, id: u8) -> Node<'static, u8> {
    Node::leaf([width, height], id)
}
#[test]
fn nested_flow_and_positioned_regions_resolve_in_painter_order() {
    let row = [leaf(20, 10, 1), leaf(30, 10, 2)];
    let column = [Node::row([55, 10], 5, &row), leaf(55, 7, 3)];
    let layers = [
        Node::column([55, 20], 3, &column).at(12, 15),
        leaf(5, 5, 4).at(1, 2),
    ];
    let result = resolve(Node::layer([100, 100], &layers)).unwrap();
    assert_eq!(
        result
            .iter()
            .map(|leaf| (
                leaf.component,
                leaf.bounds.x,
                leaf.bounds.y,
                leaf.bounds.width,
                leaf.bounds.height
            ))
            .collect::<Vec<_>>(),
        vec![
            (1, 12, 15, 20, 10),
            (2, 37, 15, 30, 10),
            (3, 12, 28, 55, 7),
            (4, 1, 2, 5, 5)
        ]
    );
    assert!(result[1].bounds.contains((37.0, 15.0)));
    assert!(!result[1].bounds.contains((67.0, 15.0)));
}
#[test]
fn invalid_sizes_origins_gaps_and_parent_overruns_reject() {
    for size in [[0, 1], [1, 0], [-1, 1], [1, -1]] {
        assert!(resolve(Node::leaf(size, 0)).is_err());
    }
    for origin in [[-1, 0], [0, -1], [99, 0], [0, 99]] {
        let layers = [leaf(2, 2, 1).at(origin[0], origin[1])];
        assert!(resolve(Node::layer([100, 100], &layers)).is_err());
    }
    let children = [leaf(40, 20, 0), leaf(40, 20, 1)];
    assert!(resolve(Node::row([100, 20], -1, &children)).is_err());
    assert!(resolve(Node::column([40, 100], -1, &children)).is_err());
    assert!(resolve(Node::row([80, 20], 1, &children)).is_err());
    assert!(resolve(Node::column([40, 40], 1, &children)).is_err());
    assert!(resolve(Node::row([100, 19], 0, &children)).is_err());
}
#[test]
fn checked_arithmetic_rejects_extreme_declarations_without_wrapping() {
    let children = [leaf(1, 1, 0).at(i64::MAX, 0)];
    assert!(resolve(Node::layer([i64::MAX, 10], &children)).is_err());
    let row = [leaf(i64::MAX - 1, 1, 0), leaf(1, 1, 1)];
    assert!(resolve(Node::row([i64::MAX, 1], i64::MAX, &row)).is_err());
    let column = [leaf(1, i64::MAX - 1, 0), leaf(1, 1, 1)];
    assert!(resolve(Node::column([1, i64::MAX], i64::MAX, &column)).is_err());
}
#[test]
fn depth_and_node_budgets_bound_mount_work() {
    fn nested(depth: usize) -> Node<'static, u8> {
        if depth == 0 {
            return leaf(1, 1, 0);
        }
        let children = Box::leak(Box::new([nested(depth - 1)]));
        Node::column([1, 1], 0, children)
    }
    assert!(resolve(nested(32)).is_ok());
    assert!(resolve(nested(33)).is_err());
    let children = vec![leaf(1, 1, 0).at(0, 0); 1023];
    assert_eq!(resolve(Node::layer([1, 1], &children)).unwrap().len(), 1023);
    let children = vec![leaf(1, 1, 0).at(0, 0); 1024];
    assert!(resolve(Node::layer([1, 1], &children)).is_err());
}

fn mounted_geometry<T: Copy>(layout: &MountedLayout<T>) -> Vec<(usize, [i64; 4], [i64; 4])> {
    let bounds = |b: super::super::interaction::Bounds| [b.x, b.y, b.width, b.height];
    layout
        .leaves()
        .iter()
        .map(|leaf| {
            (
                leaf.id.0,
                bounds(leaf.geometry.bounds),
                bounds(leaf.geometry.clip),
            )
        })
        .collect()
}

#[test]
fn mounted_child_size_reflows_later_siblings_without_replacing_unrelated_branch() {
    let row = [leaf(20, 20, 1), leaf(30, 20, 2), leaf(10, 20, 3)];
    let isolated = [leaf(10, 10, 4).at(0, 0)];
    let branches = [
        Node::row([90, 20], 5, &row).at(5, 7),
        Node::layer([10, 10], &isolated).at(100, 60),
    ];
    let mut layout = MountedLayout::mount(Node::layer([120, 80], &branches)).unwrap();
    assert_eq!(
        layout
            .leaves()
            .iter()
            .map(|leaf| (leaf.id.0, leaf.component))
            .collect::<Vec<_>>(),
        vec![(2, 1), (3, 2), (4, 3), (6, 4)]
    );
    assert_eq!(
        layout
            .leaves()
            .iter()
            .map(|leaf| leaf.geometry.bounds.x)
            .collect::<Vec<_>>(),
        vec![5, 30, 65, 100]
    );
    let isolated_before = mounted_geometry(&layout)[3];
    let revision = layout.revision();
    assert!(layout
        .update(&[LayoutUpdate {
            id: NodeId(2),
            change: LayoutChange::Size([25, 20])
        }])
        .unwrap());
    assert_eq!(layout.revision(), revision + 1);
    assert_eq!(
        layout
            .leaves()
            .iter()
            .map(|leaf| leaf.geometry.bounds.x)
            .collect::<Vec<_>>(),
        vec![5, 35, 70, 100]
    );
    assert_eq!(mounted_geometry(&layout)[3], isolated_before);
    assert_eq!(layout.changed_nodes(), &[NodeId(2), NodeId(3), NodeId(4)]);
    assert!(!layout
        .update(&[LayoutUpdate {
            id: NodeId(2),
            change: LayoutChange::Size([25, 20])
        }])
        .unwrap());
    assert_eq!(layout.revision(), revision + 1);
    assert!(layout
        .update(&[LayoutUpdate {
            id: NodeId(1),
            change: LayoutChange::Origin([8, 9])
        }])
        .unwrap());
    assert_eq!(
        layout
            .leaves()
            .iter()
            .map(|leaf| (leaf.id.0, leaf.geometry.bounds.x, leaf.geometry.bounds.y))
            .collect::<Vec<_>>(),
        vec![(2, 8, 9), (3, 38, 9), (4, 73, 9), (6, 100, 60)]
    );
    layout
        .update(&[LayoutUpdate {
            id: NodeId(1),
            change: LayoutChange::Gap(10),
        }])
        .unwrap();
    assert_eq!(
        layout
            .leaves()
            .iter()
            .map(|leaf| leaf.geometry.bounds.x)
            .collect::<Vec<_>>(),
        vec![8, 43, 83, 100]
    );
    assert_eq!(layout.changed_nodes(), &[NodeId(3), NodeId(4)]);
}

#[test]
fn mounted_fill_uses_parent_remaining_extent_and_zero_extent_resumes_same_identity() {
    let row = [leaf(20, 10, 1), Node::leaf([1, 10], 2).fill([true, false])];
    let mut layout = MountedLayout::mount(Node::row([100, 10], 5, &row)).unwrap();
    assert_eq!(
        mounted_geometry(&layout)
            .iter()
            .map(|(_, b, _)| *b)
            .collect::<Vec<_>>(),
        vec![[0, 0, 20, 10], [25, 0, 75, 10]]
    );
    assert!(layout.resize([120, 10]).unwrap());
    assert_eq!(
        mounted_geometry(&layout)
            .iter()
            .map(|(_, b, _)| *b)
            .collect::<Vec<_>>(),
        vec![[0, 0, 20, 10], [25, 0, 95, 10]]
    );
    let visible = mounted_geometry(&layout);
    assert!(layout.resize([0, 10]).unwrap());
    assert!(layout.suspended());
    assert_eq!(mounted_geometry(&layout), visible);
    assert!(layout.resize([120, 10]).unwrap());
    assert!(!layout.suspended());
    assert_eq!(mounted_geometry(&layout), visible);
    let revision = layout.revision();
    assert!(!layout.resize([120, 10]).unwrap());
    assert_eq!(layout.revision(), revision);
}

#[test]
fn mounted_edit_batches_refuse_atomically_without_changing_published_identity_or_geometry() {
    let row = [leaf(20, 10, 1), leaf(30, 10, 2)];
    let mut layout = MountedLayout::mount(Node::row([60, 10], 5, &row)).unwrap();
    let before = mounted_geometry(&layout);
    let revision = layout.revision();
    let dirty = layout.changed_nodes().to_vec();
    for invalid in [
        LayoutUpdate {
            id: NodeId(1),
            change: LayoutChange::Size([-1, 10]),
        },
        LayoutUpdate {
            id: NodeId(2),
            change: LayoutChange::Size([i64::MAX, 10]),
        },
        LayoutUpdate {
            id: NodeId(0),
            change: LayoutChange::Gap(-1),
        },
        LayoutUpdate {
            id: NodeId(99),
            change: LayoutChange::Size([1, 1]),
        },
    ] {
        assert!(layout
            .update(&[
                LayoutUpdate {
                    id: NodeId(1),
                    change: LayoutChange::Size([21, 10])
                },
                invalid
            ])
            .is_err());
        assert_eq!(mounted_geometry(&layout), before);
        assert_eq!(layout.revision(), revision);
        assert_eq!(layout.changed_nodes(), dirty);
        assert_eq!(layout.extent(), [60, 10]);
    }
    assert!(layout.resize([40, 10]).is_err());
    assert_eq!(mounted_geometry(&layout), before);
    assert_eq!(layout.revision(), revision);
}

#[test]
fn persistent_mount_preserves_depth_and_total_node_limits() {
    fn nested(depth: usize) -> Node<'static, u8> {
        if depth == 0 {
            return leaf(1, 1, 1);
        }
        Node::column([1, 1], 0, Box::leak(Box::new([nested(depth - 1)])))
    }
    let deep = MountedLayout::mount(nested(32)).unwrap();
    assert_eq!(deep.leaves()[0].id.0, 32);
    assert!(MountedLayout::mount(nested(33)).is_err());
    let children = vec![leaf(1, 1, 1).at(0, 0); 1023];
    let full = MountedLayout::mount(Node::layer([1, 1], &children)).unwrap();
    assert_eq!(full.leaves().len(), 1023);
    assert_eq!(full.leaves().last().unwrap().id.0, 1023);
    let children = vec![leaf(1, 1, 1).at(0, 0); 1024];
    assert!(MountedLayout::mount(Node::layer([1, 1], &children)).is_err());
}

#[test]
fn explicitly_clipped_parent_keeps_original_child_allocation_and_crops_inherited_clip() {
    let children = [leaf(20, 20, 1).at(15, 5)];
    let layers = [Node::layer([20, 20], &children).clipped().at(10, 10)];
    let layout = MountedLayout::mount(Node::layer([60, 60], &layers)).unwrap();
    assert_eq!(
        mounted_geometry(&layout),
        vec![(2, [25, 15, 20, 20], [25, 15, 5, 15])]
    );
    let layers = [Node::layer([20, 20], &children).at(10, 10)];
    assert!(MountedLayout::mount(Node::layer([60, 60], &layers)).is_err());
}

#[test]
fn component_clip_query_separates_leaf_source_local_clip_and_true_inherited_parent() {
    let children = [leaf(30, 20, 1).at(10, 20)];
    let mut layout = MountedLayout::mount(Node::layer([100, 80], &children)).unwrap();
    let b =
        |value: super::super::interaction::Bounds| [value.x, value.y, value.width, value.height];
    let first = layout.component_clips(NodeId(1)).unwrap();
    assert_eq!(b(first.source), [10, 20, 30, 20]);
    assert_eq!(b(first.inherited), [0, 0, 100, 80]);
    layout
        .update(&[LayoutUpdate {
            id: NodeId(1),
            change: LayoutChange::Clip(Some(super::super::interaction::Bounds {
                x: 5,
                y: 5,
                width: 20,
                height: 10,
            })),
        }])
        .unwrap();
    let locally_clipped = layout.component_clips(NodeId(1)).unwrap();
    assert_eq!(b(locally_clipped.source), [15, 25, 20, 10]);
    assert_eq!(b(locally_clipped.inherited), [0, 0, 100, 80]);
    assert_eq!(
        b(layout.geometry(NodeId(1)).unwrap().bounds),
        [10, 20, 30, 20]
    );
    let ordinary_clip = b(layout.geometry(NodeId(1)).unwrap().clip);
    layout
        .update(&[LayoutUpdate {
            id: NodeId(0),
            change: LayoutChange::Clip(Some(super::super::interaction::Bounds {
                x: 0,
                y: 0,
                width: 40,
                height: 80,
            })),
        }])
        .unwrap();
    let ancestor_changed = layout.component_clips(NodeId(1)).unwrap();
    assert_eq!(b(ancestor_changed.source), [15, 25, 20, 10]);
    assert_eq!(b(ancestor_changed.inherited), [0, 0, 40, 80]);
    assert_eq!(
        b(layout.geometry(NodeId(1)).unwrap().clip),
        ordinary_clip,
        "effective leaf geometry stays identical while its inherited dependency changes"
    );
    assert!(layout.component_clips(NodeId(999)).is_none());
}
