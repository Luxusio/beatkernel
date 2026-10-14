//! Windows-gated production ticket/start/stop/join seams with owned threads and
//! native Events, plus actual complete cold setup and Source prefill with
//! injected MMCSS/native-buffer effects. No COM client, device clock or acoustic
//! delivery is exercised. Foreign typing does not execute these tests.
use super::*;
use beatkernel::{audio::*, time::ClockDomainId};
use std::sync::atomic::AtomicBool;

fn rig(paused: bool) -> (CommandProducer, NativeOutputState, ChannelMatrix) {
    let source = AudioFormat::new(48_000, 1).unwrap();
    let limits = PcmLimits::new(512, 512, 1).unwrap();
    let mut bank = SampleBank::new(source, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(source, vec![0.25; 128], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            source,
            ClockDomainId(91),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 32, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.; 3]).unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
    let mut state = match NativeOutputState::new(mixer, format(), Some(matrix.clone()), 16) {
        Ok(state) => state,
        Err(_) => panic!("valid complete owner must prepare"),
    };
    producer.request_pause(paused);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    (producer, state, matrix)
}
fn format() -> DeviceFormat {
    DeviceFormat::new(48_000, 2, SampleEncoding::Float32, None).unwrap()
}
fn request() -> AudioStreamRequest {
    AudioStreamRequest::new(
        AudioDeviceId("never-opened-complete-fixture".into()),
        AudioBackendKind::Wasapi,
        AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
        format(),
        BufferRequest::Frames(8),
        PeriodRequest::Frames(4),
    )
    .unwrap()
}
fn primary(phase: WasapiSourcePhase, code: i32) -> WasapiSourceFailureReport {
    let mut report = WasapiSourceFailureReport::default();
    report.record_primary(phase, code);
    report
}
enum Command {
    Decide,
    Finish,
}
struct Driver {
    commands: mpsc::Sender<Command>,
    decisions: mpsc::Receiver<WasapiSourceStep>,
    dropped: Arc<AtomicBool>,
}
impl Driver {
    fn decide(&self) -> WasapiSourceStep {
        self.commands.send(Command::Decide).unwrap();
        self.decisions
            .recv_timeout(StdDuration::from_secs(5))
            .unwrap()
    }
    fn await_drop(&self) {
        let deadline = Instant::now() + StdDuration::from_secs(5);
        while !self.dropped.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "owned worker did not finish");
            thread::yield_now();
        }
    }
}
struct DropFlag(Arc<AtomicBool>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn stream(
    mut state: NativeOutputState,
    matrix: ChannelMatrix,
    report: WasapiSourceFailureReport,
    start_ack: Result<(), i32>,
    recover: bool,
    panic_on_stop: bool,
    shared_common: Option<Arc<Control>>,
) -> (WasapiCompleteStream, Driver) {
    let basis = state.output_frame_basis();
    let facts = SourceRetirementFacts::from_state(&state, Some(matrix)).unwrap();
    let common = shared_common.unwrap_or_else(|| {
        Arc::new(Control {
            request: AtomicU8::new(0),
            telemetry: complete_source_telemetry(&state).unwrap(),
            cadence: crate::audio::cadence::Capture::new(),
        })
    });
    common.request.store(0, Ordering::Release);
    let control = Arc::new(SourceControl {
        common,
        retirement: SourceRetirementSlot::new(),
        failures: FailurePublication::new(),
        nonce: AtomicU64::new(0),
    });
    let (ack_tx, started) = mpsc::sync_channel(1);
    let (commands, command_rx) = mpsc::channel();
    let (decision_tx, decisions) = mpsc::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let worker = {
        let control = Arc::clone(&control);
        let dropped = Arc::clone(&dropped);
        thread::spawn(move || {
            let _drop_flag = DropFlag(dropped);
            let mut acked = false;
            loop {
                let operation = control.common.request.load(Ordering::Acquire);
                if operation == 2 {
                    break;
                }
                if operation == 1 && !acked {
                    control.failures.publish(report);
                    if start_ack.is_ok() {
                        control
                            .common
                            .telemetry
                            .status
                            .store(status_code(AudioStreamStatus::Running), Ordering::SeqCst);
                    }
                    ack_tx.send(start_ack).unwrap();
                    acked = true;
                }
                match command_rx.try_recv() {
                    Ok(Command::Decide) => {
                        decision_tx
                            .send(control.retirement.worker_decision(&state))
                            .unwrap();
                    }
                    Ok(Command::Finish) => break,
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
                thread::yield_now();
            }
            control.failures.publish(report);
            if panic_on_stop {
                panic!("deliberate complete worker panic during cleanup");
            }
            if report.release.is_some() {
                state.mark_admission_uncertain();
            }
            CompleteWorkerResult {
                state: recover.then_some(state),
                report,
            }
        })
    };
    let configuration = AppliedStreamConfig {
        requested: request(),
        format: format(),
        buffer_frames: 8,
        buffer_duration: Duration::from_nanos(166_667),
        period_frames: Some(4),
        period_duration: Duration::from_nanos(83_334),
        stream_latency: Duration::ZERO,
        sizing_adjusted: false,
    };
    (
        WasapiCompleteStream {
            basis,
            configuration,
            options: WasapiOptions::default(),
            control,
            wake: Event::new().unwrap(),
            worker: Some(worker),
            recovered: None,
            retired: false,
            started,
            has_started: false,
            clock_frequency: 3_000_000_000,
            failure: WasapiSourceFailureReport::default(),
            facts,
        },
        Driver {
            commands,
            decisions,
            dropped,
        },
    )
}
fn ordinary(paused: bool) -> (CommandProducer, WasapiCompleteStream, Driver, ChannelMatrix) {
    let (producer, state, matrix) = rig(paused);
    let (stream, driver) = stream(
        state,
        matrix.clone(),
        WasapiSourceFailureReport::default(),
        Ok(()),
        true,
        false,
        None,
    );
    (producer, stream, driver, matrix)
}
fn submit(stream: &mut WasapiCompleteStream, matrix: &ChannelMatrix) -> SourceRetirementTicket {
    stream
        .submit_source_conditional_retirement(&request(), Some(matrix), 16)
        .unwrap()
        .unwrap()
}

#[test]
fn native_ticket_identity_checked_nonces_busy_and_unconsumed_results_use_actual_controls() {
    let (_, mut first, first_driver, matrix) = ordinary(false);
    let (_, mut other, _other_driver, _) = ordinary(false);
    let ticket = submit(&mut first, &matrix);
    assert_eq!(ticket.nonce, 1);
    assert!(ticket.control.ptr_eq(&Arc::downgrade(&first.control)));
    assert_eq!(
        other
            .poll_source_conditional_retirement(&ticket)
            .unwrap_err()
            .error,
        AudioPlatformError::InvalidRequest
    );
    assert_eq!(
        other
            .cancel_source_conditional_retirement(&ticket)
            .unwrap_err()
            .error,
        AudioPlatformError::InvalidRequest
    );
    assert_eq!(
        first.poll_source_conditional_retirement(&ticket).unwrap(),
        SourceRetirementStatus::Pending
    );
    assert!(first
        .submit_source_conditional_retirement(&request(), Some(&matrix), 16)
        .unwrap()
        .is_none());
    assert_eq!(first_driver.decide(), WasapiSourceStep::Continue);
    assert!(
        first
            .submit_source_conditional_retirement(&request(), Some(&matrix), 16)
            .unwrap()
            .is_none(),
        "unconsumed refusal cannot be overwritten"
    );
    assert_eq!(
        first.poll_source_conditional_retirement(&ticket).unwrap(),
        SourceRetirementStatus::Refused
    );
    let next = submit(&mut first, &matrix);
    assert_eq!(next.nonce, ticket.nonce + 1);
    assert_eq!(
        first
            .poll_source_conditional_retirement(&ticket)
            .unwrap_err()
            .error,
        AudioPlatformError::InvalidRequest
    );
    let zero = SourceRetirementTicket {
        control: Arc::downgrade(&first.control),
        nonce: 0,
    };
    assert_eq!(
        first
            .cancel_source_conditional_retirement(&zero)
            .unwrap_err()
            .error,
        AudioPlatformError::InvalidRequest
    );
    assert_eq!(
        first.cancel_source_conditional_retirement(&next).unwrap(),
        SourceRetirementStatus::Cancelled
    );
    assert!(first
        .submit_source_conditional_retirement(&request(), Some(&matrix), 16)
        .unwrap()
        .is_none());
    assert_eq!(
        first.poll_source_conditional_retirement(&next).unwrap(),
        SourceRetirementStatus::Cancelled
    );
    assert_eq!(submit(&mut first, &matrix).nonce, next.nonce + 1);
}

#[test]
fn dropped_control_cannot_rebind_a_ticket_even_when_legacy_common_allocation_is_reused() {
    let (_, mut original, original_driver, matrix) = ordinary(false);
    let ticket = submit(&mut original, &matrix);
    let original_address = ticket.control.as_ptr();
    let common = Arc::clone(&original.control.common);
    drop(original);
    original_driver.await_drop();
    assert!(ticket.control.upgrade().is_none());
    assert_eq!(ticket.control.strong_count(), 0);
    let (_, state, matrix) = rig(false);
    let (mut replacement, _replacement_driver) = stream(
        state,
        matrix,
        WasapiSourceFailureReport::default(),
        Ok(()),
        true,
        false,
        Some(Arc::clone(&common)),
    );
    assert!(Arc::ptr_eq(&replacement.control.common, &common));
    // Weak retains the original allocation, preventing address reuse while the
    // ticket exists. Reusing the inner legacy Control still grants no identity.
    assert_ne!(Arc::as_ptr(&replacement.control), original_address);
    assert_eq!(
        replacement
            .poll_source_conditional_retirement(&ticket)
            .unwrap_err()
            .error,
        AudioPlatformError::InvalidRequest
    );
}

#[test]
fn actual_native_ticket_authorization_is_sticky_and_distinct_from_stop_join_recovery() {
    let (_, mut stream, driver, matrix) = ordinary(true);
    let ticket = submit(&mut stream, &matrix);
    assert_eq!(driver.decide(), WasapiSourceStep::Retire);
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket).unwrap(),
        SourceRetirementStatus::Authorized
    );
    assert_eq!(
        stream
            .cancel_source_conditional_retirement(&ticket)
            .unwrap(),
        SourceRetirementStatus::Authorized
    );
    assert!(stream.worker.is_some());
    assert!(!stream.retired);
    assert!(stream.recovered.is_none());
    assert!(stream
        .submit_source_conditional_retirement(&request(), Some(&matrix), 16)
        .unwrap()
        .is_none());
    assert_eq!(
        stream.start().unwrap_err().error,
        AudioPlatformError::WorkerFailure
    );
    assert_eq!(stream.control.common.request.load(Ordering::Acquire), 0);
    stream.stop().unwrap();
    assert!(driver.dropped.load(Ordering::Acquire));
    assert!(stream.worker.is_none());
    assert!(stream.retired);
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket).unwrap(),
        SourceRetirementStatus::Authorized
    );
    let state = stream.take_stopped_output_state().unwrap().unwrap();
    assert_eq!(state.pending_samples(), [0.; 6]);
    assert_eq!(state.admitted_frames(), 1);
    assert_eq!(
        (
            state.output_frame_basis().start_physical_frame(),
            state.mixer().frame_cursor()
        ),
        (4, 7)
    );
    assert!(state.paused_tail_admissible());
}

