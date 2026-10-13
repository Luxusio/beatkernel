//! Mathematical phase DI fixtures; these establish no native presentation authority.
use super::*;
use beatkernel::{
    audio::{CommandConsumer, QueuePopError, SampleId, SourcePosition},
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, TimedObject,
        VisualId,
    },
    input::{
        Binding, ButtonEvent, ButtonState, ContactId, EventMeta, GameControlId, PhysicalControlId,
        TouchEvent, TouchPhase,
    },
    interaction::{
        ActiveInteraction, BeginContext, InteractionContext, InteractionEvaluator,
        InteractionOutput, InteractionState, PressInstantEvaluator,
    },
    judge::{JudgeError, JudgeGrade, JudgeProfile, JudgeWindow, Rule},
    time::{ClockMappingQuality, Duration},
    transport::{OutputPhaseBasis, OutputPhaseError, OutputPhasePoint},
};
use std::cell::Cell;

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
fn basis() -> OutputPhaseBasis {
    OutputPhaseBasis::new(ClockDomainId(1), 17, 3, 2, 3).unwrap()
}
fn phase(b: OutputPhaseBasis, logical: i64, frame: u64, n: u64, d: u64) -> OutputPhasePoint {
    b.point(
        point(1, logical),
        SourcePosition {
            frame,
            numerator: n,
            denominator: d,
        },
    )
    .unwrap()
}
fn transport(b: OutputPhaseBasis) -> Transport {
    Transport::new_with_output_phase(
        b,
        point(1, 1000),
        ts(0),
        Duration::ZERO,
        Rate::new(3, 2).unwrap(),
        None,
    )
    .unwrap()
}
struct Mapper;
impl ClockMapper for Mapper {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == ClockDomainId(9) && to == ClockDomainId(1))
            .then(|| from.timestamp.checked_add(Duration::from_nanos(1000)))
            .flatten()
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn input(player: u32, host: i64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(u64::from(player)), point(9, host), 1),
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    })
}
fn member_with(player: u32, evaluator: Box<dyn InteractionEvaluator>) -> MemberConfig {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(249_999_999).unwrap(),
        end: None,
        interaction: InteractionId(1),
        visual: VisualId(0),
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
                evaluator,
            }],
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(7),
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
            stage: beatkernel::judge::JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(u64::from(player)),
            gain: 0.5,
        }],
    }
}
fn member(player: u32) -> MemberConfig {
    member_with(player, Box::new(PressInstantEvaluator))
}
fn group(ids: &[u32], capacity: usize) -> (RuntimeGroup, CommandConsumer) {
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(basis()),
        producer,
        ids.iter().copied().map(member).collect(),
        8,
        &[],
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    (group, consumer)
}
fn processed(result: InputResult) -> Vec<PlayerReport> {
    match result {
        InputResult::Processed(reports) => reports,
        _ => panic!("assigned input ignored"),
    }
}
fn assert_pristine(group: &RuntimeGroup, before: &Transport, hashes: &[u64], admitted: u64) {
    assert!(!group.started);
    assert!(!group.poisoned);
    assert_eq!(group.transport(), before);
    assert_eq!(group.admitted_audio_commands(), admitted);
    for (member, hash) in group.members.iter().zip(hashes) {
        assert_eq!(member.runtime.judge().stable_hash().unwrap(), *hash);
        assert_eq!(member.runtime.telemetry().counters().inputs, 0);
        assert_eq!(member.runtime.telemetry().counters().audio_commands, 0);
    }
}

