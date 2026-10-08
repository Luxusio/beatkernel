//! Accepted-input facts tested against public transitions and independent fixtures.
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

use beatkernel::{
    chart::*,
    input::*,
    interaction::*,
    judge::*,
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn dur(n: i64) -> Duration {
    Duration::from_nanos(n)
}
fn profile(offset: i64) -> JudgeProfile {
    JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(7),
            early: dur(10),
            late: dur(10),
        }],
        dur(offset),
    )
    .unwrap()
}
fn chart(objects: &[(u64, i64, Option<i64>, u32)]) -> CompiledChart {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = objects
        .iter()
        .map(|&(id, at, end, interaction)| SourceObject {
            id: ObjectId(id),
            start: Beat::new(at).unwrap(),
            end: end.map(|n| Beat::new(n).unwrap()),
            interaction: InteractionId(interaction),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .collect();
    source.compile().unwrap()
}
fn rules(contacts: bool) -> Vec<Rule> {
    vec![
        Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: if contacts {
                Box::new(PressInstantEvaluator)
            } else {
                Box::new(InstantEvaluator)
            },
        },
        Rule {
            interaction: InteractionId(2),
            control: GameControlId(1),
            evaluator: if contacts {
                Box::new(PressHoldEvaluator)
            } else {
                Box::new(HoldEvaluator)
            },
        },
    ]
}
fn engine(objects: &[(u64, i64, Option<i64>, u32)], contacts: bool, offset: i64) -> JudgeEngine {
    if contacts {
        JudgeEngine::new_with_contacts(chart(objects), rules(true), profile(offset)).unwrap()
    } else {
        JudgeEngine::new(chart(objects), rules(false), profile(offset)).unwrap()
    }
}
fn meta(source: u64) -> EventMeta {
    let mut meta = EventMeta::new(
        DeviceId(source),
        ClockPoint {
            domain: ClockDomainId(42),
            timestamp: ts(9_007_199_254_740_993),
        },
        u64::MAX,
    );
    meta.original_clock_point = Some(ClockPoint {
        domain: ClockDomainId(43),
        timestamp: ts(-99),
    });
    meta.native = Some(NativeEventMeta {
        backend: BackendId(8),
        code: Some(17),
        timestamp: Some(ClockPoint {
            domain: ClockDomainId(44),
            timestamp: ts(-101),
        }),
    });
    meta
}
fn button(source: u64, key: u16, game: u32, state: ButtonState) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(game),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: meta(source),
            control: PhysicalControlId::keyboard(key),
            state,
        }),
    }
}
fn touch(source: u64, key: u16, game: u32, contact: u64, phase: TouchPhase) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(game),
        physical: PhysicalInputEvent::Touch(TouchEvent {
            meta: meta(source),
            control: PhysicalControlId::keyboard(key),
            contact: ContactId(contact),
            phase,
            position: Position2 { x: -0.0, y: 12.5 },
            pressure: Some(0.5),
        }),
    }
}
fn facts(
    report: &JudgeInputReport,
    freshness: InputFreshness,
    candidates: usize,
    selected: Option<ObjectId>,
    dispatched: usize,
    input_results: usize,
    passive_results: usize,
) {
    let copied = report.disposition;
    let second_copy = copied;
    assert_eq!(second_copy.freshness(), freshness);
    assert_eq!(copied.candidate_count(), candidates);
    assert_eq!(copied.selected(), selected);
    assert_eq!(copied.dispatched_count(), dispatched);
    assert_eq!(copied.input_result_count(), input_results);
    assert_eq!(copied.passive_result_count(), passive_results);
}

