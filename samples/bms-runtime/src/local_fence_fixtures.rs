//! Deferred shared software ownership; no device, gauge-triggered fencing or drain proof.
use crate::{
    local_players::PlayerId,
    local_runtime::{FailureKind, InputResult, MemberConfig, RuntimeGroup, SoloRuntime},
};
use beatkernel::{
    audio::{command_queue, AudioCommand, CommandConsumer, SampleId, VoiceId},
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::{InteractionState, PressHoldEvaluator},
    judge::{
        HazardId, HazardMarker, HazardOutcome, HazardTimeline, JudgeEngine, JudgeGrade,
        JudgeProfile, JudgeStage, JudgeWindow, Rule,
    },
    runtime::{
        RuntimeProcessingClock, SoundBinding,
        hazard_sound::{HazardSoundBinding, HazardSoundTimeline},
    },
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn player(index: usize, count: usize) -> PlayerId {
    PlayerId(if index + 1 == count {
        u32::MAX
    } else {
        index as u32 * 37 + 1
    })
}
fn device(index: usize) -> DeviceId {
    DeviceId(u64::MAX - index as u64 * 17)
}
fn input(index: usize, at: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(device(index), point(1, 1000 + at), sequence),
        control: PhysicalControlId::keyboard(4),
        state,
    })
}
fn config(index: usize, count: usize) -> MemberConfig {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(10).unwrap(),
        end: Some(Beat::new(100).unwrap()),
        interaction: InteractionId(1),
        visual: VisualId(0),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    let mut judge = JudgeEngine::new(
        source.compile().unwrap(),
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(PressHoldEvaluator),
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
    .unwrap();
    judge
        .configure_hazards(
            HazardTimeline::new(
                vec![HazardMarker {
                    id: HazardId(u64::MAX),
                    at: ts(50),
                    control: GameControlId(1),
                    value: 1,
                }],
                1,
            )
            .unwrap(),
        )
        .unwrap();
    MemberConfig {
        player: player(index, count),
        device: Some(device(index)),
        judge,
        bindings: BindingMap::from_bindings([Binding {
            device: DeviceSelector::Exact(device(index)),
            physical: PhysicalControlId::keyboard(4),
            game_control: GameControlId(1),
        }])
        .unwrap(),
        sounds: [JudgeStage::HoldHead, JudgeStage::HoldTail]
            .map(|stage| SoundBinding {
                object: ObjectId(1),
                stage,
                sample: SampleId(1),
                voice: VoiceId(1 + index as u64),
                gain: 1.0,
            })
            .to_vec(),
    }
}
fn play(voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(1),
        voice: VoiceId(voice),
        at: ts(at),
        gain: 1.0,
    }
}
fn group(count: usize, capacity: usize) -> (RuntimeGroup, CommandConsumer) {
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(1000), ts(0), Rate::NORMAL),
        producer,
        (0..count).map(|index| config(index, count)).collect(),
        8,
        &[VoiceId(999)],
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    group
        .configure_hazard_sounds(
            (0..count)
                .map(|index| {
                    (
                        player(index, count),
                        HazardSoundTimeline::new(
                            vec![HazardSoundBinding {
                                hazard: HazardId(u64::MAX),
                                sample: SampleId(1),
                                voice: VoiceId(1000 + index as u64),
                                gain: 1.0,
                            }],
                            1,
                        )
                        .unwrap(),
                    )
                })
                .collect(),
        )
        .unwrap();
    (group, consumer)
}

