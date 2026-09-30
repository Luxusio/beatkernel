use std::sync::{Arc, Mutex};

use beatkernel::{
    chart::*,
    input::*,
    interaction::*,
    judge::*,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn dur(n: i64) -> Duration {
    Duration::from_nanos(n)
}
fn window(grade: u32, early: i64, late: i64) -> JudgeWindow {
    JudgeWindow {
        grade: JudgeGrade(grade),
        early: dur(early),
        late: dur(late),
    }
}
fn profile(offset: i64) -> JudgeProfile {
    JudgeProfile::new(vec![window(7, 5, 10), window(9, 20, 30)], dur(offset)).unwrap()
}
fn chart(objects: &[(u64, i64, Option<i64>, u32)]) -> CompiledChart {
    // At 60 BPM and one billion ticks/beat, one tick is exactly one ns.
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = objects
        .iter()
        .map(|&(id, start, end, interaction)| SourceObject {
            id: ObjectId(id),
            start: Beat::new(start).unwrap(),
            end: end.map(|n| Beat::new(n).unwrap()),
            interaction: InteractionId(interaction),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .collect();
    compile(&source).unwrap()
}
fn rule(id: u32, control: u32, hold: bool) -> Rule {
    Rule {
        interaction: InteractionId(id),
        control: GameControlId(control),
        evaluator: if hold {
            Box::new(HoldEvaluator)
        } else {
            Box::new(InstantEvaluator)
        },
    }
}
fn engine(objects: &[(u64, i64, Option<i64>, u32)]) -> JudgeEngine {
    JudgeEngine::new(
        chart(objects),
        vec![rule(1, 1, false), rule(2, 1, true)],
        profile(0),
    )
    .unwrap()
}
fn meta(source: u64) -> EventMeta {
    let mut m = EventMeta::new(
        DeviceId(source),
        ClockPoint {
            domain: ClockDomainId(42),
            timestamp: ts(90_000),
        },
        11,
    );
    m.native = Some(NativeEventMeta {
        backend: BackendId(8),
        code: Some(91),
        timestamp: Some(ClockPoint {
            domain: ClockDomainId(43),
            timestamp: ts(-123),
        }),
    });
    m.original_clock_point = Some(ClockPoint {
        domain: ClockDomainId(44),
        timestamp: ts(12),
    });
    m
}
fn button(source: u64, key: u16, control: u32, state: ButtonState) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(control),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: meta(source),
            control: PhysicalControlId::keyboard(key),
            state,
        }),
    }
}
fn down() -> GameInputEvent {
    button(1, 4, 1, ButtonState::Down)
}
fn owner(source: u64, key: u16, control: u32) -> InputOwner {
    InputOwner {
        source: DeviceId(source),
        physical: PhysicalControlId::keyboard(key),
        game_control: GameControlId(control),
    }
}
fn hit(
    id: u64,
    stage: JudgeStage,
    grade: u32,
    delta: i64,
    at: i64,
    input: EventMeta,
) -> JudgeEvent {
    JudgeEvent {
        object: ObjectId(id),
        stage,
        outcome: JudgeOutcome::Hit {
            grade: JudgeGrade(grade),
            delta: dur(delta),
        },
        at: ts(at),
        input: Some(input),
    }
}
fn miss(
    id: u64,
    stage: JudgeStage,
    reason: MissReason,
    at: i64,
    input: Option<EventMeta>,
) -> JudgeEvent {
    JudgeEvent {
        object: ObjectId(id),
        stage,
        outcome: JudgeOutcome::Miss { reason },
        at: ts(at),
        input,
    }
}

#[test]
fn invalid_profiles_are_typed_and_nested_windows_preserve_caller_order() {
    for windows in [
        vec![],
        vec![window(1, -1, 0)],
        vec![window(1, 0, -1)],
        vec![window(1, 4, 5), window(2, 3, 6)],
        vec![window(1, 4, 5), window(2, 5, 4)],
        vec![window(1, 0, 0), window(1, 5, 5)],
    ] {
        assert_eq!(
            JudgeProfile::new(windows, dur(0)),
            Err(JudgeError::InvalidProfile)
        );
    }
    let p = JudgeProfile::new(vec![window(99, 0, 0), window(3, 0, 0)], dur(-8)).unwrap();
    assert_eq!(p.grade(0), Some(JudgeGrade(99)));
    assert_eq!(p.grade(-1), None);
    assert_eq!(p.grade(1), None);
    assert_eq!(p.input_offset(), dur(-8));
}

#[test]
fn asymmetric_boundaries_emit_exact_grades_or_timeout() {
    for (time, expected) in [
        (
            479,
            vec![miss(
                1,
                JudgeStage::Instant,
                MissReason::HeadTimeout,
                531,
                None,
            )],
        ),
        (480, vec![hit(1, JudgeStage::Instant, 9, -20, 480, meta(1))]),
        (494, vec![hit(1, JudgeStage::Instant, 9, -6, 494, meta(1))]),
        (495, vec![hit(1, JudgeStage::Instant, 7, -5, 495, meta(1))]),
        (500, vec![hit(1, JudgeStage::Instant, 7, 0, 500, meta(1))]),
        (510, vec![hit(1, JudgeStage::Instant, 7, 10, 510, meta(1))]),
        (511, vec![hit(1, JudgeStage::Instant, 9, 11, 511, meta(1))]),
        (530, vec![hit(1, JudgeStage::Instant, 9, 30, 530, meta(1))]),
        (
            531,
            vec![miss(
                1,
                JudgeStage::Instant,
                MissReason::HeadTimeout,
                531,
                None,
            )],
        ),
    ] {
        let mut e = engine(&[(1, 500, None, 1)]);
        let mut actual = e.push_input(&down(), ts(time)).unwrap();
        actual.extend(e.advance_to(ts(531)).unwrap());
        assert_eq!(actual, expected, "input at {time}");
        assert!(e.advance_to(ts(900)).unwrap().is_empty());
        assert_eq!(e.state(ObjectId(1)), Some(InteractionState::Completed));
    }
}

