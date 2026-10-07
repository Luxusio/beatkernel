//! Native control commits follow genuine Runtime reports; held servicing does not.
use super::*;
use beatkernel::{
    audio::{command_queue, CommandConsumer},
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::InstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow, Rule},
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockMapper, ClockMappingQuality},
    transport::{Rate, Transport},
};

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn host(nanos: i64) -> ClockPoint {
    point(1, nanos)
}
fn raw(nanos: i64) -> ClockPoint {
    point(2, nanos)
}
fn logical(nanos: i64) -> ClockPoint {
    point(3, nanos)
}
fn pair(raw_ns: i64, host_ns: i64) -> ClockPair {
    ClockPair {
        source: raw(raw_ns),
        target: host(host_ns),
    }
}
fn epoch() -> AudioAuthorityEpoch {
    AudioAuthorityEpoch {
        id: 1,
        stream_origin: raw(1000),
        logical_origin: logical(5000),
        host_domain: ClockDomainId(1),
    }
}
fn config(capacity: usize) -> AudioAuthorityConfig {
    AudioAuthorityConfig {
        history_capacity: capacity,
        max_observation_age: Duration::from_nanos(1000),
        input_extrapolation: ExtrapolationPolicy::Forbid,
        max_input_ahead: Duration::ZERO,
    }
}
fn event(nanos: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(u64::MAX), host(nanos), sequence),
        control: PhysicalControlId::keyboard(7),
        state: ButtonState::Down,
    })
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
struct Seed {
    authority: AudioAuthority,
    merger: InputMerger,
    runtime: Runtime,
    _commands: CommandConsumer,
}
fn genuine_seed(held: bool, capacity: usize) -> Seed {
    let mut chart = SourceChart::new(1000, Bpm::new(60, 1).unwrap()).unwrap();
    chart.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(0).unwrap(),
        end: None,
        interaction: InteractionId(1),
        visual: VisualId(1),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    let judge = JudgeEngine::new(
        chart.compile().unwrap(),
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(InstantEvaluator),
        }],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::from_nanos(1000),
                late: Duration::from_nanos(1000),
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Exact(DeviceId(u64::MAX)),
        physical: PhysicalControlId::keyboard(7),
        game_control: GameControlId(1),
    }])
    .unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(3),
        ClockDomainId(2),
        Transport::new(Timestamp::from_nanos(5000), Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let mut authority = AudioAuthority::new(config(capacity), epoch()).unwrap();
    let mut merger =
        InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(u64::MAX)], 8).unwrap();
    authority.observe(1, pair(1100, 100)).unwrap();
    authority.observe(1, pair(1300, 200)).unwrap();
    merger.admit(event(125, 1), host(250)).unwrap();
    authority.record_acquired_prefix(host(250)).unwrap();
    let prepared = authority
        .prepare_input(host(125), host(250))
        .unwrap()
        .unwrap();
    let original = merger.pop_ready(host(250)).unwrap().unwrap();
    let report = runtime
        .process_input(original, prepared.mapper(), raw(1400))
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(150));
    assert!(report.input.is_some());
    assert_eq!(
        report.judge_events.len(),
        1,
        "seed comes from an actual judged input report"
    );
    authority.commit_input(prepared).unwrap();
    if held {
        let first = authority
            .prepare_held_frontier(host(250), &merger)
            .unwrap()
            .unwrap();
        authority.commit_held_frontier(first, &mut merger).unwrap();
        authority.observe(1, pair(1600, 300)).unwrap();
        authority.record_acquired_prefix(host(350)).unwrap();
        let next = authority
            .prepare_held_frontier(host(350), &merger)
            .unwrap()
            .unwrap();
        authority.commit_held_frontier(next, &mut merger).unwrap();
    }
    Seed {
        authority,
        merger,
        runtime,
        _commands: consumer,
    }
}
fn state(authority: &AudioAuthority) -> (String, usize) {
    (format!("{authority:?}"), authority.history.capacity())
}
fn marks(authority: &AudioAuthority) -> [Option<ClockPoint>; 5] {
    [
        authority.acquired_prefix(),
        authority.closed_host_prefix(),
        authority.committed_input_host(),
        authority.committed_operation(),
        authority.committed_presentation(),
    ]
}