#[test]
fn fresh_empty_held_repeat_and_up_have_distinct_facts_without_synthetic_events() {
    let mut judge = engine(&[], false, 0);
    for (state, expected) in [
        (ButtonState::Repeat, InputFreshness::ExplicitRepeat),
        (ButtonState::Down, InputFreshness::FreshPress),
        (ButtonState::Down, InputFreshness::HeldDown),
        (ButtonState::Repeat, InputFreshness::ExplicitRepeat),
        (ButtonState::Up, InputFreshness::Other),
        (ButtonState::Down, InputFreshness::FreshPress),
    ] {
        let report = judge
            .push_input_report(&button(1, 4, 1, state), ts(0))
            .unwrap();
        facts(&report, expected, 0, None, 0, 0, 0);
        assert_eq!(report.disposition.input_hazard_count(), 0);
        assert_eq!(
            report.disposition.unmatched_fresh_press(),
            expected == InputFreshness::FreshPress
        );
        assert!(report.events.is_empty());
    }
}

#[test]
fn contact_freshness_respects_enablement_and_every_owner_dimension() {
    let first = touch(1, 4, 1, 10, TouchPhase::Down);
    let mut disabled = engine(&[], false, 0);
    for _ in 0..2 {
        let report = disabled.push_input_report(&first, ts(0)).unwrap();
        facts(&report, InputFreshness::Other, 0, None, 0, 0, 0);
        assert!(!report.disposition.unmatched_fresh_press());
    }
    let mut enabled = engine(&[], true, 0);
    for input in [
        first.clone(),
        touch(2, 4, 1, 10, TouchPhase::Down),
        touch(1, 5, 1, 10, TouchPhase::Down),
        touch(1, 4, 2, 10, TouchPhase::Down),
        touch(1, 4, 1, 11, TouchPhase::Down),
        button(1, 4, 1, ButtonState::Down),
    ] {
        let report = enabled.push_input_report(&input, ts(0)).unwrap();
        facts(&report, InputFreshness::FreshPress, 0, None, 0, 0, 0);
        assert!(report.disposition.unmatched_fresh_press());
        let held = enabled.push_input_report(&input, ts(0)).unwrap();
        assert_eq!(held.disposition.freshness(), InputFreshness::HeldDown);
    }
    for phase in [TouchPhase::Move, TouchPhase::Up, TouchPhase::Cancel] {
        let report = enabled
            .push_input_report(&touch(1, 4, 1, 10, phase), ts(0))
            .unwrap();
        assert_eq!(report.disposition.freshness(), InputFreshness::Other);
        assert!(!report.disposition.unmatched_fresh_press());
    }
    let rearmed = enabled.push_input_report(&first, ts(0)).unwrap();
    assert_eq!(rearmed.disposition.freshness(), InputFreshness::FreshPress);
}

#[test]
fn actual_hit_has_precommit_candidates_and_one_offset_with_original_provenance() {
    for (at, delta) in [(90, -10), (100, 0), (110, 10)] {
        let mut judge = engine(&[(9, 100, None, 1), (2, 100, None, 1)], false, 25);
        let input = button(1, 4, 1, ButtonState::Down);
        let report = judge.push_input_report(&input, ts(at - 25)).unwrap();
        facts(
            &report,
            InputFreshness::FreshPress,
            2,
            Some(ObjectId(2)),
            1,
            1,
            0,
        );
        assert!(!report.disposition.unmatched_fresh_press());
        assert_eq!(
            report.events,
            vec![JudgeEvent {
                object: ObjectId(2),
                stage: JudgeStage::Instant,
                outcome: JudgeOutcome::Hit {
                    grade: JudgeGrade(7),
                    delta: dur(delta)
                },
                at: ts(at),
                input: Some(*input.physical.meta()),
            }]
        );
        assert_eq!(judge.effective_song_time(), Some(ts(at)));
    }
}

#[derive(Clone, Copy)]
struct Choose(Option<ObjectId>);
impl CandidateResolver for Choose {
    fn select(&self, _: &[Candidate]) -> Option<ObjectId> {
        self.0
    }
    fn snapshot_clone(&self) -> Option<Box<dyn CandidateResolver>> {
        Some(Box::new(*self))
    }
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        let mut bytes = b"disposition-test/choose/v1".to_vec();
        bytes.push(u8::from(self.0.is_some()));
        bytes.extend_from_slice(&self.0.map_or(0, |id| id.0).to_le_bytes());
        Some(bytes)
    }
}

