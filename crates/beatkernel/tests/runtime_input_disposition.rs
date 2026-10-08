//! Public Runtime admission evidence and legacy owner/output parity.
use beatkernel::{
    audio::{command_queue, AudioCommand, CommandConsumer, QueuePushError, SampleId, VoiceId},
    chart::*,
    input::*,
    interaction::PressInstantEvaluator,
    judge::*,
    runtime::{
        input_sound::{InputSoundMarker, InputSoundTimeline},
        Runtime, RuntimeError, RuntimeProcessingClock, RuntimeReport, SoundBinding,
    },
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockMapper, ClockMappingQuality,
        ClockPair, ClockPoint, Duration, Timestamp,
    },
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
struct NoMapping;
impl ClockMapper for NoMapping {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn mapper() -> AffineClockMapper {
    AffineClockMapper::exact_offset(
        ClockPair {
            source: point(9, 0),
            target: point(1, 1000),
        },
        ClockInterval {
            start: ts(0),
            end: ts(1000),
        },
    )
    .unwrap()
}
fn physical() -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(17),
        code: 7,
    }
}
fn button(song: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(55), point(1, 1000 + song), sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(17),
        code: Some(7),
        timestamp: Some(point(9, song)),
    });
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: physical(),
        state,
    })
}
fn touch(song: i64, sequence: u64, phase: TouchPhase) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: *button(song, sequence, ButtonState::Down).meta(),
        control: physical(),
        contact: ContactId(88),
        phase,
        position: Position2 {
            x: 500.0,
            y: -500.0,
        },
        pressure: Some(0.75),
    })
}
fn bound(input: PhysicalInputEvent, control: u32) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(control),
        physical: input,
    }
}
fn marker(id: u64, at: i64, control: u32) -> HazardMarker {
    HazardMarker {
        id: HazardId(id),
        at: ts(at),
        control: GameControlId(control),
        value: id,
    }
}
fn engine(
    notes: &[(u64, i64, u32)],
    offset: i64,
    resolver: Box<dyn CandidateResolver>,
    hazards: Vec<HazardMarker>,
) -> JudgeEngine {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = notes
        .iter()
        .map(|&(id, at, _)| SourceObject {
            id: ObjectId(id),
            start: Beat::new(at).unwrap(),
            end: None,
            interaction: InteractionId(id as u32),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .collect();
    let rules = notes
        .iter()
        .map(|&(id, _, control)| Rule {
            interaction: InteractionId(id as u32),
            control: GameControlId(control),
            evaluator: Box::new(PressInstantEvaluator),
        })
        .collect();
    let mut judge = JudgeEngine::with_policies_and_contacts(
        source.compile().unwrap(),
        rules,
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::from_nanos(offset),
        )
        .unwrap(),
        resolver,
        Box::new(WindowJudgePolicy),
    )
    .unwrap();
    if !hazards.is_empty() {
        judge
            .configure_hazards(HazardTimeline::new(hazards, 100).unwrap())
            .unwrap();
    }
    judge
}
fn runtime(
    judge: JudgeEngine,
    controls: &[u32],
    capacity: usize,
    sounds: Vec<SoundBinding>,
) -> (Runtime, CommandConsumer) {
    let bindings = BindingMap::from_bindings(controls.iter().map(|&control| Binding {
        device: DeviceSelector::Any,
        physical: physical(),
        game_control: GameControlId(control),
    }))
    .unwrap();
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut runtime = Runtime::new(
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
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    (runtime, consumer)
}
fn assert_report_eq(left: &RuntimeReport, right: &RuntimeReport) {
    assert_eq!(left.input, right.input);
    assert_eq!(left.bound_inputs, right.bound_inputs);
    assert_eq!(left.song_time, right.song_time);
    assert_eq!(left.song_end_reached, right.song_end_reached);
    assert_eq!(left.audio_at, right.audio_at);
    assert_eq!(left.input_mapping_quality, right.input_mapping_quality);
    assert_eq!(left.audio_mapping_quality, right.audio_mapping_quality);
    assert_eq!(left.judge_events, right.judge_events);
    assert_eq!(left.hazard_events, right.hazard_events);
    assert_eq!(left.judge_error, right.judge_error);
    assert_eq!(left.audio_commands, right.audio_commands);
    assert_eq!(left.audio_failures, right.audio_failures);
}
#[derive(Clone, Copy)]
struct RejectSecond;
impl CandidateResolver for RejectSecond {
    fn select(&self, candidates: &[Candidate]) -> Option<ObjectId> {
        candidates.first().map(|c| {
            if c.object == ObjectId(2) {
                ObjectId(999)
            } else {
                c.object
            }
        })
    }
    fn snapshot_clone(&self) -> Option<Box<dyn CandidateResolver>> {
        Some(Box::new(*self))
    }
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        Some(b"runtime-disposition-reject-second/v1".to_vec())
    }
}

#[derive(Clone, Copy)]
struct Decline;
impl CandidateResolver for Decline {
    fn select(&self, _: &[Candidate]) -> Option<ObjectId> {
        None
    }
    fn snapshot_clone(&self) -> Option<Box<dyn CandidateResolver>> {
        Some(Box::new(*self))
    }
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        Some(b"runtime-disposition-decline/v1".to_vec())
    }
}

