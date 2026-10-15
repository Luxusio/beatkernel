//! Remote retained owners driven by actual WorkletAudio PCM/events and history.
//! Supplied presentation pairs are portable DI evidence, not device acceptance.
#[path = "support/replay_speed.rs"]
mod fixture;

use beatkernel::{
    audio::{
        AudioLimits, OutputFrameBasis, PracticeBoundaryKind, PracticeEvent, PracticeRateEvent,
        PracticeRateOutcome, PracticeRegion, ProjectedPracticeEvent, SampleBank,
        SourcePlaybackSpan, SourcePlaybackState,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeOutcome, JudgeStage},
    replay::{
        codec::{decode_replay, encode_replay, ReplayCodecError, ReplayFile},
        ReplayHeader, ReplayOperation, ReplayRecord,
    },
    runtime::RuntimeReport,
    time::{ClockDomainId, ClockPair, ClockPoint, Duration, ExtrapolationPolicy, Timestamp},
    transport::Rate,
};
use beatkernel_bms::BmsInputMode;
use beatkernel_bms_runtime::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
    },
    local_input::InputMerger,
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::InputResult,
    native_output_coordinate::NativeOutputCoordinate,
    practice_control::{
        PracticeAction, PracticeRateState, PracticeRequest, PracticeTransactionReply,
        PracticeTransactionRequest,
    },
    replay_capture::CaptureError,
    replay_playback::decode_section_setup,
    replay_speed::ReplaySpeed,
    replay_speed_policy::split_schedule,
    section_start::prepare_retained_at,
    step_gameplay::{
        StepGameplay, StepGameplayConfig, StepGameplayError, StepLocalGameplay,
        StepLocalGameplayError, StepRemoteHistorySnapshot, StepRetainedAdmissionKind,
        StepRetainedOptions, StepRetainedSetup,
    },
    worklet_audio::{
        WorkletAudio, WorkletAudioBuilder, WorkletAudioConfig, WorkletRetainedConfig,
        WorkletRetainedEnvelope, WorkletRetainedEvent,
    },
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static FAIL_ENCODING_ALLOCATION: Cell<bool> = const { Cell::new(false) };
    static FAILED_ENCODING_ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
fn refuse_encoding_allocation() -> bool {
    FAIL_ENCODING_ALLOCATION
        .try_with(|fail| {
            if !fail.replace(false) {
                return false;
            }
            let _ = FAILED_ENCODING_ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
            true
        })
        .unwrap_or(false)
}
struct EncodingAllocator;
// SAFETY: Every successful allocation and every deallocation uses System.
// Failure bookkeeping uses only allocation-free per-thread scalar cells.
unsafe impl GlobalAlloc for EncodingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse_encoding_allocation() {
            return std::ptr::null_mut();
        }
        // SAFETY: The caller supplies a valid allocation layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if refuse_encoding_allocation() {
            return std::ptr::null_mut();
        }
        // SAFETY: The caller supplies a valid allocation layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refuse_encoding_allocation() {
            return std::ptr::null_mut();
        }
        // SAFETY: The caller supplies a live System allocation and valid size.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: Successful pointers retain their original System ownership.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: EncodingAllocator = EncodingAllocator;
struct EncodingFailureScope;
impl Drop for EncodingFailureScope {
    fn drop(&mut self) {
        FAIL_ENCODING_ALLOCATION.with(|fail| fail.set(false));
    }
}
fn fail_next_encoding_allocation<T>(encode: impl FnOnce() -> T) -> (T, usize) {
    FAIL_ENCODING_ALLOCATION.with(|fail| assert!(!fail.get()));
    FAILED_ENCODING_ALLOCATIONS.with(|count| count.set(0));
    FAIL_ENCODING_ALLOCATION.with(|fail| fail.set(true));
    let scope = EncodingFailureScope;
    let result = encode();
    drop(scope);
    (result, FAILED_ENCODING_ALLOCATIONS.with(Cell::get))
}

const PROGRAM: u64 = (1 << 53) + 913;
const CONTEXT: u64 = (1 << 53) + 177;
const SEED: u64 = 73;
const NS: i64 = 1_000_000;
// Four beats occupy4ms. The hold is [1ms,3ms]; BGM cues remain at0 and4ms.
const CHART: &str = "#BPM 60000\n#LNTYPE 1\n#WAV01 key.wav\n\
    #00051:00010001\n#00012:00000100\n#00001:01\n#00101:01\n";

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
fn host(n: i64) -> ClockPoint {
    point(11, 10_000_000_000 + n)
}
fn raw(n: i64) -> ClockPoint {
    point(22, 1_000_000_000 + n)
}
fn logical(n: i64) -> ClockPoint {
    point(33, 5_000_000_000 + n)
}
fn rate(n: i64, d: u64) -> Rate {
    Rate::new(n, d).unwrap()
}
fn speed(n: i64, d: u64) -> ReplaySpeed {
    ReplaySpeed::new(rate(n, d)).unwrap()
}
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: host(0),
        output_origin: raw(0),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 64,
        bgm_pending: 8,
        bgm_lookahead: Duration::from_nanos(20 * NS),
        telemetry_capacity: 32,
    }
}
fn authority(coordinates: bool) -> AudioAuthority {
    authority_basis(coordinates.then(|| {
        AudioAuthorityCoordinateBasis::SourceFrame(OutputFrameBasis::new(raw(0), 1000, 0).unwrap())
    }))
}
fn authority_basis(basis: Option<AudioAuthorityCoordinateBasis>) -> AudioAuthority {
    let config = AudioAuthorityConfig {
        history_capacity: 64,
        max_observation_age: Duration::from_nanos(30_000_000_000),
        input_extrapolation: ExtrapolationPolicy::Forbid,
        max_input_ahead: Duration::ZERO,
    };
    let epoch = AudioAuthorityEpoch {
        id: 1,
        stream_origin: raw(0),
        logical_origin: logical(0),
        host_domain: host(0).domain,
    };
    if let Some(basis) = basis {
        AudioAuthority::new_with_coordinates(config, epoch, basis).unwrap()
    } else {
        AudioAuthority::new(config, epoch).unwrap()
    }
}
fn bindings(device: Option<DeviceId>) -> BindingMap {
    BindingMap::from_bindings(
        [0x11, 0x12]
            .into_iter()
            .enumerate()
            .map(|(i, lane)| Binding {
                device: device.map_or(DeviceSelector::Any, DeviceSelector::Exact),
                physical: PhysicalControlId::keyboard(7 + i as u16),
                game_control: GameControlId(lane),
            }),
    )
    .unwrap()
}
fn options() -> StepRetainedOptions {
    StepRetainedOptions {
        worklet: WorkletRetainedConfig {
            program_id: PROGRAM,
            initial_region: PracticeRegion::new(ts(0), ts(20 * NS), false).unwrap(),
            initial_rate: rate(1, 2),
            output_preroll: Duration::ZERO,
            open_end: false,
            limits: beatkernel::audio::PracticeLimits::new(32, 8, 256, 32, 32).unwrap(),
            source_history_capacity: 64,
        },
        chart_seed: SEED,
        identity_limits: fixture::limits(),
    }
}
fn prepared() -> beatkernel_bms_runtime::section_start::PreparedRetainedSection {
    let pcm: Vec<_> = (0..32).map(|i| i as f32 / 32.0).collect();
    prepare_retained_at(fixture::prepared(CHART, 1000, &[(1, &pcm)]), ts(0)).unwrap()
}
fn audio(bank: SampleBank, setup: &StepRetainedSetup) -> WorkletAudio {
    let mut builder = WorkletAudioBuilder::new(WorkletAudioConfig {
        format: bank.format(),
        pcm_limits: fixture::pcm_limits(),
        audio_limits: AudioLimits::new(64, 128, 64, 64, 64).unwrap(),
    })
    .unwrap();
    for (id, sample) in bank.into_samples() {
        builder
            .insert_sample(id, sample.format(), sample.samples().to_vec())
            .unwrap();
    }
    let mut audio = builder
        .finish_retained(setup.cues.clone(), setup.config)
        .unwrap();
    audio.arm(CONTEXT, CONTEXT).unwrap();
    audio
}
fn history(audio: &WorkletAudio) -> StepRemoteHistorySnapshot {
    let h = audio.source_playback_history().unwrap();
    StepRemoteHistorySnapshot {
        program_id: PROGRAM,
        context_start: CONTEXT,
        source_hz: h.source_rate(),
        initial_physical: h.initial_physical_frame(),
        initial_playback: h.initial_playback_frame(),
        retained_start: h.retained_start_frame(),
        pulled_end: h.pulled_end_frame(),
        retirement_revision: h.retirement_revision(),
        spans: (0..h.len()).map(|i| h.span(i).unwrap()).collect(),
    }
}
fn input(at: i64, device: u64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), host(at), sequence),
        control: PhysicalControlId::keyboard(7),
        state,
    })
}
fn rate_request(
    id: u64,
    cap: beatkernel_bms_runtime::practice_control::PracticeTransactionCapability,
    n: i64,
    d: u64,
) -> PracticeTransactionRequest {
    PracticeTransactionRequest::Rate {
        id,
        generation: cap.practice.generation,
        expected: cap.rate,
        requested_rate: rate(n, d),
    }
}
fn assert_schedule(header: &ReplayHeader, expected: &[(i128, i64, u64)]) {
    let (_, schedule) = split_schedule(&header.options).unwrap();
    assert_eq!(schedule.initial_speed(), speed(1, 2));
    let actual: Vec<_> = schedule
        .cuts()
        .iter()
        .map(|cut| {
            assert_eq!((cut.numerator(), cut.denominator()), (0, 1));
            (
                cut.whole(),
                cut.speed().rate().numerator(),
                cut.speed().rate().denominator(),
            )
        })
        .collect();
    assert_eq!(actual, expected);
}
fn as_rate(event: WorkletRetainedEnvelope) -> PracticeRateEvent {
    match event.event {
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Rate(rate),
            ..
        }) => rate,
        other => panic!("expected real rate event, got {other:?}"),
    }
}

