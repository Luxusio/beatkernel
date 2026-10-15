//! Actual production complete-state lifecycle with memory-backed native effects.
//! These SDK-gated fixtures require Windows typing; they are not driver proof.
use super::super::control::{AsioChannelInfo, AsioControlErrorDomain, CloseOutcome};
use super::*;
use crate::audio::asio::{AsioBufferConstraints, AsioPcmEncoding};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioLimits, CommandProducer, MixerConfig, PcmLimits,
        PcmSample, SampleBank, SampleId, TargetTime, VoiceId,
    },
    time::{ClockDomainId, Timestamp},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColdFailure {
    Rate,
    Buffers,
    Channel0,
    Channel1,
    Prepare,
    Latencies,
}

// Only primitive native effects are replaced. Prepare staging, unique-owner
// recovery, B priming, actual Start order, quarantine and extraction are real.
struct FaultOps {
    planes: [Box<[u8]>; 4],
    events: Vec<&'static str>,
    fail: Option<ColdFailure>,
    rate: f64,
    synchronous_a: bool,
    start_code: i32,
    diagnostics_calls: usize,
    fail_diagnostics: Option<usize>,
    reported_native: Option<AsioDiagnostics>,
    close_retired: bool,
    close_code: i32,
    close_calls: usize,
    closed: bool,
    start_b: Option<[Vec<u8>; 2]>,
}
fn native(operation: &'static str, code: i32) -> AsioControlError {
    AsioControlError::Native {
        operation,
        domain: AsioControlErrorDomain::Asio,
        code,
    }
}
impl FaultOps {
    fn new() -> Self {
        Self {
            planes: std::array::from_fn(|index| {
                vec![0xa5; if index < 2 { 4 } else { 8 }].into_boxed_slice()
            }),
            events: Vec::new(),
            fail: None,
            rate: 1000.,
            synchronous_a: false,
            start_code: 0,
            diagnostics_calls: 0,
            fail_diagnostics: None,
            reported_native: None,
            close_retired: true,
            close_code: 0,
            close_calls: 0,
            closed: false,
            start_b: None,
        }
    }
    fn cold(&mut self, at: ColdFailure, operation: &'static str) -> Result<(), AsioControlError> {
        self.events.push(operation);
        if self.fail == Some(at) {
            Err(native(operation, -71))
        } else {
            Ok(())
        }
    }
}
impl NativeLifecycleOps for FaultOps {
    fn diagnostics(
        &mut self,
        _: Option<&AsioControl>,
        cached: AsioDiagnostics,
    ) -> Result<AsioDiagnostics, AsioControlError> {
        self.events.push("diagnostics");
        self.diagnostics_calls += 1;
        if self.fail_diagnostics == Some(self.diagnostics_calls) {
            Err(native("diagnostics", -73))
        } else {
            Ok(self.reported_native.unwrap_or(cached))
        }
    }
    fn start(
        &mut self,
        _: Option<&AsioControl>,
        context: &RenderContext,
    ) -> Result<(), AsioControlError> {
        self.events.push("start");
        self.start_b = Some([self.planes[1].to_vec(), self.planes[3].to_vec()]);
        if self.synchronous_a {
            // SAFETY: this synchronous Start owns the only callback admission;
            // all referenced plane boxes are disjoint and live through return.
            unsafe {
                assert_eq!(context.preflight(0), 0);
                context.fill(0, Some(event(0))).unwrap();
            }
        }
        if self.start_code == 0 {
            Ok(())
        } else {
            Err(native("start", self.start_code))
        }
    }
    fn close(&mut self, control: &mut Option<AsioControl>) -> Option<CloseOutcome> {
        assert!(
            control.is_none(),
            "fixture never constructs a native control"
        );
        if self.closed {
            return None;
        }
        self.closed = true;
        self.events.push("close");
        self.close_calls += 1;
        Some(CloseOutcome {
            retired: self.close_retired,
            error: (self.close_code != 0).then(|| native("close", self.close_code)),
        })
    }
}
impl NativePrepareOps for FaultOps {
    fn sample_rate(&mut self, _: Option<&mut AsioControl>) -> Result<f64, AsioControlError> {
        self.cold(ColdFailure::Rate, "rate")?;
        Ok(self.rate)
    }
    fn buffer_constraints(
        &mut self,
        _: Option<&mut AsioControl>,
    ) -> Result<AsioBufferConstraints, AsioControlError> {
        self.cold(ColdFailure::Buffers, "buffers")?;
        Ok(AsioBufferConstraints::from_raw(2, 8, 2, 0).unwrap())
    }
    fn channel_info(
        &mut self,
        _: Option<&mut AsioControl>,
        channel: u32,
    ) -> Result<AsioChannelInfo, AsioControlError> {
        self.cold(
            if channel == 0 {
                ColdFailure::Channel0
            } else {
                ColdFailure::Channel1
            },
            if channel == 0 { "channel0" } else { "channel1" },
        )?;
        // SAFETY: every field is an integer, bool or byte array, so all-zero is
        // valid. Private name storage remains empty; no foreign layout is used.
        let mut info: AsioChannelInfo = unsafe { std::mem::zeroed() };
        info.channel = channel;
        info.sample_type = if channel == 0 { 16 } else { 3 };
        Ok(info)
    }
    fn latencies(
        &mut self,
        _: Option<&mut AsioControl>,
    ) -> Result<AsioLatencies, AsioControlError> {
        self.cold(ColdFailure::Latencies, "latencies")?;
        Ok(AsioLatencies {
            input_frames: 1,
            output_frames: 3,
        })
    }
    fn prepare(
        &mut self,
        _: Option<&AsioControl>,
        rows: &mut [RawOutput],
        frames: u32,
        rate: f64,
        context: *mut c_void,
    ) -> Result<(), AsioControlError> {
        self.cold(ColdFailure::Prepare, "prepare")?;
        assert_eq!((rows.len(), frames, rate), (2, 2, 1000.));
        assert!(!context.is_null());
        assert_eq!(
            (rows[0].channel, rows[0].width, rows[0].sample_type),
            (0, 2, 16)
        );
        assert_eq!(
            (rows[1].channel, rows[1].width, rows[1].sample_type),
            (1, 4, 3)
        );
        rows[0].buffer0 = self.planes[0].as_mut_ptr().cast();
        rows[0].buffer1 = self.planes[1].as_mut_ptr().cast();
        rows[1].buffer0 = self.planes[2].as_mut_ptr().cast();
        rows[1].buffer1 = self.planes[3].as_mut_ptr().cast();
        // Actual native prepare clears both halves without generating Source.
        for plane in &mut self.planes {
            plane.fill(0);
        }
        Ok(())
    }
}