#[test]
fn declined_nonempty_candidates_are_not_unmatched_and_preserve_pending_note() {
    let mut judge = JudgeEngine::with_policies(
        chart(&[(1, 100, None, 1)]),
        rules(false),
        profile(0),
        Box::new(Choose(None)),
        Box::new(WindowJudgePolicy),
    )
    .unwrap();
    let report = judge
        .push_input_report(&button(1, 4, 1, ButtonState::Down), ts(100))
        .unwrap();
    facts(&report, InputFreshness::FreshPress, 1, None, 0, 0, 0);
    assert!(!report.disposition.unmatched_fresh_press());
    assert!(report.events.is_empty());
    assert_eq!(judge.state(ObjectId(1)), Some(InteractionState::Pending));
    let expiry = judge.advance_to(ts(111)).unwrap();
    assert_eq!(expiry.len(), 1);
    assert_eq!(expiry[0].input, None);
}

#[test]
fn invalid_selection_returns_no_report_and_leaves_expiry_ownership_and_hash_unchanged() {
    for selected in [ObjectId(99), ObjectId(1)] {
        let mut judge = JudgeEngine::with_policies(
            chart(&[(1, 0, None, 1), (2, 100, None, 1)]),
            rules(false),
            profile(0),
            Box::new(Choose(Some(selected))),
            Box::new(WindowJudgePolicy),
        )
        .unwrap();
        judge
            .configure_hazards(HazardTimeline::new(vec![marker(1, 0, 1)], 1).unwrap())
            .unwrap();
        let before = judge.stable_hash().unwrap();
        let input = button(1, 4, 1, ButtonState::Down);
        assert!(
            matches!(judge.push_input_report(&input, ts(100)), Err(JudgeError::InvalidCandidate { object }) if object == selected)
        );
        assert_eq!(judge.stable_hash().unwrap(), before);
        assert_eq!(judge.effective_song_time(), None);
        assert!(!judge.is_held(InputOwner {
            source: DeviceId(1),
            physical: PhysicalControlId::keyboard(4),
            game_control: GameControlId(1)
        }));
        assert!(judge.hazard_events().is_empty());
        assert_eq!(judge.state(ObjectId(1)), Some(InteractionState::Pending));
        assert_eq!(judge.state(ObjectId(2)), Some(InteractionState::Pending));
        assert_eq!(judge.advance_to(ts(11)).unwrap().len(), 1);
    }
}

#[test]
fn checked_time_errors_do_not_change_engine_state_or_accept_an_observation() {
    let input = button(1, 4, 1, ButtonState::Down);
    let mut judge = engine(&[], false, 1);
    let before = judge.stable_hash().unwrap();
    assert!(matches!(
        judge.push_input_report(&input, ts(i64::MAX)),
        Err(JudgeError::Overflow)
    ));
    assert_eq!(judge.stable_hash().unwrap(), before);
    let first = judge.push_input_report(&input, ts(10)).unwrap();
    assert_eq!(first.disposition.freshness(), InputFreshness::FreshPress);
    let before = judge.stable_hash().unwrap();
    assert!(matches!(
        judge.push_input_report(&input, ts(9)),
        Err(JudgeError::NonMonotonicSongTime)
    ));
    assert_eq!(judge.stable_hash().unwrap(), before);
}

