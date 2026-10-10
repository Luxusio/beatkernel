//! Real Scene component uniforms, painter batches, inverse clips and note caches.
use super::*;
use crate::{screen_lifecycle::ScreenInstanceId, ui::layout::NodeId};
use std::sync::Arc;

fn key(screen: u64, node: usize) -> UiComponentKey {
    UiComponentKey {
        screen: ScreenInstanceId(screen),
        node: NodeId(node),
    }
}

fn rectangles(scene: &Scene) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    scene
        .rectangles()
        .iter()
        .map(|rectangle| (rectangle.bounds, rectangle.color, rectangle.uv))
        .collect()
}

#[test]
fn fractional_component_inverse_matches_transformed_source_and_fixed_parent_half_open_clips() {
    let mut scene = Scene::with_capacity(100, 100, 4);
    scene.rect(10, 20, 30, 20, 0xabcdef);
    let id = scene
        .bind_component(
            key(41, 1),
            &[0..1],
            ClipRect::new([10, 20, 30, 20]).unwrap(),
            Some(ClipRect::new([0, 0, 60, 100]).unwrap()),
        )
        .unwrap();
    let transform = UiTransform::new([5.5, 3.25], [2.0, 0.5], 0.75).unwrap();
    let original = rectangles(&scene);
    let stamp = scene.geometry_stamp().1;
    let identity = Arc::clone(scene.geometry_stamp().0);
    assert!(scene.set_component_transforms(&[(id, transform)]).unwrap());
    assert_eq!(scene.component_transform(id), Some(transform));
    // Scale about the source clip's top-left[10,20], then translate: the
    // rectangle becomes[15.5,23.25,60,10], cropped at fixed parentx60.
    assert_eq!(
        scene.project_component_point(id, (15.5, 23.25)),
        Some((10.0, 20.0))
    );
    assert_eq!(
        scene.project_component_point(id, (35.5, 28.25)),
        Some((20.0, 30.0))
    );
    for point in [
        (15.499, 28.0),
        (60.0, 28.0),
        (35.5, 33.25),
        (35.5, 23.249),
        (f64::NAN, 20.0),
        (30.0, f64::INFINITY),
    ] {
        assert_eq!(scene.project_component_point(id, point), None, "{point:?}");
    }
    assert_eq!(rectangles(&scene), original);
    assert_eq!(scene.geometry_stamp().1, stamp);
    assert!(Arc::ptr_eq(scene.geometry_stamp().0, &identity));
    assert!(!scene.set_component_transforms(&[(id, transform)]).unwrap());
    scene.set_ui_translation(UiTranslation::new(4, -2).unwrap());
    assert_eq!(
        scene.project_component_point(id, (39.5, 26.25)),
        Some((20.0, 30.0))
    );
    let hidden = UiTransform::new(transform.offset(), transform.scale(), 0.0).unwrap();
    scene.set_component_transforms(&[(id, hidden)]).unwrap();
    assert_eq!(scene.project_component_point(id, (39.5, 26.25)), None);
}

#[test]
fn range_binding_and_transform_batch_refusal_preserve_accepted_scene_and_other_component() {
    let mut scene = Scene::with_capacity(100, 100, 8);
    for x in [10, 30, 50] {
        scene.rect(x, 10, 10, 10, 0xabcdef);
    }
    let clip = ClipRect::new([0, 0, 100, 100]).unwrap();
    let first = scene
        .bind_component(key(42, 1), &[0..1], clip, Some(clip))
        .unwrap();
    let second = scene
        .bind_component(key(42, 2), &[1..2], clip, Some(clip))
        .unwrap();
    let stamp = scene.geometry_stamp().1;
    let accepted = rectangles(&scene);
    for ranges in [vec![2..1], vec![2..4], vec![0..2, 1..3], vec![0..1]] {
        assert!(scene
            .bind_component(key(42, 3), &ranges, clip, Some(clip))
            .is_err());
        assert_eq!(scene.geometry_stamp().1, stamp);
        assert_eq!(rectangles(&scene), accepted);
    }
    let pose = UiTransform::new([0.5, 1.25], [1.0, 1.0], 0.5).unwrap();
    assert!(scene
        .set_component_transforms(&[(first, pose), (first, UiTransform::default())])
        .is_err());
    assert_eq!(
        scene.component_transform(first),
        Some(UiTransform::default())
    );
    assert_eq!(
        scene.component_transform(second),
        Some(UiTransform::default())
    );
    assert!(scene.set_component_transforms(&[(first, pose)]).unwrap());
    assert_eq!(
        scene.component_transform(second),
        Some(UiTransform::default())
    );
    assert_eq!(scene.dispose_components(ScreenInstanceId(42)), 2);
    assert!(scene.set_component_transforms(&[(first, pose)]).is_err());
    assert_eq!(scene.project_component_point(first, (10.0, 10.0)), None);
    let replacement = scene
        .bind_component(key(42, 1), &[0..1], clip, Some(clip))
        .unwrap();
    assert_ne!(
        replacement, first,
        "reusing a slot must not revive an obsolete component ID"
    );
    assert!(scene
        .set_component_transforms(&[(replacement, pose), (second, pose)])
        .is_err());
    assert_eq!(
        scene.component_transform(replacement),
        Some(UiTransform::default())
    );
    assert_eq!(rectangles(&scene), accepted);
}