#[test]
fn sparse_two_and_four_players_share_fractional_phase_and_real_producer() {
    for ids in [&[7, 29][..], &[7, 29, 101, 409][..]] {
        let (mut group, mut queue) = group(ids, 16);
        let q = phase(basis(), 1100, 2, 1, 2);
        assert_eq!(q.elapsed_nanos(), 166_666_666);
        // Scalar chronology would select 150ns, not this literal original song point.
        for &id in ids {
            let original = input(id, 100);
            let reports = processed(
                group
                    .process_input_output_phase(original.clone(), &Mapper, point(2, 777), q)
                    .unwrap(),
            );
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].player, PlayerId(id));
            let report = &reports[0].report;
            assert_eq!(report.song_time, ts(249_999_999));
            assert_eq!(report.audio_at, point(2, 777));
            assert_eq!(report.judge_events.len(), 1);
            assert_eq!(report.judge_events[0].at, ts(249_999_999));
            assert_eq!(
                report.input.as_ref().unwrap().meta().original_clock_point,
                Some(point(9, 100))
            );
            assert_eq!(
                queue.try_pop().unwrap(),
                AudioCommand::Play {
                    voice: VoiceId(u64::from(id)),
                    sample: SampleId(1),
                    at: ts(777),
                    gain: 0.5,
                }
            );
        }
        let reports = group
            .advance_to_output_phase(
                point(9, 101),
                &Mapper,
                point(2, 888),
                phase(basis(), 1101, 2, 1, 2),
            )
            .unwrap();
        assert_eq!(reports.iter().map(|r| r.player.0).collect::<Vec<_>>(), ids);
        assert!(reports
            .iter()
            .all(|r| r.report.song_time == ts(249_999_999)));
        assert_eq!(group.admitted_audio_commands(), ids.len() as u64);
        assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
        assert!(!group.poisoned());
    }
}

#[test]
fn unknown_device_is_ignored_before_phase_or_mapper_validation() {
    let (mut group, mut queue) = group(&[7, 29], 8);
    let before = group.transport().clone();
    let hashes: Vec<_> = group
        .members
        .iter()
        .map(|m| m.runtime.judge().stable_hash().unwrap())
        .collect();
    let q = phase(
        OutputPhaseBasis::new(ClockDomainId(1), 999, 999, 2, 3).unwrap(),
        9999,
        3,
        0,
        1,
    );
    assert!(matches!(
        group
            .process_input_output_phase(input(999, 100), &Mapper, point(99, 0), q)
            .unwrap(),
        InputResult::Ignored {
            device: DeviceId(999)
        }
    ));
    assert_pristine(&group, &before, &hashes, 0);
    assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
}

#[test]
fn common_phase_refusals_preserve_every_owner_flag_engine_and_queue() {
    for case in 0..6 {
        let (mut group, mut queue) = group(&[7, 29], 8);
        if case == 5 {
            group.transport = Transport::new(ts(1000), ts(0), Rate::NORMAL);
        }
        let before = group.transport().clone();
        let hashes: Vec<_> = group
            .members
            .iter()
            .map(|m| m.runtime.judge().stable_hash().unwrap())
            .collect();
        let b = match case {
            0 => OutputPhaseBasis::new(ClockDomainId(1), 18, 3, 2, 3).unwrap(),
            1 => OutputPhaseBasis::new(ClockDomainId(1), 17, 4, 2, 3).unwrap(),
            2 => OutputPhaseBasis::new(ClockDomainId(1), 17, 3, 2, 4).unwrap(),
            3 => OutputPhaseBasis::new(ClockDomainId(1), 17, 3, 1, 3).unwrap(),
            _ => basis(),
        };
        let q = phase(b, if case == 4 { 1101 } else { 1100 }, 2, 1, 2);
        let expected = match case {
            4 => OutputPhaseError::LogicalPointMismatch,
            5 => OutputPhaseError::MissingPhase,
            _ => OutputPhaseError::WrongBasis,
        };
        let failure = group
            .process_input_output_phase(input(29, 100), &Mapper, point(2, 777), q)
            .unwrap_err();
        assert_eq!(failure.failed_player, Some(PlayerId(29)));
        assert!(
            matches!(failure.kind, FailureKind::Core(RuntimeError::OutputPhase(e)) if e == expected)
        );
        assert!(failure.completed_reports.is_empty());
        assert_pristine(&group, &before, &hashes, 0);
        let failure = group
            .advance_to_output_phase(point(9, 100), &Mapper, point(2, 777), q)
            .unwrap_err();
        assert_eq!(failure.failed_player, None);
        assert!(
            matches!(failure.kind, FailureKind::Core(RuntimeError::OutputPhase(e)) if e == expected)
        );
        assert!(failure.completed_reports.is_empty());
        assert_pristine(&group, &before, &hashes, 0);
        assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
        // Refused common preflight leaves configuration unlocked.
        group.set_song_end(ts(1_000_000_000)).unwrap();
    }
}

