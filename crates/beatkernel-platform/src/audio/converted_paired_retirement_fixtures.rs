//! Paired retirement must prevalidate both histories before either prefix moves.
use super::*;
use crate::audio::{count_heap_calls, SampleEncoding};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

fn rig(
    source: u32,
    target: u32,
    source_capacity: Option<usize>,
    phase_capacity: Option<usize>,
) -> (CommandProducer, ConvertedNativeOutputState) {
    let format = AudioFormat::new(source, 1).unwrap();
    let limits = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            format,
            (0..1024).map(|i| 0.125 + i as f32 / 2048.0).collect(),
            limits,
        )
        .unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(17),
            Timestamp::ZERO,
            AudioLimits::new(16, 4, 16, 128, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    // Genuine original creation is nonzero and precedes converter construction.
    mixer.render(&mut [0.0]).unwrap();
    if let Some(capacity) = source_capacity {
        mixer.enable_source_playback_history(capacity).unwrap();
    }
    let device = DeviceFormat::new(target, 1, SampleEncoding::Float32, None).unwrap();
    let mut state = ConvertedNativeOutputState::new(
        mixer,
        device,
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|failure| panic!("real owner: {:?}", failure.error()));
    if let Some(capacity) = phase_capacity {
        state.enable_phase_history(capacity).unwrap();
    }
    (producer, state)
}

#[derive(Debug, PartialEq)]
struct Snapshot {
    pcm: Vec<f32>,
    pending_held: bool,
    format: DeviceFormat,
    max_frames: usize,
    source_meta: Option<(u32, u64, u64, u64, u64, usize)>,
    phase_meta: Option<(
        u32,
        TargetTime,
        SourcePosition,
        TargetTime,
        TargetTime,
        usize,
    )>,
    report: Option<ConvertedRenderReport>,
    admitted: usize,
    admitted_time: Option<TargetTime>,
    basis: TargetFrameBasis,
    boundaries: ConvertedBoundaryFacts,
    last_source: Option<RenderReport>,
    counters: AudioCounters,
    physical: u64,
    position: SourcePosition,
    time: TargetTime,
    phases: Vec<[u64; 8]>,
    phase_revision: Option<u64>,
    sources: Vec<(u64, Option<u64>, u64, SourcePlaybackState)>,
    source_revision: Option<u64>,
}
fn snapshot(state: &ConvertedNativeOutputState) -> Snapshot {
    let phase = state.phase_history();
    let source = state.mixer().source_playback_history();
    Snapshot {
        pcm: state.pending_samples().to_vec(),
        pending_held: state.pending_is_held(),
        format: state.format(),
        max_frames: state.max_frames(),
        source_meta: source.map(|h| {
            (
                h.source_rate(),
                h.initial_physical_frame(),
                h.initial_playback_frame(),
                h.pulled_end_frame(),
                h.retained_start_frame(),
                h.capacity(),
            )
        }),
        phase_meta: phase.map(|h| {
            (
                h.source_rate(),
                h.start_time(),
                h.initial_source_position(),
                h.end_time(),
                h.retained_start_time(),
                h.capacity(),
            )
        }),
        report: state.pending_report(),
        admitted: state.admitted_frames(),
        admitted_time: state.admitted_target_time(),
        basis: state.target_frame_basis(),
        boundaries: state.boundaries(),
        last_source: state.last_real_source_report(),
        counters: state.mixer().counters(),
        physical: state.mixer().frame_cursor(),
        position: state.converter_owner().source_position(),
        time: state.converter_owner().target_time(),
        phases: phase.map_or_else(Vec::new, |h| {
            (0..h.len())
                .map(|i| h.span(i).unwrap().to_words())
                .collect()
        }),
        phase_revision: phase.map(ConvertedPhaseHistory::retirement_revision),
        sources: source.map_or_else(Vec::new, |h| {
            (0..h.len())
                .map(|i| {
                    let s = h.span(i).unwrap();
                    (
                        s.physical_start(),
                        s.physical_end(),
                        s.playback_start(),
                        s.state(),
                    )
                })
                .collect()
        }),
        source_revision: source.map(SourcePlaybackHistory::retirement_revision),
    }
}
fn full(
    state: &mut ConvertedNativeOutputState,
    held: bool,
    frames: usize,
) -> ConvertedRenderReport {
    let report = if held {
        state.render_held_pending(frames)
    } else {
        state.render_pending(frames)
    }
    .unwrap();
    state.admit(frames).unwrap();
    report
}

