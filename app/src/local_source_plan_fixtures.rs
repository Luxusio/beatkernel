//! Deferred source fixtures for validated local ownership and the real RuntimeGroup.
use crate::{
    local_players::{InputPlan, PlayerId, ResolvedInputPlan},
    local_runtime::{InputResult, MemberConfig, RuntimeGroup},
    settings::MAX_VALUE_BYTES,
};
use beatkernel::{
    audio::{AudioCommand, CommandConsumer, SampleId, VoiceId, command_queue},
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::InstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    runtime::{RuntimeProcessingClock, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};

const HOST: ClockDomainId = ClockDomainId(91);
const OUTPUT: ClockDomainId = ClockDomainId(92);
const ORIGIN: i64 = 9_007_199_254_740_993;

struct SameDomain;
impl ClockMapper for SameDomain {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn host(at: i64) -> ClockPoint {
    ClockPoint {
        domain: HOST,
        timestamp: Timestamp::from_nanos(at),
    }
}
fn output(at: i64) -> ClockPoint {
    ClockPoint {
        domain: OUTPUT,
        timestamp: Timestamp::from_nanos(at),
    }
}
fn input(device: DeviceId, at: i64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(device, host(at), u64::MAX),
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    })
}
fn member(player: PlayerId, device: Option<DeviceId>, selector: DeviceSelector) -> MemberConfig {
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
        player,
        device,
        bindings: BindingMap::from_bindings([Binding {
            device: selector,
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
            voice: VoiceId(u64::from(player.0)),
            gain: 1.0,
        }],
    }
}
fn group(configs: Vec<MemberConfig>) -> Result<(RuntimeGroup, CommandConsumer), String> {
    let (producer, consumer) = command_queue(8).unwrap();
    let mut owner = RuntimeGroup::new(
        HOST,
        OUTPUT,
        Transport::new(Timestamp::from_nanos(ORIGIN), Timestamp::ZERO, Rate::NORMAL),
        producer,
        configs,
        8,
        &[],
    )?;
    owner.set_processing_clock(RuntimeProcessingClock::Disabled);
    Ok((owner, consumer))
}

#[test]
fn resolved_plan_roundtrips_literal_words_with_full_width_order_and_independent_storage() {
    let automatic = ResolvedInputPlan::new(vec![(PlayerId(u32::MAX), None)]).unwrap();
    assert_eq!(automatic.to_words(), [u32::MAX, 0, 0, 0]);
    assert_eq!(
        ResolvedInputPlan::from_words(&[u32::MAX, 0, 0, 0]).unwrap(),
        automatic
    );
    let exact_zero = ResolvedInputPlan::from_words(&[7, 1, 0, 0]).unwrap();
    assert_eq!(exact_zero.members(), &[(PlayerId(7), Some(DeviceId(0)))]);
    assert_eq!(exact_zero.to_words(), [7, 1, 0, 0]);
    let mut wire = vec![
        19,
        1,
        0x4433_2211,
        0x8877_6655,
        u32::MAX,
        1,
        u32::MAX,
        u32::MAX,
        2,
        1,
        0,
        0,
    ];
    let admitted = ResolvedInputPlan::from_words(&wire).unwrap();
    let expected = vec![
        (PlayerId(19), Some(DeviceId(0x8877_6655_4433_2211))),
        (PlayerId(u32::MAX), Some(DeviceId(u64::MAX))),
        (PlayerId(2), Some(DeviceId(0))),
    ];
    assert_eq!(admitted.members(), expected.as_slice());
    assert_eq!(ResolvedInputPlan::new(expected).unwrap(), admitted);
    let original = wire.clone();
    wire.fill(0);
    assert_eq!(admitted.to_words(), original);
    let mut exported = admitted.to_words();
    exported[0] = 0;
    assert_eq!(admitted.clone().to_words(), original);
}