#[test]
fn exact_sparse_members_keep_independent_frontiers_and_survivors_judge_on_the_one_shared_queue() {
    for count in [2, 3, 4, 64] {
        let (mut group, mut consumer) = group(count, 256);
        let first = player(0, count);
        assert_eq!(group.fence_player(first).unwrap(), None);
        assert!(group.fence_player(PlayerId(0)).is_err());
        assert!(!group.poisoned());
        assert_eq!(group.player_gameplay_fence(PlayerId(0)), None);
        group.set_song_end(ts(150)).unwrap(); // No-commit fencing left member setup open.
        for index in 0..count {
            let InputResult::Processed(reports) = group
                .process_input(
                    input(index, 10, 1, ButtonState::Down),
                    &Identity,
                    point(2, 700),
                )
                .unwrap()
            else {
                panic!("known exact source")
            };
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].player, player(index, count));
            assert_eq!(
                reports[0].report.bound_inputs[0].physical.meta().source,
                device(index)
            );
            assert_eq!(
                reports[0].report.judge_events[0].stage,
                JudgeStage::HoldHead
            );
            assert_eq!(
                reports[0].report.audio_commands,
                [play(1 + index as u64, 700)]
            );
        }
        let hash = group.member_judge(first).unwrap().stable_hash().unwrap();
        assert_eq!(group.fence_player(first).unwrap(), Some(ts(10)));
        let anchors = group.transport().anchors().to_vec();
        let mut unknown = input(0, 10_000, u64::MAX, ButtonState::Down);
        unknown.meta_mut().source = DeviceId(42);
        assert!(matches!(
            group
                .process_input(unknown, &Identity, point(2, 0))
                .unwrap(),
            InputResult::Ignored {
                device: DeviceId(42)
            }
        ));
        let advanced = group
            .advance_to(point(1, 1050), &Identity, point(2, 800))
            .unwrap();
        assert_eq!(advanced.len(), count);
        for (index, report) in advanced.iter().enumerate() {
            assert_eq!(report.player, player(index, count));
            if index == 0 {
                assert_eq!(report.report.song_time, ts(10));
                assert!(report.report.hazard_events.is_empty());
                assert!(report.report.audio_commands.is_empty());
            } else {
                assert_eq!(report.report.song_time, ts(50));
                assert_eq!(report.report.hazard_events.len(), 1);
                assert_eq!(
                    report.report.hazard_events[0].outcome,
                    HazardOutcome::Triggered
                );
                assert_eq!(
                    report.report.audio_commands,
                    [play(1000 + index as u64, 800)]
                );
                assert_eq!(group.player_gameplay_fence(player(index, count)), None);
            }
        }
        for index in 0..count {
            let physical = input(index, 100, 2, ButtonState::Up);
            let InputResult::Processed(reports) = group
                .process_input(physical.clone(), &Identity, point(2, 900))
                .unwrap()
            else {
                panic!("known release")
            };
            if index == 0 {
                assert_eq!(reports[0].report.song_time, ts(10));
                assert_eq!(reports[0].report.input, Some(physical));
                assert!(
                    reports[0].report.bound_inputs.is_empty()
                        && reports[0].report.judge_events.is_empty()
                        && reports[0].report.audio_commands.is_empty()
                );
            } else {
                assert_eq!(
                    reports[0].report.judge_events[0].stage,
                    JudgeStage::HoldTail
                );
                assert_eq!(
                    reports[0].report.audio_commands,
                    [play(1 + index as u64, 900)]
                );
                assert_eq!(
                    group
                        .member_judge(player(index, count))
                        .unwrap()
                        .state(ObjectId(1)),
                    Some(InteractionState::Completed)
                );
            }
        }
        let final_reports = group
            .advance_to(point(1, 1200), &Identity, point(2, 1000))
            .unwrap();
        assert!(
            final_reports
                .iter()
                .all(|entry| entry.report.song_end_reached)
        );
        assert_eq!(final_reports[0].report.song_time, ts(10));
        assert_eq!(group.fence_player(first).unwrap(), Some(ts(10)));
        assert_eq!(
            group.member_judge(first).unwrap().stable_hash().unwrap(),
            hash
        );
        assert_eq!(
            group.member_judge(first).unwrap().state(ObjectId(1)),
            Some(InteractionState::Active)
        );
        assert_eq!(group.member_judge(first).unwrap().remaining_hazards(), 1);
        assert_eq!(group.transport().anchors(), anchors);
        assert!(!group.poisoned());
        for index in 0..count {
            assert_eq!(consumer.try_pop().unwrap(), play(1 + index as u64, 700));
        }
        for index in 1..count {
            assert_eq!(consumer.try_pop().unwrap(), play(1000 + index as u64, 800));
        }
        for index in 1..count {
            assert_eq!(consumer.try_pop().unwrap(), play(1 + index as u64, 900));
        }
        assert!(consumer.try_pop().is_err());
        let stop = AudioCommand::Stop {
            voice: VoiceId(999),
            at: ts(1001),
        };
        group.enqueue_audio(stop).unwrap();
        assert_eq!(consumer.try_pop().unwrap(), stop);
    }
}

