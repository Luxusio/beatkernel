//! Deferred pure competition display policy; no native IO, UI or clock reads.
use crate::{
    competition::{Competition, OpponentKind},
    competition_presentation::{
        CompetitionPresentationHost, CompetitionSnapshot, GhostSnapshot, NetworkSnapshot,
        NetworkStatus, PresentationResult, SoloNetworkPresentation, display_basename,
        publication_due, publish_group, publish_solo, remote_members, selected_remote_member,
    },
    local_players::PlayerId,
    multiplayer::Progress,
    multiplayer_group::{GroupPrefix, MemberProgress},
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    input::{
        ButtonEvent, ButtonState, CodecLimits, DeviceId, EventMeta, GameInputEvent,
        PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::{
        ReplaySession,
        codec::{ReplayCodecLimits, ReplayFile},
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn member(player: u32, song_ns: i64, hits: u64) -> MemberProgress {
    MemberProgress {
        player: PlayerId(player),
        progress: Progress {
            song_ns,
            hits,
            misses: 0,
            combo: hits,
            max_combo: hits,
        },
    }
}
fn prefix(members: Vec<MemberProgress>) -> GroupPrefix {
    GroupPrefix {
        sequence: u64::MAX,
        final_prefix: false,
        members,
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(1 << 20, 100, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn competition(labels: &[(&str, OpponentKind)]) -> Competition {
    let source = beatkernel_bms::parse(
        "#BPM 60\n#WAV01 tap.wav\n#00011:00010001\n",
        Default::default(),
    )
    .unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let domain = ClockDomainId(23);
    let header = LiveReplayCapture::new(&judge, domain, limits())
        .unwrap()
        .header()
        .clone();
    let mut replay = ReplaySession::new(header.clone(), judge).unwrap();
    replay
        .push_input(
            GameInputEvent {
                game_control: source.notes[0].lane.control(),
                physical: PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(
                        DeviceId(u64::MAX),
                        ClockPoint {
                            domain,
                            timestamp: ts(1_000_000_000),
                        },
                        u64::MAX,
                    ),
                    control: PhysicalControlId::keyboard(4),
                    state: ButtonState::Down,
                }),
            },
            ts(1_000_000_000),
        )
        .unwrap();
    let file = ReplayFile::new(header.clone(), replay.records().to_vec());
    let mut competition = Competition::new(header, labels.len()).unwrap();
    for &(label, kind) in labels {
        competition
            .add_replay(&source, file.clone(), limits(), kind, label)
            .unwrap();
    }
    // Display time can exceed a truncated recording without inventing misses.
    competition.observe(&[], ts(604_800_000_000_000)).unwrap();
    competition
}

#[derive(Debug)]
struct Refusal;
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("fake presentation refused")
    }
}
impl std::error::Error for Refusal {}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Publication {
    Saved(PlayerId, Vec<GhostSnapshot>),
    Solo(PlayerId, CompetitionSnapshot),
    Group(Vec<(PlayerId, NetworkSnapshot)>),
}
struct Host {
    attached: bool,
    now: u64,
    clock_reads: usize,
    attempts: usize,
    reject: bool,
    clock_reject: bool,
    effect_delay: u64,
    post_effect_now: Option<u64>,
    post_effect_clock_reject: bool,
    publications: Vec<Publication>,
}
impl Default for Host {
    fn default() -> Self {
        Self {
            attached: true,
            now: 0,
            clock_reads: 0,
            attempts: 0,
            reject: false,
            clock_reject: false,
            effect_delay: 0,
            post_effect_now: None,
            post_effect_clock_reject: false,
            publications: Vec::new(),
        }
    }
}
impl Host {
    fn publish(&mut self, publication: Publication) -> PresentationResult<()> {
        self.attempts += 1;
        if self.reject {
            return Err(Box::new(Refusal));
        }
        self.publications.push(publication);
        self.now = self
            .post_effect_now
            .unwrap_or(self.now.checked_add(self.effect_delay).unwrap());
        self.clock_reject |= self.post_effect_clock_reject;
        Ok(())
    }
}
impl CompetitionPresentationHost for Host {
    fn attached(&self) -> bool {
        self.attached
    }
    fn now_ns(&mut self) -> PresentationResult<u64> {
        self.clock_reads += 1;
        if self.clock_reject {
            Err(Box::new(Refusal))
        } else {
            Ok(self.now)
        }
    }
    fn publish_saved(
        &mut self,
        player: PlayerId,
        ghosts: Vec<GhostSnapshot>,
    ) -> PresentationResult<()> {
        self.publish(Publication::Saved(player, ghosts))
    }
    fn publish_solo(
        &mut self,
        player: PlayerId,
        snapshot: CompetitionSnapshot,
    ) -> PresentationResult<()> {
        self.publish(Publication::Solo(player, snapshot))
    }
    fn publish_group(&mut self, rows: &[(PlayerId, NetworkSnapshot)]) -> PresentationResult<()> {
        self.publish(Publication::Group(rows.to_vec()))
    }
}
fn network<'a>(
    status: NetworkStatus,
    roster: &'a [PlayerId],
    prefix: &'a GroupPrefix,
) -> SoloNetworkPresentation<'a> {
    SoloNetworkPresentation {
        status: Some(status),
        roster: Some(roster),
        prefix: Some(prefix),
    }
}

