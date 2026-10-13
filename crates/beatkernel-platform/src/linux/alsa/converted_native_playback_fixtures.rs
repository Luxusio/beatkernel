//! Actual converted PCM worker publications; scripted native calls prove no acoustic timing.
use super::*;
use super::super::converted_fixtures::rig_boundaries;
use beatkernel::audio::*;
use std::cell::Cell;

fn at(frame: u64, hz: u32) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, hz).unwrap(), 0, 0, 1, 0, 1).unwrap()
}
fn beyond(frame: u64, hz: u32) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(
        TargetTime::from_frames(frame, hz).unwrap(),
        0,
        1,
        u64::MAX,
        1,
        u64::MAX - 1,
    )
    .unwrap()
}
fn stamp(submitted: u64) -> AlsaTimingSnapshot {
    timing::interpret(
        3,
        submitted,
        1,
        9,
        AlsaNativeTimestamp {
            seconds: 1,
            nanoseconds: 123,
        },
        ClockPoint {
            domain: ClockDomainId(8),
            timestamp: Timestamp::from_nanos(2_000_000_000),
        },
        ClockPoint {
            domain: ClockDomainId(8),
            timestamp: Timestamp::from_nanos(2_000_000_010),
        },
        1,
        1,
    )
}
fn read(
    shared: &Shared,
    telemetry: &ConvertedTelemetry,
    basis: TargetFrameBasis,
    q: &InterpolatedTargetTime,
) -> Option<AlsaConvertedPlaybackSnapshot> {
    shared_converted_playback_with(
        shared,
        telemetry,
        basis,
        ClockDomainId(8),
        q,
        1000,
        0,
        0,
        |_| {},
    )
}
fn rig() -> (
    CommandProducer,
    ConvertedNativeOutputState,
    AlsaAppliedConfig,
    ConvertedTelemetry,
    Shared,
) {
    let (producer, mut state, config) = rig_boundaries(8, 1000, 1000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(8).unwrap();
    let telemetry =
        ConvertedTelemetry::with_phase_history(&state, state.target_frame_basis()).unwrap();
    let shared = Shared::with_converted_history(&telemetry);
    shared.status.store(1, Ordering::SeqCst);
    (producer, state, config, telemetry, shared)
}
fn publish(
    state: &mut ConvertedNativeOutputState,
    telemetry: &ConvertedTelemetry,
    shared: &Shared,
    frames: usize,
    admitted: usize,
) -> ConvertedRenderReport {
    let update = shared.timing.update();
    let report = state.render_pending(frames).unwrap();
    telemetry.publish_generated(Some(report), state);
    state.admit(admitted).unwrap();
    telemetry.publish_submitted(state.admitted_target_time().unwrap());
    shared
        .submitted
        .fetch_add(admitted as u64, Ordering::SeqCst);
    drop(update);
    report
}

#[derive(Clone, Copy)]
enum Write {
    Count(usize),
    Again,
    Fail,
    Excess,
}
struct Pcm<'a> {
    shared: &'a Shared,
    telemetry: &'a ConvertedTelemetry,
    basis: TargetFrameBasis,
    steps: &'a [Write],
    writes: usize,
    waits: usize,
    timing_calls: usize,
    timing_fail: bool,
    timing_some: bool,
    wait_snapshot: Option<AlsaConvertedPlaybackSnapshot>,
    output: [f32; 64],
    length: usize,
}
impl<'a> Pcm<'a> {
    fn new(
        shared: &'a Shared,
        telemetry: &'a ConvertedTelemetry,
        basis: TargetFrameBasis,
        steps: &'a [Write],
    ) -> Self {
        Self {
            shared,
            telemetry,
            basis,
            steps,
            writes: 0,
            waits: 0,
            timing_calls: 0,
            timing_fail: false,
            timing_some: true,
            wait_snapshot: None,
            output: [0.; 64],
            length: 0,
        }
    }
}
impl PcmOperations for Pcm<'_> {
    fn write(&mut self, bytes: &[u8], frames: usize) -> Result<Option<usize>, LinuxError> {
        assert_eq!(
            self.shared.timing.sequence() & 1,
            1,
            "actual converted write enclosed by outer publication"
        );
        assert!(read(self.shared, self.telemetry, self.basis, &at(0, 1000)).is_none());
        let step = self.steps.get(self.writes).copied().unwrap_or(Write::Fail);
        self.writes += 1;
        match step {
            Write::Count(n) => {
                assert!(n <= frames);
                for word in bytes[..n * 4].chunks_exact(4) {
                    self.output[self.length] = f32::from_le_bytes(word.try_into().unwrap());
                    self.length += 1;
                }
                Ok(Some(n))
            }
            Write::Again => Ok(None),
            Write::Fail => Err(LinuxError::InvalidConfiguration("script PCM failure")),
            Write::Excess => Ok(Some(frames + 1)),
        }
    }
    fn wait(&mut self) -> Result<bool, LinuxError> {
        self.waits += 1;
        assert_eq!(
            self.shared.timing.sequence() & 1,
            0,
            "release publication before native wait"
        );
        self.wait_snapshot = read(self.shared, self.telemetry, self.basis, &at(2, 1000));
        if let Some(snapshot) = self.wait_snapshot {
            assert_eq!(snapshot.submitted_target_frames, 2);
            assert!(
                snapshot.timing.is_none(),
                "waiting does not reuse the previous native query"
            );
        }
        self.shared.stop.store(true, Ordering::SeqCst);
        Ok(false)
    }
    fn timing<C: WorkerClock>(
        &mut self,
        _clock: &C,
        submitted: u64,
    ) -> Result<Option<AlsaTimingSnapshot>, LinuxError> {
        self.timing_calls += 1;
        if self.timing_fail {
            Err(LinuxError::InvalidConfiguration(
                "script native query failure",
            ))
        } else {
            Ok(self.timing_some.then(|| stamp(submitted)))
        }
    }
    fn drop_stream(&mut self) -> Result<(), LinuxError> {
        Ok(())
    }
}
struct Clock {
    calls: Cell<usize>,
    fail_at: usize,
}
impl WorkerClock for Clock {
    fn now(&self) -> Result<ClockPoint, LinuxError> {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        if call == self.fail_at {
            return Err(LinuxError::InvalidConfiguration("script clock failure"));
        }
        Ok(ClockPoint {
            domain: ClockDomainId(8),
            timestamp: Timestamp::from_nanos(call as i64),
        })
    }
}
struct Encoder {
    fail: bool,
}
impl PcmEncoder for Encoder {
    fn encode(
        &mut self,
        format: DeviceFormat,
        samples: &[f32],
        bytes: &mut [u8],
    ) -> Result<(), LinuxError> {
        if self.fail {
            Err(LinuxError::InvalidConfiguration("script encoder failure"))
        } else {
            encode_pcm(format, samples, bytes).map_err(LinuxError::Conversion)
        }
    }
}
fn pump(
    pcm: &mut Pcm<'_>,
    state: &mut ConvertedNativeOutputState,
    config: &AlsaAppliedConfig,
    fail_at: usize,
    encode_fail: bool,
) -> Result<(), LinuxError> {
    let mut bytes = [0; 128];
    let shared = pcm.shared;
    let telemetry = pcm.telemetry;
    run_converted_worker(
        pcm,
        state,
        config,
        &mut bytes[..config.period_frames as usize * 4],
        shared,
        &Clock {
            calls: Cell::new(0),
            fail_at,
        },
        &mut Encoder { fail: encode_fail },
        telemetry,
    )
}

