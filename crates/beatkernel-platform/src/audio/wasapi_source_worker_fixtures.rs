//! The actual portable WASAPI step, slot and complete software owner.
use super::*;
use crate::audio::{
    AudioBackendKind, AudioDeviceId, AudioStreamMode, AudioStreamRequest, BufferRequest,
    DeviceFormat, PeriodRequest, SampleEncoding, SharedPeriodPolicy,
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use std::cell::RefCell;

const PADDING_ERROR: i32 = -101;
const ACQUIRE_ERROR: i32 = -102;
const RENDER_ERROR: i32 = -103;
const ENCODE_ERROR: i32 = -104;
const RELEASE_ERROR: i32 = -105;
const CLOCK_ERROR: i32 = -106;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    None,
    Padding,
    History,
    Acquire(u32),
    Render(usize),
    Encode(usize),
    Release(u32, bool),
    Admitted(u32),
    Clock,
}
struct Effects {
    entries: [Effect; 64],
    len: usize,
}
impl Effects {
    fn push(&mut self, effect: Effect) {
        self.entries[self.len] = effect;
        self.len += 1;
    }
    fn as_slice(&self) -> &[Effect] {
        &self.entries[..self.len]
    }
}
#[derive(Clone, Copy, Default)]
struct Failures {
    padding: bool,
    acquire: bool,
    render: bool,
    encode: bool,
    release: bool,
    clock: bool,
}
struct Lease {
    frames: u32,
    index: usize,
}
struct Ops<'a> {
    effects: RefCell<Effects>,
    available: u32,
    failures: Failures,
    history: Option<SourceHistoryRetirementRequest>,
    acquired: usize,
    released: usize,
    released_mask: u64,
    encoded: [f32; 128],
    encoded_len: usize,
    request_during: Option<(&'a SourceRetirementSlot, Effect)>,
    requested_nonce: Option<u64>,
}
impl Ops<'_> {
    fn new(available: u32) -> Self {
        Self {
            effects: RefCell::new(Effects {
                entries: [Effect::None; 64],
                len: 0,
            }),
            available,
            failures: Failures::default(),
            history: None,
            acquired: 0,
            released: 0,
            released_mask: 0,
            encoded: [0.; 128],
            encoded_len: 0,
            request_during: None,
            requested_nonce: None,
        }
    }
    fn effect(&mut self, effect: Effect) {
        self.effects.borrow_mut().push(effect);
        if let Some((slot, at)) = self.request_during {
            if at == effect && self.requested_nonce.is_none() {
                self.requested_nonce = slot
                    .submit(SourceRetirementPolicy::PreservePausedSuffix)
                    .unwrap();
            }
        }
    }
}
impl WasapiSourceOps for Ops<'_> {
    type Lease = Lease;
    fn available_frames(&mut self, _: u32) -> Result<u32, i32> {
        self.effect(Effect::Padding);
        if self.failures.padding {
            Err(PADDING_ERROR)
        } else {
            Ok(self.available)
        }
    }
    fn acquire(&mut self, frames: u32) -> Result<Lease, i32> {
        self.effect(Effect::Acquire(frames));
        if self.failures.acquire {
            return Err(ACQUIRE_ERROR);
        }
        let lease = Lease {
            frames,
            index: self.acquired,
        };
        self.acquired += 1;
        Ok(lease)
    }
    fn history_request(&self) -> Option<SourceHistoryRetirementRequest> {
        self.effects.borrow_mut().push(Effect::History);
        self.history
    }
    fn render_pending(
        &mut self,
        state: &mut NativeOutputState,
        frames: usize,
    ) -> Result<RenderReport, i32> {
        self.effect(Effect::Render(frames));
        if self.failures.render {
            Err(RENDER_ERROR)
        } else {
            state.render_pending(frames).map_err(|_| RENDER_ERROR)
        }
    }
    fn encode(&mut self, _: &mut Lease, _: DeviceFormat, samples: &[f32]) -> Result<(), i32> {
        self.effect(Effect::Encode(samples.len()));
        if self.failures.encode {
            return Err(ENCODE_ERROR);
        }
        self.encoded[..samples.len()].copy_from_slice(samples);
        self.encoded_len = samples.len();
        Ok(())
    }
    fn release(&mut self, lease: Lease, silent: bool) -> Result<(), i32> {
        self.effect(Effect::Release(lease.frames, silent));
        let bit = 1 << lease.index;
        assert_eq!(self.released_mask & bit, 0, "a lease is released once");
        self.released_mask |= bit;
        self.released += 1;
        if self.failures.release {
            Err(RELEASE_ERROR)
        } else {
            Ok(())
        }
    }
    fn admitted(&mut self, _: RenderReport, frames: u32) {
        self.effect(Effect::Admitted(frames));
    }
    fn read_clock(&mut self) -> Result<(), i32> {
        self.effect(Effect::Clock);
        if self.failures.clock {
            Err(CLOCK_ERROR)
        } else {
            Ok(())
        }
    }
}
fn format(channels: u16) -> DeviceFormat {
    DeviceFormat::new(1000, channels, SampleEncoding::Float32, None).unwrap()
}
fn matrix() -> ChannelMatrix {
    ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()
}
fn rig() -> (CommandProducer, NativeOutputState) {
    let (producer, state, _) = rig_with(false, None);
    (producer, state)
}
fn rig_with(
    practice: bool,
    end: Option<u64>,
) -> (
    CommandProducer,
    NativeOutputState,
    Option<PracticeController>,
) {
    rig_with_rate(practice, end, 1000)
}
fn rig_with_rate(
    practice: bool,
    end: Option<u64>,
    rate: u32,
) -> (
    CommandProducer,
    NativeOutputState,
    Option<PracticeController>,
) {
    let source = AudioFormat::new(rate, 1).unwrap();
    let sample_frames = 128;
    let asset_bytes = sample_frames * std::mem::size_of::<f32>();
    let limits = PcmLimits::new(asset_bytes, asset_bytes, 1).unwrap();
    let mut bank = SampleBank::new(source, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(source, vec![0.25; sample_frames], limits).unwrap(),
    )
    .unwrap();
    let program = practice.then(|| {
        PreparedPracticeProgram::new(
            &bank,
            vec![PracticeCue {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.,
            }],
            PracticeLimits::new(8, 1, 128, 8, 32).unwrap(),
        )
        .unwrap()
    });
    let (mut producer, consumer) = command_queue(16).unwrap();
    if !practice {
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.,
            })
            .unwrap();
    }
    let config = MixerConfig::new(
        source,
        ClockDomainId(71),
        Timestamp::ZERO,
        AudioLimits::new(16, 4, 16, 32, 16).unwrap(),
    );
    let config = end.map_or(config, |end| config.with_playback_end_frame(end));
    let mut mixer = Mixer::new(config, bank, consumer).unwrap();
    let controller = program.map(|program| {
        let (controller, endpoint) = practice_queue(&program).unwrap();
        mixer
            .install_practice(
                program,
                endpoint,
                PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(100_000_000), false)
                    .unwrap(),
            )
            .unwrap();
        controller
    });
    mixer.enable_source_playback_history(16).unwrap();
    let output = DeviceFormat::new(rate, 2, SampleEncoding::Float32, None).unwrap();
    let state = match NativeOutputState::new(mixer, output, Some(matrix()), 8) {
        Ok(state) => state,
        Err(_) => panic!("valid complete owner must prepare"),
    };
    (producer, state, controller)
}
fn step(
    state: &mut NativeOutputState,
    slot: &SourceRetirementSlot,
    ops: &mut Ops<'_>,
    work: WasapiSourceWork,
) -> WasapiSourceStep {
    source_worker_step(state, slot, ops, work, &mut 0)
}
fn fill(
    state: &mut NativeOutputState,
    slot: &SourceRetirementSlot,
    ops: &mut Ops<'_>,
) -> WasapiSourceStep {
    step(
        state,
        slot,
        ops,
        WasapiSourceWork::Fill { buffer_frames: 8 },
    )
}
fn effects(ops: &Ops<'_>, expected: &[Effect]) {
    assert_eq!(ops.effects.borrow().as_slice(), expected);
    assert_eq!(ops.acquired, ops.released);
}
fn failed(
    result: WasapiSourceStep,
    phase: WasapiSourcePhase,
    code: i32,
) -> WasapiSourceFailureReport {
    let WasapiSourceStep::Failure(report) = result else {
        panic!("expected {phase:?} failure, got {result:?}");
    };
    assert_eq!(report.primary, Some(WasapiSourceFailure { phase, code }));
    report
}
fn request(
    format: DeviceFormat,
    backend: AudioBackendKind,
    mode: AudioStreamMode,
    buffer: u32,
    period: u32,
) -> AudioStreamRequest {
    AudioStreamRequest::new(
        AudioDeviceId("original-endpoint".into()),
        backend,
        mode,
        format,
        BufferRequest::Frames(buffer),
        PeriodRequest::Frames(period),
    )
    .unwrap()
}
fn shared() -> AudioStreamMode {
    // These fixtures request explicit periods, which use the engine-period API.
    AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod)
}