fn owner() -> (CommandProducer, NativeOutputState) {
    let format = AudioFormat::new(1000, 2).unwrap();
    let limits = PcmLimits::new(128, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    let samples = (0..32)
        .flat_map(|frame| {
            let left = (frame % 8 + 1) as f32 / 16.;
            [left, -left / 2.]
        })
        .collect();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, samples, limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(123_000),
            gain: 1.,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::from_nanos(123_000),
            AudioLimits::new(16, 2, 16, 32, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(16).unwrap();
    let mut state = NativeOutputState::from_mixer(mixer);
    let layout = AsioPlanarLayout::new(
        format,
        &[
            (0, AsioPcmEncoding::Int16Lsb),
            (1, AsioPcmEncoding::Float32Msb),
        ],
    )
    .unwrap();
    state.reconfigure_asio(layout, None, 8).unwrap();
    (producer, state)
}
fn prepare(
    state: NativeOutputState,
    ops: &mut FaultOps,
) -> Result<AsioStream, AsioStatePrepareFailure> {
    AsioStream::prepare_state_with(
        None,
        state,
        &[0, 1],
        AsioBufferRequest::Frames(2),
        None,
        None,
        None,
        ops,
    )
}
fn state(stream: &AsioStream) -> &NativeOutputState {
    // SAFETY: this fixture has no native driver; all callbacks are synchronous
    // and completed before state is read through this exclusive test sequence.
    match &unsafe { &*stream.context.state.get() }.renderer {
        RenderOwner::Complete(Some(state)) => state,
        _ => panic!("actual complete owner must remain uniquely present"),
    }
}
fn take(stream: &mut AsioStream) -> Result<Option<NativeOutputState>, AsioStreamError> {
    stream.take_stopped_output_state()
}
fn event(index: i32) -> AsioCallbackEvent {
    AsioCallbackEvent {
        buffer_index: index,
        direct_process: -7,
        flags: 7,
        sample_position: 9001,
        system_nanoseconds: 8123,
        sample_rate: 1000.,
    }
}
fn query(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 1000).unwrap(), 0, 0, 1, 0, 1)
        .unwrap()
}
fn control_error(error: &AsioStreamError, operation: &'static str, code: i32) {
    match error {
        AsioStreamError::Control(AsioControlError::Native {
            operation: actual,
            domain: AsioControlErrorDomain::Asio,
            code: actual_code,
        }) => assert_eq!((*actual, *actual_code), (operation, code)),
        other => panic!("original typed native failure was replaced: {other:?}"),
    }
}