struct SilentEvaluator {
    calls: Arc<AtomicUsize>,
    gate: Arc<AtomicBool>,
}
struct SilentInteraction {
    calls: Arc<AtomicUsize>,
    gate: Arc<AtomicBool>,
    control: GameControlId,
    state: InteractionState,
}
impl InteractionEvaluator for SilentEvaluator {
    fn validate(&self, _: &TimedObject, _: &JudgeProfile) -> Result<(), JudgeError> {
        Ok(())
    }
    fn begin(&self, _: &TimedObject, context: &BeginContext<'_>) -> Box<dyn ActiveInteraction> {
        Box::new(SilentInteraction {
            calls: self.calls.clone(),
            gate: self.gate.clone(),
            control: context.control,
            state: InteractionState::Pending,
        })
    }
}
impl ActiveInteraction for SilentInteraction {
    fn state(&self) -> InteractionState {
        self.state
    }
    fn accepts_input(&self, input: &GameInputEvent, _: &InteractionContext<'_>) -> bool {
        input.game_control == self.control && self.gate.load(Ordering::SeqCst)
    }
    fn on_input(&mut self, _: &GameInputEvent, _: &InteractionContext<'_>) -> InteractionOutput {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.state = InteractionState::Active;
        InteractionOutput::default()
    }
    fn advance_to(&mut self, _: Timestamp, _: &InteractionContext<'_>) -> InteractionOutput {
        InteractionOutput::default()
    }
    fn deadline(&self, _: &JudgeProfile) -> Option<i128> {
        None
    }
}
struct CloseGateOnSelection(Arc<AtomicBool>);
impl CandidateResolver for CloseGateOnSelection {
    fn select(&self, candidates: &[Candidate]) -> Option<ObjectId> {
        self.0.store(false, Ordering::SeqCst);
        candidates.first().map(|candidate| candidate.object)
    }
}
fn silent_engine(
    calls: Arc<AtomicUsize>,
    gate: Arc<AtomicBool>,
    resolver: Box<dyn CandidateResolver>,
) -> JudgeEngine {
    JudgeEngine::with_policies(
        chart(&[(1, 100, None, 77)]),
        vec![Rule {
            interaction: InteractionId(77),
            control: GameControlId(1),
            evaluator: Box::new(SilentEvaluator { calls, gate }),
        }],
        profile(0),
        resolver,
        Box::new(WindowJudgePolicy),
    )
    .unwrap()
}

