//! Fresh phase installation is mathematical state replacement, not native admission.
use super::*;
use beatkernel::{
    audio::{CommandConsumer, QueuePopError, SampleId, SourcePosition},
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{Binding, ButtonEvent, ButtonState, EventMeta, GameControlId, PhysicalControlId},
    interaction::PressInstantEvaluator,
    judge::{JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    time::{ClockMappingQuality, Duration},
    transport::OutputPhaseBasis,
};

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn clock(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
fn basis(generation: u64, origin: u64) -> OutputPhaseBasis {
    OutputPhaseBasis::new(ClockDomainId(1), 17, generation, origin, 1000).unwrap()
}
fn transport(b: OutputPhaseBasis, anchor: i64, rate: Rate) -> Transport {
    // A candidate must be internally valid even when its domain differs from
    // the retained owner; replacement, not construction, rejects that mismatch.
    let logical_origin = ClockPoint {
        domain: b.domain(),
        timestamp: ts(anchor),
    };
    Transport::new_with_output_phase(b, logical_origin, ts(0), Duration::ZERO, rate, None).unwrap()
}
fn phase(b: OutputPhaseBasis, logical: i64, frame: u64) -> OutputPhasePoint {
    b.point(
        clock(1, logical),
        SourcePosition {
            frame,
            numerator: 0,
            denominator: 1,
        },
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
        meta: EventMeta::new(DeviceId(u64::from(player)), clock(9, host), 1),
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    })
}
fn member(player: u32, song: i64, grade: u32) -> MemberConfig {
    let mut chart = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    chart.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(song).unwrap(),
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
            chart.compile().unwrap(),
            vec![Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(PressInstantEvaluator),
            }],
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(grade),
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
            gain: 0.5,
        }],
    }
}
fn group(ids: &[u32], rate: Rate) -> (RuntimeGroup, CommandConsumer) {
    let (producer, consumer) = command_queue(32).unwrap();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(basis(1, 2), 1000, rate),
        producer,
        ids.iter().map(|id| member(*id, 3_000_000, 7)).collect(),
        8,
        &[],
    )
    .unwrap();
    group.set_audio_scope(CommandScope(1));
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    (group, consumer)
}
fn judges(ids: &[u32]) -> Vec<(PlayerId, JudgeEngine)> {
    ids.iter()
        .map(|id| (PlayerId(*id), member(*id, 3_000_000, 7).judge))
        .collect()
}
fn hit_frame(rate: Rate, origin: u64) -> u64 {
    origin + if rate == Rate::NORMAL { 3 } else { 2 }
}
fn hit(
    group: &mut RuntimeGroup,
    player: u32,
    host: i64,
    b: OutputPhaseBasis,
    rate: Rate,
    origin: u64,
) {
    let InputResult::Processed(reports) = group
        .process_input_output_phase(
            input(player, host),
            &Mapper,
            clock(2, host + 700),
            phase(b, host + 1000, hit_frame(rate, origin)),
        )
        .unwrap()
    else {
        panic!("assigned input was ignored")
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].player, PlayerId(player));
    assert_eq!(reports[0].report.song_time, ts(3_000_000));
    assert_eq!(reports[0].report.judge_events.len(), 1);
}