fn assert_exact_subframe_duration_preserves_live_owner(buffer_duration: bool) {
    let (_, mut state, _) = rig_with_rate(false, None, 48_000);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let report = state.pending_report();
    let basis = state.output_frame_basis();
    let configuration = state.mixer().configuration();
    let counters = state.mixer().counters();
    let history_origin = state
        .mixer()
        .source_playback_history()
        .unwrap()
        .initial_physical_frame();
    let facts = SourceRetirementFacts::from_state(&state, Some(matrix())).unwrap();
    let slot = SourceRetirementSlot::new();
    let word = slot.word.load(Ordering::Acquire);
    let duration = beatkernel::time::Duration::from_nanos(1);
    let request = AudioStreamRequest::new(
        AudioDeviceId("original-endpoint".into()),
        AudioBackendKind::Wasapi,
        AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
        state.format().unwrap(),
        if buffer_duration {
            BufferRequest::Duration(duration)
        } else {
            BufferRequest::Frames(4)
        },
        if buffer_duration {
            PeriodRequest::DeviceDefault
        } else {
            PeriodRequest::Duration(duration)
        },
    )
    .unwrap();
    let result = slot.submit_validated(&facts, &request, Some(&matrix()), 8);
    assert!(
        result.is_err(),
        "Exact subframe duration was accepted: {result:?}"
    );
    assert_eq!(slot.word.load(Ordering::Acquire), word);
    assert_eq!(state.pending_report(), report);
    assert_eq!(state.admitted_frames(), 1);
    assert_eq!(
        state.pending_samples(),
        [0.25, -0.125, 0.25, -0.125, 0.25, -0.125]
    );
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(state.mixer().configuration(), configuration);
    assert_eq!(state.mixer().counters(), counters);
    assert_eq!(state.mixer().frame_cursor(), 4);
    assert!(!state.admission_is_uncertain());
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .initial_physical_frame(),
        history_origin
    );
    assert_eq!(
        slot.submit(SourceRetirementPolicy::PreservePausedSuffix),
        Ok(Some(1)),
        "refused sizing must not consume a retirement nonce"
    );
}

#[test]
fn cold_exact_subframe_buffer_duration_preserves_live_retirement_owner() {
    assert_exact_subframe_duration_preserves_live_owner(true);
}

#[test]
fn cold_exact_subframe_period_duration_preserves_live_retirement_owner() {
    assert_exact_subframe_duration_preserves_live_owner(false);
}