fn cycle(producer: &mut CommandProducer, state: &mut ConvertedNativeOutputState, frames: usize) {
    producer.request_pause(false);
    full(state, false, frames);
    producer.request_pause(true);
    full(state, false, frames);
    full(state, true, frames);
    producer.request_pause(false);
    full(state, false, frames);
    assert_eq!(state.mixer().source_playback_history().unwrap().len(), 3);
    assert_eq!(state.phase_history().unwrap().len(), 3);
}
fn pins(state: &ConvertedNativeOutputState) -> (u64, TargetTime) {
    (
        state.converter_owner().source_position().frame,
        state.phase_history().unwrap().end_time(),
    )
}
fn just_trimmed(
    state: &ConvertedNativeOutputState,
    before: &Snapshot,
    source_removed: usize,
    phase_removed: usize,
) {
    let after = snapshot(state);
    let mut expected = Snapshot {
        pcm: before.pcm.clone(),
        pending_held: before.pending_held,
        format: before.format,
        max_frames: before.max_frames,
        source_meta: before.source_meta,
        phase_meta: before.phase_meta,
        report: before.report,
        admitted: before.admitted,
        admitted_time: before.admitted_time,
        basis: before.basis,
        boundaries: before.boundaries,
        last_source: before.last_source,
        counters: before.counters,
        physical: before.physical,
        position: before.position,
        time: before.time,
        phases: before.phases[phase_removed..].to_vec(),
        phase_revision: before
            .phase_revision
            .map(|r| r + u64::from(phase_removed != 0)),
        sources: before.sources[source_removed..].to_vec(),
        source_revision: before
            .source_revision
            .map(|r| r + u64::from(source_removed != 0)),
    };
    if source_removed != 0 {
        expected.source_meta.as_mut().unwrap().4 = expected.sources[0].0;
    }
    if phase_removed != 0 {
        let words = expected.phases[0];
        expected.phase_meta.as_mut().unwrap().4 =
            TargetTime::new(words[4], words[5], words[6]).unwrap();
    }
    assert_eq!(after, expected);
}

#[test]
fn genuine_source_pause_and_target_hold_retire_together_without_other_owner_changes() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (mut producer, mut state) = rig(source, target, Some(3), Some(3));
        cycle(&mut producer, &mut state, target as usize);
        let before = snapshot(&state);
        let (source_pin, target_pin) = pins(&state);
        assert_eq!(
            state.retire_histories_before(source_pin, target_pin),
            Ok((2, 2))
        );
        just_trimmed(&state, &before, 2, 2);
        let trimmed = snapshot(&state);
        assert_eq!(
            state.retire_histories_before(source_pin, target_pin),
            Ok((0, 0))
        );
        assert_eq!(snapshot(&state), trimmed);
    }
}

#[test]
fn mixed_positive_and_noop_pairs_preserve_tuple_order_and_independent_nonces() {
    for source_positive in [false, true] {
        let (mut producer, mut state) = rig(3, 7, Some(8), Some(8));
        cycle(&mut producer, &mut state, 7);
        let (source_end, target_end) = pins(&state);
        let source_cold = state
            .mixer()
            .source_playback_history()
            .unwrap()
            .retained_start_frame();
        let target_cold = state.phase_history().unwrap().retained_start_time();
        let before = snapshot(&state);
        let (source_pin, target_pin, expected) = if source_positive {
            (source_end, target_cold, (2, 0))
        } else {
            (source_cold, target_end, (0, 2))
        };
        assert_eq!(
            state.retire_histories_before(source_pin, target_pin),
            Ok(expected)
        );
        just_trimmed(&state, &before, expected.0, expected.1);
    }
}

#[test]
fn bad_target_after_valid_positive_source_cannot_retire_source_first() {
    let (mut producer, mut state) = rig(3, 7, Some(3), Some(3));
    cycle(&mut producer, &mut state, 7);
    let (source_pin, target_pin) = pins(&state);
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .validate_retirement_before(source_pin),
        Ok(())
    );
    assert_eq!(state.mixer().source_playback_history().unwrap().len(), 3);
    let before = snapshot(&state);
    let bad = target_pin.checked_add_frames(1, 7).unwrap();
    assert_eq!(
        state.retire_histories_before(source_pin, bad),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(
        state.retire_histories_before(source_pin, target_pin),
        Ok((2, 2))
    );
}