#[test]
fn practice_phase_replacement_preserves_positive_speed_shared_producer_and_member_routes() {
    for ids in [&[1][..], &[7, 29][..], &[7, 29, 101, 409][..]] {
        for rate in [Rate::NORMAL, Rate::new(3, 2).unwrap()] {
            let (mut group, mut queue) = group(ids, rate);
            for &id in ids {
                hit(&mut group, id, 100, basis(1, 2), rate, 2);
                group.fence_player(PlayerId(id)).unwrap();
            }
            group.seal_gameplay_audio();
            group
                .replace_practice_states_with_output_phase(
                    judges(ids),
                    transport(basis(2, 10), 1200, rate),
                    Some(ts(9_000_000)),
                    CommandScope(2),
                )
                .unwrap();
            assert_eq!(group.transport().output_phase_basis(), Some(basis(2, 10)));
            assert_eq!(group.transport().anchor().rate, rate);
            assert_eq!(group.audio_scope(), CommandScope(2));
            assert_eq!(group.song_end(), Some(ts(9_000_000)));
            assert!(!group.started && !group.poisoned());
            assert!(!group.gameplay_audio_sealed());
            for &id in ids {
                assert_eq!(group.player_gameplay_fence(PlayerId(id)), None);
                assert!(group
                    .member_judge(PlayerId(id))
                    .unwrap()
                    .effective_song_time()
                    .is_none());
                hit(&mut group, id, 300, basis(2, 10), rate, 10);
            }
            let reports = group
                .advance_to_output_phase(
                    clock(9, 301),
                    &Mapper,
                    clock(2, 1001),
                    phase(basis(2, 10), 1301, hit_frame(rate, 10)),
                )
                .unwrap();
            assert_eq!(reports.len(), ids.len());
            assert!(reports
                .iter()
                .all(|report| report.report.song_time == ts(3_000_000)));
            // Old and new hits both pass through the original consumer and full sound plan.
            for _ in 0..2 {
                for &id in ids {
                    assert!(matches!(queue.try_pop().unwrap(), AudioCommand::Play {
                        voice, sample: SampleId(1), gain: 0.5, ..
                    } if voice == VoiceId(u64::from(id))));
                }
            }
            assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
            assert_eq!(group.admitted_audio_commands(), (ids.len() * 2) as u64);
            assert!(group.set_song_end(ts(10_000_000)).is_err());
        }
    }
}

#[test]
fn solo_phase_replacement_forwards_to_the_real_single_member_transaction() {
    let rate = Rate::new(3, 2).unwrap();
    let config = member(1, 3_000_000, 7);
    let (producer, mut queue) = command_queue(8).unwrap();
    let mut solo = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(basis(1, 2), 1000, rate),
        config.bindings,
        config.judge,
        producer,
        config.sounds,
        8,
    )
    .unwrap();
    solo.set_audio_scope(CommandScope(1));
    solo.replace_practice_state_with_output_phase(
        member(1, 3_000_000, 7).judge,
        transport(basis(2, 10), 1200, rate),
        None,
        CommandScope(2),
    )
    .unwrap();
    let report = solo
        .process_input_output_phase(
            input(1, 300),
            &Mapper,
            clock(2, 777),
            phase(basis(2, 10), 1300, 12),
        )
        .unwrap();
    assert_eq!(report.song_time, ts(3_000_000));
    assert_eq!(report.judge_events.len(), 1);
    assert_eq!(solo.audio_scope(), CommandScope(2));
    assert!(
        matches!(queue.try_pop().unwrap(), AudioCommand::Play { voice: VoiceId(1), at, .. } if at == ts(777))
    );
    assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
}

fn assert_unchanged(group: &RuntimeGroup, before: &Transport, hashes: &[u64], admitted: u64) {
    assert_eq!(group.transport(), before);
    assert_eq!(group.audio_scope(), CommandScope(1));
    assert_eq!(group.song_end(), Some(ts(9_000_000)));
    assert_eq!(group.admitted_audio_commands(), admitted);
    assert!(!group.poisoned());
    for (member, hash) in group.members.iter().zip(hashes) {
        assert_eq!(member.runtime.judge().stable_hash().unwrap(), *hash);
    }
}

