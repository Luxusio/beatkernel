use super::*;
use crate::competition_presentation::{CompetitionSnapshot, GhostSnapshot, NetworkSnapshot};
use crate::competition_terminal::{
    finalize_terminal, solo_delivery_intent, CompetitionTerminalPort, DeliveryIntent,
    DeliveryStatus, TerminalGuard,
};
use std::{cell::Cell, collections::VecDeque, sync::Arc};

struct Opaque(Arc<u8>);
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    Delivery,
    Cleanup,
    Drain,
}
struct Port {
    effects: Vec<Effect>,
    failures: [Option<Opaque>; 3],
    sent: Vec<Vec<MemberProgress>>,
    pointers: Vec<usize>,
}
impl CompetitionTerminalPort for Port {
    type Error = Opaque;
    fn deliver(&mut self, rows: &[MemberProgress]) -> std::result::Result<(), Opaque> {
        self.effects.push(Effect::Delivery);
        self.pointers.push(rows.as_ptr() as usize);
        self.sent.push(rows.to_vec());
        self.failures[0].take().map_or(Ok(()), Err)
    }
    fn cleanup(&mut self) -> std::result::Result<(), Opaque> {
        self.effects.push(Effect::Cleanup);
        self.failures[1].take().map_or(Ok(()), Err)
    }
    fn drain(&mut self) -> std::result::Result<(), Opaque> {
        self.effects.push(Effect::Drain);
        self.failures[2].take().map_or(Ok(()), Err)
    }
}
fn port(mask: u8) -> (Port, [Arc<u8>; 3]) {
    let identities = [Arc::new(17), Arc::new(29), Arc::new(43)];
    let failures =
        std::array::from_fn(|i| (mask & (1 << i) != 0).then(|| Opaque(identities[i].clone())));
    (
        Port {
            effects: vec![],
            failures,
            sent: vec![],
            pointers: vec![],
        },
        identities,
    )
}
fn member(player: u32, song: i64) -> MemberProgress {
    MemberProgress {
        player: PlayerId(player),
        progress: Progress {
            song_ns: song,
            hits: u64::MAX,
            misses: 0,
            combo: u64::MAX,
            max_combo: u64::MAX,
        },
    }
}
fn retained<T>(result: &std::result::Result<T, Opaque>, expected: Option<&Arc<u8>>) {
    match (result, expected) {
        (Ok(_), None) => {}
        (Err(Opaque(actual)), Some(expected)) => assert!(Arc::ptr_eq(actual, expected)),
        _ => panic!("terminal outcome must retain the original error for each effect"),
    }
}