#[test]
fn exact_cadence_forced_transition_and_regression_keep_successful_clock_only() {
    let competition = competition(&[]);
    let mut host = Host {
        now: 123,
        ..Default::default()
    };
    let mut last = None;
    publish_solo(
        &mut host,
        &mut last,
        false,
        PlayerId(u32::MAX),
        &competition,
        None,
    )
    .unwrap();
    assert_eq!(last, Some(123));
    host.now = 50_000_122;
    publish_solo(
        &mut host,
        &mut last,
        false,
        PlayerId(u32::MAX),
        &competition,
        None,
    )
    .unwrap();
    assert_eq!(host.attempts, 1);
    assert_eq!(last, Some(123));
    host.now = 50_000_123;
    publish_solo(
        &mut host,
        &mut last,
        false,
        PlayerId(u32::MAX),
        &competition,
        None,
    )
    .unwrap();
    assert_eq!(host.attempts, 2);
    assert_eq!(last, Some(50_000_123));
    host.now = 50_000_124;
    publish_solo(
        &mut host,
        &mut last,
        true,
        PlayerId(u32::MAX),
        &competition,
        None,
    )
    .unwrap();
    assert_eq!(host.attempts, 3);
    assert_eq!(last, Some(50_000_124));
    let before = host.publications.clone();
    host.now = 50_000_123;
    for force in [false, true] {
        assert!(
            publish_solo(
                &mut host,
                &mut last,
                force,
                PlayerId(u32::MAX),
                &competition,
                None
            )
            .is_err()
        );
        assert_eq!(last, Some(50_000_124));
        assert_eq!(host.publications, before);
        assert_eq!(host.attempts, 3);
    }
    host.now = u64::MAX;
    assert_eq!(
        publication_due(&mut host, last, false).unwrap(),
        Some(u64::MAX)
    );
    assert_eq!(last, Some(50_000_124));
}

#[test]
fn unattached_room_and_cadence_suppression_skip_clock_or_invalid_projection() {
    let competition = competition(&[]);
    let invalid = prefix(vec![member(0, 0, 0)]);
    let mut host = Host {
        attached: false,
        clock_reject: true,
        ..Default::default()
    };
    let mut last = Some(7);
    publish_solo(
        &mut host,
        &mut last,
        true,
        PlayerId(9),
        &competition,
        Some(network(NetworkStatus::Connected, &[PlayerId(0)], &invalid)),
    )
    .unwrap();
    publish_group(
        &mut host,
        &mut last,
        true,
        false,
        &[],
        NetworkStatus::Connected,
        Some(&[PlayerId(0)]),
        Some(&invalid),
    )
    .unwrap();
    assert_eq!((host.clock_reads, host.attempts, last), (0, 0, Some(7)));
    host.attached = true;
    publish_group(
        &mut host,
        &mut last,
        true,
        true,
        &[],
        NetworkStatus::Connected,
        Some(&[PlayerId(0)]),
        Some(&invalid),
    )
    .unwrap();
    assert_eq!((host.clock_reads, host.attempts, last), (0, 0, Some(7)));
    host.clock_reject = false;
    host.now = 50_000_006;
    // An unsuppressed group would reject these rows; no projection occurs yet.
    publish_group(
        &mut host,
        &mut last,
        false,
        false,
        &[],
        NetworkStatus::Connected,
        Some(&[PlayerId(0)]),
        Some(&invalid),
    )
    .unwrap();
    assert_eq!((host.clock_reads, host.attempts, last), (1, 0, Some(7)));
    host.now = 50_000_007;
    assert!(
        publish_group(
            &mut host,
            &mut last,
            false,
            false,
            &[],
            NetworkStatus::Connected,
            Some(&[PlayerId(0)]),
            Some(&invalid)
        )
        .is_err()
    );
    assert_eq!((host.attempts, last), (0, Some(7)));
}