#[test]
fn every_cold_native_failure_returns_original_complete_pending_pcm_after_proven_close() {
    for failure_at in [
        ColdFailure::Rate,
        ColdFailure::Buffers,
        ColdFailure::Channel0,
        ColdFailure::Channel1,
        ColdFailure::Prepare,
        ColdFailure::Latencies,
    ] {
        let (_, mut original) = owner();
        let report = original.render_pending(8).unwrap();
        original.admit(2).unwrap();
        let pcm = original.pending_samples().to_vec();
        let counters = original.mixer().counters();
        let layout = original.layout().copied();
        let mut ops = FaultOps::new();
        ops.fail = Some(failure_at);
        let failure = prepare(original, &mut ops).err().expect("cold failure");
        let operation = match failure_at {
            ColdFailure::Rate => "rate",
            ColdFailure::Buffers => "buffers",
            ColdFailure::Channel0 => "channel0",
            ColdFailure::Channel1 => "channel1",
            ColdFailure::Prepare => "prepare",
            ColdFailure::Latencies => "latencies",
        };
        control_error(failure.error(), operation, -71);
        assert!(failure.pending_owner().is_none(), "{failure_at:?}");
        assert!(failure.cleanup_error().is_none(), "{failure_at:?}");
        let recovered = failure.mixer().expect("proven close returns actual owner");
        assert_eq!(recovered.pending_report(), Some(report));
        assert_eq!(recovered.pending_samples(), pcm);
        assert_eq!(recovered.admitted_frames(), 2);
        assert_eq!(recovered.layout().copied(), layout);
        assert_eq!(recovered.mixer().counters(), counters);
        assert!(!recovered.admission_is_uncertain());
        assert_eq!(ops.close_calls, 1);
        assert_eq!(ops.events.last(), Some(&"close"));
        assert!(!ops.events.contains(&"start"));
    }
}

#[test]
fn complete_ready_is_unprimed_and_close_returns_exact_old_pending_prefix() {
    let (_, mut original) = owner();
    let report = original.render_pending(8).unwrap();
    original.admit(2).unwrap();
    let pcm = original.pending_samples().to_vec();
    let counters = original.mixer().counters();
    let mut ops = FaultOps::new();
    let mut stream = prepare(original, &mut ops).unwrap();
    assert_eq!(stream.phase, AsioStreamPhase::Ready);
    assert_eq!(stream.frame_basis().start_physical_frame(), 2);
    assert_eq!(state(&stream).pending_report(), Some(report));
    assert_eq!(state(&stream).admitted_frames(), 2);
    assert_eq!(state(&stream).pending_samples(), pcm);
    assert_eq!(state(&stream).mixer().counters(), counters);
    assert!(ops
        .planes
        .iter()
        .all(|plane| plane.iter().all(|&byte| byte == 0)));
    assert_eq!(stream.context.state.get_mut().prepared_frames, 0);
    assert_eq!(stream.context.state.get_mut().buffer_fills, 0);
    assert!(
        take(&mut stream).is_err(),
        "Ready cannot imply callback retirement"
    );
    stream.stop_with(&mut ops).unwrap();
    assert!(
        beatkernel::audio::StoppedMixerSource::take_stopped_mixer(&mut stream).is_err(),
        "legacy Mixer extraction cannot flatten complete owner state"
    );
    let recovered = take(&mut stream).unwrap().unwrap();
    assert_eq!(recovered.pending_report(), Some(report));
    assert_eq!(recovered.admitted_frames(), 2);
    assert_eq!(recovered.pending_samples(), pcm);
    assert_eq!(recovered.mixer().counters(), counters);
    assert!(take(&mut stream).unwrap().is_none());
}

