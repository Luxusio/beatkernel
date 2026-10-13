//! Payload math only: callers must protect publication and selection with their revision.
use super::*;
use crate::audio::{count_heap_calls, fail_allocation};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use std::sync::atomic::Ordering;

fn rig(hz: u32, capacity: usize, gate: Option<u64>, end: Option<u64>) -> (CommandProducer, Mixer) {
    let (producer, mut mixer) = bare_rig(hz, gate, end);
    mixer.enable_source_playback_history(capacity).unwrap();
    (producer, mixer)
}
fn bare_rig(hz: u32, gate: Option<u64>, end: Option<u64>) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(hz, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(1024, 1024, 1).unwrap()).unwrap();
    let (mut producer, consumer) = if gate.is_some() {
        command_queue_with_start_gate(8)
    } else {
        command_queue(8)
    }
    .unwrap();
    if let Some(frame) = gate {
        producer.schedule_start_at(frame).unwrap();
    }
    let mut config = MixerConfig::new(
        format,
        ClockDomainId(7),
        Timestamp::ZERO,
        AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    let mixer = Mixer::new(config, bank, consumer).unwrap();
    (producer, mixer)
}
fn history(mixer: &Mixer) -> &SourcePlaybackHistory {
    mixer.source_playback_history().unwrap()
}
fn select(
    publication: &SourcePlaybackPublication,
    mixer: &Mixer,
    time: TargetTime,
) -> Option<PublishedSourcePlayback> {
    let history = history(mixer);
    let query = InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap();
    publication.select(
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
        |frame| {
            query
                .compare_source_time_frame(frame, history.source_rate())
                .ok()
        },
    )
}
fn at(frame: u64) -> TargetTime {
    TargetTime::from_frames(frame, 1000).unwrap()
}
fn words(publication: &SourcePlaybackPublication) -> Vec<[u64; 3]> {
    publication
        .test_slots()
        .iter()
        .map(|slot| std::array::from_fn(|i| slot[i].load(Ordering::SeqCst)))
        .collect()
}

#[test]
fn supplied_buffer_geometry_reserves_exact_history_and_publication_slot_budget() {
    // Portable supplied geometry exercises the shared helper, not native negotiation.
    let (_, mut mixer) = bare_rig(1000, None, None);
    prepare_native_source_history(&mut mixer, 64, 4354).unwrap();
    assert_eq!(history(&mixer).capacity(), 4418);
    assert!(history(&mixer).is_empty());
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    assert_eq!(publication.test_slots().len(), 4418);
    assert_eq!(publication.slots.capacity(), 4418);
    let slot_bytes = std::mem::size_of_val(publication.test_slots());
    assert_eq!(slot_bytes, 106_032);
    let ceiling_bytes = (AudioLimits::MAX_RENDER_FRAMES + 4354)
        * std::mem::size_of::<[std::sync::atomic::AtomicU64; 3]>();
    assert_eq!(ceiling_bytes, 25_270_320);
    assert!(ceiling_bytes > slot_bytes * 200);
    assert!(select(&publication, &mixer, at(0)).is_none());
}

#[test]
fn supplied_geometry_cold_enable_keeps_nonzero_frozen_creation_identity() {
    let (mut producer, mut mixer) = bare_rig(1000, None, None);
    mixer.render(&mut [0.0; 5]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 3]).unwrap();
    let before = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.counters(),
        producer.counters(),
        mixer.output_frame_basis(),
    );
    prepare_native_source_history(&mut mixer, 64, 4354).unwrap();
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.counters(),
            producer.counters(),
            mixer.output_frame_basis(),
        ),
        before
    );
    assert_eq!(
        (
            history(&mixer).source_rate(),
            history(&mixer).initial_physical_frame(),
            history(&mixer).initial_playback_frame(),
            history(&mixer).pulled_end_frame(),
        ),
        (1000, 8, 5, 8)
    );
    mixer.render(&mut [0.0; 2]).unwrap();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    let selected = select(&publication, &mixer, at(8)).unwrap();
    assert_eq!(selected.span.state(), SourcePlaybackState::Frozen);
    assert_eq!(selected.span.playback_start(), 5);
    assert_eq!(selected.pulled_end_frame, 10);
    assert!(select(&publication, &mixer, at(7)).is_none());
}