fn accepted_input_record(report: &RuntimeReport, ordinal: u64, song_ns: i64) -> ReplayRecord {
    assert_eq!(report.song_time, ts(song_ns));
    assert_eq!(report.bound_inputs.len(), 1);
    ReplayRecord {
        ordinal,
        song_time: ts(song_ns),
        operation: ReplayOperation::Input(report.bound_inputs[0].clone()),
    }
}

fn render_local(game: &mut StepLocalGameplay, audio: &mut WorkletAudio, from: u64, frames: usize) {
    audio.render(CONTEXT + from, frames).unwrap();
    let frame = from + frames as u64;
    let ns = frame as i64 * NS;
    game.observe_audio_coordinate(
        1,
        ClockPair {
            source: raw(ns),
            target: host(ns),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap();
    game.observe_retained_history(history(audio)).unwrap();
}

fn finish_local_event(
    game: &mut StepLocalGameplay,
    audio: &mut WorkletAudio,
    inputs: &mut InputMerger,
    frame: u64,
) {
    let event = audio
        .poll_retained_event()
        .unwrap()
        .expect("actual local event");
    game.retain_output_event(event).unwrap();
    let ns = frame as i64 * NS;
    game.record_audio_prefix(host(ns)).unwrap();
    assert_eq!(
        game.service_retained_event(inputs, host(ns), raw(ns))
            .unwrap(),
        Some(event.serial)
    );
    game.acknowledge_retained_event(event.serial).unwrap();
    audio.acknowledge_retained_event(event.serial).unwrap();
}

struct Solo {
    game: StepGameplay,
    audio: WorkletAudio,
    inputs: InputMerger,
    physical: u64,
    counter: bool,
}
impl Solo {
    fn new(capture: bool) -> Self {
        Self::configured(capture, options(), false)
    }
    fn configured(capture: bool, retained: StepRetainedOptions, counter: bool) -> Self {
        let authority = if counter {
            authority_basis(Some(AudioAuthorityCoordinateBasis::SourceCounter {
                basis: OutputFrameBasis::new(raw(0), 1000, 0).unwrap(),
                frequency: 1_000_000_000,
            }))
        } else {
            authority(true)
        };
        let end = retained.worklet.initial_region.end;
        let (mut game, bank) = StepGameplay::new_retained_audio_section(
            prepared(),
            config(),
            bindings(None),
            Some(end),
            BmsInputMode::ButtonOnly,
            authority,
            None,
            retained,
        )
        .unwrap();
        if capture {
            game.configure_capture(fixture::limits(), SEED).unwrap();
        }
        let audio = audio(bank, game.retained_setup().unwrap());
        game.activate_audio().unwrap();
        game.observe_audio_coordinate(
            1,
            ClockPair {
                source: raw(0),
                target: host(0),
            },
            if counter {
                NativeOutputCoordinate::Counter {
                    position: 0,
                    frequency: 1_000_000_000,
                }
            } else {
                NativeOutputCoordinate::StreamFrame { frame: 0 }
            },
        )
        .unwrap();
        Self {
            game,
            audio,
            inputs: InputMerger::new(host(0).domain, host(0), vec![DeviceId(1)], 32).unwrap(),
            physical: 0,
            counter,
        }
    }
    fn render(&mut self, frames: usize) {
        self.audio.render(CONTEXT + self.physical, frames).unwrap();
        self.physical += frames as u64;
        if frames > 0 {
            let ns = self.physical as i64 * NS;
            self.game
                .observe_audio_coordinate(
                    1,
                    ClockPair {
                        source: raw(ns),
                        target: host(ns),
                    },
                    if self.counter {
                        NativeOutputCoordinate::Counter {
                            position: self.physical * NS as u64,
                            frequency: 1_000_000_000,
                        }
                    } else {
                        NativeOutputCoordinate::StreamFrame {
                            frame: self.physical,
                        }
                    },
                )
                .unwrap();
            self.game
                .observe_retained_history(history(&self.audio))
                .unwrap();
        }
    }
    fn event(&mut self) -> WorkletRetainedEnvelope {
        let event = self
            .audio
            .poll_retained_event()
            .unwrap()
            .expect("actual rendered event");
        self.game.retain_output_event(event).unwrap();
        event
    }
    fn service(&mut self) -> Option<u64> {
        let ns = self.physical as i64 * NS;
        self.game.record_audio_prefix(host(ns)).unwrap();
        self.game
            .service_retained_event(&mut self.inputs, host(ns), raw(ns))
            .unwrap()
    }
    fn finish_event(&mut self, event: WorkletRetainedEnvelope) {
        assert_eq!(self.service(), Some(event.serial));
        self.game.acknowledge_retained_event(event.serial).unwrap();
        self.audio.acknowledge_retained_event(event.serial).unwrap();
    }
    fn started(&mut self) {
        self.render(3);
        let event = self.event();
        self.finish_event(event);
        assert!(self.game.take_retained_response().is_none());
    }
    fn admit_rate(&mut self, id: u64, n: i64, d: u64) {
        let cap = self.game.retained_capability().unwrap();
        let prepared = self
            .game
            .prepare_retained_transaction(rate_request(id, cap, n, d), false)
            .unwrap();
        self.audio
            .try_set_rate_next(
                id,
                cap.practice.generation,
                cap.rate.revision,
                prepared.preparation.expect("cold retained preparation"),
            )
            .unwrap();
        self.game
            .acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, id, Ok(()))
            .unwrap();
    }
}

#[test]
fn retained_cold_setup_keeps_original_cues_assets_identity_and_avoids_fixed_feeder() {
    let mut rig = Solo::new(false);
    let setup = rig.game.retained_setup().unwrap();
    assert_eq!(setup.config.program_id, PROGRAM);
    assert_eq!(setup.sample_count, 1);
    assert_eq!(
        setup
            .cues
            .iter()
            .map(|cue| cue.at.as_nanos())
            .collect::<Vec<_>>(),
        [0, 4 * NS]
    );
    assert_eq!(setup.geometry.len(), 2);
    assert!(setup
        .geometry
        .iter()
        .all(|g| g.source_frames == 32 && g.source_hz == 1000));
    assert!(
        rig.game.take_commands(64).unwrap().is_none(),
        "no fixed SetRate or duplicate BGM"
    );
    assert_eq!(rig.audio.playback_end_frame(), None);
    let header = rig.game.effective_header(PlayerId(1)).unwrap();
    assert_eq!(
        decode_section_setup(&header.options).unwrap().chart_seed,
        SEED
    );
    assert_eq!(header.normalized_clock, logical(0).domain);
    assert_schedule(header, &[]);
    assert!(rig.game.effective_header(PlayerId(9)).is_none());
    rig.render(3);
    assert_eq!(&rig.audio.output()[..3], &[0.0, 0.015625, 0.03125]);
}

#[test]
fn retained_constructor_rejects_missing_actual_coordinate_basis() {
    assert!(StepGameplay::new_retained_audio_section(
        prepared(),
        config(),
        bindings(None),
        Some(ts(20 * NS)),
        BmsInputMode::ButtonOnly,
        authority(false),
        None,
        options(),
    )
    .is_err());
    let mut wrong = options();
    wrong.worklet.output_preroll = Duration::from_nanos(1);
    assert!(StepGameplay::new_retained_audio_section(
        prepared(),
        config(),
        bindings(None),
        Some(ts(20 * NS)),
        BmsInputMode::ButtonOnly,
        authority(true),
        None,
        wrong,
    )
    .is_err());
}

fn limits_words(limits: beatkernel::audio::PracticeLimits) -> [usize; 5] {
    [
        limits.max_cues,
        limits.max_overlap,
        limits.max_events_per_render,
        limits.control_capacity,
        limits.receipt_capacity,
    ]
}

#[test]
fn cold_solo_limits_use_agreed_actual_capacity_and_cannot_change_capture_or_active_owner() {
    let (mut game, _) = StepGameplay::new_retained_audio_section(
        prepared(),
        config(),
        bindings(None),
        Some(ts(20 * NS)),
        BmsInputMode::ButtonOnly,
        authority(true),
        None,
        options(),
    )
    .unwrap();
    let header = game.effective_header(PlayerId(1)).unwrap().clone();
    let song = game.song_time();
    let cap = game.retained_capability();
    let agreed = beatkernel::audio::PracticeLimits::new(16, 4, 128, 16, 16).unwrap();
    assert!(game.configure_retained_limits(agreed, 0).is_err());
    let too_small = beatkernel::audio::PracticeLimits::new(1, 4, 128, 16, 16).unwrap();
    assert!(game.configure_retained_limits(too_small, 8).is_err());
    assert_eq!(
        game.retained_setup()
            .unwrap()
            .config
            .source_history_capacity,
        64
    );
    game.configure_retained_limits(agreed, 8).unwrap();
    let setup = &game.retained_setup().unwrap().config;
    assert_eq!(limits_words(setup.limits), limits_words(agreed));
    assert_eq!(setup.source_history_capacity, 8);
    assert_eq!(
        (setup.program_id, setup.initial_rate),
        (PROGRAM, rate(1, 2))
    );
    assert_eq!(game.effective_header(PlayerId(1)).unwrap(), &header);
    assert_eq!(game.song_time(), song);
    assert_eq!(game.retained_capability(), cap);
    game.configure_capture(fixture::limits(), SEED).unwrap();
    assert!(game.configure_retained_limits(agreed, 9).is_err());
    game.activate_audio().unwrap();
    assert!(game.configure_retained_limits(agreed, 10).is_err());
    assert_eq!(
        game.retained_setup()
            .unwrap()
            .config
            .source_history_capacity,
        8
    );
    assert_eq!(game.effective_header(PlayerId(1)).unwrap(), &header);
}

#[test]
fn cold_local_limits_preserve_every_member_identity_and_reject_after_activation() {
    let members = vec![
        (PlayerId(3), Some(DeviceId(1))),
        (PlayerId(7), Some(DeviceId(2))),
    ];
    let maps = members
        .iter()
        .map(|(_, device)| bindings(*device))
        .collect();
    let (mut game, _) = StepLocalGameplay::new_retained_audio_section(
        prepared(),
        config(),
        ResolvedInputPlan::new(members).unwrap(),
        maps,
        Some(ts(20 * NS)),
        BmsInputMode::ButtonOnly,
        authority(true),
        None,
        options(),
    )
    .unwrap();
    let headers =
        [PlayerId(3), PlayerId(7)].map(|id| (id, game.effective_header(id).unwrap().clone()));
    let cap = game.retained_capability();
    let agreed = beatkernel::audio::PracticeLimits::new(16, 4, 128, 16, 16).unwrap();
    game.configure_retained_limits(agreed, 8).unwrap();
    let setup = &game.retained_setup().unwrap().config;
    assert_eq!(limits_words(setup.limits), limits_words(agreed));
    assert_eq!(setup.source_history_capacity, 8);
    assert_eq!(
        (setup.program_id, setup.initial_rate),
        (PROGRAM, rate(1, 2))
    );
    assert_eq!(game.retained_capability(), cap);
    for (id, expected) in &headers {
        assert_eq!(game.effective_header(*id).unwrap(), expected);
    }
    game.activate_audio().unwrap();
    assert!(game.configure_retained_limits(agreed, 9).is_err());
    assert_eq!(
        game.retained_setup()
            .unwrap()
            .config
            .source_history_capacity,
        8
    );
    for (id, expected) in &headers {
        assert_eq!(game.effective_header(*id).unwrap(), expected);
    }
}

#[test]
fn legacy_fixed_step_still_accepts_legacy_authority_without_enabling_retained_transactions() {
    let pcm = [0.0, 0.25, 0.5];
    let (mut game, _) = StepGameplay::new_audio_section_with_speed(
        fixture::prepared(CHART, 1000, &[(1, &pcm)]),
        config(),
        bindings(None),
        ts(0),
        Some(ts(20 * NS)),
        BmsInputMode::ButtonOnly,
        authority(false),
        speed(1, 2),
    )
    .unwrap();
    assert_eq!(game.playback_speed(), speed(1, 2));
    assert!(game.retained_setup().is_none());
    assert!(game.retained_capability().is_none());
    assert_eq!(game.retained_history_pin().unwrap(), None);
    assert!(game
        .prepare_retained_transaction(
            PracticeTransactionRequest::Rate {
                id: 1,
                generation: 1,
                expected: PracticeRateState {
                    revision: 0,
                    rate: rate(1, 2)
                },
                requested_rate: rate(2, 1),
            },
            false
        )
        .is_err());
    assert_eq!(game.playback_speed(), speed(1, 2));
    game.activate_audio().unwrap();
    let batch = game.take_commands(64).unwrap().unwrap();
    assert!(
        matches!(batch.commands[0], beatkernel::audio::AudioCommand::SetRate { rate: actual, .. }
        if actual == rate(1, 2))
    );
}

#[test]
fn genuine_nanosecond_source_counter_supports_actual_rate_cut_and_fractional_input() {
    let mut rig = Solo::configured(true, options(), true);
    rig.started();
    rig.inputs
        .admit(input(2 * NS, 1, 0, ButtonState::Down), host(3 * NS))
        .unwrap();
    let head = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(3 * NS), raw(3 * NS), None)
        .unwrap()
        .unwrap();
    assert_eq!(head.song_time, ts(NS));
    rig.admit_rate(120, 2, 1);
    rig.render(2);
    assert_eq!(&rig.audio.output()[..2], &[0.046875, 0.109375]);
    let event = rig.event();
    let actual = as_rate(event);
    assert_eq!(
        (actual.physical_frame, actual.active_elapsed_nanos),
        (3, 3_000_000)
    );
    rig.finish_event(event);
    assert_schedule(
        rig.game.effective_header(PlayerId(1)).unwrap(),
        &[(1_500_000, 2, 1)],
    );
    rig.inputs
        .admit(input(3_750_000, 1, 1, ButtonState::Up), host(5 * NS))
        .unwrap();
    let tail = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(5 * NS), raw(5 * NS), None)
        .unwrap()
        .unwrap();
    assert_eq!(tail.song_time, ts(3 * NS));
    assert!(tail
        .judge_events
        .iter()
        .any(|e| e.stage == JudgeStage::HoldTail && matches!(e.outcome, JudgeOutcome::Hit { .. })));
    assert_eq!(rig.game.score().hits, 2);
    let before = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    assert!(rig
        .game
        .observe_audio_coordinate(
            1,
            ClockPair {
                source: raw(6 * NS),
                target: host(6 * NS)
            },
            NativeOutputCoordinate::Counter {
                position: 6_000_000,
                frequency: 999_999_999
            }
        )
        .is_err());
    assert_eq!(rig.game.effective_header(PlayerId(1)).unwrap(), &before);
}

