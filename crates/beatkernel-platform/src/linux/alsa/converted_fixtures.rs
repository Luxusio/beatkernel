//! Scripted native operations call the actual statically dispatched ALSA pump.
use super::*;
use crate::audio::ConvertedNativeOutputState;
use beatkernel::audio::*;
use std::cell::Cell;

fn sample(frame: usize) -> f32 {
    0.125 + frame as f32 / 512.0
}
pub(super) fn rig(
    period: usize,
) -> (
    CommandProducer,
    ConvertedNativeOutputState,
    AlsaAppliedConfig,
) {
    rig_boundaries(period, 44_100, 48_000, None, None)
}
pub(super) fn rig_boundaries(
    period: usize,
    source_rate: u32,
    target_rate: u32,
    gate: Option<u64>,
    end: Option<u64>,
) -> (
    CommandProducer,
    ConvertedNativeOutputState,
    AlsaAppliedConfig,
) {
    let source = AudioFormat::new(source_rate, 1).unwrap();
    let limits = PcmLimits::new(1024, 1024, 1).unwrap();
    let mut bank = SampleBank::new(source, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(source, (0..128).map(sample).collect(), limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = if gate.is_some() {
        command_queue_with_start_gate(16).unwrap()
    } else {
        command_queue(16).unwrap()
    };
    if let Some(frame) = gate {
        producer.schedule_start_at(frame).unwrap();
    }
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(-123),
            gain: 1.0,
        })
        .unwrap();
    let mut mixer_config = MixerConfig::new(
        source,
        ClockDomainId(7),
        Timestamp::from_nanos(-123),
        AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
    );
    if let Some(frame) = end {
        mixer_config = mixer_config.with_playback_end_frame(frame);
    }
    let mixer = Mixer::new(mixer_config, bank, consumer).unwrap();
    let format = DeviceFormat::new(target_rate, 1, SampleEncoding::Float32, None).unwrap();
    let requested = AlsaRequest {
        device: "memory-no-native".into(),
        format,
        buffer_frames: 32,
        period_frames: period as u32,
        allow_size_rounding: false,
        monotonic_domain: ClockDomainId(8),
    };
    let config = AlsaAppliedConfig {
        requested,
        format,
        buffer_frames: 32,
        period_frames: period as u32,
        sizing_adjusted: false,
        output_domain: ClockDomainId(7),
        output_origin: Timestamp::from_nanos(-123),
    };
    let owner = ConvertedNativeOutputState::new(
        mixer,
        format,
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        period,
    )
    .unwrap_or_else(|_| panic!("valid converted owner"));
    (producer, owner, config)
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
    waits: usize,
    wait_ready: bool,
    wait_fail: bool,
    timing_fail: bool,
    drop_fail: bool,
    stop_after: usize,
    stop_on_wait: bool,
    output: [f32; 128],
    length: usize,
    drops: usize,
}
impl<'a> Pcm<'a> {
    fn new(shared: &'a Shared, steps: &'a [Write], stop_after: usize) -> Self {
        Self {
            shared,
            steps,
            writes: 0,
            waits: 0,
            wait_ready: true,
            wait_fail: false,
            timing_fail: false,
            drop_fail: false,
            stop_after,
            stop_on_wait: false,
            output: [0.; 128],
            length: 0,
            drops: 0,
        }
    }
}
impl PcmOperations for Pcm<'_> {
    fn write(&mut self, bytes: &[u8], frames: usize) -> Result<Option<usize>, LinuxError> {
        let step = self
            .steps
            .get(self.writes)
            .copied()
            .unwrap_or(Write::Count(frames));
        self.writes += 1;
        if self.writes == self.stop_after {
            self.shared.stop.store(true, Ordering::Release);
        }
        match step {
            Write::Again | Write::Interrupted => Ok(None),
            Write::Fail => Err(LinuxError::Alsa {
                operation: "converted script write",
                code: -32,
            }),
            Write::Excess => Ok(Some(frames + 1)),
            Write::Count(count) => {
                assert!(count <= frames);
                for word in bytes[..count * 4].chunks_exact(4) {
                    self.output[self.length] = f32::from_le_bytes(word.try_into().unwrap());
                    self.length += 1;
                }
                Ok(Some(count))
            }
        }
    }
    fn wait(&mut self) -> Result<bool, LinuxError> {
        self.waits += 1;
        if self.stop_on_wait {
            self.shared.stop.store(true, Ordering::Release);
        }
        if self.wait_fail {
            Err(LinuxError::Alsa {
                operation: "converted script wait",
                code: -32,
            })
        } else {
            Ok(self.wait_ready)
        }
    }
    fn timing<C: WorkerClock>(
        &mut self,
        _clock: &C,
        _submitted: u64,
    ) -> Result<Option<AlsaTimingSnapshot>, LinuxError> {
        if self.timing_fail {
            Err(LinuxError::Alsa {
                operation: "converted script timing",
                code: -32,
            })
        } else {
            Ok(None)
        }
    }
    fn drop_stream(&mut self) -> Result<(), LinuxError> {
        self.drops += 1;
        if self.drop_fail {
            Err(LinuxError::Alsa {
                operation: "converted script drop",
                code: -32,
            })
        } else {
            Ok(())
        }
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
            return Err(LinuxError::InvalidConfiguration("converted script clock"));
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
            Err(LinuxError::InvalidConfiguration("converted script encode"))
        } else {
            encode_pcm(format, samples, bytes).map_err(LinuxError::Conversion)
        }
    }
}
fn pump(
    pcm: &mut Pcm<'_>,
    owner: &mut ConvertedNativeOutputState,
    config: &AlsaAppliedConfig,
    clock: &Clock,
    encoder: &mut Encoder,
    telemetry: &converted_telemetry::ConvertedTelemetry,
) -> Result<(), LinuxError> {
    let mut bytes = [0_u8; 128];
    let shared = pcm.shared;
    converted::run_converted_worker(
        pcm,
        owner,
        config,
        &mut bytes[..config.period_frames as usize * 4],
        shared,
        clock,
        encoder,
        telemetry,
    )
}
fn expected(first: usize, count: usize) -> Vec<f32> {
    (first..first + count)
        .map(|frame| (0.125 + frame as f64 * 147.0 / (160.0 * 512.0)) as f32)
        .collect()
}
fn close(actual: &[f32], reference: &[f32]) {
    assert_eq!(actual.len(), reference.len());
    for (a, b) in actual.iter().zip(reference) {
        assert!((a - b).abs() < 2e-6, "{a} != {b}");
    }
}