#[test]
fn pending_ticket_does_not_conceal_finished_worker_or_genuine_native_failure() {
    let (_, mut stream, driver, matrix) = ordinary(false);
    let ticket = submit(&mut stream, &matrix);
    stream.control.common.telemetry.status.store(
        status_code(AudioStreamStatus::Failed { hresult: -71 }),
        Ordering::SeqCst,
    );
    assert_eq!(
        stream
            .poll_source_conditional_retirement(&ticket)
            .unwrap_err()
            .error,
        native(-71)
    );
    stream
        .control
        .common
        .telemetry
        .status
        .store(status_code(AudioStreamStatus::Ready), Ordering::SeqCst);
    driver.commands.send(Command::Finish).unwrap();
    driver.await_drop();
    let deadline = Instant::now() + StdDuration::from_secs(5);
    while !stream.worker.as_ref().unwrap().is_finished() {
        assert!(Instant::now() < deadline, "owned worker did not terminate");
        thread::yield_now();
    }
    assert_eq!(
        stream
            .poll_source_conditional_retirement(&ticket)
            .unwrap_err()
            .error,
        AudioPlatformError::WorkerFailure
    );
    assert_eq!(
        stream.control.retirement.status(ticket.nonce).unwrap(),
        crate::audio::wasapi_source_worker::SourceRetirementStatus::Pending
    );
    stream.stop().unwrap();
    assert!(stream.take_stopped_output_state().unwrap().is_some());
}