#[test]
fn empty_retained_parts_keep_identity_and_charge_the_bounded_slot_without_inventing_paint() {
    let mut scene = Scene::with_capacity(100, 100, 4);
    scene.rect(10, 20, 30, 20, 0xabcdef);
    let accepted = rectangles(&scene);
    let pointer = scene.rectangles.as_ptr();
    let stamp = scene.geometry_stamp().1;
    let identity = Arc::clone(scene.geometry_stamp().0);
    let batches: Vec<_> = scene
        .batches()
        .iter()
        .map(|batch| (batch.first, batch.count, batch.component, batch.playfield))
        .collect();
    let clip = ClipRect::new([10, 20, 30, 20]).unwrap();
    let mut ids = Vec::new();
    for node in 0..64 {
        let ranges = if node % 2 == 0 { vec![] } else { vec![0..0] };
        let component_key = key(49, node);
        let id = scene
            .bind_component(component_key, &ranges, clip, None)
            .unwrap();
        assert_eq!(scene.component_id(component_key), Some(id));
        assert_eq!(scene.project_component_point(id, (11.0, 21.0)), None);
        ids.push(id);
        assert_eq!(rectangles(&scene), accepted);
        assert_eq!(scene.rectangles.as_ptr(), pointer);
        assert_eq!(scene.geometry_stamp().1, stamp);
        assert!(Arc::ptr_eq(scene.geometry_stamp().0, &identity));
        assert_eq!(
            scene
                .batches()
                .iter()
                .map(|batch| (batch.first, batch.count, batch.component, batch.playfield))
                .collect::<Vec<_>>(),
            batches
        );
    }
    assert!(
        scene.bind_component(key(49, 64), &[], clip, None).is_err(),
        "metadata-only nodes still consume the64 component budget"
    );
    let pose = UiTransform::new([0.5, -0.25], [1.0, 1.0], 0.5).unwrap();
    assert!(scene
        .set_component_transforms(&ids.iter().map(|&id| (id, pose)).collect::<Vec<_>>())
        .unwrap());
    assert_eq!(rectangles(&scene), accepted);
    assert_eq!(scene.geometry_stamp().1, stamp);
    assert_eq!(scene.rectangles.as_ptr(), pointer);
}

#[test]
fn component_slots_are_bounded_and_binding_identity_survives_ordinary_clear_recomposition() {
    let mut scene = Scene::with_capacity(200, 100, 80);
    for index in 0..65 {
        scene.rect(index * 2, 10, 1, 1, 0xabcdef);
    }
    let clip = ClipRect::new([0, 0, 200, 100]).unwrap();
    let mut ids = Vec::new();
    for index in 0..64u32 {
        ids.push(
            scene
                .bind_component(
                    key(43, index as usize),
                    &[index..index + 1],
                    clip,
                    Some(clip),
                )
                .unwrap(),
        );
    }
    let stamp = scene.geometry_stamp().1;
    assert!(scene
        .bind_component(key(43, 64), &[64..65], clip, Some(clip))
        .is_err());
    assert_eq!(scene.geometry_stamp().1, stamp);
    scene.clear();
    scene.rect(10, 10, 10, 10, 0xabcdef);
    let rebuilt = scene
        .bind_component(key(43, 0), &[0..1], clip, Some(clip))
        .unwrap();
    assert_eq!(rebuilt, ids[0]);
    assert_eq!(scene.component_id(key(43, 0)), Some(rebuilt));
    let invisible = Scene::bind_component(&mut scene, key(44, 7), &[0..1], clip, None);
    assert!(
        invisible.is_err(),
        "a different key cannot steal an existing component range"
    );
    let mut hidden_scene = Scene::with_capacity(200, 100, 4);
    hidden_scene.rect(10, 10, 10, 10, 0xabcdef);
    let hidden = hidden_scene
        .bind_component(key(44, 1), &[0..1], clip, None)
        .unwrap();
    assert_eq!(
        hidden_scene.project_component_point(hidden, (11.0, 11.0)),
        None,
        "an absent inherited clip never invents a visible hit surface"
    );
}

