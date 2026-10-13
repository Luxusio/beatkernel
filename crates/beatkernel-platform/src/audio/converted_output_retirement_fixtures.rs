//! Retirement through the complete owner, including positively unsubmitted PCM.
use super::*;
use crate::audio::{count_heap_calls, SampleEncoding};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

fn rig(
    source: u32,
    target: u32,
    capacity: Option<usize>,
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
    if let Some(capacity) = capacity {
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
    if let Some(capacity) = capacity {
        state.enable_phase_history(capacity).unwrap();
    }
    (producer, state)
}

#[derive(Debug, PartialEq)]
struct Snapshot {
    pcm: Vec<f32>,
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

#[test]
fn cold_noop_disabled_and_out_of_range_are_readonly() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut state) = rig(source, target, Some(3));
        let cold = snapshot(&state);
        assert_eq!(state.retire_phase_history_before(cold.time), Ok(0));
        assert_eq!(state.retire_source_playback_before(1), Ok(0));
        assert_eq!(
            state.retire_source_playback_before(0),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(
            state.retire_source_playback_before(2),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(
            state.retire_phase_history_before(cold.time.checked_add_frames(1, target).unwrap()),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(snapshot(&state), cold);
        let (_, mut disabled) = rig(source, target, None);
        disabled.render_pending(2).unwrap();
        let before = snapshot(&disabled);
        assert_eq!(
            disabled.retire_phase_history_before(before.time),
            Err(AudioError::InvalidCapacity)
        );
        assert_eq!(
            disabled.retire_source_playback_before(u64::MAX),
            Err(AudioError::InvalidCapacity)
        );
        assert_eq!(snapshot(&disabled), before);
    }
}

#[test]
fn active_original_block_start_remains_protected_through_partial_admission() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut state) = rig(source, target, Some(8));
        full(&mut state, false, 2);
        full(&mut state, true, 2);
        let report = state.render_pending(4).unwrap();
        state.admit(2).unwrap();
        let before = snapshot(&state);
        assert!(InterpolatedTargetTime::new(
            state.target_frame_basis().start_time(),
            0,
            0,
            1,
            0,
            1
        )
        .unwrap()
        .compare_target_time(report.target_start_time)
        .is_gt());
        // The first unsent basis is insufficient: original report interpretation remains live.
        assert_eq!(
            state.retire_phase_history_before(state.target_frame_basis().start_time()),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(
            state.retire_source_playback_before(report.source_start_position.frame + 1),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(snapshot(&state), before);
        assert_eq!(
            state.retire_phase_history_before(report.target_start_time),
            Ok(2)
        );
        let mut expected = before;
        expected.phases = expected.phases[2..].to_vec();
        expected.phase_revision = Some(1);
        assert_eq!(snapshot(&state), expected);
        assert_eq!(
            state.retire_phase_history_before(report.target_start_time),
            Ok(0)
        );
        state.admit(2).unwrap();
        assert_eq!(
            state.retire_phase_history_before(report.target_end_time),
            Ok(0)
        );
    }
}

#[test]
fn exact_fraction_beyond_pending_start_refuses_even_in_same_nanosecond() {
    let (_, mut state) = rig(3, 7, Some(8));
    full(&mut state, false, 2);
    let report = state.render_held_pending(2).unwrap();
    let start = report.target_start_time;
    let scale = 10_000_000_000_u64;
    let above = TargetTime::new(
        start.seconds(),
        start.numerator() * scale + 1,
        start.denominator() * scale,
    )
    .unwrap();
    let floor = |t: TargetTime| {
        i128::from(t.seconds()) * 1_000_000_000
            + i128::from(t.numerator()) * 1_000_000_000 / i128::from(t.denominator())
    };
    assert_eq!(floor(start), floor(above));
    let before = snapshot(&state);
    assert_eq!(
        state.retire_phase_history_before(above),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(state.retire_phase_history_before(start), Ok(1));
}

#[test]
fn held_pending_pins_and_pcm_are_preserved_until_full_admission() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut state) = rig(source, target, Some(8));
        full(&mut state, false, 2);
        let held = state.render_held_pending(4).unwrap();
        assert_eq!(held.source, None);
        state.admit(1).unwrap();
        assert!(state.pending_is_held());
        assert_eq!(state.pending_samples(), [0.0; 3]);
        let before = snapshot(&state);
        assert_eq!(
            state.retire_phase_history_before(held.target_end_time),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(
            state.retire_source_playback_before(held.source_start_position.frame + 1),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(snapshot(&state), before);
        assert_eq!(
            state.retire_phase_history_before(held.target_start_time),
            Ok(1)
        );
        state.admit(3).unwrap();
        assert!(!state.pending_is_held());
        assert_eq!(
            state.retire_phase_history_before(held.target_end_time),
            Ok(0)
        );
        let next = state.render_pending(2).unwrap();
        assert_eq!(next.source_start_position, held.source_position);
        assert_eq!(next.target_start_time, held.target_end_time);
    }
}

#[test]
fn cached_active_block_keeps_original_source_floor_and_target_pin() {
    let (_, mut state) = rig(3, 7, Some(8));
    full(&mut state, false, 1);
    let cached = state.render_pending(1).unwrap();
    assert_eq!(cached.state, ConvertedOutputState::Active);
    assert_eq!(cached.source.unwrap().frames, 0);
    assert_ne!(cached.source_start_position.numerator, 0);
    let before = snapshot(&state);
    assert_eq!(
        state.retire_source_playback_before(cached.source_start_position.frame + 1),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(
        state.retire_phase_history_before(cached.target_end_time),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(
        state.retire_source_playback_before(cached.source_start_position.frame),
        Ok(0)
    );
    assert_eq!(
        state.retire_phase_history_before(cached.target_start_time),
        Ok(0)
    );
    assert_eq!(snapshot(&state), before);
}

#[test]
fn source_pending_protection_releases_only_after_complete_native_admission() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (mut producer, mut state) = rig(source, target, Some(8));
        full(&mut state, false, target as usize);
        producer.request_pause(true);
        full(&mut state, false, target as usize);
        producer.request_pause(false);
        let resumed = state.render_pending(target as usize).unwrap();
        assert_eq!(state.mixer().source_playback_history().unwrap().len(), 3);
        state.admit(1).unwrap();
        let before = snapshot(&state);
        let end = state
            .mixer()
            .source_playback_history()
            .unwrap()
            .pulled_end_frame();
        assert!(end > resumed.source_start_position.frame);
        assert_eq!(
            state.retire_source_playback_before(end),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(snapshot(&state), before);
        state.admit(target as usize - 1).unwrap();
        let before = snapshot(&state);
        let qualified_pin = state.converter_owner().source_position().frame;
        assert_eq!(state.retire_source_playback_before(qualified_pin), Ok(2));
        let mut expected = before;
        expected.sources = expected.sources[2..].to_vec();
        expected.source_revision = Some(1);
        assert_eq!(snapshot(&state), expected);
        let next = state.render_pending(1).unwrap();
        assert_eq!(next.source_start_position, resumed.source_position);
        assert_eq!(next.target_start_time, resumed.target_end_time);
    }
}

#[test]
fn positive_noop_and_refused_owner_retirement_do_not_allocate() {
    let (_, mut state) = rig(3, 7, Some(3));
    full(&mut state, false, 2);
    full(&mut state, true, 2);
    let active = state.render_pending(2).unwrap();
    let ((positive, noop, refused), calls) = count_heap_calls(|| {
        (
            state.retire_phase_history_before(active.target_start_time),
            state.retire_phase_history_before(active.target_start_time),
            state.retire_phase_history_before(active.target_end_time),
        )
    });
    assert_eq!(
        (positive, noop, refused),
        (Ok(2), Ok(0), Err(AudioError::InvalidFormat))
    );
    assert_eq!(calls, 0);
    state.admit(2).unwrap();
    let (mut producer, mut source_state) = rig(3, 7, Some(3));
    full(&mut source_state, false, 7);
    producer.request_pause(true);
    full(&mut source_state, false, 7);
    producer.request_pause(false);
    full(&mut source_state, false, 7);
    let end = source_state.converter_owner().source_position().frame;
    let ((positive, noop, refused), calls) = count_heap_calls(|| {
        (
            source_state.retire_source_playback_before(end),
            source_state.retire_source_playback_before(end),
            source_state.retire_source_playback_before(end + 1),
        )
    });
    assert_eq!(
        (positive, noop, refused),
        (Ok(2), Ok(0), Err(AudioError::InvalidFormat))
    );
    assert_eq!(calls, 0);
}

#[test]
fn repeated_capacity_three_phase_retirement_matches_unretired_complete_owner() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut bounded) = rig(source, target, Some(3));
        let (_, mut reference) = rig(source, target, Some(80));
        let cold = bounded.phase_history().unwrap().start_time();
        for cycle in 0..20 {
            for held in [false, true, false] {
                let actual = if held {
                    bounded.render_held_pending(2)
                } else {
                    bounded.render_pending(2)
                }
                .unwrap();
                let expected = if held {
                    reference.render_held_pending(2)
                } else {
                    reference.render_pending(2)
                }
                .unwrap();
                assert_eq!(actual, expected);
                assert_eq!(bounded.pending_samples(), reference.pending_samples());
                assert_eq!(bounded.boundaries(), reference.boundaries());
                bounded.admit(1).unwrap();
                reference.admit(1).unwrap();
                assert_eq!(bounded.pending_samples(), reference.pending_samples());
                bounded.admit(1).unwrap();
                reference.admit(1).unwrap();
            }
            let h = bounded.phase_history().unwrap();
            assert_eq!(h.len(), 3);
            let end = h.end_time();
            assert_eq!(bounded.retire_phase_history_before(end), Ok(2));
            let h = bounded.phase_history().unwrap();
            assert_eq!(h.capacity(), 3);
            assert_eq!(h.len(), 1);
            assert_eq!(h.start_time(), cold);
            assert_eq!(h.retirement_revision(), cycle + 1);
            assert_eq!(
                bounded.admitted_target_time(),
                reference.admitted_target_time()
            );
            assert_eq!(bounded.mixer().counters(), reference.mixer().counters());
        }
    }
}