#[test]
fn zero_window_and_inclusive_deadline_accept_both_call_orders() {
    for advance_first in [false, true] {
        for (time, expected) in [
            (499, None),
            (500, Some(hit(1, JudgeStage::Instant, 7, 0, 500, meta(1)))),
            (501, None),
        ] {
            let mut e = JudgeEngine::new(
                chart(&[(1, 500, None, 1)]),
                vec![rule(1, 1, false)],
                JudgeProfile::new(vec![window(7, 0, 0)], dur(0)).unwrap(),
            )
            .unwrap();
            let mut events = Vec::new();
            if advance_first {
                events.extend(e.advance_to(ts(time)).unwrap());
            }
            events.extend(e.push_input(&down(), ts(time)).unwrap());
            if !advance_first {
                events.extend(e.advance_to(ts(time)).unwrap());
            }
            events.extend(e.advance_to(ts(502)).unwrap());
            assert_eq!(
                events,
                vec![expected.unwrap_or_else(|| miss(
                    1,
                    JudgeStage::Instant,
                    MissReason::HeadTimeout,
                    if time == 501 { 501 } else { 502 },
                    None
                ))]
            );
        }
        let mut e = engine(&[(1, 500, None, 1)]);
        if advance_first {
            assert!(e.advance_to(ts(530)).unwrap().is_empty());
        }
        assert_eq!(
            e.push_input(&down(), ts(530)).unwrap(),
            vec![hit(1, JudgeStage::Instant, 9, 30, 530, meta(1))]
        );
        assert!(e.advance_to(ts(530)).unwrap().is_empty());
    }
}

#[test]
fn offset_applies_once_to_input_and_advance_and_overflow_is_atomic() {
    for (offset, mapped, effective, delta) in [(10, 495, 505, 5), (-10, 505, 495, -5)] {
        let mut e = JudgeEngine::new(
            chart(&[(1, 500, None, 1)]),
            vec![rule(1, 1, false)],
            profile(offset),
        )
        .unwrap();
        assert!(e.advance_to(ts(mapped)).unwrap().is_empty());
        assert_eq!(e.effective_song_time(), Some(ts(effective)));
        assert_eq!(
            e.push_input(&down(), ts(mapped)).unwrap(),
            vec![hit(1, JudgeStage::Instant, 7, delta, effective, meta(1))]
        );
    }
    for (offset, overflow, valid, effective) in [
        (1, Timestamp::MAX, ts(-1), 0),
        (-1, Timestamp::MIN, ts(1), 0),
    ] {
        let mut e = JudgeEngine::new(
            chart(&[(1, 0, None, 1)]),
            vec![rule(1, 1, false)],
            profile(offset),
        )
        .unwrap();
        assert_eq!(e.push_input(&down(), overflow), Err(JudgeError::Overflow));
        assert_eq!(e.advance_to(overflow), Err(JudgeError::Overflow));
        assert_eq!(e.effective_song_time(), None);
        assert_eq!(e.state(ObjectId(1)), Some(InteractionState::Pending));
        assert!(!e.is_held(owner(1, 4, 1)));
        assert_eq!(
            e.push_input(&down(), valid).unwrap(),
            vec![hit(1, JudgeStage::Instant, 7, 0, effective, meta(1))]
        );
    }
    let mut e = engine(&[]);
    assert!(e.advance_to(Timestamp::MIN).unwrap().is_empty());
    assert!(e.advance_to(Timestamp::MAX).unwrap().is_empty());
}

#[test]
fn offset_advance_expiry_uses_same_inclusive_coordinate_as_input() {
    for (offset, exact_mapped, after_mapped) in [(10, 520, 521), (-10, 540, 541)] {
        let mut e = JudgeEngine::new(
            chart(&[(1, 500, None, 1)]),
            vec![rule(1, 1, false)],
            profile(offset),
        )
        .unwrap();
        assert!(e.advance_to(ts(exact_mapped)).unwrap().is_empty());
        assert_eq!(e.effective_song_time(), Some(ts(530)));
        assert_eq!(
            e.advance_to(ts(after_mapped)).unwrap(),
            vec![miss(
                1,
                JudgeStage::Instant,
                MissReason::HeadTimeout,
                531,
                None
            ),]
        );
    }
}

#[test]
fn extreme_timestamps_use_wide_delta_and_deadline_arithmetic() {
    let huge = JudgeProfile::new(vec![window(5, i64::MAX, i64::MAX)], dur(0)).unwrap();
    let mut e = JudgeEngine::new(
        chart(&[(1, 0, None, 1)]),
        vec![rule(1, 1, false)],
        huge.clone(),
    )
    .unwrap();
    // MIN is one ns outside the widest early window; subtraction must not wrap.
    assert!(e.push_input(&down(), Timestamp::MIN).unwrap().is_empty());
    assert!(e
        .push_input(&button(1, 4, 1, ButtonState::Up), ts(i64::MIN + 1))
        .unwrap()
        .is_empty());
    assert_eq!(
        e.push_input(&down(), ts(i64::MIN + 1)).unwrap(),
        vec![hit(
            1,
            JudgeStage::Instant,
            5,
            -i64::MAX,
            i64::MIN + 1,
            meta(1)
        ),]
    );
    let mut e = JudgeEngine::new(chart(&[(1, 1, None, 1)]), vec![rule(1, 1, false)], huge).unwrap();
    // The late deadline exceeds MAX; it must neither wrap nor expire.
    assert!(e.advance_to(Timestamp::MAX).unwrap().is_empty());
    assert_eq!(e.state(ObjectId(1)), Some(InteractionState::Pending));
    assert_eq!(
        e.push_input(&down(), Timestamp::MAX).unwrap(),
        vec![hit(
            1,
            JudgeStage::Instant,
            5,
            i64::MAX - 1,
            i64::MAX,
            meta(1)
        ),]
    );
}