#[test]
fn cold_sizing_refusals_preserve_every_live_slot_state_and_nonce() {
    for phase in 0..5 {
        let (_, mut state, _) = rig_with_rate(false, None, 48_000);
        state.render_pending(4).unwrap();
        state.admit(1).unwrap();
        let facts = SourceRetirementFacts::from_state(&state, Some(matrix())).unwrap();
        let slot = SourceRetirementSlot::new();
        let nonce = if phase == 0 {
            None
        } else {
            let nonce = slot
                .submit(SourceRetirementPolicy::PreservePausedSuffix)
                .unwrap()
                .unwrap();
            match phase {
                1 => {}
                2 => {
                    assert_eq!(slot.worker_decision(&state), WasapiSourceStep::Continue);
                    assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Refused));
                }
                3 => assert_eq!(slot.cancel(nonce), Ok(SourceRetirementStatus::Cancelled)),
                4 => {
                    state.admit(3).unwrap();
                    assert_eq!(slot.worker_decision(&state), WasapiSourceStep::Retire);
                    assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Authorized));
                }
                _ => unreachable!(),
            }
            Some(nonce)
        };
        let word = slot.word.load(Ordering::Acquire);
        let status = nonce.map(|nonce| slot.status(nonce).unwrap());
        let report = state.pending_report();
        let samples = state.pending_samples().to_vec();
        let admitted = state.admitted_frames();
        let basis = state.output_frame_basis();
        let configuration = state.mixer().configuration();
        let counters = state.mixer().counters();
        for (mode, buffer, period) in [
            (
                AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
                BufferRequest::Duration(beatkernel::time::Duration::from_nanos(1)),
                PeriodRequest::DeviceDefault,
            ),
            (
                AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
                BufferRequest::Frames(4),
                PeriodRequest::Duration(beatkernel::time::Duration::from_nanos(1)),
            ),
            (
                AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
                BufferRequest::Duration(beatkernel::time::Duration::MAX),
                PeriodRequest::DeviceDefault,
            ),
            (
                AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
                BufferRequest::Frames(4),
                PeriodRequest::Duration(beatkernel::time::Duration::MAX),
            ),
            (
                AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
                BufferRequest::Frames(4),
                PeriodRequest::Frames(3),
            ),
        ] {
            let draft = AudioStreamRequest::new(
                AudioDeviceId("original-endpoint".into()),
                AudioBackendKind::Wasapi,
                mode,
                state.format().unwrap(),
                buffer,
                period,
            )
            .unwrap();
            assert!(
                slot.submit_validated(&facts, &draft, Some(&matrix()), 8)
                    .is_err(),
                "invalid draft altered phase {phase}: {draft:?}"
            );
            assert_eq!(slot.word.load(Ordering::Acquire), word);
            assert_eq!(nonce.map(|nonce| slot.status(nonce).unwrap()), status);
            assert_eq!(state.pending_report(), report);
            assert_eq!(state.pending_samples(), samples);
            assert_eq!(state.admitted_frames(), admitted);
            assert_eq!(state.output_frame_basis(), basis);
            assert_eq!(state.mixer().configuration(), configuration);
            assert_eq!(state.mixer().counters(), counters);
            assert_eq!(state.mixer().frame_cursor(), 4);
            assert!(!state.admission_is_uncertain());
        }
        if let Some(nonce) = nonce {
            if phase == 4 {
                assert_eq!(
                    slot.submit(SourceRetirementPolicy::PreservePausedSuffix),
                    Ok(None)
                );
                assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Authorized));
                continue;
            }
            if phase == 1 {
                assert_eq!(slot.cancel(nonce), Ok(SourceRetirementStatus::Cancelled));
            }
            assert!(matches!(
                slot.poll(nonce),
                Ok(SourceRetirementStatus::Refused | SourceRetirementStatus::Cancelled)
            ));
        }
        assert_eq!(
            slot.submit(SourceRetirementPolicy::PreservePausedSuffix),
            Ok(Some(if phase == 0 { 1 } else { 2 }))
        );
    }
}

#[test]
fn cold_sizing_accepts_representable_default_and_authorized_rounding_without_effects() {
    use crate::audio::NegotiationPolicy;
    use beatkernel::time::Duration;
    let (_, mut state, _) = rig_with_rate(false, None, 48_000);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let facts = SourceRetirementFacts::from_state(&state, Some(matrix())).unwrap();
    let report = state.pending_report();
    let basis = state.output_frame_basis();
    for (buffer, period, policy, capacity) in [
        (
            BufferRequest::DeviceDefault,
            PeriodRequest::DeviceDefault,
            NegotiationPolicy::Exact,
            8,
        ),
        (
            BufferRequest::Duration(Duration::from_nanos(62_500)),
            PeriodRequest::Frames(3),
            NegotiationPolicy::Exact,
            3,
        ),
        (
            BufferRequest::Frames(3),
            PeriodRequest::Duration(Duration::from_nanos(62_500)),
            NegotiationPolicy::Exact,
            3,
        ),
        (
            BufferRequest::Duration(Duration::from_nanos(1)),
            PeriodRequest::Duration(Duration::from_nanos(1)),
            NegotiationPolicy::AllowSupportedRounding,
            1,
        ),
        (
            BufferRequest::Duration(Duration::from_nanos(20_834)),
            PeriodRequest::Duration(Duration::from_nanos(20_834)),
            NegotiationPolicy::AllowSupportedRounding,
            2,
        ),
    ] {
        let draft = AudioStreamRequest::new(
            AudioDeviceId("original-endpoint".into()),
            AudioBackendKind::Wasapi,
            AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
            state.format().unwrap(),
            buffer,
            period,
        )
        .unwrap()
        .with_negotiation(policy);
        let slot = SourceRetirementSlot::new();
        assert_eq!(
            facts.validate(&draft, Some(&matrix()), capacity),
            Ok(SourceRetirementPolicy::PreservePausedSuffix)
        );
        assert_eq!(
            slot.submit_validated(&facts, &draft, Some(&matrix()), capacity),
            Ok(Some(1))
        );
        assert_eq!(slot.status(1), Ok(SourceRetirementStatus::Pending));
        assert_eq!(state.pending_report(), report);
        assert_eq!(state.admitted_frames(), 1);
        assert_eq!(
            state.pending_samples(),
            [0.25, -0.125, 0.25, -0.125, 0.25, -0.125]
        );
        assert_eq!(state.output_frame_basis(), basis);
        assert_eq!(state.mixer().frame_cursor(), 4);
    }
}

