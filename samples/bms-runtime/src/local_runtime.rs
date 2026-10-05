//! Local cohorts execute the existing Runtime with one transport and audio owner.
use crate::local_players::{PlayerId, validate_source_routes};
use beatkernel::{
    audio::{AudioCommand, CommandProducer, CommandPushError, VoiceId, command_queue},
    input::{
        BindingMap, DeviceId, DeviceSelector, PhysicalInputEvent, Position2, TouchRegion,
        TouchRouter,
    },
    judge::JudgeEngine,
    runtime::{
        Runtime, RuntimeError, RuntimeProcessingClock, RuntimeReport, SoundBinding,
        input_sound::InputSoundTimeline,
        hazard_sound::{HazardSoundBinding, HazardSoundTimeline},
    },
    telemetry::RuntimeTelemetry,
    time::{ClockDomainId, ClockMapper, ClockPoint, Timestamp},
    transport::{Rate, Transport},
};
use std::collections::{BTreeMap, HashMap, HashSet};

pub struct MemberConfig {
    pub player: PlayerId,
    pub device: Option<DeviceId>,
    pub bindings: BindingMap,
    pub judge: JudgeEngine,
    pub sounds: Vec<SoundBinding>,
}

#[derive(Clone, Debug)]
pub struct PlayerReport {
    pub player: PlayerId,
    pub report: RuntimeReport,
}

#[derive(Clone, Debug)]
pub enum InputResult {
    Ignored { device: DeviceId },
    Processed(Vec<PlayerReport>),
}

#[derive(Clone, Debug)]
pub enum FailureKind {
    Poisoned,
    Core(RuntimeError),
    /// Exact judge/audio failure evidence is in the last completed report.
    ReportedFailure,
}

#[derive(Clone, Debug)]
pub struct GroupError {
    pub failed_player: Option<PlayerId>,
    pub kind: FailureKind,
    /// Includes the failing member's report when core returned a partial report.
    pub completed_reports: Vec<PlayerReport>,
}
impl std::fmt::Display for GroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "local runtime {:?} failed: {:?}; {} reports remain committed",
            self.failed_player,
            self.kind,
            self.completed_reports.len()
        )
    }
}
impl std::error::Error for GroupError {}

struct Member {
    player: PlayerId,
    device: Option<DeviceId>,
    runtime: Runtime,
}

/// One control owner. Audio callbacks never borrow or execute this composition.
pub struct RuntimeGroup {
    members: Vec<Member>,
    transport: Transport,
    producer: CommandProducer,
    poisoned: bool,
    started: bool,
    song_end: Option<Timestamp>,
    occupied_voices: HashSet<VoiceId>,
    input_sounds_configured: bool,
    hazard_sounds_configured: bool,
}

/// Parks the member's disconnected placeholders while it uses shared owners.
/// No producer or transport cloning occurs; Drop restores both on unwind.
struct OwnerGuard<'a> {
    runtime: &'a mut Runtime,
    transport: &'a mut Transport,
    producer: &'a mut CommandProducer,
}
impl<'a> OwnerGuard<'a> {
    fn new(
        runtime: &'a mut Runtime,
        transport: &'a mut Transport,
        producer: &'a mut CommandProducer,
    ) -> Self {
        runtime.exchange_transport(transport);
        runtime.exchange_audio_producer(producer);
        Self {
            runtime,
            transport,
            producer,
        }
    }
}
impl Drop for OwnerGuard<'_> {
    fn drop(&mut self) {
        self.runtime.exchange_audio_producer(self.producer);
        self.runtime.exchange_transport(self.transport);
    }
}