#[test]
fn retained_input_pin_prevents_real_remote_retirement_past_unread_input_and_event_ack() {
    let mut rig = Solo::new(false);
    rig.started();
    assert_eq!(rig.game.retained_history_pin().unwrap(), Some(0));
    rig.inputs
        .admit(input(2 * NS, 1, 0, ButtonState::Down), host(3 * NS))
        .unwrap();
    let cap = rig.game.retained_capability().unwrap();
    rig.game
        .prepare_retained_pause(121, cap.practice.generation, true)
        .unwrap();
    rig.audio
        .request_pause(121, cap.practice.generation, true)
        .unwrap();
    rig.game
        .acknowledge_retained_admission(StepRetainedAdmissionKind::Pause, 121, Ok(()))
        .unwrap();
    rig.render(2);
    let pause = rig.event();
    assert!(matches!(pause.event, WorkletRetainedEvent::Pause(e) if e.physical_frame == 3));
    let pin = rig.game.retained_history_pin().unwrap().unwrap();
    assert!(pin <= 2, "unread host2ms requires its original source span");
    assert!(pin <= 3, "held pause evidence also pins its actual cut");
    assert!(pin <= history(&rig.audio).pulled_end);
    let header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    // The PCM owner knows its event cut but does not own the input queue. This
    // deliberate bad retirement is real history and must be rejected by Step.
    rig.audio.retire_source_playback_before(3).unwrap();
    assert_eq!(history(&rig.audio).retained_start, 3);
    assert!(rig
        .game
        .observe_retained_history(history(&rig.audio))
        .is_err());
    assert_eq!(rig.game.retained_history_pin().unwrap(), Some(pin));
    assert_eq!(rig.game.effective_header(PlayerId(1)).unwrap(), &header);
    assert_eq!(rig.service(), None);
    let head = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(5 * NS), raw(5 * NS), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        head.song_time,
        ts(NS),
        "rejected retirement retained the original input mapping"
    );
    assert_eq!(head.judge_events.len(), 1);
    assert_eq!(rig.service(), Some(pause.serial));
    let pin_before_ack = rig.game.retained_history_pin().unwrap();
    rig.game.acknowledge_retained_event(pause.serial).unwrap();
    rig.audio.acknowledge_retained_event(pause.serial).unwrap();
    assert_eq!(
        rig.game.retained_history_pin().unwrap(),
        pin_before_ack,
        "event ACK does not close the unread/future-input history"
    );
}