#[test]
fn supplied_created_counter_metadata_and_ready_history_do_not_grant_progress() {
    let (_, stream, _driver, _) = ordinary(false);
    assert_eq!(stream.clock_frequency(), 3_000_000_000);
    assert_ne!(
        stream.clock_frequency(),
        u64::from(stream.frame_basis().sample_rate())
    );
    assert_eq!(stream.frame_basis().start_physical_frame(), 4);
    let query =
        InterpolatedTargetTime::new(TargetTime::from_frames(6, 48_000).unwrap(), 0, 0, 1, 0, 1)
            .unwrap();
    let source = stream.source_playback_at(&query, 48_000, 3, 3).unwrap();
    assert_eq!(source.pulled_source_end, 7);
    assert_eq!(source.snapshot.status, AudioStreamStatus::Ready);
    assert_eq!(source.snapshot.counters.submitted_frames, 0);
    assert!(source.snapshot.clock.is_none());
    assert!(source.snapshot.render.is_none());
    assert!(stream.source_playback_at(&query, 48_000, 4, 3).is_none());
}

#[test]
fn complete_successful_start_stop_retains_original_nonzero_pcm_program_and_producer() {
    let (mut producer, mut stream, driver, _) = ordinary(false);
    stream.start().unwrap();
    assert!(stream.has_started);
    assert_eq!(stream.snapshot().status, AudioStreamStatus::Running);
    stream.start().unwrap();
    assert!(stream.take_stopped_output_state().is_err());
    stream.stop().unwrap();
    assert!(driver.dropped.load(Ordering::Acquire));
    let mut state = stream.take_stopped_output_state().unwrap().unwrap();
    assert_eq!(
        state.pending_samples(),
        [0.25, -0.125, 0.25, -0.125, 0.25, -0.125]
    );
    assert_eq!(state.admitted_frames(), 1);
    assert_eq!(state.pending_report().unwrap().start_frame, 3);
    assert_eq!(state.output_frame_basis().start_physical_frame(), 4);
    assert_eq!(state.mixer().frame_cursor(), 7);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(
        (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame()
        ),
        (48_000, 3, 3)
    );
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 0.5,
        })
        .unwrap();
    state.admit(3).unwrap();
    state.render_pending(2).unwrap();
    assert_eq!(state.pending_samples(), [0.375, -0.1875, 0.375, -0.1875]);
    assert_eq!(state.mixer().counters().commands_consumed, 2);
    assert!(stream.take_stopped_output_state().unwrap().is_none());
}

