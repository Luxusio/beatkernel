//! Deferred native menu adapters. No winit window or graphics device opens.
use super::*;
use winit::dpi::PhysicalPosition;

#[test]
fn native_hit_gestures_and_pixel_wheels_share_the_rendered_viewport_without_bar_hits_or_stretch() {
    let logical = (WIDTH as u32, HEIGHT as u32);
    assert_eq!(
        logical_point((160.0, 0.0), (1280, 720), logical),
        Some((0.0, 0.0))
    );
    assert_eq!(
        logical_point((640.25, 360.5), (1280, 720), logical),
        Some((480.25, 360.5))
    );
    for point in [(0.0, 0.0), (159.99, 200.0), (1120.0, 200.0), (640.0, 720.0)] {
        assert_eq!(logical_point(point, (1280, 720), logical), None);
    }
    let button = Bounds {
        x: 0,
        y: 0,
        width: 100,
        height: 50,
    };
    let hit = |point| {
        logical_point(point, (1280, 720), logical)
            .filter(|&point| button.contains(point))
            .map(|_| ControlId(1))
    };
    let mut gesture = Gesture::default();
    gesture.press(hit((170.0, 10.0)));
    assert_eq!(gesture.release(hit((100.0, 10.0))), None);
    gesture.press(hit((100.0, 10.0)));
    assert_eq!(gesture.release(hit((170.0, 10.0))), None);
    gesture.press(hit((170.0, 10.0)));
    assert_eq!(gesture.release(hit((180.0, 20.0))), Some(ControlId(1)));
    for physical in [(1280, 720), (960, 960)] {
        assert_eq!(
            catalog_scroll_lines(
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 34.0)),
                physical
            ),
            1.0
        );
        assert_eq!(
            catalog_scroll_lines(MouseScrollDelta::LineDelta(0.0, -0.5), physical),
            -0.5
        );
    }
    assert_eq!(
        catalog_scroll_lines(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 68.0)),
            (1920, 1440)
        ),
        1.0
    );
    for physical in [(0, 720), (960, 0)] {
        assert!(logical_point((1.0, 1.0), physical, logical).is_none());
        assert!(
            !catalog_scroll_lines(
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 1.0)),
                physical
            )
            .is_finite()
        );
    }
    let mut steps = WheelSteps::default();
    for _ in 0..3 {
        assert_eq!(
            steps.push(catalog_scroll_lines(
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 8.5)),
                (960, 960)
            )),
            0
        );
    }
    assert_eq!(
        steps.push(catalog_scroll_lines(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 8.5)),
            (960, 960)
        )),
        1
    );
}