#[test]
fn supplied_geometry_preserves_retired_history_and_nonce_without_heap_or_headroom_math() {
    let (mut producer, mut mixer) = rig(1000, 8, None, None);
    for paused in [false, true, false, true] {
        producer.request_pause(paused);
        mixer.render(&mut [0.0; 2]).unwrap();
    }
    assert_eq!(mixer.retire_source_playback_before(5).unwrap(), 2);
    let snapshot = (
        history(&mixer).capacity(),
        history(&mixer).source_rate(),
        history(&mixer).initial_physical_frame(),
        history(&mixer).initial_playback_frame(),
        history(&mixer).pulled_end_frame(),
        history(&mixer).retained_start_frame(),
        history(&mixer).retirement_revision(),
        history(&mixer).len(),
        history(&mixer).span(0),
        history(&mixer).span(1),
    );
    assert_eq!(snapshot.6, 1);
    let cursors = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.counters(),
        producer.counters(),
        mixer.output_frame_basis(),
    );
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    for (frames, expected) in [
        (0, Err(AudioError::InvalidCapacity)),
        (257, Err(AudioError::RenderCapacity)),
        (64, Ok(())),
        (256, Ok(())),
    ] {
        let (result, calls) =
            count_heap_calls(|| prepare_native_source_history(&mut mixer, frames, usize::MAX));
        assert_eq!(result, expected);
        assert_eq!(calls, 0);
        assert_eq!(
            (
                history(&mixer).capacity(),
                history(&mixer).source_rate(),
                history(&mixer).initial_physical_frame(),
                history(&mixer).initial_playback_frame(),
                history(&mixer).pulled_end_frame(),
                history(&mixer).retained_start_frame(),
                history(&mixer).retirement_revision(),
                history(&mixer).len(),
                history(&mixer).span(0),
                history(&mixer).span(1),
            ),
            snapshot
        );
        assert_eq!(
            (
                mixer.frame_cursor(),
                mixer.playback_frame_cursor(),
                mixer.counters(),
                producer.counters(),
                mixer.output_frame_basis(),
            ),
            cursors
        );
        assert!(publication.unchanged(history(&mixer)));
    }
}

#[test]
fn supplied_geometry_refusals_preserve_bare_mixer_and_pending_pause() {
    let (mut producer, mut mixer) = bare_rig(1000, None, None);
    mixer.render(&mut [0.0; 5]).unwrap();
    producer.request_pause(true);
    let before = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.counters(),
        producer.counters(),
        mixer.output_frame_basis(),
    );
    for (frames, headroom, expected) in [
        (0, 4354, AudioError::InvalidCapacity),
        (257, 4354, AudioError::RenderCapacity),
        (64, usize::MAX, AudioError::Overflow),
    ] {
        let (result, calls) =
            count_heap_calls(|| prepare_native_source_history(&mut mixer, frames, headroom));
        assert_eq!(result, Err(expected));
        assert_eq!(calls, 0);
        assert!(mixer.source_playback_history().is_none());
    }
    assert_eq!(
        fail_allocation(0, || prepare_native_source_history(&mut mixer, 64, 4354)),
        Err(AudioError::AllocationFailed)
    );
    assert!(mixer.source_playback_history().is_none());
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.counters(),
            producer.counters(),
            mixer.output_frame_basis(),
        ),
        before
    );
    prepare_native_source_history(&mut mixer, 64, 4354).unwrap();
    assert!(matches!(
        fail_allocation(0, || SourcePlaybackPublication::new(history(&mixer))),
        Err(AudioError::AllocationFailed)
    ));
    assert!(history(&mixer).is_empty());
    assert_eq!(history(&mixer).capacity(), 4418);
    assert_eq!(history(&mixer).initial_physical_frame(), 5);
    assert_eq!(history(&mixer).initial_playback_frame(), 5);
    assert_eq!(history(&mixer).pulled_end_frame(), 5);
    assert_eq!(history(&mixer).retirement_revision(), 0);
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.counters(),
            producer.counters(),
            mixer.output_frame_basis(),
        ),
        before
    );
    assert!(mixer.render(&mut [0.0; 2]).unwrap().paused);
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (7, 5)
    );
}