#[test]
fn refused_effect_and_clock_error_leave_local_progress_retryable_without_cadence_commit() {
    let mut competition = competition(&[]);
    competition.observe(&[], ts(604_800_000_000_001)).unwrap();
    let mut host = Host {
        now: 99,
        reject: true,
        ..Default::default()
    };
    let mut last = None;
    let error =
        publish_solo(&mut host, &mut last, false, PlayerId(7), &competition, None).unwrap_err();
    assert!(error.downcast_ref::<Refusal>().is_some());
    assert_eq!(last, None);
    assert_eq!(competition.song_time(), Some(ts(604_800_000_000_001)));
    assert!(host.publications.is_empty());
    host.reject = false;
    publish_solo(&mut host, &mut last, false, PlayerId(7), &competition, None).unwrap();
    assert_eq!((host.attempts, last), (2, Some(99)));
    host.now = 50_000_099;
    host.clock_reject = true;
    let before = host.publications.clone();
    let error =
        publish_solo(&mut host, &mut last, true, PlayerId(7), &competition, None).unwrap_err();
    assert!(error.downcast_ref::<Refusal>().is_some());
    assert_eq!(last, Some(99));
    assert_eq!(host.publications, before);
    assert_eq!(competition.song_time(), Some(ts(604_800_000_000_001)));
}

#[test]
fn saved_payload_preserves_recorded_prefix_and_portable_sanitized_scalar_labels() {
    let unicode = format!("/private/{}\n", "界".repeat(70));
    let competition = competition(&[
        ("C:\\private\\history\\own\nrecord.bkr", OpponentKind::Own),
        ("/private/\t\r\n", OpponentKind::Other),
        (&unicode, OpponentKind::Other),
    ]);
    let mut host = Host::default();
    let mut last = None;
    publish_solo(&mut host, &mut last, false, PlayerId(7), &competition, None).unwrap();
    assert_eq!(
        host.publications,
        [Publication::Saved(
            PlayerId(7),
            vec![
                GhostSnapshot {
                    kind: OpponentKind::Own,
                    label: "ownrecord.bkr".into(),
                    hits: 1,
                    misses: 0,
                    combo: 1,
                    max_combo: 1,
                    recorded_until: Some(ts(1_000_000_000))
                },
                GhostSnapshot {
                    kind: OpponentKind::Other,
                    label: "RECORD".into(),
                    hits: 1,
                    misses: 0,
                    combo: 1,
                    max_combo: 1,
                    recorded_until: Some(ts(1_000_000_000))
                },
                GhostSnapshot {
                    kind: OpponentKind::Other,
                    label: "界".repeat(64),
                    hits: 1,
                    misses: 0,
                    combo: 1,
                    max_combo: 1,
                    recorded_until: Some(ts(1_000_000_000))
                },
            ]
        )]
    );
    assert_eq!(display_basename("C:\\private/other\\safe.bkr"), "safe.bkr");
    assert_eq!(display_basename("/private/"), "RECORD");
    assert_eq!(competition.score().hits, 0);
    assert!(
        competition
            .opponents()
            .iter()
            .all(|opponent| opponent.score().misses == 0)
    );
}