#[test]
fn cold_validation_preserves_live_slot_and_complete_owner() {
    let (_, mut state) = rig();
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let report = state.pending_report();
    let basis = state.output_frame_basis();
    let facts = SourceRetirementFacts::from_state(&state, Some(matrix())).unwrap();
    let slot = SourceRetirementSlot::new();
    for (request, capacity, matrix) in [
        (
            request(format(2), AudioBackendKind::Wasapi, shared(), 8, 4),
            0,
            Some(matrix()),
        ),
        (
            request(format(2), AudioBackendKind::Wasapi, shared(), 8, 4),
            33,
            Some(matrix()),
        ),
        (
            request(format(2), AudioBackendKind::Wasapi, shared(), 33, 4),
            8,
            Some(matrix()),
        ),
        (
            request(format(2), AudioBackendKind::Wasapi, shared(), 8, 33),
            8,
            Some(matrix()),
        ),
        (
            request(format(2), AudioBackendKind::Asio, shared(), 8, 4),
            8,
            Some(matrix()),
        ),
        (
            request(
                format(2),
                AudioBackendKind::Wasapi,
                AudioStreamMode::Exclusive,
                8,
                4,
            ),
            8,
            Some(matrix()),
        ),
        (
            request(
                DeviceFormat::new(2000, 2, SampleEncoding::Float32, None).unwrap(),
                AudioBackendKind::Wasapi,
                shared(),
                8,
                4,
            ),
            8,
            Some(matrix()),
        ),
        (
            request(
                DeviceFormat::new(
                    1000,
                    2,
                    SampleEncoding::Pcm {
                        container_bits: 16,
                        valid_bits: 16,
                    },
                    None,
                )
                .unwrap(),
                AudioBackendKind::Wasapi,
                shared(),
                8,
                4,
            ),
            8,
            Some(matrix()),
        ),
        (
            request(format(2), AudioBackendKind::Wasapi, shared(), 8, 4),
            8,
            Some(ChannelMatrix::new(2, 2, &[1., 0., 0., 1.]).unwrap()),
        ),
    ] {
        assert!(slot
            .submit_validated(&facts, &request, matrix.as_ref(), capacity)
            .is_err());
        assert_eq!(state.pending_report(), report);
        assert_eq!(state.admitted_frames(), 1);
        assert_eq!(
            state.pending_samples(),
            [0.25, -0.125, 0.25, -0.125, 0.25, -0.125]
        );
        assert_eq!(state.output_frame_basis(), basis);
    }
    let same = request(format(2), AudioBackendKind::Wasapi, shared(), 8, 4);
    assert_eq!(
        facts.validate(&same, Some(&matrix()), 8),
        Ok(SourceRetirementPolicy::PreservePausedSuffix)
    );
    let smaller = request(format(2), AudioBackendKind::Wasapi, shared(), 2, 1);
    assert_eq!(
        facts.validate(&smaller, Some(&matrix()), 2),
        Ok(SourceRetirementPolicy::PreservePausedSuffix)
    );
    let changed = ChannelMatrix::new(1, 2, &[0.5, 0.5]).unwrap();
    assert_eq!(
        facts.validate(&same, Some(&changed), 8),
        Ok(SourceRetirementPolicy::RequireEmptySuffix)
    );
    let changed = request(format(1), AudioBackendKind::Wasapi, shared(), 8, 4);
    assert_eq!(
        facts.validate(&changed, None, 8),
        Ok(SourceRetirementPolicy::RequireEmptySuffix)
    );
    assert!(slot
        .submit(SourceRetirementPolicy::PreservePausedSuffix)
        .unwrap()
        .is_some());
}

#[test]
fn actual_empty_paused_and_active_pending_state_controls_both_work_paths() {
    for clock_only in [false, true] {
        for tail in 0..3 {
            for policy in [
                SourceRetirementPolicy::PreservePausedSuffix,
                SourceRetirementPolicy::RequireEmptySuffix,
            ] {
                let (mut producer, mut state) = rig();
                if tail == 1 {
                    producer.request_pause(true);
                }
                if tail != 0 {
                    state.render_pending(4).unwrap();
                    state.admit(1).unwrap();
                }
                let pending = state.pending_report();
                let prefix = state.admitted_frames();
                let slot = SourceRetirementSlot::new();
                let nonce = slot.submit(policy).unwrap().unwrap();
                let mut ops = Ops::new(0);
                let work = if clock_only {
                    WasapiSourceWork::ClockOnly
                } else {
                    WasapiSourceWork::Fill { buffer_frames: 8 }
                };
                let authorized = tail == 0
                    || (tail == 1 && policy == SourceRetirementPolicy::PreservePausedSuffix);
                assert_eq!(
                    step(&mut state, &slot, &mut ops, work),
                    if authorized {
                        WasapiSourceStep::Retire
                    } else {
                        WasapiSourceStep::Continue
                    }
                );
                assert_eq!(
                    slot.status(nonce).unwrap(),
                    if authorized {
                        SourceRetirementStatus::Authorized
                    } else {
                        SourceRetirementStatus::Refused
                    }
                );
                assert_eq!(state.pending_report(), pending);
                assert_eq!(state.admitted_frames(), prefix);
                if authorized {
                    effects(&ops, &[]);
                    let mut after = Ops::new(4);
                    assert_eq!(
                        fill(&mut state, &slot, &mut after),
                        WasapiSourceStep::Retire
                    );
                    assert_eq!(
                        step(&mut state, &slot, &mut after, WasapiSourceWork::ClockOnly),
                        WasapiSourceStep::Retire
                    );
                    effects(&after, &[]);
                } else if clock_only {
                    effects(&ops, &[Effect::Clock]);
                }
            }
        }
    }
}

