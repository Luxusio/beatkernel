//! Deferred explicit contact ownership over real empty and button-only judges.
use beatkernel::{
    audio::{AudioCommand, SampleId, VoiceId},
    chart::*,
    input::*,
    interaction::{InstantEvaluator, PressInstantEvaluator},
    judge::*,
    runtime::input_sound::{InputSoundMarker, InputSoundTimeline},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn chart(note: bool) -> CompiledChart {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    if note {
        source.objects.push(SourceObject {
            id: ObjectId(1),
            start: Beat::new(100).unwrap(),
            end: None,
            interaction: InteractionId(1),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    source.compile().unwrap()
}
fn profile() -> JudgeProfile {
    JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::ZERO,
    )
    .unwrap()
}
fn rules(press: bool) -> Vec<Rule> {
    vec![Rule {
        interaction: InteractionId(1),
        control: GameControlId(1),
        evaluator: if press {
            Box::new(PressInstantEvaluator)
        } else {
            Box::new(InstantEvaluator)
        },
    }]
}
fn meta(source: u64) -> EventMeta {
    let mut meta = EventMeta::new(
        DeviceId(source),
        ClockPoint {
            domain: ClockDomainId(7),
            timestamp: ts(9_007_199_254_740_993),
        },
        u64::MAX,
    );
    meta.original_clock_point = Some(ClockPoint {
        domain: ClockDomainId(8),
        timestamp: ts(-19),
    });
    meta
}
fn surface(code: u32) -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(u32::MAX),
        code,
    }
}
fn touch(source: u64, code: u32, logical: u32, contact: u64, phase: TouchPhase) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(logical),
        physical: PhysicalInputEvent::Touch(TouchEvent {
            meta: meta(source),
            control: surface(code),
            contact: ContactId(contact),
            phase,
            position: Position2 {
                x: -13.25,
                y: 65536.5,
            },
            pressure: Some(0.75),
        }),
    }
}
fn button(state: ButtonState) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(1),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: meta(u64::MAX),
            control: surface(u32::MAX),
            state,
        }),
    }
}

