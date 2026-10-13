//! Scripted native calls around the actual production worker. These prove software
//! publication boundaries, not acoustic delivery or native clock accuracy.
use super::*;
use crate::audio::{count_heap_calls, fail_allocation};
use beatkernel::audio::{CommandProducer, InterpolatedTargetTime, SourcePlaybackState, TargetTime};
use std::cell::Cell;

fn rig(capacity: usize) -> (CommandProducer, NativeOutputState, AlsaAppliedConfig) {
    let (producer, state, config) = super::continuity_fixtures::rig(4);
    let mut mixer = state
        .into_mixer()
        .unwrap_or_else(|_| panic!("no pending output"));
    mixer.enable_source_playback_history(capacity).unwrap();
    let state = NativeOutputState::new(mixer, config.format, None, 4)
        .unwrap_or_else(|_| panic!("prepared memory state"));
    (producer, state, config)
}
fn at(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 8).unwrap(), 0, 0, 1, 0, 1).unwrap()
}
fn after(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(
        TargetTime::from_frames(frame, 8).unwrap(),
        0,
        1,
        4_000_000_000,
        1,
        1,
    )
    .unwrap()
}
fn read(shared: &Shared, query: &InterpolatedTargetTime) -> Option<AlsaSourcePlaybackSnapshot> {
    shared_source_playback_at(shared, ClockDomainId(8), query, 8, 0, 0)
}
fn stamp(submitted: u64) -> AlsaTimingSnapshot {
    timing::interpret(
        3,
        submitted,
        -2,
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
#[derive(Clone, Copy)]
enum Write {
    Count(usize),
    Again,
    Interrupted,
    Fail,
    Excess,
}
struct Pcm<'a> {
    shared: &'a Shared,
    steps: &'a [Write],
    writes: usize,
    stop_after: usize,
    waits: usize,
    wait_ready: bool,
    wait_fail: bool,
    stop_on_wait: bool,
    timing_fail: bool,
    timing_some: bool,
    timing_calls: usize,
    check_write_odd: bool,
    wait_probe: Option<u64>,
    output: [f32; 64],
    length: usize,
    producer: Option<&'a mut CommandProducer>,
    pause_after: Option<usize>,
    resume_after: Option<usize>,
}
impl<'a> Pcm<'a> {
    fn new(shared: &'a Shared, steps: &'a [Write], stop_after: usize) -> Self {
        Self {
            shared,
            steps,
            writes: 0,
            stop_after,
            waits: 0,
            wait_ready: true,
            wait_fail: false,
            stop_on_wait: false,
            timing_fail: false,
            timing_some: false,
            timing_calls: 0,
            check_write_odd: false,
            wait_probe: None,
            output: [0.0; 64],
            length: 0,
            producer: None,
            pause_after: None,
            resume_after: None,
        }
    }
}
impl PcmOperations for Pcm<'_> {
    fn write(&mut self, bytes: &[u8], frames: usize) -> Result<Option<usize>, LinuxError> {
        if self.check_write_odd {
            assert_eq!(self.shared.timing.sequence() & 1, 1);
            assert!(read(self.shared, &at(0)).is_none());
        }
        let step = self
            .steps
            .get(self.writes)
            .copied()
            .unwrap_or(Write::Count(frames));
        self.writes += 1;
        if let Some(producer) = self.producer.as_mut() {
            if self.pause_after == Some(self.writes) {
                producer.request_pause(true);
            }
            if self.resume_after == Some(self.writes) {
                producer.request_pause(false);
            }
        }
        if self.writes == self.stop_after {
            self.shared.stop.store(true, Ordering::Release);
        }
        match step {
            Write::Again | Write::Interrupted => Ok(None),
            Write::Fail => Err(LinuxError::InvalidConfiguration("script write")),
            Write::Excess => Ok(Some(frames + 1)),
            Write::Count(count) => {
                assert!(count <= frames);
                for chunk in bytes[..count * 4].chunks_exact(4) {
                    self.output[self.length] = f32::from_le_bytes(chunk.try_into().unwrap());
                    self.length += 1;
                }
                Ok(Some(count))
            }
        }
    }
    fn wait(&mut self) -> Result<bool, LinuxError> {
        self.waits += 1;
        if let Some(frame) = self.wait_probe {
            assert_eq!(
                self.shared.timing.sequence() & 1,
                0,
                "guard dropped before bounded wait"
            );
            let facts = read(self.shared, &at(frame))
                .expect("real previous positive prefix remains readable while waiting");
            assert!(
                facts.timing.is_none(),
                "old native query must not survive invalidation"
            );
            assert_eq!(facts.submitted_source_end, frame);
        }
        if self.stop_on_wait {
            self.shared.stop.store(true, Ordering::Release);
        }
        if self.wait_fail {
            Err(LinuxError::InvalidConfiguration("script wait"))
        } else {
            Ok(self.wait_ready)
        }
    }
    fn timing<C: WorkerClock>(
        &mut self,
        _: &C,
        submitted: u64,
    ) -> Result<Option<AlsaTimingSnapshot>, LinuxError> {
        self.timing_calls += 1;
        assert_eq!(
            self.shared.timing.sequence() & 1,
            1,
            "opted native query is inside reacquired outer guard"
        );
        if self.timing_fail {
            Err(LinuxError::InvalidConfiguration("script timing"))
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
impl Clock {
    fn new(fail_at: usize) -> Self {
        Self {
            calls: Cell::new(0),
            fail_at,
        }
    }
}
impl WorkerClock for Clock {
    fn now(&self) -> Result<ClockPoint, LinuxError> {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        if call == self.fail_at {
            Err(LinuxError::InvalidConfiguration("script clock"))
        } else {
            Ok(ClockPoint {
                domain: ClockDomainId(8),
                timestamp: Timestamp::from_nanos(call as i64),
            })
        }
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
            Err(LinuxError::InvalidConfiguration("script encoding"))
        } else {
            encode_pcm(format, samples, bytes).map_err(LinuxError::Conversion)
        }
    }
}
fn pump(
    pcm: &mut Pcm<'_>,
    state: &mut NativeOutputState,
    config: &AlsaAppliedConfig,
    clock: &Clock,
    encode_fail: bool,
) -> Result<(), LinuxError> {
    let mut bytes = [0; 16];
    let shared = pcm.shared;
    run_worker(
        pcm,
        state,
        config,
        &mut bytes,
        shared,
        clock,
        &mut Encoder { fail: encode_fail },
    )
}

#[test]
fn actual_partial_admission_guards_write_and_refuses_same_floor_fractional_future() {
    let (_, mut state, config) = rig(8);
    let shared = Shared::with_source_history(&state).unwrap();
    assert!(read(&shared, &at(0)).is_none());
    let mut pcm = Pcm::new(&shared, &[Write::Count(1)], 1);
    pcm.check_write_odd = true;
    pump(&mut pcm, &mut state, &config, &Clock::new(0), false).unwrap();
    let value = read(&shared, &at(1)).unwrap();
    assert_eq!(value.submitted_source_end, 1);
    assert_eq!(value.pulled_source_end, 4);
    assert_eq!(value.render.unwrap().frames, 4);
    assert_eq!(value.revision & 1, 0);
    assert!(read(&shared, &after(1)).is_none());
    assert_eq!(
        &pcm.output[..pcm.length],
        &super::continuity_fixtures::PCM[..1]
    );
    assert_eq!(state.pending_frames(), 3);
}

#[test]
fn actual_pause_resume_keeps_original_history_and_actual_native_prefix() {
    let (mut producer, mut state, config) = rig(8);
    let shared = Shared::with_source_history(&state).unwrap();
    let mut pcm = Pcm::new(&shared, &[], 3);
    pcm.producer = Some(&mut producer);
    pcm.pause_after = Some(1);
    pcm.resume_after = Some(2);
    pump(&mut pcm, &mut state, &config, &Clock::new(0), false).unwrap();
    assert_eq!(state.mixer().frame_cursor(), 12);
    assert_eq!(state.mixer().playback_frame_cursor(), 8);
    for (frame, start, playback, status) in [
        (0, 0, 0, SourcePlaybackState::Running),
        (4, 4, 4, SourcePlaybackState::Frozen),
        (7, 4, 4, SourcePlaybackState::Frozen),
        (8, 8, 4, SourcePlaybackState::Running),
        (12, 8, 4, SourcePlaybackState::Running),
    ] {
        let value = read(&shared, &at(frame)).unwrap();
        assert_eq!(value.playback.physical_start(), start);
        assert_eq!(value.playback.playback_start(), playback);
        assert_eq!(value.playback.state(), status);
        assert_eq!(value.submitted_source_end, 12);
    }
    assert!(read(&shared, &after(12)).is_none());
}

#[test]
fn pending_tail_reopen_has_original_history_but_only_new_stream_positive_prefix() {
    let (_, mut state, config) = rig(8);
    let first = Shared::with_source_history(&state).unwrap();
    pump(
        &mut Pcm::new(&first, &[Write::Count(1)], 1),
        &mut state,
        &config,
        &Clock::new(0),
        false,
    )
    .unwrap();
    let reopened = Shared::with_source_history(&state).unwrap();
    assert!(
        read(&reopened, &at(1)).is_none(),
        "old generated history is not a new submission"
    );
    let mut pcm = Pcm::new(&reopened, &[Write::Count(1)], 1);
    pump(&mut pcm, &mut state, &config, &Clock::new(0), false).unwrap();
    let value = read(&reopened, &at(2)).unwrap();
    assert_eq!(value.submitted_source_end, 2);
    assert_eq!(value.pulled_source_end, 4);
    assert!(
        value.render.is_none(),
        "retained tail fabricated no fresh callback"
    );
    assert!(read(&reopened, &at(0)).is_none());
    assert!(read(&reopened, &after(2)).is_none());
    assert_eq!(state.mixer().frame_cursor(), 4);
    assert_eq!(
        &pcm.output[..pcm.length],
        &super::continuity_fixtures::PCM[1..2]
    );
}

#[test]
fn cold_source_publication_allocation_refusal_preserves_complete_pending_owner() {
    let (_, mut state, _) = rig(8);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let basis = state.output_frame_basis();
    assert!(matches!(
        fail_allocation(0, || Shared::with_source_history(&state)),
        Err(AudioError::AllocationFailed)
    ));
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(
        state.pending_samples(),
        &super::continuity_fixtures::PCM[1..4]
    );
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .pulled_end_frame(),
        4
    );
    let retry = Shared::with_source_history(&state).unwrap();
    assert!(read(&retry, &at(1)).is_none());
}

#[test]
fn bounded_wait_exposes_true_source_without_timing_then_reacquires_for_ready_query() {
    for step in [Write::Count(0), Write::Again, Write::Interrupted] {
        for ready in [false, true] {
            let (_, mut state, config) = rig(8);
            let shared = Shared::with_source_history(&state).unwrap();
            let steps = [Write::Count(1), step];
            let mut pcm = Pcm::new(&shared, &steps, usize::MAX);
            pcm.timing_some = true;
            pcm.wait_probe = Some(1);
            pcm.wait_ready = ready;
            pcm.stop_on_wait = true;
            pump(&mut pcm, &mut state, &config, &Clock::new(0), false).unwrap();
            let value = read(&shared, &at(1)).unwrap();
            assert_eq!(pcm.waits, 1);
            assert_eq!(pcm.timing_calls, if ready { 2 } else { 1 });
            assert_eq!(value.timing, ready.then(|| stamp(1)));
            assert_eq!(value.submitted_source_end, 1);
            assert_eq!(value.pulled_source_end, 4);
        }
    }
}

#[test]
fn write_encoding_and_wait_failure_keep_actual_generation_without_native_submission() {
    for failure in 0..4 {
        let (_, mut state, config) = rig(8);
        let shared = Shared::with_source_history(&state).unwrap();
        let steps = [match failure {
            0 => Write::Fail,
            1 => Write::Excess,
            _ => Write::Again,
        }];
        let mut pcm = Pcm::new(&shared, &steps, usize::MAX);
        pcm.wait_fail = true;
        assert!(pump(&mut pcm, &mut state, &config, &Clock::new(0), failure == 3).is_err());
        assert_eq!(state.mixer().frame_cursor(), 4);
        assert_eq!(state.pending_frames(), 4);
        assert_eq!(shared.submitted.load(Ordering::SeqCst), 0);
        assert_eq!(shared.render_telemetry.read().render.unwrap().frames, 4);
        assert!(read(&shared, &at(0)).is_none());
        assert!(shared.timing.snapshot(ClockDomainId(8)).is_none());
        assert_eq!(shared.timing.sequence() & 1, 0);
    }
}

#[test]
fn positive_prefix_survives_native_timing_and_later_clock_refusal() {
    for fail_timing in [false, true] {
        let (_, mut state, config) = rig(8);
        let shared = Shared::with_source_history(&state).unwrap();
        let mut pcm = Pcm::new(&shared, &[Write::Count(1)], usize::MAX);
        pcm.timing_some = true;
        pcm.timing_fail = fail_timing;
        assert!(pump(
            &mut pcm,
            &mut state,
            &config,
            &Clock::new(if fail_timing { 0 } else { 2 }),
            false
        )
        .is_err());
        let value = read(&shared, &at(1)).unwrap();
        assert_eq!(value.submitted_source_end, 1);
        assert_eq!(value.pulled_source_end, 4);
        assert_eq!(value.timing, (!fail_timing).then(|| stamp(1)));
        assert_eq!(state.pending_frames(), 3);
        assert!(!shared.observed_valid.load(Ordering::Acquire));
    }
}

#[test]
fn actual_native_query_keeps_raw_signed_delay_htstamp_and_unknown_quality() {
    let (_, mut state, config) = rig(8);
    let shared = Shared::with_source_history(&state).unwrap();
    let mut pcm = Pcm::new(&shared, &[], 1);
    pcm.timing_some = true;
    pump(&mut pcm, &mut state, &config, &Clock::new(0), false).unwrap();
    let timing = read(&shared, &at(4)).unwrap().timing.unwrap();
    assert_eq!(timing, stamp(4));
    assert_eq!(timing.delay_frames, -2);
    assert_eq!(timing.native_htstamp.nanoseconds, 123);
    assert_eq!(timing.estimated_played_frames, None);
    assert_eq!(
        timing.quality,
        beatkernel::time::ClockMappingQuality::Unknown
    );
}

#[test]
fn odd_exhausted_and_three_changed_outer_generations_never_authorize_crossed_facts() {
    let (_, mut state, config) = rig(8);
    let shared = Shared::with_source_history(&state).unwrap();
    pump(
        &mut Pcm::new(&shared, &[], 1),
        &mut state,
        &config,
        &Clock::new(0),
        false,
    )
    .unwrap();
    let version = shared.timing.sequence();
    shared
        .timing
        .test_sequence()
        .store(version + 1, Ordering::SeqCst);
    assert!(read(&shared, &at(1)).is_none());
    shared
        .timing
        .test_sequence()
        .store(version + 2, Ordering::SeqCst);
    let attempts = Cell::new(0);
    let value = shared_source_playback_with(&shared, ClockDomainId(8), &at(1), 8, 0, 0, |_| {
        attempts.set(attempts.get() + 1);
        shared.timing.test_sequence().fetch_add(2, Ordering::SeqCst);
    });
    assert!(value.is_none());
    assert_eq!(attempts.get(), 3);
    shared
        .timing
        .test_sequence()
        .store(u64::MAX - 1, Ordering::SeqCst);
    drop(shared.timing.update());
    assert_eq!(shared.timing.sequence(), u64::MAX);
    assert!(read(&shared, &at(1)).is_none());
    drop(shared.timing.update());
    assert_eq!(shared.timing.sequence(), u64::MAX, "no wrapping ABA");
}

#[test]
fn exhausted_publication_does_not_stop_actual_audio_or_allocate() {
    let (_, mut state, config) = rig(8);
    let shared = Shared::with_source_history(&state).unwrap();
    shared
        .timing
        .test_sequence()
        .store(u64::MAX - 1, Ordering::SeqCst);
    let mut pcm = Pcm::new(&shared, &[], 1);
    // No timing callback is required in this exhausted run, but production still
    // may ask for a real query. Its odd MAX is unavailable, never wrapped.
    let (result, calls) =
        count_heap_calls(|| pump(&mut pcm, &mut state, &config, &Clock::new(0), false));
    result.unwrap();
    assert_eq!(calls, 0);
    assert_eq!(pcm.length, 4);
    assert_eq!(state.admitted_frames(), 4);
    assert_eq!(shared.timing.sequence(), u64::MAX);
    assert!(read(&shared, &at(1)).is_none());
}

#[test]
fn actual_opted_render_pending_replay_and_exact_reader_have_zero_hot_heap_operations() {
    for retained in [false, true] {
        let (_, mut state, config) = rig(8);
        if retained {
            state.render_pending(4).unwrap();
            state.admit(1).unwrap();
        }
        let shared = Shared::with_source_history(&state).unwrap();
        let mut pcm = Pcm::new(&shared, &[], 1);
        let (result, calls) =
            count_heap_calls(|| pump(&mut pcm, &mut state, &config, &Clock::new(0), false));
        result.unwrap();
        assert_eq!(calls, 0);
        let query = at(4);
        let (value, calls) = count_heap_calls(|| read(&shared, &query));
        assert!(value.is_some());
        assert_eq!(calls, 0);
    }
}

#[test]
fn actual_history_capacity_render_refusal_notifies_without_new_report_or_pcm() {
    let (mut producer, mut state, config) = rig(1);
    let shared = Shared::with_source_history(&state).unwrap();
    let mut pcm = Pcm::new(&shared, &[], usize::MAX);
    pcm.producer = Some(&mut producer);
    pcm.pause_after = Some(1);
    let (result, calls) =
        count_heap_calls(|| pump(&mut pcm, &mut state, &config, &Clock::new(0), false));
    assert!(result.is_err());
    assert_eq!(calls, 0);
    assert_eq!(pcm.writes, 1);
    assert_eq!(state.mixer().frame_cursor(), 4);
    assert_eq!(state.pending_frames(), 0);
    let value = read(&shared, &at(4)).unwrap();
    assert_eq!(value.pulled_source_end, 4);
    assert_eq!(value.submitted_source_end, 4);
    assert_eq!(value.render.unwrap().start_frame, 0);
    assert!(
        value.timing.is_none(),
        "failed render invalidated previous timing"
    );
}

#[test]
fn identity_refusal_and_legacy_disabled_publication_do_not_supply_source_proof() {
    let (_, mut state, config) = rig(8);
    let shared = Shared::with_source_history(&state).unwrap();
    pump(
        &mut Pcm::new(&shared, &[], 1),
        &mut state,
        &config,
        &Clock::new(0),
        false,
    )
    .unwrap();
    for (hz, physical, playback) in [(9, 0, 0), (8, 1, 0), (8, 0, 1)] {
        assert!(shared_source_playback_at(
            &shared,
            ClockDomainId(8),
            &at(1),
            hz,
            physical,
            playback
        )
        .is_none());
    }
    assert!(read(&Shared::new(), &at(0)).is_none());
}

#[test]
fn actual_generic_worker_notifies_render_refusal_before_propagating_error() {
    let (mut producer, mut state, config) = rig(1);
    let shared = Shared::with_source_history(&state).unwrap();
    let mut pcm = Pcm::new(&shared, &[], usize::MAX);
    pcm.producer = Some(&mut producer);
    pcm.pause_after = Some(1);
    let mut reports = 0;
    let mut refusals = 0;
    let mut bytes = [0; 16];
    let result = run_output_worker(
        &mut pcm,
        &mut state,
        &config,
        &mut bytes,
        &shared,
        &Clock::new(0),
        &mut Encoder { fail: false },
        |report, state, _| {
            if let Some(report) = report {
                reports += 1;
                assert_eq!(report.start_frame, 0);
            } else {
                refusals += 1;
                assert_eq!(state.mixer().frame_cursor(), 4);
                assert_eq!(
                    state
                        .mixer()
                        .source_playback_history()
                        .unwrap()
                        .pulled_end_frame(),
                    4
                );
                assert_eq!(state.pending_frames(), 0);
            }
        },
    );
    assert!(result.is_err());
    assert_eq!((reports, refusals), (1, 1));
    assert_eq!(pcm.writes, 1);
}

#[test]
fn actual_start_gate_and_finite_tail_publish_three_real_source_intervals() {
    use beatkernel::audio::{
        command_queue_with_start_gate, AudioFormat, AudioLimits, MixerConfig, PcmLimits, SampleBank,
    };
    let (_, _, config) = rig(8);
    let format = AudioFormat::new(8, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(128, 512, 1).unwrap()).unwrap();
    let (mut producer, consumer) = command_queue_with_start_gate(16).unwrap();
    producer.schedule_start_at(1).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(16, 4, 16, 16, 16).unwrap(),
        )
        .with_playback_end_frame(2),
        bank,
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(3).unwrap();
    let mut state = NativeOutputState::new(mixer, config.format, None, 4)
        .unwrap_or_else(|_| panic!("gate memory state"));
    let shared = Shared::with_source_history(&state).unwrap();
    pump(
        &mut Pcm::new(&shared, &[], 1),
        &mut state,
        &config,
        &Clock::new(0),
        false,
    )
    .unwrap();
    for (frame, physical, playback, status) in [
        (0, 0, 0, SourcePlaybackState::Frozen),
        (1, 1, 0, SourcePlaybackState::Running),
        (2, 1, 0, SourcePlaybackState::Running),
        (3, 3, 2, SourcePlaybackState::Frozen),
        (4, 3, 2, SourcePlaybackState::Frozen),
    ] {
        let value = read(&shared, &at(frame)).unwrap();
        assert_eq!(value.playback.physical_start(), physical);
        assert_eq!(value.playback.playback_start(), playback);
        assert_eq!(value.playback.state(), status);
        assert_eq!(value.pulled_source_end, 4);
        assert_eq!(value.submitted_source_end, 4);
    }
}

