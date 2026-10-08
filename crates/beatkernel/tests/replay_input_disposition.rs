use std::{cell::RefCell, rc::Rc};

use beatkernel::replay::codec as replay_codec;
use beatkernel::{chart::*, input::*, interaction::*, judge::*, replay::*, time::*};

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}

fn header() -> ReplayHeader {
    ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: b"disposition-chart".to_vec(),
        rules_identity: b"builtin-hold-instant".to_vec(),
        options: vec![9],
        seed: 17,
        normalized_clock: ClockDomainId(7),
    }
}

fn engine(offset: i64, hazards: bool) -> JudgeEngine {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = [(1, 100, Some(200), 1), (2, 300, None, 2)]
        .into_iter()
        .map(|(id, at, end, interaction)| SourceObject {
            id: ObjectId(id),
            start: Beat::new(at).unwrap(),
            end: end.map(|n| Beat::new(n).unwrap()),
            interaction: InteractionId(interaction),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .collect();
    let mut judge = JudgeEngine::new(
        compile(&source).unwrap(),
        vec![
            Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(HoldEvaluator),
            },
            Rule {
                interaction: InteractionId(2),
                control: GameControlId(1),
                evaluator: Box::new(InstantEvaluator),
            },
        ],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::from_nanos(10),
                late: Duration::from_nanos(10),
            }],
            Duration::from_nanos(offset),
        )
        .unwrap(),
    )
    .unwrap();
    if hazards {
        judge
            .configure_hazards(
                HazardTimeline::new(
                    [(1, 99, 1), (2, 100, 1), (3, 100, 2)]
                        .into_iter()
                        .map(|(id, at, control)| HazardMarker {
                            id: HazardId(id),
                            at: ts(at),
                            control: GameControlId(control),
                            value: id,
                        })
                        .collect(),
                    3,
                )
                .unwrap(),
            )
            .unwrap();
    }
    judge
}

fn button(source: u64, state: ButtonState) -> GameInputEvent {
    let mut meta = EventMeta::new(
        DeviceId(source),
        ClockPoint {
            domain: ClockDomainId(7),
            timestamp: ts(1000),
        },
        source,
    );
    meta.native = Some(NativeEventMeta {
        backend: BackendId(2),
        code: Some(42),
        timestamp: Some(ClockPoint {
            domain: ClockDomainId(8),
            timestamp: ts(999),
        }),
    });
    GameInputEvent {
        game_control: GameControlId(1),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta,
            control: PhysicalControlId::keyboard(4),
            state,
        }),
    }
}

fn input_record(ordinal: u64, at: i64, input: GameInputEvent) -> ReplayRecord {
    ReplayRecord {
        ordinal,
        song_time: ts(at),
        operation: ReplayOperation::Input(input),
    }
}

