//! Deterministic software ports around the original retained gameplay owners.
use app::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    bgm::{BgmConfig, BgmFeeder},
    competition::ScoreSummary,
    gameplay::output::{
        application::{owner::GameplayOutputOwner, requests::GameplayOutputUi},
        domain::control::{OutputCapability, OutputControls, OutputReply, OutputRequest},
        ports::{OriginalNativeOutputBackend, OutputReplacementBackend, OutputUiPort},
    },
    gameplay_competition::{NoopGroupCompetition, NoopSoloCompetition},
    gameplay_presentation::{GameplayAudioOutputContext, GameplayDevice, GameplayPauseControl},
    gauge::BmsGauge,
    live_pause::LivePauseObservation,
    local_input::InputMerger,
    local_players::PlayerId,
    local_runtime::{MemberConfig, PlayerReport, RuntimeGroup, SoloRuntime},
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_cohort::{AudioCohortSession, GameplayPlayerState},
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::{
        AudioGameplayConfig, AudioGameplaySession, GameplaySession, InputBatch,
        NativeGameplayConfig, NativeGameplayResult,
    },
    native_gameplay_host::{
        NativeGameplayDiagnostic, NativeGameplayHost, NoopGameplayHost, PauseState,
    },
    native_pump_control::NativePumpControl,
    play_policy::ResolvedPlayPolicy,
    playback_pause::NativePause,
    practice_control::{PracticeAction, PracticeCapability, PracticeReply, PracticeRequest},
    practice_playback::{PracticeMember, PracticePlayback, PracticeRecordingPort},
    replay_capture::LiveReplayCapture,
    session_launch::SessionLaunch,
};
use beatkernel::{
    audio::{
        command_queue, practice_queue, AudioCommand, AudioFormat, AudioLimits, CommandScope, Mixer,
        MixerConfig, OutputFrameBasis, OutputOpenFailure, PcmLimits, PcmSample, PracticeCue,
        PracticeLimits, PracticeRegion, PreparedPracticeProgram, RenderReport, SampleBank,
        SampleId, SoftwareOutputState, StoppedMixerSource, VoiceId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeEvent, JudgeOutcome, JudgeStage},
    replay::{
        codec::{ReplayCodecLimits, ReplayFile},
        ReplayOperation,
    },
    runtime::{RuntimeProcessingClock, RuntimeReport, SoundBinding},
    telemetry::InputDeliveryTelemetry,
    time::{
        presentation::PresentationEstimator, ClockDomainId, ClockPair, ClockPoint, Duration,
        ExtrapolationPolicy, Timestamp,
    },
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime as app;
use beatkernel_platform::audio::presentation::validation::{
    NativePresentationValidator, OriginalNativePresentationEvidence,
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    path::{Path, PathBuf},
    rc::Rc,
};

const MS: i64 = 1_000_000;
const LOGICAL: i64 = 5_000_000_000;
const MAX_NOTES: usize = 262_144;
const MAX_RENDER_FRAMES: usize = 64 * (40 * 64 + 40) + 16;
const MAX_RECORDS: usize = 64 * (MAX_RENDER_FRAMES + 2 * 64 * 64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub members: usize,
    pub cycles: usize,
    pub notes_per_member: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            members: 2,
            cycles: 8,
            notes_per_member: 8,
        }
    }
}
impl Options {
    pub fn validate(self) -> Result<(), String> {
        if !(1..=64).contains(&self.members)
            || !(1..=64).contains(&self.cycles)
            || !(1..=64).contains(&self.notes_per_member)
        {
            return Err("members, cycles and notes-per-member must each be 1..64".into());
        }
        let notes = self
            .members
            .checked_mul(self.cycles)
            .and_then(|v| v.checked_mul(self.notes_per_member))
            .ok_or("note budget overflow")?;
        if notes > MAX_NOTES {
            return Err("aggregate note budget exceeds 262144".into());
        }
        // All allocations below follow these cold aggregate bounds. Recording
        // retains at most one Advance per source frame plus physical edges.
        let frames = self
            .cycles
            .checked_mul(40 * self.notes_per_member + 40)
            .and_then(|frames| frames.checked_add(16))
            .ok_or("render budget overflow")?;
        let records = self
            .members
            .checked_mul(frames + 2 * self.cycles * self.notes_per_member)
            .ok_or("record budget overflow")?;
        if frames > MAX_RENDER_FRAMES || records > MAX_RECORDS {
            return Err("aggregate render/record budget exceeded".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct CaptureEvidence {
    pub player: PlayerId,
    pub device: DeviceId,
    pub attempt: usize,
    pub path: PathBuf,
    pub file: ReplayFile,
    pub bytes: Vec<u8>,
    pub reconstructed_results: Vec<JudgeEvent>,
    pub live_results: Vec<JudgeEvent>,
    pub judge_hash: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct AttemptEvidence {
    pub attempt: usize,
    pub generation: u64,
    pub physical_frame: u64,
    pub playback_frame: u64,
    pub song_start: Timestamp,
}
#[derive(Clone, Debug)]
pub struct ReplacementEvidence {
    pub mixer_identity_before: usize,
    pub mixer_identity_after: usize,
    pub epoch_before: u64,
    pub epoch_after: u64,
    pub basis: OutputFrameBasis,
    pub paused_render_reports: Vec<RenderReport>,
    pub snapshots: Vec<NativeAudioSnapshot>,
    pub retire_calls: usize,
    pub open_calls: usize,
    pub premature_recovery_refused: bool,
}
#[derive(Debug)]
pub struct Report {
    pub options: Options,
    pub source: beatkernel_bms::BmsChart,
    pub captures: Vec<CaptureEvidence>,
    pub pcm: Vec<f32>,
    pub render_reports: Vec<RenderReport>,
    pub attempt_starts: Vec<AttemptEvidence>,
    pub generations: Vec<u64>,
    pub applied_generations: Vec<u64>,
    pub committed_generations: Vec<u64>,
    pub pause_states: Vec<PauseState>,
    pub transitions: usize,
    pub authored_edges: usize,
    pub captured_input_edges: usize,
    pub advance_records: usize,
    pub repair_records: usize,
    pub mixer_identity_before: usize,
    pub mixer_identity_after: usize,
    pub replacement: ReplacementEvidence,
    pub replacement_refusal: String,
    pub refusal_mapping_calls: usize,
    pub refusal_retire_calls: usize,
    pub pending_inputs: usize,
    pub pending_commands: usize,
    pub active_voices: usize,
    pub sentinel_injected: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Archive,
    Presentation,
    LateFrontier,
    UnacknowledgedPause,
    ReplacementTimeout,
}
#[derive(Debug)]
pub struct FailureEvidence {
    pub error: String,
    pub generation: u64,
    pub captures: Vec<CaptureEvidence>,
    pub applied_generations: Vec<u64>,
    pub committed_generations: Vec<u64>,
    pub pending_inputs: usize,
}

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
fn raw(ns: i64) -> ClockPoint {
    point(2, ns)
}
fn logical(ns: i64) -> ClockPoint {
    point(3, LOGICAL + ns)
}
fn pair(ns: i64) -> ClockPair {
    ClockPair {
        source: raw(ns),
        target: host(ns),
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(2_097_152, 8192, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn source(notes: usize) -> NativeGameplayResult<beatkernel_bms::BmsChart> {
    let mut text = String::from("#BPM 6000\n#TOTAL 100\n#WAV01 key.wav\n");
    for i in 0..notes {
        if i % 2 == 0 {
            text.push_str(&format!("#{i:03}11:0001\n"));
        } else {
            text.push_str(&format!("#{i:03}51:00010001\n"));
        }
    }
    Ok(beatkernel_bms::parse(&text, Default::default())?)
}
fn bindings(device: Option<DeviceId>) -> NativeGameplayResult<BindingMap> {
    Ok(BindingMap::from_bindings([Binding {
        device: device.map_or(DeviceSelector::Any, DeviceSelector::Exact),
        physical: PhysicalControlId::keyboard(7),
        game_control: GameControlId(0x11),
    }])?)
}
fn judge(
    chart: &beatkernel_bms::BmsChart,
    policy: &ResolvedPlayPolicy,
) -> NativeGameplayResult<JudgeEngine> {
    Ok(JudgeEngine::new(
        chart.compile()?.chart,
        chart.rules(),
        policy.judge().clone(),
    )?)
}
fn sounds(
    chart: &beatkernel_bms::BmsChart,
    member: usize,
) -> NativeGameplayResult<Vec<SoundBinding>> {
    let mut sounds = Vec::new();
    for object in chart.compile()?.chart.objects() {
        for stage in if object.time.end.is_some() {
            vec![JudgeStage::HoldHead, JudgeStage::HoldTail]
        } else {
            vec![JudgeStage::Instant]
        } {
            sounds.push(SoundBinding {
                object: object.id,
                stage,
                sample: SampleId(1),
                voice: VoiceId(
                    (member as u64 + 1) * 1000
                        + object.id.0 * 2
                        + u64::from(stage == JudgeStage::HoldTail),
                ),
                gain: 1.0,
            });
        }
    }
    Ok(sounds)
}
fn presentation() -> NativeGameplayResult<NativeAudioPresentation> {
    Ok(NativeAudioPresentation::new(
        AudioAuthority::new(
            AudioAuthorityConfig {
                // Ten-frame idle acquisition batches need their original lower/upper
                // brackets retained until the pump commits the covered prefix. The
                // existing authority retires older pairs only after that real commit.
                history_capacity: 32,
                max_observation_age: Duration::from_nanos(100 * MS),
                input_extrapolation: ExtrapolationPolicy::Forbid,
                max_input_ahead: Duration::ZERO,
            },
            AudioAuthorityEpoch {
                id: 0,
                stream_origin: raw(0),
                logical_origin: logical(0),
                host_domain: ClockDomainId(1),
            },
        )?,
        NativePresentationValidator::new(0, raw(0), ClockDomainId(1)),
    )?)
}
fn config(end: i64) -> AudioGameplayConfig {
    AudioGameplayConfig {
        gameplay: NativeGameplayConfig {
            origin: host(0),
            stream_origin: raw(0),
            playback_origin: raw(0),
            song_origin: Timestamp::ZERO,
            sample_rate: 1000,
            end_song: Some(Timestamp::from_nanos(end)),
            advance_lag: Duration::ZERO,
            seconds: None,
            pause_supported: true,
            logical_schedule: true,
        },
        section_start: Timestamp::ZERO,
    }
}

struct RetainedMixer {
    mixer: Box<Mixer>,
}
impl RetainedMixer {
    fn identity(&self) -> usize {
        self.mixer.as_ref() as *const Mixer as usize
    }
}
impl SoftwareOutputState for RetainedMixer {
    fn mixer(&self) -> &Mixer {
        &self.mixer
    }
    fn output_frame_basis(&self) -> OutputFrameBasis {
        self.mixer.output_frame_basis()
    }
    fn paused_tail_admissible(&self) -> bool {
        true
    }
}
#[derive(Default)]
struct Observations {
    pcm: Vec<f32>,
    reports: Vec<RenderReport>,
    snapshots: Vec<NativeAudioSnapshot>,
    starts: Vec<AttemptEvidence>,
    opens: usize,
    retires: usize,
}
struct VirtualOutput {
    owner: Option<RetainedMixer>,
    epoch: u64,
    basis: OutputFrameBasis,
    retired: bool,
    report: Option<RenderReport>,
}
impl VirtualOutput {
    fn new(owner: RetainedMixer, epoch: u64) -> Self {
        let basis = owner.output_frame_basis();
        Self {
            owner: Some(owner),
            epoch,
            basis,
            retired: false,
            report: None,
        }
    }
}
impl StoppedMixerSource<RetainedMixer> for VirtualOutput {
    type Error = std::io::Error;
    fn take_stopped_mixer(&mut self) -> Result<Option<RetainedMixer>, Self::Error> {
        if !self.retired {
            return Err(std::io::Error::other("virtual output is not retired"));
        }
        Ok(self.owner.take())
    }
}
struct Backend {
    observations: Rc<RefCell<Observations>>,
    max_frames: usize,
    quiet: bool,
}
impl OutputReplacementBackend<RetainedMixer> for Backend {
    type Presentation = PresentationEstimator;
    type Output = VirtualOutput;
    type Request = ();
    type Error = std::io::Error;
    fn open(
        &mut self,
        _: (),
        owner: RetainedMixer,
        epoch: u64,
    ) -> Result<VirtualOutput, OutputOpenFailure<Self::Error, VirtualOutput, RetainedMixer>> {
        self.observations.borrow_mut().opens += 1;
        Ok(VirtualOutput::new(owner, epoch))
    }
    fn retire(&mut self, output: &mut VirtualOutput) -> Result<(), Self::Error> {
        self.observations.borrow_mut().retires += 1;
        output.retired = true;
        Ok(())
    }
    fn start(&mut self, _: &mut VirtualOutput) -> Result<(), Self::Error> {
        Ok(())
    }
    fn epoch(&self, output: &VirtualOutput) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &VirtualOutput) -> OutputFrameBasis {
        output.basis
    }
    fn observe(
        &mut self,
        _: &mut VirtualOutput,
        _: &mut PresentationEstimator,
    ) -> Result<(), Self::Error> {
        Err(std::io::Error::other(
            "legacy presentation is not a native audio observation",
        ))
    }
    fn render_report(&self, output: &VirtualOutput) -> Result<Option<RenderReport>, Self::Error> {
        Ok(output.report)
    }
}
impl OriginalNativeOutputBackend<RetainedMixer> for Backend {
    fn observe_native(
        &mut self,
        output: &mut VirtualOutput,
    ) -> Result<Option<NativeAudioSnapshot>, Self::Error> {
        let observations = &mut *self.observations.borrow_mut();
        if observations.reports.len() >= self.max_frames {
            return Err(std::io::Error::other("virtual render bound exhausted"));
        }
        if output.retired {
            return Err(std::io::Error::other("retired output cannot render"));
        }
        let mixer = &mut output
            .owner
            .as_mut()
            .ok_or_else(|| std::io::Error::other("missing complete owner"))?
            .mixer;
        let mut pcm = [0.0];
        let report = mixer.render(&mut pcm).map_err(std::io::Error::other)?;
        if let Some(generation) = mixer
            .practice_generation()
            .filter(|_| !mixer.practice_finished())
        {
            if observations
                .starts
                .last()
                .is_none_or(|start| start.generation != generation)
            {
                observations.starts.push(AttemptEvidence {
                    attempt: generation as usize - 1,
                    generation,
                    physical_frame: report.start_frame,
                    playback_frame: report.playback_start_frame,
                    song_start: Timestamp::ZERO,
                });
            }
        }
        let snapshot = NativeAudioSnapshot {
            epoch: output.epoch,
            basis: output.basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(pair(
                (report.start_frame + report.frames as u64) as i64 * MS,
            )),
        };
        output.report = Some(report);
        observations.pcm.extend(pcm);
        observations.reports.push(report);
        observations.snapshots.push(snapshot);
        Ok((!self.quiet).then_some(snapshot))
    }
}

struct Ui(Rc<RefCell<OutputControls>>);
impl OutputUiPort for Ui {
    fn advertise(&mut self, cap: Option<OutputCapability>) -> std::io::Result<()> {
        self.0
            .borrow_mut()
            .advertise(cap)
            .map_err(std::io::Error::other)
    }
    fn take_request(&mut self) -> std::io::Result<Option<OutputRequest>> {
        Ok(self.0.borrow_mut().take_request())
    }
    fn reply(&mut self, reply: &OutputReply) -> std::io::Result<()> {
        self.0
            .borrow_mut()
            .reply(reply)
            .map_err(std::io::Error::other)
    }
    fn pending(&self) -> bool {
        self.0.borrow().pending()
    }
}
#[derive(Default)]
struct Shared {
    generation: u64,
    desired_pause: bool,
    pause_renders: usize,
    live_results: Vec<Vec<JudgeEvent>>,
}
struct Device {
    output: GameplayOutputOwner<Backend, RetainedMixer>,
    observations: Rc<RefCell<Observations>>,
    shared: Rc<RefCell<Shared>>,
    options: Options,
    emitted: Vec<usize>,
    paused_attempts: Vec<bool>,
    fault: Option<Fault>,
    ui: GameplayOutputUi<Ui>,
    ui_controls: Rc<RefCell<OutputControls>>,
    refusal: String,
    mapping_calls: usize,
    refusal_retires: usize,
    last_cut: ClockPoint,
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn observe(&mut self, _: &mut PresentationEstimator) -> NativeGameplayResult<()> {
        Err("legacy device path".into())
    }
    fn observe_audio(&mut self, p: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        self.output.observe_native(p)?;
        Ok(())
    }
    fn audio_pause_observation(
        &mut self,
        p: &NativeAudioPresentation,
        now: ClockPoint,
    ) -> NativeGameplayResult<LivePauseObservation> {
        Ok(self.output.audio_pause_observation(p, now)?)
    }
    fn observe_audio_end(
        &mut self,
        end: &mut NativeEnd,
        p: &NativeAudioPresentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        // The output owner uses its coherently cached original render report.
        self.output.observe_audio_end(end, p)
    }
    fn publish_paused_audio_output(
        &mut self,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
    ) -> NativeGameplayResult<bool> {
        if !self.refusal.is_empty() {
            return Ok(false);
        }
        let before = self.observations.borrow().retires;
        let mut map = |_: &OutputRequest, _: &VirtualOutput| -> Result<(), String> {
            self.mapping_calls += 1;
            Err("practice refusal unexpectedly reached mapping".into())
        };
        let mut applied = |_: &VirtualOutput| -> Result<OutputCapability, String> {
            Err("practice replacement unexpectedly applied".into())
        };
        self.ui
            .service_audio(&mut self.output, context, now, &mut map, &mut applied)?;
        if let Some(reply) = self.ui_controls.borrow_mut().take_reply() {
            self.refusal = reply
                .result
                .err()
                .ok_or("practice replacement unexpectedly succeeded")?;
        }
        self.refusal_retires = self.observations.borrow().retires - before;
        Ok(false)
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.output.render_report())
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        let report = self
            .output
            .render_report()
            .ok_or("missing rendered host grid")?;
        Ok(host(
            (report.start_frame + report.frames as u64) as i64 * MS,
        ))
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        let report = self
            .output
            .render_report()
            .ok_or("missing acquisition grid")?;
        let now = self.host_now()?;
        let old_edges = events.len();
        let mut shared = self.shared.borrow_mut();
        let generation = shared.generation.max(1) as usize;
        let attempt = generation - 1;
        if attempt >= self.options.cycles {
            return Err("unexpected extra attempt".into());
        }
        // Original physical acquisition uses the actual frame's song position.
        // No edge is emitted while the new receipt awaits its owning pump commit.
        let mixer_generation = self
            .output
            .current()
            .and_then(|output| output.owner.as_ref())
            .and_then(|owner| owner.mixer.practice_generation())
            .unwrap_or(1) as usize;
        let start = self
            .observations
            .borrow()
            .starts
            .last()
            .copied()
            .ok_or("missing original attempt grid")?;
        let song_ns = (report.playback_start_frame + report.playback_frames as u64
            - start.playback_frame) as i64
            * MS;
        if !report.paused && mixer_generation == generation {
            while self.emitted[attempt] < 2 * self.options.notes_per_member {
                let edge = self.emitted[attempt];
                let note = edge / 2;
                let at_ms = 40 * note
                    + if note % 2 == 0 {
                        20 + edge % 2
                    } else {
                        10 + 20 * (edge % 2)
                    };
                if song_ns < at_ms as i64 * MS {
                    break;
                }
                if song_ns > at_ms as i64 * MS {
                    return Err("original input schedule passed an authored edge".into());
                }
                for member in 0..self.options.members {
                    events.push_back(PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(
                            DeviceId(1001 + member as u64),
                            now,
                            (attempt * 2 * self.options.notes_per_member + edge) as u64,
                        ),
                        control: PhysicalControlId::keyboard(7),
                        state: if edge % 2 == 0 {
                            ButtonState::Down
                        } else {
                            ButtonState::Up
                        },
                    }));
                }
                self.emitted[attempt] += 1;
            }
            let final_note = self.options.notes_per_member - 1;
            let pause_at =
                (40 * final_note + if final_note % 2 == 0 { 23 } else { 32 }) as i64 * MS;
            if song_ns >= pause_at && !self.paused_attempts[attempt] {
                self.paused_attempts[attempt] = true;
                shared.desired_pause = true;
                shared.pause_renders = 0;
            }
        }
        if report.paused && shared.desired_pause {
            shared.pause_renders += 1;
            if shared.pause_renders >= 3 {
                shared.desired_pause = false;
            }
        }
        // A bounded virtual collector drains ten-frame idle batches, and every
        // authored edge or lifecycle boundary. This is independent acquisition
        // evidence, not an inference made by the pump from an empty queue.
        if now.timestamp.as_nanos() % (10 * MS) == 0
            || events.len() != old_edges
            || report.paused
            || shared.desired_pause
            || mixer_generation != generation
            || song_ns >= (40 * self.options.notes_per_member + 20) as i64 * MS
        {
            self.last_cut = now;
        }
        let cut = if self.fault == Some(Fault::LateFrontier) && now.timestamp.as_nanos() >= 20 * MS
        {
            Some(host(0))
        } else {
            Some(self.last_cut)
        };
        Ok(InputBatch {
            completed_through: cut,
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
        Err("legacy end path".into())
    }
    fn seed_resume(
        &mut self,
        _: &mut PresentationEstimator,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        Err("legacy resume path".into())
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        let mixer = &self
            .output
            .current()
            .ok_or("missing output")?
            .owner
            .as_ref()
            .ok_or("missing owner")?
            .mixer;
        Ok(raw(mixer.playback_frame_cursor() as i64 * MS))
    }
}
#[derive(Default)]
struct Control {
    waits: usize,
    limit: usize,
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        Err("untimed software pump must not read wall clock".into())
    }
    fn checked_add(now: u64, duration: std::time::Duration) -> Option<u64> {
        now.checked_add(u64::try_from(duration.as_nanos()).ok()?)
    }
    fn wait(&mut self, _: std::time::Duration) -> NativeGameplayResult<()> {
        self.waits += 1;
        if self.waits > self.limit {
            Err("virtual pump wait budget exhausted".into())
        } else {
            Ok(())
        }
    }
}
struct Host {
    shared: Rc<RefCell<Shared>>,
    cycles: usize,
    disabled: bool,
    fault: Option<Fault>,
    generations: Vec<u64>,
    applied: Vec<u64>,
    committed: Vec<u64>,
    pauses: Vec<PauseState>,
}
impl NativeGameplayHost for Host {
    fn prepare_play_policies(
        &mut self,
        policies: &[(PlayerId, &ResolvedPlayPolicy)],
    ) -> NativeGameplayResult<()> {
        NoopGameplayHost.prepare_play_policies(policies)
    }
    fn cancelled(&self) -> bool {
        false
    }
    fn pause_requested(&self) -> bool {
        self.shared.borrow().desired_pause
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, state: PauseState) {
        self.pauses.push(state);
    }
    fn publish_section_end(&mut self, _: Timestamp) {}
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        self.shared.borrow_mut().live_results[0].extend(report.judge_events.iter().copied());
        Ok(())
    }
    fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()> {
        let mut shared = self.shared.borrow_mut();
        for report in reports {
            shared.live_results[report.player.0 as usize - 1]
                .extend(report.report.judge_events.iter().copied());
        }
        Ok(())
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
    fn advertise_practice(&mut self, cap: Option<PracticeCapability>) -> NativeGameplayResult<()> {
        if let Some(cap) = cap {
            // Started and mode controls can republish the same active identity.
            if self.generations.last() != Some(&cap.generation) {
                self.generations.push(cap.generation);
            }
            self.shared.borrow_mut().generation = cap.generation;
        }
        Ok(())
    }
    fn take_practice_request(&mut self) -> NativeGameplayResult<Option<PracticeRequest>> {
        let generation = self.shared.borrow().generation;
        if self.cycles > 1
            && generation as usize == self.cycles
            && !self.disabled
            && !self.generations.is_empty()
        {
            self.disabled = true;
            Ok(Some(PracticeRequest {
                id: 1,
                generation,
                action: PracticeAction::DisableLoop,
            }))
        } else {
            Ok(None)
        }
    }
    fn commit_practice_reply(&mut self, reply: &PracticeReply) -> NativeGameplayResult<()> {
        reply.result.as_ref().map_err(|error| error.clone())?;
        Ok(())
    }
    fn prepare_practice_presentation(
        &mut self,
        generation: u64,
        attempts: &[(PlayerId, &app::practice_session::PreparedPracticeAttempt)],
    ) -> NativeGameplayResult<app::player::PreparedPracticePresentation> {
        app::player::prepare_practice_presentation(generation, attempts)
    }
    fn apply_practice_identity(
        &mut self,
        prepared: &mut app::player::PreparedPracticePresentation,
        generation: u64,
    ) {
        app::player::apply_practice_identity(prepared, generation);
        self.applied.push(generation);
        self.shared.borrow_mut().generation = generation;
    }
    fn commit_practice_presentation(
        &mut self,
        prepared: app::player::PreparedPracticePresentation,
        generation: u64,
    ) -> NativeGameplayResult<()> {
        if self.fault == Some(Fault::Presentation) {
            return Err("injected practice presentation refusal".into());
        }
        app::player::commit_practice_presentation(prepared, generation)?;
        self.committed.push(generation);
        Ok(())
    }
}
struct Recordings {
    source: beatkernel_bms::BmsChart,
    captures: Vec<CaptureEvidence>,
    fault: Option<Fault>,
    shared: Rc<RefCell<Shared>>,
}
impl PracticeRecordingPort for Recordings {
    fn archive(
        &mut self,
        player: PlayerId,
        path: &Path,
        capture: LiveReplayCapture,
    ) -> NativeGameplayResult<()> {
        if self.fault == Some(Fault::Archive) {
            return Err("injected original capture archive refusal".into());
        }
        if self.captures.iter().any(|existing| existing.path == path) {
            return Err("duplicate original recording path".into());
        }
        let attempt = self
            .captures
            .iter()
            .filter(|evidence| evidence.player == player)
            .count();
        let file = ReplayFile::new(capture.header().clone(), capture.records().to_vec());
        let bytes = capture.into_bytes()?;
        let restored =
            app::replay_playback::reconstruct_section(&self.source, file.clone(), limits())?;
        let judge_hash = restored.engine().stable_hash()?;
        let live_results =
            std::mem::take(&mut self.shared.borrow_mut().live_results[player.0 as usize - 1]);
        if live_results != restored.results() {
            return Err("original live judge and canonical reconstruction differ".into());
        }
        self.captures.push(CaptureEvidence {
            player,
            device: DeviceId(1000 + player.0 as u64),
            attempt,
            path: path.to_owned(),
            file,
            bytes,
            reconstructed_results: restored.results().to_vec(),
            live_results,
            judge_hash,
        });
        Ok(())
    }
}

struct RunEvidence {
    source: beatkernel_bms::BmsChart,
    recording: Recordings,
    host: Host,
    device: Device,
    generation: u64,
    mixer_before: usize,
    mixer_after: usize,
    pending: usize,
    outcome: NativeGameplayResult<()>,
}
fn execute(options: Options, fault: Option<Fault>) -> NativeGameplayResult<RunEvidence> {
    options.validate()?;
    let chart = source(options.notes_per_member)?;
    let policy = ResolvedPlayPolicy::builtin(0, 0, 0)?;
    let end_ns = (40 * options.notes_per_member + 20) as i64 * MS;
    let region = PracticeRegion::new(
        Timestamp::ZERO,
        Timestamp::from_nanos(end_ns),
        options.cycles > 1,
    )?;
    let format = AudioFormat::new(1000, 1)?;
    let pcm_limits = PcmLimits::new(4, 12, 3)?;
    let mut bank = SampleBank::new(format, pcm_limits)?;
    for (id, value) in [(1, 0.25), (2, 0.125), (3, -0.75)] {
        bank.insert(
            SampleId(id),
            PcmSample::new(format, vec![value], pcm_limits)?,
        )?;
    }
    let program = PreparedPracticeProgram::new(
        &bank,
        vec![PracticeCue {
            voice: VoiceId(999_999),
            sample: SampleId(2),
            at: Timestamp::ZERO,
            gain: 1.0,
        }],
        PracticeLimits::new(1, 1, 4, 4, 8)?,
    )?;
    let (controller, endpoint) = practice_queue(&program)?;
    let capacity = options.members * 4 + 8;
    let (mut producer, consumer) = command_queue(capacity)?;
    producer.set_scope(CommandScope(1));
    if options.cycles > 1 {
        producer
            .try_push_scoped(
                CommandScope(1),
                AudioCommand::Play {
                    voice: VoiceId(999_998),
                    sample: SampleId(3),
                    at: Timestamp::from_nanos(end_ns + 5 * MS),
                    gain: 1.0,
                },
            )
            .map_err(|error| format!("stale-scope sentinel command admission failed: {error:?}"))?;
    }
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(capacity, options.members * 2 + 2, capacity, 1, capacity)?,
        ),
        bank,
        consumer,
    )?;
    mixer.install_practice(program, endpoint, region)?;
    let owner = RetainedMixer {
        mixer: Box::new(mixer),
    };
    let mixer_before = owner.identity();
    let basis = owner.output_frame_basis();
    let observations = Rc::new(RefCell::new(Observations::default()));
    let shared = Rc::new(RefCell::new(Shared {
        live_results: vec![vec![]; options.members],
        ..Shared::default()
    }));
    let max_frames = options.cycles * (40 * options.notes_per_member + 40) + 16;
    let output = GameplayOutputOwner::new(
        Backend {
            observations: observations.clone(),
            max_frames,
            quiet: false,
        },
        VirtualOutput::new(owner, 0),
    );
    let ui_controls = Rc::new(RefCell::new(OutputControls::new()));
    let mut ui = GameplayOutputUi::new(Ui(ui_controls.clone()));
    ui.advertise(Some(OutputCapability {
        host: app::settings::SettingsHost::Linux,
        current_args: vec![
            "--rate".into(),
            "48000".into(),
            "--period-frames".into(),
            "32".into(),
            "--buffer-frames".into(),
            "128".into(),
        ],
    }))?;
    ui_controls
        .borrow_mut()
        .request(vec!["--rate".into(), "32000".into()])?;
    let mut device = Device {
        output,
        observations,
        shared: shared.clone(),
        options,
        emitted: vec![0; options.cycles],
        paused_attempts: vec![false; options.cycles],
        fault,
        ui,
        ui_controls,
        refusal: String::new(),
        mapping_calls: 0,
        refusal_retires: 0,
        last_cut: host(0),
    };
    let mut host_port = Host {
        shared: shared.clone(),
        cycles: options.cycles,
        disabled: false,
        fault,
        generations: vec![],
        applied: vec![],
        committed: vec![],
        pauses: vec![],
    };
    let mut members = Vec::new();
    let mut states = Vec::new();
    let mut practice_members = Vec::new();
    for i in 0..options.members {
        let player = PlayerId(i as u32 + 1);
        let device_id = DeviceId(1001 + i as u64);
        let engine = judge(&chart, &policy)?;
        let capture = LiveReplayCapture::new_with_policy(
            &engine,
            ClockDomainId(3),
            limits(),
            Timestamp::ZERO,
            0,
            Some(Timestamp::from_nanos(end_ns)),
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            &policy,
        )?;
        states.push(GameplayPlayerState::<NoopSoloCompetition> {
            player,
            capture: Some(capture),
            competition: None,
            completion: None,
            score: ScoreSummary::default(),
            gauge: BmsGauge::new(policy.gauge().try_copy()?),
            last_song: Timestamp::ZERO,
        });
        members.push(MemberConfig {
            player,
            device: Some(device_id),
            bindings: bindings(Some(device_id))?,
            judge: engine,
            sounds: sounds(&chart, i)?,
        });
        practice_members.push(PracticeMember {
            player,
            policy: ResolvedPlayPolicy::builtin(0, 0, 0)?,
            launch: SessionLaunch::new(vec![
                "--chart".into(),
                "generated.bms".into(),
                "--record-replay".into(),
                format!("member{}.bkr", player.0),
            ])?,
            chart_seed: 0,
            capture_limits: Some(limits()),
        });
    }
    let mut playback = PracticePlayback::new(
        controller,
        chart.clone(),
        practice_members,
        region,
        Timestamp::from_nanos(end_ns + 40 * MS),
        basis,
        1,
    )?;
    let mut recording = Recordings {
        source: chart.clone(),
        captures: Vec::new(),
        fault,
        shared,
    };
    let mut bgm = BgmFeeder::new(
        vec![],
        BgmConfig {
            output_origin: raw(0),
            sample_rate: 1000,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(10 * MS),
            max_pending: 1,
        },
    )?;
    let mut p = presentation()?;
    p.admit(NativeAudioSnapshot {
        epoch: 0,
        basis,
        evidence: OriginalNativePresentationEvidence::SuppliedPair(pair(0)),
    })?;
    let mut pause = NativePause::new(raw(0), ClockDomainId(1), 1000)?;
    let mut end = None;
    let mut delivery = InputDeliveryTelemetry::new(0, ClockDomainId(1))?;
    let mut pre = 0;
    let mut merger = if options.members == 1 {
        // The solo pump owns registration of actual arriving sources, with its
        // original bounded 4096-source admission contract.
        InputMerger::new_dynamic(ClockDomainId(1), host(0), 4096, options.members * 4 + 8)?
    } else {
        InputMerger::new(
            ClockDomainId(1),
            host(0),
            (0..options.members)
                .map(|i| DeviceId(1001 + i as u64))
                .collect(),
            options.members * 4 + 8,
        )?
    };
    let mut control = Control {
        waits: 0,
        limit: max_frames,
    };
    let outcome = if options.members == 1 {
        let member = members.remove(0);
        let mut runtime = SoloRuntime::new(
            ClockDomainId(3),
            ClockDomainId(2),
            Transport::new(logical(0).timestamp, Timestamp::ZERO, Rate::NORMAL),
            member.bindings,
            member.judge,
            producer,
            member.sounds,
            0,
        )?;
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        runtime.set_audio_scope(CommandScope(1));
        runtime.set_song_end(Timestamp::from_nanos(end_ns))?;
        let state = &mut states[0];
        app::native_gameplay::run_gameplay_audio_with_practice_and_ports(
            &mut device,
            AudioGameplaySession {
                session: GameplaySession {
                    runtime: &mut runtime,
                    gauge: &mut state.gauge,
                    bgm: &mut bgm,
                    discipline: &mut p,
                    pause: &mut pause,
                    end: &mut end,
                    completion: &mut state.completion,
                    capture: &mut state.capture,
                    competition: &mut state.competition,
                    delivery: &mut delivery,
                    pre_origin_inputs: &mut pre,
                },
                merger: &mut merger,
            },
            config(end_ns),
            &mut control,
            &mut host_port,
            &mut playback,
            &mut recording,
        )
        .map(|_| ())
    } else {
        let mut group = RuntimeGroup::new(
            ClockDomainId(3),
            ClockDomainId(2),
            Transport::new(logical(0).timestamp, Timestamp::ZERO, Rate::NORMAL),
            producer,
            members,
            0,
            &[],
        )?;
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        group.set_audio_scope(CommandScope(1));
        group.set_song_end(Timestamp::from_nanos(end_ns))?;
        let policies = (0..options.members)
            .map(|i| (PlayerId(i as u32 + 1), &policy))
            .collect::<Vec<_>>();
        app::native_cohort::run_cohort_audio_practice_with_policies_and_results_and_ports(
            &mut device,
            AudioCohortSession {
                group: &mut group,
                network: None::<&mut NoopGroupCompetition>,
                states: &mut states,
                merger: &mut merger,
                bgm: &mut bgm,
                discipline: &mut p,
                pause: &mut pause,
                end: &mut end,
                delivery: &mut delivery,
                pre_origin_inputs: &mut pre,
            },
            config(end_ns),
            &mut control,
            &mut host_port,
            &policies,
            &mut playback,
            &mut recording,
        )
        .map(|_| ())
    };
    // The pump leaves the final successful attempt for its launcher. Failed
    // publication preserves the actually archived prefix and applied identity;
    // it does not manufacture an additional completed-attempt archive effect.
    if outcome.is_ok() {
        for state in &mut states {
            if let Some(capture) = state.capture.take() {
                let launch = &playback
                    .members()
                    .iter()
                    .find(|member| member.player == state.player)
                    .ok_or("lost original practice member")?
                    .launch;
                let path = launch
                    .args()
                    .windows(2)
                    .find(|pair| pair[0] == "--record-replay")
                    .map(|pair| PathBuf::from(&pair[1]))
                    .ok_or("lost original replay path")?;
                recording.archive(state.player, &path, capture)?;
            }
        }
    }
    if outcome.is_ok() {
        // Completion may enqueue its final source fences. Consume those on the
        // same complete owner and keep the actual drain renders inspectable.
        device.observe_audio(&mut p)?;
        device.observe_audio(&mut p)?;
    }
    let pending = merger.pending();
    let mixer_after = device
        .output
        .current()
        .ok_or("missing final output")?
        .owner
        .as_ref()
        .ok_or("missing final owner")?
        .identity();
    Ok(RunEvidence {
        source: chart,
        recording,
        host: host_port,
        device,
        generation: playback.generation(),
        mixer_before,
        mixer_after,
        pending,
        outcome,
    })
}

pub fn run_fault(options: Options, fault: Fault) -> NativeGameplayResult<FailureEvidence> {
    options.validate()?;
    if matches!(
        fault,
        Fault::UnacknowledgedPause | Fault::ReplacementTimeout
    ) {
        let error = replacement_evidence(Some(fault))
            .err()
            .ok_or("replacement fault unexpectedly succeeded")?
            .to_string();
        return Ok(FailureEvidence {
            error,
            generation: 0,
            captures: vec![],
            applied_generations: vec![],
            committed_generations: vec![],
            pending_inputs: 0,
        });
    }
    let evidence = execute(options, Some(fault))?;
    let error = evidence
        .outcome
        .err()
        .ok_or("injected failure unexpectedly succeeded")?
        .to_string();
    Ok(FailureEvidence {
        error,
        generation: evidence.generation,
        captures: evidence.recording.captures,
        applied_generations: evidence.host.applied,
        committed_generations: evidence.host.committed,
        pending_inputs: evidence.pending,
    })
}

pub fn run(options: Options) -> NativeGameplayResult<Report> {
    let evidence = execute(options, None)?;
    evidence.outcome?;
    let captures = evidence.recording.captures;
    let mut inputs = 0;
    let mut advances = 0;
    let mut repairs = 0;
    for capture in &captures {
        for record in &capture.file.records {
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
        if capture.reconstructed_results.len()
            != options.notes_per_member + options.notes_per_member / 2
            || capture
                .reconstructed_results
                .iter()
                .any(|event| !matches!(event.outcome, JudgeOutcome::Hit { .. }))
        {
            return Err("original captured attempt judge facts differ from authored notes".into());
        }
    }
    let observations = evidence.device.observations.borrow();
    let last = observations.reports.last().ok_or("no rendered evidence")?;
    let mut expected_pcm = vec![0.0; observations.pcm.len()];
    if observations.starts.len() != options.cycles {
        return Err(format!(
            "original attempt render grid is incomplete: active starts={}, expected={}",
            observations.starts.len(),
            options.cycles
        )
        .into());
    }
    for start in &observations.starts {
        let first = usize::try_from(start.physical_frame)?;
        *expected_pcm
            .get_mut(first)
            .ok_or("original cue frame is outside rendered PCM")? = 0.125;
        for note in 0..options.notes_per_member {
            let offsets = if note % 2 == 0 {
                vec![40 * note + 20]
            } else {
                vec![40 * note + 10, 40 * note + 30]
            };
            for offset in offsets {
                // Every pause lies after this attempt's last authored edge;
                // preceding attempts' actual silent frames are in `first`.
                *expected_pcm
                    .get_mut(first + offset)
                    .ok_or("original note frame is outside rendered PCM")? =
                    (options.members as f32 * 0.25).min(1.0);
            }
        }
    }
    if observations.pcm != expected_pcm {
        return Err("original rendered PCM differs from literal cue/note schedule".into());
    }
    let authored_edges = 2 * options.members * options.cycles * options.notes_per_member;
    if captures.len() != options.members * options.cycles || inputs != authored_edges
        // The real Ended receipt advances control generation once more, but
        // does not create a fresh active attempt or recording identity.
        || repairs != 0 || evidence.generation != options.cycles as u64 + 1
        || evidence.host.committed.len() != options.cycles - 1 || evidence.pending != 0
        || last.pending_commands != 0 || last.active_voices != 0
        || evidence.mixer_before != evidence.mixer_after
        || evidence.device.refusal != "output replacement is unavailable during retained practice"
        || evidence.device.mapping_calls != 0 || evidence.device.refusal_retires != 0
        || observations.pcm.iter().any(|sample| *sample < 0.0)
    {
        return Err(format!("retained workload invariant failed: captures={}, inputs={}, repairs={}, control generation={}, commits={}, pending inputs={}, pending commands={}, voices={}, refusal={:?}, mapping calls={}, retire calls={}",
            captures.len(), inputs, repairs, evidence.generation, evidence.host.committed.len(),
            evidence.pending, last.pending_commands, last.active_voices, evidence.device.refusal,
            evidence.device.mapping_calls, evidence.device.refusal_retires).into());
    }
    let replacement = replacement_evidence(None)?;
    Ok(Report {
        options,
        source: evidence.source,
        captures,
        pcm: observations.pcm.clone(),
        render_reports: observations.reports.clone(),
        attempt_starts: observations.starts.clone(),
        generations: evidence.host.generations,
        applied_generations: evidence.host.applied,
        committed_generations: evidence.host.committed,
        pause_states: evidence.host.pauses,
        transitions: options.cycles - 1,
        authored_edges,
        captured_input_edges: inputs,
        advance_records: advances,
        repair_records: repairs,
        mixer_identity_before: evidence.mixer_before,
        mixer_identity_after: evidence.mixer_after,
        replacement,
        replacement_refusal: evidence.device.refusal,
        refusal_mapping_calls: evidence.device.mapping_calls,
        refusal_retire_calls: evidence.device.refusal_retires,
        pending_inputs: evidence.pending,
        pending_commands: last.pending_commands,
        active_voices: last.active_voices,
        sentinel_injected: options.cycles > 1,
    })
}

fn replacement_evidence(fault: Option<Fault>) -> NativeGameplayResult<ReplacementEvidence> {
    // Separate admitted output composition: retained practice deliberately has
    // no successful replacement path in the current application policy.
    let chart = source(1)?;
    let policy = ResolvedPlayPolicy::builtin(0, 0, 0)?;
    let format = AudioFormat::new(1000, 1)?;
    let bank = SampleBank::new(format, PcmLimits::new(4, 4, 1)?)?;
    let (mut producer, consumer) = command_queue(8)?;
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 1, 8)?,
        ),
        bank,
        consumer,
    )?;
    mixer.render(&mut [0.0])?;
    mixer.render(&mut [0.0])?;
    let mut pause = NativePause::new(raw(0), ClockDomainId(1), 1000)?;
    pause.request(true, pair(2 * MS))?;
    producer.request_pause(true);
    let ack = mixer.render(&mut [0.0])?;
    if fault != Some(Fault::UnacknowledgedPause) {
        pause
            .observe(Some(ack), pair(3 * MS))?
            .ok_or("original nonempty pause was not acknowledged")?;
    }
    let owner = RetainedMixer {
        mixer: Box::new(mixer),
    };
    let before = owner.identity();
    let output = VirtualOutput::new(owner, 0);
    let old_basis = OutputFrameBasis::new(raw(0), 1000, 0)?;
    let mut p = presentation()?;
    for ns in [MS, 3 * MS] {
        p.admit(NativeAudioSnapshot {
            epoch: 0,
            basis: old_basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(pair(ns)),
        })?;
    }
    let mut runtime = SoloRuntime::new(
        ClockDomainId(3),
        ClockDomainId(2),
        Transport::new(logical(0).timestamp, Timestamp::ZERO, Rate::NORMAL),
        bindings(None)?,
        judge(&chart, &policy)?,
        producer,
        vec![],
        0,
    )?;
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let mut merger = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(1001)], 8)?;
    p.authority_mut().record_acquired_prefix(host(4 * MS))?;
    let cutoff = p
        .authority()
        .prepare_control_cutoff(0, raw(2 * MS), host(2 * MS), host(4 * MS), &merger)?
        .ok_or("missing original control frontier")?;
    struct Identity;
    impl beatkernel::time::ClockMapper for Identity {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            (from.domain == to).then_some(from.timestamp)
        }
        fn quality(&self) -> beatkernel::time::ClockMappingQuality {
            beatkernel::time::ClockMappingQuality::Exact
        }
    }
    runtime.advance_to(cutoff.output(), &Identity, raw(4 * MS))?;
    p.authority_mut().commit_control_cutoff(cutoff, &merger)?;
    runtime.transport_mut().pause(cutoff.output().timestamp)?;
    let held = p
        .authority()
        .prepare_held_frontier(host(4 * MS), &merger)?
        .ok_or("missing acquired held frontier")?;
    p.authority_mut().commit_held_frontier(held, &mut merger)?;
    let observations = Rc::new(RefCell::new(Observations::default()));
    // The original output basis is captured at stream creation, while a new
    // replacement captures its retained next frame. Preserve both explicitly.
    let mut output = output;
    output.basis = old_basis;
    let premature = output.take_stopped_mixer().is_err();
    let mut output_owner = GameplayOutputOwner::new(
        Backend {
            observations: observations.clone(),
            max_frames: 8,
            quiet: fault == Some(Fault::ReplacementTimeout),
        },
        output,
    );
    output_owner
        .queue((), 100 * MS as u64)
        .map_err(|_| "replacement queue refused")?;
    let mut cfg = config(60 * MS).gameplay;
    cfg.end_song = None;
    let mut end = None;
    let mut published = false;
    for ns in [20 * MS, 21 * MS, 120 * MS] {
        published = output_owner.publish_paused_audio(
            GameplayAudioOutputContext {
                control: GameplayPauseControl::solo(&mut runtime),
                presentation: &mut p,
                pause: &mut pause,
                merger: &merger,
                config: &mut cfg,
                end: &mut end,
            },
            host(ns),
        )?;
        if published {
            break;
        }
    }
    if !published {
        return Err("two progressing original snapshots did not publish replacement".into());
    }
    let current = output_owner
        .current()
        .ok_or("ready publication lost current output")?;
    let after = current
        .owner
        .as_ref()
        .ok_or("ready publication lost complete owner")?
        .identity();
    let observations = observations.borrow();
    if before != after
        || p.authority().epoch().id != 1
        || pause.epoch() != 1
        || observations.snapshots.len() != 2
        || observations.retires != 1
        || observations.opens != 1
        || observations
            .reports
            .iter()
            .any(|report| !report.paused || report.frames == 0 || report.playback_frames != 0)
    {
        return Err("original owner replacement invariant failed".into());
    }
    Ok(ReplacementEvidence {
        mixer_identity_before: before,
        mixer_identity_after: after,
        epoch_before: 0,
        epoch_after: p.authority().epoch().id,
        basis: current.basis,
        paused_render_reports: std::iter::once(ack)
            .chain(observations.reports.iter().copied())
            .collect(),
        snapshots: observations.snapshots.clone(),
        retire_calls: observations.retires,
        open_calls: observations.opens,
        premature_recovery_refused: premature,
    })
}
