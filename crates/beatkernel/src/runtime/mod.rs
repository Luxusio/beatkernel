//! Single-owner forward runtime connecting canonical input to scalar audio.

pub mod gameplay_sound_stop;
pub mod hazard_sound;
pub mod input_sound;
pub mod playback;
pub mod restart;

pub use gameplay_sound_stop::{GameplaySoundStop, RuntimeSoundStopReport};

use crate::{
    audio::{AudioCommand, CommandProducer, CommandPushError, QueuePushError, SampleId, VoiceId},
    chart::ObjectId,
    input::{
        BindingMap, DeviceId, GameInputEvent, PhysicalInputEvent, Position2, TouchRegion,
        TouchRoute, TouchRouter, TouchRoutingError,
    },
    judge::{HazardEvent, JudgeEngine, JudgeError, JudgeEvent, JudgeOutcome, JudgeStage},
    telemetry::{RuntimeCounters, RuntimeTelemetry},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
    transport::{Transport, TransportError},
};
use std::{collections::HashMap, fmt, time::Instant};
use hazard_sound::{HazardSoundError, HazardSoundTimeline};
use input_sound::InputSoundTimeline;

/// Explicit sound associated with one accepted chart stage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundBinding {
    /// Chart object receiving the sound.
    pub object: ObjectId,
    /// Stage that triggers this sound on a hit.
    pub stage: JudgeStage,
    /// Preloaded PCM asset.
    pub sample: SampleId,
    /// Explicit mixer voice, allowing caller-selected replacement behavior.
    pub voice: VoiceId,
    /// Finite signed gain.
    pub gain: f32,
}
impl SoundBinding {
    /// Selects a matching hit without changing its independently supplied output time.
    ///
    /// Hosts validate finite gains during setup. This performs no clock mapping,
    /// queue admission, allocation or judge mutation.
    pub fn command_for(&self, event: &JudgeEvent, at: Timestamp) -> Option<AudioCommand> {
        (matches!(event.outcome, JudgeOutcome::Hit { .. })
            && self.object == event.object
            && self.stage == event.stage)
            .then_some(AudioCommand::Play {
                voice: self.voice,
                sample: self.sample,
                at,
                gain: self.gain,
            })
    }
}

/// Operation failure before binding; judge fanout failures live in the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    /// No relation or representable mapping exists for these domains.
    UnmappedClock {
        /// Source clock.
        from: ClockDomainId,
        /// Destination clock.
        to: ClockDomainId,
    },
    /// Input or deadline host chronology regressed.
    NonMonotonicHost,
    /// Device acquisition sequence regressed.
    SequenceRegression {
        /// Source device.
        device: DeviceId,
        /// Previously admitted sequence.
        last: u64,
        /// Rejected sequence.
        received: u64,
    },
    /// Forward judge cannot traverse this reversed segment without restoration.
    RequiresReplayRestore,
    /// Host-to-song query failed.
    Transport(TransportError),
    /// Sound configuration contains a nonfinite gain.
    InvalidGain,
    /// Input sounds are already configured or a runtime operation was committed.
    InputSoundConfigurationLocked,
    /// A logical song endpoint must be nonnegative.
    InvalidSongEnd,
    /// Endpoint setup is immutable after configuration or a committed operation.
    SongEndConfigurationLocked,
    /// Touch-region routing rejected a sample before acquisition was committed.
    TouchRouting(TouchRoutingError),
    /// A touch router is already installed or the runtime has committed processing.
    TouchRoutingConfigurationLocked,
    /// No touch router is installed for an explicit layout/policy change.
    TouchRoutingUnavailable,
}
impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "runtime: {self:?}")
    }
}
impl std::error::Error for RuntimeError {}