#[test]
fn resume_control_preserves_equal_cutoff_original_until_after_real_control_report() {
    let mut seed = genuine_seed(false, 4);
    let pause = seed
        .authority
        .prepare_control_cutoff(1, raw(1200), host(150), host(250), &seed.merger)
        .unwrap()
        .unwrap();
    seed.runtime
        .transport_mut()
        .pause(pause.output().timestamp)
        .unwrap();
    let paused = seed
        .runtime
        .advance_to(pause.output(), &Identity, raw(1400))
        .unwrap();
    assert_eq!(paused.song_time, Timestamp::from_nanos(200));
    seed.authority
        .commit_control_cutoff(pause, &seed.merger)
        .unwrap();
    let mut original = event(200, 2);
    if let PhysicalInputEvent::Button(button) = &mut original {
        button.state = ButtonState::Up;
    }
    seed.merger.admit(original.clone(), host(250)).unwrap();
    let before = state(&seed.authority);
    assert!(
        seed.authority
            .prepare_control_cutoff(1, raw(1300), host(200), host(250), &seed.merger)
            .unwrap()
            .is_none(),
        "pause retains its inclusive input guard"
    );
    let resume = seed
        .authority
        .prepare_resume_control_cutoff(1, raw(1300), host(200), host(250), &seed.merger)
        .unwrap()
        .unwrap();
    assert!(resume.is_resume());
    assert_eq!(state(&seed.authority), before);
    assert_eq!(seed.merger.pending(), 1);
    assert_eq!(seed.merger.peek_ready(host(250)).unwrap(), Some(&original));
    seed.runtime
        .transport_mut()
        .resume(resume.output().timestamp)
        .unwrap();
    let resumed = seed
        .runtime
        .advance_to(resume.output(), &Identity, raw(1450))
        .unwrap();
    assert_eq!(resumed.song_time, paused.song_time);
    assert!(resumed.input.is_none());
    seed.authority
        .commit_control_cutoff(resume, &seed.merger)
        .unwrap();
    assert_eq!(seed.authority.committed_input_host(), Some(host(125)));
    assert_eq!(seed.merger.pending(), 1);
    let mapped = seed
        .authority
        .prepare_input(host(200), host(250))
        .unwrap()
        .unwrap();
    let packet = seed.merger.pop_ready(host(250)).unwrap().unwrap();
    assert_eq!(packet, original);
    let input = seed
        .runtime
        .process_input(packet, mapped.mapper(), raw(1450))
        .unwrap();
    let recorded = input.input.as_ref().unwrap().meta();
    assert_eq!(recorded.clock_domain, ClockDomainId(3));
    assert_eq!(recorded.timestamp, logical(5300).timestamp);
    assert_eq!(recorded.original_clock_point, Some(host(200)));
    assert_eq!(recorded.source, DeviceId(u64::MAX));
    assert_eq!(recorded.sequence, 2);
    seed.authority.commit_input(mapped).unwrap();
    assert_eq!(seed.authority.committed_input_host(), Some(host(200)));
    assert_eq!(seed.merger.pending(), 0);

    let mut earlier = genuine_seed(false, 4);
    earlier.merger.admit(event(199, 2), host(250)).unwrap();
    let unchanged = state(&earlier.authority);
    assert!(earlier
        .authority
        .prepare_resume_control_cutoff(1, raw(1300), host(200), host(250), &earlier.merger)
        .unwrap()
        .is_none());
    assert_eq!(state(&earlier.authority), unchanged);
    assert_eq!(earlier.merger.pending(), 1);
}

