//! Single-owner forward runtime connecting canonical input to scalar audio.

pub mod playback;
pub mod restart;

use crate::{
    audio::{AudioCommand, CommandProducer, CommandPushError, QueuePushError, SampleId, VoiceId},
    chart::ObjectId,
    input::{BindingMap, DeviceId, GameInputEvent, PhysicalInputEvent},
    judge::{JudgeEngine, JudgeError, JudgeEvent, JudgeOutcome, JudgeStage},
    telemetry::{RuntimeCounters, RuntimeTelemetry},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
    transport::{Transport, TransportError},
};
use std::{collections::HashMap, fmt, time::Instant};

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
    /// Independently supplied and normalized output scheduling point.
    pub audio_at: ClockPoint,
    /// Mapper's declared relation quality (same-domain identity is exact).
    pub input_mapping_quality: ClockMappingQuality,
    /// Quality declared for output-time mapping.
    pub audio_mapping_quality: ClockMappingQuality,
    /// Emitted judge results; never undone by queue failures.
    pub judge_events: Vec<JudgeEvent>,
    /// First judge fanout failure; prior results remain committed.
    pub judge_error: Option<JudgeError>,
    /// Successfully published scalar commands.
    pub audio_commands: Vec<AudioCommand>,
    /// Exact failed commands and reasons, with no implicit retry.
    pub audio_failures: Vec<CommandPushError>,
}

/// Owns control-thread state; no callbacks or platform dependency.
pub struct Runtime {
    host_domain: ClockDomainId,
    audio_domain: ClockDomainId,
    transport: Transport,
    bindings: BindingMap,
    judge: JudgeEngine,
    producer: CommandProducer,
    sounds: Vec<SoundBinding>,
    last_host: Option<Timestamp>,
    last_song: Option<Timestamp>,
    sequences: HashMap<DeviceId, u64>,
    telemetry: RuntimeTelemetry,
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
        Ok(Self {
            host_domain,
            audio_domain,
            transport,
            bindings,
            judge,
            producer,
            sounds,
            last_host: None,
            last_song: None,
            sequences: HashMap::new(),
            telemetry: RuntimeTelemetry::new(telemetry_capacity),
        })
    }

    /// Normalizes, binds, judges and publishes at an independently supplied output time.
    /// Queue failures do not fail the operation or revert judge state. Binding fanout
    /// stops at its first judge error; inspect `judge_error` before continuing.
    pub fn process_input(
        &mut self,
        mut input: PhysicalInputEvent,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, RuntimeError> {
        let started = Instant::now();
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
            let mut report = self.prepare(host, audio_at, mapper, input_quality)?;
            if incoming.domain != self.host_domain {
                let meta = input.meta_mut();
                meta.original_clock_point.get_or_insert(incoming);
                meta.timestamp = host.timestamp;
                meta.clock_domain = host.domain;
            }
            self.sequences.insert(meta.source, meta.sequence);
            self.commit_time(host.timestamp, report.song_time);
            let counters = self.telemetry.counters_mut();
            counters.inputs = counters.inputs.saturating_add(1);
            for bound in self.bindings.map(&input) {
                match self.judge.push_input(&bound, report.song_time) {
                    Ok(events) => {
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
            self.publish(&mut report);
            Ok(report)
        })();
        self.observe(started, result.as_ref().is_err());
        result
    }

    /// Advances judge deadlines using explicit host and output clock points.
    pub fn advance_to(
        &mut self,
        host: ClockPoint,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, RuntimeError> {
        let started = Instant::now();
        let result = (|| {
            let (host, quality) = normalize(host, self.host_domain, mapper)?;
            let mut report = self.prepare(host, audio_at, mapper, quality)?;
            match self.judge.advance_to(report.song_time) {
                Ok(events) => {
                    report.judge_events = events;
                    self.commit_time(host.timestamp, report.song_time);
                }
                Err(error) => report.judge_error = Some(error),
            }
            self.publish(&mut report);
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
    ) -> Result<RuntimeReport, RuntimeError> {
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
        Ok(RuntimeReport {
            input: None,
            bound_inputs: Vec::new(),
            song_time,
            audio_at,
            input_mapping_quality,
            audio_mapping_quality,
            judge_events: Vec::new(),
            judge_error: None,
            audio_commands: Vec::new(),
            audio_failures: Vec::new(),
        })
    }

    fn commit_time(&mut self, host: Timestamp, song: Timestamp) {
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

    /// Request audio scheduling pause through the same producer as keysounds.
    /// This does not pause Transport or judging. The session owner must fence
    /// input/admission and coordinate them with actual render/presentation evidence.
    /// A full command ring cannot prevent a pause or resume request.
    pub fn request_audio_pause(&mut self, paused: bool) {
        self.producer.request_pause(paused);
    }

    fn publish(&mut self, report: &mut RuntimeReport) {
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
                match admit_audio(&mut self.producer, counters, command) {
                    Ok(()) => {
                        report.audio_commands.push(command);
                    }
                    Err(error) => {
                        report.audio_failures.push(error);
                    }
                }
            }
        }
    }

    fn observe(&mut self, started: Instant, rejected: bool) {
        self.telemetry
            .record_processing_ns(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
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
    /// Resets input chronology and acquisition sequences; leaves telemetry,
    /// bindings and already queued audio commands intact. The caller must
    /// separately synchronize audio output when restoring a timeline.
    pub fn replace_state(
        &mut self,
        judge: JudgeEngine,
        transport: Transport,
    ) -> (JudgeEngine, Transport) {
        let previous_judge = std::mem::replace(&mut self.judge, judge);
        let previous_transport = std::mem::replace(&mut self.transport, transport);
        self.last_host = None;
        self.last_song = None;
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
    /// Host reporting of actual loss and underruns.
    pub fn telemetry_mut(&mut self) -> &mut RuntimeTelemetry {
        &mut self.telemetry
    }
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