#[test]
fn cold_empty_seed_and_allocation_refusal_never_fabricate_source_pull() {
    let (_, mut mixer) = rig(1000, 3, None, None);
    assert!(matches!(
        fail_allocation(0, || SourcePlaybackPublication::new(history(&mixer))),
        Err(AudioError::AllocationFailed)
    ));
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    assert!(publication.unchanged(history(&mixer)));
    assert_eq!(publication.initial_playback_frame(), 0);
    assert!(select(&publication, &mixer, at(0)).is_none());
    mixer.render(&mut []).unwrap();
    assert!(publication.unchanged(history(&mixer)));
    assert!(select(&publication, &mixer, at(0)).is_none());
}

#[test]
fn one_actual_callback_three_intervals_selects_exact_new_boundary_and_closed_pull_endpoint() {
    let (_, mut mixer) = rig(1000, 3, Some(2), Some(3));
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    mixer.render(&mut [0.0; 8]).unwrap();
    assert!(!publication.unchanged(history(&mixer)));
    let ((), calls) = count_heap_calls(|| publication.publish(Some(history(&mixer))));
    assert_eq!(calls, 0);
    for (frame, start, playback, state, end) in [
        (0, 0, 0, SourcePlaybackState::Frozen, Some(2)),
        (1, 0, 0, SourcePlaybackState::Frozen, Some(2)),
        (2, 2, 0, SourcePlaybackState::Running, Some(5)),
        (4, 2, 0, SourcePlaybackState::Running, Some(5)),
        (5, 5, 3, SourcePlaybackState::Frozen, None),
        (8, 5, 3, SourcePlaybackState::Frozen, None),
    ] {
        let (selected, calls) = count_heap_calls(|| select(&publication, &mixer, at(frame)));
        assert_eq!(calls, 0);
        let selected = selected.unwrap();
        assert_eq!(
            (
                selected.span.physical_start(),
                selected.span.playback_start(),
                selected.span.state(),
                selected.span.physical_end()
            ),
            (start, playback, state, end)
        );
        assert_eq!(selected.pulled_end_frame, 8);
    }
    assert!(select(
        &publication,
        &mixer,
        TargetTime::new(0, 16_000_001, 2_000_000_000).unwrap()
    )
    .is_none());
    assert!(select(&publication, &mixer, at(9)).is_none());
}

#[test]
fn actual_pause_resume_merges_frontier_without_rewriting_old_slots_or_allocating() {
    let (mut producer, mut mixer) = rig(1000, 5, None, None);
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    mixer.render(&mut [0.0; 4]).unwrap();
    publication.publish(Some(history(&mixer)));
    producer.request_pause(true);
    mixer.render(&mut [0.0; 3]).unwrap();
    publication.publish(Some(history(&mixer)));
    let prior = words(&publication);
    let ((), calls) = count_heap_calls(|| {
        mixer.render(&mut [0.0; 2]).unwrap();
        publication.publish(Some(history(&mixer)));
    });
    assert_eq!(calls, 0);
    assert_eq!(words(&publication), prior);
    assert_eq!(publication.test_header()[0].load(Ordering::SeqCst), 2);
    assert_eq!(publication.test_header()[4].load(Ordering::SeqCst), 9);
    producer.request_pause(false);
    mixer.render(&mut [0.0; 4]).unwrap();
    publication.publish(Some(history(&mixer)));
    assert_eq!(&words(&publication)[..2], &prior[..2]);
    for (frame, start, state) in [
        (3, 0, SourcePlaybackState::Running),
        (4, 4, SourcePlaybackState::Frozen),
        (8, 4, SourcePlaybackState::Frozen),
        (9, 9, SourcePlaybackState::Running),
        (13, 9, SourcePlaybackState::Running),
    ] {
        let selected = select(&publication, &mixer, at(frame)).unwrap();
        assert_eq!(
            (selected.span.physical_start(), selected.span.state()),
            (start, state)
        );
    }
    let (unchanged, calls) = count_heap_calls(|| publication.unchanged(history(&mixer)));
    assert!(unchanged);
    assert_eq!(calls, 0);
}