#[test]
fn chronology_rejection_preserves_pending_active_and_held_state() {
    let mut e = engine(&[(1, 0, Some(100), 2), (2, 50, None, 1)]);
    assert!(e.advance_to(ts(-20)).unwrap().is_empty());
    assert_eq!(
        e.push_input(&down(), ts(0)).unwrap(),
        vec![hit(1, JudgeStage::HoldHead, 7, 0, 0, meta(1))]
    );
    let up = button(1, 4, 1, ButtonState::Up);
    assert_eq!(
        e.push_input(&up, ts(-1)),
        Err(JudgeError::NonMonotonicSongTime)
    );
    assert_eq!(e.advance_to(ts(-1)), Err(JudgeError::NonMonotonicSongTime));
    assert_eq!(e.effective_song_time(), Some(ts(0)));
    assert!(e.is_held(owner(1, 4, 1)));
    assert_eq!(e.state(ObjectId(1)), Some(InteractionState::Active));
    assert_eq!(e.state(ObjectId(2)), Some(InteractionState::Pending));
    assert_eq!(
        e.push_input(&up, ts(100)).unwrap(),
        vec![
            miss(2, JudgeStage::Instant, MissReason::HeadTimeout, 100, None),
            hit(1, JudgeStage::HoldTail, 7, 0, 100, meta(1))
        ]
    );
}

struct Pick(ObjectId);
impl CandidateResolver for Pick {
    fn select(&self, _: &[Candidate]) -> Option<ObjectId> {
        Some(self.0)
    }
}
struct Decline;
impl CandidateResolver for Decline {
    fn select(&self, _: &[Candidate]) -> Option<ObjectId> {
        None
    }
}

#[test]
fn closest_ties_earliest_and_custom_select_one_exact_object() {
    for (resolver, time, selected, delta) in [
        (
            Box::new(ClosestCandidate) as Box<dyn CandidateResolver>,
            515,
            2,
            -5,
        ),
        (Box::new(ClosestCandidate), 510, 1, 10),
        (Box::new(EarliestCandidate), 515, 1, 15),
        (Box::new(Pick(ObjectId(3))), 515, 3, -5),
    ] {
        let mut e = JudgeEngine::with_policies(
            chart(&[(3, 520, None, 1), (2, 520, None, 1), (1, 500, None, 1)]),
            vec![rule(1, 1, false)],
            profile(0),
            resolver,
            Box::new(WindowJudgePolicy),
        )
        .unwrap();
        assert_eq!(
            e.push_input(&down(), ts(time)).unwrap(),
            vec![hit(
                selected,
                JudgeStage::Instant,
                if delta == 15 { 9 } else { 7 },
                delta,
                time,
                meta(1)
            )]
        );
    }
    let mut e = JudgeEngine::with_policies(
        chart(&[(1, 500, None, 1)]),
        vec![rule(1, 1, false)],
        profile(0),
        Box::new(Decline),
        Box::new(WindowJudgePolicy),
    )
    .unwrap();
    assert!(e.push_input(&down(), ts(500)).unwrap().is_empty());
    assert_eq!(
        e.advance_to(ts(531)).unwrap(),
        vec![miss(
            1,
            JudgeStage::Instant,
            MissReason::HeadTimeout,
            531,
            None
        )]
    );
}

#[test]
fn invalid_resolver_selection_does_not_expire_deadlines_or_capture_press() {
    for selected in [ObjectId(99), ObjectId(1)] {
        let mut e = JudgeEngine::with_policies(
            chart(&[(1, 0, None, 1), (2, 500, None, 1)]),
            vec![rule(1, 1, false)],
            profile(0),
            Box::new(Pick(selected)),
            Box::new(WindowJudgePolicy),
        )
        .unwrap();
        assert_eq!(
            e.push_input(&down(), ts(500)),
            Err(JudgeError::InvalidCandidate { object: selected })
        );
        assert_eq!(e.effective_song_time(), None);
        assert!(!e.is_held(owner(1, 4, 1)));
        assert_eq!(e.state(ObjectId(1)), Some(InteractionState::Pending));
        assert_eq!(e.state(ObjectId(2)), Some(InteractionState::Pending));
        assert_eq!(
            e.advance_to(ts(31)).unwrap(),
            vec![miss(
                1,
                JudgeStage::Instant,
                MissReason::HeadTimeout,
                31,
                None
            )]
        );
    }
}

#[test]
fn repeat_duplicate_down_unmatched_up_and_independent_physical_sources() {
    let mut e = engine(&[(1, 500, None, 1), (2, 500, None, 1), (3, 500, None, 1)]);
    assert!(e
        .push_input(&button(1, 4, 1, ButtonState::Repeat), ts(500))
        .unwrap()
        .is_empty());
    assert!(!e.is_held(owner(1, 4, 1)));
    assert!(e
        .push_input(&button(8, 4, 1, ButtonState::Up), ts(500))
        .unwrap()
        .is_empty());
    assert_eq!(
        e.push_input(&down(), ts(500)).unwrap(),
        vec![hit(1, JudgeStage::Instant, 7, 0, 500, meta(1))]
    );
    for state in [ButtonState::Down, ButtonState::Repeat] {
        assert!(e
            .push_input(&button(1, 4, 1, state), ts(500))
            .unwrap()
            .is_empty());
    }
    assert_eq!(
        e.push_input(&button(2, 4, 1, ButtonState::Down), ts(500))
            .unwrap(),
        vec![hit(2, JudgeStage::Instant, 7, 0, 500, meta(2))]
    );
    assert_eq!(
        e.push_input(&button(1, 5, 1, ButtonState::Down), ts(500))
            .unwrap(),
        vec![hit(3, JudgeStage::Instant, 7, 0, 500, meta(1))]
    );
    for o in [owner(1, 4, 1), owner(2, 4, 1), owner(1, 5, 1)] {
        assert!(e.is_held(o));
    }
}