fn assert_same(left: &ReplaySession, right: &ReplaySession) {
    assert_eq!(left.records(), right.records());
    assert_eq!(left.results(), right.results());
    assert_eq!(left.cursor(), right.cursor());
    assert_eq!(left.stable_hash().unwrap(), right.stable_hash().unwrap());
    assert_eq!(
        left.engine().stable_hash().unwrap(),
        right.engine().stable_hash().unwrap()
    );
    assert_eq!(
        left.engine().hazard_events(),
        right.engine().hazard_events()
    );
    let limits = replay_codec::ReplayCodecLimits::new(
        1_000_000,
        128,
        256,
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap();
    let left_file = replay_codec::ReplayFile::new(left.header().clone(), left.records().to_vec());
    let right_file =
        replay_codec::ReplayFile::new(right.header().clone(), right.records().to_vec());
    assert_eq!(
        replay_codec::encode_replay(&left_file, limits).unwrap(),
        replay_codec::encode_replay(&right_file, limits).unwrap()
    );
}

#[test]
fn report_records_original_input_and_preserves_offset_ownership_and_hazards() {
    let mut observed = ReplaySession::new(header(), engine(7, true)).unwrap();
    let mut legacy = ReplaySession::new(header(), engine(7, true)).unwrap();
    let down = button(1, ButtonState::Down);
    let report = observed.push_input_report(down.clone(), ts(93)).unwrap();
    assert_eq!(
        report.events,
        legacy.push_input(down.clone(), ts(93)).unwrap()
    );
    assert_eq!(report.disposition.freshness(), InputFreshness::FreshPress);
    assert_eq!(report.disposition.candidate_count(), 1);
    assert_eq!(report.disposition.selected(), Some(ObjectId(1)));
    assert_eq!(report.disposition.dispatched_count(), 1);
    assert_eq!(report.disposition.input_result_count(), 1);
    assert_eq!(report.disposition.passive_result_count(), 0);
    assert_eq!(report.disposition.input_hazard_count(), 2);
    assert!(!report.disposition.unmatched_fresh_press());
    assert_eq!(report.events[0].at, ts(100));
    assert_eq!(report.events[0].input, Some(*down.physical.meta()));
    assert_eq!(observed.records(), &[input_record(0, 93, down.clone())]);
    assert_eq!(observed.results(), report.events);
    assert_eq!(observed.cursor(), 1);
    assert_eq!(observed.engine().effective_song_time(), Some(ts(100)));
    assert_eq!(observed.engine().hazard_events().len(), 3);
    assert_same(&observed, &legacy);

    for (input, at, freshness) in [
        (down, 94, InputFreshness::HeldDown),
        (
            button(1, ButtonState::Repeat),
            95,
            InputFreshness::ExplicitRepeat,
        ),
        (button(2, ButtonState::Up), 193, InputFreshness::Other),
        (button(1, ButtonState::Up), 193, InputFreshness::Other),
    ] {
        let report = observed.push_input_report(input.clone(), ts(at)).unwrap();
        assert_eq!(report.disposition.freshness(), freshness);
        assert_eq!(report.events, legacy.push_input(input, ts(at)).unwrap());
        assert_same(&observed, &legacy);
    }
    assert_eq!(
        observed.results().last().unwrap().stage,
        JudgeStage::HoldTail
    );
    assert_eq!(
        observed.results().last().unwrap().input.unwrap().source,
        DeviceId(1)
    );
}

#[test]
fn observed_reconstruction_streams_inputs_before_requesting_the_next_record() {
    let records = vec![
        input_record(0, 100, button(1, ButtonState::Down)),
        ReplayRecord {
            ordinal: 1,
            song_time: ts(150),
            operation: ReplayOperation::Advance,
        },
        input_record(2, 200, button(1, ButtonState::Up)),
        ReplayRecord {
            ordinal: 3,
            song_time: ts(500),
            operation: ReplayOperation::Advance,
        },
    ];
    let seen = Rc::new(RefCell::new(Vec::new()));
    let iteration_seen = Rc::clone(&seen);
    let iter = records
        .clone()
        .into_iter()
        .enumerate()
        .map(move |(index, record)| {
            let expected_callbacks = [0, 1, 1, 2][index];
            assert_eq!(iteration_seen.borrow().len(), expected_callbacks);
            record
        });
    let callback_seen = Rc::clone(&seen);
    let observed = ReplaySession::from_records_observed(
        header(),
        engine(0, false),
        iter,
        move |record, disposition| {
            assert!(matches!(record.operation, ReplayOperation::Input(_)));
            callback_seen
                .borrow_mut()
                .push((record.clone(), disposition));
        },
    )
    .unwrap();
    let captured = seen.borrow();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].0, records[0]);
    assert_eq!(captured[1].0, records[2]);
    assert_eq!(captured[0].1.freshness(), InputFreshness::FreshPress);
    assert_eq!(captured[1].1.freshness(), InputFreshness::Other);
    assert_eq!(captured[1].1.input_result_count(), 1);
    let legacy = ReplaySession::from_records(header(), engine(0, false), records).unwrap();
    assert_same(&observed, &legacy);
    assert_eq!(observed.cursor(), 4);
}

#[test]
fn rejected_records_have_no_callback_and_later_failure_keeps_the_accepted_prefix() {
    let accepted = input_record(0, 100, button(1, ButtonState::Down));
    let mut bad_domain = input_record(1, 110, button(2, ButtonState::Down));
    if let ReplayOperation::Input(input) = &mut bad_domain.operation {
        input.physical.meta_mut().clock_domain = ClockDomainId(99);
    }
    let mut bad_ordinal = input_record(1, 110, button(2, ButtonState::Down));
    bad_ordinal.ordinal = 9;
    for (bad, expected) in [
        (bad_domain, ReplayError::ClockDomainMismatch),
        (bad_ordinal, ReplayError::InvalidOrdinal),
        (
            input_record(1, 99, button(2, ButtonState::Down)),
            ReplayError::NonMonotonicSongTime,
        ),
        (
            input_record(1, i64::MAX, button(2, ButtonState::Down)),
            ReplayError::Judge(JudgeError::Overflow),
        ),
    ] {
        let mut seen = Vec::new();
        let result = ReplaySession::from_records_observed(
            header(),
            engine(1, false),
            [accepted.clone(), bad],
            |record, facts| seen.push((record.clone(), facts)),
        );
        assert_eq!(result.err(), Some(expected));
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, accepted);
        assert_eq!(seen[0].1.freshness(), InputFreshness::FreshPress);
        assert_eq!(seen[0].1.selected(), Some(ObjectId(1)));
    }
    for record in [
        input_record(1, 100, button(1, ButtonState::Down)),
        input_record(0, i64::MAX, button(1, ButtonState::Down)),
    ] {
        let mut callbacks = 0;
        assert!(ReplaySession::from_records_observed(
            header(),
            engine(1, false),
            [record],
            |_, _| callbacks += 1
        )
        .is_err());
        assert_eq!(callbacks, 0);
    }
}