#[test]
fn complete_plan_bounds_reject_invalid_selectors_payloads_and_ambiguous_members() {
    for words in [
        vec![],
        vec![1],
        vec![1, 1, 2],
        vec![1, 1, 2, 0, 9],
        vec![0, 1, 0, 0],
        vec![1, 2, 0, 0],
        vec![1, u32::MAX, 0, 0],
        vec![1, 0, 1, 0],
        vec![1, 0, 0, 1],
        vec![1, 1, 3, 0, 1, 1, 4, 0],
        vec![1, 1, 3, 0, 2, 1, 3, 0],
        vec![1, 0, 0, 0, 2, 1, 3, 0],
        vec![1, 1, 3, 0, 2, 0, 0, 0],
    ] {
        assert!(
            ResolvedInputPlan::from_words(&words).is_err(),
            "accepted malformed words: {words:?}"
        );
    }
    for members in [
        vec![],
        vec![(PlayerId(0), None)],
        vec![(PlayerId(1), None), (PlayerId(2), None)],
        vec![
            (PlayerId(1), Some(DeviceId(0))),
            (PlayerId(2), Some(DeviceId(0))),
        ],
        vec![
            (PlayerId(1), Some(DeviceId(3))),
            (PlayerId(1), Some(DeviceId(4))),
        ],
    ] {
        assert!(ResolvedInputPlan::new(members).is_err());
    }
    let maximum: Vec<_> = (0..64_u32)
        .map(|index| {
            (
                PlayerId(u32::MAX - index),
                Some(DeviceId(u64::MAX - u64::from(index))),
            )
        })
        .collect();
    let admitted = ResolvedInputPlan::new(maximum.clone()).unwrap();
    assert_eq!(admitted.members(), maximum.as_slice());
    let mut words = admitted.to_words();
    assert_eq!(words.len(), 256);
    assert_eq!(ResolvedInputPlan::from_words(&words).unwrap(), admitted);
    words.extend_from_slice(&[1, 1, 0, 0]);
    assert!(ResolvedInputPlan::from_words(&words).is_err());
    let mut excess = maximum;
    excess.push((PlayerId(1), Some(DeviceId(0))));
    assert!(ResolvedInputPlan::new(excess).is_err());
}

#[test]
fn native_drafts_are_fully_preflighted_before_lookup_and_keep_real_lookup_or_alias_evidence() {
    let route = |player, identity: &str| (PlayerId(player), identity.to_string());
    for assignments in [
        vec![],
        vec![route(1, "first")],
        vec![route(1, "first"), route(0, "last")],
        vec![route(1, "first"), route(1, "last")],
        vec![route(1, "first"), route(2, "first")],
        vec![route(1, "first"), route(2, "")],
        vec![route(1, "first"), route(2, "bad\nidentity")],
        vec![route(1, "first"), route(2, "bad\u{2028}identity")],
        vec![
            route(1, "first"),
            route(2, &"x".repeat(MAX_VALUE_BYTES + 1)),
        ],
        (1..=65)
            .map(|id| route(id, &format!("device-{id}")))
            .collect(),
    ] {
        let mut looked_up = Vec::new();
        assert!(
            InputPlan::Assigned(assignments)
                .resolve(|identity| {
                    looked_up.push(identity.to_string());
                    Ok(DeviceId(looked_up.len() as u64))
                })
                .is_err()
        );
        assert!(
            looked_up.is_empty(),
            "malformed tail acquired a valid native prefix"
        );
    }
    assert!(
        InputPlan::Automatic {
            player: PlayerId(1)
        }
        .resolve(|_| { panic!("automatic ownership has no explicit device lookup") })
        .is_err()
    );
    let native = InputPlan::Assigned(vec![
        route(9, "zero"),
        route(u32::MAX, "wide"),
        route(2, "last"),
    ]);
    let mut looked_up = Vec::new();
    let resolved = native
        .resolve(|identity| {
            looked_up.push(identity.to_string());
            Ok(DeviceId(match identity {
                "zero" => 0,
                "wide" => u64::MAX,
                _ => 0x8877_6655_4433_2211,
            }))
        })
        .unwrap();
    assert_eq!(looked_up, ["zero", "wide", "last"]);
    assert_eq!(
        resolved,
        [
            (PlayerId(9), DeviceId(0)),
            (PlayerId(u32::MAX), DeviceId(u64::MAX)),
            (PlayerId(2), DeviceId(0x8877_6655_4433_2211))
        ]
    );
    let checked = ResolvedInputPlan::new(
        resolved
            .iter()
            .map(|&(id, device)| (id, Some(device)))
            .collect(),
    )
    .unwrap();
    assert_eq!(
        ResolvedInputPlan::from_words(&checked.to_words()).unwrap(),
        checked
    );
    looked_up.clear();
    let failure = native
        .resolve(|identity| {
            looked_up.push(identity.to_string());
            if identity == "wide" {
                Err("actual native lookup failure".into())
            } else {
                Ok(DeviceId(0))
            }
        })
        .unwrap_err();
    assert_eq!(failure, "actual native lookup failure");
    assert_eq!(looked_up, ["zero", "wide"]);
    looked_up.clear();
    assert!(
        native
            .resolve(|identity| {
                looked_up.push(identity.to_string());
                Ok(DeviceId(u64::MAX))
            })
            .is_err()
    );
    assert_eq!(
        looked_up,
        ["zero", "wide"],
        "aliases refuse before acquiring later devices"
    );
}