#[test]
fn production_pump_full_and_short_writes_deliver_unequal_rate_target_pcm_once_without_heap() {
    for steps in [
        &[Write::Count(8)][..],
        &[Write::Count(2), Write::Count(6)][..],
    ] {
        let (_producer, mut state, config) = rig(8);
        let shared = Shared::new();
        let mut pcm = Pcm::new(&shared, steps, steps.len());
        let telemetry = converted_telemetry::ConvertedTelemetry::new();
        let clock = Clock::new(0);
        let mut encoder = Encoder { fail: false };
        let (result, calls) = crate::audio::count_heap_calls(|| {
            pump(
                &mut pcm,
                &mut state,
                &config,
                &clock,
                &mut encoder,
                &telemetry,
            )
        });
        result.unwrap();
        assert_eq!(calls, 0);
        close(&pcm.output[..pcm.length], &expected(0, 8));
        assert_eq!(state.pending_frames(), 0);
        assert_eq!(shared.submitted.load(Ordering::Relaxed), 8);
        assert_eq!(shared.rendered.load(Ordering::Relaxed), 8);
        assert_eq!(shared.renders.load(Ordering::Relaxed), 1);
        assert_eq!(pcm.drops, 1);
        let report = telemetry.read().unwrap().0;
        assert_eq!(report.target_frames, 8);
        assert_eq!(report.target_rate, 48_000);
        assert_eq!(
            state.target_frame_basis().start_time(),
            TargetTime::from_frames(8, 48_000).unwrap()
        );
    }
}

#[test]
fn zero_again_interrupted_and_wait_variants_keep_original_target_block() {
    for step in [Write::Count(0), Write::Again, Write::Interrupted] {
        for ready in [false, true] {
            let (_producer, mut state, config) = rig(8);
            let shared = Shared::new();
            let steps = [step];
            let mut pcm = Pcm::new(&shared, &steps, usize::MAX);
            pcm.wait_ready = ready;
            pcm.stop_on_wait = true;
            let telemetry = converted_telemetry::ConvertedTelemetry::new();
            pump(
                &mut pcm,
                &mut state,
                &config,
                &Clock::new(0),
                &mut Encoder { fail: false },
                &telemetry,
            )
            .unwrap();
            assert_eq!(pcm.waits, 1);
            assert_eq!(pcm.length, 0);
            close(state.pending_samples(), &expected(0, 8));
            assert_eq!(state.admitted_frames(), 0);
            assert_eq!(shared.renders.load(Ordering::Relaxed), 1);
            assert_eq!(shared.submitted.load(Ordering::Relaxed), 0);
        }
    }
}