#[test]
fn positioned_touch_projects_for_one_assigned_member_without_rewriting_payload() {
    let (mut group, mut queue) = group(&[7, 29], 8);
    group
        .configure_touch_router(
            PlayerId(29),
            TouchRouter::new(
                vec![TouchRegion {
                    device: DeviceSelector::Exact(DeviceId(29)),
                    physical: PhysicalControlId::keyboard(4),
                    game_control: GameControlId(1),
                    min: Position2 { x: 0.0, y: 0.0 },
                    max: Position2 { x: 10.0, y: 10.0 },
                }],
                2,
            )
            .unwrap(),
        )
        .unwrap();
    let original_position = Position2 {
        x: 500.0,
        y: -500.0,
    };
    let touch = PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(29), point(9, 100), 1),
        control: PhysicalControlId::keyboard(4),
        contact: ContactId(3),
        phase: TouchPhase::Down,
        position: original_position,
        pressure: Some(0.75),
    });
    let reports = processed(
        group
            .process_input_at_output_phase(
                touch,
                Position2 { x: 2.0, y: 2.0 },
                &Mapper,
                point(2, 991),
                phase(basis(), 1100, 2, 1, 2),
            )
            .unwrap(),
    );
    assert_eq!(reports[0].player, PlayerId(29));
    assert_eq!(reports[0].report.judge_events.len(), 1);
    let PhysicalInputEvent::Touch(retained) = reports[0].report.input.as_ref().unwrap() else {
        panic!("touch lost")
    };
    assert_eq!(retained.position, original_position);
    assert_eq!(retained.pressure, Some(0.75));
    assert_eq!(
        group
            .member_telemetry(PlayerId(7))
            .unwrap()
            .counters()
            .inputs,
        0
    );
    assert!(
        matches!(queue.try_pop().unwrap(), AudioCommand::Play { voice: VoiceId(29), at, .. } if at == ts(991))
    );
}

#[test]
fn actual_second_member_chronology_failure_keeps_completed_prefix_and_poison() {
    let (mut group, mut queue) = group(&[7, 29, 101, 409], 8);
    group
        .process_input_output_phase(
            input(29, 200),
            &Mapper,
            point(2, 777),
            phase(basis(), 1200, 2, 1, 2),
        )
        .unwrap();
    assert!(queue.try_pop().is_ok());
    let failure = group
        .advance_to_output_phase(
            point(9, 100),
            &Mapper,
            point(2, 888),
            phase(basis(), 1100, 2, 1, 2),
        )
        .unwrap_err();
    assert_eq!(failure.failed_player, Some(PlayerId(29)));
    assert!(matches!(failure.kind, FailureKind::Core(_)));
    assert_eq!(failure.completed_reports.len(), 1);
    assert_eq!(failure.completed_reports[0].player, PlayerId(7));
    assert_eq!(
        failure.completed_reports[0].report.song_time,
        ts(249_999_999)
    );
    assert!(group.poisoned());
    assert!(matches!(
        group
            .advance_to_output_phase(
                point(9, 300),
                &Mapper,
                point(2, 999),
                phase(basis(), 1300, 3, 0, 1)
            )
            .unwrap_err()
            .kind,
        FailureKind::Poisoned
    ));
    let cleanup = AudioCommand::Stop {
        voice: VoiceId(29),
        at: ts(999),
    };
    group.enqueue_audio(cleanup).unwrap();
    assert_eq!(queue.try_pop().unwrap(), cleanup);
}