#[test]
fn per_component_updates_keep_actual_indexed_note_instances_scratch_and_painter_order() {
    let source = beatkernel_bms::parse(
        "#BPM 120\n#WAV01 head.wav\n#00011:0101\n",
        Default::default(),
    )
    .unwrap();
    let chart =
        crate::player_chart::PlayerChart::from_compiled(&source, &source.compile().unwrap().chart)
            .unwrap();
    let mut scene = Scene::with_capacity(960, 720, 8);
    scene.rect(10, 10, 20, 10, 0xabcdef);
    scene
        .playfield(
            &chart,
            beatkernel::time::Timestamp::ZERO,
            1_000_000_000,
            crate::ui::interaction::Bounds {
                x: 80,
                y: 106,
                width: 640,
                height: 528,
            },
        )
        .unwrap();
    scene.rect(40, 10, 20, 10, 0xffffff);
    let clip = ClipRect::new([0, 0, 960, 720]).unwrap();
    let unbound_stamp = scene.geometry_stamp().1;
    // Refuse a range spanning the actual note layer before any other component
    // owns these rectangles, so overlap alone cannot explain the refusal.
    assert!(scene
        .bind_component(key(45, 3), &[0..2], clip, Some(clip))
        .is_err());
    assert_eq!(scene.geometry_stamp().1, unbound_stamp);
    let first = scene
        .bind_component(key(45, 1), &[0..1], clip, Some(clip))
        .unwrap();
    let second = scene
        .bind_component(key(45, 2), &[1..2], clip, Some(clip))
        .unwrap();
    let accepted = rectangles(&scene);
    let instance_pointer = scene.rectangles.as_ptr();
    let note_instances = Arc::clone(&scene.playfields[0].instances);
    let scratch = scene.visible_note_indices.as_ptr();
    let membership = scene.visible_note_indices.clone();
    let capacity = scene.visible_note_indices.capacity();
    let stamp = scene.geometry_stamp().1;
    let painter_order: Vec<_> = scene
        .batches()
        .iter()
        .map(|batch| (batch.first, batch.count, batch.playfield))
        .collect();
    assert_eq!(
        painter_order
            .iter()
            .filter(|(_, _, playfield)| playfield.is_some())
            .count(),
        1
    );
    for frame in 0..20 {
        let pose = UiTransform::new([frame as f32 * 0.25, -0.5], [1.0, 1.0], 0.75).unwrap();
        scene.set_component_transforms(&[(first, pose)]).unwrap();
        assert_eq!(
            scene.component_transform(second),
            Some(UiTransform::default())
        );
        assert_eq!(rectangles(&scene), accepted);
        assert_eq!(scene.rectangles.as_ptr(), instance_pointer);
        assert_eq!(scene.geometry_stamp().1, stamp);
        assert!(Arc::ptr_eq(&scene.playfields[0].instances, &note_instances));
        assert_eq!(scene.visible_note_indices.as_ptr(), scratch);
        assert_eq!(scene.visible_note_indices.capacity(), capacity);
        assert_eq!(scene.visible_note_indices, membership);
        assert_eq!(
            scene
                .batches()
                .iter()
                .map(|batch| (batch.first, batch.count, batch.playfield))
                .collect::<Vec<_>>(),
            painter_order
        );
    }
}