#[test]
fn all_solo_delivery_gates_borrow_the_original_member_without_inventing_progress() {
    for row in [
        member(u32::MAX, 9_007_199_254_740_993),
        member(7, -604_800_000_000_001),
    ] {
        for room in [false, true] {
            for failed in [false, true] {
                for ready in [false, true] {
                    for observed in [false, true] {
                        let input = observed.then_some(&row);
                        let intent = solo_delivery_intent::<Opaque>(room, failed, ready, input);
                        if room || (observed && !failed && ready) {
                            match intent {
                                DeliveryIntent::Send(rows) => {
                                    assert_eq!(rows.len(), usize::from(observed));
                                    if observed {
                                        assert_eq!(rows[0], row);
                                        assert_eq!(rows.as_ptr(), &row as *const MemberProgress);
                                    }
                                }
                                _ => panic!("eligible solo delivery must borrow actual data"),
                            }
                        } else {
                            assert!(matches!(intent, DeliveryIntent::Skip));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn one_shot_guard_prevents_repeated_effects_for_every_terminal_failure_mask() {
    for mask in 0..8 {
        let (mut port, identities) = port(mask);
        let row = member(u32::MAX, i64::MAX);
        let mut guard = TerminalGuard::new();
        assert!(!guard.is_claimed());
        assert!(guard.claim());
        let outcome = finalize_terminal(
            &mut port,
            solo_delivery_intent(false, false, true, Some(&row)),
        );
        assert_eq!(outcome.has_failed(), mask != 0);
        retained(&outcome.delivery, (mask & 1 != 0).then_some(&identities[0]));
        retained(&outcome.cleanup, (mask & 2 != 0).then_some(&identities[1]));
        retained(&outcome.drain, (mask & 4 != 0).then_some(&identities[2]));
        if mask & 1 == 0 {
            assert!(matches!(&outcome.delivery, Ok(DeliveryStatus::Accepted)));
        }
        match outcome.into_result() {
            Ok(()) => assert_eq!(mask, 0),
            Err(Opaque(actual)) => {
                let index = (0..3).find(|i| mask & (1 << i) != 0).unwrap();
                assert!(Arc::ptr_eq(&actual, &identities[index]));
            }
        }
        for _ in 0..3 {
            if guard.claim() {
                let _ = finalize_terminal(
                    &mut port,
                    solo_delivery_intent(false, false, true, Some(&row)),
                );
                panic!("a terminal claim must never be reusable");
            }
            assert!(guard.is_claimed());
        }
        assert_eq!(
            port.effects,
            [Effect::Delivery, Effect::Cleanup, Effect::Drain]
        );
        assert_eq!(port.sent, [vec![row]]);
        assert_eq!(port.pointers, [&row as *const MemberProgress as usize]);
    }
    let mut default = TerminalGuard::default();
    assert!(!default.is_claimed());
    assert!(default.claim());
    assert!(!default.claim());
}

#[test]
fn unobserved_room_cancellation_is_accepted_while_bilateral_delivery_is_skipped() {
    for room in [false, true] {
        let (mut port, _) = port(0);
        let outcome = finalize_terminal(&mut port, solo_delivery_intent(room, true, false, None));
        assert!(
            matches!(outcome.delivery, Ok(status) if status == if room {DeliveryStatus::Accepted} else {DeliveryStatus::Skipped})
        );
        assert_eq!(port.sent.len(), usize::from(room));
        if room {
            assert!(port.sent[0].is_empty());
        }
        assert_eq!(
            port.effects,
            if room {
                vec![Effect::Delivery, Effect::Cleanup, Effect::Drain]
            } else {
                vec![Effect::Cleanup, Effect::Drain]
            }
        );
        assert!(outcome.cleanup.is_ok());
        assert!(outcome.drain.is_ok());
    }
}

fn offline() -> (LiveCompetition, JudgeEngine) {
    use beatkernel::judge::{JudgeGrade, JudgeProfile, JudgeWindow};
    let source = parse_seeded(
        "#BPM 60\n#WAV01 key.wav\n#00011:01",
        ParseOptions::default(),
        0,
    )
    .unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: beatkernel::time::Duration::ZERO,
                late: beatkernel::time::Duration::ZERO,
            }],
            beatkernel::time::Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let header = LiveReplayCapture::new(&judge, ClockDomainId(17), replay_limits().unwrap())
        .unwrap()
        .into_file()
        .header;
    (
        LiveCompetition {
            admitted_policy_header: None,
            player: PlayerId(u32::MAX),
            competition: Competition::new(header, 0).unwrap(),
            network: None,
            last_publish: None,
            last_display: None,
            network_failed: false,
            network_status: None,
            last_presentation: None,
            network_setup_timeout: Duration::from_secs(10),
            terminal: TerminalGuard::new(),
        },
        judge,
    )
}
struct Host {
    attached: Cell<usize>,
    times: VecDeque<u64>,
    reads: usize,
    saved: Vec<(PlayerId, Vec<GhostSnapshot>)>,
    refuse: bool,
}
impl Host {
    fn new(refuse: bool) -> Self {
        Self {
            attached: Cell::new(0),
            times: [101, 103].into(),
            reads: 0,
            saved: vec![],
            refuse,
        }
    }
}
impl CompetitionPresentationHost for Host {
    fn attached(&self) -> bool {
        self.attached.set(self.attached.get() + 1);
        true
    }
    fn now_ns(&mut self) -> Result<u64> {
        self.reads += 1;
        Ok(self
            .times
            .pop_front()
            .expect("unexpected presentation clock read"))
    }
    fn publish_saved(&mut self, player: PlayerId, ghosts: Vec<GhostSnapshot>) -> Result<()> {
        self.saved.push((player, ghosts));
        if self.refuse {
            Err("injected offline presentation refusal".into())
        } else {
            Ok(())
        }
    }
    fn publish_solo(&mut self, _: PlayerId, _: CompetitionSnapshot) -> Result<()> {
        panic!("offline owner cannot publish network state")
    }
    fn publish_group(&mut self, _: &[(PlayerId, NetworkSnapshot)]) -> Result<()> {
        panic!("solo owner cannot publish cohort state")
    }
}

#[test]
fn actual_offline_owner_finishes_once_even_after_presentation_refusal_without_completion_proof() {
    for refuse in [false, true] {
        for observed in [false, true] {
            let (mut owner, _) = offline();
            if observed {
                owner
                    .competition
                    .observe(&[], Timestamp::from_nanos(604_800_000_000_001))
                    .unwrap();
            }
            let prefix = owner.terminal_prefix();
            let mut host = Host::new(refuse);
            owner.finish_with_presentation(&mut host);
            assert!(owner.terminal.is_claimed());
            assert!(!owner.native_completed());
            assert_eq!(owner.terminal_prefix(), prefix);
            assert_eq!(host.saved, [(PlayerId(u32::MAX), vec![])]);
            assert_eq!(host.attached.get(), 1);
            assert_eq!(host.reads, if refuse { 1 } else { 2 });
            assert_eq!(
                owner.last_presentation,
                if refuse { None } else { Some(103) }
            );
            for _ in 0..3 {
                owner.finish_with_presentation(&mut host);
            }
            assert_eq!(host.saved.len(), 1);
            assert_eq!(host.attached.get(), 1);
            assert_eq!(host.reads, if refuse { 1 } else { 2 });
            assert!(!owner.native_completed());
        }
    }
}

struct NoControl;
impl crate::native_pump_control::NativePumpControl for NoControl {
    type Moment = u64;
    fn now(&mut self) -> Result<u64> {
        panic!("claimed owner must not acquire control time")
    }
    fn checked_add(_: u64, _: Duration) -> Option<u64> {
        panic!("claimed owner must not prepare a deadline")
    }
    fn wait(&mut self, _: Duration) -> Result<()> {
        panic!("claimed owner must not wait")
    }
}
impl CompetitionSetupControl for NoControl {
    fn remaining_duration(_: u64, _: u64) -> Duration {
        panic!("claimed owner must not compute remaining time")
    }
}

#[test]
fn actual_late_report_and_start_requests_refuse_before_judge_host_control_or_service_effects() {
    use beatkernel::{
        audio::command_queue,
        input::BindingMap,
        runtime::{Runtime, RuntimeProcessingClock},
        time::{ClockMapper, ClockMappingQuality, ClockPoint},
        transport::{Rate, Transport},
    };
    struct Identity;
    impl ClockMapper for Identity {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            (from.domain == to).then_some(from.timestamp)
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Exact
        }
    }
    let (mut owner, judge) = offline();
    let (producer, _consumer) = command_queue(1).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(17),
        ClockDomainId(17),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        BindingMap::from_bindings([]).unwrap(),
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let point = ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(1),
    };
    let report = runtime.advance_to(point, &Identity, point).unwrap();
    assert_eq!(report.judge_events.len(), 1);
    let mut host = Host::new(false);
    owner.finish_with_presentation(&mut host);
    assert!(owner.observe_with_presentation(&report, &mut host).is_err());
    assert_eq!(owner.competition.song_time(), None);
    assert_eq!(owner.competition.score().misses, 0);
    assert_eq!(owner.last_display, None);
    assert_eq!(owner.last_publish, None);
    for release in [false, true] {
        assert!(owner
            .await_network_start_with_ports(
                || panic!("claimed owner must not service acquisition"),
                release,
                &mut NoControl,
                &mut host
            )
            .is_err());
    }
    assert_eq!(host.saved.len(), 1);
    assert_eq!(host.reads, 2);
    assert_eq!(host.attached.get(), 1);
    assert_eq!(owner.network_status, None);
    assert!(!owner.network_failed);
    assert!(!owner.native_completed());
}