#[test]
fn repeated_real_pause_resume_source_retirement_matches_reference_pcm_and_reports() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (mut input, mut bounded) = rig(source, target, Some(3));
        let (mut reference_input, mut reference) = rig(source, target, Some(80));
        let cold = bounded
            .mixer()
            .source_playback_history()
            .unwrap()
            .initial_physical_frame();
        for cycle in 0..16 {
            for paused in [false, true, false] {
                input.request_pause(paused);
                reference_input.request_pause(paused);
                let actual = bounded.render_pending(target as usize).unwrap();
                let expected = reference.render_pending(target as usize).unwrap();
                assert_eq!(actual, expected);
                assert_eq!(bounded.pending_samples(), reference.pending_samples());
                assert_eq!(bounded.boundaries(), reference.boundaries());
                assert_eq!(
                    bounded.last_real_source_report(),
                    reference.last_real_source_report()
                );
                bounded.admit(target as usize).unwrap();
                reference.admit(target as usize).unwrap();
            }
            let h = bounded.mixer().source_playback_history().unwrap();
            assert_eq!(h.len(), 3);
            let end = bounded.converter_owner().source_position().frame;
            assert_eq!(bounded.retire_source_playback_before(end), Ok(2));
            let h = bounded.mixer().source_playback_history().unwrap();
            assert_eq!(h.capacity(), 3);
            assert_eq!(h.len(), 1);
            assert_eq!(h.initial_physical_frame(), cold);
            assert_eq!(h.retirement_revision(), cycle + 1);
            assert_eq!(
                bounded.converter_owner().source_position(),
                reference.converter_owner().source_position()
            );
            assert_eq!(
                bounded.admitted_target_time(),
                reference.admitted_target_time()
            );
            assert_eq!(bounded.mixer().counters(), reference.mixer().counters());
        }
    }
}