#[test]
fn active_zero_result_callback_matches_even_when_no_candidate_and_no_event() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut judge = silent_engine(
        calls.clone(),
        Arc::new(AtomicBool::new(true)),
        Box::new(ClosestCandidate),
    );
    let first = judge
        .push_input_report(&button(1, 4, 1, ButtonState::Down), ts(100))
        .unwrap();
    facts(
        &first,
        InputFreshness::FreshPress,
        1,
        Some(ObjectId(1)),
        1,
        0,
        0,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(judge.state(ObjectId(1)), Some(InteractionState::Active));
    let next = judge
        .push_input_report(&button(2, 4, 1, ButtonState::Down), ts(100))
        .unwrap();
    facts(&next, InputFreshness::FreshPress, 0, None, 1, 0, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(next.events.is_empty());
    assert!(!next.disposition.unmatched_fresh_press());
}

#[test]
fn selected_object_filtered_by_actual_acceptance_is_not_counted_as_dispatch() {
    // A caller-owned policy changes evaluator acceptance during selection.
    // This deliberately exercises the public callback filter after admission.
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(AtomicBool::new(true));
    let mut judge = silent_engine(
        calls.clone(),
        gate.clone(),
        Box::new(CloseGateOnSelection(gate)),
    );
    let report = judge
        .push_input_report(&button(1, 4, 1, ButtonState::Down), ts(100))
        .unwrap();
    facts(
        &report,
        InputFreshness::FreshPress,
        1,
        Some(ObjectId(1)),
        0,
        0,
        0,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(judge.state(ObjectId(1)), Some(InteractionState::Pending));
    assert!(!report.disposition.unmatched_fresh_press());
}

#[test]
fn unrelated_passive_miss_does_not_mask_unmatched_fresh_press() {
    let mut judge = engine(&[(1, 0, None, 1)], false, 0);
    let report = judge
        .push_input_report(&button(1, 4, 2, ButtonState::Down), ts(11))
        .unwrap();
    facts(&report, InputFreshness::FreshPress, 0, None, 0, 0, 1);
    assert!(report.disposition.unmatched_fresh_press());
    assert_eq!(
        report.events,
        vec![JudgeEvent {
            object: ObjectId(1),
            stage: JudgeStage::Instant,
            outcome: JudgeOutcome::Miss {
                reason: MissReason::HeadTimeout
            },
            at: ts(11),
            input: None,
        }]
    );
}

#[test]
fn passive_and_callback_results_are_counted_separately_in_one_successful_hit() {
    let mut judge = engine(&[(1, 0, None, 1), (2, 100, None, 1)], false, 0);
    let input = button(1, 4, 1, ButtonState::Down);
    let report = judge.push_input_report(&input, ts(100)).unwrap();
    facts(
        &report,
        InputFreshness::FreshPress,
        1,
        Some(ObjectId(2)),
        1,
        1,
        1,
    );
    assert!(!report.disposition.unmatched_fresh_press());
    assert_eq!(
        report.events,
        vec![
            JudgeEvent {
                object: ObjectId(1),
                stage: JudgeStage::Instant,
                outcome: JudgeOutcome::Miss {
                    reason: MissReason::HeadTimeout
                },
                at: ts(100),
                input: None,
            },
            JudgeEvent {
                object: ObjectId(2),
                stage: JudgeStage::Instant,
                outcome: JudgeOutcome::Hit {
                    grade: JudgeGrade(7),
                    delta: dur(0)
                },
                at: ts(100),
                input: Some(*input.physical.meta()),
            },
        ]
    );
}

fn marker(id: u64, at: i64, control: u32) -> HazardMarker {
    HazardMarker {
        id: HazardId(id),
        at: ts(at),
        control: GameControlId(control),
        value: id,
    }
}

#[test]
fn inclusive_hazard_count_includes_avoided_other_controls_and_excludes_passive_markers() {
    let mut judge = engine(&[], false, 0);
    judge
        .configure_hazards(
            HazardTimeline::new(vec![marker(1, 9, 1), marker(2, 10, 1), marker(3, 10, 2)], 3)
                .unwrap(),
        )
        .unwrap();
    let input = button(1, 4, 1, ButtonState::Down);
    let report = judge.push_input_report(&input, ts(10)).unwrap();
    facts(&report, InputFreshness::FreshPress, 0, None, 0, 0, 0);
    assert_eq!(report.disposition.input_hazard_count(), 2);
    assert!(report.disposition.unmatched_fresh_press());
    assert!(report.events.is_empty());
    let hazards = judge.hazard_events();
    assert_eq!(hazards.len(), 3);
    assert_eq!(
        (hazards[0].id, hazards[0].outcome, hazards[0].input),
        (HazardId(1), HazardOutcome::Avoided, None)
    );
    assert_eq!(
        (hazards[1].id, hazards[1].outcome, hazards[1].input),
        (
            HazardId(2),
            HazardOutcome::Triggered,
            Some(*input.physical.meta())
        )
    );
    assert_eq!(
        (hazards[2].id, hazards[2].outcome, hazards[2].input),
        (
            HazardId(3),
            HazardOutcome::Avoided,
            Some(*input.physical.meta())
        )
    );
    let repeated = judge.push_input_report(&input, ts(10)).unwrap();
    assert_eq!(repeated.disposition.input_hazard_count(), 0);
    assert!(!repeated.disposition.unmatched_fresh_press());

    let mut advanced = engine(&[], false, 0);
    advanced
        .configure_hazards(HazardTimeline::new(vec![marker(4, 10, 2)], 1).unwrap())
        .unwrap();
    advanced.advance_to(ts(10)).unwrap();
    assert_eq!(advanced.hazard_events().len(), 1);
    let report = advanced.push_input_report(&input, ts(10)).unwrap();
    assert_eq!(report.disposition.input_hazard_count(), 0);
    assert!(report.disposition.unmatched_fresh_press());
    assert!(advanced.hazard_events().is_empty());
}

#[test]
fn legacy_and_reporting_transitions_preserve_events_hashes_and_restored_contact_ownership() {
    let objects = [(1, 100, Some(200), 2), (2, 200, None, 1), (3, 100, None, 1)];
    let mut legacy = engine(&objects, true, 25);
    let mut observed = engine(&objects, true, 25);
    for judge in [&mut legacy, &mut observed] {
        judge
            .configure_hazards(
                HazardTimeline::new(
                    vec![marker(1, 100, 1), marker(2, 150, 2), marker(3, 200, 1)],
                    3,
                )
                .unwrap(),
            )
            .unwrap();
    }
    let down = touch(1, 4, 1, 9, TouchPhase::Down);
    let old = legacy.push_input(&down, ts(75)).unwrap();
    let new = observed.push_input_report(&down, ts(75)).unwrap();
    assert_eq!(new.events, old);
    assert_eq!(new.disposition.selected(), Some(ObjectId(1)));
    assert_eq!(
        observed.stable_hash().unwrap(),
        legacy.stable_hash().unwrap()
    );
    let snapshot = observed.snapshot().unwrap();
    let held_hash = observed.stable_hash().unwrap();
    let mut restored = JudgeEngine::from_snapshot(&snapshot).unwrap();
    let mut restored_legacy = JudgeEngine::from_snapshot(&snapshot).unwrap();
    assert_eq!(restored.stable_hash().unwrap(), held_hash);
    let held = restored.push_input_report(&down, ts(75)).unwrap();
    let legacy_held = restored_legacy.push_input(&down, ts(75)).unwrap();
    assert_eq!(held.disposition.freshness(), InputFreshness::HeldDown);
    assert_eq!(held.events, legacy_held);
    assert!(restored.hazard_events().is_empty());
    assert!(restored_legacy.hazard_events().is_empty());
    assert_eq!(
        restored.stable_hash().unwrap(),
        restored_legacy.stable_hash().unwrap()
    );
    for (input, at) in [
        (touch(1, 4, 1, 9, TouchPhase::Move), 100),
        (touch(1, 4, 1, 9, TouchPhase::Up), 175),
        (down.clone(), 175),
        (button(1, 4, 1, ButtonState::Repeat), 175),
    ] {
        let old = legacy.push_input(&input, ts(at)).unwrap();
        let report = observed.push_input_report(&input, ts(at)).unwrap();
        assert_eq!(report.events, old);
        assert_eq!(
            restored.push_input_report(&input, ts(at)).unwrap().events,
            old
        );
        assert_eq!(restored_legacy.push_input(&input, ts(at)).unwrap(), old);
        assert_eq!(
            observed.stable_hash().unwrap(),
            legacy.stable_hash().unwrap()
        );
        assert_eq!(
            restored.stable_hash().unwrap(),
            restored_legacy.stable_hash().unwrap()
        );
        assert_eq!(observed.hazard_events(), legacy.hazard_events());
    }
    assert_eq!(
        observed.advance_to(ts(250)).unwrap(),
        legacy.advance_to(ts(250)).unwrap()
    );
    assert_eq!(
        observed.stable_hash().unwrap(),
        legacy.stable_hash().unwrap()
    );
    observed.restore(&snapshot).unwrap();
    assert_eq!(observed.stable_hash().unwrap(), held_hash);
    restored_legacy.restore(&snapshot).unwrap();
    let report = observed.push_input_report(&down, ts(75)).unwrap();
    assert_eq!(report.disposition.freshness(), InputFreshness::HeldDown);
    assert_eq!(
        report.events,
        restored_legacy.push_input(&down, ts(75)).unwrap()
    );
    assert_eq!(
        observed.stable_hash().unwrap(),
        restored_legacy.stable_hash().unwrap()
    );
}
