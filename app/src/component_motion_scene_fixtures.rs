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