#[test]
fn actual_start_primes_b_then_synchronous_a_consumes_the_next_original_prefix() {
    let (_, mut original) = owner();
    let report = original.render_pending(8).unwrap();
    original.admit(2).unwrap();
    let mut ops = FaultOps::new();
    ops.synchronous_a = true;
    let mut stream = prepare(original, &mut ops).unwrap();
    stream.start_with(&mut ops).unwrap();
    let b = ops.start_b.as_ref().unwrap();
    assert_eq!(b[0], [0, 0x18, 0, 0x20]);
    let b_right: Vec<_> = [-0.09375f32, -0.125]
        .into_iter()
        .flat_map(f32::to_be_bytes)
        .collect();
    assert_eq!(b[1], b_right);
    assert_eq!(&*ops.planes[0], &[0, 0x28, 0, 0x30]);
    let a_right: Vec<_> = [-0.15625f32, -0.1875]
        .into_iter()
        .flat_map(f32::to_be_bytes)
        .collect();
    assert_eq!(&*ops.planes[2], a_right);
    let (software, observation) = stream.context.read();
    let observation = observation.unwrap();
    assert_eq!(observation.event, event(0));
    assert_eq!(observation.render, report);
    assert_eq!(
        (
            observation.delivered_first_frame,
            observation.delivered_frames
        ),
        (4, 2)
    );
    assert_eq!(
        (
            software.counters.submitted_frames,
            software.counters.buffer_fills
        ),
        (4, 2)
    );
    assert_eq!(state(&stream).admitted_frames(), 6);
    assert_eq!(state(&stream).pending_frames(), 2);
    assert_eq!(state(&stream).mixer().counters(), report.counters);
    assert_eq!(stream.phase, AsioStreamPhase::Running);
    stream.stop_with(&mut ops).unwrap();
}

#[test]
fn failed_attempted_start_quarantines_primed_b_even_without_any_native_callback() {
    let (_, mut original) = owner();
    let report = original.render_pending(8).unwrap();
    original.admit(2).unwrap();
    let mut ops = FaultOps::new();
    ops.start_code = -72;
    let mut stream = prepare(original, &mut ops).unwrap();
    control_error(&stream.start_with(&mut ops).unwrap_err(), "start", -72);
    assert_eq!(ops.close_calls, 1);
    assert!(
        ops.start_b.is_some(),
        "possibly audible B was submitted before Start"
    );
    assert!(state(&stream).admission_is_uncertain());
    assert_eq!(state(&stream).pending_report(), Some(report));
    assert_eq!(state(&stream).admitted_frames(), 4);
    assert_eq!(stream.context.state.get_mut().buffer_fills, 1);
    assert!(stream.context.read().1.is_none());
    assert!(stream
        .source_playback_view_at(&query(2), 1000, 0, 0)
        .is_none());
    assert!(stream
        .request_source_history_retirement(stream.basis, (1000, 0, 0), 2)
        .is_err());
    assert!(matches!(
        stream.start_with(&mut ops),
        Err(AsioStreamError::InvalidState)
    ));
    let recovered = take(&mut stream).unwrap().unwrap();
    assert!(recovered.admission_is_uncertain());
}

#[test]
fn failed_prestart_diagnostics_never_primes_b_or_consumes_pending_source() {
    let (_, mut original) = owner();
    let report = original.render_pending(8).unwrap();
    original.admit(2).unwrap();
    let pcm = original.pending_samples().to_vec();
    let mut ops = FaultOps::new();
    ops.fail_diagnostics = Some(1);
    let mut stream = prepare(original, &mut ops).unwrap();
    control_error(
        &stream.start_with(&mut ops).unwrap_err(),
        "diagnostics",
        -73,
    );
    assert!(ops.start_b.is_none() && !ops.events.contains(&"start"));
    let recovered = take(&mut stream).unwrap().unwrap();
    assert_eq!(recovered.pending_report(), Some(report));
    assert_eq!(recovered.admitted_frames(), 2);
    assert_eq!(recovered.pending_samples(), pcm);
    assert!(!recovered.admission_is_uncertain());
}

#[test]
fn close_error_is_sticky_and_repeated_stop_cannot_invent_native_release_proof() {
    let (_, original) = owner();
    let mut ops = FaultOps::new();
    let mut stream = prepare(original, &mut ops).unwrap();
    ops.close_code = -81;
    assert!(stream.stop_with(&mut ops).is_err());
    assert!(
        stream.retired,
        "callback drain differs from complete native release"
    );
    assert!(stream.native_release_unproved());
    assert!(stream.latest_cleanup_error().is_some());
    let calls = ops.close_calls;
    // The native handle was consumed by the first close; a later apparent OK
    // cannot erase either its actual error or the remaining HWND obligation.
    ops.close_code = 0;
    assert!(stream.stop_with(&mut ops).is_err());
    assert_eq!(ops.close_calls, calls);
    assert!(stream.native_release_unproved());
    assert!(stream.latest_cleanup_error().is_some());
    assert!(take(&mut stream).is_err());
}