#[test]
fn hold_release_boundaries_early_break_and_late_timeout_are_terminal() {
    for (release, expected) in [
        (
            979,
            miss(
                1,
                JudgeStage::HoldTail,
                MissReason::EarlyRelease,
                979,
                Some(meta(1)),
            ),
        ),
        (980, hit(1, JudgeStage::HoldTail, 9, -20, 980, meta(1))),
        (995, hit(1, JudgeStage::HoldTail, 7, -5, 995, meta(1))),
        (1000, hit(1, JudgeStage::HoldTail, 7, 0, 1000, meta(1))),
        (1010, hit(1, JudgeStage::HoldTail, 7, 10, 1010, meta(1))),
        (1030, hit(1, JudgeStage::HoldTail, 9, 30, 1030, meta(1))),
        (
            1031,
            miss(1, JudgeStage::HoldTail, MissReason::TailTimeout, 1031, None),
        ),
    ] {
        let mut e = engine(&[(1, 500, Some(1000), 2)]);
        assert_eq!(
            e.push_input(&down(), ts(500)).unwrap(),
            vec![hit(1, JudgeStage::HoldHead, 7, 0, 500, meta(1))]
        );
        assert_eq!(
            e.push_input(&button(1, 4, 1, ButtonState::Up), ts(release))
                .unwrap(),
            vec![expected]
        );
        assert_eq!(e.state(ObjectId(1)), Some(InteractionState::Completed));
        assert!(e.push_input(&down(), ts(release)).unwrap().is_empty());
        assert!(e.advance_to(ts(2000)).unwrap().is_empty());
    }
}

#[test]
fn hold_head_miss_and_held_tail_timeout_do_not_fabricate_input() {
    let mut e = engine(&[(1, 500, Some(1000), 2)]);
    assert!(e.advance_to(ts(529)).unwrap().is_empty());
    assert!(e.advance_to(ts(530)).unwrap().is_empty());
    assert_eq!(
        e.advance_to(ts(531)).unwrap(),
        vec![miss(
            1,
            JudgeStage::HoldHead,
            MissReason::HeadTimeout,
            531,
            None
        )]
    );
    assert!(e.advance_to(ts(2000)).unwrap().is_empty());
    let mut e = engine(&[(1, 500, Some(1000), 2)]);
    e.push_input(&down(), ts(500)).unwrap();
    for time in [500, 600, 980, 1000, 1030, 1030] {
        assert!(e.advance_to(ts(time)).unwrap().is_empty());
        assert_eq!(e.state(ObjectId(1)), Some(InteractionState::Active));
    }
    assert_eq!(
        e.advance_to(ts(1031)).unwrap(),
        vec![miss(
            1,
            JudgeStage::HoldTail,
            MissReason::TailTimeout,
            1031,
            None
        )]
    );
    assert!(e.is_held(owner(1, 4, 1)));
    assert!(e.advance_to(ts(2000)).unwrap().is_empty());
}

#[test]
fn wrong_device_key_or_destination_cannot_release_hold_and_new_press_routes_separately() {
    let mut e = engine(&[(1, 500, Some(1000), 2), (2, 700, None, 1)]);
    e.push_input(&down(), ts(500)).unwrap();
    for (source, key, control) in [(2, 4, 1), (1, 5, 1), (1, 4, 2)] {
        assert!(e
            .push_input(&button(source, key, control, ButtonState::Up), ts(600))
            .unwrap()
            .is_empty());
        assert!(e.is_held(owner(1, 4, 1)));
        assert_eq!(e.state(ObjectId(1)), Some(InteractionState::Active));
    }
    assert_eq!(
        e.push_input(&button(1, 5, 1, ButtonState::Down), ts(700))
            .unwrap(),
        vec![hit(2, JudgeStage::Instant, 7, 0, 700, meta(1))]
    );
    assert_eq!(
        e.push_input(&button(1, 4, 1, ButtonState::Up), ts(1000))
            .unwrap(),
        vec![hit(1, JudgeStage::HoldTail, 7, 0, 1000, meta(1))]
    );
    assert!(e.is_held(owner(1, 5, 1)));
}

#[test]
fn simultaneous_timeouts_sort_by_deadline_then_identity_before_input_results() {
    let mut e = engine(&[
        (8, 100, None, 1),
        (4, 100, None, 1),
        (1, 150, None, 1),
        (9, 200, None, 1),
        (3, 50, Some(100), 2),
    ]);
    assert_eq!(
        e.push_input(&down(), ts(50)).unwrap(),
        vec![hit(3, JudgeStage::HoldHead, 7, 0, 50, meta(1))]
    );
    assert_eq!(
        e.push_input(&button(2, 4, 1, ButtonState::Down), ts(200))
            .unwrap(),
        vec![
            miss(3, JudgeStage::HoldTail, MissReason::TailTimeout, 200, None),
            miss(4, JudgeStage::Instant, MissReason::HeadTimeout, 200, None),
            miss(8, JudgeStage::Instant, MissReason::HeadTimeout, 200, None),
            miss(1, JudgeStage::Instant, MissReason::HeadTimeout, 200, None),
            hit(9, JudgeStage::Instant, 7, 0, 200, meta(2)),
        ]
    );
}