#[test]
fn solo_validates_every_remote_row_before_selecting_ordinal_zero_and_retains_disconnected_prefix() {
    let first = member(u32::MAX, i64::MIN, u64::MAX);
    let accepted = prefix(vec![first, member(7, 17, 3), member(91, i64::MAX, 1)]);
    let roster = [PlayerId(u32::MAX), PlayerId(7), PlayerId(91)];
    let competition = competition(&[]);
    let mut host = Host::default();
    let mut last = None;
    for status in [NetworkStatus::Connected, NetworkStatus::Disconnected] {
        publish_solo(
            &mut host,
            &mut last,
            true,
            PlayerId(7),
            &competition,
            Some(network(status, &roster, &accepted)),
        )
        .unwrap();
        assert_eq!(
            host.publications.last(),
            Some(&Publication::Solo(
                PlayerId(7),
                CompetitionSnapshot {
                    ghosts: vec![],
                    network: Some(NetworkSnapshot {
                        status,
                        progress: Some(first.progress)
                    }),
                }
            ))
        );
    }
    for corruption in 0..7 {
        let mut invalid = accepted.clone();
        match corruption {
            0 => invalid.members[2].progress.combo = 2,
            1 => invalid.members[2].progress.max_combo = 2,
            2 => invalid.members[2].progress.misses = u64::MAX,
            3 => invalid.members[2].player = PlayerId(0),
            4 => invalid.members[2].player = PlayerId(7),
            5 => invalid.members.swap(1, 2),
            _ => {
                invalid.members.pop();
            }
        }
        assert_eq!(selected_remote_member(Some(&roster), Some(&invalid)), None);
        publish_solo(
            &mut host,
            &mut last,
            true,
            PlayerId(7),
            &competition,
            Some(network(NetworkStatus::Connected, &roster, &invalid)),
        )
        .unwrap();
        assert_eq!(
            host.publications.last(),
            Some(&Publication::Solo(
                PlayerId(7),
                CompetitionSnapshot {
                    ghosts: vec![],
                    network: Some(NetworkSnapshot {
                        status: NetworkStatus::Connected,
                        progress: None
                    }),
                }
            ))
        );
    }
    assert_eq!(selected_remote_member(None, Some(&accepted)), None);
    assert_eq!(selected_remote_member(Some(&[]), Some(&accepted)), None);
}

#[test]
fn cohort_maps_ordinal_remote_prefix_preserves_local_order_and_refuses_invalid_whole_rows() {
    let local = [PlayerId(91), PlayerId(7), PlayerId(u32::MAX)];
    let roster = [PlayerId(33), PlayerId(44)];
    let accepted = prefix(vec![member(33, -604_800_000_000_017, 3), member(44, 17, 8)]);
    let mut host = Host::default();
    let mut last = None;
    for status in [
        NetworkStatus::Connected,
        NetworkStatus::Disconnected,
        NetworkStatus::Stopped,
    ] {
        publish_group(
            &mut host,
            &mut last,
            true,
            false,
            &local,
            status,
            Some(&roster),
            Some(&accepted),
        )
        .unwrap();
        assert_eq!(
            host.publications.last(),
            Some(&Publication::Group(vec![
                (
                    local[0],
                    NetworkSnapshot {
                        status,
                        progress: Some(accepted.members[0].progress)
                    }
                ),
                (
                    local[1],
                    NetworkSnapshot {
                        status,
                        progress: Some(accepted.members[1].progress)
                    }
                ),
                (
                    local[2],
                    NetworkSnapshot {
                        status,
                        progress: None
                    }
                ),
            ]))
        );
    }
    assert_eq!(
        remote_members(&local[..1], Some(&roster), Some(&accepted)).unwrap(),
        [(local[0], Some(accepted.members[0]))]
    );
    assert_eq!(
        remote_members(&local, None, None).unwrap(),
        [(local[0], None), (local[1], None), (local[2], None)]
    );
    let before = host.publications.clone();
    let mut invalid = accepted.clone();
    invalid.members[1].progress.combo = 9;
    assert!(
        publish_group(
            &mut host,
            &mut last,
            true,
            false,
            &local,
            NetworkStatus::Connected,
            Some(&roster),
            Some(&invalid)
        )
        .is_err()
    );
    assert_eq!(host.publications, before);
    assert_eq!(last, Some(0));
    for invalid_local in [
        vec![],
        vec![PlayerId(0)],
        vec![PlayerId(7), PlayerId(7)],
        (1..=65).map(PlayerId).collect(),
    ] {
        assert!(remote_members(&invalid_local, Some(&roster), Some(&accepted)).is_err());
    }
    for invalid_remote in [
        vec![],
        vec![PlayerId(0)],
        vec![PlayerId(33), PlayerId(33)],
        vec![PlayerId(44), PlayerId(33)],
        vec![PlayerId(33)],
    ] {
        assert!(remote_members(&local, Some(&invalid_remote), Some(&accepted)).is_err());
    }
    assert!(remote_members(&local, None, Some(&accepted)).is_err());
    host.reject = true;
    host.now = 50_000_000;
    assert!(
        publish_group(
            &mut host,
            &mut last,
            false,
            false,
            &local,
            NetworkStatus::Disconnected,
            Some(&roster),
            Some(&accepted)
        )
        .unwrap_err()
        .downcast_ref::<Refusal>()
        .is_some()
    );
    assert_eq!(last, Some(0));
    host.reject = false;
    publish_group(
        &mut host,
        &mut last,
        false,
        false,
        &local,
        NetworkStatus::Disconnected,
        Some(&roster),
        Some(&accepted),
    )
    .unwrap();
    assert_eq!(last, Some(50_000_000));
    assert_eq!(host.publications.len(), before.len() + 1);
}

