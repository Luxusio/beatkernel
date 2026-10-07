use super::*;
use crate::scene::Scene;
use crate::ui::interaction::{Bounds, logical_point};

#[test]
fn translation_endpoints_zero_duration_and_clamped_completion() {
    let from = UiTranslation::new(30, -20).unwrap();
    let to = UiTranslation::new(-70, 80).unwrap();
    let motion = TranslationMotion::new(from, to, Duration::from_secs(10));
    assert_eq!(motion.sample(Duration::ZERO), from);
    assert_eq!(motion.sample(Duration::from_secs(5)).offset(), [-20, 30]);
    assert_eq!(motion.sample(Duration::from_secs(10)), to);
    assert_eq!(motion.sample(Duration::MAX), to);
    assert_eq!(
        TranslationMotion::new(from, to, Duration::ZERO).sample(Duration::ZERO),
        to
    );
    let small = TranslationMotion::new(
        UiTranslation::default(),
        UiTranslation::new(-3, 3).unwrap(),
        Duration::from_nanos(4),
    );
    assert_eq!(small.sample(Duration::from_nanos(1)).offset(), [0, 0]);
    assert_eq!(small.sample(Duration::from_nanos(3)).offset(), [-2, 2]);
}

#[test]
fn maximum_duration_and_opposite_extreme_offsets_do_not_overflow() {
    let limit = UiTranslation::MAX_OFFSET;
    let from = UiTranslation::new(-limit, limit).unwrap();
    let to = UiTranslation::new(limit, -limit).unwrap();
    let motion = TranslationMotion::new(from, to, Duration::MAX);
    assert_eq!(motion.sample(Duration::ZERO), from);
    let midpoint = Duration::MAX / 2;
    assert_eq!(motion.sample(midpoint).offset(), [-1, 1]);
    let before_end = Duration::MAX - Duration::from_nanos(1);
    assert_eq!(motion.sample(before_end).offset(), [limit - 1, -limit + 1]);
    assert_eq!(motion.sample(Duration::MAX), to);
    for invalid in [i32::MIN, i32::MAX, -limit - 1, limit + 1] {
        assert!(UiTranslation::new(invalid, 0).is_err());
        assert!(UiTranslation::new(0, invalid).is_err());
    }
}

#[test]
fn transformed_hit_projection_agrees_with_physical_viewport_and_original_clip() {
    let mut scene = Scene::new(960, 720);
    let button = Bounds {
        x: 100,
        y: 100,
        width: 80,
        height: 30,
    };
    scene.set_ui_translation(UiTranslation::new(40, -20).unwrap());
    // 1280x720 contains the logical surface with 160px side bars.
    let physical = (310.0, 90.0);
    let logical = logical_point(physical, (1280, 720), (960, 720)).unwrap();
    let local = scene.project_ui_point(logical).unwrap();
    assert_eq!(local, (110.0, 110.0));
    assert!(button.contains(local));
    assert!(!button.contains(logical));
    assert!(logical_point((159.0, 90.0), (1280, 720), (960, 720)).is_none());
    assert!(logical_point((1120.0, 90.0), (1280, 720), (960, 720)).is_none());
    assert_eq!(scene.project_ui_point((40.0, 0.0)), Some((0.0, 20.0)));
    for point in [
        (39.0, 0.0),
        (40.0, 700.0),
        (960.0, 20.0),
        (40.0, 720.0),
        (-1.0, 0.0),
        (f64::NAN, 0.0),
        (0.0, f64::INFINITY),
    ] {
        assert!(scene.project_ui_point(point).is_none(), "{point:?}");
    }
    scene.set_ui_translation(UiTranslation::new(-40, 20).unwrap());
    assert_eq!(scene.project_ui_point((0.0, 20.0)), Some((40.0, 0.0)));
    assert!(scene.project_ui_point((920.0, 20.0)).is_none());
    scene.clear();
    assert_eq!(scene.ui_translation(), UiTranslation::default());
    assert_eq!(scene.project_ui_point((100.0, 100.0)), Some((100.0, 100.0)));
    assert!(Scene::new(0, 720).project_ui_point((0.0, 0.0)).is_none());
}