#[test]
fn rule_and_object_validation_returns_configuration_errors() {
    for (objects, rules, expected) in [
        (
            vec![(1, 500, None, 99)],
            vec![rule(1, 1, false)],
            JudgeError::UnknownInteraction {
                id: InteractionId(99),
            },
        ),
        (
            vec![],
            vec![rule(1, 1, false), rule(1, 2, true)],
            JudgeError::DuplicateRule {
                id: InteractionId(1),
            },
        ),
        (
            vec![(1, 500, Some(600), 1)],
            vec![rule(1, 1, false)],
            JudgeError::InvalidObjectRange {
                object: ObjectId(1),
            },
        ),
        (
            vec![(1, 500, None, 2)],
            vec![rule(2, 1, true)],
            JudgeError::InvalidObjectRange {
                object: ObjectId(1),
            },
        ),
        (
            vec![(1, 500, Some(500), 2)],
            vec![rule(2, 1, true)],
            JudgeError::InvalidObjectRange {
                object: ObjectId(1),
            },
        ),
    ] {
        assert!(
            matches!(JudgeEngine::new(chart(&objects), rules, profile(0)), Err(error) if error == expected)
        );
    }
    // A reversed range cannot reach the judge via the compiler. Validate the public evaluator directly.
    let mut object = chart(&[(1, 500, Some(600), 2)]).objects()[0].clone();
    object.time.end = Some(ts(499));
    assert_eq!(
        HoldEvaluator.validate(&object, &profile(0)),
        Err(JudgeError::InvalidObjectRange {
            object: ObjectId(1)
        })
    );
}

struct RejectLate;
impl JudgePolicy for RejectLate {
    fn grade(&self, delta: i128, _: &JudgeProfile) -> Option<JudgeGrade> {
        (delta == 0).then_some(JudgeGrade(123))
    }
}
#[test]
fn replacement_grading_rejects_head_until_timeout_and_terminates_rejected_tail() {
    let build = || {
        JudgeEngine::with_policies(
            chart(&[(1, 500, Some(1000), 2)]),
            vec![rule(2, 1, true)],
            profile(0),
            Box::new(ClosestCandidate),
            Box::new(RejectLate),
        )
        .unwrap()
    };
    let mut e = build();
    assert!(e.push_input(&down(), ts(505)).unwrap().is_empty());
    assert_eq!(e.state(ObjectId(1)), Some(InteractionState::Pending));
    assert_eq!(
        e.advance_to(ts(531)).unwrap(),
        vec![miss(
            1,
            JudgeStage::HoldHead,
            MissReason::HeadTimeout,
            531,
            None
        )]
    );
    let mut e = build();
    assert_eq!(
        e.push_input(&down(), ts(500)).unwrap(),
        vec![hit(1, JudgeStage::HoldHead, 123, 0, 500, meta(1))]
    );
    assert_eq!(
        e.push_input(&button(1, 4, 1, ButtonState::Up), ts(1005))
            .unwrap(),
        vec![miss(
            1,
            JudgeStage::HoldTail,
            MissReason::RejectedInput,
            1005,
            Some(meta(1))
        )]
    );
    assert!(e.advance_to(ts(2000)).unwrap().is_empty());
}

type Samples = Arc<Mutex<Vec<GameInputEvent>>>;
struct AxisEvaluator(Samples);
struct AxisInteraction {
    samples: Samples,
    state: InteractionState,
    control: GameControlId,
}
impl InteractionEvaluator for AxisEvaluator {
    fn validate(&self, _: &TimedObject, _: &JudgeProfile) -> Result<(), JudgeError> {
        Ok(())
    }
    fn begin(&self, _: &TimedObject, context: &BeginContext<'_>) -> Box<dyn ActiveInteraction> {
        Box::new(AxisInteraction {
            samples: self.0.clone(),
            state: InteractionState::Pending,
            control: context.control,
        })
    }
}
impl ActiveInteraction for AxisInteraction {
    fn state(&self) -> InteractionState {
        self.state
    }
    fn accepts_input(&self, event: &GameInputEvent, _: &InteractionContext<'_>) -> bool {
        self.state != InteractionState::Completed
            && event.game_control == self.control
            && matches!(event.physical, PhysicalInputEvent::Axis(_))
    }
    fn on_input(
        &mut self,
        event: &GameInputEvent,
        _: &InteractionContext<'_>,
    ) -> InteractionOutput {
        self.samples.lock().unwrap().push(event.clone());
        self.state = InteractionState::Completed;
        InteractionOutput {
            results: vec![InteractionResult {
                stage: JudgeStage::Custom(77),
                outcome: JudgeOutcome::Hit {
                    grade: JudgeGrade(88),
                    delta: dur(0),
                },
            }],
        }
    }
    fn advance_to(&mut self, _: Timestamp, _: &InteractionContext<'_>) -> InteractionOutput {
        InteractionOutput::default()
    }
    fn deadline(&self, _: &JudgeProfile) -> Option<i128> {
        None
    }
}
#[test]
fn custom_axis_consumes_retained_typed_sample_while_builtins_ignore_axis_and_touch() {
    let axis = GameInputEvent {
        game_control: GameControlId(1),
        physical: PhysicalInputEvent::Axis(AxisEvent {
            meta: meta(1),
            control: PhysicalControlId::keyboard(7),
            value: -0.0,
            mode: AxisMode::Relative,
        }),
    };
    let touch = GameInputEvent {
        game_control: GameControlId(1),
        physical: PhysicalInputEvent::Touch(TouchEvent {
            meta: meta(1),
            control: PhysicalControlId::keyboard(7),
            contact: ContactId(9),
            phase: TouchPhase::Down,
            position: Position2 { x: 1.0, y: -0.0 },
            pressure: Some(0.7),
        }),
    };
    let mut builtin = engine(&[(1, 500, None, 1), (2, 500, Some(1000), 2)]);
    for event in [&axis, &touch] {
        assert!(builtin.push_input(event, ts(500)).unwrap().is_empty());
    }
    assert_eq!(builtin.state(ObjectId(1)), Some(InteractionState::Pending));
    assert_eq!(builtin.state(ObjectId(2)), Some(InteractionState::Pending));
    let samples = Arc::new(Mutex::new(Vec::new()));
    let mut custom = JudgeEngine::new(
        chart(&[(1, 500, None, 77)]),
        vec![Rule {
            interaction: InteractionId(77),
            control: GameControlId(1),
            evaluator: Box::new(AxisEvaluator(samples.clone())),
        }],
        profile(0),
    )
    .unwrap();
    assert_eq!(
        custom.push_input(&axis, ts(500)).unwrap(),
        vec![hit(1, JudgeStage::Custom(77), 88, 0, 500, meta(1))]
    );
    let observed = samples.lock().unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].game_control, GameControlId(1));
    let PhysicalInputEvent::Axis(sample) = &observed[0].physical else {
        panic!("axis payload flattened")
    };
    assert_eq!(sample.meta, meta(1));
    assert_eq!(sample.control, PhysicalControlId::keyboard(7));
    assert_eq!(sample.mode, AxisMode::Relative);
    assert_eq!(sample.value.to_bits(), 0x8000_0000);
}