#[test]
fn an_observed_empty_snapshot_cannot_authorize_a_later_actual_active_suffix() {
    let (_, mut state) = rig();
    assert_eq!(state.pending_frames(), 0);
    let slot = SourceRetirementSlot::new();
    let nonce = slot
        .submit(SourceRetirementPolicy::PreservePausedSuffix)
        .unwrap()
        .unwrap();
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let report = state.pending_report();
    let mut ops = Ops::new(4);
    assert_eq!(
        step(&mut state, &slot, &mut ops, WasapiSourceWork::ClockOnly),
        WasapiSourceStep::Continue
    );
    assert_eq!(slot.status(nonce).unwrap(), SourceRetirementStatus::Refused);
    effects(&ops, &[Effect::Clock]);
    assert_eq!(state.pending_report(), report);
    assert_eq!(state.admitted_frames(), 1);
    assert_eq!(state.mixer().frame_cursor(), 4);
}

#[test]
fn busy_and_unconsumed_results_cannot_be_overwritten_and_nonces_do_not_alias() {
    let (_, mut state) = rig();
    state.render_pending(4).unwrap();
    let slot = SourceRetirementSlot::new();
    for bad in [0, 1, u64::MAX] {
        assert_eq!(slot.status(bad), Err(SourceRetirementError::InvalidTicket));
    }
    let first = slot
        .submit(SourceRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    assert_eq!(
        slot.submit(SourceRetirementPolicy::PreservePausedSuffix),
        Ok(None)
    );
    assert_eq!(slot.poll(first), Ok(SourceRetirementStatus::Pending));
    assert_eq!(slot.worker_decision(&state), WasapiSourceStep::Continue);
    assert_eq!(slot.status(first), Ok(SourceRetirementStatus::Refused));
    assert_eq!(
        slot.submit(SourceRetirementPolicy::PreservePausedSuffix),
        Ok(None)
    );
    assert_eq!(slot.poll(first), Ok(SourceRetirementStatus::Refused));
    assert_eq!(
        slot.status(first),
        Err(SourceRetirementError::InvalidTicket)
    );
    let second = slot
        .submit(SourceRetirementPolicy::PreservePausedSuffix)
        .unwrap()
        .unwrap();
    assert!(second > first);
    for bad in [0, first, second + 1, u64::MAX] {
        assert_eq!(slot.poll(bad), Err(SourceRetirementError::InvalidTicket));
        assert_eq!(slot.cancel(bad), Err(SourceRetirementError::InvalidTicket));
    }
    assert_eq!(slot.cancel(second), Ok(SourceRetirementStatus::Cancelled));
    assert_eq!(
        slot.submit(SourceRetirementPolicy::PreservePausedSuffix),
        Ok(None)
    );
    assert_eq!(slot.worker_decision(&state), WasapiSourceStep::Continue);
    assert_eq!(slot.poll(second), Ok(SourceRetirementStatus::Cancelled));
    let third = slot
        .submit(SourceRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    state.admit(4).unwrap();
    assert_eq!(slot.worker_decision(&state), WasapiSourceStep::Retire);
    assert_eq!(slot.cancel(third), Ok(SourceRetirementStatus::Authorized));
    assert_eq!(slot.poll(third), Ok(SourceRetirementStatus::Authorized));
    assert_eq!(slot.poll(third), Ok(SourceRetirementStatus::Authorized));
    assert_eq!(
        slot.submit(SourceRetirementPolicy::PreservePausedSuffix),
        Ok(None)
    );

    let exhausted = SourceRetirementSlot::new();
    exhausted
        .word
        .store(MAX_NONCE << NONCE_SHIFT, Ordering::Release);
    assert_eq!(
        exhausted.submit(SourceRetirementPolicy::PreservePausedSuffix),
        Err(SourceRetirementError::NoncesExhausted)
    );
    assert_eq!(
        exhausted.word.load(Ordering::Acquire),
        MAX_NONCE << NONCE_SHIFT
    );
}

#[test]
fn cancellation_and_authorization_share_one_atomic_decision() {
    use std::sync::Barrier;
    for _ in 0..32 {
        let (_, state) = rig();
        let slot = SourceRetirementSlot::new();
        let nonce = slot
            .submit(SourceRetirementPolicy::PreservePausedSuffix)
            .unwrap()
            .unwrap();
        let barrier = Barrier::new(2);
        std::thread::scope(|scope| {
            let cancel = scope.spawn(|| {
                barrier.wait();
                slot.cancel(nonce).unwrap()
            });
            barrier.wait();
            let decision = slot.worker_decision(&state);
            let cancellation = cancel.join().unwrap();
            match slot.status(nonce).unwrap() {
                SourceRetirementStatus::Cancelled => {
                    assert_eq!(cancellation, SourceRetirementStatus::Cancelled);
                    assert_eq!(decision, WasapiSourceStep::Continue);
                }
                SourceRetirementStatus::Authorized => {
                    assert_eq!(cancellation, SourceRetirementStatus::Authorized);
                    assert_eq!(decision, WasapiSourceStep::Retire);
                    assert_eq!(slot.cancel(nonce), Ok(SourceRetirementStatus::Authorized));
                }
                other => panic!("race left an undecided or impossible result: {other:?}"),
            }
        });
    }
}

#[test]
fn retirement_submitted_inside_a_lease_waits_until_that_lease_and_clock_complete() {
    for during in [
        Effect::Acquire(4),
        Effect::Render(4),
        Effect::Encode(8),
        Effect::Release(4, false),
        Effect::Clock,
    ] {
        let (_, mut state) = rig();
        let slot = SourceRetirementSlot::new();
        let mut ops = Ops::new(4);
        ops.request_during = Some((&slot, during));
        assert_eq!(
            fill(&mut state, &slot, &mut ops),
            WasapiSourceStep::Continue
        );
        effects(
            &ops,
            &[
                Effect::Padding,
                Effect::Acquire(4),
                Effect::History,
                Effect::Render(4),
                Effect::Encode(8),
                Effect::Release(4, false),
                Effect::Admitted(4),
                Effect::Clock,
            ],
        );
        assert_eq!(
            slot.status(ops.requested_nonce.unwrap()),
            Ok(SourceRetirementStatus::Pending)
        );
        assert_eq!(state.admitted_frames(), 4);
        assert_eq!(state.pending_frames(), 0);
        assert_eq!(
            &ops.encoded[..ops.encoded_len],
            &[0.25, -0.125, 0.25, -0.125, 0.25, -0.125, 0.25, -0.125]
        );
        let mut after = Ops::new(4);
        assert_eq!(
            fill(&mut state, &slot, &mut after),
            WasapiSourceStep::Retire
        );
        assert_eq!(
            step(&mut state, &slot, &mut after, WasapiSourceWork::ClockOnly),
            WasapiSourceStep::Retire
        );
        effects(&after, &[]);
    }
}

#[test]
fn positive_release_admits_only_the_actual_suffix_before_a_later_clock_failure() {
    let (mut producer, mut state) = rig();
    let report = state.render_pending(6).unwrap();
    state.admit(2).unwrap();
    let identity = state
        .mixer()
        .source_playback_history()
        .unwrap()
        .initial_physical_frame();
    let slot = SourceRetirementSlot::new();
    let mut ops = Ops::new(2);
    ops.failures.clock = true;
    failed(
        fill(&mut state, &slot, &mut ops),
        WasapiSourcePhase::Clock,
        CLOCK_ERROR,
    );
    effects(
        &ops,
        &[
            Effect::Padding,
            Effect::Acquire(2),
            Effect::History,
            Effect::Encode(4),
            Effect::Release(2, false),
            Effect::Admitted(2),
            Effect::Clock,
        ],
    );
    assert_eq!(state.admitted_frames(), 4);
    assert_eq!(state.pending_frames(), 2);
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.pending_samples(), [0.25, -0.125, 0.25, -0.125]);
    assert_eq!(state.output_frame_basis().start_physical_frame(), 4);
    assert_eq!(state.mixer().frame_cursor(), 6);
    assert!(!state.admission_is_uncertain());
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .initial_physical_frame(),
        identity
    );
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(6_000_000),
            gain: 0.5,
        })
        .unwrap();
    let mut next = Ops::new(8);
    assert_eq!(
        fill(&mut state, &slot, &mut next),
        WasapiSourceStep::Continue
    );
    assert_eq!(
        &next.encoded[..next.encoded_len],
        &[0.25, -0.125, 0.25, -0.125]
    );
    assert_eq!(
        state.mixer().frame_cursor(),
        6,
        "retained bytes are never rendered again"
    );
    let mut fresh = Ops::new(1);
    assert_eq!(
        fill(&mut state, &slot, &mut fresh),
        WasapiSourceStep::Continue
    );
    assert_eq!(&fresh.encoded[..fresh.encoded_len], &[0.375, -0.1875]);
    assert_eq!(state.mixer().counters().commands_consumed, 2);
}

