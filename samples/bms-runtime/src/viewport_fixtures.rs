//! Deferred portable presentation geometry; no GPU surface is constructed.
use crate::viewport::Viewport;

#[test]
fn contained_integer_viewports_preserve_the_complete_scene_and_bound_extreme_products() {
    for (physical, logical, expected) in [
        ([960, 720], [960, 720], [0, 0, 960, 720]),
        ([1280, 720], [960, 720], [160, 0, 960, 720]),
        ([960, 960], [960, 720], [0, 120, 960, 720]),
        ([1000, 1000], [960, 720], [0, 125, 1000, 750]),
        ([1281, 721], [960, 720], [160, 0, 961, 721]),
        ([1, u32::MAX], [960, 720], [0, 2147483647, 1, 1]),
        ([u32::MAX, 1], [960, 720], [2147483647, 0, 1, 1]),
        (
            [u32::MAX, u32::MAX],
            [960, 720],
            [0, 536870912, u32::MAX, 3221225471],
        ),
        (
            [u32::MAX, u32::MAX],
            [u32::MAX, u32::MAX],
            [0, 0, u32::MAX, u32::MAX],
        ),
        ([1, 1], [1, u32::MAX], [0, 0, 1, 1]),
    ] {
        let viewport = Viewport::new(physical, logical).unwrap();
        assert_eq!(viewport.rect(), expected);
        let [x, y, width, height] = viewport.rect();
        assert!(width > 0 && height > 0);
        assert!(u64::from(x) + u64::from(width) <= u64::from(physical[0]));
        assert!(u64::from(y) + u64::from(height) <= u64::from(physical[1]));
        assert!(physical[0] - width - x <= x + 1);
        assert!(physical[1] - height - y <= y + 1);
        let center = viewport
            .project((
                f64::from(x) + f64::from(width) / 2.0,
                f64::from(y) + f64::from(height) / 2.0,
            ))
            .unwrap();
        assert!((center.0 - f64::from(logical[0]) / 2.0).abs() < 0.000001);
        assert!((center.1 - f64::from(logical[1]) / 2.0).abs() < 0.000001);
        assert_eq!(viewport, viewport.clone());
    }
    for (physical, logical) in [
        ([0, 720], [960, 720]),
        ([960, 0], [960, 720]),
        ([960, 720], [0, 720]),
        ([960, 720], [960, 0]),
    ] {
        assert!(Viewport::new(physical, logical).is_err());
    }
    #[cfg(feature = "graphics")]
    {
        let scene = crate::scene::Scene::new(960, 720);
        assert_eq!(scene.logical_extent(), [960, 720]);
        assert_eq!(
            Viewport::new([1280, 720], scene.logical_extent())
                .unwrap()
                .rect(),
            [160, 0, 960, 720]
        );
    }
}

#[test]
fn inverse_projection_excludes_menu_bars_but_preserves_fractional_and_captured_off_surface_touch_positions()
 {
    let wide = Viewport::new([1280, 720], [960, 720]).unwrap();
    assert_eq!(wide.project((160.0, 0.0)), Some((0.0, 0.0)));
    assert_eq!(wide.project((640.25, 360.5)), Some((480.25, 360.5)));
    assert_eq!(wide.project((1119.75, 719.5)), Some((959.75, 719.5)));
    for point in [
        (159.999, 0.0),
        (1120.0, 0.0),
        (640.0, 720.0),
        (-1.0, 10.0),
        (1281.0, 10.0),
    ] {
        assert_eq!(wide.project(point), None);
    }
    assert_eq!(
        wide.project_unclipped((0.0, 360.0)).unwrap(),
        (-160.0, 360.0)
    );
    assert_eq!(
        wide.project_unclipped((1280.0, 360.0)).unwrap(),
        (1120.0, 360.0)
    );
    assert_eq!(
        wide.project_unclipped((-12.5, 800.25)).unwrap(),
        (-172.5, 800.25)
    );
    let tall = Viewport::new([1000, 1000], [960, 720]).unwrap();
    assert_eq!(tall.project((500.0, 125.0)), Some((480.0, 0.0)));
    assert_eq!(tall.project((500.0, 875.0)), None);
    assert_eq!(
        tall.project_unclipped((500.0, 0.0)).unwrap(),
        (480.0, -120.0)
    );
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(wide.project((invalid, 1.0)).is_none());
        assert!(wide.project((1.0, invalid)).is_none());
        assert!(wide.project_unclipped((invalid, 1.0)).is_err());
        assert!(wide.project_unclipped((1.0, invalid)).is_err());
    }
    let tiny = Viewport::new([1, 1], [u32::MAX, u32::MAX]).unwrap();
    assert!(tiny.project_unclipped((f64::MAX, f64::MAX)).is_err());
}

#[test]
fn extreme_finite_captured_points_preserve_identity_and_downscale_without_intermediate_overflow() {
    let identity = Viewport::new([u32::MAX, u32::MAX], [u32::MAX, u32::MAX]).unwrap();
    assert_eq!(
        identity.project_unclipped((f64::MAX, -f64::MAX)).unwrap(),
        (f64::MAX, -f64::MAX)
    );
    let half = u32::MAX / 2;
    let smaller = Viewport::new([u32::MAX, u32::MAX], [half, half]).unwrap();
    let result = smaller.project_unclipped((f64::MAX, -f64::MAX)).unwrap();
    assert!(result.0.is_finite() && result.1.is_finite());
    let expected = f64::MAX / f64::from(u32::MAX) * f64::from(half);
    assert_eq!(result, (expected, -expected));
    assert!(result.0 > 0.0 && result.0 < f64::MAX);
    assert!(smaller.project_unclipped((f64::INFINITY, 0.0)).is_err());
}