#[test]
fn actual_positive_partial_write_joins_native_query_generated_source_and_boundaries() {
    let (_producer, mut state, config, telemetry, shared) = rig();
    let basis = state.target_frame_basis();
    let mut pcm = Pcm::new(&shared, &telemetry, basis, &[Write::Count(2)]);
    assert!(pump(&mut pcm, &mut state, &config, 2, false).is_err());
    let snapshot = read(&shared, &telemetry, basis, &at(2, 1000)).unwrap();
    assert_eq!(snapshot.timing, Some(stamp(2)));
    assert_eq!(snapshot.submitted_target_frames, 2);
    assert_eq!(snapshot.revision, shared.timing.sequence());
    assert_eq!(snapshot.revision & 1, 0);
    assert_eq!(
        snapshot.phase.phase.submitted_end,
        TargetTime::from_frames(2, 1000).unwrap()
    );
    assert_eq!(
        snapshot.phase.phase.generated_end,
        TargetTime::from_frames(8, 1000).unwrap()
    );
    assert_eq!(snapshot.phase.phase.report.unwrap().target_frames, 8);
    assert_eq!(
        snapshot.phase.pulled_source_end,
        snapshot
            .phase
            .phase
            .report
            .unwrap()
            .pulled_source_frame_cursor
    );
    assert_eq!(
        snapshot.phase.phase.last_real_source,
        state.last_real_source_report()
    );
    assert_eq!(snapshot.phase.phase.boundaries, state.boundaries());
    assert_eq!(state.pending_frames(), 6);
    assert_eq!(pcm.length, 2);
    assert_eq!(&pcm.output[..2], &[0.125, 0.125 + 1. / 512.]);
}