impl RuntimeGroup {
    /// Selects only software profiling for every actual member Runtime.
    /// Browser hosts must configure this before processing any member.
    pub fn set_processing_clock(&mut self, clock: RuntimeProcessingClock) {
        for member in &mut self.members {
            member.runtime.set_processing_clock(clock);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host_domain: ClockDomainId,
        audio_domain: ClockDomainId,
        transport: Transport,
        producer: CommandProducer,
        configs: Vec<MemberConfig>,
        telemetry_capacity: usize,
        reserved_bgm_voices: &[VoiceId],
    ) -> Result<Self, String> {
        validate_source_routes(configs.iter().map(|config| (config.player, config.device)))?;
        if telemetry_capacity > 65_536
            || telemetry_capacity
                .checked_mul(configs.len())
                .is_none_or(|total| total > 1_048_576)
        {
            return Err("local telemetry exceeds per-member/aggregate sample capacity".into());
        }
        let reserved: HashSet<_> = reserved_bgm_voices.iter().copied().collect();
        let mut voices = HashMap::new();
        for config in &configs {
            if let Some(device) = config.device {
                if config
                    .bindings
                    .bindings()
                    .iter()
                    .any(|binding| binding.device != DeviceSelector::Exact(device))
                {
                    return Err("assigned player's bindings must select its exact device".into());
                }
            }
            for sound in &config.sounds {
                if reserved.contains(&sound.voice) {
                    return Err("player voice collides with reserved BGM voice".into());
                }
                if voices
                    .insert(sound.voice, config.player)
                    .is_some_and(|owner| owner != config.player)
                {
                    return Err("voice identity collides across local players".into());
                }
            }
        }
        let mut occupied_voices = reserved;
        occupied_voices.extend(voices.into_keys());
        let mut members = Vec::with_capacity(configs.len());
        for config in configs {
            let (placeholder, disconnected) =
                command_queue(1).map_err(|error| error.to_string())?;
            drop(disconnected);
            let runtime = Runtime::new(
                host_domain,
                audio_domain,
                Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
                config.bindings,
                config.judge,
                placeholder,
                config.sounds,
                telemetry_capacity,
            )
            .map_err(|error| error.to_string())?;
            members.push(Member {
                player: config.player,
                device: config.device,
                runtime,
            });
        }
        Ok(Self {
            members,
            transport,
            producer,
            poisoned: false,
            started: false,
            song_end: None,
            occupied_voices,
            input_sounds_configured: false,
            hazard_sounds_configured: false,
        })
    }

    /// Installs one timeline per member in exact source-plan order. All voice
    /// checks precede any private Runtime mutation, so refusals permit retry.
    pub fn configure_input_sounds(
        &mut self,
        timelines: Vec<(PlayerId, InputSoundTimeline)>,
    ) -> Result<(), String> {
        if self.poisoned || self.started || self.input_sounds_configured {
            return Err("shared input sound configuration is locked".into());
        }
        if timelines.len() != self.members.len() {
            return Err("input sound timeline count differs from cohort".into());
        }
        let mut voices = HashMap::new();
        for (member, (player, timeline)) in self.members.iter().zip(&timelines) {
            if member.player != *player {
                return Err("input sound players differ from source-plan order".into());
            }
            for marker in timeline.markers() {
                if self.occupied_voices.contains(&marker.voice) {
                    return Err("input sound voice collides with an occupied voice".into());
                }
                if voices
                    .insert(marker.voice, *player)
                    .is_some_and(|owner| owner != *player)
                {
                    return Err("input sound voice collides across local players".into());
                }
            }
        }
        self.occupied_voices
            .try_reserve(voices.len())
            .map_err(|_| "input sound voice reservation failed")?;
        for (member, (_, timeline)) in self.members.iter_mut().zip(timelines) {
            member
                .runtime
                .configure_input_sounds(timeline)
                .expect("validated fresh private member input sound setup cannot reject");
        }
        self.occupied_voices.extend(voices.into_keys());
        self.input_sounds_configured = true;
        Ok(())
    }

    /// Installs exact roster-ordered hazard sounds after complete voice preflight.
    /// Within-member aliases are retained; other occupied voices cannot collide.
    pub fn configure_hazard_sounds(
        &mut self,
        timelines: Vec<(PlayerId, HazardSoundTimeline)>,
    ) -> Result<(), String> {
        if self.poisoned || self.started || self.hazard_sounds_configured {
            return Err("shared hazard sound configuration is locked".into());
        }
        if timelines.len() != self.members.len() {
            return Err("hazard sound timeline count differs from cohort".into());
        }
        let mut voices = HashMap::new();
        for (member, (player, timeline)) in self.members.iter().zip(&timelines) {
            if member.player != *player {
                return Err("hazard sound players differ from source-plan order".into());
            }
            for binding in timeline.bindings() {
                if self.occupied_voices.contains(&binding.voice) {
                    return Err("hazard sound voice collides with an occupied voice".into());
                }
                if voices
                    .insert(binding.voice, *player)
                    .is_some_and(|owner| owner != *player)
                {
                    return Err("hazard sound voice collides across local players".into());
                }
            }
        }
        self.occupied_voices
            .try_reserve(voices.len())
            .map_err(|_| "hazard sound voice reservation failed")?;
        for (member, (_, timeline)) in self.members.iter_mut().zip(timelines) {
            member
                .runtime
                .configure_hazard_sounds(timeline)
                .expect("validated fresh private member hazard sound setup cannot reject");
        }
        self.occupied_voices.extend(voices.into_keys());
        self.hazard_sounds_configured = true;
        Ok(())
    }

    /// Install one immutable original-song boundary before any member runs.
    /// Private members are unconfigured and unprocessed while `started` is false.
    pub fn set_song_end(&mut self, end: Timestamp) -> Result<(), String> {
        if end.as_nanos() < 0 {
            return Err("song end must be nonnegative".into());
        }
        if self.poisoned || self.started || self.song_end.is_some() {
            return Err("shared song end configuration is locked".into());
        }
        for member in &mut self.members {
            member
                .runtime
                .set_song_end(end)
                .expect("validated private member setup cannot reject the shared end");
        }
        self.song_end = Some(end);
        Ok(())
    }
    pub const fn song_end(&self) -> Option<Timestamp> {
        self.song_end
    }

    /// Installs one member's spatial routing before the shared owner starts.
    /// Assigned devices must be exact in every region, as for static bindings.
    pub fn configure_touch_router(
        &mut self,
        player: PlayerId,
        router: TouchRouter,
    ) -> Result<(), String> {
        if self.poisoned || self.started {
            return Err("shared touch routing configuration is locked".into());
        }
        let member = self
            .members
            .iter_mut()
            .find(|member| member.player == player)
            .ok_or_else(|| "touch routing player is not in this cohort".to_string())?;
        if let Some(device) = member.device {
            if router
                .regions()
                .iter()
                .any(|region| region.device != DeviceSelector::Exact(device))
            {
                return Err("assigned player's touch regions must select its exact device".into());
            }
        }
        member
            .runtime
            .configure_touch_router(router)
            .map_err(|error| error.to_string())
    }

    /// Layout control does not execute a member or alter shared chronology.
    pub fn remap_touch_regions(
        &mut self,
        player: PlayerId,
        regions: Vec<TouchRegion>,
    ) -> Result<(), String> {
        self.ensure_usable().map_err(|error| error.to_string())?;
        let member = self
            .members
            .iter_mut()
            .find(|member| member.player == player)
            .ok_or_else(|| "touch routing player is not in this cohort".to_string())?;
        member
            .runtime
            .remap_touch_regions(regions)
            .map_err(|error| error.to_string())
    }

    /// Hidden players keep existing bound/unbound contacts; only fresh contact
    /// selection changes. Poisoned or absent owners refuse without mutation.
    pub fn set_touch_routing_enabled(
        &mut self,
        player: PlayerId,
        enabled: bool,
    ) -> Result<(), String> {
        self.ensure_usable().map_err(|error| error.to_string())?;
        let member = self
            .members
            .iter_mut()
            .find(|member| member.player == player)
            .ok_or_else(|| "touch routing player is not in this cohort".to_string())?;
        member
            .runtime
            .set_touch_routing_enabled(enabled)
            .map_err(|error| error.to_string())
    }

    fn ensure_usable(&self) -> Result<(), GroupError> {
        if self.poisoned {
            Err(GroupError {
                failed_player: None,
                kind: FailureKind::Poisoned,
                completed_reports: Vec::new(),
            })
        } else {
            Ok(())
        }
    }

    pub fn process_input(
        &mut self,
        input: PhysicalInputEvent,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<InputResult, GroupError> {
        self.process_input_with_position(input, None, mapper, audio_at)
    }

    pub fn process_input_at(
        &mut self,
        input: PhysicalInputEvent,
        position: Position2,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<InputResult, GroupError> {
        self.process_input_with_position(input, Some(position), mapper, audio_at)
    }

    fn process_input_with_position(
        &mut self,
        input: PhysicalInputEvent,
        position: Option<Position2>,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<InputResult, GroupError> {
        self.ensure_usable()?;
        let device = input.meta().source;
        let Some(index) = self
            .members
            .iter()
            .position(|member| member.device.is_none_or(|selected| selected == device))
        else {
            return Ok(InputResult::Ignored { device });
        };
        self.poisoned = true;
        self.started = true;
        let player = self.members[index].player;
        let result = {
            let guard = OwnerGuard::new(
                &mut self.members[index].runtime,
                &mut self.transport,
                &mut self.producer,
            );
            match position {
                Some(position) => guard
                    .runtime
                    .process_input_at(input, position, mapper, audio_at),
                None => guard.runtime.process_input(input, mapper, audio_at),
            }
        };
        let report = result.map_err(|error| GroupError {
            failed_player: Some(player),
            kind: FailureKind::Core(error),
            completed_reports: Vec::new(),
        })?;
        let failed = report.judge_error.is_some() || !report.audio_failures.is_empty();
        let reports = vec![PlayerReport { player, report }];
        if failed {
            return Err(GroupError {
                failed_player: Some(player),
                kind: FailureKind::ReportedFailure,
                completed_reports: reports,
            });
        }
        self.poisoned = false;
        Ok(InputResult::Processed(reports))
    }

    pub fn advance_to(
        &mut self,
        host: ClockPoint,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<Vec<PlayerReport>, GroupError> {
        self.ensure_usable()?;
        self.poisoned = true;
        self.started = true;
        let mut reports = Vec::with_capacity(self.members.len());
        for member in &mut self.members {
            let result = {
                let guard =
                    OwnerGuard::new(&mut member.runtime, &mut self.transport, &mut self.producer);
                guard.runtime.advance_to(host, mapper, audio_at)
            };
            let report = match result {
                Ok(report) => report,
                Err(error) => {
                    return Err(GroupError {
                        failed_player: Some(member.player),
                        kind: FailureKind::Core(error),
                        completed_reports: reports,
                    });
                }
            };
            let failed = report.judge_error.is_some() || !report.audio_failures.is_empty();
            reports.push(PlayerReport {
                player: member.player,
                report,
            });
            if failed {
                return Err(GroupError {
                    failed_player: Some(member.player),
                    kind: FailureKind::ReportedFailure,
                    completed_reports: reports,
                });
            }
        }
        self.poisoned = false;
        Ok(reports)
    }

    pub fn poisoned(&self) -> bool {
        self.poisoned
    }
    pub fn transport(&self) -> &Transport {
        &self.transport
    }
    /// Host control remains available for cleanup; mutation does not clear poison.
    pub fn transport_mut(&mut self) -> &mut Transport {
        &mut self.transport
    }
    pub fn member_judge(&self, player: PlayerId) -> Option<&JudgeEngine> {
        self.members
            .iter()
            .find(|member| member.player == player)
            .map(|member| member.runtime.judge())
    }
    pub fn member_telemetry(&self, player: PlayerId) -> Option<&RuntimeTelemetry> {
        self.members
            .iter()
            .find(|member| member.player == player)
            .map(|member| member.runtime.telemetry())
    }
    /// Explicit queue admission/cleanup remains available after logical failure.
    /// Callers must stop gameplay after failure; admission never resumes judging.
    pub fn enqueue_audio(&mut self, command: AudioCommand) -> Result<(), CommandPushError> {
        self.producer.try_push(command)
    }
    /// Shared output control only; acknowledged Transport/input coordination
    /// remains the native session owner's responsibility for the whole cohort.
    pub fn request_audio_pause(&mut self, paused: bool) {
        self.producer.request_pause(paused);
    }
}

/// Production solo adapter uses exactly the same cohort execution path.
pub struct SoloRuntime(RuntimeGroup);
impl SoloRuntime {
    /// Uses the same atomic one-member hazard-sound installation as local play.
    pub fn configure_hazard_sounds(&mut self, timeline: HazardSoundTimeline) -> Result<(), String> {
        self.0
            .configure_hazard_sounds(vec![(PlayerId(1), timeline)])
    }

    /// Uses the same atomic one-member input-sound installation as local play.
    pub fn configure_input_sounds(&mut self, timeline: InputSoundTimeline) -> Result<(), String> {
        self.0.configure_input_sounds(vec![(PlayerId(1), timeline)])
    }

    pub fn configure_touch_router(&mut self, router: TouchRouter) -> Result<(), String> {
        self.0.configure_touch_router(PlayerId(1), router)
    }

    /// Uses the same member profiling configuration as a local cohort.
    pub fn set_processing_clock(&mut self, clock: RuntimeProcessingClock) {
        self.0.set_processing_clock(clock);
    }

    pub fn set_song_end(&mut self, end: Timestamp) -> Result<(), String> {
        self.0.set_song_end(end)
    }
    pub const fn song_end(&self) -> Option<Timestamp> {
        self.0.song_end()
    }
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: ClockDomainId,
        audio: ClockDomainId,
        transport: Transport,
        bindings: BindingMap,
        judge: JudgeEngine,
        producer: CommandProducer,
        sounds: Vec<SoundBinding>,
        telemetry_capacity: usize,
    ) -> Result<Self, String> {
        Ok(Self(RuntimeGroup::new(
            host,
            audio,
            transport,
            producer,
            vec![MemberConfig {
                player: PlayerId(1),
                device: None,
                bindings,
                judge,
                sounds,
            }],
            telemetry_capacity,
            &[],
        )?))
    }
    pub fn process_input(
        &mut self,
        input: PhysicalInputEvent,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, GroupError> {
        Self::sole_input(self.0.process_input(input, mapper, audio_at))
    }
    pub fn process_input_at(
        &mut self,
        input: PhysicalInputEvent,
        position: Position2,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, GroupError> {
        Self::sole_input(self.0.process_input_at(input, position, mapper, audio_at))
    }
    fn sole_input(result: Result<InputResult, GroupError>) -> Result<RuntimeReport, GroupError> {
        match result {
            Ok(InputResult::Processed(reports)) => Self::sole_report(Ok(reports)),
            Err(error) => Self::sole_report(Err(error)),
            Ok(InputResult::Ignored { .. }) => {
                unreachable!("solo adapter accepts every source, preserving core binding semantics")
            }
        }
    }
    pub fn advance_to(
        &mut self,
        host: ClockPoint,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, GroupError> {
        Self::sole_report(self.0.advance_to(host, mapper, audio_at))
    }
    // Existing native capture/UI paths must receive committed partial reports.
    // Group poison still prevents any subsequent logical continuation.
    fn sole_report(
        result: Result<Vec<PlayerReport>, GroupError>,
    ) -> Result<RuntimeReport, GroupError> {
        match result {
            Ok(mut reports) => Ok(reports.remove(0).report),
            Err(mut error) if matches!(error.kind, FailureKind::ReportedFailure) => {
                Ok(error.completed_reports.remove(0).report)
            }
            Err(error) => Err(error),
        }
    }
    pub fn enqueue_audio(&mut self, command: AudioCommand) -> Result<(), CommandPushError> {
        let guard = OwnerGuard::new(
            &mut self.0.members[0].runtime,
            &mut self.0.transport,
            &mut self.0.producer,
        );
        guard.runtime.enqueue_audio(command)
    }
    pub fn transport_mut(&mut self) -> &mut Transport {
        self.0.transport_mut()
    }
    /// Uses the same shared output control as a multi-player cohort.
    pub fn request_audio_pause(&mut self, paused: bool) {
        self.0.request_audio_pause(paused);
    }
    pub fn judge(&self) -> &JudgeEngine {
        self.0.members[0].runtime.judge()
    }
    pub fn telemetry(&self) -> &RuntimeTelemetry {
        self.0.members[0].runtime.telemetry()
    }
}

/// Checked setup-only voice remapping. Calls for different players allocate
/// distinct namespaces; repeated voices within one player retain replacement.
pub struct VoiceAllocator {
    next: Option<u64>,
}
impl VoiceAllocator {
    /// Caller chooses a range excluding its reserved BGM voices.
    pub fn new(first: u64) -> Self {
        Self { next: Some(first) }
    }
    /// Overflow leaves both sounds and allocator unchanged.
    pub fn remap(&mut self, sounds: &mut [SoundBinding]) -> Result<(), String> {
        let mut mapping = BTreeMap::new();
        let mut next = self.next;
        for sound in sounds.iter() {
            if let std::collections::btree_map::Entry::Vacant(entry) = mapping.entry(sound.voice.0)
            {
                let id = next.ok_or("voice identity namespace exhausted")?;
                entry.insert(VoiceId(id));
                next = id.checked_add(1);
            }
        }
        for sound in sounds {
            sound.voice = mapping[&sound.voice.0];
        }
        self.next = next;
        Ok(())
    }

    /// Remaps distinct fallback voices, preserving each lane's replacement
    /// aliases. Exhaustion leaves every marker and the allocator unchanged.
    pub fn remap_input_sounds(
        &mut self,
        markers: &mut [beatkernel::runtime::input_sound::InputSoundMarker],
    ) -> Result<(), String> {
        let mut mapping = BTreeMap::new();
        let mut next = self.next;
        for marker in markers.iter() {
            if let std::collections::btree_map::Entry::Vacant(entry) = mapping.entry(marker.voice.0)
            {
                let id = next.ok_or("voice identity namespace exhausted")?;
                entry.insert(VoiceId(id));
                next = id.checked_add(1);
            }
        }
        for marker in markers {
            marker.voice = mapping[&marker.voice.0];
        }
        self.next = next;
        Ok(())
    }

    /// Remaps distinct hazard voices atomically, retaining intentional aliases.
    pub fn remap_hazard_sounds(
        &mut self,
        bindings: &mut [HazardSoundBinding],
    ) -> Result<(), String> {
        let mut mapping = BTreeMap::new();
        let mut next = self.next;
        for binding in bindings.iter() {
            if let std::collections::btree_map::Entry::Vacant(entry) =
                mapping.entry(binding.voice.0)
            {
                let id = next.ok_or("voice identity namespace exhausted")?;
                entry.insert(VoiceId(id));
                next = id.checked_add(1);
            }
        }
        for binding in bindings {
            binding.voice = mapping[&binding.voice.0];
        }
        self.next = next;
        Ok(())
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::{
        audio::{CommandConsumer, SampleId},
        chart::{
            Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
        },
        input::{Binding, ButtonEvent, ButtonState, EventMeta, GameControlId, PhysicalControlId},
        interaction::InstantEvaluator,
        judge::{JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, Rule},
        time::{ClockMappingQuality, Duration},
    };
    struct Identity;
    impl ClockMapper for Identity {
        fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
            None
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Exact
        }
    }
    fn point(at: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(at),
        }
    }
    fn input(device: u64, at: i64) -> PhysicalInputEvent {
        PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(device), point(at), 1),
            control: PhysicalControlId::keyboard(4),
            state: ButtonState::Down,
        })
    }
    fn config(player: u32) -> MemberConfig {
        let mut source = SourceChart::new(1000, Bpm::new(60, 1).unwrap()).unwrap();
        source.objects.push(SourceObject {
            id: ObjectId(1),
            start: Beat::new(2).unwrap(),
            end: None,
            interaction: InteractionId(1),
            visual: VisualId(1),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
        MemberConfig {
            player: PlayerId(player),
            device: Some(DeviceId(u64::from(player))),
            bindings: BindingMap::from_bindings([Binding {
                device: DeviceSelector::Exact(DeviceId(u64::from(player))),
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(1),
            }])
            .unwrap(),
            judge: JudgeEngine::new(
                source.compile().unwrap(),
                vec![Rule {
                    interaction: InteractionId(1),
                    control: GameControlId(1),
                    evaluator: Box::new(InstantEvaluator),
                }],
                JudgeProfile::new(
                    vec![JudgeWindow {
                        grade: JudgeGrade(1),
                        early: Duration::ZERO,
                        late: Duration::ZERO,
                    }],
                    Duration::ZERO,
                )
                .unwrap(),
            )
            .unwrap(),
            sounds: vec![SoundBinding {
                object: ObjectId(1),
                stage: JudgeStage::Instant,
                sample: SampleId(1),
                voice: VoiceId(u64::from(player)),
                gain: 1.0,
            }],
        }
    }
    fn group(capacity: usize) -> (RuntimeGroup, CommandConsumer) {
        let (producer, consumer) = command_queue(capacity).unwrap();
        (
            RuntimeGroup::new(
                ClockDomainId(1),
                ClockDomainId(1),
                Transport::new(Timestamp::from_nanos(100), Timestamp::ZERO, Rate::NORMAL),
                producer,
                (1..=4).map(config).collect(),
                8,
                &[],
            )
            .unwrap(),
            consumer,
        )
    }

    #[test]
    fn four_players_route_once_and_advance_independent_deadlines_at_shared_time() {
        let (mut group, mut consumer) = group(8);
        assert!(matches!(
            group
                .process_input(input(99, i64::MAX), &Identity, point(0))
                .unwrap(),
            InputResult::Ignored {
                device: DeviceId(99)
            }
        ));
        let InputResult::Processed(reports) = group
            .process_input(input(1, 2_000_100), &Identity, point(2_000_100))
            .unwrap()
        else {
            panic!("assigned input ignored")
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].player, PlayerId(1));
        assert_eq!(reports[0].report.audio_commands.len(), 1);
        assert!(matches!(
            consumer.try_pop().unwrap(),
            AudioCommand::Play {
                voice: VoiceId(1),
                ..
            }
        ));
        let reports = group
            .advance_to(point(2_000_101), &Identity, point(2_000_101))
            .unwrap();
        assert_eq!(reports.len(), 4);
        assert!(
            reports
                .iter()
                .all(|r| r.report.song_time == Timestamp::from_nanos(2_000_001))
        );
        assert!(reports[0].report.judge_events.is_empty());
        for report in &reports[1..] {
            assert_eq!(report.report.judge_events.len(), 1);
            assert!(matches!(
                report.report.judge_events[0].outcome,
                beatkernel::judge::JudgeOutcome::Miss { .. }
            ));
        }
    }

    #[test]
    fn setup_rejects_device_voice_and_storage_collisions() {
        for case in 0..7 {
            let (producer, _consumer) = command_queue(4).unwrap();
            let mut members: Vec<_> = (1..=4).map(config).collect();
            let mut reserved = Vec::new();
            let mut telemetry = 8;
            match case {
                0 => members[1].device = members[0].device,
                1 => members[1].sounds[0].voice = members[0].sounds[0].voice,
                2 => reserved.push(VoiceId(1)),
                3 => members[0].device = None,
                4 => telemetry = 65_537,
                5 => members.clear(),
                _ => members = (1..=65).map(config).collect(),
            }
            assert!(
                RuntimeGroup::new(
                    ClockDomainId(1),
                    ClockDomainId(1),
                    Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
                    producer,
                    members,
                    telemetry,
                    &reserved
                )
                .is_err()
            );
        }
    }

    #[test]
    fn actual_queue_failure_fences_judging_and_keeps_committed_report() {
        let (mut group, _consumer) = group(1);
        group
            .enqueue_audio(AudioCommand::Stop {
                voice: VoiceId(99),
                at: Timestamp::ZERO,
            })
            .unwrap();
        let failure = group
            .process_input(input(1, 2_000_100), &Identity, point(2_000_100))
            .unwrap_err();
        assert_eq!(failure.completed_reports.len(), 1);
        assert_eq!(failure.completed_reports[0].report.judge_events.len(), 1);
        assert_eq!(failure.completed_reports[0].report.audio_failures.len(), 1);
        assert!(group.poisoned());
        assert!(matches!(
            group
                .advance_to(point(2_000_101), &Identity, point(2_000_101))
                .unwrap_err()
                .kind,
            FailureKind::Poisoned
        ));
    }

    #[test]
    fn partial_deadline_failure_keeps_earlier_members_reports() {
        let (mut group, _consumer) = group(8);
        group
            .process_input(input(2, 2_000_100), &Identity, point(2_000_100))
            .unwrap();
        let failure = group
            .advance_to(point(1_000_100), &Identity, point(1_000_100))
            .unwrap_err();
        assert_eq!(failure.failed_player, Some(PlayerId(2)));
        assert_eq!(failure.completed_reports.len(), 1);
        assert_eq!(failure.completed_reports[0].player, PlayerId(1));
        assert!(group.poisoned());
    }

    #[test]
    fn native_pause_pipeline_keeps_pcm_judging_and_recorded_replay_on_one_playback_grid() {
        use crate::{
            playback_pause::{NativePause, PauseKeyboard},
            replay_capture::LiveReplayCapture,
        };
        use beatkernel::{
            audio::{
                AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample, SampleBank,
            },
            replay::{ReplaySession, codec::ReplayCodecLimits},
            time::ClockPair,
        };
        let output = |ns| ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(ns),
        };
        let pair = |ns| ClockPair {
            source: output(ns),
            target: point(ns + 100),
        };
        let member = config(1);
        let mut capture = LiveReplayCapture::new(
            &member.judge,
            ClockDomainId(1),
            ReplayCodecLimits::new(
                65536,
                128,
                4096,
                beatkernel::input::CodecLimits::new(4096, 4096).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let format = AudioFormat::new(1000, 1).unwrap();
        let limits = AudioLimits::new(8, 2, 8, 16, 8).unwrap();
        let pcm_limits = PcmLimits::new(4096, 4096, 2).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.5, 0.25], pcm_limits).unwrap(),
        )
        .unwrap();
        let (producer, consumer) = command_queue(8).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(format, ClockDomainId(2), Timestamp::ZERO, limits),
            bank,
            consumer,
        )
        .unwrap();
        let mut solo = SoloRuntime::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::from_nanos(100), Timestamp::ZERO, Rate::NORMAL),
            member.bindings,
            member.judge,
            producer,
            member.sounds,
            8,
        )
        .unwrap();
        let mut pause = NativePause::new(output(0), ClockDomainId(1), 1000).unwrap();
        let mut keyboard = PauseKeyboard::new();
        let initial = mixer.render(&mut [0.0]).unwrap();
        assert!(pause.observe(Some(initial), pair(0)).unwrap().is_none());
        let early = input(1, 500_100);
        assert!(keyboard.accept(&early).unwrap());
        let report = solo
            .process_input(early, &Identity, pause.scheduling_point(initial).unwrap())
            .unwrap();
        capture.record_report(&report).unwrap();
        assert!(report.judge_events.is_empty());
        assert!(pause.request(true, pair(0)).unwrap());
        solo.request_audio_pause(true);
        let mut silence = [1.0; 4];
        let paused = mixer.render(&mut silence).unwrap();
        assert_eq!(silence, [0.0; 4]);
        let boundary = pause
            .observe(Some(paused), pair(1_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(boundary.host, point(1_000_100));
        solo.transport_mut().pause(boundary.host.timestamp).unwrap();
        let report = solo
            .advance_to(
                boundary.host,
                &Identity,
                pause.scheduling_point(paused).unwrap(),
            )
            .unwrap();
        capture.record_report(&report).unwrap();
        assert_eq!(report.song_time, Timestamp::from_nanos(1_000_000));
        let before_idle = capture.records().len();
        let mut released = input(1, 2_000_100);
        if let PhysicalInputEvent::Button(event) = &mut released {
            event.state = ButtonState::Up;
        }
        keyboard.observe_paused(released).unwrap();
        assert_eq!(capture.records().len(), before_idle);
        assert!(pause.request(false, pair(4_000_000)).unwrap());
        solo.request_audio_pause(false);
        let resumed = mixer.render(&mut [0.0]).unwrap();
        let boundary = pause
            .observe(Some(resumed), pair(5_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(boundary.host, point(5_000_100));
        assert_eq!(
            pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
            Timestamp::from_nanos(-4_000_000)
        );
        solo.transport_mut()
            .resume(boundary.host.timestamp)
            .unwrap();
        let releases = keyboard.resume(boundary.host).unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(
            releases[0].meta().original_clock_point,
            Some(point(2_000_100))
        );
        for event in releases {
            let report = solo
                .process_input(event, &Identity, pause.scheduling_point(resumed).unwrap())
                .unwrap();
            assert_eq!(report.song_time, Timestamp::from_nanos(1_000_000));
            capture.record_report(&report).unwrap();
        }
        let hit = input(1, 6_000_100);
        assert!(keyboard.accept(&hit).unwrap());
        let report = solo
            .process_input(hit, &Identity, pause.scheduling_point(resumed).unwrap())
            .unwrap();
        assert_eq!(report.song_time, Timestamp::from_nanos(2_000_000));
        assert_eq!(report.judge_events.len(), 1);
        assert!(report.audio_failures.is_empty());
        capture.record_report(&report).unwrap();
        let mut audible = [0.0; 2];
        let final_render = mixer.render(&mut audible).unwrap();
        assert_eq!(audible, [0.5, 0.25]);
        assert_eq!(final_render.playback_start_frame, 2);
        assert_eq!(final_render.start_frame, 6);
        let file = capture.into_file();
        let replay =
            ReplaySession::from_records(file.header, config(1).judge, file.records).unwrap();
        assert_eq!(
            replay.engine().stable_hash().unwrap(),
            solo.judge().stable_hash().unwrap()
        );
        assert_eq!(replay.results(), report.judge_events);
    }

    #[test]
    fn local_cohorts_share_pause_pcm_and_time_but_keep_sparse_player_replays_independent() {
        use crate::{
            local_input::InputMerger,
            playback_pause::{NativePause, PauseKeyboard},
            replay_capture::LiveReplayCapture,
        };
        use beatkernel::{
            audio::{
                AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample, SampleBank,
            },
            replay::{ReplaySession, codec::ReplayCodecLimits},
            time::ClockPair,
        };
        for count in [2, 3, 4, 64] {
            let ids = (0..count)
                .map(|index| {
                    if index == count - 1 {
                        u32::MAX
                    } else if index == 0 {
                        7
                    } else {
                        999 + index as u32
                    }
                })
                .collect::<Vec<_>>();
            let mut configs = ids.iter().copied().map(config).collect::<Vec<_>>();
            let limits = ReplayCodecLimits::new(
                65536,
                128,
                4096,
                beatkernel::input::CodecLimits::new(4096, 4096).unwrap(),
            )
            .unwrap();
            let mut captures = configs
                .iter()
                .map(|member| {
                    (
                        member.player,
                        LiveReplayCapture::new(&member.judge, ClockDomainId(1), limits).unwrap(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            for member in &mut configs {
                member.sounds[0].gain = 1.0 / count as f32;
            }
            let output = |ns| ClockPoint {
                domain: ClockDomainId(2),
                timestamp: Timestamp::from_nanos(ns),
            };
            let pair = |ns| ClockPair {
                source: output(ns),
                target: point(ns + 100),
            };
            let format = AudioFormat::new(1000, 1).unwrap();
            let audio_limits = AudioLimits::new(128, 64, 128, 16, 128).unwrap();
            let pcm_limits = PcmLimits::new(4096, 4096, 2).unwrap();
            let mut bank = SampleBank::new(format, pcm_limits).unwrap();
            bank.insert(
                SampleId(1),
                PcmSample::new(format, vec![0.5, 0.25], pcm_limits).unwrap(),
            )
            .unwrap();
            let (producer, consumer) = command_queue(128).unwrap();
            let mut mixer = Mixer::new(
                MixerConfig::new(format, ClockDomainId(2), Timestamp::ZERO, audio_limits),
                bank,
                consumer,
            )
            .unwrap();
            let mut group = RuntimeGroup::new(
                ClockDomainId(1),
                ClockDomainId(2),
                Transport::new(Timestamp::from_nanos(100), Timestamp::ZERO, Rate::NORMAL),
                producer,
                configs,
                8,
                &[],
            )
            .unwrap();
            let mut merger = InputMerger::new(
                ClockDomainId(1),
                point(100),
                ids.iter().map(|id| DeviceId(u64::from(*id))).collect(),
                256,
            )
            .unwrap();
            let mut pause = NativePause::new(output(0), ClockDomainId(1), 1000).unwrap();
            let mut keys = PauseKeyboard::new();
            let initial = mixer.render(&mut [0.0]).unwrap();
            pause.observe(Some(initial), pair(0)).unwrap();
            // Acquisition order differs from deterministic merged device order.
            for id in ids.iter().rev() {
                merger
                    .admit(input(u64::from(*id), 500_100), point(600_100))
                    .unwrap();
            }
            while let Some(event) = merger.pop_ready(point(600_100)).unwrap() {
                assert!(keys.accept(&event).unwrap());
                let InputResult::Processed(reports) = group
                    .process_input(event, &Identity, pause.scheduling_point(initial).unwrap())
                    .unwrap()
                else {
                    panic!("assigned source ignored");
                };
                for tagged in reports {
                    captures
                        .get_mut(&tagged.player)
                        .unwrap()
                        .record_report(&tagged.report)
                        .unwrap();
                }
            }
            assert!(pause.request(true, pair(0)).unwrap());
            group.request_audio_pause(true);
            let mut silent = [1.0; 4];
            let paused = mixer.render(&mut silent).unwrap();
            assert_eq!(silent, [0.0; 4]);
            let boundary = pause
                .observe(Some(paused), pair(1_000_000))
                .unwrap()
                .unwrap();
            group
                .transport_mut()
                .pause(boundary.host.timestamp)
                .unwrap();
            let reports = group
                .advance_to(
                    boundary.host,
                    &Identity,
                    pause.scheduling_point(paused).unwrap(),
                )
                .unwrap();
            assert_eq!(reports.len(), count);
            for tagged in reports {
                assert_eq!(tagged.report.song_time, Timestamp::from_nanos(1_000_000));
                captures
                    .get_mut(&tagged.player)
                    .unwrap()
                    .record_report(&tagged.report)
                    .unwrap();
            }
            merger.commit(boundary.host).unwrap();
            for id in ids.iter().rev() {
                let mut event = input(u64::from(*id), 2_000_100);
                if let PhysicalInputEvent::Button(button) = &mut event {
                    button.state = ButtonState::Up;
                    button.meta.sequence = 2;
                }
                merger.admit(event, point(3_000_100)).unwrap();
            }
            while let Some(event) = merger.pop_ready(point(3_000_100)).unwrap() {
                keys.observe_paused(event).unwrap();
            }
            let lengths = captures
                .values()
                .map(|capture| capture.records().len())
                .collect::<Vec<_>>();
            assert!(pause.request(false, pair(4_000_000)).unwrap());
            group.request_audio_pause(false);
            let resumed = mixer.render(&mut [0.0]).unwrap();
            let boundary = pause
                .observe(Some(resumed), pair(5_000_000))
                .unwrap()
                .unwrap();
            group
                .transport_mut()
                .resume(boundary.host.timestamp)
                .unwrap();
            let delayed = merger
                .watermark(point(5_500_100), 1_000_000, false)
                .unwrap()
                .unwrap();
            assert!(delayed.timestamp < boundary.host.timestamp);
            assert!(
                merger
                    .watermark(point(6_000_100), 0, true)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                captures
                    .values()
                    .map(|capture| capture.records().len())
                    .collect::<Vec<_>>(),
                lengths
            );
            let releases = keys.resume(boundary.host).unwrap();
            assert_eq!(releases.len(), count);
            for event in releases {
                assert_eq!(event.meta().original_clock_point, Some(point(2_000_100)));
                let InputResult::Processed(reports) = group
                    .process_input(event, &Identity, pause.scheduling_point(resumed).unwrap())
                    .unwrap()
                else {
                    panic!("release source ignored");
                };
                for tagged in reports {
                    assert_eq!(tagged.report.song_time, Timestamp::from_nanos(1_000_000));
                    captures
                        .get_mut(&tagged.player)
                        .unwrap()
                        .record_report(&tagged.report)
                        .unwrap();
                }
            }
            for id in ids.iter().rev() {
                let mut event = input(u64::from(*id), 6_000_100);
                event.meta_mut().sequence = 3;
                merger.admit(event, point(6_000_100)).unwrap();
            }
            while let Some(event) = merger.pop_ready(point(6_000_100)).unwrap() {
                assert!(keys.accept(&event).unwrap());
                let InputResult::Processed(reports) = group
                    .process_input(event, &Identity, pause.scheduling_point(resumed).unwrap())
                    .unwrap()
                else {
                    panic!("hit source ignored");
                };
                assert_eq!(reports.len(), 1);
                let tagged = &reports[0];
                assert_eq!(tagged.report.song_time, Timestamp::from_nanos(2_000_000));
                assert_eq!(tagged.report.judge_events.len(), 1);
                assert!(tagged.report.audio_failures.is_empty());
                captures
                    .get_mut(&tagged.player)
                    .unwrap()
                    .record_report(&tagged.report)
                    .unwrap();
            }
            let mut audible = [0.0; 2];
            let rendered = mixer.render(&mut audible).unwrap();
            assert!((audible[0] - 0.5).abs() < 0.00001);
            assert!((audible[1] - 0.25).abs() < 0.00001);
            assert_eq!(
                (rendered.start_frame, rendered.playback_start_frame),
                (6, 2)
            );
            for (player, capture) in captures {
                let file = capture.into_file();
                let replay =
                    ReplaySession::from_records(file.header, config(player.0).judge, file.records)
                        .unwrap();
                assert_eq!(replay.results().len(), 1);
                assert_eq!(
                    replay.engine().stable_hash().unwrap(),
                    group.member_judge(player).unwrap().stable_hash().unwrap()
                );
            }
        }
    }

    #[test]
    fn solo_returns_committed_partial_report_before_fencing_next_operation() {
        let member = config(1);
        let (producer, _consumer) = command_queue(1).unwrap();
        let mut solo = SoloRuntime::new(
            ClockDomainId(1),
            ClockDomainId(1),
            Transport::new(Timestamp::from_nanos(100), Timestamp::ZERO, Rate::NORMAL),
            member.bindings,
            member.judge,
            producer,
            member.sounds,
            8,
        )
        .unwrap();
        solo.enqueue_audio(AudioCommand::Stop {
            voice: VoiceId(99),
            at: Timestamp::ZERO,
        })
        .unwrap();
        let report = solo
            .process_input(input(1, 2_000_100), &Identity, point(2_000_100))
            .unwrap();
        assert_eq!(report.judge_events.len(), 1);
        assert_eq!(report.audio_failures.len(), 1);
        assert!(matches!(
            solo.advance_to(point(2_000_101), &Identity, point(2_000_101))
                .unwrap_err()
                .kind,
            FailureKind::Poisoned
        ));
        assert_eq!(solo.telemetry().counters().audio_commands, 1);
        assert_eq!(solo.telemetry().counters().queue_full, 1);
    }

    #[test]
    fn unwinding_restores_authoritative_transport_and_actual_producer() {
        struct PanicMapper;
        impl ClockMapper for PanicMapper {
            fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
                panic!("fixture mapping failure")
            }
            fn quality(&self) -> ClockMappingQuality {
                ClockMappingQuality::Exact
            }
        }
        let (mut group, mut consumer) = group(8);
        let mut event = input(1, 100);
        event.meta_mut().clock_domain = ClockDomainId(99);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                group.process_input(event, &PanicMapper, point(100))
            }))
            .is_err()
        );
        assert!(group.poisoned());
        assert_eq!(
            group.transport().anchor().host_time,
            Timestamp::from_nanos(100)
        );
        let command = AudioCommand::Stop {
            voice: VoiceId(1),
            at: Timestamp::ZERO,
        };
        group.enqueue_audio(command).unwrap();
        assert_eq!(consumer.try_pop().unwrap(), command);
    }

    fn boundary_config(player: u32) -> MemberConfig {
        let mut member = config(player);
        let mut source = SourceChart::new(1000, Bpm::new(60, 1).unwrap()).unwrap();
        for id in [1, 2] {
            source.objects.push(SourceObject {
                id: ObjectId(id),
                start: Beat::new(id as i64).unwrap(),
                end: None,
                interaction: InteractionId(1),
                visual: VisualId(1),
                audio: None,
                metadata: ObjectMetadata::default(),
            });
        }
        member.judge = JudgeEngine::new(
            source.compile().unwrap(),
            vec![Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(InstantEvaluator),
            }],
            member.judge.profile().clone(),
        )
        .unwrap();
        let mut second = member.sounds[0];
        second.object = ObjectId(2);
        member.sounds.push(second);
        member
    }

    #[test]
    fn finite_pcm_and_shared_judging_capture_only_the_original_song_prefix() {
        use crate::replay_capture::LiveReplayCapture;
        use beatkernel::{
            audio::{
                AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample, SampleBank,
            },
            replay::{ReplayOperation, ReplaySession, codec::ReplayCodecLimits},
        };
        for count in [1, 2, 3, 4, 64] {
            let ids = (0..count)
                .map(|index| {
                    if index == count - 1 {
                        u32::MAX
                    } else {
                        7 + index as u32 * 999
                    }
                })
                .collect::<Vec<_>>();
            let limits = ReplayCodecLimits::new(
                65536,
                128,
                4096,
                beatkernel::input::CodecLimits::new(4096, 4096).unwrap(),
            )
            .unwrap();
            let mut configs = ids.iter().copied().map(boundary_config).collect::<Vec<_>>();
            let mut captures = configs
                .iter()
                .map(|member| {
                    (
                        member.player,
                        LiveReplayCapture::new(&member.judge, ClockDomainId(1), limits).unwrap(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            for member in &mut configs {
                for sound in &mut member.sounds {
                    sound.gain = 1.0 / count as f32;
                }
            }
            let audio = |ns| ClockPoint {
                domain: ClockDomainId(2),
                timestamp: Timestamp::from_nanos(ns),
            };
            let format = AudioFormat::new(1000, 1).unwrap();
            let pcm_limits = PcmLimits::new(4096, 4096, 2).unwrap();
            let mut bank = SampleBank::new(format, pcm_limits).unwrap();
            bank.insert(
                SampleId(1),
                PcmSample::new(format, vec![0.5, 0.25], pcm_limits).unwrap(),
            )
            .unwrap();
            let (producer, consumer) = command_queue(128).unwrap();
            let mut mixer = Mixer::new(
                MixerConfig::new(
                    format,
                    ClockDomainId(2),
                    Timestamp::ZERO,
                    AudioLimits::new(128, 64, 128, 16, 128).unwrap(),
                )
                .with_playback_end_frame(2),
                bank,
                consumer,
            )
            .unwrap();
            let mut group = RuntimeGroup::new(
                ClockDomainId(1),
                ClockDomainId(2),
                Transport::new(Timestamp::from_nanos(100), Timestamp::ZERO, Rate::NORMAL),
                producer,
                configs,
                8,
                &[],
            )
            .unwrap();
            assert!(group.set_song_end(Timestamp::from_nanos(-1)).is_err());
            assert_eq!(group.song_end(), None);
            // Unknown input sources do not start or lock private member runtimes.
            assert!(matches!(
                group
                    .process_input(input(0, 100), &Identity, audio(0))
                    .unwrap(),
                InputResult::Ignored { .. }
            ));
            group
                .set_song_end(Timestamp::from_nanos(2_000_000))
                .unwrap();
            assert!(
                group
                    .set_song_end(Timestamp::from_nanos(3_000_000))
                    .is_err()
            );
            assert_eq!(group.song_end(), Some(Timestamp::from_nanos(2_000_000)));
            for id in &ids {
                let InputResult::Processed(reports) = group
                    .process_input(
                        input(u64::from(*id), 1_000_100),
                        &Identity,
                        audio(1_000_000),
                    )
                    .unwrap()
                else {
                    panic!("assigned pre-end event ignored");
                };
                let tagged = &reports[0];
                assert_eq!(tagged.player, PlayerId(*id));
                assert!(!tagged.report.song_end_reached);
                assert_eq!(tagged.report.judge_events.len(), 1);
                assert_eq!(tagged.report.audio_commands.len(), 1);
                captures
                    .get_mut(&tagged.player)
                    .unwrap()
                    .record_report(&tagged.report)
                    .unwrap();
            }
            for at in [2_000_100, 9_000_100] {
                for id in &ids {
                    let mut event = input(u64::from(*id), at);
                    event.meta_mut().sequence = if at == 2_000_100 { 2 } else { 3 };
                    let InputResult::Processed(reports) = group
                        .process_input(event, &Identity, audio(2_000_000))
                        .unwrap()
                    else {
                        panic!("assigned fenced acquisition ignored");
                    };
                    let tagged = &reports[0];
                    assert!(tagged.report.song_end_reached);
                    assert_eq!(tagged.report.song_time, Timestamp::from_nanos(2_000_000));
                    assert!(tagged.report.input.is_none());
                    assert!(tagged.report.bound_inputs.is_empty());
                    assert!(tagged.report.judge_events.is_empty());
                    assert!(tagged.report.audio_commands.is_empty());
                    assert!(tagged.report.judge_error.is_none());
                    captures
                        .get_mut(&tagged.player)
                        .unwrap()
                        .record_report(&tagged.report)
                        .unwrap();
                }
            }
            for tagged in group
                .advance_to(point(10_000_100), &Identity, audio(2_000_000))
                .unwrap()
            {
                assert!(tagged.report.song_end_reached);
                assert!(tagged.report.judge_events.is_empty());
                captures
                    .get_mut(&tagged.player)
                    .unwrap()
                    .record_report(&tagged.report)
                    .unwrap();
            }
            let mut samples = [1.0; 5];
            let rendered = mixer.render(&mut samples).unwrap();
            assert_eq!(
                (rendered.frames, rendered.playback_frames, rendered.paused),
                (5, 2, true)
            );
            assert_eq!(samples[0], 0.0);
            assert!((samples[1] - 0.5).abs() < 0.00001);
            assert_eq!(&samples[2..], &[0.0; 3]);
            group.request_audio_pause(false);
            let mut tail = [1.0; 3];
            let frozen = mixer.render(&mut tail).unwrap();
            assert_eq!(frozen.playback_frames, 0);
            assert_eq!(tail, [0.0; 3]);
            for (player, capture) in captures {
                let counters = group.member_telemetry(player).unwrap().counters();
                assert_eq!(
                    (
                        counters.inputs,
                        counters.unbound,
                        counters.judge_results,
                        counters.audio_commands
                    ),
                    (3, 0, 1, 1)
                );
                let file = capture.into_file();
                assert_eq!(file.records.len(), 4);
                assert!(file.records[1..].iter().all(|record| record.song_time
                    == Timestamp::from_nanos(2_000_000)
                    && matches!(record.operation, ReplayOperation::Advance)));
                let replay = ReplaySession::from_records(
                    file.header,
                    boundary_config(player.0).judge,
                    file.records,
                )
                .unwrap();
                assert_eq!(replay.results().len(), 1);
                // The note exactly at the exclusive end is neither hit nor fabricated as a miss.
                assert_eq!(
                    replay.engine().stable_hash().unwrap(),
                    group.member_judge(player).unwrap().stable_hash().unwrap()
                );
            }
        }
    }

    #[test]
    fn shared_finite_resume_drains_all_sources_and_reconstructs_independent_prefixes() {
        use crate::{
            local_input::InputMerger,
            native_end::NativeEnd,
            playback_pause::{NativePause, PauseKeyboard},
            replay_capture::LiveReplayCapture,
        };
        use beatkernel::{
            audio::{
                AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample, SampleBank,
            },
            replay::{ReplaySession, codec::ReplayCodecLimits},
            time::ClockPair,
        };
        for count in [2, 3, 4, 64] {
            let ids = (0..count)
                .map(|index| {
                    if index == count - 1 {
                        u32::MAX
                    } else {
                        7 + index as u32 * 999
                    }
                })
                .collect::<Vec<_>>();
            let mut configs = ids.iter().copied().map(boundary_config).collect::<Vec<_>>();
            for (index, member) in configs.iter_mut().enumerate() {
                let device = DeviceId(index as u64 + 1);
                member.device = Some(device);
                member.bindings = BindingMap::from_bindings([Binding {
                    device: DeviceSelector::Exact(device),
                    physical: PhysicalControlId::keyboard(4),
                    game_control: GameControlId(1),
                }])
                .unwrap();
                for sound in &mut member.sounds {
                    sound.gain = 1.0 / count as f32;
                }
            }
            let limits = ReplayCodecLimits::new(
                65536,
                128,
                4096,
                beatkernel::input::CodecLimits::new(4096, 4096).unwrap(),
            )
            .unwrap();
            let mut captures = configs
                .iter()
                .map(|member| {
                    (
                        member.player,
                        LiveReplayCapture::new(&member.judge, ClockDomainId(1), limits).unwrap(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let output = |ns| ClockPoint {
                domain: ClockDomainId(2),
                timestamp: Timestamp::from_nanos(ns),
            };
            let pair = |ns| ClockPair {
                source: output(ns),
                target: point(ns + 100),
            };
            let format = AudioFormat::new(1000, 1).unwrap();
            let pcm_limits = PcmLimits::new(4096, 4096, 2).unwrap();
            let mut bank = SampleBank::new(format, pcm_limits).unwrap();
            bank.insert(
                SampleId(1),
                PcmSample::new(format, vec![0.5; 8], pcm_limits).unwrap(),
            )
            .unwrap();
            let (producer, consumer) = command_queue(128).unwrap();
            let mut mixer = Mixer::new(
                MixerConfig::new(
                    format,
                    ClockDomainId(2),
                    Timestamp::ZERO,
                    AudioLimits::new(128, 64, 128, 16, 128).unwrap(),
                )
                .with_playback_end_frame(2),
                bank,
                consumer,
            )
            .unwrap();
            let mut group = RuntimeGroup::new(
                ClockDomainId(1),
                ClockDomainId(2),
                Transport::new(Timestamp::from_nanos(100), Timestamp::ZERO, Rate::NORMAL),
                producer,
                configs,
                8,
                &[],
            )
            .unwrap();
            group
                .set_song_end(Timestamp::from_nanos(2_000_000))
                .unwrap();
            let mut merger = InputMerger::new(
                ClockDomainId(1),
                point(100),
                (1..=count).map(|index| DeviceId(index as u64)).collect(),
                256,
            )
            .unwrap();
            let mut pause = NativePause::new(output(0), ClockDomainId(1), 1000)
                .unwrap()
                .with_playback_end_frame(2)
                .unwrap();
            let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 2).unwrap();
            let mut keys = PauseKeyboard::new();
            let first = mixer.render(&mut [0.0]).unwrap();
            pause.observe(Some(first), pair(0)).unwrap();
            end.observe(Some(first), pair(0)).unwrap();
            for index in (1..=count).rev() {
                merger
                    .admit(input(index as u64, 1_000_100), point(1_100_100))
                    .unwrap();
            }
            while let Some(event) = merger.pop_ready(point(1_000_100)).unwrap() {
                let source = event.meta().source.0;
                assert!(keys.accept(&event).unwrap());
                let InputResult::Processed(reports) = group
                    .process_input(event, &Identity, output(1_000_000))
                    .unwrap()
                else {
                    panic!("assigned source ignored");
                };
                assert_eq!(reports[0].player, PlayerId(ids[source as usize - 1]));
                assert_eq!(reports[0].report.judge_events.len(), 1);
                for tagged in reports {
                    captures
                        .get_mut(&tagged.player)
                        .unwrap()
                        .record_report(&tagged.report)
                        .unwrap();
                }
            }
            for tagged in group
                .advance_to(point(1_000_100), &Identity, output(1_000_000))
                .unwrap()
            {
                captures
                    .get_mut(&tagged.player)
                    .unwrap()
                    .record_report(&tagged.report)
                    .unwrap();
            }
            merger.commit(point(1_000_100)).unwrap();
            pause.request(true, pair(0)).unwrap();
            group.request_audio_pause(true);
            let paused = mixer.render(&mut [0.0; 3]).unwrap();
            let boundary = pause
                .observe(Some(paused), pair(1_000_000))
                .unwrap()
                .unwrap();
            group
                .transport_mut()
                .pause(boundary.host.timestamp)
                .unwrap();
            assert!(
                end.observe(Some(paused), pair(1_000_000))
                    .unwrap()
                    .is_none()
            );
            for index in (1..=count).rev() {
                let mut up = input(index as u64, 2_000_100);
                if let PhysicalInputEvent::Button(button) = &mut up {
                    button.state = ButtonState::Up;
                    button.meta.sequence = 2;
                }
                merger.admit(up, point(3_000_100)).unwrap();
            }
            while let Some(event) = merger.pop_ready(point(3_000_100)).unwrap() {
                keys.observe_paused(event).unwrap();
            }
            pause.request(false, pair(3_000_000)).unwrap();
            group.request_audio_pause(false);
            let mut samples = [1.0; 4];
            let crossing = mixer.render(&mut samples).unwrap();
            assert!((samples[0] - 0.5).abs() < 0.00001);
            assert_eq!(&samples[1..], &[0.0; 3]);
            assert_eq!(crossing.playback_end_physical_frame, Some(5));
            let latest = mixer.render(&mut [0.0]).unwrap();
            let resumed = pause
                .observe(Some(latest), pair(4_000_000))
                .unwrap()
                .unwrap();
            group
                .transport_mut()
                .resume(resumed.host.timestamp)
                .unwrap();
            let releases = keys.resume(resumed.host).unwrap();
            assert_eq!(releases.len(), count);
            for event in releases {
                assert_eq!(event.meta().original_clock_point, Some(point(2_000_100)));
                let InputResult::Processed(reports) = group
                    .process_input(event, &Identity, output(2_000_000))
                    .unwrap()
                else {
                    panic!("resume source ignored");
                };
                for tagged in reports {
                    captures
                        .get_mut(&tagged.player)
                        .unwrap()
                        .record_report(&tagged.report)
                        .unwrap();
                }
            }
            assert!(
                end.observe(Some(latest), pair(4_000_000))
                    .unwrap()
                    .is_none()
            );
            let terminal = end.observe(None, pair(6_000_000)).unwrap().unwrap();
            assert_eq!(terminal.host, point(5_000_100));
            let mut earlier = input(1, 4_500_100);
            earlier.meta_mut().sequence = 3;
            merger.admit(earlier, point(6_000_100)).unwrap();
            for index in (1..=count).rev() {
                let mut at_end = input(index as u64, 5_000_100);
                at_end.meta_mut().sequence = if index == 1 { 4 } else { 3 };
                merger.admit(at_end, point(6_000_100)).unwrap();
            }
            let pending = merger.pending();
            assert!(
                merger
                    .watermark(point(6_000_100), 0, true)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(merger.pending(), pending); // One backlogged source prevents any release/deadline.
            let frontier = merger
                .watermark(point(6_000_100), 0, false)
                .unwrap()
                .unwrap();
            let mut excluded = 0;
            while let Some(event) = merger.pop_ready(frontier).unwrap() {
                if event.meta().timestamp >= terminal.host.timestamp {
                    excluded += 1;
                    continue;
                }
                assert_eq!(event.meta().timestamp, Timestamp::from_nanos(4_500_100));
                assert!(keys.accept(&event).unwrap());
                let InputResult::Processed(reports) = group
                    .process_input(event, &Identity, output(2_000_000))
                    .unwrap()
                else {
                    panic!("pre-boundary source ignored");
                };
                assert_eq!(
                    reports[0].report.song_time,
                    Timestamp::from_nanos(1_500_000)
                );
                assert!(reports[0].report.judge_events.is_empty());
                for tagged in reports {
                    captures
                        .get_mut(&tagged.player)
                        .unwrap()
                        .record_report(&tagged.report)
                        .unwrap();
                }
            }
            assert_eq!(excluded, count);
            for tagged in group
                .advance_to(frontier, &Identity, output(2_000_000))
                .unwrap()
            {
                assert!(tagged.report.song_end_reached);
                assert_eq!(tagged.report.song_time, Timestamp::from_nanos(2_000_000));
                assert!(tagged.report.judge_events.is_empty());
                captures
                    .get_mut(&tagged.player)
                    .unwrap()
                    .record_report(&tagged.report)
                    .unwrap();
            }
            merger.commit(frontier).unwrap();
            assert!(frontier.timestamp >= terminal.host.timestamp);
            assert_eq!(merger.pending(), 0);
            for (player, capture) in captures {
                let file = capture.into_file();
                assert!(
                    file.records
                        .iter()
                        .all(|record| record.song_time <= Timestamp::from_nanos(2_000_000))
                );
                let replay = ReplaySession::from_records(
                    file.header,
                    boundary_config(player.0).judge,
                    file.records,
                )
                .unwrap();
                assert_eq!(replay.results().len(), 1);
                assert_eq!(
                    replay.engine().stable_hash().unwrap(),
                    group.member_judge(player).unwrap().stable_hash().unwrap()
                );
                let counters = group.member_telemetry(player).unwrap().counters();
                assert_eq!(counters.audio_commands, 1);
                assert_eq!(
                    counters.inputs,
                    if player == PlayerId(ids[0]) { 3 } else { 2 }
                );
            }
        }
    }

    #[test]
    fn solo_adapter_uses_the_same_immutable_end_and_capture_operation() {
        let member = boundary_config(1);
        let (producer, mut consumer) = command_queue(8).unwrap();
        let mut solo = SoloRuntime::new(
            ClockDomainId(1),
            ClockDomainId(1),
            Transport::new(Timestamp::from_nanos(100), Timestamp::ZERO, Rate::NORMAL),
            member.bindings,
            member.judge,
            producer,
            member.sounds,
            8,
        )
        .unwrap();
        assert!(solo.set_song_end(Timestamp::from_nanos(-1)).is_err());
        solo.set_song_end(Timestamp::from_nanos(2_000_000)).unwrap();
        let pre = solo
            .process_input(input(1, 1_000_100), &Identity, point(1_000_100))
            .unwrap();
        assert!(pre.input.is_some());
        assert_eq!(pre.judge_events.len(), 1);
        assert!(consumer.try_pop().is_ok());
        let mut end = input(1, 2_000_100);
        end.meta_mut().sequence = 2;
        let capped = solo
            .process_input(end, &Identity, point(2_000_100))
            .unwrap();
        assert!(capped.song_end_reached && capped.input.is_none());
        assert!(capped.judge_events.is_empty() && capped.audio_commands.is_empty());
        assert_eq!(
            consumer.try_pop(),
            Err(beatkernel::audio::QueuePopError::Empty)
        );
        assert_eq!(solo.song_end(), Some(Timestamp::from_nanos(2_000_000)));
        assert!(solo.set_song_end(Timestamp::from_nanos(3_000_000)).is_err());
    }

    #[test]
    fn remap_keeps_replacement_aliases_and_overflow_is_transactional() {
        let mut first = config(1).sounds;
        first.push(first[0]);
        let mut second = config(2).sounds;
        let mut allocator = VoiceAllocator::new(100);
        allocator.remap(&mut first).unwrap();
        allocator.remap(&mut second).unwrap();
        assert_eq!(first[0].voice, first[1].voice);
        assert_ne!(first[0].voice, second[0].voice);
        let mut overflow = vec![first[0], second[0]];
        let before = overflow.clone();
        let mut allocator = VoiceAllocator::new(u64::MAX);
        assert!(allocator.remap(&mut overflow).is_err());
        assert_eq!(overflow, before);
        allocator.remap(&mut overflow[..1]).unwrap();
        assert_eq!(overflow[0].voice, VoiceId(u64::MAX));
    }
}