#[test]
fn wait_failure_after_positive_prefix_preserves_true_facts_without_old_native_timing() {
    let (_, mut state, config) = rig(8);
    let shared = Shared::with_source_history(&state).unwrap();
    let mut pcm = Pcm::new(&shared, &[Write::Count(1), Write::Again], usize::MAX);
    pcm.wait_fail = true;
    pcm.wait_probe = Some(1);
    pcm.timing_some = true;
    assert!(pump(&mut pcm, &mut state, &config, &Clock::new(0), false).is_err());
    let value = read(&shared, &at(1)).unwrap();
    assert_eq!(value.submitted_source_end, 1);
    assert_eq!(value.pulled_source_end, 4);
    assert!(value.timing.is_none());
    assert_eq!(state.pending_frames(), 3);
}

#[test]
fn native_timing_from_another_submitted_prefix_cannot_enter_compound_proof() {
    let (_, mut state, config) = rig(8);
    let shared = Shared::with_source_history(&state).unwrap();
    pump(
        &mut Pcm::new(&shared, &[], 1),
        &mut state,
        &config,
        &Clock::new(0),
        false,
    )
    .unwrap();
    let guard = shared.timing.update();
    guard.publish(stamp(3));
    drop(guard);
    assert!(read(&shared, &at(1)).is_none());
    let guard = shared.timing.update();
    guard.publish(stamp(4));
    drop(guard);
    assert_eq!(read(&shared, &at(1)).unwrap().timing, Some(stamp(4)));
}