#[test]
fn cold_failure_keeps_actual_pending_owner_and_original_error_when_native_release_is_unproved() {
    for (retired, close_code) in [(false, -81), (true, -82)] {
        let (_, mut original) = owner();
        let report = original.render_pending(8).unwrap();
        original.admit(2).unwrap();
        let pcm = original.pending_samples().to_vec();
        let mut ops = FaultOps::new();
        ops.fail = Some(ColdFailure::Rate);
        ops.close_retired = retired;
        ops.close_code = close_code;
        let mut failure = prepare(original, &mut ops).err().expect("pending owner");
        control_error(failure.error(), "rate", -71);
        assert!(failure.mixer().is_none());
        let pending = failure.pending_owner().unwrap();
        assert_eq!(pending.retired, retired);
        assert!(pending.native_release_unproved());
        assert_eq!(state(pending).pending_report(), Some(report));
        assert_eq!(state(pending).admitted_frames(), 2);
        assert_eq!(state(pending).pending_samples(), pcm);
        let original_error = std::ptr::from_ref(failure.error());
        let cleanup_error = std::ptr::from_ref(pending.latest_cleanup_error().unwrap());
        assert!(AsioStream::retry_state_prepare_cleanup(&mut failure).is_err());
        assert_eq!(std::ptr::from_ref(failure.error()), original_error);
        let pending = failure.pending_owner().unwrap();
        assert_eq!(
            std::ptr::from_ref(pending.latest_cleanup_error().unwrap()),
            cleanup_error
        );
        assert!(pending.native_release_unproved());
        assert!(failure.mixer().is_none());
        control_error(failure.error(), "rate", -71);
    }
}

#[test]
fn retirement_ticket_is_stream_bound_and_cancelled_nonce_cannot_control_a_later_request() {
    let (_, original) = owner();
    let (_, foreign_owner) = owner();
    let mut ops = FaultOps::new();
    let mut foreign_ops = FaultOps::new();
    let mut stream = prepare(original, &mut ops).unwrap();
    let mut foreign = prepare(foreign_owner, &mut foreign_ops).unwrap();
    let ticket = stream
        .submit_source_conditional_retirement(&[0, 1], AsioBufferRequest::Frames(2), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket).unwrap(),
        SourceRetirementStatus::Pending
    );
    assert!(foreign.poll_source_conditional_retirement(&ticket).is_err());
    assert!(foreign
        .cancel_source_conditional_retirement(&ticket)
        .is_err());
    assert_eq!(
        stream
            .cancel_source_conditional_retirement(&ticket)
            .unwrap(),
        SourceRetirementStatus::Cancelled
    );
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket).unwrap(),
        SourceRetirementStatus::Cancelled
    );
    let next = stream
        .submit_source_conditional_retirement(&[0, 1], AsioBufferRequest::Frames(4), None)
        .unwrap()
        .unwrap();
    assert_ne!(next.nonce, ticket.nonce);
    assert!(stream.poll_source_conditional_retirement(&ticket).is_err());
    assert!(stream
        .cancel_source_conditional_retirement(&ticket)
        .is_err());
    assert_eq!(
        stream.poll_source_conditional_retirement(&next).unwrap(),
        SourceRetirementStatus::Pending
    );
    stream.cancel_source_conditional_retirement(&next).unwrap();
    stream.stop_with(&mut ops).unwrap();
    foreign.stop_with(&mut foreign_ops).unwrap();
}

#[test]
fn pending_request_is_busy_without_overwriting_its_frozen_geometry() {
    let (_, original) = owner();
    let mut ops = FaultOps::new();
    let mut stream = prepare(original, &mut ops).unwrap();
    let ticket = stream
        .submit_source_conditional_retirement(&[0, 1], AsioBufferRequest::Frames(2), None)
        .unwrap()
        .unwrap();
    assert!(stream
        .submit_source_conditional_retirement(&[0, 1], AsioBufferRequest::Frames(4), None)
        .unwrap()
        .is_none());
    assert_eq!(
        stream
            .context
            .source_control
            .geometry
            .load(Ordering::Relaxed),
        2
    );
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket).unwrap(),
        SourceRetirementStatus::Pending
    );
    stream
        .cancel_source_conditional_retirement(&ticket)
        .unwrap();
    stream.stop_with(&mut ops).unwrap();
}