#[test]
fn cold_seed_after_owner_move_retains_real_initial_identity_and_range() {
    let (_, mut mixer) = rig(1000, 3, None, None);
    mixer.render(&mut [0.0; 4]).unwrap();
    let mut moved = mixer;
    let publication = SourcePlaybackPublication::new(history(&moved)).unwrap();
    assert_eq!(
        select(&publication, &moved, at(4))
            .unwrap()
            .pulled_end_frame,
        4
    );
    moved.render(&mut [0.0; 2]).unwrap();
    publication.publish(Some(history(&moved)));
    assert_eq!(
        select(&publication, &moved, at(6))
            .unwrap()
            .pulled_end_frame,
        6
    );
}

#[test]
fn selector_rejects_wrong_expected_identity_and_comparison_failure_without_heap() {
    let (_, mut mixer) = rig(1000, 3, None, None);
    mixer.render(&mut [0.0; 4]).unwrap();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    for (hz, physical, playback) in [(0, 0, 0), (999, 0, 0), (1000, 1, 0), (1000, 0, 1)] {
        let (result, calls) = count_heap_calls(|| {
            publication.select(hz, physical, playback, |frame| Some(2u64.cmp(&frame)))
        });
        assert!(result.is_none());
        assert_eq!(calls, 0);
    }
    assert!(publication.select(1000, 0, 0, |_| None).is_none());
    assert!(select(&publication, &mixer, at(2)).is_some());
}

#[test]
fn comparison_failure_at_actual_transition_or_late_selected_end_refuses() {
    let (_, mut mixer) = rig(1000, 3, Some(2), Some(3));
    mixer.render(&mut [0.0; 8]).unwrap();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    let (result, calls) = count_heap_calls(|| {
        publication.select(1000, 0, 0, |frame| {
            if frame == 2 {
                None
            } else {
                Some(3u64.cmp(&frame))
            }
        })
    });
    assert!(result.is_none());
    assert_eq!(calls, 0);
    let end_comparisons = std::cell::Cell::new(0);
    let (result, calls) = count_heap_calls(|| {
        publication.select(1000, 0, 0, |frame| {
            if frame == 5 {
                end_comparisons.set(end_comparisons.get() + 1);
                if end_comparisons.get() > 1 {
                    return None;
                }
            }
            Some(3u64.cmp(&frame))
        })
    });
    assert!(end_comparisons.get() > 1);
    assert!(result.is_none());
    assert_eq!(calls, 0);
    assert!(select(&publication, &mixer, at(3)).is_some());
}

#[test]
fn selector_refuses_malformed_counts_states_and_adjacent_continuity() {
    let (_, mut mixer) = rig(1000, 3, Some(2), Some(3));
    mixer.render(&mut [0.0; 8]).unwrap();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    let header = publication.test_header();
    let slots = publication.test_slots();
    for (field, bad) in [
        (&header[0], 0),
        (&header[0], u64::MAX),
        (&header[1], 0),
        (&header[1], 999),
        (&header[2], 1),
        (&header[3], 1),
        (&header[4], 4),
        (&slots[0][0], 1),
        (&slots[0][1], 1),
        (&slots[0][2], 2),
        (&slots[1][0], 0),
        (&slots[1][1], 1),
        (&slots[1][2], 2),
        (&slots[2][0], 2),
        (&slots[2][0], u64::MAX),
        (&slots[2][1], 2),
        (&slots[2][2], 2),
    ] {
        let old = field.load(Ordering::SeqCst);
        field.store(bad, Ordering::SeqCst);
        let (result, calls) = count_heap_calls(|| select(&publication, &mixer, at(3)));
        assert!(result.is_none());
        assert_eq!(calls, 0);
        field.store(old, Ordering::SeqCst);
        assert!(select(&publication, &mixer, at(3)).is_some());
    }
}