#[test]
fn native_control_uses_direct_raw_boundary_after_real_runtime_report_and_only_commits_operation() {
    let mut seed = genuine_seed(true, 4);
    assert_eq!(
        marks(&seed.authority),
        [
            Some(host(350)),
            Some(host(350)),
            Some(host(125)),
            Some(logical(5150)),
            Some(logical(5600))
        ]
    );
    let before = state(&seed.authority);
    let accepted = marks(&seed.authority);
    let history: Vec<_> = seed.authority.history.iter().copied().collect();
    let merger_before = format!("{:?}", seed.merger);
    let revision = seed.authority.revision;
    let cutoff = seed
        .authority
        .prepare_control_cutoff(1, raw(1400), host(200), host(350), &seed.merger)
        .unwrap()
        .unwrap();
    assert_eq!(cutoff.host(), host(200));
    assert_eq!(cutoff.raw_output(), raw(1400));
    assert_eq!(
        cutoff.output(),
        logical(5400),
        "the raw boundary differs from HOST200's estimated logical5300"
    );
    assert_eq!(state(&seed.authority), before);
    let report = seed
        .runtime
        .advance_to(cutoff.output(), &Identity, raw(1700))
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(400));
    assert_eq!(report.audio_at, raw(1700));
    assert!(report.input.is_none());
    assert!(report.judge_error.is_none());
    assert_eq!(
        state(&seed.authority),
        before,
        "Runtime acceptance precedes authority commitment"
    );
    seed.authority
        .commit_control_cutoff(cutoff, &seed.merger)
        .unwrap();
    let after = marks(&seed.authority);
    assert_eq!(after[0..3], accepted[0..3]);
    assert_eq!(after[3], Some(logical(5400)));
    assert_eq!(
        after[4], accepted[4],
        "held presentation is not a control operation"
    );
    assert_eq!(
        seed.authority.history.iter().copied().collect::<Vec<_>>(),
        history
    );
    assert_eq!(seed.authority.revision, revision + 1);
    assert_eq!(format!("{:?}", seed.merger), merger_before);
}

#[test]
fn control_unavailable_coverage_and_stale_evidence_hold_without_changing_any_state() {
    let seed = genuine_seed(true, 4);
    let before = state(&seed.authority);
    for (raw_output, cutoff, now) in [
        (raw(1700), host(200), host(350)),
        (raw(1400), host(400), host(350)),
        (raw(1400), host(351), host(400)),
        (raw(1400), host(200), host(1301)),
    ] {
        assert!(seed
            .authority
            .prepare_control_cutoff(1, raw_output, cutoff, now, &seed.merger)
            .unwrap()
            .is_none());
        assert_eq!(state(&seed.authority), before);
    }
    let merger = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 2).unwrap();
    let mut startup = AudioAuthority::new(config(2), epoch()).unwrap();
    startup.record_acquired_prefix(host(200)).unwrap();
    for anchors in 0..2 {
        if anchors == 1 {
            startup.observe(1, pair(1100, 100)).unwrap();
        }
        let before = state(&startup);
        assert!(startup
            .prepare_control_cutoff(1, raw(1100), host(100), host(200), &merger)
            .unwrap()
            .is_none());
        assert_eq!(state(&startup), before);
    }
}

#[test]
fn control_refuses_wrong_domains_epoch_and_actual_operation_or_original_input_regression_atomically(
) {
    let seed = genuine_seed(true, 4);
    let before = state(&seed.authority);
    for (epoch, output, cutoff, now, error) in [
        (
            2,
            raw(1400),
            host(200),
            host(350),
            AudioAuthorityError::WrongEpoch,
        ),
        (
            1,
            point(4, 1400),
            host(200),
            host(350),
            AudioAuthorityError::DomainMismatch,
        ),
        (
            1,
            raw(1400),
            point(7, 200),
            host(350),
            AudioAuthorityError::DomainMismatch,
        ),
        (
            1,
            raw(1400),
            host(200),
            point(7, 350),
            AudioAuthorityError::DomainMismatch,
        ),
        (
            1,
            raw(999),
            host(200),
            host(350),
            AudioAuthorityError::ObservationRegression,
        ),
        (
            1,
            raw(1149),
            host(200),
            host(350),
            AudioAuthorityError::OperationRegression,
        ),
        (
            1,
            raw(1400),
            host(124),
            host(350),
            AudioAuthorityError::OperationRegression,
        ),
    ] {
        assert!(
            matches!(seed.authority.prepare_control_cutoff(epoch, output, cutoff, now, &seed.merger), Err(actual) if actual == error)
        );
        assert_eq!(state(&seed.authority), before);
        assert_eq!(seed.merger.pending(), 0);
    }
}