#[test]
fn full_admission_keeps_converter_lookahead_source_pin() {
    let (mut producer, mut state) = rig(3, 7, Some(8));
    full(&mut state, false, 1);
    producer.request_pause(true);
    let mut paused_pull = None;
    for _ in 0..16 {
        let report = full(&mut state, false, 1);
        if report
            .source
            .is_some_and(|source| source.paused && source.frames != 0)
        {
            paused_pull = Some(report);
            break;
        }
    }
    let paused = paused_pull.expect("real nonempty source pause adoption");
    assert_eq!(state.pending_report(), None);
    let h = state.mixer().source_playback_history().unwrap();
    assert_eq!(h.len(), 2);
    assert_eq!(h.span(0).unwrap().state(), SourcePlaybackState::Running);
    assert_eq!(h.span(1).unwrap().state(), SourcePlaybackState::Frozen);
    let next_floor = state.converter_owner().source_position().frame;
    assert!(next_floor < h.span(1).unwrap().physical_start());
    let pulled = h.pulled_end_frame();
    assert!(pulled > next_floor);
    let before = snapshot(&state);
    assert_eq!(
        state.retire_source_playback_before(pulled),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(state.retire_source_playback_before(next_floor), Ok(0));
    assert_eq!(snapshot(&state), before);
    let next = state.render_pending(1).unwrap();
    assert_eq!(next.source.unwrap().frames, 0);
    assert_eq!(next.source_start_position, paused.source_position);
    assert_eq!(next.target_start_time, paused.target_end_time);
    let h = state.mixer().source_playback_history().unwrap();
    assert_eq!(h.span(0).unwrap().state(), SourcePlaybackState::Running);
    let phase = state.phase_history().unwrap();
    let query = InterpolatedTargetTime::new(next.target_start_time, 0, 0, 1, 0, 1).unwrap();
    assert_eq!(
        h.lookup_at_interpolated_time(phase.span(0).unwrap(), query, phase.end_time())
            .unwrap()
            .state(),
        SourcePlaybackState::Running
    );
}