#[test]
fn phase_identity_generation_origin_anchor_and_selected_rate_refuse_before_mutation() {
    for case in 0..12 {
        let rate = Rate::new(3, 2).unwrap();
        let (mut group, mut queue) = group(&[7, 29, 101, 409], rate);
        group.set_song_end(ts(9_000_000)).unwrap();
        let mut b = basis(2, 10);
        let mut anchor = 1200;
        let mut selected = rate;
        let mut scope = CommandScope(2);
        match case {
            0 => b = OutputPhaseBasis::new(ClockDomainId(1), 18, 2, 10, 1000).unwrap(),
            1 => b = OutputPhaseBasis::new(ClockDomainId(3), 17, 2, 10, 1000).unwrap(),
            2 => b = OutputPhaseBasis::new(ClockDomainId(1), 17, 2, 10, 1001).unwrap(),
            3 => b = basis(3, 10),
            4 => {
                b = basis(1, 10);
                scope = CommandScope(1);
            }
            5 => b = basis(2, 1),
            6 => anchor = 999,
            7 => selected = Rate::NORMAL,
            8 => scope = CommandScope(0),
            9 => scope = CommandScope(1),
            _ => {}
        }
        let replacement = if case == 10 {
            Transport::new(ts(1200), ts(0), rate)
        } else {
            transport(b, anchor, selected)
        };
        if case == 11 {
            // Actual public scalar construction establishes the missing old phase case.
            let (producer, consumer) = command_queue(32).unwrap();
            queue = consumer;
            group = RuntimeGroup::new(
                ClockDomainId(1),
                ClockDomainId(2),
                Transport::new(ts(1000), ts(0), rate),
                producer,
                [7, 29, 101, 409]
                    .into_iter()
                    .map(|id| member(id, 3_000_000, 7))
                    .collect(),
                8,
                &[],
            )
            .unwrap();
            group.set_audio_scope(CommandScope(1));
            group.set_song_end(ts(9_000_000)).unwrap();
        }
        let before = group.transport().clone();
        let hashes: Vec<_> = group
            .members
            .iter()
            .map(|m| m.runtime.judge().stable_hash().unwrap())
            .collect();
        let admitted = group.admitted_audio_commands();
        assert!(
            group
                .replace_practice_states_with_output_phase(
                    judges(&[7, 29, 101, 409]),
                    replacement,
                    Some(ts(9_000_000)),
                    scope
                )
                .is_err(),
            "case {case}"
        );
        assert_unchanged(&group, &before, &hashes, admitted);
        assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
    }
}

#[test]
fn phase_cohort_preflight_rejects_roster_judge_policy_identity_endpoint_and_nonfresh_anchor() {
    for case in 0..9 {
        let rate = Rate::new(3, 2).unwrap();
        let (mut group, mut queue) = group(&[7, 29], rate);
        group.set_song_end(ts(9_000_000)).unwrap();
        let before = group.transport().clone();
        let hashes: Vec<_> = group
            .members
            .iter()
            .map(|m| m.runtime.judge().stable_hash().unwrap())
            .collect();
        let mut fresh = judges(&[7, 29]);
        let mut replacement = transport(basis(2, 10), 1200, rate);
        let mut end = Some(ts(9_000_000));
        match case {
            0 => {
                fresh.pop();
            }
            1 => fresh.swap(0, 1),
            2 => {
                fresh[1].1.advance_to(ts(0)).unwrap();
            }
            3 => fresh[1].1 = member(29, 3_000_000, 8).judge,
            4 => fresh[1].1 = member(29, 3_000_001, 7).judge,
            5 => end = Some(ts(-1)),
            6 => end = Some(ts(0)),
            7 => replacement
                .pause_at_output_phase(phase(basis(2, 10), 1201, 10))
                .unwrap(),
            _ => fresh.push((PlayerId(101), member(101, 3_000_000, 7).judge)),
        }
        assert!(
            group
                .replace_practice_states_with_output_phase(fresh, replacement, end, CommandScope(2))
                .is_err(),
            "case {case}"
        );
        assert_unchanged(&group, &before, &hashes, 0);
        assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
    }
}

#[test]
fn legacy_practice_api_keeps_non_normal_rate_refusal_and_does_not_install_phase() {
    let rate = Rate::new(3, 2).unwrap();
    let (mut group, mut queue) = group(&[7, 29], rate);
    group.set_song_end(ts(9_000_000)).unwrap();
    let before = group.transport().clone();
    let hashes: Vec<_> = group
        .members
        .iter()
        .map(|m| m.runtime.judge().stable_hash().unwrap())
        .collect();
    assert!(group
        .replace_practice_states(
            judges(&[7, 29]),
            transport(basis(2, 10), 1200, rate),
            None,
            CommandScope(2)
        )
        .is_err());
    assert_unchanged(&group, &before, &hashes, 0);
    assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
}