#[test]
fn request_submitted_after_sole_preflight_waits_until_next_callback() {
    let (_, original) = owner();
    let mut ops = FaultOps::new();
    let mut stream = prepare(original, &mut ops).unwrap();
    // SAFETY: this fixture has one synchronous callback and live planar boxes.
    assert_eq!(unsafe { stream.context.preflight(0) }, 0);
    let ticket = stream
        .submit_source_conditional_retirement(&[0, 1], AsioBufferRequest::Frames(2), None)
        .unwrap()
        .unwrap();
    // Its previously minted Continue permit must be consumed without a second
    // retirement decision after native evidence publication.
    unsafe {
        stream.context.fill(0, Some(event(0))).unwrap();
    }
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket).unwrap(),
        SourceRetirementStatus::Pending
    );
    assert_eq!(stream.context.state.get_mut().buffer_fills, 1);
    assert_eq!(stream.context.read().1.unwrap().event, event(0));
    assert_eq!(unsafe { stream.context.preflight(1) }, 1);
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket).unwrap(),
        SourceRetirementStatus::Authorized
    );
    stream.stop_with(&mut ops).unwrap();
}

#[test]
fn authorized_retirement_is_sticky_and_fences_source_and_history_without_hiding_diagnostics() {
    let (_, original) = owner();
    let mut ops = FaultOps::new();
    let mut stream = prepare(original, &mut ops).unwrap();
    stream.start_with(&mut ops).unwrap();
    assert!(stream
        .source_playback_view_at(&query(1), 1000, 0, 0)
        .is_some());
    let before = stream.context.read().0;
    let ticket = stream
        .submit_source_conditional_retirement(&[0, 1], AsioBufferRequest::Frames(2), None)
        .unwrap()
        .unwrap();
    for index in [0, 1, 0, 1] {
        // SAFETY: exclusive test sequence, no native callback exists.
        assert_eq!(unsafe { stream.context.preflight(index) }, 1);
    }
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
    assert!(matches!(
        stream.source_liveness(),
        Err(AsioStreamError::RecoveryUnavailable)
    ));
    assert!(stream
        .source_playback_view_at(&query(1), 1000, 0, 0)
        .is_none());
    assert!(matches!(
        stream.request_source_history_retirement(stream.basis, (1000, 0, 0), 1),
        Err(AsioStreamError::RecoveryUnavailable)
    ));
    let after = stream.context.read().0;
    assert_eq!(after.counters, before.counters);
    assert_eq!(after.render, before.render);
    assert!(
        after.telemetry_available,
        "diagnostic telemetry remains available"
    );
    stream.stop_with(&mut ops).unwrap();
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket).unwrap(),
        SourceRetirementStatus::Authorized
    );
    assert!(take(&mut stream).unwrap().is_some());
}

#[test]
fn active_pending_suffix_refuses_retirement_while_matching_paused_whole_suffix_authorizes() {
    for paused in [false, true] {
        let (mut producer, mut original) = owner();
        producer.request_pause(paused);
        let report = original.render_pending(8).unwrap();
        original.admit(2).unwrap();
        assert_eq!(report.paused, paused);
        let pcm = original.pending_samples().to_vec();
        let mut ops = FaultOps::new();
        let mut stream = prepare(original, &mut ops).unwrap();
        let ticket = stream
            .submit_source_conditional_retirement(&[0, 1], AsioBufferRequest::Frames(2), None)
            .unwrap()
            .unwrap();
        // SAFETY: exclusive memory-backed fixture; gate itself writes no PCM.
        assert_eq!(
            unsafe { stream.context.preflight(0) },
            if paused { 1 } else { 0 }
        );
        assert_eq!(
            stream.poll_source_conditional_retirement(&ticket).unwrap(),
            if paused {
                SourceRetirementStatus::Authorized
            } else {
                SourceRetirementStatus::Refused
            }
        );
        assert_eq!(state(&stream).pending_report(), Some(report));
        assert_eq!(state(&stream).pending_samples(), pcm);
        assert_eq!(state(&stream).admitted_frames(), 2);
        assert!(ops.planes.iter().all(|plane| plane.iter().all(|&b| b == 0)));
        stream.stop_with(&mut ops).unwrap();
    }
}

#[test]
fn nonwhole_paused_suffix_cannot_authorize_a_different_buffer_geometry() {
    let (mut producer, mut original) = owner();
    producer.request_pause(true);
    original.render_pending(8).unwrap();
    original.admit(2).unwrap();
    assert_eq!(original.pending_frames(), 6);
    let mut ops = FaultOps::new();
    let mut stream = prepare(original, &mut ops).unwrap();
    let ticket = stream
        .submit_source_conditional_retirement(&[0, 1], AsioBufferRequest::Frames(4), None)
        .unwrap()
        .unwrap();
    assert_eq!(unsafe { stream.context.preflight(0) }, 0);
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket).unwrap(),
        SourceRetirementStatus::Refused
    );
    assert_eq!(state(&stream).pending_frames(), 6);
    stream.stop_with(&mut ops).unwrap();
}