struct ObservedCustomEvaluator {
    predicates: Samples,
    samples: Samples,
    accepts_up: bool,
}
struct ObservedCustomInteraction {
    predicates: Samples,
    inner: AxisInteraction,
    accepts_up: bool,
}
impl InteractionEvaluator for ObservedCustomEvaluator {
    fn validate(&self, _: &TimedObject, _: &JudgeProfile) -> Result<(), JudgeError> {
        Ok(())
    }
    fn begin(&self, _: &TimedObject, context: &BeginContext<'_>) -> Box<dyn ActiveInteraction> {
        Box::new(ObservedCustomInteraction {
            predicates: self.predicates.clone(),
            inner: AxisInteraction {
                samples: self.samples.clone(),
                state: InteractionState::Pending,
                control: context.control,
            },
            accepts_up: self.accepts_up,
        })
    }
}
impl ActiveInteraction for ObservedCustomInteraction {
    fn state(&self) -> InteractionState {
        self.inner.state()
    }
    fn accepts_input(&self, event: &GameInputEvent, context: &InteractionContext<'_>) -> bool {
        self.predicates.lock().unwrap().push(event.clone());
        if self.accepts_up {
            self.inner.state == InteractionState::Pending
                && event.game_control == self.inner.control
                && matches!(&event.physical, PhysicalInputEvent::Button(button) if button.state == ButtonState::Up)
        } else {
            self.inner.accepts_input(event, context)
        }
    }
    fn on_input(
        &mut self,
        event: &GameInputEvent,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        self.inner.on_input(event, context)
    }
    fn advance_to(
        &mut self,
        time: Timestamp,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        self.inner.advance_to(time, context)
    }
    fn deadline(&self, profile: &JudgeProfile) -> Option<i128> {
        self.inner.deadline(profile)
    }
}
struct ObservedResolver(Arc<Mutex<Vec<Vec<Candidate>>>>);
impl CandidateResolver for ObservedResolver {
    fn select(&self, candidates: &[Candidate]) -> Option<ObjectId> {
        self.0.lock().unwrap().push(candidates.to_vec());
        ClosestCandidate.select(candidates)
    }
}
fn assert_custom_start(event: GameInputEvent, at: i64, accepts_up: bool) {
    let predicates = Arc::new(Mutex::new(Vec::new()));
    let samples = Arc::new(Mutex::new(Vec::new()));
    let selections = Arc::new(Mutex::new(Vec::new()));
    let mut custom = JudgeEngine::with_policies(
        chart(&[(1, 500, None, 77)]),
        vec![Rule {
            interaction: InteractionId(77),
            control: GameControlId(1),
            evaluator: Box::new(ObservedCustomEvaluator {
                predicates: predicates.clone(),
                samples: samples.clone(),
                accepts_up,
            }),
        }],
        profile(0),
        Box::new(ObservedResolver(selections.clone())),
        Box::new(WindowJudgePolicy),
    )
    .unwrap();
    assert_eq!(
        custom.push_input(&event, ts(at)).unwrap(),
        vec![hit(1, JudgeStage::Custom(77), 88, 0, at, meta(1))]
    );
    assert_eq!(custom.state(ObjectId(1)), Some(InteractionState::Completed));
    assert_eq!(
        *selections.lock().unwrap(),
        vec![vec![Candidate {
            object: ObjectId(1),
            target: ts(500),
            delta: i128::from(at - 500),
        }]]
    );
    // Predicate and callback receive the full original bound event. The axis
    // signed-zero bits are checked separately because float equality hides them.
    let observed_predicates = predicates.lock().unwrap().clone();
    assert!(!observed_predicates.is_empty());
    assert!(observed_predicates.iter().all(|sample| sample == &event));
    assert_eq!(*samples.lock().unwrap(), vec![event.clone()]);
    for sample in observed_predicates
        .iter()
        .chain(samples.lock().unwrap().iter())
    {
        if let PhysicalInputEvent::Axis(axis) = &sample.physical {
            assert_eq!(axis.value.to_bits(), 0x8000_0000);
        }
    }
    let predicate_count = observed_predicates.len();
    assert!(custom.push_input(&event, ts(at)).unwrap().is_empty());
    assert!(custom.advance_to(ts(900)).unwrap().is_empty());
    assert_eq!(samples.lock().unwrap().len(), 1);
    assert_eq!(predicates.lock().unwrap().len(), predicate_count);
    assert_eq!(selections.lock().unwrap().len(), 1);
}
#[test]
fn custom_pending_button_up_starts_at_exact_target_with_original_payload() {
    assert_custom_start(button(1, 4, 1, ButtonState::Up), 500, true);
}
#[test]
fn custom_pending_axis_without_deadline_starts_outside_builtin_profile_window() {
    for at in [479, 531] {
        assert_custom_start(
            GameInputEvent {
                game_control: GameControlId(1),
                physical: PhysicalInputEvent::Axis(AxisEvent {
                    meta: meta(1),
                    control: PhysicalControlId::keyboard(7),
                    value: -0.0,
                    mode: AxisMode::Relative,
                }),
            },
            at,
            false,
        );
    }
}