#[test]
fn actual_unequal_rate_native_packet_counts_target_admission_and_source_pull_separately() {
    let (_, mut state, config) = rig_boundaries(8, 1000, 2000, None, None);
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(8).unwrap();
    let basis = state.target_frame_basis();
    let telemetry = ConvertedTelemetry::with_phase_history(&state, basis).unwrap();
    let shared = Shared::with_converted_history(&telemetry);
    shared.status.store(1, Ordering::SeqCst);
    let mut pcm = Pcm::new(&shared, &telemetry, basis, &[Write::Count(2)]);
    assert!(pump(&mut pcm, &mut state, &config, 2, false).is_err());
    let snapshot = shared_converted_playback_with(
        &shared,
        &telemetry,
        basis,
        ClockDomainId(8),
        &at(2, 2000),
        1000,
        0,
        0,
        |_| {},
    )
    .unwrap();
    assert_eq!(snapshot.submitted_target_frames, 2);
    assert_eq!(snapshot.timing.unwrap().submitted_frames, 2);
    let report = snapshot.phase.phase.report.unwrap();
    assert_eq!((report.source_rate, report.target_rate), (1000, 2000));
    assert_eq!(report.target_frames, 8);
    assert_eq!(
        snapshot.phase.pulled_source_end,
        report.pulled_source_frame_cursor
    );
    assert_eq!(
        snapshot.phase.phase.submitted_end,
        TargetTime::from_frames(2, 2000).unwrap()
    );
    assert_eq!(&pcm.output[..2], &[0.125, 0.125 + 0.5 / 512.]);
    assert_eq!(state.pending_frames(), 6);
}

#[test]
fn actual_startup_and_finite_tail_keep_original_source_spans_and_target_boundaries() {
    let (_, mut state, _) = rig_boundaries(16, 1000, 1000, Some(2), Some(3));
    state.enable_phase_history(8).unwrap();
    state.enable_source_playback_history(8).unwrap();
    let basis = state.target_frame_basis();
    let telemetry = ConvertedTelemetry::with_phase_history(&state, basis).unwrap();
    let shared = Shared::with_converted_history(&telemetry);
    shared.status.store(1, Ordering::SeqCst);
    let report = publish(&mut state, &telemetry, &shared, 8, 8);
    for (frame, physical, playback, expected) in [
        (1, 0, 0, SourcePlaybackState::Frozen),
        (2, 2, 0, SourcePlaybackState::Running),
        (5, 5, 3, SourcePlaybackState::Frozen),
    ] {
        let snapshot = read(&shared, &telemetry, basis, &at(frame, 1000)).unwrap();
        assert_eq!(
            (
                snapshot.phase.playback.physical_start(),
                snapshot.phase.playback.playback_start(),
                snapshot.phase.playback.state()
            ),
            (physical, playback, expected)
        );
        assert_eq!(snapshot.phase.phase.report, Some(report));
        assert_eq!(snapshot.phase.phase.boundaries, state.boundaries());
        assert_eq!(
            snapshot.phase.phase.boundaries.startup.unwrap().target_time,
            TargetTime::from_frames(2, 1000).unwrap()
        );
        assert_eq!(
            snapshot.phase.phase.boundaries.end.unwrap().target_time,
            TargetTime::from_frames(5, 1000).unwrap()
        );
        assert!(snapshot.timing.is_none());
    }
}