#[test]
fn solo_and_poisoned_cohort_retain_committed_fences_without_reopening_failure_or_flushing_audio() {
    let MemberConfig {
        bindings,
        judge,
        sounds,
        ..
    } = config(0, 1);
    let (producer, mut consumer) = command_queue(4).unwrap();
    let mut solo = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(1000), ts(0), Rate::NORMAL),
        bindings,
        judge,
        producer,
        sounds,
        8,
    )
    .unwrap();
    solo.set_processing_clock(RuntimeProcessingClock::Disabled);
    assert_eq!(solo.fence_gameplay(), None);
    assert_eq!(solo.gameplay_fence(), None);
    let accepted = solo
        .process_input(input(0, 10, 1, ButtonState::Down), &Identity, point(2, 700))
        .unwrap();
    let hash = solo.judge().stable_hash().unwrap();
    assert_eq!(solo.fence_gameplay(), Some(ts(10)));
    let ignored = solo
        .process_input(input(0, 100, 2, ButtonState::Up), &Identity, point(2, 800))
        .unwrap();
    assert!(ignored.judge_events.is_empty() && ignored.bound_inputs.is_empty());
    assert_eq!(ignored.song_time, ts(10));
    let later = solo
        .advance_to(point(1, 1200), &Identity, point(2, 900))
        .unwrap();
    assert!(later.hazard_events.is_empty() && later.audio_commands.is_empty());
    assert_eq!(solo.judge().stable_hash().unwrap(), hash);
    assert_eq!(solo.gameplay_fence(), Some(ts(10)));
    assert_eq!(consumer.try_pop().unwrap(), accepted.audio_commands[0]);
    assert!(consumer.try_pop().is_err());

    let (mut group, mut consumer) = group(2, 1);
    group
        .process_input(input(0, 10, 1, ButtonState::Down), &Identity, point(2, 700))
        .unwrap();
    let failure = group
        .process_input(input(1, 10, 1, ButtonState::Down), &Identity, point(2, 701))
        .unwrap_err();
    assert!(matches!(failure.kind, FailureKind::ReportedFailure));
    assert!(group.poisoned());
    assert_eq!(failure.failed_player, Some(player(1, 2)));
    assert_eq!(failure.completed_reports.len(), 1);
    assert_eq!(
        failure.completed_reports[0].report.judge_events[0].stage,
        JudgeStage::HoldHead
    );
    assert_eq!(
        failure.completed_reports[0].report.audio_failures[0].command,
        play(2, 701)
    );
    let hashes: Vec<_> = (0..2)
        .map(|index| {
            group
                .member_judge(player(index, 2))
                .unwrap()
                .stable_hash()
                .unwrap()
        })
        .collect();
    for index in 0..2 {
        assert_eq!(group.fence_player(player(index, 2)).unwrap(), Some(ts(10)));
    }
    assert!(group.fence_player(PlayerId(0)).is_err());
    assert!(group.poisoned());
    let still_failed = group
        .advance_to(point(1, 1200), &Identity, point(2, 900))
        .unwrap_err();
    assert!(matches!(still_failed.kind, FailureKind::Poisoned));
    assert!(still_failed.completed_reports.is_empty());
    for index in 0..2 {
        assert_eq!(
            group
                .member_judge(player(index, 2))
                .unwrap()
                .stable_hash()
                .unwrap(),
            hashes[index]
        );
        assert_eq!(group.player_gameplay_fence(player(index, 2)), Some(ts(10)));
    }
    assert_eq!(consumer.try_pop().unwrap(), play(1, 700));
    assert!(consumer.try_pop().is_err());
    let stop = AudioCommand::Stop {
        voice: VoiceId(2),
        at: ts(1000),
    };
    group.enqueue_audio(stop).unwrap();
    assert_eq!(consumer.try_pop().unwrap(), stop);
    assert!(group.poisoned());
}