#[test]
fn complete_open_cold_refusal_returns_exact_owner_before_any_com_acquisition() {
    let requests = [
        AudioStreamRequest::new(
            AudioDeviceId("never-opened".into()),
            AudioBackendKind::Asio,
            AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
            format(),
            BufferRequest::DeviceDefault,
            PeriodRequest::DeviceDefault,
        )
        .unwrap(),
        AudioStreamRequest::new(
            AudioDeviceId("never-opened".into()),
            AudioBackendKind::Wasapi,
            AudioStreamMode::Exclusive,
            format(),
            BufferRequest::DeviceDefault,
            PeriodRequest::DeviceDefault,
        )
        .unwrap(),
        AudioStreamRequest::new(
            AudioDeviceId("never-opened".into()),
            AudioBackendKind::Wasapi,
            AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
            DeviceFormat::new(44_100, 2, SampleEncoding::Float32, None).unwrap(),
            BufferRequest::DeviceDefault,
            PeriodRequest::DeviceDefault,
        )
        .unwrap(),
    ];
    for requested in requests {
        let (mut producer, state, matrix) = rig(false);
        let pending = state.pending_samples().to_vec();
        let report = state.pending_report();
        let counters = state.mixer().counters();
        let basis = state.output_frame_basis();
        let clock = QpcClock::new(ClockDomainId(92)).unwrap();
        let failure = match WasapiBackend.open_complete_recoverable(
            requested,
            state,
            clock,
            WasapiOptions::default(),
            Some(matrix),
            4,
        ) {
            Err(failure) => failure,
            Ok(_) => panic!("unsupported retained request must refuse cold"),
        };
        assert!(matches!(
            failure.error().error,
            AudioPlatformError::InvalidRequest | AudioPlatformError::InvalidFormat
        ));
        assert_eq!(
            failure.error().report,
            WasapiCompleteFailureReport::default()
        );
        let (_, state) = failure.into_parts();
        let mut state = state.unwrap();
        assert!(!state.admission_is_uncertain());
        assert_eq!(state.pending_samples(), pending);
        assert_eq!(state.pending_report(), report);
        assert_eq!(state.output_frame_basis(), basis);
        assert_eq!(state.admitted_frames(), 1);
        assert_eq!(state.max_frames(), 16);
        assert_eq!(state.mixer().counters(), counters);
        assert_eq!(
            state
                .mixer()
                .source_playback_history()
                .unwrap()
                .initial_physical_frame(),
            3
        );
        producer.request_pause(true);
        state.admit(3).unwrap();
        assert!(state.render_pending(1).unwrap().paused);
        assert_eq!(state.pending_samples(), [0., 0.]);
    }
}

#[test]
fn complete_open_rejects_known_duration_and_legacy_period_errors_before_native_acquisition() {
    let fractional_buffer = AudioPlatformError::ConfigurationUnsupported {
        constraint: ConfigurationConstraint::EngineManagedBuffer,
        constraints: PeriodConstraints::default(),
        suggested_buffer_frames: Some(1),
        suggested_period_frames: None,
    };
    let fractional_period = AudioPlatformError::ConfigurationUnsupported {
        constraint: ConfigurationConstraint::PeriodBounds,
        constraints: PeriodConstraints::default(),
        suggested_buffer_frames: None,
        suggested_period_frames: Some(1),
    };
    let legacy_period = AudioPlatformError::ConfigurationUnsupported {
        constraint: ConfigurationConstraint::LegacyDeviceDefaultPeriod,
        constraints: PeriodConstraints::default(),
        suggested_buffer_frames: None,
        suggested_period_frames: None,
    };
    for (mode, buffer, period, expected) in [
        (
            SharedPeriodPolicy::EnginePeriod,
            BufferRequest::Duration(Duration::from_nanos(1)),
            PeriodRequest::Frames(4),
            fractional_buffer,
        ),
        (
            SharedPeriodPolicy::EnginePeriod,
            BufferRequest::Frames(8),
            PeriodRequest::Duration(Duration::from_nanos(1)),
            fractional_period,
        ),
        (
            SharedPeriodPolicy::EnginePeriod,
            BufferRequest::Duration(Duration::from_nanos(1_000_000)),
            PeriodRequest::Frames(4),
            AudioPlatformError::Capacity,
        ),
        (
            SharedPeriodPolicy::EnginePeriod,
            BufferRequest::Frames(8),
            PeriodRequest::Duration(Duration::from_nanos(1_000_000)),
            AudioPlatformError::Capacity,
        ),
        (
            SharedPeriodPolicy::EnginePeriod,
            BufferRequest::Duration(Duration::MAX),
            PeriodRequest::Frames(4),
            AudioPlatformError::Capacity,
        ),
        (
            SharedPeriodPolicy::EnginePeriod,
            BufferRequest::Frames(8),
            PeriodRequest::Duration(Duration::MAX),
            AudioPlatformError::Capacity,
        ),
        (
            SharedPeriodPolicy::DeviceDefault,
            BufferRequest::Frames(8),
            PeriodRequest::Frames(4),
            legacy_period,
        ),
    ] {
        let requested = AudioStreamRequest::new(
            AudioDeviceId("never-opened-complete-sizing-fixture".into()),
            AudioBackendKind::Wasapi,
            AudioStreamMode::Shared(mode),
            format(),
            buffer,
            period,
        )
        .unwrap();
        let (mut producer, state, matrix) = rig(false);
        let before = CompleteOwnerSnapshot::of(&state);
        let failure = match WasapiBackend.open_complete_recoverable(
            requested,
            state,
            QpcClock::new(ClockDomainId(92)).unwrap(),
            WasapiOptions::default(),
            Some(matrix),
            4,
        ) {
            Err(failure) => failure,
            Ok(_) => panic!("known invalid sizing must refuse before native acquisition"),
        };
        assert_eq!(failure.error().error, expected);
        assert_eq!(
            failure.error().report,
            WasapiCompleteFailureReport::default()
        );
        let (_, recovered) = failure.into_parts();
        let mut recovered = recovered.expect("cold refusal must return the original owner");
        assert_eq!(CompleteOwnerSnapshot::of(&recovered), before);
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(2),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 0.5,
            })
            .unwrap();
        recovered.admit(3).unwrap();
        recovered.render_pending(2).unwrap();
        assert_eq!(
            recovered.pending_samples(),
            [0.375, -0.1875, 0.375, -0.1875]
        );
        assert_eq!(recovered.mixer().counters().commands_consumed, 2);
    }
}

