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