#[test]
fn missing_history_permanently_closes_payload_even_after_valid_republication() {
    let (_, mut mixer) = rig(1000, 3, None, None);
    mixer.render(&mut [0.0; 4]).unwrap();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    publication.publish(None);
    assert!(!publication.test_available().load(Ordering::SeqCst));
    publication.publish(Some(history(&mixer)));
    assert!(select(&publication, &mixer, at(2)).is_none());
}

#[test]
fn publish_rejects_changed_rate_capacity_and_cold_initial_identity_permanently() {
    let (_, mut source) = rig(1000, 3, None, None);
    source.render(&mut [0.0; 4]).unwrap();
    let (_, mut wrong_rate) = rig(999, 3, None, None);
    wrong_rate.render(&mut [0.0; 4]).unwrap();
    let (_, mut wrong_capacity) = rig(1000, 4, None, None);
    wrong_capacity.render(&mut [0.0; 4]).unwrap();
    // Re-enabling history on an already advanced bare owner records a new initial range.
    let format = AudioFormat::new(1000, 1).unwrap();
    let (_, consumer) = command_queue(8).unwrap();
    let mut advanced = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
        ),
        SampleBank::new(format, PcmLimits::new(1024, 1024, 1).unwrap()).unwrap(),
        consumer,
    )
    .unwrap();
    advanced.render(&mut [0.0; 2]).unwrap();
    advanced.enable_source_playback_history(3).unwrap();
    advanced.render(&mut [0.0; 2]).unwrap();
    for other in [&wrong_rate, &wrong_capacity, &advanced] {
        let publication = SourcePlaybackPublication::new(history(&source)).unwrap();
        let ((), calls) = count_heap_calls(|| publication.publish(Some(history(other))));
        assert_eq!(calls, 0);
        assert!(!publication.test_available().load(Ordering::SeqCst));
        publication.publish(Some(history(&source)));
        assert!(select(&publication, &source, at(2)).is_none());
    }
}

#[test]
fn publish_rejects_regressing_count_or_frontier_and_more_than_three_appended_transitions() {
    let (mut producer, mut mixer) = rig(1000, 8, None, None);
    let empty = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    for paused in [false, true, false, true] {
        producer.request_pause(paused);
        mixer.render(&mut [0.0; 2]).unwrap();
    }
    assert_eq!(history(&mixer).len(), 4);
    empty.publish(Some(history(&mixer)));
    assert!(!empty.test_available().load(Ordering::SeqCst));
    empty.publish(Some(history(&mixer)));
    assert!(select(&empty, &mixer, at(1)).is_none());
    for (index, value) in [(0, 5), (4, 9)] {
        let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
        publication.test_header()[index].store(value, Ordering::SeqCst);
        publication.publish(Some(history(&mixer)));
        assert!(!publication.test_available().load(Ordering::SeqCst));
        publication.test_header()[index].store(if index == 0 { 4 } else { 8 }, Ordering::SeqCst);
        publication.publish(Some(history(&mixer)));
        assert!(select(&publication, &mixer, at(1)).is_none());
    }
}

#[test]
fn cold_seed_with_nonzero_initial_range_refuses_before_range_without_coherence_claim() {
    let format = AudioFormat::new(1000, 1).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
        ),
        SampleBank::new(format, PcmLimits::new(1024, 1024, 1).unwrap()).unwrap(),
        consumer,
    )
    .unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 4]).unwrap();
    mixer.enable_source_playback_history(3).unwrap();
    mixer.render(&mut [0.0; 2]).unwrap();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    assert_eq!(
        (
            history(&mixer).initial_physical_frame(),
            publication.initial_playback_frame()
        ),
        (4, 0)
    );
    assert!(select(&publication, &mixer, at(3)).is_none());
    assert_eq!(
        select(&publication, &mixer, at(4)).unwrap().span.state(),
        SourcePlaybackState::Frozen
    );
    // This single-threaded mathematical selection has no native revision or admission proof.
    assert!(select(&publication, &mixer, at(6)).is_some());
}