#[test]
fn empty_explicit_judge_owns_full_contact_identity_without_objects_or_synthetic_results() {
    let compiled = chart(false);
    assert!(compiled.objects().is_empty());
    let mut judge = JudgeEngine::new_with_contacts(compiled, vec![], profile()).unwrap();
    let down = touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Down);
    let original = down.clone();
    let unchanged = judge.stable_hash().unwrap();
    assert!(judge.is_fresh_press(&down));
    assert!(judge.is_fresh_press(&down));
    assert_eq!(judge.stable_hash().unwrap(), unchanged);
    let moving = touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Move);
    assert!(!judge.is_fresh_press(&moving));
    assert!(judge.push_input(&moving, ts(0)).unwrap().is_empty());
    assert!(
        judge.is_fresh_press(&down),
        "Move does not acquire an unseen contact"
    );
    let timeline = InputSoundTimeline::new(
        vec![InputSoundMarker {
            control: GameControlId(1),
            at: ts(0),
            sample: SampleId(3843),
            voice: VoiceId(u64::MAX),
            gain: 0.25,
        }],
        1,
    )
    .unwrap();
    let fresh = judge.is_fresh_press(&down);
    let results = judge.push_input(&down, ts(0)).unwrap();
    assert!(results.is_empty());
    assert_eq!(judge.state(ObjectId(1)), None);
    assert_eq!(
        timeline.command_for_press(&down, fresh, ts(0), ts(-7), &results),
        Some(AudioCommand::Play {
            sample: SampleId(3843),
            voice: VoiceId(u64::MAX),
            at: ts(-7),
            gain: 0.25
        })
    );
    let held = judge.stable_hash().unwrap();
    let fresh = judge.is_fresh_press(&down);
    let duplicate = judge.push_input(&down, ts(0)).unwrap();
    assert!(!fresh && duplicate.is_empty());
    assert_eq!(judge.stable_hash().unwrap(), held);
    assert_eq!(
        timeline.command_for_press(&down, fresh, ts(0), ts(8), &duplicate),
        None
    );
    for (source, physical, control, id) in [
        (u64::MAX - 1, u32::MAX, 1, u64::MAX),
        (u64::MAX, u32::MAX - 1, 1, u64::MAX),
        (u64::MAX, u32::MAX, 2, u64::MAX),
        (u64::MAX, u32::MAX, 1, u64::MAX - 1),
    ] {
        let other = touch(source, physical, control, id, TouchPhase::Down);
        assert!(judge.is_fresh_press(&other));
        assert!(
            judge
                .push_input(&touch(source, physical, control, id, TouchPhase::Up), ts(0))
                .unwrap()
                .is_empty()
        );
        assert!(
            !judge.is_fresh_press(&down),
            "a different identity cannot release this contact"
        );
    }
    assert!(judge.is_fresh_press(&button(ButtonState::Down)));
    assert!(
        judge
            .push_input(&button(ButtonState::Down), ts(0))
            .unwrap()
            .is_empty()
    );
    judge
        .push_input(
            &touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Up),
            ts(0),
        )
        .unwrap();
    assert!(judge.is_fresh_press(&down));
    assert!(!judge.is_fresh_press(&button(ButtonState::Down)));
    judge.push_input(&down, ts(0)).unwrap();
    judge.push_input(&button(ButtonState::Up), ts(0)).unwrap();
    assert!(!judge.is_fresh_press(&down));
    assert!(judge.is_fresh_press(&button(ButtonState::Down)));
    judge
        .push_input(
            &touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Cancel),
            ts(0),
        )
        .unwrap();
    assert!(judge.is_fresh_press(&down));
    assert_eq!(down, original);
    assert!(
        judge
            .advance_to(ts(604_800_000_000_000))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn enabled_identity_clones_and_restores_atomically_and_inferred_configuration_has_the_same_hash() {
    let mut explicit = JudgeEngine::new_with_contacts(chart(true), rules(true), profile()).unwrap();
    let mut inferred = JudgeEngine::new(chart(true), rules(true), profile()).unwrap();
    let policy_ctor = JudgeEngine::with_policies_and_contacts(
        chart(true),
        rules(true),
        profile(),
        Box::new(ClosestCandidate),
        Box::new(WindowJudgePolicy),
    )
    .unwrap();
    assert_eq!(
        explicit.stable_hash().unwrap(),
        inferred.stable_hash().unwrap()
    );
    assert_eq!(
        explicit.stable_hash().unwrap(),
        policy_ctor.stable_hash().unwrap()
    );
    let down = touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Down);
    assert_eq!(
        explicit.push_input(&down, ts(0)).unwrap(),
        inferred.push_input(&down, ts(0)).unwrap()
    );
    assert_eq!(
        explicit.stable_hash().unwrap(),
        inferred.stable_hash().unwrap()
    );
    let checkpoint = explicit.snapshot().unwrap();
    let mut clone = JudgeEngine::from_snapshot(&checkpoint).unwrap();
    assert_eq!(
        clone.stable_hash().unwrap(),
        explicit.stable_hash().unwrap()
    );
    clone
        .push_input(
            &touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Cancel),
            ts(0),
        )
        .unwrap();
    assert!(clone.is_fresh_press(&down));
    assert!(!explicit.is_fresh_press(&down));
    clone.restore(&checkpoint).unwrap();
    assert!(!clone.is_fresh_press(&down));
    inferred.restore(&checkpoint).unwrap();
    assert_eq!(
        inferred.stable_hash().unwrap(),
        explicit.stable_hash().unwrap()
    );

    let mut enabled = JudgeEngine::new_with_contacts(chart(false), vec![], profile()).unwrap();
    enabled.push_input(&down, ts(0)).unwrap();
    let enabled_snapshot = enabled.snapshot().unwrap();
    let enabled_hash = enabled.stable_hash().unwrap();
    let mut disabled = JudgeEngine::new(chart(false), vec![], profile()).unwrap();
    let mut legacy_policy = JudgeEngine::with_policies(
        chart(false),
        vec![],
        profile(),
        Box::new(ClosestCandidate),
        Box::new(WindowJudgePolicy),
    )
    .unwrap();
    assert_eq!(
        disabled.stable_hash().unwrap(),
        legacy_policy.stable_hash().unwrap()
    );
    // Legacy ignored touch changes only the actual time frontier, just as advance.
    for phase in [TouchPhase::Down, TouchPhase::Move, TouchPhase::Cancel] {
        assert!(
            disabled
                .push_input(&touch(u64::MAX, u32::MAX, 1, u64::MAX, phase), ts(0))
                .unwrap()
                .is_empty()
        );
        legacy_policy.advance_to(ts(0)).unwrap();
        assert_eq!(
            disabled.stable_hash().unwrap(),
            legacy_policy.stable_hash().unwrap()
        );
        assert!(!disabled.is_fresh_press(&down));
    }
    let disabled_snapshot = disabled.snapshot().unwrap();
    let disabled_hash = disabled.stable_hash().unwrap();
    assert_ne!(disabled_hash, enabled_hash);
    assert_eq!(
        disabled.restore(&enabled_snapshot),
        Err(SnapshotError::ConfigurationMismatch)
    );
    assert_eq!(disabled.stable_hash().unwrap(), disabled_hash);
    assert_eq!(
        enabled.restore(&disabled_snapshot),
        Err(SnapshotError::ConfigurationMismatch)
    );
    assert_eq!(enabled.stable_hash().unwrap(), enabled_hash);
    assert!(!enabled.is_fresh_press(&down));
}