#[test]
fn exact_submitted_endpoint_accepts_but_same_floor_fractional_future_refuses() {
    let (_, mut state, _, telemetry, shared) = rig();
    let basis = state.target_frame_basis();
    publish(&mut state, &telemetry, &shared, 8, 2);
    assert!(read(&shared, &telemetry, basis, &at(2, 1000)).is_some());
    assert!(read(&shared, &telemetry, basis, &beyond(2, 1000)).is_none());
    assert!(read(&shared, &telemetry, basis, &at(3, 1000)).is_none());
}

#[test]
fn explicit_original_source_identity_survives_later_conversion_history_creation() {
    let source = AudioFormat::new(1000, 1).unwrap();
    let (_, consumer) = command_queue(16).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            source,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
        ),
        SampleBank::new(source, PcmLimits::new(1024, 1024, 1).unwrap()).unwrap(),
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    mixer.render(&mut [0.; 4]).unwrap();
    let format = DeviceFormat::new(1000, 1, SampleEncoding::Float32, None).unwrap();
    let mut state = ConvertedNativeOutputState::new(
        mixer,
        format,
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        8,
    )
    .unwrap_or_else(|_| panic!("actual advanced Mixer is preserved"));
    state.enable_phase_history(8).unwrap();
    let basis = state.target_frame_basis();
    let history = state.converter_owner().source_playback_history().unwrap();
    assert_eq!(
        (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame()
        ),
        (1000, 0, 0)
    );
    assert_eq!(
        state
            .phase_history()
            .unwrap()
            .initial_source_position()
            .frame,
        4
    );
    let telemetry = ConvertedTelemetry::with_phase_history(&state, basis).unwrap();
    let shared = Shared::with_converted_history(&telemetry);
    shared.status.store(1, Ordering::SeqCst);
    publish(&mut state, &telemetry, &shared, 4, 2);
    let query =
        InterpolatedTargetTime::new(basis.time_at_stream_frame(2).unwrap(), 0, 0, 1, 0, 1).unwrap();
    let snapshot = read(&shared, &telemetry, basis, &query).unwrap();
    assert_eq!(snapshot.submitted_target_frames, 2);
    assert_eq!(snapshot.phase.playback.physical_start(), 0);
    assert!(shared_converted_playback_with(
        &shared,
        &telemetry,
        basis,
        ClockDomainId(8),
        &query,
        1000,
        4,
        4,
        |_| {}
    )
    .is_none());
}

#[test]
fn every_original_history_identity_field_must_match() {
    let (_, mut state, _, telemetry, shared) = rig();
    let basis = state.target_frame_basis();
    publish(&mut state, &telemetry, &shared, 8, 2);
    for (hz, physical, playback) in [(0, 0, 0), (999, 0, 0), (1000, 1, 0), (1000, 0, 1)] {
        assert!(shared_converted_playback_with(
            &shared,
            &telemetry,
            basis,
            ClockDomainId(8),
            &at(2, 1000),
            hz,
            physical,
            playback,
            |_| {}
        )
        .is_none());
    }
    assert!(read(&shared, &telemetry, basis, &at(2, 1000)).is_some());
}

#[test]
fn missing_phase_or_missing_source_history_never_grants_compound_playback() {
    for phase in [false, true] {
        let (_, mut state, _) = rig_boundaries(8, 1000, 1000, None, None);
        if phase {
            state.enable_phase_history(8).unwrap();
        } else {
            state.enable_source_playback_history(8).unwrap();
        }
        let basis = state.target_frame_basis();
        let telemetry = ConvertedTelemetry::with_phase_history(&state, basis).unwrap();
        let shared = Shared::with_converted_history(&telemetry);
        shared.status.store(1, Ordering::SeqCst);
        publish(&mut state, &telemetry, &shared, 8, 2);
        assert!(read(&shared, &telemetry, basis, &at(2, 1000)).is_none());
    }
}

#[test]
fn unopted_outer_shared_refuses_even_when_inner_history_has_positive_generation() {
    let (_, mut state, _, telemetry, _) = rig();
    let basis = state.target_frame_basis();
    let shared = Shared::new();
    shared.status.store(1, Ordering::SeqCst);
    publish(&mut state, &telemetry, &shared, 8, 2);
    assert!(read(&shared, &telemetry, basis, &at(2, 1000)).is_none());
    assert!(telemetry
        .playback_phase_snapshot_at_interpolated_time(&at(2, 1000))
        .is_some());
}