#[derive(Debug, PartialEq)]
struct CompleteOwnerSnapshot {
    format: Option<DeviceFormat>,
    configuration: MixerConfig,
    pending: Vec<f32>,
    pending_report: Option<RenderReport>,
    basis: OutputFrameBasis,
    admitted: usize,
    max_frames: usize,
    cursor: u64,
    counters: AudioCounters,
    history: (u32, u64, u64),
    history_capacity: usize,
    history_end: u64,
    history_revision: u64,
    history_spans: Vec<SourcePlaybackSpan>,
    uncertain: bool,
}
impl CompleteOwnerSnapshot {
    fn of(state: &NativeOutputState) -> Self {
        let history = state.mixer().source_playback_history().unwrap();
        Self {
            format: state.format(),
            configuration: state.mixer().config(),
            pending: state.pending_samples().to_vec(),
            pending_report: state.pending_report(),
            basis: state.output_frame_basis(),
            admitted: state.admitted_frames(),
            max_frames: state.max_frames(),
            cursor: state.mixer().frame_cursor(),
            counters: state.mixer().counters(),
            history: (
                history.source_rate(),
                history.initial_physical_frame(),
                history.initial_playback_frame(),
            ),
            history_capacity: history.capacity(),
            history_end: history.pulled_end_frame(),
            history_revision: history.retirement_revision(),
            history_spans: (0..history.len())
                .map(|index| history.span(index).unwrap())
                .collect(),
            uncertain: state.admission_is_uncertain(),
        }
    }
}

#[derive(Debug, PartialEq)]
enum MmcssCall {
    Register,
    Priority(usize, WasapiPriority),
    Revert(usize),
}
#[derive(Default)]
struct SetupMmcssOps {
    calls: Vec<MmcssCall>,
    registration_error: Option<i32>,
    priority_error: Option<i32>,
    revert_error: Option<i32>,
}
impl MmcssOps for SetupMmcssOps {
    fn register(&mut self) -> Result<HANDLE, i32> {
        self.calls.push(MmcssCall::Register);
        self.registration_error
            .map_or(Ok(HANDLE(0x71usize as *mut std::ffi::c_void)), Err)
    }
    fn set_priority(&mut self, handle: HANDLE, priority: WasapiPriority) -> Result<(), i32> {
        self.calls
            .push(MmcssCall::Priority(handle.0 as usize, priority));
        self.priority_error.map_or(Ok(()), Err)
    }
    fn revert(&mut self, handle: HANDLE) -> Result<(), i32> {
        self.calls.push(MmcssCall::Revert(handle.0 as usize));
        self.revert_error.map_or(Ok(()), Err)
    }
}

// Always consume a returned fake registration through the supplied operations
// before any assertion. Even an unexpectedly successful result cannot send a
// fake HANDLE into Mmcss's native Drop path. This also proves repeated cleanup
// cannot revert a registration twice, including when the first revert fails.
fn finish_injected_setup<T>(
    result: Result<(T, Option<Mmcss>), AudioPlatformError>,
    report: &mut WasapiSourceFailureReport,
    ops: &mut SetupMmcssOps,
) -> Result<T, AudioPlatformError> {
    result.map(|(prepared, mut registration)| {
        retire_setup_mmcss(&mut registration, report, ops);
        retire_setup_mmcss(&mut registration, report, ops);
        prepared
    })
}

fn prepared_setup_rig() -> (CommandProducer, NativeOutputState, ChannelMatrix) {
    let (producer, mut state, matrix) = rig(false);
    // Match the actual complete constructor: software preparation and
    // telemetry allocation finish before registering the worker with MMCSS.
    state
        .validate_reconfigure(format(), Some(&matrix), 8)
        .unwrap();
    state.prepare_native_source_history(8, 4).unwrap();
    state
        .reconfigure(format(), Some(matrix.clone()), 8)
        .unwrap();
    complete_source_telemetry(&state).unwrap();
    (producer, state, matrix)
}