#[test]
fn acquire_and_padding_failures_create_no_lease_or_later_effect() {
    for padding in [false, true] {
        let (_, mut state) = rig();
        let slot = SourceRetirementSlot::new();
        let mut ops = Ops::new(4);
        ops.failures = Failures {
            padding,
            acquire: true,
            release: true,
            clock: true,
            ..Failures::default()
        };
        failed(
            fill(&mut state, &slot, &mut ops),
            if padding {
                WasapiSourcePhase::Padding
            } else {
                WasapiSourcePhase::Acquire
            },
            if padding {
                PADDING_ERROR
            } else {
                ACQUIRE_ERROR
            },
        );
        effects(
            &ops,
            if padding {
                &[Effect::Padding]
            } else {
                &[Effect::Padding, Effect::Acquire(4)]
            },
        );
        assert_eq!(state.mixer().frame_cursor(), 0);
        assert!(!state.admission_is_uncertain());
    }
    let (_, mut state) = rig();
    let mut ops = Ops::new(9);
    failed(
        fill(&mut state, &SourceRetirementSlot::new(), &mut ops),
        WasapiSourcePhase::Padding,
        INVALID_ARGUMENT,
    );
    effects(&ops, &[Effect::Padding]);
}

#[test]
fn local_render_and_encode_failures_release_silence_once_and_preserve_both_errors() {
    for render in [false, true] {
        for release in [false, true] {
            let (_, mut state) = rig();
            let slot = SourceRetirementSlot::new();
            let mut ops = Ops::new(4);
            ops.failures = Failures {
                render,
                encode: true,
                release,
                clock: true,
                ..Failures::default()
            };
            let report = failed(
                fill(&mut state, &slot, &mut ops),
                if render {
                    WasapiSourcePhase::Render
                } else {
                    WasapiSourcePhase::Encode
                },
                if render { RENDER_ERROR } else { ENCODE_ERROR },
            );
            assert_eq!(report.release, release.then_some(RELEASE_ERROR));
            effects(
                &ops,
                if render {
                    &[
                        Effect::Padding,
                        Effect::Acquire(4),
                        Effect::History,
                        Effect::Render(4),
                        Effect::Release(4, true),
                    ]
                } else {
                    &[
                        Effect::Padding,
                        Effect::Acquire(4),
                        Effect::History,
                        Effect::Render(4),
                        Effect::Encode(8),
                        Effect::Release(4, true),
                    ]
                },
            );
            assert_eq!(state.admitted_frames(), 0);
            assert_eq!(state.pending_frames(), if render { 0 } else { 4 });
            assert_eq!(state.admission_is_uncertain(), release);
            if !render {
                assert_eq!(
                    state.pending_samples(),
                    [0.25, -0.125, 0.25, -0.125, 0.25, -0.125, 0.25, -0.125]
                );
            }
        }
    }
}

