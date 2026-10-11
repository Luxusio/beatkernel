//! Full, bounded virtual traversal through the production solo audio player.
use app::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    bgm::{BgmConfig, BgmFeeder},
    gameplay_competition::NoopSoloCompetition,
    gameplay_presentation::GameplayDevice,
    gauge::BmsGauge,
    live_pause::LivePauseObservation,
    local_input::InputMerger,
    local_runtime::SoloRuntime,
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::{
        AudioGameplayConfig, AudioGameplaySession, GameplaySession, InputBatch,
        NativeGameplayConfig, NativeGameplayResult,
    },
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_pump_control::NativePumpControl,
    play_policy::ResolvedPlayPolicy,
    playback_pause::NativePause,
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample,
        RenderReport, SampleBank, SampleId, VoiceId, command_queue,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeEvent, JudgeOutcome, JudgeStage},
    replay::{
        ReplayOperation, ReplayRecord,
        codec::{ReplayCodecLimits, ReplayFile, decode_replay},
    },
    runtime::{RuntimeProcessingClock, RuntimeReport, SoundBinding},
    telemetry::InputDeliveryTelemetry,
    time::{
        ClockDomainId, ClockPair, ClockPoint, Duration, ExtrapolationPolicy, Timestamp,
        presentation::PresentationEstimator,
    },
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime as app;
use beatkernel_platform::audio::presentation::validation::{
    NativePresentationValidator, OriginalNativePresentationEvidence,
};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