#[test]
fn complete_setup_registration_and_priority_failures_preserve_owner_and_cleanup_once() {
    for (registration_error, priority_error, revert_error, expected, expected_calls) in [
        (Some(-201), None, None, -201, vec![MmcssCall::Register]),
        (
            None,
            Some(-202),
            None,
            -202,
            vec![
                MmcssCall::Register,
                MmcssCall::Priority(0x71, WasapiPriority::High),
                MmcssCall::Revert(0x71),
            ],
        ),
        (
            None,
            Some(-202),
            Some(-203),
            -202,
            vec![
                MmcssCall::Register,
                MmcssCall::Priority(0x71, WasapiPriority::High),
                MmcssCall::Revert(0x71),
            ],
        ),
    ] {
        let (_, state, _) = prepared_setup_rig();
        let before = CompleteOwnerSnapshot::of(&state);
        let mut called_setup = false;
        let mut report = WasapiSourceFailureReport::default();
        let mut ops = SetupMmcssOps {
            registration_error,
            priority_error,
            revert_error,
            ..Default::default()
        };
        let result =
            complete_mmcss_setup(Some(WasapiPriority::High), &mut report, &mut ops, |_| {
                called_setup = true;
                Ok(())
            });
        let result = finish_injected_setup(result, &mut report, &mut ops);
        assert_eq!(result, Err(native(expected)));
        assert!(!called_setup);
        assert_eq!(ops.calls, expected_calls);
        assert_eq!(
            report,
            WasapiSourceFailureReport {
                recovery: revert_error,
                ..primary(WasapiSourcePhase::Recovery, expected)
            }
        );
        assert_eq!(CompleteOwnerSnapshot::of(&state), before);
    }
}

#[test]
fn complete_setup_cleanup_keeps_every_existing_primary_and_cleanup_failure() {
    let (_, state, _) = prepared_setup_rig();
    let before = CompleteOwnerSnapshot::of(&state);
    let mut original = primary(WasapiSourcePhase::Start, -231);
    original.release = Some(0);
    original.stop = Some(-232);
    original.start_cleanup = Some(-233);
    original.join = Some(-234);
    original.recovery = Some(-235);
    let mut report = original;
    let mut ops = SetupMmcssOps {
        priority_error: Some(-237),
        revert_error: Some(-236),
        ..Default::default()
    };
    let mut called_setup = false;
    let result = complete_mmcss_setup(Some(WasapiPriority::High), &mut report, &mut ops, |_| {
        called_setup = true;
        Ok(())
    });
    let result = finish_injected_setup(result, &mut report, &mut ops);
    assert_eq!(result, Err(native(-237)));
    assert!(!called_setup);
    assert_eq!(report, original);
    assert_eq!(
        ops.calls,
        [
            MmcssCall::Register,
            MmcssCall::Priority(0x71, WasapiPriority::High),
            MmcssCall::Revert(0x71),
        ]
    );
    assert_eq!(CompleteOwnerSnapshot::of(&state), before);
}

#[derive(Default)]
struct SetupPrefillOps {
    encode_error: Option<i32>,
    release_error: Option<i32>,
    acquired: usize,
    encoded: Vec<f32>,
    releases: Vec<(u32, bool)>,
    clocks: usize,
}
impl WasapiSourceOps for SetupPrefillOps {
    type Lease = u32;
    fn available_frames(&mut self, _: u32) -> Result<u32, i32> {
        Ok(2)
    }
    fn acquire(&mut self, frames: u32) -> Result<u32, i32> {
        self.acquired += 1;
        Ok(frames)
    }
    fn encode(&mut self, _: &mut u32, _: DeviceFormat, samples: &[f32]) -> Result<(), i32> {
        self.encoded.extend_from_slice(samples);
        self.encode_error.map_or(Ok(()), Err)
    }
    fn release(&mut self, lease: u32, silent: bool) -> Result<(), i32> {
        self.releases.push((lease, silent));
        self.release_error.map_or(Ok(()), Err)
    }
    fn read_clock(&mut self) -> Result<(), i32> {
        self.clocks += 1;
        Ok(())
    }
}