#[test]
fn control_waits_for_ready_original_inputs_but_preserves_later_pending_input() {
    let mut seed = genuine_seed(false, 4);
    let earlier = event(150, 2);
    seed.merger.admit(earlier.clone(), host(250)).unwrap();
    let before = state(&seed.authority);
    assert!(seed
        .authority
        .prepare_control_cutoff(1, raw(1300), host(200), host(250), &seed.merger)
        .unwrap()
        .is_none());
    assert_eq!(state(&seed.authority), before);
    assert_eq!(seed.merger.peek_ready(host(250)).unwrap(), Some(&earlier));
    // Deliver the real earlier event before granting a control operation.
    let input = seed
        .authority
        .prepare_input(host(150), host(250))
        .unwrap()
        .unwrap();
    assert_eq!(
        seed.merger.pop_ready(host(250)).unwrap(),
        Some(earlier.clone())
    );
    let report = seed
        .runtime
        .process_input(earlier, input.mapper(), raw(1400))
        .unwrap();
    assert!(report.input.is_some());
    seed.authority.commit_input(input).unwrap();
    let later = event(225, 3);
    seed.merger.admit(later.clone(), host(250)).unwrap();
    let cutoff = seed
        .authority
        .prepare_control_cutoff(1, raw(1300), host(200), host(250), &seed.merger)
        .unwrap()
        .unwrap();
    let report = seed
        .runtime
        .advance_to(cutoff.output(), &Identity, raw(1400))
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(300));
    seed.authority
        .commit_control_cutoff(cutoff, &seed.merger)
        .unwrap();
    assert_eq!(seed.authority.committed_input_host(), Some(host(150)));
    assert_eq!(seed.merger.pending(), 1);
    assert_eq!(seed.merger.peek_ready(host(250)).unwrap(), Some(&later));
    assert!(seed
        .authority
        .prepare_held_frontier(host(250), &seed.merger)
        .unwrap()
        .is_none());
}

#[test]
fn held_frontiers_close_real_prefix_and_retire_two_slot_history_without_runtime_operations() {
    let mut seed = genuine_seed(false, 2);
    let operation = seed.authority.committed_operation();
    let original_input = seed.authority.committed_input_host();
    let capacity = seed.authority.history.capacity();
    for (index, raw_ns, host_ns, prefix) in [
        (0, 1300, 200, 250),
        (1, 1600, 300, 350),
        (2, 1800, 400, 450),
        (3, 2200, 500, 550),
    ] {
        if index != 0 {
            seed.authority.observe(1, pair(raw_ns, host_ns)).unwrap();
        }
        seed.authority.record_acquired_prefix(host(prefix)).unwrap();
        let before = state(&seed.authority);
        let frontier = seed
            .authority
            .prepare_held_frontier(host(prefix), &seed.merger)
            .unwrap()
            .unwrap();
        assert_eq!(frontier.host(), host(prefix));
        assert_eq!(frontier.observed_host(), host(host_ns));
        assert_eq!(frontier.output(), logical(5000 + raw_ns - 1000));
        assert_eq!(state(&seed.authority), before);
        seed.authority
            .commit_held_frontier(frontier, &mut seed.merger)
            .unwrap();
        assert_eq!(seed.authority.closed_host_prefix(), Some(host(prefix)));
        assert_eq!(
            seed.authority.committed_presentation(),
            Some(logical(5000 + raw_ns - 1000))
        );
        assert_eq!(seed.authority.committed_operation(), operation);
        assert_eq!(seed.authority.committed_input_host(), original_input);
        assert_eq!(seed.authority.history_len(), 2);
        assert_eq!(seed.authority.history.capacity(), capacity);
    }
    assert_eq!(seed.authority.history.front().unwrap().target, host(400));
    let report = seed
        .runtime
        .advance_to(logical(5150), &Identity, raw(2300))
        .unwrap();
    assert_eq!(
        report.song_time,
        Timestamp::from_nanos(150),
        "held servicing did not advance the actual Runtime"
    );
    assert!(seed.merger.admit(event(550, 2), host(600)).is_err());
}