const MS: i64 = 1_000_000;
const HOUR: u64 = 3_600_000;
const LOGICAL: i64 = 5_000_000_000;
const MAX_OPERATIONS: usize = 200_000;
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub hours: usize,
    pub block_frames: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            hours: 20,
            block_frames: 8192,
        }
    }
}
impl Options {
    pub fn validate(self) -> Result<(), String> {
        if !(1..=168).contains(&self.hours) || !(128..=65536).contains(&self.block_frames) {
            return Err("hours must be 1..168 and block-frames 128..65536".into());
        }
        let frames = self
            .hours
            .checked_mul(HOUR as usize)
            .ok_or("horizon overflow")?;
        let blocks = frames
            .checked_add(self.block_frames - 1)
            .ok_or("block count overflow")?
            / self.block_frames;
        let bound = blocks
            .checked_mul(2)
            .and_then(|v| v.checked_add(8 * self.hours))
            .and_then(|v| v.checked_add(64))
            .ok_or("operation budget overflow")?;
        if bound > MAX_OPERATIONS {
            return Err("combined render/record allowance exceeds 200000".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    LateInput,
    IncompleteInput,
    NativeRegression,
    MissingProgress,
    UnacknowledgedPause,
    HostPublication,
    OperationBudget,
}
#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    pub physical_frame: u64,
    pub playback_frame: u64,
    pub paused: bool,
}
#[derive(Debug)]
pub struct Report {
    pub options: Options,
    pub source: beatkernel_bms::BmsChart,
    pub file: ReplayFile,
    pub bytes: Vec<u8>,
    pub live_results: Vec<JudgeEvent>,
    pub reconstructed_results: Vec<JudgeEvent>,
    pub physical_frames: u64,
    pub playback_frames: u64,
    pub verified_samples: u64,
    pub render_calls: usize,
    pub record_operations: usize,
    pub input_edges: usize,
    pub advance_records: usize,
    pub repair_records: usize,
    pub pcm_hash: u64,
    pub replay_hash: u64,
    pub checkpoints: Vec<Checkpoint>,
    pub pause_states: Vec<PauseState>,
    pub mixer_identity_before: usize,
    pub mixer_identity_after: usize,
    pub pending_inputs: usize,
    pub pending_commands: usize,
    pub active_voices: usize,
    pub cached_report_reads: usize,
    pub pause_render_count: usize,
    pub key_samples: usize,
    pub bgm_samples: usize,
    pub completed: bool,
    pub section_end: Timestamp,
}
#[derive(Debug)]
pub struct FailureEvidence {
    pub error: String,
    pub physical_frames: u64,
    pub playback_frames: u64,
    pub render_calls: usize,
    pub record_operations: usize,
    pub input_edges: usize,
    pub acquired_input_edges: usize,
    pub pending_inputs: usize,
    pub live_results: Vec<JudgeEvent>,
    pub records: Vec<ReplayRecord>,
    pub pause_render_count: usize,
    pub acquisition_cut_missing: bool,
    pub native_observation_missing: bool,
    pub pause_callback_missing: bool,
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(frame: u64) -> ClockPoint {
    point(1, frame as i64 * MS)
}
fn raw(frame: u64) -> ClockPoint {
    point(2, frame as i64 * MS)
}
fn pair(frame: u64) -> ClockPair {
    ClockPair {
        source: raw(frame),
        target: host(frame),
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(
        32 * 1024 * 1024,
        MAX_OPERATIONS,
        4096,
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap()
}
fn hash(mut state: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        state = (state ^ u64::from(*byte)).wrapping_mul(FNV_PRIME);
    }
    state
}
fn source(hours: usize) -> NativeGameplayResult<beatkernel_bms::BmsChart> {
    let mut text = String::from("#BPM 60\n#TOTAL 100\n#WAV01 key.wav\n#WAV02 bgm.wav\n");
    for hour in 0..hours {
        text.push_str(&format!("#{hour:03}02:900\n#{hour:03}01:0002\n"));
        text.push_str(&if hour % 2 == 0 {
            format!("#{hour:03}11:00010000\n")
        } else {
            format!("#{hour:03}51:00010001\n")
        });
    }
    Ok(beatkernel_bms::parse(&text, Default::default())?)
}
#[derive(Default)]
struct Shared {
    desired_pause: bool,
    stalled: bool,
    records: usize,
    live: Vec<JudgeEvent>,
    pauses: Vec<PauseState>,
    section_end: Option<Timestamp>,
}
struct Host {
    shared: Rc<RefCell<Shared>>,
    fault: Option<Fault>,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        false
    }
    fn pause_requested(&self) -> bool {
        self.shared.borrow().desired_pause
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, state: PauseState) {
        self.shared.borrow_mut().pauses.push(state);
    }
    fn publish_section_end(&mut self, end: Timestamp) {
        self.shared.borrow_mut().section_end = Some(end);
    }
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        let mut s = self.shared.borrow_mut();
        s.records += report.bound_inputs.len()
            + usize::from(report.input.is_none() && report.judge_error.is_none());
        s.live.extend(report.judge_events.iter().copied());
        if self.fault == Some(Fault::HostPublication) {
            return Err("injected host publication refusal".into());
        }
        Ok(())
    }
    fn publish_local_reports(
        &mut self,
        _: &[app::local_runtime::PlayerReport],
    ) -> NativeGameplayResult<()> {
        Err("solo traversal cannot publish cohort reports".into())
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
}
struct Device {
    mixer: Box<Mixer>,
    basis: beatkernel::audio::OutputFrameBasis,
    buffer: Vec<f32>,
    options: Options,
    shared: Rc<RefCell<Shared>>,
    fault: Option<Fault>,
    report: Option<RenderReport>,
    physical: u64,
    playback: u64,
    renders: usize,
    reads: usize,
    pcm_hash: u64,
    checkpoints: Vec<Checkpoint>,
    pause_at: u64,
    pause_started: bool,
    pause_renders: usize,
    edges: Vec<(u64, ButtonState)>,
    emitted: usize,
    expected: Vec<(u64, f32)>,
    next_sample: usize,
    key_samples: usize,
    bgm_samples: usize,
}
impl Device {
    fn step_frames(&self) -> usize {
        if self.mixer.pause_requested() || self.playback >= self.options.hours as u64 * HOUR {
            return 1;
        }
        if self.report.is_none() {
            return 1;
        }
        let mut next = (self.playback + self.options.block_frames as u64)
            .min(self.options.hours as u64 * HOUR);
        if let Some((frame, _)) = self.edges.get(self.emitted) {
            if *frame > self.playback {
                next = next.min(*frame);
            }
        }
        if let Some((frame, _)) = self.expected.get(self.next_sample) {
            if *frame > self.playback {
                next = next.min(*frame);
            }
        }
        if !self.pause_started && self.pause_at > self.playback {
            next = next.min(self.pause_at);
        }
        (next - self.playback).max(1) as usize
    }
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn observe(&mut self, _: &mut PresentationEstimator) -> NativeGameplayResult<()> {
        Err("legacy observation forbidden".into())
    }
    fn observe_audio(&mut self, p: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        let limit = if self.fault == Some(Fault::OperationBudget) {
            4
        } else {
            MAX_OPERATIONS
        };
        if self.renders + self.shared.borrow().records >= limit {
            return Err("combined runtime operation budget exhausted".into());
        }
        // A missing callback cannot acknowledge a producer pause request. Keep
        // the last actual running report; never render and conceal a real ACK.
        if (self.fault == Some(Fault::UnacknowledgedPause) && self.mixer.pause_requested())
            || (self.fault == Some(Fault::IncompleteInput) && self.shared.borrow().stalled)
        {
            self.shared.borrow_mut().stalled = true;
            return Ok(());
        }
        let frames = self.step_frames();
        let report = self.mixer.render(&mut self.buffer[..frames])?;
        if report.start_frame != self.physical || report.playback_start_frame != self.playback {
            return Err("noncontiguous original output frame coverage".into());
        }
        for (offset, actual) in self.buffer[..frames].iter().enumerate() {
            let playback = report.playback_start_frame + offset as u64;
            let active = offset < report.playback_frames;
            let expected = if active
                && self
                    .expected
                    .get(self.next_sample)
                    .is_some_and(|event| event.0 == playback)
            {
                let value = self.expected[self.next_sample].1;
                self.next_sample += 1;
                if value == 0.25 {
                    self.key_samples += 1;
                } else {
                    self.bgm_samples += 1;
                }
                value
            } else {
                0.0
            };
            if actual.to_bits() != expected.to_bits() {
                return Err(format!("PCM mismatch at physical frame {} playback {}: actual={actual}, expected={expected}", self.physical + offset as u64, playback).into());
            }
            self.pcm_hash = hash(self.pcm_hash, &actual.to_bits().to_le_bytes());
        }
        self.physical += frames as u64;
        self.playback += report.playback_frames as u64;
        self.renders += 1;
        self.report = Some(report);
        if self.playback == self.pause_at && !self.pause_started {
            self.pause_started = true;
            self.shared.borrow_mut().desired_pause = true;
        }
        if report.paused && report.playback_end_physical_frame.is_none() {
            self.pause_renders += 1;
            if self.pause_renders == 3 {
                self.shared.borrow_mut().desired_pause = false;
            }
        }
        if self.checkpoints.len() < 512
            && (self.renders <= 4 || self.renders.is_multiple_of(256) || report.paused)
        {
            self.checkpoints.push(Checkpoint {
                physical_frame: self.physical,
                playback_frame: self.playback,
                paused: report.paused,
            });
        }
        let frame = if self.fault == Some(Fault::NativeRegression) && self.renders == 3 {
            0
        } else {
            self.physical
        };
        if self.fault == Some(Fault::MissingProgress) && self.renders > 2 {
            // The callback completed, but no native presentation correlation
            // arrived. Original authority must refuse its stale association.
            self.shared.borrow_mut().stalled = true;
        } else {
            p.admit(NativeAudioSnapshot {
                epoch: 0,
                basis: self.basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(pair(frame)),
            })?;
        }
        Ok(())
    }
    fn audio_pause_observation(
        &mut self,
        p: &NativeAudioPresentation,
        _: ClockPoint,
    ) -> NativeGameplayResult<LivePauseObservation> {
        Ok(LivePauseObservation::Point(
            p.latest_record()
                .ok_or("missing original pause pair")?
                .pair(),
        ))
    }
    fn observe_audio_end(
        &mut self,
        end: &mut NativeEnd,
        p: &NativeAudioPresentation,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        Ok(end.observe(
            report,
            p.latest_record().ok_or("missing original end pair")?.pair(),
        )?)
    }
    fn publish_paused_audio_output(
        &mut self,
        _: app::gameplay_presentation::GameplayAudioOutputContext<'_>,
        _: ClockPoint,
    ) -> NativeGameplayResult<bool> {
        Ok(false)
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        self.reads += 1;
        Ok(self.report)
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(host(self.physical))
    }
    fn acquire(
        &mut self,
        pending: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        let report = self.report.ok_or("missing acquisition grid")?;
        if !report.paused {
            while let Some((at, state)) = self.edges.get(self.emitted).copied() {
                if at > self.playback {
                    break;
                }
                if at < self.playback {
                    return Err("authored input boundary was skipped".into());
                }
                let stamp = if self.fault == Some(Fault::LateInput) {
                    host(0)
                } else {
                    host(self.physical)
                };
                pending.push_back(PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(1001), stamp, self.emitted as u64),
                    control: PhysicalControlId::keyboard(7),
                    state,
                }));
                self.emitted += 1;
            }
        }
        if self.fault == Some(Fault::IncompleteInput) && self.emitted > 0 {
            self.shared.borrow_mut().stalled = true;
            return Ok(InputBatch {
                completed_through: None,
                backlog: true,
                closed: false,
            });
        }
        Ok(InputBatch {
            completed_through: Some(host(self.physical)),
            backlog: false,
            closed: false,
        })
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &PresentationEstimator,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        Err("legacy end forbidden".into())
    }
    fn seed_resume(
        &mut self,
        _: &mut PresentationEstimator,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        Err("legacy resume forbidden".into())
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        Ok(raw(self.playback))
    }
}
struct Control {
    waits: usize,
    stalled_waits: usize,
    shared: Rc<RefCell<Shared>>,
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        Err("virtual traversal must not read wall clock".into())
    }
    fn checked_add(now: u64, duration: std::time::Duration) -> Option<u64> {
        now.checked_add(u64::try_from(duration.as_nanos()).ok()?)
    }
    fn wait(&mut self, _: std::time::Duration) -> NativeGameplayResult<()> {
        self.waits += 1;
        if self.shared.borrow().stalled {
            self.stalled_waits += 1;
        }
        if self.waits > MAX_OPERATIONS || self.stalled_waits > 64 {
            Err("finite original-port nonprogress wait budget exhausted".into())
        } else {
            Ok(())
        }
    }
}
struct Evidence {
    source: beatkernel_bms::BmsChart,
    device: Device,
    capture: LiveReplayCapture,
    shared: Rc<RefCell<Shared>>,
    pending: usize,
    mixer_before: usize,
    outcome: NativeGameplayResult<bool>,
}
fn execute(options: Options, fault: Option<Fault>) -> NativeGameplayResult<Evidence> {
    options.validate()?;
    let chart = source(options.hours)?;
    let policy = ResolvedPlayPolicy::builtin(0, 0, 0)?;
    let end_frame = options.hours as u64 * HOUR;
    let end_song = Timestamp::from_nanos(end_frame as i64 * MS);
    let compiled = chart.compile()?;
    if compiled.bgm.len() != options.hours {
        return Err("compiled BGM cue count differs from the pinned hourly source".into());
    }
    let mut bgm_commands = Vec::with_capacity(compiled.bgm.len());
    for (hour, cue) in compiled.bgm.iter().enumerate() {
        let expected_at = Timestamp::from_nanos((hour as i64 * HOUR as i64 + 1_800_000) * MS);
        if cue.sample != SampleId(2) || cue.at != expected_at {
            return Err(
                "compiled BGM sample, order or timing differs from the pinned hourly source".into(),
            );
        }
        bgm_commands.push(AudioCommand::Play {
            voice: VoiceId(1000 + hour as u64),
            sample: cue.sample,
            at: cue.at,
            gain: 1.0,
        });
    }
    let judge = JudgeEngine::new(compiled.chart, chart.rules(), policy.judge().clone())?;
    let capture = LiveReplayCapture::new_with_policy(
        &judge,
        ClockDomainId(3),
        limits(),
        Timestamp::ZERO,
        0,
        Some(end_song),
        beatkernel_bms::BmsInputMode::ButtonOnly,
        None,
        &policy,
    )?;
    let mut sounds = Vec::new();
    for object in judge.chart().objects() {
        for stage in if object.time.end.is_some() {
            vec![JudgeStage::HoldHead, JudgeStage::HoldTail]
        } else {
            vec![JudgeStage::Instant]
        } {
            sounds.push(SoundBinding {
                object: object.id,
                stage,
                sample: SampleId(1),
                voice: VoiceId(object.id.0 * 2 + u64::from(stage == JudgeStage::HoldTail)),
                gain: 1.0,
            });
        }
    }
    let format = AudioFormat::new(1000, 1)?;
    let pcm_limits = PcmLimits::new(4, 8, 2)?;
    let mut bank = SampleBank::new(format, pcm_limits)?;
    for (id, value) in [(1, 0.25), (2, 0.125)] {
        bank.insert(
            SampleId(id),
            PcmSample::new(format, vec![value], pcm_limits)?,
        )?;
    }
    let (producer, consumer) = command_queue(32)?;
    let mixer = Box::new(Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(32, 8, 32, options.block_frames, 32)?,
        )
        .with_playback_end_frame(end_frame),
        bank,
        consumer,
    )?);
    let mixer_before = mixer.as_ref() as *const Mixer as usize;
    let basis = mixer.output_frame_basis();
    let shared = Rc::new(RefCell::new(Shared::default()));
    let mut expected = Vec::new();
    let mut edges = Vec::new();
    for hour in 0..options.hours {
        let start = hour as u64 * HOUR;
        let head = start + 900_000;
        let tail = start + if hour % 2 == 0 { 900_001 } else { 2_700_000 };
        edges.extend([(head, ButtonState::Down), (tail, ButtonState::Up)]);
        expected.push((head, 0.25));
        if hour % 2 == 1 {
            expected.push((tail, 0.25));
        }
        let bgm_at = start + 1_800_000;
        expected.push((bgm_at, 0.125));
    }
    expected.sort_by_key(|event| event.0);
    let mut device = Device {
        mixer,
        basis,
        buffer: vec![0.0; options.block_frames],
        options,
        shared: shared.clone(),
        fault,
        report: None,
        physical: 0,
        playback: 0,
        renders: 0,
        reads: 0,
        pcm_hash: FNV_OFFSET,
        checkpoints: vec![],
        pause_at: end_frame / 2 + 123,
        pause_started: false,
        pause_renders: 0,
        edges,
        emitted: 0,
        expected,
        next_sample: 0,
        key_samples: 0,
        bgm_samples: 0,
    };
    let mut p = NativeAudioPresentation::new(
        AudioAuthority::new(
            AudioAuthorityConfig {
                history_capacity: 64,
                max_observation_age: Duration::from_nanos((options.block_frames as i64 + 100) * MS),
                input_extrapolation: ExtrapolationPolicy::Forbid,
                max_input_ahead: Duration::ZERO,
            },
            AudioAuthorityEpoch {
                id: 0,
                stream_origin: raw(0),
                logical_origin: point(3, LOGICAL),
                host_domain: ClockDomainId(1),
            },
        )?,
        NativePresentationValidator::new(0, raw(0), ClockDomainId(1)),
    )?;
    p.admit(NativeAudioSnapshot {
        epoch: 0,
        basis: device.basis,
        evidence: OriginalNativePresentationEvidence::SuppliedPair(pair(0)),
    })?;
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Exact(DeviceId(1001)),
        physical: PhysicalControlId::keyboard(7),
        game_control: GameControlId(0x11),
    }])?;
    let mut runtime = SoloRuntime::new(
        ClockDomainId(3),
        ClockDomainId(2),
        Transport::new(
            Timestamp::from_nanos(LOGICAL),
            Timestamp::ZERO,
            Rate::NORMAL,
        ),
        bindings,
        judge,
        producer,
        sounds,
        0,
    )?;
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    runtime.set_song_end(end_song)?;
    let mut bgm = BgmFeeder::new(
        bgm_commands,
        BgmConfig {
            output_origin: raw(0),
            sample_rate: 1000,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(options.block_frames as i64 * MS),
            max_pending: 8,
        },
    )?;
    let mut pause =
        NativePause::new(raw(0), ClockDomainId(1), 1000)?.with_playback_end_frame(end_frame)?;
    let mut end = Some(NativeEnd::new(raw(0), ClockDomainId(1), 1000, end_frame)?);
    let mut capture = Some(capture);
    let mut completion = None;
    let mut competition: Option<NoopSoloCompetition> = None;
    let mut gauge = BmsGauge::new(policy.gauge().try_copy()?);
    let mut delivery = InputDeliveryTelemetry::new(0, ClockDomainId(1))?;
    let mut pre = 0;
    let mut merger = InputMerger::new_dynamic(ClockDomainId(1), host(0), 4096, 32)?;
    let mut host_port = Host {
        shared: shared.clone(),
        fault,
    };
    let mut control = Control {
        waits: 0,
        stalled_waits: 0,
        shared: shared.clone(),
    };
    let outcome = app::native_gameplay::run_gameplay_audio_with_result_and_ports(
        &mut device,
        AudioGameplaySession {
            session: GameplaySession {
                runtime: &mut runtime,
                gauge: &mut gauge,
                bgm: &mut bgm,
                discipline: &mut p,
                pause: &mut pause,
                end: &mut end,
                completion: &mut completion,
                capture: &mut capture,
                competition: &mut competition,
                delivery: &mut delivery,
                pre_origin_inputs: &mut pre,
            },
            merger: &mut merger,
        },
        AudioGameplayConfig {
            gameplay: NativeGameplayConfig {
                origin: host(0),
                stream_origin: raw(0),
                playback_origin: raw(0),
                song_origin: Timestamp::ZERO,
                sample_rate: 1000,
                end_song: Some(end_song),
                advance_lag: Duration::ZERO,
                seconds: None,
                pause_supported: true,
                logical_schedule: true,
            },
            section_start: Timestamp::ZERO,
        },
        &mut control,
        &mut host_port,
    )
    .map(|result| result.is_some());
    Ok(Evidence {
        source: chart,
        device,
        capture: capture.ok_or("capture owner disappeared")?,
        shared,
        pending: merger.pending(),
        mixer_before,
        outcome,
    })
}
pub fn run_fault(options: Options, fault: Fault) -> NativeGameplayResult<FailureEvidence> {
    let e = execute(options, Some(fault))?;
    let error = e
        .outcome
        .err()
        .ok_or("fault unexpectedly completed")?
        .to_string();
    let shared = e.shared.borrow();
    let input_edges = e
        .capture
        .records()
        .iter()
        .filter(|record| matches!(record.operation, ReplayOperation::Input(_)))
        .count();
    Ok(FailureEvidence {
        error,
        physical_frames: e.device.physical,
        playback_frames: e.device.playback,
        render_calls: e.device.renders,
        record_operations: e.capture.records().len(),
        input_edges,
        acquired_input_edges: e.device.emitted,
        pending_inputs: e.pending,
        live_results: shared.live.clone(),
        records: e.capture.records().to_vec(),
        pause_render_count: e.device.pause_renders,
        acquisition_cut_missing: fault == Fault::IncompleteInput && shared.stalled,
        native_observation_missing: fault == Fault::MissingProgress && shared.stalled,
        pause_callback_missing: fault == Fault::UnacknowledgedPause && shared.stalled,
    })
}
pub fn run(options: Options) -> NativeGameplayResult<Report> {
    let e = execute(options, None)?;
    let completed = e.outcome?;
    let d = e.device;
    let s = e.shared.borrow();
    let bytes = e.capture.into_bytes()?;
    let file = decode_replay(&bytes, limits())?;
    let restored = app::replay_playback::reconstruct_section(&e.source, file.clone(), limits())?;
    let mut inputs = 0;
    let mut advances = 0;
    let mut repairs = 0;
    for record in &file.records {
        match &record.operation {
            ReplayOperation::Advance => advances += 1,
            ReplayOperation::Input(input) => {
                if input.physical.meta().original_clock_point.is_some() {
                    inputs += 1;
                } else {
                    repairs += 1;
                }
            }
        }
    }
    let last = d.report.ok_or("missing terminal render evidence")?;
    let end = Timestamp::from_nanos(options.hours as i64 * HOUR as i64 * MS);
    if !completed
        || d.playback != options.hours as u64 * HOUR
        || d.next_sample != d.expected.len()
        || d.pause_renders != 3
        || inputs != 2 * options.hours
        || repairs != 0
        || e.pending != 0
        || last.pending_commands != 0
        || last.active_voices != 0
        || s.live != restored.results()
        || s.live.len() != options.hours + options.hours / 2
        || s.live
            .iter()
            .any(|event| !matches!(event.outcome, JudgeOutcome::Hit { .. }))
        || d.renders + file.records.len() > MAX_OPERATIONS
        || s.section_end != Some(end)
    {
        return Err(
            "full timeline, original judge/capture/pause/end/drain invariant failed".into(),
        );
    }
    let replay_hash = hash(FNV_OFFSET, &bytes);
    Ok(Report {
        options,
        source: e.source,
        file,
        bytes,
        live_results: s.live.clone(),
        reconstructed_results: restored.results().to_vec(),
        physical_frames: d.physical,
        playback_frames: d.playback,
        verified_samples: d.physical,
        render_calls: d.renders,
        record_operations: inputs + repairs + advances,
        input_edges: inputs,
        advance_records: advances,
        repair_records: repairs,
        pcm_hash: d.pcm_hash,
        replay_hash,
        checkpoints: d.checkpoints,
        pause_states: s.pauses.clone(),
        mixer_identity_before: e.mixer_before,
        mixer_identity_after: d.mixer.as_ref() as *const Mixer as usize,
        pending_inputs: e.pending,
        pending_commands: last.pending_commands,
        active_voices: last.active_voices,
        cached_report_reads: d.reads,
        pause_render_count: d.pause_renders,
        key_samples: d.key_samples,
        bgm_samples: d.bgm_samples,
        completed,
        section_end: end,
    })
}