#[test]
fn actual_runtime_group_rejects_forged_source_plans_and_exact_selector_mismatches() {
    for configs in [
        vec![],
        vec![member(PlayerId(0), None, DeviceSelector::Any)],
        vec![
            member(PlayerId(1), None, DeviceSelector::Any),
            member(
                PlayerId(2),
                Some(DeviceId(2)),
                DeviceSelector::Exact(DeviceId(2)),
            ),
        ],
        vec![
            member(
                PlayerId(1),
                Some(DeviceId(1)),
                DeviceSelector::Exact(DeviceId(1)),
            ),
            member(
                PlayerId(1),
                Some(DeviceId(2)),
                DeviceSelector::Exact(DeviceId(2)),
            ),
        ],
        vec![
            member(
                PlayerId(1),
                Some(DeviceId(0)),
                DeviceSelector::Exact(DeviceId(0)),
            ),
            member(
                PlayerId(2),
                Some(DeviceId(0)),
                DeviceSelector::Exact(DeviceId(0)),
            ),
        ],
        vec![member(PlayerId(1), Some(DeviceId(0)), DeviceSelector::Any)],
        vec![member(
            PlayerId(1),
            Some(DeviceId(0)),
            DeviceSelector::Exact(DeviceId(u64::MAX)),
        )],
        (1..=65_u32)
            .map(|id| {
                member(
                    PlayerId(id),
                    Some(DeviceId(u64::from(id))),
                    DeviceSelector::Exact(DeviceId(u64::from(id))),
                )
            })
            .collect(),
    ] {
        assert!(group(configs).is_err());
    }
    for selected in [None, Some(DeviceId(0))] {
        let (mut owner, mut audio) = group(vec![member(
            PlayerId(u32::MAX),
            selected,
            selected.map_or(DeviceSelector::Any, DeviceSelector::Exact),
        )])
        .unwrap();
        let device = selected.unwrap_or(DeviceId(u64::MAX));
        let InputResult::Processed(reports) = owner
            .process_input(input(device, ORIGIN + 2_000_000), &SameDomain, output(400))
            .unwrap()
        else {
            panic!("valid solo source was ignored")
        };
        assert_eq!(reports[0].player, PlayerId(u32::MAX));
        assert_eq!(reports[0].report.judge_events.len(), 1);
        assert!(matches!(
            reports[0].report.judge_events[0].outcome,
            JudgeOutcome::Hit { .. }
        ));
        assert!(
            matches!(audio.try_pop().unwrap(), AudioCommand::Play { voice: VoiceId(value), .. } if value == u64::from(u32::MAX))
        );
    }
}