#[test]
fn malformed_and_nonpositive_phase_construction_cannot_reach_replacement() {
    let (group, mut queue) = group(&[1], Rate::NORMAL);
    let before = group.transport().clone();
    for rate in [Rate::new(0, 1).unwrap(), Rate::new(-1, 1).unwrap()] {
        assert!(Transport::new_with_output_phase(
            basis(2, 10),
            clock(1, 1200),
            ts(0),
            Duration::ZERO,
            rate,
            None
        )
        .is_err());
    }
    assert!(Transport::new_with_output_phase(
        basis(2, 10),
        clock(3, 1200),
        ts(0),
        Duration::ZERO,
        Rate::NORMAL,
        None
    )
    .is_err());
    assert_eq!(group.transport(), &before);
    assert_eq!(group.audio_scope(), CommandScope(1));
    assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
}

fn runtime_transport(b: OutputPhaseBasis, anchor: i64, rate: Rate) -> Transport {
    Transport::new_with_runtime_output_phase(
        b,
        ClockPoint {
            domain: b.domain(),
            timestamp: ts(anchor),
        },
        ts(0),
        Duration::ZERO,
        rate,
        None,
    )
    .unwrap()
}
fn runtime_group(ids: &[u32], paused: bool) -> (RuntimeGroup, CommandConsumer) {
    let mut transport = runtime_transport(basis(1, 2), 1000, Rate::NORMAL);
    let cut = phase(basis(1, 2), 1100, 3);
    transport
        .set_rate_at_output_phase(cut, Rate::new(3, 2).unwrap())
        .unwrap();
    if paused {
        transport.pause_at_output_phase(cut).unwrap();
    }
    let (producer, consumer) = command_queue(32).unwrap();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport,
        producer,
        ids.iter().map(|id| member(*id, 3_000_000, 7)).collect(),
        8,
        &[],
    )
    .unwrap();
    group.set_audio_scope(CommandScope(1));
    group.set_song_end(ts(9_000_000)).unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    (group, consumer)
}

#[test]
fn runtime_region_replacement_preserves_current_rate_even_when_paused_anchor_has_zero_rate() {
    for paused in [false, true] {
        let (mut group, mut queue) = runtime_group(&[7, 29], paused);
        let current = Rate::new(3, 2).unwrap();
        assert_eq!(group.transport().runtime_output_rate(), Some(current));
        if paused {
            assert_eq!(group.transport().anchor().rate, Rate::new(0, 1).unwrap());
        }
        let before = group.transport().clone();
        let hashes: Vec<_> = group
            .members
            .iter()
            .map(|m| m.runtime.judge().stable_hash().unwrap())
            .collect();
        assert!(
            group
                .replace_practice_states_with_output_phase(
                    judges(&[7, 29]),
                    runtime_transport(basis(2, 10), 1200, Rate::NORMAL),
                    Some(ts(9_000_000)),
                    CommandScope(2),
                )
                .is_err(),
            "a region cannot restore the immutable original rate"
        );
        assert_unchanged(&group, &before, &hashes, 0);
        group
            .replace_practice_states_with_output_phase(
                judges(&[7, 29]),
                runtime_transport(basis(2, 10), 1200, current),
                Some(ts(9_000_000)),
                CommandScope(2),
            )
            .unwrap();
        assert_eq!(group.transport().runtime_output_rate(), Some(current));
        assert_eq!(group.transport().output_phase_basis(), Some(basis(2, 10)));
        assert!(!group.transport().is_paused());
        assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
    }
}