#[test]
fn actual_queue_failure_preserves_committed_phase_report_and_solo_delegation() {
    let config = member(1);
    let (producer, mut queue) = command_queue(1).unwrap();
    let mut solo = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(basis()),
        config.bindings,
        config.judge,
        producer,
        config.sounds,
        8,
    )
    .unwrap();
    let sentinel = AudioCommand::Stop {
        voice: VoiceId(999),
        at: ts(0),
    };
    solo.enqueue_audio(sentinel).unwrap();
    let report = solo
        .process_input_output_phase(
            input(1, 100),
            &Mapper,
            point(2, 777),
            phase(basis(), 1100, 2, 1, 2),
        )
        .unwrap();
    assert_eq!(report.song_time, ts(249_999_999));
    assert_eq!(report.judge_events.len(), 1);
    assert_eq!(report.audio_failures.len(), 1);
    assert_eq!(report.audio_at, point(2, 777));
    assert!(solo.poisoned());
    assert_eq!(queue.try_pop().unwrap(), sentinel);
    assert!(matches!(
        solo.advance_to_output_phase(
            point(9, 101),
            &Mapper,
            point(2, 888),
            phase(basis(), 1101, 2, 1, 2)
        )
        .unwrap_err()
        .kind,
        FailureKind::Poisoned
    ));
    let cleanup = AudioCommand::Stop {
        voice: VoiceId(1),
        at: ts(888),
    };
    solo.enqueue_audio(cleanup).unwrap();
    assert_eq!(queue.try_pop().unwrap(), cleanup);
}

#[test]
fn actual_second_judge_refusal_reports_both_committed_members_and_stops_suffix() {
    let (mut group, mut queue) = group(&[7, 29, 101, 409], 8);
    // Explicit DI restriction: seed only the existing public judge chronology.
    // This is not a native clock or acquired-prefix simulation. The ensuing
    // group operation executes the real judge refusal and partial-report path.
    group.members[1]
        .runtime
        .judge_mut()
        .advance_to(ts(300_000_000))
        .unwrap();
    let failure = group
        .advance_to_output_phase(
            point(9, 100),
            &Mapper,
            point(2, 777),
            phase(basis(), 1100, 2, 1, 2),
        )
        .unwrap_err();
    assert_eq!(failure.failed_player, Some(PlayerId(29)));
    assert!(matches!(failure.kind, FailureKind::ReportedFailure));
    assert_eq!(
        failure
            .completed_reports
            .iter()
            .map(|r| r.player.0)
            .collect::<Vec<_>>(),
        [7, 29]
    );
    assert!(failure.completed_reports[0].report.judge_error.is_none());
    assert!(failure.completed_reports[1].report.judge_error.is_some());
    assert!(group
        .member_judge(PlayerId(101))
        .unwrap()
        .effective_song_time()
        .is_none());
    assert!(group
        .member_judge(PlayerId(409))
        .unwrap()
        .effective_song_time()
        .is_none());
    assert!(group.poisoned());
    assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
}

#[test]
fn solo_positioned_phase_and_shared_advance_use_the_same_original_song() {
    let config = member(1);
    let (producer, mut queue) = command_queue(8).unwrap();
    let mut solo = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(basis()),
        config.bindings,
        config.judge,
        producer,
        config.sounds,
        8,
    )
    .unwrap();
    solo.configure_touch_router(
        TouchRouter::new(
            vec![TouchRegion {
                device: DeviceSelector::Exact(DeviceId(1)),
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(1),
                min: Position2 { x: 0.0, y: 0.0 },
                max: Position2 { x: 10.0, y: 10.0 },
            }],
            2,
        )
        .unwrap(),
    )
    .unwrap();
    let touch = PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(1), point(9, 100), 1),
        control: PhysicalControlId::keyboard(4),
        contact: ContactId(3),
        phase: TouchPhase::Down,
        position: Position2 { x: 500.0, y: 500.0 },
        pressure: None,
    });
    let report = solo
        .process_input_at_output_phase(
            touch,
            Position2 { x: 2.0, y: 2.0 },
            &Mapper,
            point(2, 777),
            phase(basis(), 1100, 2, 1, 2),
        )
        .unwrap();
    assert_eq!(report.song_time, ts(249_999_999));
    assert_eq!(report.judge_events.len(), 1);
    assert!(matches!(queue.try_pop().unwrap(), AudioCommand::Play { at, .. } if at == ts(777)));
    let advanced = solo
        .advance_to_output_phase(
            point(9, 101),
            &Mapper,
            point(2, 888),
            phase(basis(), 1101, 2, 1, 2),
        )
        .unwrap();
    assert_eq!(advanced.song_time, ts(249_999_999));
    assert_eq!(advanced.audio_at, point(2, 888));
    assert!(!solo.poisoned());
}