#[test]
fn poststart_diagnostics_error_preserves_primary_and_quarantines_the_possibly_played_prime() {
    let (_, mut original) = owner();
    let report = original.render_pending(8).unwrap();
    original.admit(2).unwrap();
    let mut ops = FaultOps::new();
    ops.fail_diagnostics = Some(2);
    ops.close_code = -84;
    let mut stream = prepare(original, &mut ops).unwrap();
    control_error(
        &stream.start_with(&mut ops).unwrap_err(),
        "diagnostics",
        -73,
    );
    assert_eq!(ops.close_calls, 1);
    assert!(ops.start_b.is_some());
    assert!(state(&stream).admission_is_uncertain());
    assert_eq!(state(&stream).pending_report(), Some(report));
    assert!(stream.native_release_unproved());
    match stream.latest_cleanup_error().unwrap() {
        AsioControlError::Native {
            operation,
            domain: AsioControlErrorDomain::Asio,
            code,
        } => {
            assert_eq!((*operation, *code), ("close", -84));
        }
        other => panic!("original native cleanup was replaced: {other:?}"),
    }
    assert!(take(&mut stream).is_err());
    assert!(stream
        .source_playback_view_at(&query(2), 1000, 0, 0)
        .is_none());
}

#[test]
fn prestart_counter_exhaustion_refuses_before_prime_copy_or_actual_start() {
    for frame_counter in [false, true] {
        let (_, mut original) = owner();
        let report = original.render_pending(8).unwrap();
        original.admit(2).unwrap();
        let pcm = original.pending_samples().to_vec();
        let mut ops = FaultOps::new();
        let mut stream = prepare(original, &mut ops).unwrap();
        if frame_counter {
            stream.context.state.get_mut().prepared_frames = u64::MAX;
        } else {
            stream.context.state.get_mut().version = u64::MAX - 1;
        }
        assert!(stream.start_with(&mut ops).is_err());
        assert!(!ops.events.contains(&"start") && ops.start_b.is_none());
        assert!(ops
            .planes
            .iter()
            .all(|plane| plane.iter().all(|&byte| byte == 0)));
        assert_eq!(state(&stream).pending_report(), Some(report));
        assert_eq!(state(&stream).admitted_frames(), 2);
        assert_eq!(state(&stream).pending_samples(), pcm);
        assert!(!state(&stream).admission_is_uncertain());
        assert!(take(&mut stream).unwrap().is_some());
    }
}

#[test]
fn rate_mismatch_and_invalid_buffer_request_never_create_native_buffers() {
    for wrong_rate in [true, false] {
        let (_, original) = owner();
        let mut ops = FaultOps::new();
        if wrong_rate {
            ops.rate = 999.;
        }
        let request = if wrong_rate {
            AsioBufferRequest::Frames(2)
        } else {
            AsioBufferRequest::Frames(9)
        };
        let failure = AsioStream::prepare_state_with(
            None,
            original,
            &[0, 1],
            request,
            None,
            None,
            None,
            &mut ops,
        )
        .err()
        .expect("invalid cold configuration");
        if wrong_rate {
            assert!(matches!(
                failure.error(),
                AsioStreamError::RateMismatch {
                    expected: 1000,
                    actual: 999.
                }
            ));
        } else {
            assert!(matches!(
                failure.error(),
                AsioStreamError::Control(AsioControlError::Configuration(_))
            ));
        }
        assert!(!ops.events.contains(&"prepare"));
        assert!(!ops.events.contains(&"channel0"));
        assert!(failure.mixer().is_some() && failure.pending_owner().is_none());
        assert_eq!(ops.close_calls, 1);
    }
}