#[test]
fn no_positive_native_admission_refuses_generated_only_payload() {
    let (_, mut state, _, telemetry, shared) = rig();
    let basis = state.target_frame_basis();
    let report = state.render_pending(8).unwrap();
    telemetry.publish_generated(Some(report), &state);
    assert_eq!(shared.submitted.load(Ordering::SeqCst), 0);
    assert!(read(&shared, &telemetry, basis, &at(0, 1000)).is_none());
}

#[test]
fn odd_and_exhausted_outer_revision_refuse_without_callback_or_heap() {
    let (_, mut state, _, telemetry, shared) = rig();
    let basis = state.target_frame_basis();
    publish(&mut state, &telemetry, &shared, 8, 2);
    for revision in [1, u64::MAX] {
        shared
            .timing
            .test_sequence()
            .store(revision, Ordering::SeqCst);
        let calls = Cell::new(0);
        let (snapshot, allocations) = crate::audio::count_heap_calls(|| {
            shared_converted_playback_with(
                &shared,
                &telemetry,
                basis,
                ClockDomainId(8),
                &at(2, 1000),
                1000,
                0,
                0,
                |_| calls.set(calls.get() + 1),
            )
        });
        assert!(snapshot.is_none());
        assert_eq!(calls.get(), 0);
        assert_eq!(allocations, 0);
    }
}

#[test]
fn three_changed_outer_publications_are_the_exact_retry_bound() {
    let (_, mut state, _, telemetry, shared) = rig();
    let basis = state.target_frame_basis();
    publish(&mut state, &telemetry, &shared, 8, 2);
    let calls = Cell::new(0);
    let snapshot = shared_converted_playback_with(
        &shared,
        &telemetry,
        basis,
        ClockDomainId(8),
        &at(2, 1000),
        1000,
        0,
        0,
        |attempt| {
            assert_eq!(attempt, calls.get());
            calls.set(calls.get() + 1);
            shared.timing.invalidate();
        },
    );
    assert!(snapshot.is_none());
    assert_eq!(calls.get(), 3);
}

#[test]
fn native_timing_must_share_positive_submitted_target_count() {
    let (_, mut state, _, telemetry, shared) = rig();
    let basis = state.target_frame_basis();
    publish(&mut state, &telemetry, &shared, 8, 2);
    shared.timing.publish(stamp(1));
    assert!(read(&shared, &telemetry, basis, &at(2, 1000)).is_none());
    shared.timing.publish(stamp(2));
    assert_eq!(
        read(&shared, &telemetry, basis, &at(2, 1000))
            .unwrap()
            .timing,
        Some(stamp(2))
    );
}

#[test]
fn absent_native_timing_keeps_only_qualified_software_facts_without_heap() {
    let (_, mut state, _, telemetry, shared) = rig();
    let basis = state.target_frame_basis();
    publish(&mut state, &telemetry, &shared, 8, 2);
    let (snapshot, allocations) =
        crate::audio::count_heap_calls(|| read(&shared, &telemetry, basis, &at(2, 1000)));
    assert_eq!(allocations, 0);
    let snapshot = snapshot.unwrap();
    assert!(snapshot.timing.is_none());
    assert_eq!(
        snapshot.phase.playback.state(),
        SourcePlaybackState::Running
    );
    assert_eq!(snapshot.submitted_target_frames, 2);
}

#[test]
fn stop_or_nonrunning_lifecycle_refuses_before_and_after_read() {
    let (_, mut state, _, telemetry, shared) = rig();
    let basis = state.target_frame_basis();
    publish(&mut state, &telemetry, &shared, 8, 2);
    for status in [0, 2, 3, 4, 255] {
        shared.status.store(status, Ordering::SeqCst);
        assert!(read(&shared, &telemetry, basis, &at(2, 1000)).is_none());
    }
    shared.status.store(1, Ordering::SeqCst);
    assert!(shared_converted_playback_with(
        &shared,
        &telemetry,
        basis,
        ClockDomainId(8),
        &at(2, 1000),
        1000,
        0,
        0,
        |_| shared.stop.store(true, Ordering::SeqCst)
    )
    .is_none());
    shared.stop.store(false, Ordering::SeqCst);
    assert!(shared_converted_playback_with(
        &shared,
        &telemetry,
        basis,
        ClockDomainId(8),
        &at(2, 1000),
        1000,
        0,
        0,
        |_| shared.status.store(2, Ordering::SeqCst)
    )
    .is_none());
}