struct PanicEvaluator;
impl InteractionEvaluator for PanicEvaluator {
    fn validate(&self, _: &TimedObject, _: &JudgeProfile) -> Result<(), JudgeError> {
        Ok(())
    }
    fn begin(
        &self,
        object: &TimedObject,
        context: &BeginContext<'_>,
    ) -> Box<dyn ActiveInteraction> {
        // Judge construction eagerly begins objects. Defer the fault to the
        // real routed input callback executed by Runtime inside OwnerGuard.
        Box::new(PanicOnInput(PressInstantEvaluator.begin(object, context)))
    }
}
struct PanicOnInput(Box<dyn ActiveInteraction>);
impl ActiveInteraction for PanicOnInput {
    fn state(&self) -> InteractionState {
        self.0.state()
    }
    fn accepts_input(
        &self,
        event: &beatkernel::input::GameInputEvent,
        context: &InteractionContext<'_>,
    ) -> bool {
        self.0.accepts_input(event, context)
    }
    fn on_input(
        &mut self,
        _: &beatkernel::input::GameInputEvent,
        _: &InteractionContext<'_>,
    ) -> InteractionOutput {
        panic!("actual evaluator routed-input unwind fixture")
    }
    fn advance_to(
        &mut self,
        song_time: Timestamp,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        self.0.advance_to(song_time, context)
    }
    fn deadline(&self, profile: &JudgeProfile) -> Option<i128> {
        self.0.deadline(profile)
    }
}
#[test]
fn evaluator_unwind_restores_shared_owners_and_cleanup_uses_original_queue() {
    let (producer, mut queue) = command_queue(8).unwrap();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(basis()),
        producer,
        vec![member_with(7, Box::new(PanicEvaluator)), member(29)],
        8,
        &[],
    )
    .unwrap();
    let before = group.transport().clone();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        group.process_input_output_phase(
            input(7, 100),
            &Mapper,
            point(2, 777),
            phase(basis(), 1100, 2, 1, 2),
        )
    }))
    .is_err());
    assert!(group.poisoned());
    assert_eq!(group.transport(), &before);
    let cleanup = AudioCommand::Stop {
        voice: VoiceId(7),
        at: ts(777),
    };
    group.enqueue_audio(cleanup).unwrap();
    assert_eq!(queue.try_pop().unwrap(), cleanup);
}

#[test]
fn mapper_unwind_inside_real_runtime_guard_restores_shared_owners() {
    struct SecondMapPanics(Cell<usize>);
    impl ClockMapper for SecondMapPanics {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            let calls = self.0.get();
            self.0.set(calls + 1);
            if calls == 1 {
                panic!("actual guarded mapper unwind fixture")
            }
            Mapper.map(from, to)
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Unknown
        }
    }
    let (mut group, mut queue) = group(&[7, 29], 8);
    let before = group.transport().clone();
    let mapper = SecondMapPanics(Cell::new(0));
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        group.process_input_output_phase(
            input(7, 100),
            &mapper,
            point(2, 777),
            phase(basis(), 1100, 2, 1, 2),
        )
    }))
    .is_err());
    assert_eq!(mapper.0.get(), 2);
    assert!(group.poisoned());
    assert_eq!(group.transport(), &before);
    let cleanup = AudioCommand::Stop {
        voice: VoiceId(7),
        at: ts(777),
    };
    group.enqueue_audio(cleanup).unwrap();
    assert_eq!(queue.try_pop().unwrap(), cleanup);
}