#[test]
fn disable_loop_after_variable_rate_preserves_phase_headers_attempt_and_held_score() {
    let mut retained = options();
    retained.worklet.initial_region = PracticeRegion::new(ts(0), ts(20 * NS), true).unwrap();
    let mut rig = Solo::configured(true, retained, false);
    rig.render(1);
    let started = rig.event();
    rig.finish_event(started);
    rig.admit_rate(122, 1, 1);
    rig.render(1);
    let rate_event = rig.event();
    assert_eq!(as_rate(rate_event).active_elapsed_nanos, 1_000_000);
    rig.finish_event(rate_event);
    rig.game.take_retained_response();
    rig.inputs
        .admit(input(1_500_000, 1, 0, ButtonState::Down), host(2 * NS))
        .unwrap();
    let head = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(2 * NS), raw(2 * NS), None)
        .unwrap()
        .unwrap();
    assert_eq!(head.song_time, ts(NS));
    let cap = rig.game.retained_capability().unwrap();
    let header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    assert_schedule(&header, &[(500_000, 1, 1)]);
    let score = rig.game.score().clone();
    let gauge = *rig.game.gauge().snapshot();
    let request = PracticeTransactionRequest::Region(PracticeRequest {
        id: 123,
        generation: cap.practice.generation,
        action: PracticeAction::DisableLoop,
    });
    let prepared = rig
        .game
        .prepare_retained_transaction(request, false)
        .unwrap();
    assert!(prepared.preparation.is_none());
    rig.audio
        .try_disable_loop_next(123, cap.practice.generation, cap.rate.revision)
        .unwrap();
    rig.game
        .acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 123, Ok(()))
        .unwrap();
    rig.render(1);
    let disable = rig.event();
    assert!(matches!(disable.event,
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Boundary(receipt), ..
        }) if receipt.kind == PracticeBoundaryKind::LoopDisabled && receipt.physical_frame == 2));
    rig.finish_event(disable);
    let after = rig.game.retained_capability().unwrap();
    assert_eq!(after.practice.generation, cap.practice.generation);
    assert_eq!(after.rate.rate, rate(1, 1));
    assert_eq!(after.rate.revision, cap.rate.revision + 1);
    assert_eq!(rig.game.effective_header(PlayerId(1)).unwrap(), &header);
    assert_eq!(rig.game.score(), &score);
    assert_eq!(*rig.game.gauge().snapshot(), gauge);
    assert!(rig.game.take_retired_replay(PlayerId(1)).unwrap().is_none());
    match rig.game.take_retained_response().unwrap() {
        PracticeTransactionReply::Region {
            reply,
            rate: Some(state),
        } => {
            assert_eq!(reply.id, 123);
            assert!(reply.result.is_ok());
            assert_eq!(state, after.rate);
        }
        other => panic!("wrong disable-loop response: {other:?}"),
    }
    rig.render(2);
    rig.inputs
        .admit(input(3_500_000, 1, 1, ButtonState::Up), host(5 * NS))
        .unwrap();
    rig.game.record_audio_prefix(host(5 * NS)).unwrap();
    let tail = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(5 * NS), raw(5 * NS), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        tail.song_time,
        ts(3 * NS),
        "DisableLoop must not rebase the variable phase"
    );
    assert!(tail
        .judge_events
        .iter()
        .any(|e| e.stage == JudgeStage::HoldTail && matches!(e.outcome, JudgeOutcome::Hit { .. })));
    assert_eq!(rig.game.score().hits, 2);
}

#[test]
fn admission_success_is_not_a_cut_and_busy_or_wrong_ack_cannot_change_pending_request() {
    let mut rig = Solo::new(true);
    rig.started();
    let cap = rig.game.retained_capability().unwrap();
    let header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    let first = rig
        .game
        .prepare_retained_transaction(rate_request(10, cap, 2, 1), false)
        .unwrap();
    assert!(rig
        .game
        .prepare_retained_transaction(rate_request(11, cap, 3, 2), false)
        .is_err());
    assert!(rig
        .game
        .acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 11, Ok(()))
        .is_err());
    rig.audio
        .try_set_rate_next(
            10,
            cap.practice.generation,
            cap.rate.revision,
            first.preparation.unwrap(),
        )
        .unwrap();
    rig.game
        .acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 10, Ok(()))
        .unwrap();
    assert_eq!(rig.game.retained_capability().unwrap(), cap);
    assert_eq!(rig.game.effective_header(PlayerId(1)).unwrap(), &header);
    assert!(rig.game.take_retained_response().is_none());
    assert_eq!(rig.service(), None);
    assert!(rig.audio.poll_retained_event().unwrap().is_none());
    rig.render(2);
    let actual = rig.event();
    let cut = as_rate(actual);
    assert_eq!(
        (
            cut.physical_frame,
            cut.playback_frame,
            cut.active_elapsed_nanos
        ),
        (3, 3, 3 * NS as i128)
    );
    rig.finish_event(actual);
    match rig.game.take_retained_response().unwrap() {
        PracticeTransactionReply::Rate {
            id: 10,
            result: Ok(applied),
            ..
        } => assert_eq!(applied, cut),
        other => panic!("wrong correlated reply: {other:?}"),
    }
    assert!(rig.game.take_retained_response().is_none());
    assert_schedule(
        rig.game.effective_header(PlayerId(1)).unwrap(),
        &[(1_500_000, 2, 1)],
    );
    assert!(rig.game.take_retired_replay(PlayerId(1)).unwrap().is_none());
}

#[test]
fn stale_generation_revision_and_fixed_competition_guard_refuse_without_owner_mutation() {
    let mut rig = Solo::new(false);
    rig.started();
    let cap = rig.game.retained_capability().unwrap();
    let header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    let request = rate_request(20, cap, 2, 1);
    assert!(rig
        .game
        .prepare_retained_transaction(request, true)
        .is_err());
    let mut stale = cap;
    stale.practice.generation += 1;
    assert!(rig
        .game
        .prepare_retained_transaction(rate_request(20, stale, 2, 1), false)
        .is_err());
    stale = cap;
    stale.rate.revision += 1;
    assert!(rig
        .game
        .prepare_retained_transaction(rate_request(20, stale, 2, 1), false)
        .is_err());
    assert_eq!(rig.game.retained_capability().unwrap(), cap);
    assert_eq!(rig.game.effective_header(PlayerId(1)).unwrap(), &header);
    assert!(rig.game.take_retained_response().is_none());
    assert!(
        rig.game
            .prepare_retained_transaction(request, false)
            .is_ok(),
        "refusal leaves no hidden pending slot"
    );
}

#[test]
fn correlated_admission_failure_releases_pending_slot_and_preserves_exact_failure() {
    let mut rig = Solo::new(true);
    rig.started();
    let cap = rig.game.retained_capability().unwrap();
    let header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    rig.game
        .prepare_retained_transaction(rate_request(30, cap, 2, 1), false)
        .unwrap();
    rig.game
        .acknowledge_retained_admission(
            StepRetainedAdmissionKind::Transaction,
            30,
            Err("bounded Worklet queue busy".into()),
        )
        .unwrap();
    match rig.game.take_retained_response().unwrap() {
        PracticeTransactionReply::Rate {
            id: 30,
            result: Err(reason),
            ..
        } => assert_eq!(reason, "bounded Worklet queue busy"),
        other => panic!("admission rejection must remain a rejection: {other:?}"),
    }
    assert_eq!(rig.game.retained_capability().unwrap(), cap);
    assert_eq!(rig.game.effective_header(PlayerId(1)).unwrap(), &header);
    assert!(rig
        .game
        .prepare_retained_transaction(rate_request(31, cap, 2, 1), false)
        .is_ok());
}