#[test]
fn authorized_native_fault_keeps_original_diagnostics_in_source_and_history_refusals() {
    for (faults, render_error) in [(AsioFaults::RESET, 0), (AsioFaults::default(), 2)] {
        let (_, original) = owner();
        let mut ops = FaultOps::new();
        let mut stream = prepare(original, &mut ops).unwrap();
        stream.start_with(&mut ops).unwrap();
        let ticket = stream
            .submit_source_conditional_retirement(&[0, 1], AsioBufferRequest::Frames(2), None)
            .unwrap()
            .unwrap();
        // SAFETY: exclusive memory-backed native sequence, no concurrent callback.
        assert_eq!(unsafe { stream.context.preflight(0) }, 1);
        assert_eq!(
            stream.poll_source_conditional_retirement(&ticket).unwrap(),
            SourceRetirementStatus::Authorized
        );
        let original = AsioDiagnostics {
            faults,
            render_error,
            event: Some(event(1)),
        };
        // Only primitive native diagnostics are supplied. Production stop owns
        // caching, release, phase selection and original-error propagation.
        ops.reported_native = Some(original);
        assert!(matches!(
            stream.stop_with(&mut ops),
            Err(AsioStreamError::ReopenRequired(actual)) if actual == original
        ));
        assert_eq!(stream.native, original);
        assert!(matches!(
            stream.source_liveness(),
            Err(AsioStreamError::ReopenRequired(actual)) if actual == original
        ));
        assert!(matches!(
            stream.request_source_history_retirement(stream.basis, (1000, 0, 0), 1),
            Err(AsioStreamError::ReopenRequired(actual)) if actual == original
        ));
        assert_eq!(
            stream.poll_source_conditional_retirement(&ticket).unwrap(),
            SourceRetirementStatus::Authorized,
            "sticky permission remains separate from a real native fault"
        );
        assert!(stream
            .source_playback_view_at(&query(1), 1000, 0, 0)
            .is_none());
    }
}

#[test]
fn authorized_cleanup_failure_retains_original_arc_ahead_of_native_and_recovery_refusals() {
    for also_native_fault in [false, true] {
        let (_, original) = owner();
        let mut ops = FaultOps::new();
        let mut stream = prepare(original, &mut ops).unwrap();
        stream.start_with(&mut ops).unwrap();
        let ticket = stream
            .submit_source_conditional_retirement(&[0, 1], AsioBufferRequest::Frames(2), None)
            .unwrap()
            .unwrap();
        // SAFETY: exclusive memory-backed native sequence, no concurrent callback.
        assert_eq!(unsafe { stream.context.preflight(1) }, 1);
        assert_eq!(
            stream.poll_source_conditional_retirement(&ticket).unwrap(),
            SourceRetirementStatus::Authorized
        );
        if also_native_fault {
            ops.reported_native = Some(AsioDiagnostics {
                faults: AsioFaults::RESYNC,
                render_error: 2,
                event: Some(event(0)),
            });
        }
        ops.close_code = -88;
        let AsioStreamError::Cleanup(original_cleanup) = stream.stop_with(&mut ops).unwrap_err()
        else {
            panic!("actual consumed close must preserve its typed cleanup error");
        };
        assert!(std::ptr::eq(
            stream.latest_cleanup_error().unwrap(),
            original_cleanup.as_ref()
        ));
        match original_cleanup.as_ref() {
            AsioControlError::Native {
                operation,
                domain: AsioControlErrorDomain::Asio,
                code,
            } => assert_eq!((*operation, *code), ("close", -88)),
            other => panic!("original native cleanup identity was replaced: {other:?}"),
        }
        let AsioStreamError::Cleanup(liveness) = stream.source_liveness().unwrap_err() else {
            panic!("authorization cannot mask a real cleanup failure");
        };
        assert!(Arc::ptr_eq(&original_cleanup, &liveness));
        let AsioStreamError::Cleanup(history) = stream
            .request_source_history_retirement(stream.basis, (1000, 0, 0), 1)
            .unwrap_err()
        else {
            panic!("Source history cannot replace cleanup failure with a recovery token");
        };
        assert!(Arc::ptr_eq(&original_cleanup, &history));
        let AsioStreamError::Cleanup(repeated_stop) = stream.stop_with(&mut ops).unwrap_err()
        else {
            panic!("consumed native handle cannot erase its cleanup failure");
        };
        assert!(Arc::ptr_eq(&original_cleanup, &repeated_stop));
        assert_eq!(ops.close_calls, 1);
        assert_eq!(
            stream.poll_source_conditional_retirement(&ticket).unwrap(),
            SourceRetirementStatus::Authorized
        );
        assert!(matches!(
            stream.request_source_history_retirement(stream.basis, (999, 0, 0), 1),
            Err(AsioStreamError::Render(AsioRenderError::Core(
                beatkernel::audio::AudioError::InvalidFormat
            )))
        ));
        let wrong_basis = OutputFrameBasis::new(
            stream.basis.origin(),
            stream.basis.sample_rate(),
            stream.basis.start_physical_frame() + 1,
        )
        .unwrap();
        assert!(matches!(
            stream.request_source_history_retirement(wrong_basis, (1000, 0, 0), 1),
            Err(AsioStreamError::Render(AsioRenderError::Core(
                beatkernel::audio::AudioError::InvalidFormat
            )))
        ));
    }
}