#[test]
fn bad_source_after_valid_positive_phase_cannot_retire_phase_first() {
    let (mut producer, mut state) = rig(7, 3, Some(3), Some(3));
    cycle(&mut producer, &mut state, 3);
    let (source_pin, target_pin) = pins(&state);
    assert_eq!(
        state
            .phase_history()
            .unwrap()
            .validate_retirement_before(target_pin),
        Ok(())
    );
    let before = snapshot(&state);
    assert_eq!(
        state.retire_histories_before(0, target_pin),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(
        state.retire_histories_before(source_pin, target_pin),
        Ok((2, 2))
    );
}

#[test]
fn missing_either_history_refuses_before_any_available_prefix_changes() {
    for (source_capacity, phase_capacity) in [(Some(8), None), (None, Some(8)), (None, None)] {
        let (mut producer, mut state) = rig(3, 7, source_capacity, phase_capacity);
        producer.request_pause(false);
        full(&mut state, false, 7);
        producer.request_pause(true);
        full(&mut state, false, 7);
        full(&mut state, true, 7);
        producer.request_pause(false);
        full(&mut state, false, 7);
        let before = snapshot(&state);
        assert_eq!(
            state.retire_histories_before(
                state.converter_owner().source_position().frame,
                state.converter_owner().target_time()
            ),
            Err(AudioError::InvalidCapacity)
        );
        assert_eq!(snapshot(&state), before);
    }
}

#[test]
fn cold_noop_and_retired_lower_bounds_validate_both_histories_readonly() {
    let (mut producer, mut state) = rig(3, 7, Some(3), Some(3));
    let cold = snapshot(&state);
    assert_eq!(state.retire_histories_before(1, cold.time), Ok((0, 0)));
    assert_eq!(
        state.retire_histories_before(0, cold.time),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(
        state.retire_histories_before(1, cold.time.checked_add_frames(1, 7).unwrap()),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), cold);
    cycle(&mut producer, &mut state, 7);
    let (source_pin, target_pin) = pins(&state);
    assert_eq!(
        state.retire_histories_before(source_pin, target_pin),
        Ok((2, 2))
    );
    let trimmed = snapshot(&state);
    assert_eq!(
        state.retire_histories_before(1, target_pin),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(
        state.retire_histories_before(source_pin, cold.time),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), trimmed);
}

#[test]
fn partial_active_pcm_protects_original_target_before_positive_source_commit() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (mut producer, mut state) = rig(source, target, Some(8), Some(8));
        cycle(&mut producer, &mut state, target as usize);
        full(&mut state, true, 2);
        let report = state.render_pending(4).unwrap();
        state.admit(2).unwrap();
        let source_pin = report.source_start_position.frame;
        let bad_target = state.target_frame_basis().start_time();
        let before = snapshot(&state);
        assert_eq!(
            state.retire_histories_before(source_pin, bad_target),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(snapshot(&state), before);
        assert_eq!(
            state.retire_histories_before(source_pin, report.target_start_time),
            Ok((2, 4))
        );
        just_trimmed(&state, &before, 2, 4);
    }
}