#[test]
fn failed_native_release_quarantines_complete_owner_and_explicit_retry_has_no_effects() {
    let (_, mut state) = rig();
    let slot = SourceRetirementSlot::new();
    let mut ops = Ops::new(4);
    ops.failures.release = true;
    ops.failures.clock = true;
    let report = failed(
        fill(&mut state, &slot, &mut ops),
        WasapiSourcePhase::Release,
        RELEASE_ERROR,
    );
    assert_eq!(report.release, Some(RELEASE_ERROR));
    effects(
        &ops,
        &[
            Effect::Padding,
            Effect::Acquire(4),
            Effect::History,
            Effect::Render(4),
            Effect::Encode(8),
            Effect::Release(4, false),
        ],
    );
    let pending = state.pending_report();
    let basis = state.output_frame_basis();
    let config = state.mixer().configuration();
    assert_eq!(state.admitted_frames(), 0);
    assert!(state.admission_is_uncertain());
    assert!(state.reconfigure(format(2), Some(matrix()), 4).is_err());
    assert!(SourceRetirementFacts::from_state(&state, Some(matrix())).is_err());
    let mut state = match state.into_mixer() {
        Err(owner) => owner,
        Ok(_) => panic!("quarantine cannot discard complete state"),
    };
    let retry_slot = SourceRetirementSlot::new();
    retry_slot
        .submit(SourceRetirementPolicy::PreservePausedSuffix)
        .unwrap()
        .unwrap();
    let mut retry = Ops::new(4);
    for work in [
        WasapiSourceWork::Fill { buffer_frames: 8 },
        WasapiSourceWork::ClockOnly,
    ] {
        failed(
            step(&mut state, &retry_slot, &mut retry, work),
            WasapiSourcePhase::Quarantined,
            INVALID_ARGUMENT,
        );
    }
    effects(&retry, &[]);
    assert!(state.admit(1).is_err());
    assert!(state.render_pending(1).is_err());
    assert_eq!(state.pending_report(), pending);
    assert_eq!(state.admitted_frames(), 0);
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(state.mixer().configuration(), config);
    assert_eq!(
        state.pending_samples(),
        [0.25, -0.125, 0.25, -0.125, 0.25, -0.125, 0.25, -0.125]
    );
}

#[test]
fn clock_only_and_full_padding_paths_never_render_or_acquire() {
    for clock_only in [false, true] {
        let (_, mut state) = rig();
        let mut ops = Ops::new(0);
        ops.failures.clock = true;
        let work = if clock_only {
            WasapiSourceWork::ClockOnly
        } else {
            WasapiSourceWork::Fill { buffer_frames: 8 }
        };
        failed(
            step(&mut state, &SourceRetirementSlot::new(), &mut ops, work),
            WasapiSourcePhase::Clock,
            CLOCK_ERROR,
        );
        effects(
            &ops,
            if clock_only {
                &[Effect::Clock]
            } else {
                &[Effect::Padding, Effect::Clock]
            },
        );
        assert_eq!(state.mixer().frame_cursor(), 0);
    }
}

#[test]
fn worker_history_validates_original_request_before_clamps_and_releases_silence_on_failure() {
    for release_fails in [false, true] {
        let (_, mut state) = rig();
        state.render_pending(4).unwrap();
        state.admit(1).unwrap();
        let pending = state.pending_report();
        let slot = SourceRetirementSlot::new();
        let mut ops = Ops::new(1);
        ops.history = Some(SourceHistoryRetirementRequest {
            revision: 2,
            source_pin: 5,
        });
        ops.failures.release = release_fails;
        let mut completed = 0;
        let report = failed(
            source_worker_step(
                &mut state,
                &slot,
                &mut ops,
                WasapiSourceWork::Fill { buffer_frames: 8 },
                &mut completed,
            ),
            WasapiSourcePhase::History,
            INVALID_ARGUMENT,
        );
        assert_eq!(report.release, release_fails.then_some(RELEASE_ERROR));
        effects(
            &ops,
            &[
                Effect::Padding,
                Effect::Acquire(1),
                Effect::History,
                Effect::Release(1, true),
            ],
        );
        assert_eq!(completed, 0);
        assert_eq!(state.pending_report(), pending);
        assert_eq!(state.admitted_frames(), 1);
        assert_eq!(
            state
                .mixer()
                .source_playback_history()
                .unwrap()
                .retirement_revision(),
            0
        );
    }
}

#[test]
fn worker_history_pins_original_report_start_even_when_unsent_suffix_is_after_a_transition() {
    let (_, mut state, _) = rig_with(false, Some(2));
    let report = state.render_pending(4).unwrap();
    state.admit(3).unwrap();
    assert_eq!(report.start_frame, 0);
    assert_eq!(report.playback_frames, 2);
    assert_eq!(state.output_frame_basis().start_physical_frame(), 3);
    assert_eq!(state.pending_samples(), [0., 0.]);
    let slot = SourceRetirementSlot::new();
    let mut completed = 0;
    let mut ops = Ops::new(1);
    ops.history = Some(SourceHistoryRetirementRequest {
        revision: 2,
        source_pin: 4,
    });
    assert_eq!(
        source_worker_step(
            &mut state,
            &slot,
            &mut ops,
            WasapiSourceWork::Fill { buffer_frames: 8 },
            &mut completed
        ),
        WasapiSourceStep::Continue
    );
    effects(
        &ops,
        &[
            Effect::Padding,
            Effect::Acquire(1),
            Effect::History,
            Effect::Encode(2),
            Effect::Release(1, false),
            Effect::Admitted(1),
            Effect::Clock,
        ],
    );
    assert_eq!(
        completed, 0,
        "admission after servicing history cannot complete the earlier clamped request"
    );
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        0
    );
    let mut next = Ops::new(1);
    next.history = ops.history;
    assert_eq!(
        source_worker_step(
            &mut state,
            &slot,
            &mut next,
            WasapiSourceWork::Fill { buffer_frames: 8 },
            &mut completed
        ),
        WasapiSourceStep::Continue
    );
    assert_eq!(completed, 2);
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        2
    );
    assert_eq!(state.mixer().playback_frame_cursor(), 2);
}