#[test]
fn live_rejection_is_atomic_and_future_requires_fork() {
    let mut observed = ReplaySession::new(header(), engine(1, false)).unwrap();
    observed
        .push_input_report(button(1, ButtonState::Down), ts(99))
        .unwrap();
    let before = observed.stable_hash().unwrap();
    let original_records = observed.records().to_vec();
    let original_results = observed.results().to_vec();
    let mut bad_domain = button(2, ButtonState::Down);
    bad_domain.physical.meta_mut().clock_domain = ClockDomainId(99);
    for (input, at, expected) in [
        (bad_domain, 100, ReplayError::ClockDomainMismatch),
        (
            button(2, ButtonState::Down),
            98,
            ReplayError::NonMonotonicSongTime,
        ),
        (
            button(2, ButtonState::Down),
            i64::MAX,
            ReplayError::Judge(JudgeError::Overflow),
        ),
    ] {
        assert_eq!(
            observed.push_input_report(input, ts(at)).err(),
            Some(expected)
        );
        assert_eq!(observed.stable_hash().unwrap(), before);
        assert_eq!(observed.records(), original_records);
        assert_eq!(observed.results(), original_results);
        assert_eq!(observed.cursor(), 1);
    }
    observed.advance_to(ts(500)).unwrap();
    observed.seek(ts(149)).unwrap();
    let before = observed.stable_hash().unwrap();
    assert_eq!(
        observed
            .push_input_report(button(1, ButtonState::Up), ts(199))
            .err(),
        Some(ReplayError::FutureExists)
    );
    assert_eq!(observed.stable_hash().unwrap(), before);
    observed.fork_at_cursor();
    let report = observed
        .push_input_report(button(1, ButtonState::Up), ts(199))
        .unwrap();
    assert_eq!(report.events[0].stage, JudgeStage::HoldTail);
}

#[test]
fn observed_session_checkpoints_reverse_seek_and_forks_match_legacy() {
    let mut observed = ReplaySession::new(header(), engine(0, true)).unwrap();
    let mut legacy = ReplaySession::new(header(), engine(0, true)).unwrap();
    for (input, at) in [
        (button(1, ButtonState::Down), 100),
        (button(1, ButtonState::Up), 200),
    ] {
        observed.push_input_report(input.clone(), ts(at)).unwrap();
        legacy.push_input(input, ts(at)).unwrap();
        observed.checkpoint().unwrap();
        legacy.checkpoint().unwrap();
    }
    observed.advance_to(ts(500)).unwrap();
    legacy.advance_to(ts(500)).unwrap();
    for at in [250, 150, 500, 250] {
        observed.seek(ts(at)).unwrap();
        legacy.seek(ts(at)).unwrap();
        observed.checkpoint().unwrap();
        legacy.checkpoint().unwrap();
        assert_same(&observed, &legacy);
    }
    observed.fork_at_cursor();
    legacy.fork_at_cursor();
    let input = button(2, ButtonState::Down);
    let report = observed.push_input_report(input.clone(), ts(300)).unwrap();
    assert_eq!(report.events, legacy.push_input(input, ts(300)).unwrap());
    assert_same(&observed, &legacy);
    let mut facts = Vec::new();
    let reconstructed = ReplaySession::from_records_observed(
        header(),
        engine(0, true),
        observed.records().to_vec(),
        |_, disposition| facts.push(disposition),
    )
    .unwrap();
    assert_eq!(facts.len(), 3);
    assert_same(&reconstructed, &legacy);
    observed.seek_cursor(1).unwrap();
    legacy.seek_cursor(1).unwrap();
    assert_same(&observed, &legacy);
}

#[test]
fn unrelated_passive_misses_do_not_mask_unmatched_replay_input() {
    let mut replay = ReplaySession::new(header(), engine(0, false)).unwrap();
    let report = replay
        .push_input_report(button(1, ButtonState::Down), ts(500))
        .unwrap();
    assert_eq!(report.disposition.freshness(), InputFreshness::FreshPress);
    assert_eq!(report.disposition.candidate_count(), 0);
    assert_eq!(report.disposition.selected(), None);
    assert_eq!(report.disposition.dispatched_count(), 0);
    assert_eq!(report.disposition.input_result_count(), 0);
    assert_eq!(report.disposition.passive_result_count(), 2);
    assert!(report.disposition.unmatched_fresh_press());
    assert_eq!(report.events.len(), 2);
    assert!(report.events.iter().all(|event| event.input.is_none()));
    let mut facts = Vec::new();
    let reconstructed = ReplaySession::from_records_observed(
        header(),
        engine(0, false),
        replay.records().to_vec(),
        |_, disposition| facts.push(disposition),
    )
    .unwrap();
    assert_eq!(facts, vec![report.disposition]);
    assert_same(&replay, &reconstructed);
}