#[test]
fn encoding_write_wait_excess_and_pre_render_clock_refusals_preserve_exact_owner() {
    for failure in 0..5 {
        let (_producer, mut state, config) = rig(8);
        let shared = Shared::new();
        let steps = [match failure {
            1 => Write::Fail,
            2 => Write::Again,
            3 => Write::Excess,
            _ => Write::Count(8),
        }];
        let mut pcm = Pcm::new(&shared, &steps, usize::MAX);
        pcm.wait_fail = failure == 2;
        let telemetry = converted_telemetry::ConvertedTelemetry::new();
        assert!(pump(
            &mut pcm,
            &mut state,
            &config,
            &Clock::new(if failure == 4 { 1 } else { 0 }),
            &mut Encoder { fail: failure == 0 },
            &telemetry
        )
        .is_err());
        assert_eq!(state.admitted_frames(), 0);
        assert_eq!(shared.submitted.load(Ordering::Relaxed), 0);
        if failure == 4 {
            assert_eq!(state.pending_frames(), 0);
            assert_eq!(state.mixer().frame_cursor(), 0);
        } else {
            close(state.pending_samples(), &expected(0, 8));
            let pulled = state.mixer().frame_cursor();
            let retry = Shared::new();
            let mut retry_pcm = Pcm::new(&retry, &[], 1);
            let retry_telemetry = converted_telemetry::ConvertedTelemetry::new();
            pump(
                &mut retry_pcm,
                &mut state,
                &config,
                &Clock::new(0),
                &mut Encoder { fail: false },
                &retry_telemetry,
            )
            .unwrap();
            close(&retry_pcm.output[..retry_pcm.length], &expected(0, 8));
            assert_eq!(state.mixer().frame_cursor(), pulled);
            assert_eq!(retry.renders.load(Ordering::Relaxed), 0);
        }
    }
}

#[test]
fn positive_prefix_survives_timing_clock_counter_and_drop_failure_before_small_period_reopen() {
    for failure in 0..4 {
        let (_producer, mut state, mut config) = rig(8);
        let shared = Shared::new();
        if failure == 2 {
            shared.submitted.store(u64::MAX, Ordering::Relaxed);
        }
        let mut pcm = Pcm::new(
            &shared,
            &[Write::Count(2)],
            if failure == 3 { 1 } else { usize::MAX },
        );
        pcm.timing_fail = failure == 0;
        pcm.drop_fail = failure == 3;
        let telemetry = converted_telemetry::ConvertedTelemetry::new();
        assert!(pump(
            &mut pcm,
            &mut state,
            &config,
            &Clock::new(if failure == 1 { 2 } else { 0 }),
            &mut Encoder { fail: false },
            &telemetry
        )
        .is_err());
        close(&pcm.output[..pcm.length], &expected(0, 2));
        close(state.pending_samples(), &expected(2, 6));
        assert_eq!(state.admitted_frames(), 2);
        assert_eq!(
            state.target_frame_basis().start_time(),
            TargetTime::from_frames(2, 48_000).unwrap()
        );
        let pulled = state.mixer().frame_cursor();
        state
            .reconfigure(config.format, ChannelMatrix::default_mix(1, 1).unwrap(), 2)
            .unwrap();
        config.period_frames = 2;
        config.requested.period_frames = 2;
        let retry = Shared::new();
        let mut retry_pcm = Pcm::new(&retry, &[], 3);
        let retry_telemetry = converted_telemetry::ConvertedTelemetry::new();
        let (result, calls) = crate::audio::count_heap_calls(|| {
            pump(
                &mut retry_pcm,
                &mut state,
                &config,
                &Clock::new(0),
                &mut Encoder { fail: false },
                &retry_telemetry,
            )
        });
        result.unwrap();
        assert_eq!(calls, 0);
        close(&retry_pcm.output[..retry_pcm.length], &expected(2, 6));
        assert_eq!(state.mixer().frame_cursor(), pulled);
        assert_eq!(retry.renders.load(Ordering::Relaxed), 0);
        assert_eq!(retry.submitted.load(Ordering::Relaxed), 6);
        assert_eq!(state.pending_frames(), 0);
    }
}