#[test]
fn actual_eagain_releases_guard_before_wait_and_preserves_original_positive_prefix() {
    let (_, mut state, config, telemetry, shared) = rig();
    let basis = state.target_frame_basis();
    let mut pcm = Pcm::new(&shared, &telemetry, basis, &[Write::Count(2), Write::Again]);
    assert!(pump(&mut pcm, &mut state, &config, 0, false).is_ok());
    assert_eq!(pcm.waits, 1);
    assert_eq!(pcm.timing_calls, 1);
    let snapshot = pcm.wait_snapshot.unwrap();
    assert_eq!(snapshot.submitted_target_frames, 2);
    assert_eq!(
        snapshot.phase.phase.submitted_end,
        TargetTime::from_frames(2, 1000).unwrap()
    );
    assert_eq!(state.pending_frames(), 6);
    assert_eq!(pcm.length, 2);
    assert!(
        read(&shared, &telemetry, basis, &at(2, 1000)).is_none(),
        "stop is not native permission"
    );
}

#[test]
fn actual_query_failure_preserves_irreversible_pcm_and_history_without_native_timing() {
    let (_, mut state, config, telemetry, shared) = rig();
    let basis = state.target_frame_basis();
    let mut pcm = Pcm::new(&shared, &telemetry, basis, &[Write::Count(2)]);
    pcm.timing_fail = true;
    assert!(pump(&mut pcm, &mut state, &config, 0, false).is_err());
    let snapshot = read(&shared, &telemetry, basis, &at(2, 1000)).unwrap();
    assert!(snapshot.timing.is_none());
    assert_eq!(snapshot.submitted_target_frames, 2);
    assert_eq!(state.pending_frames(), 6);
    assert_eq!(pcm.length, 2);
    assert!(read(&shared, &telemetry, basis, &beyond(2, 1000)).is_none());
}

#[test]
fn actual_negative_excess_and_encoder_failures_grant_no_positive_admission() {
    for (step, encoder_fail) in [
        (Write::Fail, false),
        (Write::Excess, false),
        (Write::Count(2), true),
    ] {
        let (_, mut state, config, telemetry, shared) = rig();
        let basis = state.target_frame_basis();
        let steps = [step];
        let mut pcm = Pcm::new(&shared, &telemetry, basis, &steps);
        assert!(pump(&mut pcm, &mut state, &config, 0, encoder_fail).is_err());
        assert_eq!(shared.submitted.load(Ordering::SeqCst), 0);
        assert_eq!(state.pending_frames(), 8);
        assert_eq!(pcm.length, 0);
        assert_eq!(shared.timing.sequence() & 1, 0);
        assert!(read(&shared, &telemetry, basis, &at(0, 1000)).is_none());
    }
}

#[test]
fn public_stream_native_reader_forwards_same_packet_and_refuses_stop_lifecycle() {
    let (_, mut state, configuration, telemetry, shared) = rig();
    let basis = state.target_frame_basis();
    publish(&mut state, &telemetry, &shared, 8, 2);
    shared.timing.publish(stamp(2));
    let stream = ConvertedAlsaStream {
        configuration,
        basis,
        shared: Arc::new(shared),
        telemetry: Arc::new(telemetry),
        worker: None,
        recovered_output: Some(state),
        retired: false,
    };
    let snapshot = stream.native_playback_at(&at(2, 1000), 1000, 0, 0).unwrap();
    assert_eq!(snapshot.timing, Some(stamp(2)));
    assert_eq!(snapshot.submitted_target_frames, 2);
    assert!(stream
        .native_playback_at(&beyond(2, 1000), 1000, 0, 0)
        .is_none());
    stream.shared.stop.store(true, Ordering::SeqCst);
    assert!(stream
        .native_playback_at(&at(2, 1000), 1000, 0, 0)
        .is_none());
}