/// Complete successful/partially judged operation, preserving failure evidence.
#[derive(Clone, Debug)]
pub struct RuntimeReport {
    /// Admitted input, normalized into host time with original provenance.
    pub input: Option<PhysicalInputEvent>,
    /// Binding destinations successfully submitted to the judge.
    pub bound_inputs: Vec<GameInputEvent>,
    /// Unoffset transport song time.
    pub song_time: Timestamp,
    /// Raw mapped position reached the configured logical end. This is not
    /// native completion evidence; `judge_error` remains authoritative.
    pub song_end_reached: bool,
    /// Independently supplied and normalized output scheduling point.
    pub audio_at: ClockPoint,
    /// Mapper's declared relation quality (same-domain identity is exact).
    pub input_mapping_quality: ClockMappingQuality,
    /// Quality declared for output-time mapping.
    pub audio_mapping_quality: ClockMappingQuality,
    /// Emitted judge results; never undone by queue failures.
    pub judge_events: Vec<JudgeEvent>,
    /// Hazard outcomes from successful judge calls in binding order, preserving
    /// provenance and committed prefixes across later judge or queue failures.
    /// These do not automatically produce scores or audio commands.
    pub hazard_events: Vec<HazardEvent>,
    /// First judge fanout failure; prior results remain committed.
    pub judge_error: Option<JudgeError>,
    /// Successfully published scalar commands.
    pub audio_commands: Vec<AudioCommand>,
    /// Exact failed commands and reasons, with no implicit retry.
    pub audio_failures: Vec<CommandPushError>,
}

/// Software profiling only; never an input, song or output clock.
///
/// Hosts without a usable `std::time::Instant` must select `External` or
/// `Disabled` before processing. External callbacks are trusted synchronous
/// readers of monotonic nanoseconds and run on the control owner.
#[derive(Clone, Copy, Debug, Default)]
pub enum RuntimeProcessingClock {
    /// Uses the standard monotonic timer, preserving native profiling.
    #[default]
    Native,
    /// Missing or regressing readings omit the duration observation.
    External(fn() -> Option<u64>),
    /// Acquires no processing timer; operation counters remain enabled.
    Disabled,
}

enum ProcessingStarted {
    Native(Instant),
    External {
        read: fn() -> Option<u64>,
        start: Option<u64>,
    },
    Disabled,
}

impl RuntimeProcessingClock {
    fn start(self) -> ProcessingStarted {
        match self {
            Self::Native => ProcessingStarted::Native(Instant::now()),
            Self::External(read) => ProcessingStarted::External {
                read,
                start: read(),
            },
            Self::Disabled => ProcessingStarted::Disabled,
        }
    }
}

impl ProcessingStarted {
    fn elapsed(self) -> Option<u64> {
        match self {
            Self::Native(start) => {
                Some(u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX))
            }
            Self::External { read, start } => {
                let end = read();
                end?.checked_sub(start?)
            }
            Self::Disabled => None,
        }
    }
}

/// Owns control-thread state; no platform dependency.
pub struct Runtime {
    host_domain: ClockDomainId,
    audio_domain: ClockDomainId,
    transport: Transport,
    bindings: BindingMap,
    touch_router: Option<TouchRouter>,
    judge: JudgeEngine,
    producer: CommandProducer,
    sounds: Vec<SoundBinding>,
    input_sounds: Option<InputSoundTimeline>,
    hazard_sounds: Option<HazardSoundTimeline>,
    gameplay_sound_stop: GameplaySoundStop,
    input_sounds_locked: bool,
    last_host: Option<Timestamp>,
    last_song: Option<Timestamp>,
    song_end: Option<Timestamp>,
    gameplay_fence: Option<Timestamp>,
    sequences: HashMap<DeviceId, u64>,
    telemetry: RuntimeTelemetry,
    processing_clock: RuntimeProcessingClock,
}