#[test]
fn actual_cut_preserves_held_judge_score_gauge_and_original_capture_provenance() {
    let mut rig = Solo::new(true);
    rig.started();
    rig.inputs
        .admit(input(2 * NS, 1, 0, ButtonState::Down), host(3 * NS))
        .unwrap();
    let head = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(3 * NS), raw(3 * NS), None)
        .unwrap()
        .unwrap();
    assert_eq!(head.song_time, ts(NS));
    assert_eq!(head.judge_events.len(), 1);
    assert_eq!(head.judge_events[0].stage, JudgeStage::HoldHead);
    assert!(matches!(
        head.judge_events[0].outcome,
        JudgeOutcome::Hit { .. }
    ));
    let score = rig.game.score().clone();
    let gauge = *rig.game.gauge().snapshot();
    let generation = rig.game.retained_capability().unwrap().practice.generation;
    rig.admit_rate(40, 2, 1);
    rig.render(2);
    let event = rig.event();
    rig.finish_event(event);
    assert_eq!(rig.game.score(), &score);
    assert_eq!(*rig.game.gauge().snapshot(), gauge);
    assert_eq!(
        rig.game.retained_capability().unwrap().practice.generation,
        generation
    );
    assert_eq!(
        rig.game.retained_capability().unwrap().rate,
        PracticeRateState {
            revision: 1,
            rate: rate(2, 1)
        }
    );
    rig.inputs
        .admit(input(3_750_000, 1, 1, ButtonState::Up), host(5 * NS))
        .unwrap();
    let tail = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(5 * NS), raw(5 * NS), None)
        .unwrap()
        .unwrap();
    assert_eq!(tail.song_time, ts(3 * NS));
    assert!(tail
        .judge_events
        .iter()
        .any(|e| e.stage == JudgeStage::HoldTail && matches!(e.outcome, JudgeOutcome::Hit { .. })));
    assert_eq!(
        rig.game.score().hits,
        2,
        "a same-attempt rate must retain the held head"
    );
    rig.game.fail();
    let file = decode_replay(&rig.game.take_replay().unwrap().unwrap(), fixture::limits()).unwrap();
    assert_eq!(
        &file.header,
        rig.game.effective_header(PlayerId(1)).unwrap()
    );
    assert_schedule(&file.header, &[(1_500_000, 2, 1)]);
    let originals: Vec<_> = file
        .records
        .iter()
        .filter_map(|r| match &r.operation {
            ReplayOperation::Input(saved) => Some((
                saved.physical.meta().sequence,
                saved.physical.meta().original_clock_point,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        originals,
        [(0, Some(host(2 * NS))), (1, Some(host(3_750_000)))]
    );
}

#[test]
fn actual_rate_cut_waits_for_older_input_but_leaves_future_input_for_the_new_phase() {
    let mut rig = Solo::new(true);
    rig.started();
    let before = rig.game.retained_capability().unwrap();
    rig.admit_rate(45, 2, 1);
    rig.render(2);
    rig.inputs
        .admit(input(2 * NS, 1, 0, ButtonState::Down), host(5 * NS))
        .unwrap();
    rig.inputs
        .admit(input(3_750_000, 1, 1, ButtonState::Up), host(5 * NS))
        .unwrap();
    let event = rig.event();
    assert_eq!(
        rig.service(),
        None,
        "a rendered cut cannot overtake acquired pre-cut input"
    );
    assert_eq!(rig.game.retained_capability().unwrap(), before);
    assert_schedule(rig.game.effective_header(PlayerId(1)).unwrap(), &[]);
    let head = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(5 * NS), raw(5 * NS), None)
        .unwrap()
        .unwrap();
    assert_eq!(head.song_time, ts(NS));
    assert_eq!(rig.inputs.pending(), 1);
    rig.finish_event(event);
    assert_eq!(
        rig.inputs.pending(),
        1,
        "post-cut input belongs to the newly committed phase"
    );
    let tail = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(5 * NS), raw(5 * NS), None)
        .unwrap()
        .unwrap();
    assert_eq!(tail.song_time, ts(3 * NS));
    assert!(tail
        .judge_events
        .iter()
        .any(|e| e.stage == JudgeStage::HoldTail && matches!(e.outcome, JudgeOutcome::Hit { .. })));
    assert_eq!(rig.game.score().hits, 2);
}

#[test]
fn failed_event_service_preserves_pending_rate_and_later_report_for_exact_retry() {
    let mut rig = Solo::new(true);
    rig.started();
    let before = rig.game.retained_capability().unwrap();
    let header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    rig.admit_rate(46, 2, 1);
    rig.render(2);
    let event = rig.event();
    rig.game.record_audio_prefix(host(5 * NS)).unwrap();
    assert!(rig
        .game
        .service_retained_event(
            &mut rig.inputs,
            point(99, host(5 * NS).timestamp.as_nanos()),
            raw(5 * NS)
        )
        .is_err());
    assert_eq!(rig.game.retained_capability().unwrap(), before);
    assert_eq!(rig.game.effective_header(PlayerId(1)).unwrap(), &header);
    assert!(rig.game.take_retained_response().is_none());
    rig.inputs
        .admit(input(2 * NS, 1, 0, ButtonState::Down), host(5 * NS))
        .unwrap();
    let report = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(5 * NS), raw(5 * NS), None)
        .unwrap()
        .unwrap();
    assert_eq!(report.song_time, ts(NS));
    rig.finish_event(event);
    rig.game.fail();
    let file = decode_replay(&rig.game.take_replay().unwrap().unwrap(), fixture::limits()).unwrap();
    assert_schedule(&file.header, &[(1_500_000, 2, 1)]);
    assert!(file.records.iter().any(|r| matches!(&r.operation,
        ReplayOperation::Input(saved) if saved.physical.meta().sequence == 0)));
}

#[test]
fn normalized_equal_rate_is_noop_with_one_revision_and_no_history_or_new_attempt() {
    for capture in [false, true] {
        let mut rig = Solo::new(capture);
        rig.started();
        let before = rig.game.retained_capability().unwrap();
        let header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
        rig.admit_rate(50, 2, 4);
        rig.render(1);
        let event = rig.event();
        let actual = as_rate(event);
        assert_eq!(actual.outcome, PracticeRateOutcome::Noop);
        rig.finish_event(event);
        let after = rig.game.retained_capability().unwrap();
        assert_eq!(after.practice.generation, before.practice.generation);
        assert_eq!(
            after.rate,
            PracticeRateState {
                revision: 1,
                rate: rate(1, 2)
            }
        );
        assert_eq!(rig.game.effective_header(PlayerId(1)).unwrap(), &header);
        assert!(rig.game.take_retired_replay(PlayerId(1)).unwrap().is_none());
    }
}

#[test]
fn exact_event_redelivery_is_idempotent_conflicting_serial_and_wrong_ack_are_atomic() {
    let mut rig = Solo::new(false);
    rig.started();
    rig.admit_rate(60, 2, 1);
    rig.render(1);
    let actual = rig.event();
    rig.game.retain_output_event(actual).unwrap();
    let mut conflict = actual;
    conflict.context_start += 1;
    assert!(rig.game.retain_output_event(conflict).is_err());
    assert_eq!(rig.service(), Some(actual.serial));
    let header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    assert!(rig
        .game
        .acknowledge_retained_event(actual.serial + 1)
        .is_err());
    assert_eq!(
        rig.service(),
        Some(actual.serial),
        "committed serial remains until its exact local ACK"
    );
    rig.game.retain_output_event(actual).unwrap();
    assert_eq!(rig.game.effective_header(PlayerId(1)).unwrap(), &header);
    assert_schedule(&header, &[(1_500_000, 2, 1)]);
    assert!(rig.game.take_retained_response().is_some());
    assert!(rig.game.take_retained_response().is_none());
    rig.game.acknowledge_retained_event(actual.serial).unwrap();
    assert_eq!(
        rig.audio.poll_retained_event().unwrap(),
        Some(actual),
        "local ACK does not ACK the Worklet"
    );
    rig.audio.acknowledge_retained_event(actual.serial).unwrap();
}

#[test]
fn malformed_remote_history_cannot_replace_original_overlapping_or_pulled_evidence() {
    let mut rig = Solo::new(false);
    rig.started();
    let cap = rig.game.retained_capability().unwrap();
    let header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    for kind in 0..9 {
        let mut bad = history(&rig.audio);
        match kind {
            0 => bad.program_id += 1,
            1 => bad.context_start += 1,
            2 => bad.source_hz += 1,
            3 => bad.pulled_end = 2,
            4 => bad.retained_start = 1,
            5 => bad.initial_playback = 1,
            6 => {
                bad.spans =
                    vec![SourcePlaybackSpan::new(0, None, 0, SourcePlaybackState::Frozen).unwrap()]
            }
            7 => {
                bad.spans = vec![
                    SourcePlaybackSpan::new(0, Some(1), 0, SourcePlaybackState::Running).unwrap(),
                    SourcePlaybackSpan::new(2, None, 2, SourcePlaybackState::Running).unwrap(),
                ]
            }
            8 => {
                bad.spans = (0..65)
                    .map(|i| {
                        SourcePlaybackSpan::new(i, Some(i + 1), i, SourcePlaybackState::Running)
                            .unwrap()
                    })
                    .collect()
            }
            _ => unreachable!(),
        }
        assert!(
            rig.game.observe_retained_history(bad).is_err(),
            "invalid snapshot kind {kind}"
        );
        assert_eq!(rig.game.retained_capability().unwrap(), cap);
        assert_eq!(rig.game.effective_header(PlayerId(1)).unwrap(), &header);
        rig.game
            .observe_retained_history(history(&rig.audio))
            .unwrap();
    }
    rig.admit_rate(70, 2, 1);
    rig.render(1);
    let event = rig.event();
    rig.finish_event(event);
    assert_schedule(
        rig.game.effective_header(PlayerId(1)).unwrap(),
        &[(1_500_000, 2, 1)],
    );
}