#[test]
fn normal_frontier_still_requires_a_real_report_and_commits_an_operational_advance() {
    let mut seed = genuine_seed(true, 4);
    seed.authority.observe(1, pair(1700, 400)).unwrap();
    seed.authority.record_acquired_prefix(host(450)).unwrap();
    let frontier = seed
        .authority
        .prepare_frontier(host(450), &seed.merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.advance(), Some(logical(5700)));
    let report = seed
        .runtime
        .advance_to(frontier.advance().unwrap(), &Identity, raw(1800))
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(700));
    assert!(report.input.is_none());
    seed.authority
        .commit_frontier(frontier, &mut seed.merger)
        .unwrap();
    assert_eq!(seed.authority.committed_operation(), Some(logical(5700)));
    assert_eq!(seed.authority.committed_presentation(), Some(logical(5700)));
    assert_eq!(seed.authority.committed_input_host(), Some(host(125)));
}

#[test]
fn control_and_held_tokens_reject_changed_or_cross_owner_semantic_state_without_commit_effects() {
    let mut seed = genuine_seed(true, 4);
    let cutoff = seed
        .authority
        .prepare_control_cutoff(1, raw(1400), host(200), host(350), &seed.merger)
        .unwrap()
        .unwrap();
    seed.authority.record_acquired_prefix(host(360)).unwrap();
    let changed = state(&seed.authority);
    assert_eq!(
        seed.authority.commit_control_cutoff(cutoff, &seed.merger),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(state(&seed.authority), changed);
    let mut different = genuine_seed(false, 2);
    let before = state(&different.authority);
    assert_eq!(
        different
            .authority
            .commit_control_cutoff(cutoff, &different.merger),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(state(&different.authority), before);
    let frontier = different
        .authority
        .prepare_held_frontier(host(250), &different.merger)
        .unwrap()
        .unwrap();
    different
        .authority
        .record_acquired_prefix(host(260))
        .unwrap();
    let changed = state(&different.authority);
    let merger_before = format!("{:?}", different.merger);
    assert_eq!(
        different
            .authority
            .commit_held_frontier(frontier, &mut different.merger),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(state(&different.authority), changed);
    assert_eq!(format!("{:?}", different.merger), merger_before);
    let before = state(&seed.authority);
    let merger_before = format!("{:?}", seed.merger);
    assert_eq!(
        seed.authority
            .commit_held_frontier(frontier, &mut seed.merger),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(state(&seed.authority), before);
    assert_eq!(format!("{:?}", seed.merger), merger_before);
}

#[test]
fn control_and_held_preparation_check_revision_and_rebase_overflow_before_permission() {
    let mut seed = genuine_seed(true, 4);
    seed.authority.record_acquired_prefix(host(360)).unwrap();
    seed.authority.revision = u64::MAX;
    let before = state(&seed.authority);
    assert!(matches!(
        seed.authority
            .prepare_control_cutoff(1, raw(1400), host(200), host(360), &seed.merger),
        Err(AudioAuthorityError::Overflow)
    ));
    assert!(matches!(
        seed.authority
            .prepare_held_frontier(host(360), &seed.merger),
        Err(AudioAuthorityError::Overflow)
    ));
    assert_eq!(state(&seed.authority), before);
    let merger = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 2).unwrap();
    let authority = AudioAuthority::new(
        config(2),
        AudioAuthorityEpoch {
            stream_origin: raw(0),
            logical_origin: logical(i64::MAX - 1),
            ..epoch()
        },
    )
    .unwrap();
    let before = state(&authority);
    assert!(matches!(
        authority.prepare_control_cutoff(1, raw(2), host(0), host(0), &merger),
        Err(AudioAuthorityError::Overflow)
    ));
    assert_eq!(state(&authority), before);
}