#[test]
fn opted_requested_maximum_overflow_refuses_before_irrevocable_native_write() {
    let (_, mut state, config) = rig(8);
    let shared = Shared::with_source_history(&state).unwrap();
    // Corrupt software counter fixture: no claim this is produced by a driver.
    shared.submitted.store(u64::MAX, Ordering::SeqCst);
    let mut pcm = Pcm::new(&shared, &[], 1);
    assert!(matches!(
        pump(&mut pcm, &mut state, &config, &Clock::new(0), false),
        Err(LinuxError::Overflow)
    ));
    assert_eq!(pcm.writes, 0);
    assert_eq!(state.mixer().frame_cursor(), 4);
    assert_eq!(state.pending_frames(), 4);
    assert_eq!(state.admitted_frames(), 0);
    assert!(read(&shared, &at(0)).is_none());
}

#[test]
fn inner_telemetry_exhaustion_closes_proof_but_does_not_stop_audio() {
    let (_, mut state, config) = rig(8);
    let shared = Shared::with_source_history(&state).unwrap();
    let mut exhausted = u64::MAX;
    shared.render_telemetry.publish_with_source_history(
        None,
        state.mixer().source_playback_history(),
        &mut exhausted,
    );
    let mut pcm = Pcm::new(&shared, &[], 1);
    pump(&mut pcm, &mut state, &config, &Clock::new(0), false).unwrap();
    assert_eq!(pcm.length, 4);
    assert_eq!(state.mixer().frame_cursor(), 4);
    assert_eq!(state.pending_frames(), 0);
    assert_eq!(shared.submitted.load(Ordering::SeqCst), 4);
    assert!(read(&shared, &at(1)).is_none());
}