struct DeadlineAxisEvaluator {
    deadline: i64,
    samples: Samples,
}
struct DeadlineAxisInteraction {
    deadline: i64,
    inner: AxisInteraction,
}
impl InteractionEvaluator for DeadlineAxisEvaluator {
    fn validate(&self, _: &TimedObject, _: &JudgeProfile) -> Result<(), JudgeError> {
        Ok(())
    }
    fn begin(&self, _: &TimedObject, context: &BeginContext<'_>) -> Box<dyn ActiveInteraction> {
        Box::new(DeadlineAxisInteraction {
            deadline: self.deadline,
            inner: AxisInteraction {
                samples: self.samples.clone(),
                state: InteractionState::Pending,
                control: context.control,
            },
        })
    }
}
impl ActiveInteraction for DeadlineAxisInteraction {
    fn state(&self) -> InteractionState {
        self.inner.state()
    }
    fn accepts_input(&self, event: &GameInputEvent, context: &InteractionContext<'_>) -> bool {
        // Acceptance intentionally depends on the typed payload alone; the
        // declared deadline is the engine's scheduling and eligibility boundary.
        self.inner.accepts_input(event, context)
    }
    fn on_input(
        &mut self,
        event: &GameInputEvent,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        self.inner.on_input(event, context)
    }
    fn advance_to(&mut self, time: Timestamp, _: &InteractionContext<'_>) -> InteractionOutput {
        if self.inner.state == InteractionState::Pending && time > ts(self.deadline) {
            self.inner.state = InteractionState::Completed;
            InteractionOutput {
                results: vec![InteractionResult {
                    stage: JudgeStage::Custom(77),
                    outcome: JudgeOutcome::Miss {
                        reason: MissReason::HeadTimeout,
                    },
                }],
            }
        } else {
            InteractionOutput::default()
        }
    }
    fn deadline(&self, _: &JudgeProfile) -> Option<i128> {
        (self.inner.state != InteractionState::Completed).then_some(i128::from(self.deadline))
    }
}
fn deadline_axis_engine(samples: Samples, resolver: Box<dyn CandidateResolver>) -> JudgeEngine {
    JudgeEngine::with_policies(
        chart(&[(1, 100, None, 77), (2, 200, None, 78)]),
        vec![(77, 100), (78, 300)]
            .into_iter()
            .map(|(interaction, deadline)| Rule {
                interaction: InteractionId(interaction),
                control: GameControlId(1),
                evaluator: Box::new(DeadlineAxisEvaluator {
                    deadline,
                    samples: samples.clone(),
                }),
            })
            .collect(),
        profile(0),
        resolver,
        Box::new(WindowJudgePolicy),
    )
    .unwrap()
}
fn deadline_axis_input() -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(1),
        physical: PhysicalInputEvent::Axis(AxisEvent {
            meta: meta(1),
            control: PhysicalControlId::keyboard(7),
            value: -0.0,
            mode: AxisMode::Relative,
        }),
    }
}

#[test]
fn custom_expired_candidate_cannot_steal_live_input_in_either_call_order() {
    let input = deadline_axis_input();
    for advance_first in [false, true] {
        let samples = Arc::new(Mutex::new(Vec::new()));
        let selections = Arc::new(Mutex::new(Vec::new()));
        let mut e = deadline_axis_engine(
            samples.clone(),
            Box::new(ObservedResolver(selections.clone())),
        );
        let mut events = Vec::new();
        if advance_first {
            events.extend(e.advance_to(ts(150)).unwrap());
        }
        events.extend(e.push_input(&input, ts(150)).unwrap());
        if !advance_first {
            events.extend(e.advance_to(ts(150)).unwrap());
        }
        assert_eq!(
            events,
            vec![
                miss(
                    1,
                    JudgeStage::Custom(77),
                    MissReason::HeadTimeout,
                    150,
                    None
                ),
                hit(2, JudgeStage::Custom(77), 88, 0, 150, meta(1)),
            ],
            "advance_first={advance_first}"
        );
        assert_eq!(
            *selections.lock().unwrap(),
            vec![vec![Candidate {
                object: ObjectId(2),
                target: ts(200),
                delta: -50,
            }]]
        );
        for id in [ObjectId(1), ObjectId(2)] {
            assert_eq!(e.state(id), Some(InteractionState::Completed));
        }
        let captured = samples.lock().unwrap().clone();
        assert_eq!(captured, vec![input.clone()]);
        let PhysicalInputEvent::Axis(sample) = &captured[0].physical else {
            panic!("axis payload flattened")
        };
        assert_eq!(sample.value.to_bits(), 0x8000_0000);
        assert!(e.push_input(&input, ts(150)).unwrap().is_empty());
        assert!(e.advance_to(ts(301)).unwrap().is_empty());
    }
}