struct CountCandidates(Arc<AtomicUsize>);
impl CandidateResolver for CountCandidates {
    fn select(&self, candidates: &[Candidate]) -> Option<ObjectId> {
        self.0.fetch_add(1, Ordering::Relaxed);
        candidates.first().map(|candidate| candidate.object)
    }
}
struct Grade99;
impl JudgePolicy for Grade99 {
    fn grade(&self, delta: i128, _: &JudgeProfile) -> Option<JudgeGrade> {
        (delta == 0).then_some(JudgeGrade(99))
    }
}

#[test]
fn explicit_tracking_keeps_button_only_eligibility_and_real_custom_policy_validation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut explicit = JudgeEngine::with_policies_and_contacts(
        chart(true),
        rules(false),
        profile(),
        Box::new(CountCandidates(calls.clone())),
        Box::new(Grade99),
    )
    .unwrap();
    let mut legacy = JudgeEngine::with_policies(
        chart(true),
        rules(false),
        profile(),
        Box::new(CountCandidates(calls.clone())),
        Box::new(Grade99),
    )
    .unwrap();
    let down = touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Down);
    assert!(explicit.is_fresh_press(&down));
    assert!(!legacy.is_fresh_press(&down));
    assert!(explicit.push_input(&down, ts(100)).unwrap().is_empty());
    assert!(legacy.push_input(&down, ts(100)).unwrap().is_empty());
    assert!(!explicit.is_fresh_press(&down));
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    let key = button(ButtonState::Down);
    let result = explicit.push_input(&key, ts(100)).unwrap();
    assert_eq!(result, legacy.push_input(&key, ts(100)).unwrap());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].object, ObjectId(1));
    assert_eq!(result[0].stage, JudgeStage::Instant);
    assert_eq!(
        result[0].outcome,
        JudgeOutcome::Hit {
            grade: JudgeGrade(99),
            delta: Duration::ZERO
        }
    );
    assert_eq!(result[0].input, Some(*key.physical.meta()));
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let ordinary_error = JudgeEngine::new(chart(true), vec![], profile())
        .err()
        .unwrap();
    assert_eq!(
        JudgeEngine::new_with_contacts(chart(true), vec![], profile())
            .err()
            .unwrap(),
        ordinary_error
    );
    assert_eq!(
        JudgeEngine::with_policies_and_contacts(
            chart(true),
            vec![],
            profile(),
            Box::new(ClosestCandidate),
            Box::new(WindowJudgePolicy)
        )
        .err()
        .unwrap(),
        ordinary_error
    );
}
