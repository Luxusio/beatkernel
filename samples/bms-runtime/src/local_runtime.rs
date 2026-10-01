//! Local cohorts execute the existing Runtime with one transport and audio owner.
use crate::local_players::{MAX_LOCAL_PLAYERS, PlayerId};
use beatkernel::{
    audio::{AudioCommand, CommandProducer, CommandPushError, VoiceId, command_queue},
    input::{BindingMap, DeviceId, DeviceSelector, PhysicalInputEvent},
    judge::JudgeEngine,
    runtime::{Runtime, RuntimeError, RuntimeReport, SoundBinding},
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
        if !(1..=MAX_LOCAL_PLAYERS).contains(&configs.len()) {
            return Err("local runtime requires 1..64 members".into());
        }
        if telemetry_capacity > 65_536
            || telemetry_capacity
                .checked_mul(configs.len())
                .is_none_or(|total| total > 1_048_576)
        {
            return Err("local telemetry exceeds per-member/aggregate sample capacity".into());
        }
        let multiple = configs.len() > 1;
        let mut players = HashSet::new();
        let mut devices = HashSet::new();
        let reserved: HashSet<_> = reserved_bgm_voices.iter().copied().collect();
        let mut voices = HashMap::new();
        for config in &configs {
            if !players.insert(config.player) {
                return Err("duplicate local player identity".into());
            }
            if multiple && config.device.is_none() {
                return Err("multiple local players require exact devices".into());
            }
            if let Some(device) = config.device {
                if !devices.insert(device) {
                    return Err("local input device is assigned more than once".into());
                }
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
        })
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
        let player = self.members[index].player;
        let result = {
            let guard = OwnerGuard::new(
                &mut self.members[index].runtime,
                &mut self.transport,
                &mut self.producer,
            );
            guard.runtime.process_input(input, mapper, audio_at)
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
        match self.0.process_input(input, mapper, audio_at) {
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