#[test]
fn declined_candidates_and_passive_expiry_do_not_invent_matches_or_empty_penalties() {
    let make = || engine(&[(1, 0, 3), (2, 100, 1)], 0, Box::new(Decline), vec![]);
    let (mut observed, _queue) = runtime(make(), &[1, 2], 4, vec![]);
    let (mut legacy, _legacy_queue) = runtime(make(), &[1, 2], 4, vec![]);
    let input = button(100, 1, ButtonState::Down);
    let result = observed
        .process_input_report(input.clone(), &NoMapping, point(2, 0))
        .unwrap();
    let old = legacy
        .process_input(input, &NoMapping, point(2, 0))
        .unwrap();
    assert_report_eq(&result.report, &old);
    assert!(result.refused.is_none());
    assert_eq!(result.dispositions.len(), 2);
    let declined = result.dispositions[0];
    assert_eq!(declined.freshness(), InputFreshness::FreshPress);
    assert_eq!(declined.candidate_count(), 1);
    assert_eq!(declined.selected(), None);
    assert_eq!(declined.dispatched_count(), 0);
    assert_eq!(declined.input_result_count(), 0);
    assert_eq!(declined.passive_result_count(), 1);
    assert!(!declined.unmatched_fresh_press());
    let empty = result.dispositions[1];
    assert_eq!(empty.candidate_count(), 0);
    assert_eq!(empty.dispatched_count(), 0);
    assert_eq!(empty.input_result_count(), 0);
    assert_eq!(empty.passive_result_count(), 0);
    assert!(empty.unmatched_fresh_press());
    assert_eq!(
        result.report.judge_events,
        [JudgeEvent {
            object: ObjectId(1),
            stage: JudgeStage::Instant,
            at: ts(100),
            input: None,
            outcome: JudgeOutcome::Miss {
                reason: MissReason::HeadTimeout
            },
        }]
    );
    assert_eq!(
        observed.judge().stable_hash().unwrap(),
        legacy.judge().stable_hash().unwrap()
    );
}