#[test]
fn presented_visible_rect_fractional_pivot_source_and_parent_clips_2026101005() {
    let mut scene = Scene::with_capacity(100, 80, 4);
    scene.rect(0, 0, 90, 80, 0xabcdef);
    let component_key = key(51, 1);
    let id = scene
        .bind_component_clipped(
            component_key,
            &[0..1],
            [10, 20],
            Some(ClipRect::new([12, 22, 20, 16]).unwrap()),
            Some(ClipRect::new([25, 25, 25, 10]).unwrap()),
        )
        .unwrap();
    scene
        .set_component_transforms(&[(id, UiTransform::new([5.5, 3.25], [2.0, 0.5], 0.75).unwrap())])
        .unwrap();
    scene.set_ui_translation(UiTranslation::new(4, -2).unwrap());
    let pose = scene.presented_pose();
    let geometry = rectangles(&scene);
    let identity = Arc::clone(scene.geometry_stamp().0);
    let revision = scene.geometry_stamp().1;

    // Source clipping gives [12,22,32,38]. Scale about the node pivot,
    // not the source clip origin, gives [19.5,24.25,59.5,32.25].
    // Fixed parent clipping then translation gives these presented endpoints.
    assert_eq!(
        pose.visible_rect(Some(component_key), [0.0, 0.0, 90.0, 80.0]),
        Some([29.0, 23.0, 54.0, 30.25])
    );
    assert_eq!(
        pose.visible_rect(Some(component_key), [20.0, 26.0, 28.0, 34.0]),
        Some([39.5, 24.25, 54.0, 28.25])
    );
    assert_eq!(
        pose.project(Some(component_key), (29.0, 23.0)),
        Some((14.75, 23.5))
    );
    assert_eq!(
        pose.project(Some(component_key), (39.5, 26.25)),
        Some((20.0, 30.0))
    );
    for point in [(28.999, 26.0), (54.0, 26.0), (40.0, 30.25)] {
        assert_eq!(pose.project(Some(component_key), point), None, "{point:?}");
    }
    assert_eq!(rectangles(&scene), geometry);
    assert_eq!(scene.geometry_stamp().1, revision);
    assert!(Arc::ptr_eq(scene.geometry_stamp().0, &identity));
}

#[test]
fn presented_visible_rect_local_viewport_then_translation_then_screen_clip_2026101005() {
    let mut scene = Scene::with_capacity(100, 80, 4);
    scene.rect(0, 0, 100, 100, 0xabcdef);
    let component_key = key(52, 1);
    let id = scene
        .bind_component_clipped(
            component_key,
            &[0..1],
            [0, 0],
            Some(ClipRect::new([0, 0, 100, 100]).unwrap()),
            Some(ClipRect::new([-50, -50, 200, 200]).unwrap()),
        )
        .unwrap();
    scene
        .set_component_transforms(&[(
            id,
            UiTransform::new([-10.5, -5.25], [1.0, 1.0], 1.0).unwrap(),
        )])
        .unwrap();
    scene.set_ui_translation(UiTranslation::new(20, 10).unwrap());
    let pose = scene.presented_pose();
    // Local viewport removes negative local coordinates even though the
    // translation would have moved them onto the presented screen.
    assert_eq!(
        pose.visible_rect(Some(component_key), [0.0, 0.0, 100.0, 100.0]),
        Some([20.0, 10.0, 100.0, 80.0])
    );
    assert_eq!(
        pose.project(Some(component_key), (20.0, 10.0)),
        Some((10.5, 5.25))
    );
    assert_eq!(
        pose.project(Some(component_key), (50.0, 40.0)),
        Some((40.5, 35.25))
    );
    for point in [(19.999, 30.0), (40.0, 9.999), (100.0, 40.0), (50.0, 80.0)] {
        assert_eq!(pose.project(Some(component_key), point), None, "{point:?}");
    }
}

#[test]
fn presented_visible_rect_hidden_clips_opacity_and_empty_intersections_2026101005() {
    let clip = ClipRect::new([10, 20, 30, 20]).unwrap();
    for (source, parent, opacity) in [
        (None, Some(clip), 1.0),
        (Some(clip), None, 1.0),
        (Some(clip), Some(clip), 0.0),
    ] {
        let mut scene = Scene::with_capacity(100, 80, 4);
        scene.rect(10, 20, 30, 20, 0xabcdef);
        let component_key = key(53, 1);
        let id = scene
            .bind_component_clipped(component_key, &[0..1], [10, 20], source, parent)
            .unwrap();
        scene
            .set_component_transforms(&[(
                id,
                UiTransform::new([0.0, 0.0], [1.0, 1.0], opacity).unwrap(),
            )])
            .unwrap();
        let pose = scene.presented_pose();
        assert_eq!(
            pose.visible_rect(Some(component_key), [10.0, 20.0, 40.0, 40.0]),
            None
        );
        assert_eq!(pose.project(Some(component_key), (20.0, 30.0)), None);
    }
    let mut scene = Scene::with_capacity(100, 80, 4);
    scene.rect(10, 20, 30, 20, 0xabcdef);
    let component_key = key(53, 2);
    let id = scene
        .bind_component(component_key, &[0..1], clip, Some(clip))
        .unwrap();
    let pose = scene.presented_pose();
    // Touching the source edge has no area; a valid source moved beyond its
    // fixed parent also has no area.
    assert_eq!(
        pose.visible_rect(Some(component_key), [40.0, 20.0, 50.0, 40.0]),
        None
    );
    scene
        .set_component_transforms(&[(id, UiTransform::new([30.0, 0.0], [1.0, 1.0], 1.0).unwrap())])
        .unwrap();
    assert_eq!(
        scene
            .presented_pose()
            .visible_rect(Some(component_key), [10.0, 20.0, 40.0, 40.0]),
        None
    );
}