#[test]
fn three_and_four_resolved_members_keep_independent_judges_on_one_original_transport_and_queue() {
    let ordered = [
        (PlayerId(u32::MAX), Some(DeviceId(0))),
        (PlayerId(7), Some(DeviceId(u64::MAX))),
        (PlayerId(91), Some(DeviceId(0x8877_6655_4433_2211))),
        (PlayerId(2), Some(DeviceId(1))),
    ];
    for count in [3, 4] {
        let plan = ResolvedInputPlan::from_words(
            &ResolvedInputPlan::new(ordered[..count].to_vec())
                .unwrap()
                .to_words(),
        )
        .unwrap();
        let (mut owner, mut audio) = group(
            plan.members()
                .iter()
                .map(|&(player, device)| {
                    member(player, device, DeviceSelector::Exact(device.unwrap()))
                })
                .collect(),
        )
        .unwrap();
        assert!(matches!(
            owner
                .process_input(input(DeviceId(99), i64::MAX), &SameDomain, output(0))
                .unwrap(),
            InputResult::Ignored {
                device: DeviceId(99)
            }
        ));
        for &(player, device) in &plan.members()[..count - 1] {
            let original = input(device.unwrap(), ORIGIN + 2_000_000);
            let InputResult::Processed(reports) = owner
                .process_input(original.clone(), &SameDomain, output(700))
                .unwrap()
            else {
                panic!("exact resolved source was ignored")
            };
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].player, player);
            let report = &reports[0].report;
            assert_eq!(report.song_time, Timestamp::from_nanos(2_000_000));
            assert_eq!(report.input.as_ref(), Some(&original));
            assert_eq!(report.bound_inputs.len(), 1);
            assert_eq!(report.bound_inputs[0].physical, original);
            assert_eq!(report.judge_events.len(), 1);
            assert_eq!(report.judge_events[0].input, Some(*original.meta()));
            assert!(matches!(
                report.judge_events[0].outcome,
                JudgeOutcome::Hit {
                    delta: Duration::ZERO,
                    ..
                }
            ));
            assert_eq!(
                report.audio_commands,
                [AudioCommand::Play {
                    voice: VoiceId(u64::from(player.0)),
                    sample: SampleId(1),
                    at: Timestamp::from_nanos(700),
                    gain: 1.0,
                }]
            );
            assert_eq!(audio.try_pop().unwrap(), report.audio_commands[0]);
        }
        let reports = owner
            .advance_to(host(ORIGIN + 2_000_001), &SameDomain, output(701))
            .unwrap();
        assert_eq!(
            reports.iter().map(|row| row.player).collect::<Vec<_>>(),
            plan.members().iter().map(|row| row.0).collect::<Vec<_>>()
        );
        assert!(
            reports
                .iter()
                .all(|row| row.report.song_time == Timestamp::from_nanos(2_000_001))
        );
        assert!(
            reports[..count - 1]
                .iter()
                .all(|row| row.report.judge_events.is_empty())
        );
        assert_eq!(reports[count - 1].report.judge_events.len(), 1);
        assert!(matches!(
            reports[count - 1].report.judge_events[0].outcome,
            JudgeOutcome::Miss { .. }
        ));
        assert_eq!(reports[count - 1].report.judge_events[0].input, None);
        assert_eq!(
            owner
                .transport_mut()
                .position_at(host(ORIGIN + 2_000_001).timestamp)
                .unwrap(),
            Timestamp::from_nanos(2_000_001)
        );
        assert!(audio.try_pop().is_err());
        assert!(!owner.poisoned());
    }
}