#[test]
fn pause_holds_pending_rate_until_real_resume_and_excludes_frozen_source_duration() {
    let mut rig = Solo::new(false);
    rig.started();
    let before = rig.game.retained_capability().unwrap();
    rig.game
        .prepare_retained_pause(80, before.practice.generation, true)
        .unwrap();
    rig.audio
        .request_pause(80, before.practice.generation, true)
        .unwrap();
    rig.game
        .acknowledge_retained_admission(StepRetainedAdmissionKind::Pause, 80, Ok(()))
        .unwrap();
    rig.render(2);
    let pause = rig.event();
    rig.finish_event(pause);
    rig.admit_rate(81, 2, 1);
    rig.render(2);
    assert!(rig.audio.poll_retained_event().unwrap().is_none());
    assert_eq!(rig.service(), None);
    assert_eq!(rig.game.retained_capability().unwrap().rate, before.rate);
    rig.game
        .prepare_retained_pause(82, before.practice.generation, false)
        .unwrap();
    rig.audio
        .request_pause(82, before.practice.generation, false)
        .unwrap();
    rig.game
        .acknowledge_retained_admission(StepRetainedAdmissionKind::Pause, 82, Ok(()))
        .unwrap();
    rig.render(2);
    let resume = rig.event();
    assert!(matches!(resume.event, WorkletRetainedEvent::Pause(_)));
    rig.finish_event(resume);
    let rate = rig.event();
    let applied = as_rate(rate);
    assert_eq!(
        (
            applied.physical_frame,
            applied.playback_frame,
            applied.active_elapsed_nanos
        ),
        (7, 3, 3 * NS as i128)
    );
    rig.finish_event(rate);
    assert_schedule(
        rig.game.effective_header(PlayerId(1)).unwrap(),
        &[(1_500_000, 2, 1)],
    );
}

#[test]
fn region_uses_current_rate_and_f5_restores_original_invocation_preserving_each_old_capture() {
    let mut rig = Solo::new(true);
    rig.started();
    rig.inputs
        .admit(input(2 * NS, 1, 0, ButtonState::Down), host(3 * NS))
        .unwrap();
    let head = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(3 * NS), raw(3 * NS), None)
        .unwrap()
        .unwrap();
    assert_eq!(head.song_time, ts(NS));
    assert_eq!(rig.game.score().hits, 1);
    rig.admit_rate(90, 2, 1);
    rig.render(1);
    let event = rig.event();
    rig.finish_event(event);
    rig.game.take_retained_response();
    let old_header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    let before = rig.game.retained_capability().unwrap();
    let request = PracticeTransactionRequest::Region(PracticeRequest {
        id: 91,
        generation: before.practice.generation,
        action: PracticeAction::Loop {
            start: ts(NS),
            end: ts(4 * NS),
        },
    });
    rig.game
        .prepare_retained_transaction(request, false)
        .unwrap();
    rig.audio
        .try_region_next(
            91,
            before.practice.generation,
            before.rate.revision,
            PracticeRegion::new(ts(NS), ts(4 * NS), true).unwrap(),
        )
        .unwrap();
    rig.game
        .acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 91, Ok(()))
        .unwrap();
    rig.render(1);
    let region = rig.event();
    rig.finish_event(region);
    let after = rig.game.retained_capability().unwrap();
    assert_eq!(after.rate.rate, rate(2, 1));
    assert_eq!(after.practice.generation, before.practice.generation + 1);
    assert_eq!(
        rig.game.score().hits,
        0,
        "the fresh region starts its own judge attempt"
    );
    let retired = decode_replay(
        &rig.game.take_retired_replay(PlayerId(1)).unwrap().unwrap(),
        fixture::limits(),
    )
    .unwrap();
    assert_eq!(retired.header, old_header);
    assert_schedule(&retired.header, &[(1_500_000, 2, 1)]);
    assert!(retired.records.iter().any(|r| matches!(&r.operation,
        ReplayOperation::Input(saved) if saved.physical.meta().sequence == 0
            && saved.physical.meta().original_clock_point == Some(host(2 * NS)))));
    assert!(rig.game.take_retired_replay(PlayerId(1)).unwrap().is_none());
    rig.game.take_retained_response();
    let region_header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    let restart = rig
        .game
        .prepare_retained_transaction(
            PracticeTransactionRequest::RestartOriginal {
                id: 92,
                generation: after.practice.generation,
                expected: after.rate,
            },
            false,
        )
        .unwrap();
    rig.audio
        .try_restart_original_next(
            92,
            after.practice.generation,
            after.rate.revision,
            restart.preparation.unwrap(),
        )
        .unwrap();
    rig.game
        .acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 92, Ok(()))
        .unwrap();
    rig.render(1);
    let original = rig.event();
    rig.finish_event(original);
    let restored = rig.game.retained_capability().unwrap();
    assert_eq!(restored.rate.rate, rate(1, 2));
    assert_eq!(restored.practice.generation, after.practice.generation + 1);
    assert_eq!(rig.game.score().hits, 0);
    let retired_region = decode_replay(
        &rig.game.take_retired_replay(PlayerId(1)).unwrap().unwrap(),
        fixture::limits(),
    )
    .unwrap();
    assert_eq!(retired_region.header, region_header);
}

#[test]
fn retired_capture_slot_backpressures_second_fresh_boundary_until_exact_old_capture_is_taken() {
    let mut rig = Solo::new(true);
    rig.started();
    for id in [100, 101] {
        let cap = rig.game.retained_capability().unwrap();
        let prepared = rig
            .game
            .prepare_retained_transaction(
                PracticeTransactionRequest::RestartOriginal {
                    id,
                    generation: cap.practice.generation,
                    expected: cap.rate,
                },
                false,
            )
            .unwrap();
        rig.audio
            .try_restart_original_next(
                id,
                cap.practice.generation,
                cap.rate.revision,
                prepared.preparation.unwrap(),
            )
            .unwrap();
        rig.game
            .acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, id, Ok(()))
            .unwrap();
        rig.render(1);
        let event = rig.event();
        if id == 101 {
            let current = rig.game.effective_header(PlayerId(1)).unwrap().clone();
            assert_eq!(rig.service(), None);
            assert_eq!(rig.game.retained_capability().unwrap(), cap);
            assert_eq!(rig.game.effective_header(PlayerId(1)).unwrap(), &current);
            assert_eq!(rig.audio.poll_retained_event().unwrap(), Some(event));
            assert!(rig.game.take_retired_replay(PlayerId(1)).unwrap().is_some());
        }
        rig.finish_event(event);
        assert!(rig.game.take_retained_response().is_some());
    }
    assert!(
        rig.game.take_retired_replay(PlayerId(1)).unwrap().is_some(),
        "second capture was retained independently"
    );
}