#[test]
fn fanout_reports_only_successful_prefix_and_original_normalized_refusal() {
    for controls in [&[1, 2, 3][..], &[2, 1, 3][..]] {
        let make = || {
            engine(
                &[(1, 100, 1), (2, 100, 2)],
                0,
                Box::new(RejectSecond),
                vec![marker(1, 100, 1), marker(2, 100, 2)],
            )
        };
        let (mut observed, _queue) = runtime(make(), controls, 4, vec![]);
        let (mut legacy, _legacy_queue) = runtime(make(), controls, 4, vec![]);
        let mut original = button(100, 1, ButtonState::Down);
        original.meta_mut().clock_domain = ClockDomainId(9);
        original.meta_mut().timestamp = ts(100);
        let mut normalized = original.clone();
        normalized.meta_mut().clock_domain = ClockDomainId(1);
        normalized.meta_mut().timestamp = ts(1100);
        normalized.meta_mut().original_clock_point = Some(point(9, 100));
        let result = observed
            .process_input_report(original.clone(), &mapper(), point(2, -99))
            .unwrap();
        let old = legacy
            .process_input(original, &mapper(), point(2, -99))
            .unwrap();
        assert_report_eq(&result.report, &old);
        let accepted = usize::from(controls[0] == 1);
        assert_eq!(result.dispositions.len(), accepted);
        assert_eq!(
            result.report.bound_inputs,
            if accepted == 1 {
                vec![bound(normalized.clone(), 1)]
            } else {
                vec![]
            }
        );
        let refused = result.refused.unwrap();
        assert_eq!(refused.game_control, GameControlId(2));
        assert_eq!(refused.input, *normalized.meta());
        assert_eq!(
            refused.error,
            JudgeError::InvalidCandidate {
                object: ObjectId(999)
            }
        );
        assert_eq!(result.report.judge_error, Some(refused.error));
        if accepted == 1 {
            let facts = result.dispositions[0];
            assert_eq!(facts.freshness(), InputFreshness::FreshPress);
            assert_eq!(facts.candidate_count(), 1);
            assert_eq!(facts.selected(), Some(ObjectId(1)));
            assert_eq!(facts.dispatched_count(), 1);
            assert_eq!(facts.input_result_count(), 1);
            assert_eq!(facts.passive_result_count(), 0);
            assert_eq!(facts.input_hazard_count(), 2);
            assert!(!facts.unmatched_fresh_press());
        }
        assert!(observed.judge().is_fresh_press(&bound(normalized, 2)));
        assert_eq!(
            observed.judge().stable_hash().unwrap(),
            legacy.judge().stable_hash().unwrap()
        );
        assert_eq!(
            observed.telemetry().counters(),
            legacy.telemetry().counters()
        );
    }
}

#[test]
fn successful_fanout_keeps_order_and_inclusive_hazards_independent_of_empty_matches() {
    let make = || {
        engine(
            &[],
            0,
            Box::new(ClosestCandidate),
            vec![marker(1, 9, 1), marker(2, 10, 2), marker(3, 10, 1)],
        )
    };
    let (mut observed, _queue) = runtime(make(), &[2, 1], 4, vec![]);
    let (mut legacy, _legacy_queue) = runtime(make(), &[2, 1], 4, vec![]);
    let input = button(10, 1, ButtonState::Down);
    let result = observed
        .process_input_report(input.clone(), &NoMapping, point(2, 0))
        .unwrap();
    let old = legacy
        .process_input(input.clone(), &NoMapping, point(2, 0))
        .unwrap();
    assert_report_eq(&result.report, &old);
    assert_eq!(
        result.report.bound_inputs,
        [bound(input.clone(), 2), bound(input, 1)]
    );
    assert_eq!(result.dispositions.len(), 2);
    assert!(result.refused.is_none());
    for facts in &result.dispositions {
        assert_eq!(facts.freshness(), InputFreshness::FreshPress);
        assert_eq!(facts.candidate_count(), 0);
        assert_eq!(facts.selected(), None);
        assert_eq!(facts.dispatched_count(), 0);
        assert_eq!(facts.input_result_count(), 0);
        assert_eq!(facts.passive_result_count(), 0);
        assert!(facts.unmatched_fresh_press());
    }
    assert_eq!(result.dispositions[0].input_hazard_count(), 2);
    assert_eq!(result.dispositions[1].input_hazard_count(), 0);
    assert_eq!(
        result
            .report
            .hazard_events
            .iter()
            .map(|e| e.outcome)
            .collect::<Vec<_>>(),
        [
            HazardOutcome::Avoided,
            HazardOutcome::Triggered,
            HazardOutcome::Avoided
        ]
    );
    assert!(result.report.judge_events.is_empty());
    assert_eq!(
        observed.judge().stable_hash().unwrap(),
        legacy.judge().stable_hash().unwrap()
    );
}