#[test]
fn complete_setup_actual_source_prefill_keeps_primary_release_recovery_and_original_owner() {
    for (encode_error, release_error, revert_error, expected_phase, expected_code) in [
        (
            Some(-221),
            None,
            Some(-223),
            WasapiSourcePhase::Encode,
            -221,
        ),
        (
            None,
            Some(-222),
            Some(-223),
            WasapiSourcePhase::Release,
            -222,
        ),
        (
            Some(-221),
            Some(-222),
            Some(-223),
            WasapiSourcePhase::Encode,
            -221,
        ),
        (
            Some(-221),
            Some(-222),
            None,
            WasapiSourcePhase::Encode,
            -221,
        ),
    ] {
        let (mut producer, mut state, _) = prepared_setup_rig();
        let mut expected_owner = CompleteOwnerSnapshot::of(&state);
        expected_owner.uncertain = release_error.is_some();
        let mut report = WasapiSourceFailureReport::default();
        let mut ops = SetupMmcssOps {
            revert_error,
            ..Default::default()
        };
        let mut prefill = SetupPrefillOps {
            encode_error,
            release_error,
            ..Default::default()
        };
        let result = complete_mmcss_setup(
            Some(WasapiPriority::Normal),
            &mut report,
            &mut ops,
            |setup_report| match source_worker_step(
                &mut state,
                &SourceRetirementSlot::new(),
                &mut prefill,
                WasapiSourceWork::Fill { buffer_frames: 8 },
                &mut 0,
            ) {
                WasapiSourceStep::Failure(failure) => {
                    *setup_report = failure;
                    Err(native(failure.primary.unwrap().code))
                }
                WasapiSourceStep::Continue | WasapiSourceStep::Retire => Ok(()),
            },
        );
        let result = finish_injected_setup(result, &mut report, &mut ops);
        assert_eq!(result, Err(native(expected_code)));
        let mut expected_report = primary(expected_phase, expected_code);
        expected_report.release = release_error;
        expected_report.recovery = revert_error;
        assert_eq!(report, expected_report);
        assert_eq!(
            WasapiCompleteError::new(native(expected_code), report).report,
            WasapiCompleteFailureReport::from(expected_report)
        );
        assert_eq!(
            ops.calls,
            [
                MmcssCall::Register,
                MmcssCall::Priority(0x71, WasapiPriority::Normal),
                MmcssCall::Revert(0x71),
            ]
        );
        assert_eq!(prefill.acquired, 1);
        assert_eq!(prefill.encoded, [0.25, -0.125, 0.25, -0.125]);
        assert_eq!(prefill.releases, [(2, encode_error.is_some())]);
        assert_eq!(prefill.clocks, 0);
        assert_eq!(CompleteOwnerSnapshot::of(&state), expected_owner);
        producer.request_pause(true);
        if release_error.is_some() {
            assert_eq!(state.render_pending(1), Err(AudioError::InvalidBuffer));
        } else {
            state.admit(3).unwrap();
            assert!(state.render_pending(1).unwrap().paused);
        }
    }
}

#[test]
fn complete_setup_success_preserves_actual_admission_and_optional_mmcss_cleanup_once() {
    for (priority, revert_error) in [
        (None, None),
        (Some(WasapiPriority::Low), None),
        (Some(WasapiPriority::Low), Some(-241)),
    ] {
        let (_, mut state, _) = prepared_setup_rig();
        let before = CompleteOwnerSnapshot::of(&state);
        let mut report = WasapiSourceFailureReport::default();
        let mut ops = SetupMmcssOps {
            revert_error,
            ..Default::default()
        };
        let mut prefill = SetupPrefillOps::default();
        let result =
            complete_mmcss_setup(
                priority,
                &mut report,
                &mut ops,
                |_| match source_worker_step(
                    &mut state,
                    &SourceRetirementSlot::new(),
                    &mut prefill,
                    WasapiSourceWork::Fill { buffer_frames: 8 },
                    &mut 0,
                ) {
                    WasapiSourceStep::Continue => Ok(()),
                    WasapiSourceStep::Failure(failure) => {
                        Err(native(failure.primary.unwrap().code))
                    }
                    WasapiSourceStep::Retire => Err(AudioPlatformError::WorkerFailure),
                },
            );
        let result = finish_injected_setup(result, &mut report, &mut ops);
        assert_eq!(result, Ok(()));
        assert_eq!(
            report,
            WasapiSourceFailureReport {
                recovery: revert_error,
                ..Default::default()
            }
        );
        assert_eq!(
            ops.calls,
            if priority.is_some() {
                vec![
                    MmcssCall::Register,
                    MmcssCall::Priority(0x71, WasapiPriority::Low),
                    MmcssCall::Revert(0x71),
                ]
            } else {
                vec![]
            }
        );
        assert_eq!(prefill.acquired, 1);
        assert_eq!(prefill.releases, [(2, false)]);
        assert_eq!(prefill.clocks, 1);
        assert_eq!(state.pending_samples(), [0.25, -0.125]);
        assert_eq!(state.admitted_frames(), before.admitted + 2);
        assert_eq!(state.pending_report(), before.pending_report);
        assert_eq!(state.mixer().counters(), before.counters);
        assert_eq!(state.mixer().frame_cursor(), before.cursor);
        assert!(!state.admission_is_uncertain());
    }
}

#[test]
fn native_stop_keeps_primary_release_and_stop_errors_with_complete_quarantined_owner() {
    let (mut producer, state, matrix) = rig(false);
    let pending = state.pending_samples().to_vec();
    let report = state.pending_report();
    let basis = state.output_frame_basis();
    let counters = state.mixer().counters();
    let mut failures = primary(WasapiSourcePhase::Encode, -81);
    failures.release = Some(-82);
    failures.stop = Some(-83);
    let (mut stream, driver) = stream(state, matrix.clone(), failures, Ok(()), true, false, None);
    let failure = stream.stop().unwrap_err();
    assert!(driver.dropped.load(Ordering::Acquire));
    assert_eq!(failure.error, native(-81));
    assert_eq!(failure.report, WasapiCompleteFailureReport::from(failures));
    let mut state = stream.take_stopped_output_state().unwrap().unwrap();
    assert!(state.admission_is_uncertain());
    assert_eq!(state.pending_samples(), pending);
    assert_eq!(state.pending_report(), report);
    assert_eq!(state.admitted_frames(), 1);
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(state.mixer().counters(), counters);
    assert_eq!(
        state.reconfigure(format(), Some(matrix.clone()), 8),
        Err(AudioError::InvalidBuffer)
    );
    producer.request_pause(true);
    let clock = QpcClock::new(ClockDomainId(92)).unwrap();
    let refusal = match WasapiBackend.open_complete_recoverable(
        request(),
        state,
        clock,
        WasapiOptions::default(),
        Some(matrix),
        4,
    ) {
        Err(failure) => failure,
        Ok(_) => panic!("explicit Retry of uncertain admission must refuse before COM"),
    };
    assert_eq!(refusal.error().error, AudioPlatformError::InvalidRequest);
    assert_eq!(
        refusal.error().report.primary.unwrap().phase,
        WasapiCompletePhase::Quarantined
    );
    let (_, state) = refusal.into_parts();
    let state = state.unwrap();
    assert!(state.admission_is_uncertain());
    assert_eq!(state.pending_samples(), pending);
    assert_eq!(state.pending_report(), report);
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(state.mixer().counters(), counters);
}