#[test]
fn presented_visible_rect_snapshot_survives_live_change_and_disposal_2026101005() {
    let mut scene = Scene::with_capacity(100, 80, 4);
    scene.rect(10, 20, 30, 20, 0xabcdef);
    let component_key = key(54, 1);
    let clip = ClipRect::new([10, 20, 30, 20]).unwrap();
    let parent = ClipRect::new([0, 0, 100, 80]).unwrap();
    let id = scene
        .bind_component(component_key, &[0..1], clip, Some(parent))
        .unwrap();
    scene
        .set_component_transforms(&[(id, UiTransform::new([5.5, 3.25], [2.0, 0.5], 1.0).unwrap())])
        .unwrap();
    let accepted = scene.presented_pose();
    let bounds = [10.0, 20.0, 40.0, 40.0];
    let identity = Arc::clone(scene.geometry_stamp().0);
    let revision = scene.geometry_stamp().1;
    scene
        .set_component_transforms(&[(id, UiTransform::new([20.0, 0.0], [1.0, 1.0], 1.0).unwrap())])
        .unwrap();
    scene.set_ui_translation(UiTranslation::new(7, 2).unwrap());
    assert_eq!(
        scene
            .presented_pose()
            .visible_rect(Some(component_key), bounds),
        Some([37.0, 22.0, 67.0, 42.0])
    );
    assert_eq!(
        accepted.visible_rect(Some(component_key), bounds),
        Some([15.5, 23.25, 75.5, 33.25])
    );
    assert_eq!(
        accepted.project(Some(component_key), (35.5, 28.25)),
        Some((20.0, 30.0))
    );
    assert_eq!(accepted.visible_rect(Some(key(54, 99)), bounds), None);
    assert_eq!(scene.dispose_components(ScreenInstanceId(54)), 1);
    assert_eq!(
        scene
            .presented_pose()
            .visible_rect(Some(component_key), bounds),
        None
    );
    assert_eq!(
        accepted.visible_rect(Some(component_key), bounds),
        Some([15.5, 23.25, 75.5, 33.25])
    );
    assert_eq!(
        accepted.project(Some(component_key), (35.5, 28.25)),
        Some((20.0, 30.0))
    );
    assert_eq!(scene.geometry_stamp().1, revision);
    assert!(Arc::ptr_eq(scene.geometry_stamp().0, &identity));
}

#[test]
fn presented_visible_rect_plain_translation_and_invalid_endpoints_2026101005() {
    let mut scene = Scene::with_capacity(100, 80, 0);
    scene.set_ui_translation(UiTranslation::new(25, -10).unwrap());
    let pose = scene.presented_pose();
    assert_eq!(
        pose.visible_rect(None, [-10.0, 5.0, 130.0, 90.0]),
        Some([25.0, 0.0, 100.0, 70.0])
    );
    assert_eq!(
        pose.visible_rect(None, [10.0, 20.0, 30.0, 40.0]),
        Some([35.0, 10.0, 55.0, 30.0])
    );
    assert_eq!(pose.project(None, (25.0, 0.0)), Some((0.0, 10.0)));
    assert_eq!(pose.project(None, (50.0, 40.0)), Some((25.0, 50.0)));
    for point in [(24.999, 40.0), (100.0, 40.0), (50.0, 70.0)] {
        assert_eq!(pose.project(None, point), None, "{point:?}");
    }
    for bounds in [
        [10.0, 20.0, 10.0, 40.0],
        [10.0, 20.0, 30.0, 20.0],
        [30.0, 20.0, 10.0, 40.0],
        [10.0, 40.0, 30.0, 20.0],
        [100.0, 0.0, 110.0, 10.0],
    ] {
        assert_eq!(pose.visible_rect(None, bounds), None, "{bounds:?}");
    }
    for axis in 0..4 {
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut bounds = [10.0, 20.0, 30.0, 40.0];
            bounds[axis] = invalid;
            assert_eq!(pose.visible_rect(None, bounds), None, "{bounds:?}");
        }
    }
    assert_eq!(
        pose.visible_rect(Some(key(55, 1)), [10.0, 20.0, 30.0, 40.0]),
        None
    );
}