#[test]
fn solo_retired_encoding_failure_preserves_exact_prefix_and_fresh_boundary_backpressure() {
    let mut rig = Solo::new(true);
    rig.started();
    rig.inputs
        .admit(input(2 * NS, 1, 0, ButtonState::Down), host(3 * NS))
        .unwrap();
    let head = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(3 * NS), raw(3 * NS), None)
        .unwrap()
        .unwrap();
    rig.admit_rate(120, 2, 1);
    rig.render(2);
    let event = rig.event();
    rig.finish_event(event);
    rig.game.take_retained_response();
    rig.inputs
        .admit(input(3_750_000, 1, 1, ButtonState::Up), host(5 * NS))
        .unwrap();
    let tail = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(5 * NS), raw(5 * NS), None)
        .unwrap()
        .unwrap();
    let original_header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    assert_schedule(&original_header, &[(1_500_000, 2, 1)]);
    let expected = ReplayFile::new(
        original_header,
        vec![
            accepted_input_record(&head, 0, NS),
            accepted_input_record(&tail, 1, 3 * NS),
        ],
    );
    assert_eq!(
        head.bound_inputs[0].physical.meta().original_clock_point,
        Some(host(2 * NS))
    );
    assert_eq!(
        tail.bound_inputs[0].physical.meta().original_clock_point,
        Some(host(3_750_000))
    );
    let expected_bytes = encode_replay(&expected, fixture::limits()).unwrap();
    let cap = rig.game.retained_capability().unwrap();
    rig.game
        .prepare_retained_transaction(
            PracticeTransactionRequest::Region(PracticeRequest {
                id: 121,
                generation: cap.practice.generation,
                action: PracticeAction::Loop {
                    start: ts(NS),
                    end: ts(4 * NS),
                },
            }),
            false,
        )
        .unwrap();
    rig.audio
        .try_region_next(
            121,
            cap.practice.generation,
            cap.rate.revision,
            PracticeRegion::new(ts(NS), ts(4 * NS), true).unwrap(),
        )
        .unwrap();
    rig.game
        .acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 121, Ok(()))
        .unwrap();
    rig.render(1);
    let region = rig.event();
    rig.finish_event(region);
    rig.game.take_retained_response();
    let current_header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    assert_ne!(current_header, expected.header);
    rig.inputs
        .admit(input(5_500_000, 1, 2, ButtonState::Down), host(6 * NS))
        .unwrap();
    let current = rig
        .game
        .process_next_audio_input(&mut rig.inputs, host(6 * NS), raw(6 * NS), None)
        .unwrap()
        .unwrap();
    let expected_current = ReplayFile::new(
        current_header.clone(),
        vec![accepted_input_record(&current, 0, 2 * NS)],
    );
    let (failed, refusals) =
        fail_next_encoding_allocation(|| rig.game.take_retired_replay(PlayerId(1)));
    assert_eq!(
        refusals, 1,
        "the real encoder allocation must have been refused"
    );
    assert!(matches!(
        failed,
        Err(StepGameplayError::Capture {
            error: CaptureError::Codec(ReplayCodecError::AllocationFailed),
            report: None,
        })
    ));
    assert_eq!(
        rig.game.effective_header(PlayerId(1)).unwrap(),
        &current_header
    );
    let cap = rig.game.retained_capability().unwrap();
    let restart = rig
        .game
        .prepare_retained_transaction(
            PracticeTransactionRequest::RestartOriginal {
                id: 122,
                generation: cap.practice.generation,
                expected: cap.rate,
            },
            false,
        )
        .unwrap();
    rig.audio
        .try_restart_original_next(
            122,
            cap.practice.generation,
            cap.rate.revision,
            restart.preparation.unwrap(),
        )
        .unwrap();
    rig.game
        .acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 122, Ok(()))
        .unwrap();
    rig.render(1);
    let restart_event = rig.event();
    assert_eq!(
        rig.service(),
        None,
        "failed export must still occupy the retired slot"
    );
    assert_eq!(rig.game.retained_capability().unwrap(), cap);
    assert_eq!(
        rig.game.effective_header(PlayerId(1)).unwrap(),
        &current_header
    );
    assert_eq!(
        rig.audio.poll_retained_event().unwrap(),
        Some(restart_event)
    );
    let recovered = rig.game.take_retired_replay(PlayerId(1)).unwrap().unwrap();
    assert_eq!(recovered, expected_bytes);
    assert_eq!(
        decode_replay(&recovered, fixture::limits()).unwrap(),
        expected
    );
    assert!(rig.game.take_retired_replay(PlayerId(1)).unwrap().is_none());
    rig.finish_event(restart_event);
    let current_bytes = rig.game.take_retired_replay(PlayerId(1)).unwrap().unwrap();
    assert_eq!(
        decode_replay(&current_bytes, fixture::limits()).unwrap(),
        expected_current
    );
    assert!(rig.game.take_retired_replay(PlayerId(1)).unwrap().is_none());
    let final_header = rig.game.effective_header(PlayerId(1)).unwrap().clone();
    rig.game.fail();
    let final_bytes = rig.game.take_replay().unwrap().unwrap();
    assert_eq!(
        decode_replay(&final_bytes, fixture::limits()).unwrap(),
        ReplayFile::new(final_header, vec![])
    );
    assert!(rig.game.take_replay().unwrap().is_none());
}

#[test]
fn local_two_and_four_members_share_exact_cut_and_keep_all_effective_headers_with_mixed_capture() {
    for count in [2, 4] {
        let members: Vec<_> = (1..=count)
            .map(|id| (PlayerId(id), Some(DeviceId(id as u64))))
            .collect();
        let maps = members
            .iter()
            .map(|(_, device)| bindings(*device))
            .collect();
        let (mut game, bank) = StepLocalGameplay::new_retained_audio_section(
            prepared(),
            config(),
            ResolvedInputPlan::new(members).unwrap(),
            maps,
            Some(ts(20 * NS)),
            BmsInputMode::ButtonOnly,
            authority(true),
            None,
            options(),
        )
        .unwrap();
        game.configure_capture(PlayerId(1), fixture::limits(), SEED)
            .unwrap();
        let mut audio = audio(bank, game.retained_setup().unwrap());
        let mut inputs = InputMerger::new(
            host(0).domain,
            host(0),
            (1..=count).map(|id| DeviceId(id as u64)).collect(),
            32,
        )
        .unwrap();
        game.activate_audio().unwrap();
        for (frame, frames) in [(0, 0), (3, 3)] {
            if frames != 0 {
                audio.render(CONTEXT, frames).unwrap();
            }
            game.observe_audio_coordinate(
                1,
                ClockPair {
                    source: raw(frame * NS),
                    target: host(frame * NS),
                },
                NativeOutputCoordinate::StreamFrame {
                    frame: frame as u64,
                },
            )
            .unwrap();
        }
        game.observe_retained_history(history(&audio)).unwrap();
        game.record_audio_prefix(host(3 * NS)).unwrap();
        let started = audio.poll_retained_event().unwrap().unwrap();
        game.retain_output_event(started).unwrap();
        assert_eq!(game.retained_history_pin().unwrap(), Some(0));
        assert_eq!(
            game.service_retained_event(&mut inputs, host(3 * NS), raw(3 * NS))
                .unwrap(),
            Some(started.serial)
        );
        game.acknowledge_retained_event(started.serial).unwrap();
        audio.acknowledge_retained_event(started.serial).unwrap();
        assert_eq!(
            game.retained_history_pin().unwrap(),
            Some(0),
            "ACK leaves local future-input history pinned"
        );
        inputs
            .admit(
                input(2 * NS, count as u64, 0, ButtonState::Down),
                host(3 * NS),
            )
            .unwrap();
        let Some(InputResult::Processed(reports)) = game
            .process_next_audio_input(&mut inputs, host(3 * NS), raw(3 * NS), None)
            .unwrap()
        else {
            panic!("local actual held input was not routed");
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].player, PlayerId(count));
        assert_eq!(reports[0].report.song_time, ts(NS));
        let cap = game.retained_capability().unwrap();
        let prepared = game
            .prepare_retained_transaction(rate_request(110, cap, 2, 1), false)
            .unwrap();
        audio
            .try_set_rate_next(
                110,
                cap.practice.generation,
                cap.rate.revision,
                prepared.preparation.unwrap(),
            )
            .unwrap();
        game.acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 110, Ok(()))
            .unwrap();
        audio.render(CONTEXT + 3, 2).unwrap();
        game.observe_audio_coordinate(
            1,
            ClockPair {
                source: raw(5 * NS),
                target: host(5 * NS),
            },
            NativeOutputCoordinate::StreamFrame { frame: 5 },
        )
        .unwrap();
        game.observe_retained_history(history(&audio)).unwrap();
        game.record_audio_prefix(host(5 * NS)).unwrap();
        let event = audio.poll_retained_event().unwrap().unwrap();
        game.retain_output_event(event).unwrap();
        assert!(
            game.retained_history_pin().unwrap().unwrap() <= 3,
            "the shared local rate event cannot lose its actual frame3 history"
        );
        assert_eq!(
            game.service_retained_event(&mut inputs, host(5 * NS), raw(5 * NS))
                .unwrap(),
            Some(event.serial)
        );
        game.acknowledge_retained_event(event.serial).unwrap();
        audio.acknowledge_retained_event(event.serial).unwrap();
        assert!(
            game.retained_history_pin().unwrap().unwrap() <= 3,
            "a still-legal host3.75ms input needs original frame3 history"
        );
        assert_eq!(
            history(&audio).retained_start,
            0,
            "local event ACK is separate from history retirement"
        );
        inputs
            .admit(
                input(3_750_000, count as u64, 1, ButtonState::Up),
                host(5 * NS),
            )
            .unwrap();
        let Some(InputResult::Processed(tails)) = game
            .process_next_audio_input(&mut inputs, host(5 * NS), raw(5 * NS), None)
            .unwrap()
        else {
            panic!("local actual held tail was not routed");
        };
        assert_eq!(tails.len(), 1);
        assert_eq!(tails[0].player, PlayerId(count));
        assert_eq!(tails[0].report.song_time, ts(3 * NS));
        assert!(tails[0]
            .report
            .judge_events
            .iter()
            .any(|e| e.stage == JudgeStage::HoldTail
                && matches!(e.outcome, JudgeOutcome::Hit { .. })));
        for id in 1..=count {
            let header = game.effective_header(PlayerId(id)).unwrap();
            assert_schedule(header, &[(1_500_000, 2, 1)]);
            assert_eq!(
                decode_section_setup(&header.options).unwrap().chart_seed,
                SEED
            );
            assert_eq!(
                game.score(PlayerId(id)).unwrap().hits,
                2 * u64::from(id == count)
            );
        }
        assert!(game.effective_header(PlayerId(count + 1)).is_none());
        assert_eq!(
            game.retained_capability().unwrap().practice.generation,
            cap.practice.generation
        );
        assert!(game.take_retired_replay(PlayerId(1)).unwrap().is_none());
        assert!(game.take_retired_replay(PlayerId(count)).unwrap().is_none());
        let captured_header = game.effective_header(PlayerId(1)).unwrap().clone();
        game.fail();
        let captured = decode_replay(
            &game.take_replay(PlayerId(1)).unwrap().unwrap(),
            fixture::limits(),
        )
        .unwrap();
        assert_eq!(captured.header, captured_header);
        assert_schedule(&captured.header, &[(1_500_000, 2, 1)]);
        assert!(
            game.take_replay(PlayerId(count)).unwrap().is_none(),
            "capture-off member keeps identity without inventing a recording"
        );
    }
}