#[test]
fn worker_history_preserves_practice_receipt_and_pending_block_until_both_are_released() {
    let (mut producer, mut state, controller) = rig_with(true, None);
    let mut controller = controller.unwrap();
    let slot = SourceRetirementSlot::new();
    for pause in [false, true] {
        producer.request_pause(pause);
        assert_eq!(
            fill(&mut state, &slot, &mut Ops::new(2)),
            WasapiSourceStep::Continue
        );
    }
    producer.request_pause(false);
    let pending = state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    assert_eq!(pending.start_frame, 4);
    assert_eq!(state.mixer().practice_history_retention_pins().0, Some(0));
    let request = SourceHistoryRetirementRequest {
        revision: 2,
        source_pin: 8,
    };
    let mut completed = 0;
    let mut ops = Ops::new(1);
    ops.history = Some(request);
    assert_eq!(
        source_worker_step(
            &mut state,
            &slot,
            &mut ops,
            WasapiSourceWork::Fill { buffer_frames: 8 },
            &mut completed
        ),
        WasapiSourceStep::Continue
    );
    assert_eq!(completed, 0);
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        0
    );
    assert_eq!(
        controller
            .try_pop_projected_receipt()
            .unwrap()
            .receipt
            .physical_frame,
        0
    );
    assert_eq!(state.mixer().practice_history_retention_pins().0, None);
    for prefix in [3, 4] {
        let mut ops = Ops::new(1);
        ops.history = Some(request);
        assert_eq!(
            source_worker_step(
                &mut state,
                &slot,
                &mut ops,
                WasapiSourceWork::Fill { buffer_frames: 8 },
                &mut completed
            ),
            WasapiSourceStep::Continue
        );
        assert_eq!(state.admitted_frames(), prefix);
        assert_eq!(completed, 0);
        assert_eq!(
            state
                .mixer()
                .source_playback_history()
                .unwrap()
                .retained_start_frame(),
            4
        );
    }
    let mut ops = Ops::new(1);
    ops.history = Some(request);
    assert_eq!(
        source_worker_step(
            &mut state,
            &slot,
            &mut ops,
            WasapiSourceWork::Fill { buffer_frames: 8 },
            &mut completed
        ),
        WasapiSourceStep::Continue
    );
    assert_eq!(completed, 2);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(
        (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame()
        ),
        (1000, 0, 0)
    );
}

/// Shared by the external System-allocator binary; every cold owner/slot/script
/// preparation and every owner destruction stays outside the measured step.
pub(crate) fn run_allocation_boundaries() {
    use crate::audio::count_heap_calls;
    let (_, mut state) = rig();
    let slot = SourceRetirementSlot::new();
    let mut ops = Ops::new(4);
    let (result, calls) = count_heap_calls(|| fill(&mut state, &slot, &mut ops));
    assert_eq!(result, WasapiSourceStep::Continue);
    assert_eq!(calls, 0);
    assert_eq!(state.admitted_frames(), 4);

    let (_, mut state) = rig();
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let slot = SourceRetirementSlot::new();
    let nonce = slot
        .submit(SourceRetirementPolicy::PreservePausedSuffix)
        .unwrap()
        .unwrap();
    let mut ops = Ops::new(1);
    let (result, calls) = count_heap_calls(|| fill(&mut state, &slot, &mut ops));
    assert_eq!(result, WasapiSourceStep::Continue);
    assert_eq!(calls, 0);
    assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Refused));
    assert_eq!(state.admitted_frames(), 2);

    let (_, mut state) = rig();
    let slot = SourceRetirementSlot::new();
    slot.submit(SourceRetirementPolicy::PreservePausedSuffix)
        .unwrap()
        .unwrap();
    let mut ops = Ops::new(4);
    let (result, calls) = count_heap_calls(|| {
        (
            fill(&mut state, &slot, &mut ops),
            step(&mut state, &slot, &mut ops, WasapiSourceWork::ClockOnly),
        )
    });
    assert_eq!(result, (WasapiSourceStep::Retire, WasapiSourceStep::Retire));
    assert_eq!(calls, 0);
    effects(&ops, &[]);

    let (_, mut state) = rig();
    let slot = SourceRetirementSlot::new();
    let mut ops = Ops::new(4);
    ops.failures.release = true;
    let (result, calls) = count_heap_calls(|| fill(&mut state, &slot, &mut ops));
    failed(result, WasapiSourcePhase::Release, RELEASE_ERROR);
    assert_eq!(calls, 0);
    let retry_slot = SourceRetirementSlot::new();
    let mut retry = Ops::new(4);
    let (result, calls) = count_heap_calls(|| {
        (
            fill(&mut state, &retry_slot, &mut retry),
            step(
                &mut state,
                &retry_slot,
                &mut retry,
                WasapiSourceWork::ClockOnly,
            ),
        )
    });
    failed(result.0, WasapiSourcePhase::Quarantined, INVALID_ARGUMENT);
    failed(result.1, WasapiSourcePhase::Quarantined, INVALID_ARGUMENT);
    assert_eq!(calls, 0);
    effects(&retry, &[]);
    assert_eq!(state.admitted_frames(), 0);
}