#[test]
fn stop_before_pump_preserves_pending_pcm_and_never_publishes_fresh_source_or_target_report() {
    let (_producer, mut state, config) = rig(8);
    state.render_pending(8).unwrap();
    state.admit(2).unwrap();
    let shared = Shared::new();
    shared.stop.store(true, Ordering::Release);
    let mut pcm = Pcm::new(&shared, &[], 1);
    let telemetry = converted_telemetry::ConvertedTelemetry::new();
    pump(
        &mut pcm,
        &mut state,
        &config,
        &Clock::new(0),
        &mut Encoder { fail: false },
        &telemetry,
    )
    .unwrap();
    assert_eq!(pcm.writes, 0);
    assert_eq!(pcm.drops, 1);
    close(state.pending_samples(), &expected(2, 6));
    assert_eq!(telemetry.read(), None);
    assert_eq!(shared.render_telemetry.read().render, None);
}

#[test]
fn actual_worker_retirement_retains_complete_converted_suffix_and_original_error_for_single_take() {
    for failed in [false, true] {
        let (mut producer, mut owner, configuration) = rig(8);
        let basis = owner.target_frame_basis();
        let shared = Arc::new(Shared::new());
        let worker_shared = shared.clone();
        let telemetry = Arc::new(converted_telemetry::ConvertedTelemetry::new());
        let worker_telemetry = telemetry.clone();
        let (release, released) = mpsc::channel();
        let worker = thread::spawn(move || {
            released.recv().unwrap();
            let report = owner.render_pending(8).unwrap();
            owner.admit(2).unwrap();
            worker_telemetry.publish(report, owner.boundaries(), owner.last_real_source_report());
            worker_shared.rendered.store(8, Ordering::Relaxed);
            worker_shared.submitted.store(2, Ordering::Relaxed);
            if failed {
                worker_shared.status.store(3, Ordering::Release);
                worker_shared.errno.store(-32, Ordering::Relaxed);
                worker_shared.failures.store(1, Ordering::Relaxed);
            }
            let result = if failed {
                Err(LinuxError::InvalidConfiguration(
                    "original converted worker error",
                ))
            } else {
                Ok(())
            };
            (result, owner)
        });
        let mut stream = converted::ConvertedAlsaStream {
            configuration,
            basis,
            shared,
            telemetry,
            worker: Some(worker),
            recovered_output: None,
            retired: false,
        };
        assert!(matches!(
            stream.take_stopped_output(),
            Err(LinuxError::InvalidLifecycle)
        ));
        assert!(!stream.shared.stop.load(Ordering::Acquire));
        release.send(()).unwrap();
        let result = stream.stop();
        if failed {
            assert!(matches!(
                result,
                Err(LinuxError::InvalidConfiguration(
                    "original converted worker error"
                ))
            ));
        } else {
            result.unwrap();
        }
        let report = stream.last_render_report().unwrap();
        let boundaries = stream.boundary_facts();
        let source = stream.last_real_source_report();
        let mut recovered = stream.take_stopped_output().unwrap().unwrap();
        close(recovered.pending_samples(), &expected(2, 6));
        assert_eq!(
            recovered.target_frame_basis().start_time(),
            TargetTime::from_frames(2, 48_000).unwrap()
        );
        assert!(stream.take_stopped_output().unwrap().is_none());
        assert_eq!(stream.last_render_report(), Some(report));
        assert_eq!(stream.boundary_facts(), boundaries);
        assert_eq!(stream.last_real_source_report(), source);
        assert_eq!(stream.snapshot().submitted_frames, 2);
        assert_eq!(stream.frame_basis(), basis);
        assert!(stream.timing_snapshot().is_none());
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(2),
                sample: SampleId(1),
                at: Timestamp::from_nanos(1_000_000_000),
                gain: 0.5,
            })
            .unwrap();
        recovered.admit(6).unwrap();
        recovered.render_pending(2).unwrap();
        close(&recovered.pending_samples()[..1], &expected(8, 1));
        assert_eq!(recovered.mixer().counters().commands_consumed, 2);
        stream.stop().unwrap();
        assert!(stream.take_stopped_output().unwrap().is_none());
    }
}