#[test]
fn preflight_errors_leave_judge_unchanged_and_do_not_commit_acquisition() {
    let (mut owner, _queue) = runtime(
        engine(&[], 0, Box::new(ClosestCandidate), vec![]),
        &[1],
        4,
        vec![],
    );
    let before = owner.judge().stable_hash().unwrap();
    let mut unknown = button(10, 1, ButtonState::Down);
    unknown.meta_mut().clock_domain = ClockDomainId(99);
    assert!(matches!(
        owner.process_input_report(unknown, &NoMapping, point(2, 0)),
        Err(RuntimeError::UnmappedClock {
            from: ClockDomainId(99),
            to: ClockDomainId(1)
        })
    ));
    assert!(matches!(
        owner.process_input_report(button(10, 1, ButtonState::Down), &NoMapping, point(99, 0)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    assert_eq!(owner.judge().stable_hash().unwrap(), before);
    let first = owner
        .process_input_report(button(10, 1, ButtonState::Down), &NoMapping, point(2, 0))
        .unwrap();
    assert_eq!(
        first.dispositions[0].freshness(),
        InputFreshness::FreshPress
    );
    let committed = owner.judge().stable_hash().unwrap();
    assert!(matches!(
        owner.process_input_report(button(11, 0, ButtonState::Up), &NoMapping, point(2, 0)),
        Err(RuntimeError::SequenceRegression {
            last: 1,
            received: 0,
            ..
        })
    ));
    assert_eq!(
        owner
            .process_input_report(button(9, 2, ButtonState::Up), &NoMapping, point(2, 0))
            .unwrap_err(),
        RuntimeError::NonMonotonicHost
    );
    assert_eq!(owner.judge().stable_hash().unwrap(), committed);
    let up = owner
        .process_input_report(button(11, 2, ButtonState::Up), &NoMapping, point(2, 0))
        .unwrap();
    assert_eq!(up.dispositions.len(), 1);
    assert_eq!(up.dispositions[0].freshness(), InputFreshness::Other);
    assert!(up.refused.is_none());
}

#[test]
fn unbound_fenced_and_ended_operations_have_no_accepted_input_evidence() {
    for operation in ["unbound", "fenced", "ended"] {
        let (mut owner, _queue) = runtime(
            engine(&[], 0, Box::new(ClosestCandidate), vec![marker(1, 20, 1)]),
            &[1],
            4,
            vec![],
        );
        if operation == "ended" {
            owner.set_song_end(ts(20)).unwrap();
        }
        owner
            .process_input_report(button(10, 1, ButtonState::Up), &NoMapping, point(2, 0))
            .unwrap();
        if operation == "fenced" {
            assert_eq!(owner.fence_gameplay(), Some(ts(10)));
        }
        let before = owner.judge().stable_hash().unwrap();
        let mut input = button(20, 2, ButtonState::Down);
        if operation == "unbound" {
            let PhysicalInputEvent::Button(payload) = &mut input else {
                unreachable!()
            };
            payload.control = PhysicalControlId::keyboard(99);
        }
        let result = owner
            .process_input_report(input, &NoMapping, point(2, 1))
            .unwrap();
        assert!(result.dispositions.is_empty());
        assert!(result.report.bound_inputs.is_empty());
        assert!(result.refused.is_none());
        assert!(result.report.judge_error.is_none());
        if operation == "ended" {
            assert!(result.report.song_end_reached);
            assert!(result.report.input.is_none());
            assert_eq!(result.report.hazard_events.len(), 1);
            assert_eq!(result.report.hazard_events[0].input, None);
        } else {
            assert!(result.report.input.is_some());
            assert!(result.report.hazard_events.is_empty());
            assert_eq!(owner.judge().stable_hash().unwrap(), before);
        }
    }
}

#[test]
fn endpoint_advance_error_is_never_a_refused_bound_input() {
    let (mut owner, _queue) = runtime(
        engine(&[], i64::MAX, Box::new(ClosestCandidate), vec![]),
        &[1],
        4,
        vec![],
    );
    owner.set_song_end(ts(1)).unwrap();
    let first = owner
        .process_input_report(button(0, 1, ButtonState::Down), &NoMapping, point(2, 0))
        .unwrap();
    assert_eq!(first.dispositions.len(), 1);
    let before = owner.judge().stable_hash().unwrap();
    let ending = owner
        .process_input_report(button(1, 2, ButtonState::Up), &NoMapping, point(2, 0))
        .unwrap();
    assert!(ending.report.song_end_reached);
    assert_eq!(ending.report.judge_error, Some(JudgeError::Overflow));
    assert!(ending.report.input.is_none());
    assert!(ending.report.bound_inputs.is_empty());
    assert!(ending.dispositions.is_empty());
    assert!(ending.refused.is_none());
    assert_eq!(owner.judge().stable_hash().unwrap(), before);
}

#[test]
fn projected_touch_reports_routed_ownership_and_ignored_contacts_without_fabrication() {
    let make = || {
        let (mut owner, queue) = runtime(
            engine(&[], 0, Box::new(ClosestCandidate), vec![]),
            &[1, 2],
            4,
            vec![],
        );
        owner
            .configure_touch_router(
                TouchRouter::new(
                    vec![TouchRegion {
                        device: DeviceSelector::Any,
                        physical: physical(),
                        game_control: GameControlId(1),
                        min: Position2 { x: 0.0, y: 0.0 },
                        max: Position2 { x: 10.0, y: 10.0 },
                    }],
                    4,
                )
                .unwrap(),
            )
            .unwrap();
        (owner, queue)
    };
    let (mut observed, _queue) = make();
    let (mut legacy, _legacy_queue) = make();
    let before = observed.judge().stable_hash().unwrap();
    let legacy_before = legacy.judge().stable_hash().unwrap();
    let observed_error = observed
        .process_input_at_report(
            touch(10, 1, TouchPhase::Down),
            Position2 {
                x: f32::NAN,
                y: 5.0,
            },
            &NoMapping,
            point(2, 0),
        )
        .unwrap_err();
    let legacy_error = legacy
        .process_input_at(
            touch(10, 1, TouchPhase::Down),
            Position2 {
                x: f32::NAN,
                y: 5.0,
            },
            &NoMapping,
            point(2, 0),
        )
        .unwrap_err();
    assert!(matches!(observed_error, RuntimeError::TouchRouting(_)));
    assert_eq!(observed_error, legacy_error);
    assert_eq!(observed.judge().stable_hash().unwrap(), before);
    assert_eq!(legacy.judge().stable_hash().unwrap(), legacy_before);
    assert_eq!(observed.touch_router().unwrap().active_contacts(), 0);
    assert_eq!(legacy.touch_router().unwrap().active_contacts(), 0);
    assert_eq!(
        observed.telemetry().counters(),
        legacy.telemetry().counters()
    );
    for (index, (phase, x, freshness)) in [
        (TouchPhase::Down, 5.0, Some(InputFreshness::FreshPress)),
        (TouchPhase::Down, 5.0, Some(InputFreshness::HeldDown)),
        (TouchPhase::Move, 50.0, Some(InputFreshness::Other)),
        (TouchPhase::Cancel, 50.0, Some(InputFreshness::Other)),
        (TouchPhase::Down, 50.0, None),
        (TouchPhase::Up, 5.0, None),
    ]
    .into_iter()
    .enumerate()
    {
        let input = touch(10 + index as i64, 1 + index as u64, phase);
        let projected = Position2 { x, y: 5.0 };
        let result = observed
            .process_input_at_report(input.clone(), projected, &NoMapping, point(2, 0))
            .unwrap();
        let old = legacy
            .process_input_at(input.clone(), projected, &NoMapping, point(2, 0))
            .unwrap();
        assert_report_eq(&result.report, &old);
        assert_eq!(result.report.input, Some(input.clone()));
        assert!(result.refused.is_none());
        if let Some(expected) = freshness {
            assert_eq!(result.report.bound_inputs, [bound(input, 1)]);
            assert_eq!(result.dispositions.len(), 1);
            assert_eq!(result.dispositions[0].freshness(), expected);
            assert_eq!(
                result.dispositions[0].unmatched_fresh_press(),
                expected == InputFreshness::FreshPress
            );
        } else {
            assert!(result.report.bound_inputs.is_empty());
            assert!(result.dispositions.is_empty());
        }
        assert_eq!(
            observed.judge().stable_hash().unwrap(),
            legacy.judge().stable_hash().unwrap()
        );
        assert_eq!(
            observed.telemetry().counters(),
            legacy.telemetry().counters()
        );
        assert_eq!(
            observed.touch_router().unwrap().active_contacts(),
            legacy.touch_router().unwrap().active_contacts()
        );
    }
}

#[test]
fn offset_audio_queue_failure_and_press_freshness_preserve_legacy_results() {
    let make = || {
        let sounds = vec![SoundBinding {
            object: ObjectId(1),
            stage: JudgeStage::Instant,
            sample: SampleId(11),
            voice: VoiceId(21),
            gain: -0.5,
        }];
        let (mut owner, queue) = runtime(
            engine(
                &[(1, 105, 1)],
                5,
                Box::new(ClosestCandidate),
                vec![marker(1, 105, 1)],
            ),
            &[1, 2],
            1,
            sounds,
        );
        owner
            .configure_input_sounds(
                InputSoundTimeline::new(
                    vec![InputSoundMarker {
                        control: GameControlId(2),
                        at: ts(100),
                        sample: SampleId(12),
                        voice: VoiceId(22),
                        gain: 0.25,
                    }],
                    1,
                )
                .unwrap(),
            )
            .unwrap();
        (owner, queue)
    };
    let (mut observed, mut queue) = make();
    let (mut legacy, mut legacy_queue) = make();
    for (index, (state, freshness)) in [
        (ButtonState::Down, InputFreshness::FreshPress),
        (ButtonState::Down, InputFreshness::HeldDown),
        (ButtonState::Repeat, InputFreshness::ExplicitRepeat),
        (ButtonState::Up, InputFreshness::Other),
        (ButtonState::Repeat, InputFreshness::ExplicitRepeat),
        (ButtonState::Down, InputFreshness::FreshPress),
    ]
    .into_iter()
    .enumerate()
    {
        let input = button(100 + index as i64, index as u64 + 1, state);
        let result = observed
            .process_input_report(input.clone(), &NoMapping, point(2, -99))
            .unwrap();
        let old = legacy
            .process_input(input, &NoMapping, point(2, -99))
            .unwrap();
        assert_report_eq(&result.report, &old);
        assert_eq!(result.dispositions.len(), 2);
        assert!(result.refused.is_none());
        assert!(result
            .dispositions
            .iter()
            .all(|facts| facts.freshness() == freshness));
        if index == 0 {
            assert_eq!(result.report.song_time, ts(100));
            assert_eq!(result.report.judge_events[0].at, ts(105));
            assert_eq!(
                result.report.judge_events[0].outcome,
                JudgeOutcome::Hit {
                    grade: JudgeGrade(7),
                    delta: Duration::ZERO
                }
            );
            assert_eq!(result.dispositions[0].input_result_count(), 1);
            assert_eq!(result.dispositions[0].input_hazard_count(), 1);
            assert!(result.dispositions[1].unmatched_fresh_press());
            assert_eq!(
                result.report.audio_commands,
                [AudioCommand::Play {
                    sample: SampleId(11),
                    voice: VoiceId(21),
                    at: ts(-99),
                    gain: -0.5
                }]
            );
            assert_eq!(result.report.audio_failures.len(), 1);
            assert_eq!(result.report.audio_failures[0].reason, QueuePushError::Full);
            assert_eq!(
                result.report.audio_failures[0].command,
                AudioCommand::Play {
                    sample: SampleId(12),
                    voice: VoiceId(22),
                    at: ts(-99),
                    gain: 0.25
                }
            );
        } else if index == 5 {
            assert_eq!(
                result.report.audio_commands,
                [AudioCommand::Play {
                    sample: SampleId(12),
                    voice: VoiceId(22),
                    at: ts(-99),
                    gain: 0.25
                }]
            );
        } else {
            assert!(result.report.audio_commands.is_empty());
        }
        assert_eq!(queue.try_pop(), legacy_queue.try_pop());
        assert_eq!(
            observed.judge().stable_hash().unwrap(),
            legacy.judge().stable_hash().unwrap()
        );
        assert_eq!(
            observed.telemetry().counters(),
            legacy.telemetry().counters()
        );
    }
}
