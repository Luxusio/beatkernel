//! Actual queued practice receipts protect the complete retained output owner.
use super::*;
use crate::audio::{count_heap_calls, SampleEncoding};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

fn rig(
    practice: bool,
    source_history: bool,
    phase_history: bool,
) -> (ConvertedNativeOutputState, Option<PracticeController>) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(64, 64, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.1, 0.2, 0.3, 0.4], limits).unwrap(),
    )
    .unwrap();
    let prepared = practice.then(|| {
        let program = PreparedPracticeProgram::new(
            &bank,
            vec![PracticeCue {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            }],
            PracticeLimits::new(8, 1, 128, 8, 16).unwrap(),
        )
        .unwrap();
        let (controller, endpoint) = practice_queue(&program).unwrap();
        (program, controller, endpoint)
    });
    let (_, consumer) = command_queue(16).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(16, 4, 16, 128, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let controller = prepared.map(|(program, controller, endpoint)| {
        mixer
            .install_practice(
                program,
                endpoint,
                PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(1_000_000), true)
                    .unwrap(),
            )
            .unwrap();
        controller
    });
    if source_history {
        mixer.enable_source_playback_history(16).unwrap();
    }
    let mut state = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(1500, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        32,
    )
    .unwrap_or_else(|failure| panic!("real owner: {:?}", failure.error()));
    if phase_history {
        state.enable_phase_history(16).unwrap();
    }
    (state, controller)
}
fn time(frame: u64) -> TargetTime {
    TargetTime::from_frames(frame, 1500).unwrap()
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

#[derive(Debug, PartialEq)]
struct Snapshot {
    pcm: Vec<f32>,
    report: Option<ConvertedRenderReport>,
    admitted: usize,
    admitted_time: Option<TargetTime>,
    position: SourcePosition,
    target_time: TargetTime,
    physical: u64,
    playback: u64,
    boundaries: ConvertedBoundaryFacts,
    last_source: Option<RenderReport>,
    counters: AudioCounters,
    basis: TargetFrameBasis,
    pins: (Option<u64>, Option<TargetTime>),
    source: Option<(u64, u64, Vec<(u64, Option<u64>, u64, SourcePlaybackState)>)>,
    phase: Option<(u64, TargetTime, Vec<[u64; 8]>)>,
}
fn snapshot(state: &ConvertedNativeOutputState) -> Snapshot {
    Snapshot {
        pcm: state.pending_samples().to_vec(),
        report: state.pending_report(),
        admitted: state.admitted_frames(),
        admitted_time: state.admitted_target_time(),
        position: state.converter_owner().source_position(),
        target_time: state.converter_owner().target_time(),
        physical: state.mixer().frame_cursor(),
        playback: state.mixer().playback_frame_cursor(),
        boundaries: state.boundaries(),
        last_source: state.last_real_source_report(),
        counters: state.mixer().counters(),
        basis: state.target_frame_basis(),
        pins: state.practice_history_retention_pins(),
        source: state.mixer().source_playback_history().map(|h| {
            (
                h.retirement_revision(),
                h.retained_start_frame(),
                (0..h.len())
                    .map(|i| {
                        let span = h.span(i).unwrap();
                        (
                            span.physical_start(),
                            span.physical_end(),
                            span.playback_start(),
                            span.state(),
                        )
                    })
                    .collect(),
            )
        }),
        phase: state.phase_history().map(|h| {
            (
                h.retirement_revision(),
                h.retained_start_time(),
                (0..h.len())
                    .map(|i| h.span(i).unwrap().to_words())
                    .collect(),
            )
        }),
    }
}

#[test]
fn original_projected_source_and_target_minima_refuse_unsafe_pair_without_owner_changes() {
    let (mut state, _controller) = rig(true, true, true);
    full(&mut state, false, 6);
    assert_eq!(
        state.practice_history_retention_pins(),
        (Some(0), Some(time(0)))
    );
    assert!(state.converter_owner().source_position().frame > 1);
    let before = snapshot(&state);
    assert_eq!(
        state.retire_source_playback_before(1),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(
        state.retire_phase_history_before(time(1)),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(
        state.retire_histories_before(1, time(0)),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(
        state.retire_histories_before(0, time(1)),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(state.retire_histories_before(0, time(0)), Ok((0, 0)));
    assert_eq!(snapshot(&state), before);
}

#[test]
fn consumer_reclamation_releases_projected_minimum_but_preserves_raw_lookahead_source() {
    let (mut state, controller) = rig(true, true, true);
    let mut controller = controller.unwrap();
    full(&mut state, false, 3);
    let before = snapshot(&state);
    for _ in 0..64 {
        assert_eq!(
            state.practice_history_retention_pins(),
            (Some(0), Some(time(0)))
        );
    }
    assert_eq!(snapshot(&state), before);
    let first = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (first.receipt.physical_frame, first.boundary.target_time),
        (0, time(0))
    );
    assert_eq!(
        state.practice_history_retention_pins(),
        (Some(1), Some(time(2)))
    );
    let second = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (second.receipt.physical_frame, second.boundary.target_time),
        (1, time(2))
    );
    assert_eq!(state.practice_history_retention_pins(), (Some(2), None));
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    assert_eq!(state.retire_histories_before(2, time(3)), Ok((0, 0)));
}

#[test]
fn raw_receipt_does_not_invent_target_pin_and_actual_held_prefix_can_be_retired() {
    let (mut bounded, controller) = rig(true, true, true);
    let mut controller = controller.unwrap();
    let (mut reference, reference_controller) = rig(true, true, true);
    let mut reference_controller = reference_controller.unwrap();
    assert_eq!(full(&mut bounded, false, 3), full(&mut reference, false, 3));
    for _ in 0..2 {
        assert_eq!(
            controller.try_pop_projected_receipt().unwrap(),
            reference_controller.try_pop_projected_receipt().unwrap()
        );
    }
    assert_eq!(bounded.practice_history_retention_pins(), (Some(2), None));
    assert_eq!(full(&mut bounded, true, 3), full(&mut reference, true, 3));
    assert_eq!(bounded.practice_history_retention_pins(), (Some(2), None));
    assert_eq!(bounded.retire_histories_before(2, time(6)), Ok((0, 1)));
    let actual = bounded.render_pending(3).unwrap();
    let expected = reference.render_pending(3).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(bounded.pending_samples(), reference.pending_samples());
    assert_eq!(bounded.boundaries(), reference.boundaries());
    assert_eq!(
        bounded.practice_history_retention_pins(),
        (Some(2), Some(time(6)))
    );
    bounded.admit(3).unwrap();
    reference.admit(3).unwrap();
    assert_eq!(
        bounded.admitted_target_time(),
        reference.admitted_target_time()
    );
    assert_eq!(
        bounded.converter_owner().source_position(),
        reference.converter_owner().source_position()
    );
    assert_eq!(
        controller.try_pop_projected_receipt().unwrap(),
        reference_controller.try_pop_projected_receipt().unwrap()
    );
}

#[test]
fn reclaimed_practice_does_not_release_original_partially_admitted_pcm_guards() {
    let (mut state, controller) = rig(true, true, true);
    let mut controller = controller.unwrap();
    let report = state.render_pending(3).unwrap();
    state.admit(1).unwrap();
    controller.try_pop_projected_receipt().unwrap();
    controller.try_pop_projected_receipt().unwrap();
    assert_eq!(state.practice_history_retention_pins(), (Some(2), None));
    let before = snapshot(&state);
    assert_eq!(
        state.retire_histories_before(
            report.source_start_position.frame + 1,
            report.target_start_time
        ),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(
        state.retire_histories_before(report.source_start_position.frame, time(1)),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&state), before);
    state.admit(2).unwrap();
    assert_eq!(state.retire_histories_before(2, time(3)), Ok((0, 0)));
}

#[test]
fn disabled_history_priority_and_no_practice_behavior_are_preserved() {
    let (mut no_practice, _controller) = rig(false, true, true);
    full(&mut no_practice, false, 3);
    assert_eq!(no_practice.practice_history_retention_pins(), (None, None));
    assert_eq!(no_practice.retire_histories_before(2, time(3)), Ok((0, 0)));
    for (source, phase, source_pin) in [(false, true, 1), (true, false, 0)] {
        let (mut state, _controller) = rig(true, source, phase);
        full(&mut state, false, 3);
        let before = snapshot(&state);
        assert_eq!(
            state.retire_histories_before(source_pin, time(1)),
            Err(AudioError::InvalidCapacity)
        );
        assert_eq!(snapshot(&state), before);
    }
}

#[test]
fn queued_owner_minimum_validation_and_noop_are_heap_free() {
    let (_, calibration) = count_heap_calls(|| {
        let mut values = Vec::<u8>::with_capacity(1);
        values.push(1);
        values.reserve_exact(128);
        std::hint::black_box(&values);
        drop(values);
    });
    assert!(calibration > 0);
    let (mut state, _controller) = rig(true, true, true);
    full(&mut state, false, 3);
    let before = snapshot(&state);
    let ((pins, bad_source, bad_target, safe), calls) = count_heap_calls(|| {
        let mut pins = (None, None);
        for _ in 0..128 {
            pins = std::hint::black_box(state.practice_history_retention_pins());
        }
        (
            pins,
            state.retire_histories_before(1, time(0)),
            state.retire_histories_before(0, time(1)),
            state.retire_histories_before(0, time(0)),
        )
    });
    assert_eq!(calls, 0);
    assert_eq!(pins, (Some(0), Some(time(0))));
    assert_eq!(bad_source, Err(AudioError::InvalidFormat));
    assert_eq!(bad_target, Err(AudioError::InvalidFormat));
    assert_eq!(safe, Ok((0, 0)));
    assert_eq!(snapshot(&state), before);
}