#[test]
fn held_partial_pcm_rejects_unsafe_source_and_target_pairs_without_mutation() {
    let (mut producer, mut state) = rig(3, 7, Some(8), Some(8));
    cycle(&mut producer, &mut state, 7);
    let held = state.render_held_pending(4).unwrap();
    state.admit(1).unwrap();
    let before = snapshot(&state);
    assert!(before.pending_held);
    assert_eq!(
        state.retire_histories_before(held.source_start_position.frame + 1, held.target_start_time),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(
        state.retire_histories_before(held.source_start_position.frame, held.target_end_time),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(
        state.retire_histories_before(held.source_start_position.frame, held.target_start_time),
        Ok((2, 3))
    );
    just_trimmed(&state, &before, 2, 3);
}

#[test]
fn exact_target_fraction_and_cached_source_floor_remain_pair_guards() {
    let (_, mut state) = rig(3, 7, Some(8), Some(8));
    full(&mut state, false, 1);
    let cached = state.render_pending(1).unwrap();
    assert_eq!(cached.source.unwrap().frames, 0);
    assert_ne!(cached.source_start_position.numerator, 0);
    let start = cached.target_start_time;
    let scale = 10_000_000_000_u64;
    let above = TargetTime::new(
        start.seconds(),
        start.numerator() * scale + 1,
        start.denominator() * scale,
    )
    .unwrap();
    let ns = |t: TargetTime| {
        i128::from(t.seconds()) * 1_000_000_000
            + i128::from(t.numerator()) * 1_000_000_000 / i128::from(t.denominator())
    };
    assert_eq!(ns(start), ns(above));
    let before = snapshot(&state);
    assert_eq!(
        state.retire_histories_before(cached.source_start_position.frame, above),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(
        state.retire_histories_before(cached.source_start_position.frame + 1, start),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(
        state.retire_histories_before(cached.source_start_position.frame, start),
        Ok((0, 0))
    );
    assert_eq!(snapshot(&state), before);
}

#[test]
fn full_admission_still_protects_real_converter_source_lookahead_before_phase_retirement() {
    let (mut producer, mut state) = rig(3, 7, Some(8), Some(8));
    full(&mut state, false, 1);
    full(&mut state, true, 1);
    full(&mut state, false, 1);
    producer.request_pause(true);
    let paused = (0..16)
        .find_map(|_| {
            let report = full(&mut state, false, 1);
            report
                .source
                .filter(|r| r.paused && r.frames != 0)
                .map(|_| report)
        })
        .expect("genuine source pause adopted");
    let source = state.mixer().source_playback_history().unwrap();
    let next_floor = state.converter_owner().source_position().frame;
    assert!(next_floor < source.span(1).unwrap().physical_start());
    let bad_source = source.pulled_end_frame();
    let target = state.phase_history().unwrap().end_time();
    let before = snapshot(&state);
    assert_eq!(
        state.retire_histories_before(bad_source, target),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(
        state.retire_histories_before(next_floor, target),
        Ok((0, 2))
    );
    let next = state.render_pending(1).unwrap();
    assert_eq!(next.source.unwrap().frames, 0);
    assert_eq!(next.source_start_position, paused.source_position);
    assert_eq!(next.target_start_time, paused.target_end_time);
}

#[test]
fn repeated_capacity_three_real_pairs_match_reference_pcm_reports_and_boundaries() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (mut input, mut bounded) = rig(source, target, Some(3), Some(3));
        let (mut reference_input, mut reference) = rig(source, target, Some(96), Some(96));
        let cold_source = bounded
            .mixer()
            .source_playback_history()
            .unwrap()
            .initial_physical_frame();
        let cold_target = bounded.phase_history().unwrap().start_time();
        for cycle in 0..20 {
            for (paused, held) in [(false, false), (true, false), (true, true), (false, false)] {
                input.request_pause(paused);
                reference_input.request_pause(paused);
                let actual = if held {
                    bounded.render_held_pending(target as usize)
                } else {
                    bounded.render_pending(target as usize)
                }
                .unwrap();
                let expected = if held {
                    reference.render_held_pending(target as usize)
                } else {
                    reference.render_pending(target as usize)
                }
                .unwrap();
                assert_eq!(actual, expected);
                assert_eq!(bounded.pending_samples(), reference.pending_samples());
                assert_eq!(bounded.boundaries(), reference.boundaries());
                assert_eq!(
                    bounded.last_real_source_report(),
                    reference.last_real_source_report()
                );
                bounded.admit(1).unwrap();
                reference.admit(1).unwrap();
                assert_eq!(bounded.pending_samples(), reference.pending_samples());
                bounded.admit(target as usize - 1).unwrap();
                reference.admit(target as usize - 1).unwrap();
            }
            let (source_pin, target_pin) = pins(&bounded);
            let (result, calls) =
                count_heap_calls(|| bounded.retire_histories_before(source_pin, target_pin));
            assert_eq!(result, Ok((2, 2)));
            assert_eq!(calls, 0);
            let sh = bounded.mixer().source_playback_history().unwrap();
            let ph = bounded.phase_history().unwrap();
            assert_eq!(
                (
                    sh.capacity(),
                    sh.len(),
                    sh.initial_physical_frame(),
                    sh.retirement_revision()
                ),
                (3, 1, cold_source, cycle + 1)
            );
            assert_eq!(
                (
                    ph.capacity(),
                    ph.len(),
                    ph.start_time(),
                    ph.retirement_revision()
                ),
                (3, 1, cold_target, cycle + 1)
            );
            assert_eq!(
                bounded.admitted_target_time(),
                reference.admitted_target_time()
            );
            assert_eq!(
                bounded.converter_owner().source_position(),
                reference.converter_owner().source_position()
            );
            assert_eq!(bounded.mixer().counters(), reference.mixer().counters());
        }
    }
}

#[test]
fn positive_noop_and_both_refusal_directions_allocate_nothing_after_setup() {
    let (mut producer, mut state) = rig(3, 7, Some(3), Some(3));
    cycle(&mut producer, &mut state, 7);
    let (source_pin, target_pin) = pins(&state);
    let bad_target = target_pin.checked_add_frames(1, 7).unwrap();
    let ((bad_target_result, bad_source_result, positive, noop), calls) = count_heap_calls(|| {
        (
            state.retire_histories_before(source_pin, bad_target),
            state.retire_histories_before(0, target_pin),
            state.retire_histories_before(source_pin, target_pin),
            state.retire_histories_before(source_pin, target_pin),
        )
    });
    assert_eq!(bad_target_result, Err(AudioError::InvalidFormat));
    assert_eq!(bad_source_result, Err(AudioError::InvalidFormat));
    assert_eq!(positive, Ok((2, 2)));
    assert_eq!(noop, Ok((0, 0)));
    assert_eq!(calls, 0);
}