impl Runtime {
    /// Composes existing owners and validates sound gains before accepting input.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host_domain: ClockDomainId,
        audio_domain: ClockDomainId,
        transport: Transport,
        bindings: BindingMap,
        judge: JudgeEngine,
        producer: CommandProducer,
        sounds: Vec<SoundBinding>,
        telemetry_capacity: usize,
    ) -> Result<Self, RuntimeError> {
        if sounds.iter().any(|sound| !sound.gain.is_finite()) {
            return Err(RuntimeError::InvalidGain);
        }
        let gameplay_sound_stop =
            GameplaySoundStop::new(sounds.iter().map(|sound| sound.voice).collect());
        Ok(Self {
            host_domain,
            audio_domain,
            transport,
            bindings,
            touch_router: None,
            judge,
            producer,
            sounds,
            input_sounds: None,
            hazard_sounds: None,
            gameplay_sound_stop,
            input_sounds_locked: false,
            last_host: None,
            last_song: None,
            song_end: None,
            gameplay_fence: None,
            sequences: HashMap::new(),
            telemetry: RuntimeTelemetry::new(telemetry_capacity),
            processing_clock: RuntimeProcessingClock::Native,
        })
    }

    /// Installs an immutable press-sound timeline once, before committed input
    /// or advancement. Same-chart state/session restoration retains both the
    /// timeline and this configuration lock; changed charts need a new owner.
    pub fn configure_input_sounds(
        &mut self,
        timeline: InputSoundTimeline,
    ) -> Result<(), RuntimeError> {
        if self.input_sounds_locked || self.input_sounds.is_some() {
            return Err(RuntimeError::InputSoundConfigurationLocked);
        }
        let mut voices = self.gameplay_sound_stop.voices().to_vec();
        voices.extend(timeline.markers().iter().map(|marker| marker.voice));
        let stops = GameplaySoundStop::new(voices);
        self.input_sounds = Some(timeline);
        self.gameplay_sound_stop = stops;
        Ok(())
    }

    /// Installs one immutable hazard-sound timeline before committed input or
    /// advancement. Rejected setup preserves any existing timeline.
    pub fn configure_hazard_sounds(
        &mut self,
        timeline: HazardSoundTimeline,
    ) -> Result<(), HazardSoundError> {
        if self.hazard_sounds.is_some() {
            return Err(HazardSoundError::AlreadyConfigured);
        }
        if self.input_sounds_locked {
            return Err(HazardSoundError::AlreadyStarted);
        }
        let mut voices = self.gameplay_sound_stop.voices().to_vec();
        voices.extend(timeline.bindings().iter().map(|binding| binding.voice));
        let stops = GameplaySoundStop::new(voices);
        self.hazard_sounds = Some(timeline);
        self.gameplay_sound_stop = stops;
        Ok(())
    }

    /// Installs one immutable logical endpoint before any committed operation.
    /// Transport anchors may be placeholders during shared-owner composition;
    /// session start/end compatibility is validated by the outer owner.
    pub fn set_song_end(&mut self, end: Timestamp) -> Result<(), RuntimeError> {
        if self.song_end.is_some() || self.last_host.is_some() {
            return Err(RuntimeError::SongEndConfigurationLocked);
        }
        if end.as_nanos() < 0 {
            return Err(RuntimeError::InvalidSongEnd);
        }
        self.song_end = Some(end);
        Ok(())
    }
    /// Configured logical song endpoint, independent of native playback state.
    pub const fn song_end(&self) -> Option<Timestamp> {
        self.song_end
    }

    /// Fences gameplay at the latest committed song time, limited by the
    /// configured endpoint. Repeated calls retain the first frontier; before
    /// any committed operation this returns `None` without changing setup.
    /// Acquisition chronology remains active, while judging, routing and new
    /// gameplay sounds stop. Existing held state and queued audio stay intact.
    pub fn fence_gameplay(&mut self) -> Option<Timestamp> {
        if self.gameplay_fence.is_none() {
            self.gameplay_fence = self
                .last_song
                .map(|song| self.song_end.map_or(song, |end| song.min(end)));
        }
        self.gameplay_fence
    }

    /// The optional committed frontier latched by `fence_gameplay`.
    pub const fn gameplay_fence(&self) -> Option<Timestamp> {
        self.gameplay_fence
    }

    /// Attempts every configured gameplay voice once after a committed fence.
    /// `requested_at` must use this runtime's output domain. The effective time
    /// is at least the latest successfully admitted gameplay sound time, so a
    /// previously queued Play cannot follow its Stop. External `enqueue_audio`
    /// commands remain caller-owned and do not establish this watermark.
    ///
    /// Returns `None` before fencing or after any prior stop attempt, including
    /// partial failure. Accepted commands are not physical silence or drain proof.
    pub fn fence_gameplay_sounds(
        &mut self,
        requested_at: Timestamp,
    ) -> Option<RuntimeSoundStopReport> {
        if self.gameplay_fence.is_none() {
            return None;
        }
        let producer = &mut self.producer;
        let counters = self.telemetry.counters_mut();
        self.gameplay_sound_stop.attempt(requested_at, |command| {
            admit_audio(producer, counters, command)
        })
    }

    /// Installs spatial routing before any committed operation, at most once.
    /// Unconfigured inputs still use the ordinary binding map.
    pub fn configure_touch_router(&mut self, router: TouchRouter) -> Result<(), RuntimeError> {
        if self.last_host.is_some() || self.touch_router.is_some() {
            return Err(RuntimeError::TouchRoutingConfigurationLocked);
        }
        self.touch_router = Some(router);
        Ok(())
    }

    /// Read-only routing state for an explicitly paired gameplay checkpoint.
    pub fn touch_router(&self) -> Option<&TouchRouter> {
        self.touch_router.as_ref()
    }

    /// Changes only configured region bounds, preserving contact ownership and
    /// all runtime clocks, sequence watermarks, judgments and audio state.
    pub fn remap_touch_regions(&mut self, regions: Vec<TouchRegion>) -> Result<(), RuntimeError> {
        self.touch_router
            .as_mut()
            .ok_or(RuntimeError::TouchRoutingUnavailable)?
            .remap_regions(regions)
            .map_err(RuntimeError::TouchRouting)
    }

    /// Controls fresh contact admission without releasing any held input.
    pub fn set_touch_routing_enabled(&mut self, enabled: bool) -> Result<(), RuntimeError> {
        self.touch_router
            .as_mut()
            .ok_or(RuntimeError::TouchRoutingUnavailable)?
            .set_new_contacts_enabled(enabled);
        Ok(())
    }

    /// Normalizes, binds, judges and publishes at an independently supplied output time.
    /// Queue failures do not fail the operation or revert judge state. Binding fanout
    /// stops at its first judge error; inspect `judge_error` before continuing.
    /// At/after a configured end, acquisition is still validated/committed but
    /// binding is skipped and the actual judge advances only to that end.
    pub fn process_input(
        &mut self,
        input: PhysicalInputEvent,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, RuntimeError> {
        self.process_input_with_position(input, None, mapper, audio_at)
    }

    /// Processes genuine physical input with a separate touch hit-test position.
    /// The projection never changes the physical payload or capture provenance.
    /// Timing and acquisition checks precede routing; finite-end inputs never
    /// adopt or release routing ownership after the logical boundary.
    pub fn process_input_at(
        &mut self,
        input: PhysicalInputEvent,
        position: Position2,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, RuntimeError> {
        self.process_input_with_position(input, Some(position), mapper, audio_at)
    }

    fn process_input_with_position(
        &mut self,
        mut input: PhysicalInputEvent,
        position: Option<Position2>,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, RuntimeError> {
        let started = self.processing_clock.start();
        let result = (|| {
            let incoming = ClockPoint {
                domain: input.meta().clock_domain,
                timestamp: input.meta().timestamp,
            };
            let (host, input_quality) = normalize(incoming, self.host_domain, mapper)?;
            let meta = *input.meta();
            if let Some(&last) = self.sequences.get(&meta.source) {
                if meta.sequence < last {
                    return Err(RuntimeError::SequenceRegression {
                        device: meta.source,
                        last,
                        received: meta.sequence,
                    });
                }
            }
            let (mut report, mapped_song) = self.prepare(host, audio_at, mapper, input_quality)?;
            if incoming.domain != self.host_domain {
                let meta = input.meta_mut();
                meta.original_clock_point.get_or_insert(incoming);
                meta.timestamp = host.timestamp;
                meta.clock_domain = host.domain;
            }
            if self.gameplay_fence.is_some() {
                self.sequences.insert(meta.source, meta.sequence);
                self.commit_time(host.timestamp, mapped_song);
                let counters = self.telemetry.counters_mut();
                counters.inputs = counters.inputs.saturating_add(1);
                report.input = Some(input);
                return Ok(report);
            }
            let routed = if report.song_end_reached {
                TouchRoute::Unconfigured
            } else if let Some(router) = &mut self.touch_router {
                match position {
                    Some(position) => router.route_at(&input, position),
                    None => router.route(&input),
                }
                .map_err(RuntimeError::TouchRouting)?
            } else {
                TouchRoute::Unconfigured
            };
            self.sequences.insert(meta.source, meta.sequence);
            self.commit_time(host.timestamp, mapped_song);
            let counters = self.telemetry.counters_mut();
            counters.inputs = counters.inputs.saturating_add(1);
            let mut input_commands = Vec::new();
            if report.song_end_reached {
                match self.judge.advance_to(report.song_time) {
                    Ok(events) => {
                        report
                            .hazard_events
                            .extend_from_slice(self.judge.hazard_events());
                        report.judge_events = events;
                    }
                    Err(error) => report.judge_error = Some(error),
                }
            } else {
                let (single, use_bindings) = match routed {
                    TouchRoute::Unconfigured => (None, true),
                    TouchRoute::Ignored => (None, false),
                    TouchRoute::Bound(bound) => (Some(bound), false),
                };
                let fallback = if use_bindings {
                    Some(self.bindings.map(&input))
                } else {
                    None
                };
                for bound in single.into_iter().chain(fallback.into_iter().flatten()) {
                    let fresh = self.input_sounds.is_some() && self.judge.is_fresh_press(&bound);
                    match self.judge.push_input(&bound, report.song_time) {
                        Ok(events) => {
                            report
                                .hazard_events
                                .extend_from_slice(self.judge.hazard_events());
                            if let Some(command) = self.input_sounds.as_ref().and_then(|timeline| {
                                timeline.command_for_press(
                                    &bound,
                                    fresh,
                                    report.song_time,
                                    report.audio_at.timestamp,
                                    &events,
                                )
                            }) {
                                input_commands.push(command);
                            }
                            report.bound_inputs.push(bound);
                            report.judge_events.extend(events);
                        }
                        Err(error) => {
                            report.judge_error = Some(error);
                            break;
                        }
                    }
                }
                if report.bound_inputs.is_empty() && report.judge_error.is_none() {
                    let counters = self.telemetry.counters_mut();
                    counters.unbound = counters.unbound.saturating_add(1);
                }
                report.input = Some(input);
            }
            self.publish(&mut report, &input_commands);
            Ok(report)
        })();
        self.observe(started, result.as_ref().is_err());
        result
    }

    /// Advances judge deadlines using explicit host and output clock points.
    /// After a gameplay fence, only acquisition chronology advances.
    pub fn advance_to(
        &mut self,
        host: ClockPoint,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, RuntimeError> {
        let started = self.processing_clock.start();
        let result = (|| {
            let (host, quality) = normalize(host, self.host_domain, mapper)?;
            let (mut report, mapped_song) = self.prepare(host, audio_at, mapper, quality)?;
            if self.gameplay_fence.is_some() {
                self.commit_time(host.timestamp, mapped_song);
                return Ok(report);
            }
            match self.judge.advance_to(report.song_time) {
                Ok(events) => {
                    report
                        .hazard_events
                        .extend_from_slice(self.judge.hazard_events());
                    report.judge_events = events;
                    self.commit_time(host.timestamp, mapped_song);
                }
                Err(error) => report.judge_error = Some(error),
            }
            self.publish(&mut report, &[]);
            Ok(report)
        })();
        self.observe(started, result.as_ref().is_err());
        result
    }

    fn prepare(
        &self,
        host: ClockPoint,
        audio_at: ClockPoint,
        mapper: &dyn ClockMapper,
        input_mapping_quality: ClockMappingQuality,
    ) -> Result<(RuntimeReport, Timestamp), RuntimeError> {
        if self.last_host.is_some_and(|last| host.timestamp < last) {
            return Err(RuntimeError::NonMonotonicHost);
        }
        let song_time = self
            .transport
            .position_at(host.timestamp)
            .map_err(RuntimeError::Transport)?;
        let segment = self
            .transport
            .anchors()
            .iter()
            .rev()
            .find(|anchor| anchor.host_time <= host.timestamp)
            .expect("successful transport mapping has an anchor");
        if segment.rate.numerator() < 0 || self.last_song.is_some_and(|last| song_time < last) {
            return Err(RuntimeError::RequiresReplayRestore);
        }
        let (audio_at, audio_mapping_quality) = normalize(audio_at, self.audio_domain, mapper)?;
        let song_end_reached = self.song_end.is_some_and(|end| song_time >= end);
        Ok((
            RuntimeReport {
                input: None,
                bound_inputs: Vec::new(),
                song_time: self
                    .gameplay_fence
                    .unwrap_or_else(|| self.song_end.map_or(song_time, |end| song_time.min(end))),
                song_end_reached,
                audio_at,
                input_mapping_quality,
                audio_mapping_quality,
                judge_events: Vec::new(),
                hazard_events: Vec::new(),
                judge_error: None,
                audio_commands: Vec::new(),
                audio_failures: Vec::new(),
            },
            song_time,
        ))
    }

    fn commit_time(&mut self, host: Timestamp, song: Timestamp) {
        self.input_sounds_locked = true;
        self.last_host = Some(host);
        self.last_song = Some(song);
    }

    /// Admit an explicit command to the same queue used by judged keysounds.
    ///
    /// The timestamp must already use the configured audio output domain.
    /// This control-thread operation performs no clock mapping or gameplay
    /// mutation. Exact failures are returned without retry; admission does not
    /// establish successful mixer execution or native playback.
    pub fn enqueue_audio(&mut self, command: AudioCommand) -> Result<(), CommandPushError> {
        admit_audio(&mut self.producer, self.telemetry.counters_mut(), command)
    }

    /// Acquires the shared cold pause hold without changing transport or judging.
    pub fn hold_audio_pause(
        &mut self,
    ) -> Result<crate::audio::PauseHold, crate::audio::PauseHoldError> {
        self.producer.hold_pause()
    }
    /// Request audio scheduling pause through the same producer as keysounds.
    /// This does not pause Transport or judging. The session owner must fence
    /// input/admission and coordinate them with actual render/presentation evidence.
    /// A full command ring cannot prevent a pause or resume request.
    pub fn request_audio_pause(&mut self, paused: bool) {
        self.producer.request_pause(paused);
    }

    fn publish(&mut self, report: &mut RuntimeReport, input_commands: &[AudioCommand]) {
        let counters = self.telemetry.counters_mut();
        counters.judge_results = counters
            .judge_results
            .saturating_add(report.judge_events.len() as u64);
        if report.judge_error.is_some() {
            counters.rejected = counters.rejected.saturating_add(1);
        }
        for event in &report.judge_events {
            if !matches!(event.outcome, JudgeOutcome::Hit { .. }) {
                continue;
            }
            for sound in &self.sounds {
                let Some(command) = sound.command_for(event, report.audio_at.timestamp) else {
                    continue;
                };
                match admit_gameplay_audio(
                    &mut self.producer,
                    counters,
                    &mut self.gameplay_sound_stop,
                    command,
                ) {
                    Ok(()) => {
                        report.audio_commands.push(command);
                    }
                    Err(error) => {
                        report.audio_failures.push(error);
                    }
                }
            }
        }
        // Accepted bound presses retain their order, after all judged sounds.
        // A later fanout or queue failure cannot erase an earlier sound attempt.
        for &command in input_commands {
            match admit_gameplay_audio(
                &mut self.producer,
                counters,
                &mut self.gameplay_sound_stop,
                command,
            ) {
                Ok(()) => report.audio_commands.push(command),
                Err(error) => report.audio_failures.push(error),
            }
        }
        if let Some(timeline) = &self.hazard_sounds {
            for event in &report.hazard_events {
                let Some(command) = timeline.command_for(event, report.audio_at.timestamp) else {
                    continue;
                };
                match admit_gameplay_audio(
                    &mut self.producer,
                    counters,
                    &mut self.gameplay_sound_stop,
                    command,
                ) {
                    Ok(()) => report.audio_commands.push(command),
                    Err(error) => report.audio_failures.push(error),
                }
            }
        }
    }

    fn observe(&mut self, started: ProcessingStarted, rejected: bool) {
        if let Some(nanos) = started.elapsed() {
            self.telemetry.record_processing_ns(nanos);
        }
        if rejected {
            let counters = self.telemetry.counters_mut();
            counters.rejected = counters.rejected.saturating_add(1);
        }
    }

    /// Exchanges the unique output endpoint for a control-owner composition.
    ///
    /// The caller must install the intended output queue before processing and
    /// restore ownership on every return or unwind. This does not clone a
    /// producer, reset chronology, or rewrite queued commands. Never call it
    /// from an audio callback; the installed queue uses this runtime's audio domain.
    pub fn exchange_audio_producer(&mut self, producer: &mut CommandProducer) {
        std::mem::swap(&mut self.producer, producer);
    }

    /// Exchanges the authoritative song mapping without resetting judge state.
    ///
    /// The caller coordinates forward chronology and restoration on every
    /// return/unwind. Shared control-owner compositions can thereby use one
    /// mapping without cloning its history for each operation.
    pub fn exchange_transport(&mut self, transport: &mut Transport) {
        std::mem::swap(&mut self.transport, transport);
    }

    /// Read-only transport mapping and history.
    pub const fn transport(&self) -> &Transport {
        &self.transport
    }
    /// Caller mutation; seek/reverse requires separately coordinated restoration.
    pub fn transport_mut(&mut self) -> &mut Transport {
        &mut self.transport
    }
    /// Read-only judge state for projection or deterministic recording.
    pub const fn judge(&self) -> &JudgeEngine {
        &self.judge
    }
    /// Caller access for replay restoration; runtime chronology remains unchanged.
    pub fn judge_mut(&mut self) -> &mut JudgeEngine {
        &mut self.judge
    }
    /// Replaces both gameplay owners after explicit replay reconstruction.
    /// Resets input chronology, acquisition sequences, song-end setup and the
    /// gameplay fence; leaves telemetry, bindings, configured input/hazard sounds
    /// and already queued audio commands intact. The caller must separately
    /// synchronize audio output when restoring a timeline. Configured
    /// touch regions remain, but held routes are cleared for a fresh contact start;
    /// restoring active contacts requires `replace_state_with_touch_router` instead.
    /// The caller must reconstruct its failure policy and refence a failed
    /// prefix when applicable after restoration.
    /// Prepared voices remain, but the stop-attempt latch and gameplay audio
    /// watermark reset. Stop/reset the old output before changing its timeline.
    pub fn replace_state(
        &mut self,
        judge: JudgeEngine,
        transport: Transport,
    ) -> (JudgeEngine, Transport) {
        let previous = self.replace_gameplay_state(judge, transport);
        if let Some(router) = &mut self.touch_router {
            router.clear();
        }
        previous
    }

    /// Installs explicitly paired judge, transport and contact routing owners.
    /// Returns all prior owners without clearing either router's held contacts.
    /// The caller must supply a coherent checkpoint and synchronize audio;
    /// chronology/sequences/song-end setup and the gameplay fence reset as in
    /// `replace_state`, including its caller-owned failure-policy restoration.
    pub fn replace_state_with_touch_router(
        &mut self,
        judge: JudgeEngine,
        transport: Transport,
        touch_router: Option<TouchRouter>,
    ) -> (JudgeEngine, Transport, Option<TouchRouter>) {
        let (previous_judge, previous_transport) = self.replace_gameplay_state(judge, transport);
        let previous_router = std::mem::replace(&mut self.touch_router, touch_router);
        (previous_judge, previous_transport, previous_router)
    }

    fn replace_gameplay_state(
        &mut self,
        judge: JudgeEngine,
        transport: Transport,
    ) -> (JudgeEngine, Transport) {
        let previous_judge = std::mem::replace(&mut self.judge, judge);
        let previous_transport = std::mem::replace(&mut self.transport, transport);
        self.last_host = None;
        self.last_song = None;
        self.song_end = None;
        self.gameplay_fence = None;
        self.gameplay_sound_stop.reset();
        self.sequences.clear();
        (previous_judge, previous_transport)
    }

    /// Installs restored gameplay and a fresh audio producer in one owner operation.
    ///
    /// Stop/reset the old native output first and reconstruct `judge` at the
    /// replacement Transport's song origin. Producer timestamps must use this
    /// Runtime's configured audio domain. Returns all old control owners for
    /// off-thread disposal; old native buffers are not flushed by this method.
    pub fn replace_session(
        &mut self,
        judge: JudgeEngine,
        transport: Transport,
        producer: CommandProducer,
    ) -> (JudgeEngine, Transport, CommandProducer) {
        let (previous_judge, previous_transport) = self.replace_state(judge, transport);
        let previous_producer = std::mem::replace(&mut self.producer, producer);
        (previous_judge, previous_transport, previous_producer)
    }

    /// Current software observations and caller-reported native counters.
    pub const fn telemetry(&self) -> &RuntimeTelemetry {
        &self.telemetry
    }
    /// Selects future software duration measurements without clearing telemetry
    /// or changing gameplay, chronology, transport or queued audio commands.
    pub fn set_processing_clock(&mut self, clock: RuntimeProcessingClock) {
        self.processing_clock = clock;
    }
    /// Host reporting of actual loss and underruns.
    pub fn telemetry_mut(&mut self) -> &mut RuntimeTelemetry {
        &mut self.telemetry
    }
}

fn admit_gameplay_audio(
    producer: &mut CommandProducer,
    counters: &mut RuntimeCounters,
    stops: &mut GameplaySoundStop,
    command: AudioCommand,
) -> Result<(), CommandPushError> {
    admit_audio(producer, counters, command)?;
    stops.observe_admitted(command.at());
    Ok(())
}

fn admit_audio(
    producer: &mut CommandProducer,
    counters: &mut RuntimeCounters,
    command: AudioCommand,
) -> Result<(), CommandPushError> {
    let result = producer.try_push(command);
    match &result {
        Ok(()) => counters.audio_commands = counters.audio_commands.saturating_add(1),
        Err(error) => match error.reason {
            QueuePushError::Full => counters.queue_full = counters.queue_full.saturating_add(1),
            QueuePushError::Disconnected => {
                counters.queue_disconnected = counters.queue_disconnected.saturating_add(1)
            }
        },
    }
    result
}

fn normalize(
    point: ClockPoint,
    to: ClockDomainId,
    mapper: &dyn ClockMapper,
) -> Result<(ClockPoint, ClockMappingQuality), RuntimeError> {
    if point.domain == to {
        return Ok((point, ClockMappingQuality::Exact));
    }
    let timestamp = mapper.map(point, to).ok_or(RuntimeError::UnmappedClock {
        from: point.domain,
        to,
    })?;
    Ok((
        ClockPoint {
            domain: to,
            timestamp,
        },
        mapper.quality(),
    ))
}