#[test]
fn local_retired_encoding_failure_preserves_player_prefix_and_other_captures() {
    let members = vec![
        (PlayerId(1), Some(DeviceId(1))),
        (PlayerId(2), Some(DeviceId(2))),
    ];
    let maps = members
        .iter()
        .map(|(_, device)| bindings(*device))
        .collect();
    let (mut game, bank) = StepLocalGameplay::new_retained_audio_section(
        prepared(),
        config(),
        ResolvedInputPlan::new(members).unwrap(),
        maps,
        Some(ts(20 * NS)),
        BmsInputMode::ButtonOnly,
        authority(true),
        None,
        options(),
    )
    .unwrap();
    for player in [PlayerId(1), PlayerId(2)] {
        game.configure_capture(player, fixture::limits(), SEED)
            .unwrap();
    }
    let mut audio = audio(bank, game.retained_setup().unwrap());
    let mut inputs =
        InputMerger::new(host(0).domain, host(0), vec![DeviceId(1), DeviceId(2)], 32).unwrap();
    game.activate_audio().unwrap();
    game.observe_audio_coordinate(
        1,
        ClockPair {
            source: raw(0),
            target: host(0),
        },
        NativeOutputCoordinate::StreamFrame { frame: 0 },
    )
    .unwrap();
    render_local(&mut game, &mut audio, 0, 3);
    finish_local_event(&mut game, &mut audio, &mut inputs, 3);
    let mut original_records = [Vec::new(), Vec::new()];
    for player in [1, 2] {
        inputs
            .admit(input(2 * NS, player, 0, ButtonState::Down), host(3 * NS))
            .unwrap();
        let Some(InputResult::Processed(reports)) = game
            .process_next_audio_input(&mut inputs, host(3 * NS), raw(3 * NS), None)
            .unwrap()
        else {
            panic!("local original head was not processed");
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].player, PlayerId(player as u32));
        assert_eq!(
            reports[0].report.bound_inputs[0]
                .physical
                .meta()
                .original_clock_point,
            Some(host(2 * NS))
        );
        original_records[player as usize - 1].push(accepted_input_record(
            &reports[0].report,
            0,
            NS,
        ));
    }
    let cap = game.retained_capability().unwrap();
    let change = game
        .prepare_retained_transaction(rate_request(130, cap, 2, 1), false)
        .unwrap();
    audio
        .try_set_rate_next(
            130,
            cap.practice.generation,
            cap.rate.revision,
            change.preparation.unwrap(),
        )
        .unwrap();
    game.acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 130, Ok(()))
        .unwrap();
    render_local(&mut game, &mut audio, 3, 2);
    finish_local_event(&mut game, &mut audio, &mut inputs, 5);
    game.take_retained_response();
    for player in [1, 2] {
        inputs
            .admit(input(3_750_000, player, 1, ButtonState::Up), host(5 * NS))
            .unwrap();
        let Some(InputResult::Processed(reports)) = game
            .process_next_audio_input(&mut inputs, host(5 * NS), raw(5 * NS), None)
            .unwrap()
        else {
            panic!("local original tail was not processed");
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].player, PlayerId(player as u32));
        assert_eq!(
            reports[0].report.bound_inputs[0]
                .physical
                .meta()
                .original_clock_point,
            Some(host(3_750_000))
        );
        original_records[player as usize - 1].push(accepted_input_record(
            &reports[0].report,
            1,
            3 * NS,
        ));
    }
    let expected: [ReplayFile; 2] = std::array::from_fn(|index| {
        let header = game
            .effective_header(PlayerId(index as u32 + 1))
            .unwrap()
            .clone();
        assert_schedule(&header, &[(1_500_000, 2, 1)]);
        ReplayFile::new(header, std::mem::take(&mut original_records[index]))
    });
    let expected_bytes: [Vec<u8>; 2] =
        std::array::from_fn(|i| encode_replay(&expected[i], fixture::limits()).unwrap());
    let cap = game.retained_capability().unwrap();
    game.prepare_retained_transaction(
        PracticeTransactionRequest::Region(PracticeRequest {
            id: 131,
            generation: cap.practice.generation,
            action: PracticeAction::Loop {
                start: ts(NS),
                end: ts(4 * NS),
            },
        }),
        false,
    )
    .unwrap();
    audio
        .try_region_next(
            131,
            cap.practice.generation,
            cap.rate.revision,
            PracticeRegion::new(ts(NS), ts(4 * NS), true).unwrap(),
        )
        .unwrap();
    game.acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 131, Ok(()))
        .unwrap();
    render_local(&mut game, &mut audio, 5, 1);
    finish_local_event(&mut game, &mut audio, &mut inputs, 6);
    game.take_retained_response();
    let mut expected_current: [ReplayFile; 2] = std::array::from_fn(|index| {
        ReplayFile::new(
            game.effective_header(PlayerId(index as u32 + 1))
                .unwrap()
                .clone(),
            vec![],
        )
    });
    inputs
        .admit(input(5_500_000, 1, 2, ButtonState::Down), host(6 * NS))
        .unwrap();
    let Some(InputResult::Processed(reports)) = game
        .process_next_audio_input(&mut inputs, host(6 * NS), raw(6 * NS), None)
        .unwrap()
    else {
        panic!("local current input was not processed");
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].player, PlayerId(1));
    expected_current[0]
        .records
        .push(accepted_input_record(&reports[0].report, 0, 2 * NS));
    let (failed, refusals) =
        fail_next_encoding_allocation(|| game.take_retired_replay(PlayerId(1)));
    assert_eq!(
        refusals, 1,
        "the real encoder allocation must have been refused"
    );
    assert!(matches!(
        failed,
        Err(StepLocalGameplayError::Control(
            StepGameplayError::Capture {
                error: CaptureError::Codec(ReplayCodecError::AllocationFailed),
                report: None,
            }
        ))
    ));
    let other = game.take_retired_replay(PlayerId(2)).unwrap().unwrap();
    assert_eq!(other, expected_bytes[1]);
    assert_eq!(
        decode_replay(&other, fixture::limits()).unwrap(),
        expected[1]
    );
    assert!(game.take_retired_replay(PlayerId(2)).unwrap().is_none());
    let cap = game.retained_capability().unwrap();
    let restart = game
        .prepare_retained_transaction(
            PracticeTransactionRequest::RestartOriginal {
                id: 132,
                generation: cap.practice.generation,
                expected: cap.rate,
            },
            false,
        )
        .unwrap();
    audio
        .try_restart_original_next(
            132,
            cap.practice.generation,
            cap.rate.revision,
            restart.preparation.unwrap(),
        )
        .unwrap();
    game.acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 132, Ok(()))
        .unwrap();
    render_local(&mut game, &mut audio, 6, 1);
    let event = audio.poll_retained_event().unwrap().unwrap();
    game.retain_output_event(event).unwrap();
    game.record_audio_prefix(host(7 * NS)).unwrap();
    assert_eq!(
        game.service_retained_event(&mut inputs, host(7 * NS), raw(7 * NS))
            .unwrap(),
        None,
        "the failed member alone must still backpressure the whole fresh cohort"
    );
    assert_eq!(game.retained_capability().unwrap(), cap);
    for (index, current) in expected_current.iter().enumerate() {
        assert_eq!(
            game.effective_header(PlayerId(index as u32 + 1)).unwrap(),
            &current.header
        );
    }
    assert_eq!(audio.poll_retained_event().unwrap(), Some(event));
    let recovered = game.take_retired_replay(PlayerId(1)).unwrap().unwrap();
    assert_eq!(recovered, expected_bytes[0]);
    assert_eq!(
        decode_replay(&recovered, fixture::limits()).unwrap(),
        expected[0]
    );
    assert!(game.take_retired_replay(PlayerId(1)).unwrap().is_none());
    assert_eq!(
        game.service_retained_event(&mut inputs, host(7 * NS), raw(7 * NS))
            .unwrap(),
        Some(event.serial)
    );
    game.acknowledge_retained_event(event.serial).unwrap();
    audio.acknowledge_retained_event(event.serial).unwrap();
    for (index, current) in expected_current.iter().enumerate() {
        let player = PlayerId(index as u32 + 1);
        let bytes = game.take_retired_replay(player).unwrap().unwrap();
        assert_eq!(&decode_replay(&bytes, fixture::limits()).unwrap(), current);
        assert!(game.take_retired_replay(player).unwrap().is_none());
    }
    let final_headers: Vec<_> = [PlayerId(1), PlayerId(2)]
        .map(|player| game.effective_header(player).unwrap().clone())
        .into();
    game.fail();
    for (index, header) in final_headers.into_iter().enumerate() {
        let player = PlayerId(index as u32 + 1);
        let bytes = game.take_replay(player).unwrap().unwrap();
        assert_eq!(
            decode_replay(&bytes, fixture::limits()).unwrap(),
            ReplayFile::new(header, vec![])
        );
        assert!(game.take_replay(player).unwrap().is_none());
    }
}