#[test]
fn worker_panic_never_claims_retirement_or_recovers_destroyed_converted_owner() {
    let (_producer, owner, configuration) = rig(8);
    let basis = owner.target_frame_basis();
    let worker: JoinHandle<converted::ConvertedWorkerExit> = thread::spawn(move || {
        let _owned = owner;
        panic!("controlled converted memory-worker panic")
    });
    let mut stream = converted::ConvertedAlsaStream {
        configuration,
        basis,
        shared: Arc::new(Shared::new()),
        telemetry: Arc::new(converted_telemetry::ConvertedTelemetry::new()),
        worker: Some(worker),
        recovered_output: None,
        retired: false,
    };
    assert!(matches!(stream.stop(), Err(LinuxError::WorkerPanicked)));
    assert_eq!(stream.snapshot().status, AlsaStatus::WorkerPanicked);
    assert!(matches!(
        stream.take_stopped_output(),
        Err(LinuxError::InvalidLifecycle)
    ));
    assert!(stream.recovered_output.is_none());
    stream.stop().unwrap();
    assert!(matches!(
        stream.take_stopped_output(),
        Err(LinuxError::InvalidLifecycle)
    ));
}

#[test]
fn held_mode_preserves_pending_audible_tail_then_freezes_source_while_target_time_advances_without_heap(
) {
    let (_producer, mut state, mut config) = rig(8);
    state.render_pending(8).unwrap();
    state.admit(2).unwrap();
    let source_position = state.converter_owner().source_position();
    let pulled = state.mixer().frame_cursor();
    let actual_source = state.last_real_source_report();
    let telemetry = converted_telemetry::ConvertedTelemetry::new();
    telemetry.seed(state.boundaries(), actual_source);
    telemetry.set_held(true);
    let shared = Shared::new();
    let mut pcm = Pcm::new(&shared, &[], 1);
    let (result, calls) = crate::audio::count_heap_calls(|| {
        pump(
            &mut pcm,
            &mut state,
            &config,
            &Clock::new(0),
            &mut Encoder { fail: false },
            &telemetry,
        )
    });
    result.unwrap();
    assert_eq!(calls, 0);
    close(&pcm.output[..pcm.length], &expected(2, 6));
    assert_eq!(shared.renders.load(Ordering::Relaxed), 0);
    let held_shared = Shared::new();
    let mut held_pcm = Pcm::new(&held_shared, &[], 1);
    let (result, calls) = crate::audio::count_heap_calls(|| {
        pump(
            &mut held_pcm,
            &mut state,
            &config,
            &Clock::new(0),
            &mut Encoder { fail: false },
            &telemetry,
        )
    });
    result.unwrap();
    assert_eq!(calls, 0);
    assert_eq!(&held_pcm.output[..held_pcm.length], &[0.; 8]);
    assert_eq!(state.converter_owner().source_position(), source_position);
    assert_eq!(state.mixer().frame_cursor(), pulled);
    assert_eq!(
        state.target_frame_basis().start_time(),
        TargetTime::from_frames(16, 48_000).unwrap()
    );
    assert_eq!(
        telemetry.read().unwrap().0.state,
        ConvertedOutputState::Held
    );
    assert_eq!(telemetry.read().unwrap().0.source, None);
    assert_eq!(telemetry.last_real_source_report(), actual_source);
    assert_eq!(held_shared.rendered.load(Ordering::Relaxed), 8);
    assert_eq!(held_shared.submitted.load(Ordering::Relaxed), 8);
    state
        .reconfigure(
            DeviceFormat::new(32_000, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            8,
        )
        .unwrap();
    config.format = state.format();
    config.requested.format = state.format();
    telemetry.set_held(false);
    let resumed_shared = Shared::new();
    let mut resumed = Pcm::new(&resumed_shared, &[], 1);
    let (result, calls) = crate::audio::count_heap_calls(|| {
        pump(
            &mut resumed,
            &mut state,
            &config,
            &Clock::new(0),
            &mut Encoder { fail: false },
            &telemetry,
        )
    });
    result.unwrap();
    assert_eq!(calls, 0);
    for (frame, actual) in resumed.output[..resumed.length].iter().enumerate() {
        let source = 8.0 * 147.0 / 160.0 + frame as f64 * 441.0 / 320.0;
        assert!((f64::from(*actual) - (0.125 + source / 512.0)).abs() < 2e-6);
    }
    const DEN: u128 = 14_112_000;
    let ticks = 16 * (DEN / 48_000) + 8 * (DEN / 32_000);
    assert_eq!(
        state
            .target_frame_basis()
            .point_at_stream_frame(0)
            .unwrap()
            .timestamp
            .as_nanos(),
        -123 + (ticks * 1_000_000_000 / DEN) as i64
    );
}