#[test]
fn custom_declared_deadline_equality_remains_eligible_in_either_call_order() {
    let input = deadline_axis_input();
    for advance_first in [false, true] {
        let samples = Arc::new(Mutex::new(Vec::new()));
        let mut e = deadline_axis_engine(samples.clone(), Box::new(ClosestCandidate));
        let mut events = Vec::new();
        if advance_first {
            events.extend(e.advance_to(ts(100)).unwrap());
        }
        events.extend(e.push_input(&input, ts(100)).unwrap());
        if !advance_first {
            events.extend(e.advance_to(ts(100)).unwrap());
        }
        assert_eq!(
            events,
            vec![hit(1, JudgeStage::Custom(77), 88, 0, 100, meta(1))]
        );
        assert_eq!(e.state(ObjectId(1)), Some(InteractionState::Completed));
        assert_eq!(e.state(ObjectId(2)), Some(InteractionState::Pending));
        assert_eq!(*samples.lock().unwrap(), vec![input.clone()]);
        assert_eq!(
            e.advance_to(ts(301)).unwrap(),
            vec![miss(
                2,
                JudgeStage::Custom(77),
                MissReason::HeadTimeout,
                301,
                None
            )]
        );
    }
}

#[test]
fn custom_past_deadline_resolver_selection_is_rejected_before_expiry_or_chronology() {
    let samples = Arc::new(Mutex::new(Vec::new()));
    let mut e = deadline_axis_engine(samples.clone(), Box::new(Pick(ObjectId(1))));
    assert_eq!(
        e.push_input(&deadline_axis_input(), ts(150)),
        Err(JudgeError::InvalidCandidate {
            object: ObjectId(1)
        })
    );
    assert_eq!(e.effective_song_time(), None);
    for id in [ObjectId(1), ObjectId(2)] {
        assert_eq!(e.state(id), Some(InteractionState::Pending));
    }
    assert!(samples.lock().unwrap().is_empty());
    assert_eq!(
        e.advance_to(ts(101)).unwrap(),
        vec![miss(
            1,
            JudgeStage::Custom(77),
            MissReason::HeadTimeout,
            101,
            None
        )]
    );
    assert_eq!(e.state(ObjectId(2)), Some(InteractionState::Pending));
    assert_eq!(
        e.advance_to(ts(301)).unwrap(),
        vec![miss(
            2,
            JudgeStage::Custom(77),
            MissReason::HeadTimeout,
            301,
            None
        )]
    );
}

struct UnusedMapper;
impl ClockMapper for UnusedMapper {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        panic!("same-domain input must bypass mapping")
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn pipeline() -> Vec<JudgeEvent> {
    let compiled = chart(&[
        (1, 500, None, 1),
        (2, 500, Some(1000), 2),
        (3, 700, None, 3),
        (4, 900, None, 4),
    ]);
    assert_eq!(
        compiled.objects()[1].time,
        TimeRange {
            start: ts(500),
            end: Some(ts(1000))
        }
    );
    let mut judge = JudgeEngine::new(
        compiled,
        vec![
            rule(1, 1, false),
            rule(2, 2, true),
            rule(3, 3, false),
            rule(4, 4, false),
        ],
        profile(0),
    )
    .unwrap();
    let mut backend = VirtualInputBackend::new(ClockDomainId(42));
    backend
        .register_device(DeviceDescriptor {
            runtime_id: DeviceId(1),
            vendor_id: None,
            product_id: None,
            serial: None,
            name: Some("synthetic keyboard".into()),
            transport: DeviceTransport::Virtual,
            capabilities: DeviceCapabilities {
                button: true,
                ..Default::default()
            },
        })
        .unwrap();
    let bindings = BindingMap::from_bindings([
        Binding {
            device: DeviceSelector::Exact(DeviceId(1)),
            physical: PhysicalControlId::keyboard(4),
            game_control: GameControlId(1),
        },
        Binding {
            device: DeviceSelector::Exact(DeviceId(1)),
            physical: PhysicalControlId::keyboard(4),
            game_control: GameControlId(2),
        },
        Binding {
            device: DeviceSelector::Exact(DeviceId(1)),
            physical: PhysicalControlId::keyboard(5),
            game_control: GameControlId(3),
        },
        Binding {
            device: DeviceSelector::Exact(DeviceId(1)),
            physical: PhysicalControlId::keyboard(6),
            game_control: GameControlId(4),
        },
    ])
    .unwrap();
    let transport = Transport::new(ts(10_000), ts(0), Rate::NORMAL);
    for (sequence, host, key, state) in [
        (11, 10_500, 4, ButtonState::Down),
        (11, 10_700, 5, ButtonState::Down),
        (12, 11_000, 4, ButtonState::Up),
    ] {
        let mut input = button(1, key, 1, state).physical;
        input.meta_mut().timestamp = ts(host);
        input.meta_mut().sequence = sequence;
        backend.push(input, &UnusedMapper).unwrap();
    }
    let mut output = Vec::new();
    for canonical in backend.drain_events() {
        let mapped = transport.position_at(canonical.meta().timestamp).unwrap();
        for bound in bindings.map(&canonical) {
            output.extend(judge.push_input(&bound, mapped).unwrap());
        }
    }
    output.extend(judge.advance_to(ts(1100)).unwrap());
    output
}
#[test]
fn compile_virtual_device_bindings_historical_transport_four_control_pipeline_is_literal_and_repeatable(
) {
    let mut first = meta(1);
    first.timestamp = ts(10_500);
    let mut second = meta(1);
    second.timestamp = ts(10_700);
    let mut release = meta(1);
    release.timestamp = ts(11_000);
    release.sequence = 12;
    let expected = vec![
        hit(1, JudgeStage::Instant, 7, 0, 500, first),
        hit(2, JudgeStage::HoldHead, 7, 0, 500, first),
        hit(3, JudgeStage::Instant, 7, 0, 700, second),
        miss(4, JudgeStage::Instant, MissReason::HeadTimeout, 1000, None),
        hit(2, JudgeStage::HoldTail, 7, 0, 1000, release),
    ];
    assert_eq!(pipeline(), expected);
    assert_eq!(pipeline(), expected);
}