#[test]
fn cadence_begins_after_slow_successful_effect_for_solo_and_group() {
    let competition = competition(&[]);
    for group in [false, true] {
        let mut host = Host {
            now: 123,
            effect_delay: 20_000_000,
            ..Default::default()
        };
        let mut last = None;
        if group {
            publish_group(
                &mut host,
                &mut last,
                false,
                false,
                &[PlayerId(7)],
                NetworkStatus::Waiting,
                None,
                None,
            )
            .unwrap();
        } else {
            publish_solo(&mut host, &mut last, false, PlayerId(7), &competition, None).unwrap();
        }
        assert_eq!(last, Some(20_000_123));
        assert_eq!((host.clock_reads, host.attempts), (2, 1));
        host.effect_delay = 0;
        host.now = 70_000_122;
        if group {
            publish_group(
                &mut host,
                &mut last,
                false,
                false,
                &[PlayerId(7)],
                NetworkStatus::Waiting,
                None,
                None,
            )
            .unwrap();
        } else {
            publish_solo(&mut host, &mut last, false, PlayerId(7), &competition, None).unwrap();
        }
        assert_eq!((host.clock_reads, host.attempts), (3, 1));
        assert_eq!(last, Some(20_000_123));
        host.now = 70_000_123;
        if group {
            publish_group(
                &mut host,
                &mut last,
                false,
                false,
                &[PlayerId(7)],
                NetworkStatus::Waiting,
                None,
                None,
            )
            .unwrap();
        } else {
            publish_solo(&mut host, &mut last, false, PlayerId(7), &competition, None).unwrap();
        }
        assert_eq!((host.clock_reads, host.attempts), (5, 2));
        assert_eq!(last, Some(70_000_123));
    }
}

#[test]
fn post_effect_clock_failure_preserves_partial_publication_without_committing_cadence() {
    let competition = competition(&[]);
    for group in [false, true] {
        for regression in [false, true] {
            let mut host = Host {
                now: 50_000_007,
                post_effect_now: regression.then_some(50_000_006),
                post_effect_clock_reject: !regression,
                ..Default::default()
            };
            let mut last = Some(7);
            let result = if group {
                publish_group(
                    &mut host,
                    &mut last,
                    false,
                    false,
                    &[PlayerId(7)],
                    NetworkStatus::Disconnected,
                    None,
                    None,
                )
            } else {
                publish_solo(&mut host, &mut last, false, PlayerId(7), &competition, None)
            };
            let error = result.unwrap_err();
            if !regression {
                assert!(error.downcast_ref::<Refusal>().is_some());
            }
            assert_eq!(last, Some(7));
            assert_eq!(
                (host.clock_reads, host.attempts, host.publications.len()),
                (2, 1, 1)
            );
            host.now = 50_000_007;
            host.post_effect_now = None;
            host.post_effect_clock_reject = false;
            host.clock_reject = false;
            if group {
                publish_group(
                    &mut host,
                    &mut last,
                    false,
                    false,
                    &[PlayerId(7)],
                    NetworkStatus::Disconnected,
                    None,
                    None,
                )
                .unwrap();
            } else {
                publish_solo(&mut host, &mut last, false, PlayerId(7), &competition, None).unwrap();
            }
            assert_eq!(last, Some(50_000_007));
            assert_eq!(
                (host.clock_reads, host.attempts, host.publications.len()),
                (4, 2, 2)
            );
        }
    }
}