#[test]
fn complete_start_failure_joins_before_exposing_original_and_cleanup_failures() {
    let (_, state, matrix) = rig(false);
    let mut failures = primary(WasapiSourcePhase::Start, -91);
    failures.stop = Some(-92);
    failures.start_cleanup = Some(-93);
    let (mut stream, driver) = stream(state, matrix, failures, Err(-91), true, false, None);
    let error = stream.start().unwrap_err();
    assert_eq!(error.error, native(-91));
    assert_eq!(error.report, WasapiCompleteFailureReport::from(failures));
    assert!(driver.dropped.load(Ordering::Acquire));
    assert!(stream.worker.is_none());
    assert_eq!(stream.control.common.request.load(Ordering::Acquire), 2);
    assert!(stream.take_stopped_output_state().unwrap().is_some());
}

#[test]
fn complete_start_failure_retains_prior_cleanup_plus_join_and_recovery_loss() {
    let (_, state, matrix) = rig(false);
    let mut failures = primary(WasapiSourcePhase::Start, -101);
    failures.start_cleanup = Some(-102);
    let (mut stream, driver) = stream(state, matrix, failures, Err(-101), true, true, None);
    let error = stream.start().unwrap_err();
    assert!(driver.dropped.load(Ordering::Acquire));
    assert_eq!(error.error, native(-101));
    assert_eq!(
        error.report.primary,
        Some(WasapiCompleteFailure {
            phase: WasapiCompletePhase::Start,
            code: -101
        })
    );
    assert_eq!(error.report.start_cleanup, Some(-102));
    assert_eq!(error.report.join, Some(E_FAIL_CODE));
    assert_eq!(error.report.recovery, Some(E_FAIL_CODE));
    assert_eq!(stream.snapshot().status, AudioStreamStatus::WorkerPanicked);
    let recovery = match stream.take_stopped_output_state() {
        Err(error) => error,
        Ok(_) => panic!("panicked worker cannot recover its owner"),
    };
    assert_eq!(recovery.error, AudioPlatformError::RecoveryUnavailable);
    assert_eq!(recovery.report, error.report);
}

#[test]
fn joined_worker_without_owner_reports_recovery_failure_without_hiding_it_as_success() {
    let (_, state, matrix) = rig(false);
    let (mut stream, driver) = stream(
        state,
        matrix,
        WasapiSourceFailureReport::default(),
        Ok(()),
        false,
        false,
        None,
    );
    let error = stream.stop().unwrap_err();
    assert!(driver.dropped.load(Ordering::Acquire));
    assert_eq!(
        error.report.primary,
        Some(WasapiCompleteFailure {
            phase: WasapiCompletePhase::Recovery,
            code: E_FAIL_CODE
        })
    );
    assert_eq!(error.report.recovery, Some(E_FAIL_CODE));
    assert!(stream.take_stopped_output_state().unwrap().is_none());
    assert_eq!(stream.failure_report(), error.report);
}

#[test]
fn native_failure_publication_keeps_first_phase_and_each_cleanup_code_including_zero() {
    for phase in [
        WasapiSourcePhase::Quarantined,
        WasapiSourcePhase::Padding,
        WasapiSourcePhase::Acquire,
        WasapiSourcePhase::History,
        WasapiSourcePhase::Render,
        WasapiSourcePhase::Encode,
        WasapiSourcePhase::Release,
        WasapiSourcePhase::Admission,
        WasapiSourcePhase::Clock,
        WasapiSourcePhase::Start,
        WasapiSourcePhase::Stop,
        WasapiSourcePhase::Join,
        WasapiSourcePhase::Recovery,
    ] {
        let publication = FailurePublication::new();
        let mut original = primary(phase, i32::MIN);
        original.release = Some(0);
        original.stop = Some(-111);
        original.start_cleanup = Some(-112);
        original.join = Some(-113);
        original.recovery = Some(-114);
        publication.publish(original);
        let mut later = primary(WasapiSourcePhase::Recovery, -121);
        later.release = Some(-122);
        later.stop = Some(-123);
        later.start_cleanup = Some(-124);
        later.join = Some(-125);
        later.recovery = Some(-126);
        publication.publish(later);
        assert_eq!(publication.read(), original);
        assert_eq!(
            WasapiCompleteFailureReport::from(publication.read())
                .primary
                .unwrap()
                .phase,
            WasapiCompletePhase::from(phase)
        );
    }
}