#[test]
fn runtime_restart_rate_restore_keeps_all_original_identity_and_freshness_guards() {
    for case in 0..10 {
        let (mut group, mut queue) = runtime_group(&[7, 29], false);
        let before = group.transport().clone();
        let hashes: Vec<_> = group
            .members
            .iter()
            .map(|m| m.runtime.judge().stable_hash().unwrap())
            .collect();
        let mut b = basis(2, 10);
        let mut fresh = judges(&[7, 29]);
        let mut rate = Rate::NORMAL;
        let mut scope = CommandScope(2);
        match case {
            0 => b = OutputPhaseBasis::new(ClockDomainId(1), 18, 2, 10, 1000).unwrap(),
            1 => b = OutputPhaseBasis::new(ClockDomainId(3), 17, 2, 10, 1000).unwrap(),
            2 => b = OutputPhaseBasis::new(ClockDomainId(1), 17, 2, 10, 1001).unwrap(),
            3 => b = basis(3, 10),
            4 => {
                fresh.pop();
            }
            5 => fresh.swap(0, 1),
            6 => fresh[1].1 = member(29, 3_000_000, 8).judge,
            7 => {
                fresh[1].1.advance_to(ts(0)).unwrap();
            }
            8 => rate = Rate::new(1, 2).unwrap(),
            9 => scope = CommandScope(1),
            _ => unreachable!(),
        }
        assert!(
            group
                .replace_practice_states_with_runtime_restart(
                    fresh,
                    runtime_transport(b, 1200, rate),
                    Some(ts(9_000_000)),
                    scope,
                    Rate::NORMAL,
                )
                .is_err(),
            "runtime compound restart rejection case {case}"
        );
        assert_unchanged(&group, &before, &hashes, 0);
        assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
    }
    let (mut group, _) = runtime_group(&[7, 29], false);
    group
        .replace_practice_states_with_runtime_restart(
            judges(&[7, 29]),
            runtime_transport(basis(2, 10), 1200, Rate::NORMAL),
            Some(ts(9_000_000)),
            CommandScope(2),
            Rate::NORMAL,
        )
        .unwrap();
    assert_eq!(group.transport().runtime_output_rate(), Some(Rate::NORMAL));
    assert_eq!(group.audio_scope(), CommandScope(2));
}

#[test]
fn fixed_and_runtime_phase_modes_cannot_mix_in_region_or_compound_restart() {
    for old_runtime in [false, true] {
        let (mut group, _) = if old_runtime {
            runtime_group(&[7, 29], false)
        } else {
            let (mut group, queue) = group(&[7, 29], Rate::new(3, 2).unwrap());
            group.set_song_end(ts(9_000_000)).unwrap();
            (group, queue)
        };
        let before = group.transport().clone();
        let hashes: Vec<_> = group
            .members
            .iter()
            .map(|m| m.runtime.judge().stable_hash().unwrap())
            .collect();
        let candidate = if old_runtime {
            transport(basis(2, 10), 1200, Rate::new(3, 2).unwrap())
        } else {
            runtime_transport(basis(2, 10), 1200, Rate::new(3, 2).unwrap())
        };
        assert!(group
            .replace_practice_states_with_output_phase(
                judges(&[7, 29]),
                candidate.clone(),
                Some(ts(9_000_000)),
                CommandScope(2),
            )
            .is_err());
        assert_unchanged(&group, &before, &hashes, 0);
        assert!(group
            .replace_practice_states_with_runtime_restart(
                judges(&[7, 29]),
                candidate,
                Some(ts(9_000_000)),
                CommandScope(2),
                Rate::new(3, 2).unwrap(),
            )
            .is_err());
        assert_unchanged(&group, &before, &hashes, 0);
    }
}

#[test]
fn solo_runtime_compound_restart_forwards_original_rate_and_keeps_the_same_producer() {
    let config = member(1, 3_000_000, 7);
    let mut changed = runtime_transport(basis(1, 2), 1000, Rate::NORMAL);
    changed
        .set_rate_at_output_phase(phase(basis(1, 2), 1100, 3), Rate::new(3, 2).unwrap())
        .unwrap();
    let (producer, mut queue) = command_queue(8).unwrap();
    let mut solo = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        changed,
        config.bindings,
        config.judge,
        producer,
        config.sounds,
        8,
    )
    .unwrap();
    solo.set_audio_scope(CommandScope(1));
    solo.replace_practice_state_with_runtime_restart(
        member(1, 3_000_000, 7).judge,
        runtime_transport(basis(2, 10), 1200, Rate::NORMAL),
        None,
        CommandScope(2),
        Rate::NORMAL,
    )
    .unwrap();
    let report = solo
        .process_input_output_phase(
            input(1, 300),
            &Mapper,
            clock(2, 777),
            phase(basis(2, 10), 1300, 13),
        )
        .unwrap();
    assert_eq!(report.song_time, ts(3_000_000));
    assert_eq!(report.judge_events.len(), 1);
    assert_eq!(
        solo.transport_mut().runtime_output_rate(),
        Some(Rate::NORMAL)
    );
    assert!(matches!(
        queue.try_pop().unwrap(),
        AudioCommand::Play {
            voice: VoiceId(1),
            ..
        }
    ));
    assert_eq!(queue.try_pop(), Err(QueuePopError::Empty));
}
