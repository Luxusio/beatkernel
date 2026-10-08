//! Deferred common native cohort policy; no endpoint or native driver is opened.
use super::*;
use crate::{
    competition::OpponentKind,
    competition_live::replay_limits,
    local_runtime::{InputResult, RuntimeGroup},
    multiplayer::Progress,
    player::{self, GhostSnapshot, NetworkSnapshot, NetworkStatus},
    replay_capture::{setup_header, LiveReplayCapture},
    section_start::source_at,
};
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::codec::{encode_replay, ReplayFile},
    runtime::RuntimeProcessingClock,
    time::{ClockMapper, ClockMappingQuality, ClockPoint, Duration},
    transport::{Rate, Transport},
};
use beatkernel_bms::{parse_seeded, ParseOptions};

const DOMAIN: ClockDomainId = ClockDomainId(17);
const PREROLL: i64 = 100_000_000;

fn selected_policy(
    source: &BmsChart,
    kind: beatkernel_bms::BmsGaugeKind,
    class: beatkernel_bms::BmsJudgment,
    offset: i64,
) -> crate::play_policy::ResolvedPlayPolicy {
    crate::play_policy::ResolvedPlayPolicy::bms(
        source,
        kind,
        &[crate::play_policy::ClassifiedWindow {
            judgment: class,
            window: JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            },
        }],
        offset,
    )
    .unwrap()
}

#[test]
fn selected_group_identity_retains_actual_classes_gauge_section_seed_and_default_bytes() {
    let source = source();
    let options = CompetitionOptions::default();
    let start = Timestamp::from_nanos(1_000_000_000);
    let end = Some(Timestamp::from_nanos(2_500_000_000));
    let members = configs(&source, &[PlayerId(7), PlayerId(u32::MAX)], start, 7);
    let builtin = crate::play_policy::ResolvedPlayPolicy::builtin(0, 0, 7).unwrap();
    let defaults = [(PlayerId(7), &builtin), (PlayerId(u32::MAX), &builtin)];
    assert_eq!(
        canonical_identity_with_policies(
            &options, &source, &members, &defaults, DOMAIN, start, 71, end, PREROLL
        )
        .unwrap(),
        canonical_identity(&options, &source, &members, DOMAIN, start, 71, end, PREROLL).unwrap()
    );
    for kind in beatkernel_bms::BmsGaugeKind::ALL {
        let selected = selected_policy(&source, kind, beatkernel_bms::BmsJudgment::Great, 7);
        let policies = [(PlayerId(7), &selected), (PlayerId(u32::MAX), &selected)];
        let actual = canonical_identity_with_policies(
            &options, &source, &members, &policies, DOMAIN, start, 71, end, PREROLL,
        )
        .unwrap();
        let limits = replay_limits().unwrap();
        let header = crate::replay_capture::setup_play_policy_header(
            &members[0].judge,
            DOMAIN,
            limits,
            start,
            71,
            None,
            beatkernel_bms::BmsInputMode::ButtonOnly,
            crate::input_sounds::InputSoundIdentity::from_source(&source).unwrap(),
            &selected,
        )
        .unwrap();
        let expected = crate::multiplayer::competition_identity_for_section(
            &header,
            env!("CARGO_PKG_VERSION"),
            limits,
            end,
        )
        .unwrap();
        assert_eq!(actual, expected);
        for (seed, endpoint) in [
            (72, end),
            (71, None),
            (71, Some(Timestamp::from_nanos(2_500_000_001))),
        ] {
            assert_ne!(
                canonical_identity_with_policies(
                    &options, &source, &members, &policies, DOMAIN, start, seed, endpoint, PREROLL
                )
                .unwrap(),
                actual
            );
        }
        assert!(members
            .iter()
            .all(|member| member.judge.effective_song_time().is_none()));
    }
}

#[test]
fn selected_group_refuses_missing_reordered_and_mismatched_member_policy_before_endpoint_acquisition(
) {
    let source = source();
    let start = Timestamp::ZERO;
    let members = configs(&source, &[PlayerId(7), PlayerId(u32::MAX)], start, 0);
    let selected = selected_policy(
        &source,
        beatkernel_bms::BmsGaugeKind::Hard,
        beatkernel_bms::BmsJudgment::Great,
        0,
    );
    let other_class = selected_policy(
        &source,
        beatkernel_bms::BmsGaugeKind::Hard,
        beatkernel_bms::BmsJudgment::PGreat,
        0,
    );
    assert_eq!(selected.gauge(), other_class.gauge());
    let other_gauge = selected_policy(
        &source,
        beatkernel_bms::BmsGaugeKind::Hazard,
        beatkernel_bms::BmsJudgment::Great,
        0,
    );
    let other_profile = selected_policy(
        &source,
        beatkernel_bms::BmsGaugeKind::Hard,
        beatkernel_bms::BmsJudgment::Great,
        1,
    );
    let (options, _) =
        CompetitionOptions::extract(&["--mp-host".into(), "127.0.0.1:34567".into()]).unwrap();
    let cases = [
        vec![],
        vec![(PlayerId(7), &selected)],
        vec![(PlayerId(u32::MAX), &selected), (PlayerId(7), &selected)],
        vec![(PlayerId(7), &selected), (PlayerId(7), &selected)],
        vec![(PlayerId(7), &selected), (PlayerId(99), &selected)],
        vec![(PlayerId(7), &selected), (PlayerId(u32::MAX), &other_class)],
        vec![(PlayerId(7), &selected), (PlayerId(u32::MAX), &other_gauge)],
        vec![
            (PlayerId(7), &selected),
            (PlayerId(u32::MAX), &other_profile),
        ],
    ];
    let pristine = members
        .iter()
        .map(|m| m.judge.stable_hash().unwrap())
        .collect::<Vec<_>>();
    let disabled = CompetitionOptions::default();
    let matching = [(PlayerId(7), &selected), (PlayerId(u32::MAX), &selected)];
    assert!(NativeGroupCompetition::prepare_with_policies(
        &disabled, &source, &members, &matching, DOMAIN, start, 71, None, PREROLL
    )
    .unwrap()
    .is_none());
    for policies in cases {
        assert!(NativeGroupCompetition::prepare_with_policies(
            &disabled, &source, &members, &policies, DOMAIN, start, 71, None, PREROLL
        )
        .is_err());
        assert!(canonical_identity_with_policies(
            &options, &source, &members, &policies, DOMAIN, start, 71, None, PREROLL
        )
        .is_err());
        let failure = match NativeGroupCompetition::prepare_with_policies(
            &options, &source, &members, &policies, DOMAIN, start, 71, None, PREROLL,
        ) {
            Err(error) => error,
            Ok(_) => {
                panic!("invalid policy evidence must refuse before opening a network endpoint")
            }
        };
        // A transport feature or absent TLS files must not hide a cold policy
        // failure: the canonical preflight error is the same as preparation.
        let expected = canonical_identity_with_policies(
            &options, &source, &members, &policies, DOMAIN, start, 71, None, PREROLL,
        )
        .unwrap_err();
        assert_eq!(failure.to_string(), expected.to_string());
        assert_eq!(
            members
                .iter()
                .map(|m| m.judge.stable_hash().unwrap())
                .collect::<Vec<_>>(),
            pristine
        );
        assert!(members
            .iter()
            .all(|m| m.judge.effective_song_time().is_none()));
    }
}

fn source() -> BmsChart {
    parse_seeded(
        "#BPM 60\n#WAV01 key.wav\n#00011:0101",
        ParseOptions::default(),
        0,
    )
    .unwrap()
}

fn configs(
    source: &BmsChart,
    players: &[PlayerId],
    start: Timestamp,
    offset: i64,
) -> Vec<MemberConfig> {
    let selected = source_at(source, start).unwrap();
    players
        .iter()
        .enumerate()
        .map(|(index, &player)| {
            let device = DeviceId(u64::MAX - index as u64);
            MemberConfig {
                player,
                device: Some(device),
                bindings: BindingMap::from_bindings([Binding {
                    device: DeviceSelector::Exact(device),
                    physical: PhysicalControlId::keyboard(4),
                    game_control: GameControlId(0x11),
                }])
                .unwrap(),
                judge: JudgeEngine::new(
                    selected.compile().unwrap().chart,
                    selected.rules(),
                    JudgeProfile::new(
                        vec![JudgeWindow {
                            grade: JudgeGrade(1),
                            early: Duration::ZERO,
                            late: Duration::ZERO,
                        }],
                        Duration::from_nanos(offset),
                    )
                    .unwrap(),
                )
                .unwrap(),
                sounds: vec![],
            }
        })
        .collect()
}

fn row(player: u32, song_ns: i64, hits: u64) -> MemberProgress {
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

fn group(members: Vec<MemberProgress>) -> GroupPrefix {
    GroupPrefix {
        sequence: u64::MAX,
        final_prefix: false,
        members,
    }
}

#[test]
fn actual_member_judges_share_one_canonical_identity_independent_of_roster_and_input_devices() {
    let source = source();
    let options = CompetitionOptions::default();
    let limits = replay_limits().unwrap();
    let start = Timestamp::from_nanos(1_000_000_000);
    let end = Some(Timestamp::from_nanos(2_500_000_000));
    let first = configs(&source, &[PlayerId(u32::MAX)], start, 7);
    let expected = crate::multiplayer::competition_identity_for_section(
        &setup_header(&first[0].judge, DOMAIN, limits, start, u64::MAX).unwrap(),
        env!("CARGO_PKG_VERSION"),
        limits,
        end,
    )
    .unwrap();
    for count in [1u32, 3, 64] {
        let players = (0..count)
            .map(|index| PlayerId(u32::MAX - index))
            .collect::<Vec<_>>();
        let members = configs(&source, &players, start, 7);
        let hashes = members
            .iter()
            .map(|member| member.judge.stable_hash().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            canonical_identity(
                &options,
                &source,
                &members,
                DOMAIN,
                start,
                u64::MAX,
                end,
                PREROLL
            )
            .unwrap(),
            expected
        );
        assert_eq!(
            canonical_identity(
                &options,
                &source,
                &members,
                ClockDomainId(u32::MAX),
                start,
                u64::MAX,
                end,
                PREROLL
            )
            .unwrap(),
            expected
        );
        assert_eq!(
            members
                .iter()
                .map(|member| member.judge.stable_hash().unwrap())
                .collect::<Vec<_>>(),
            hashes
        );
        assert!(members
            .iter()
            .all(|member| member.judge.effective_song_time().is_none()));
    }
    assert_ne!(
        canonical_identity(&options, &source, &first, DOMAIN, start, 0, end, PREROLL).unwrap(),
        expected
    );
    assert_ne!(
        canonical_identity(
            &options,
            &source,
            &first,
            DOMAIN,
            start,
            u64::MAX,
            None,
            PREROLL
        )
        .unwrap(),
        expected
    );
    let later_end = Some(Timestamp::from_nanos(2_500_000_001));
    assert_ne!(
        canonical_identity(
            &options,
            &source,
            &first,
            DOMAIN,
            start,
            u64::MAX,
            later_end,
            PREROLL
        )
        .unwrap(),
        expected
    );
}

#[test]
fn identity_preflight_rejects_later_member_mismatch_started_judges_and_invalid_geometry() {
    let source = source();
    let options = CompetitionOptions::default();
    let players = [PlayerId(u32::MAX), PlayerId(7), PlayerId(91)];
    let mut members = configs(&source, &players, Timestamp::ZERO, 0);
    let pristine = members[0].judge.stable_hash().unwrap();
    let mut differently_calibrated = configs(&source, &[players[2]], Timestamp::ZERO, 1);
    members[2] = differently_calibrated.pop().unwrap();
    assert!(canonical_identity(
        &options,
        &source,
        &members,
        DOMAIN,
        Timestamp::ZERO,
        0,
        None,
        PREROLL
    )
    .is_err());
    assert_eq!(members[0].judge.stable_hash().unwrap(), pristine);
    members[2] = configs(&source, &[players[2]], Timestamp::ZERO, 0)
        .pop()
        .unwrap();
    members[2]
        .judge
        .advance_to(Timestamp::from_nanos(1))
        .unwrap();
    assert!(canonical_identity(
        &options,
        &source,
        &members,
        DOMAIN,
        Timestamp::ZERO,
        0,
        None,
        PREROLL
    )
    .is_err());
    assert_eq!(members[0].judge.stable_hash().unwrap(), pristine);

    let members = configs(&source, &players, Timestamp::ZERO, 0);
    let different_source = parse_seeded(
        "#BPM 61\n#WAV01 key.wav\n#00011:0101",
        ParseOptions::default(),
        0,
    )
    .unwrap();
    assert!(canonical_identity(
        &options,
        &different_source,
        &members,
        DOMAIN,
        Timestamp::ZERO,
        0,
        None,
        PREROLL
    )
    .is_err());
    for (start, end, preroll) in [
        (-1, None, PREROLL),
        (0, Some(0), PREROLL),
        (0, Some(-1), PREROLL),
        (0, None, -1),
        (0, None, 10_000_000_001),
    ] {
        assert!(canonical_identity(
            &options,
            &source,
            &members,
            DOMAIN,
            Timestamp::from_nanos(start),
            0,
            end.map(Timestamp::from_nanos),
            preroll
        )
        .is_err());
    }
    for players in [
        vec![],
        vec![PlayerId(1), PlayerId(0)],
        vec![PlayerId(1), PlayerId(1)],
        (1..=65).map(PlayerId).collect(),
    ] {
        let invalid = configs(&source, &players, Timestamp::ZERO, 0);
        assert!(canonical_identity(
            &options,
            &source,
            &invalid,
            DOMAIN,
            Timestamp::ZERO,
            0,
            None,
            PREROLL
        )
        .is_err());
    }
    assert!(canonical_identity(
        &options,
        &source,
        &members,
        DOMAIN,
        Timestamp::ZERO,
        0,
        None,
        10_000_000_000
    )
    .is_ok());
}

#[test]
fn remote_ordinals_support_independent_roster_lengths_full_width_values_and_absent_prefixes() {
    let local = [PlayerId(7), PlayerId(91), PlayerId(u32::MAX)];
    let remote = [PlayerId(u32::MAX), PlayerId(7)];
    let prefix = group(vec![row(u32::MAX, i64::MIN, u64::MAX), row(7, i64::MAX, 5)]);
    assert_eq!(
        remote_members(&local, Some(&remote), Some(&prefix)).unwrap(),
        vec![
            (local[0], Some(prefix.members[0])),
            (local[1], Some(prefix.members[1])),
            (local[2], None),
        ]
    );
    assert_eq!(
        remote_members(&local[..1], Some(&remote), Some(&prefix)).unwrap(),
        vec![(local[0], Some(prefix.members[0]))]
    );
    let missing = local
        .iter()
        .map(|&player| (player, None))
        .collect::<Vec<_>>();
    assert_eq!(remote_members(&local, None, None).unwrap(), missing);
    assert_eq!(
        remote_members(&local, Some(&remote), None).unwrap(),
        missing
    );
    assert!(remote_members(&local, None, Some(&prefix)).is_err());
    let all = (1..=64).map(PlayerId).collect::<Vec<_>>();
    let prefix = group(
        all.iter()
            .map(|player| row(player.0, -604_800_000_000_001, 1))
            .collect(),
    );
    let mapped = remote_members(&all, Some(&all), Some(&prefix)).unwrap();
    assert_eq!(mapped.len(), 64);
    assert_eq!(mapped[63], (PlayerId(64), Some(prefix.members[63])));
}

#[test]
fn remote_mapping_rejects_whole_invalid_rosters_and_later_rows_even_outside_local_slots() {
    let local = [PlayerId(91)];
    let remote = [PlayerId(7), PlayerId(u32::MAX)];
    let accepted = group(vec![row(7, 10, 4), row(u32::MAX, 20, 9)]);
    let mut candidates = Vec::new();
    let mut invalid = accepted.clone();
    invalid.members[1].progress.combo = 10;
    candidates.push(invalid);
    let mut invalid = accepted.clone();
    invalid.members[1].progress.misses = u64::MAX;
    candidates.push(invalid);
    let mut invalid = accepted.clone();
    invalid.members.swap(0, 1);
    candidates.push(invalid);
    let mut invalid = accepted.clone();
    invalid.members[1].player = remote[0];
    candidates.push(invalid);
    let mut invalid = accepted.clone();
    invalid.members.pop();
    candidates.push(invalid);
    for invalid in candidates {
        let before = invalid.clone();
        assert!(remote_members(&local, Some(&remote), Some(&invalid)).is_err());
        assert_eq!(invalid, before);
    }
    for invalid in [
        vec![],
        vec![PlayerId(0)],
        vec![PlayerId(7), PlayerId(7)],
        (1..=65).map(PlayerId).collect(),
    ] {
        assert!(remote_members(&invalid, Some(&remote), Some(&accepted)).is_err());
        assert!(remote_members(&local, Some(&invalid), None).is_err());
    }
    assert_eq!(
        remote_members(&local, Some(&remote), Some(&accepted)).unwrap(),
        vec![(local[0], Some(accepted.members[0]))]
    );
}

#[test]
fn local_prefix_admission_copies_only_a_complete_valid_cumulative_roster() {
    let players = [PlayerId(u32::MAX), PlayerId(7), PlayerId(91)];
    let original = vec![row(u32::MAX, -1, 0), row(7, 20, 4), row(91, 30, 2)];
    let previous = validated_local_prefix(&players, None, &original).unwrap();
    let mut next = vec![
        row(u32::MAX, i64::MAX, u64::MAX),
        row(7, 21, 5),
        row(91, 31, 3),
    ];
    let accepted = validated_local_prefix(&players, Some(&previous), &next).unwrap();
    next[0].progress.hits = 0;
    assert_eq!(accepted[0], row(u32::MAX, i64::MAX, u64::MAX));
    let before = accepted.clone();
    for (index, mut invalid) in [accepted.clone(), accepted.clone(), accepted.clone()]
        .into_iter()
        .enumerate()
    {
        match index {
            0 => invalid[2].progress.song_ns = 29,
            1 => invalid[2] = row(91, 31, 1),
            _ => invalid.swap(1, 2),
        }
        assert!(validated_local_prefix(&players, Some(&accepted), &invalid).is_err());
    }
    assert_eq!(accepted, before);
    assert_eq!(previous, original);
}

struct SameDomain;
impl ClockMapper for SameDomain {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}

#[test]
fn attached_network_and_saved_updates_merge_atomically_without_changing_actual_local_reports() {
    let source = source();
    let players = [PlayerId(u32::MAX), PlayerId(7), PlayerId(91)];
    let members = configs(&source, &players, Timestamp::ZERO, 0);
    let limits = replay_limits().unwrap();
    let mut capture = LiveReplayCapture::new(&members[0].judge, DOMAIN, limits).unwrap();
    let (producer, _consumer) = command_queue(1).unwrap();
    let origin = Timestamp::from_nanos(604_800_000_000_017);
    let point = ClockPoint {
        domain: DOMAIN,
        timestamp: origin,
    };
    let mut runtime = RuntimeGroup::new(
        DOMAIN,
        DOMAIN,
        Transport::new(origin, Timestamp::ZERO, Rate::NORMAL),
        producer,
        members,
        0,
        &[],
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let reports = match runtime
        .process_input(
            PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(u64::MAX), point, u64::MAX),
                control: PhysicalControlId::keyboard(4),
                state: ButtonState::Down,
            }),
            &SameDomain,
            point,
        )
        .unwrap()
    {
        InputResult::Processed(reports) => reports,
        InputResult::Ignored { .. } => panic!("assigned actual source was ignored"),
    };
    assert_eq!(reports.len(), 1);
    capture.record_report(&reports[0].report).unwrap();
    let bytes = encode_replay(
        &ReplayFile::new(capture.header().clone(), capture.records().to_vec()),
        limits,
    )
    .unwrap();
    let hashes = players
        .iter()
        .map(|&player| runtime.member_judge(player).unwrap().stable_hash().unwrap())
        .collect::<Vec<_>>();
    let ghost = GhostSnapshot {
        kind: OpponentKind::Own,
        label: "recorded-prefix".into(),
        hits: 2,
        misses: 1,
        combo: 0,
        max_combo: 2,
        recorded_until: Some(Timestamp::from_nanos(19)),
    };
    let peers = vec![
        (
            players[0],
            NetworkSnapshot {
                status: NetworkStatus::Connected,
                progress: Some(row(17, i64::MIN, u64::MAX).progress),
            },
        ),
        (
            players[1],
            NetworkSnapshot {
                status: NetworkStatus::Connected,
                progress: Some(row(18, 604_800_000_000_001, 7).progress),
            },
        ),
        (
            players[2],
            NetworkSnapshot {
                status: NetworkStatus::Connected,
                progress: None,
            },
        ),
    ];
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_local_chart(&source, &source.compile().unwrap().chart, &players).unwrap();
        player::publish_local_reports(&reports).unwrap();
        player::publish_saved_competition(players[0], vec![ghost.clone()]).unwrap();
        player::publish_networks(&peers).unwrap();
        player::publish_saved_competition(players[1], vec![ghost.clone()]).unwrap();
        let mut invalid = peers.clone();
        invalid[0].1.status = NetworkStatus::Stopped;
        invalid[2].1.progress = Some(Progress {
            song_ns: 0,
            hits: 1,
            misses: 0,
            combo: 2,
            max_combo: 1,
        });
        assert!(player::publish_networks(&invalid).is_err());
        invalid[2] = (PlayerId(8), peers[2].1.clone());
        assert!(player::publish_networks(&invalid).is_err());
        invalid[2] = invalid[1].clone();
        assert!(player::publish_networks(&invalid).is_err());
        let mut invalid_ghost = ghost.clone();
        invalid_ghost.label = "bad\nlabel".into();
        assert!(player::publish_saved_competition(players[1], vec![invalid_ghost]).is_err());
        Ok(())
    })
    .unwrap();
    let final_snapshot = viewer.take_latest().unwrap();
    assert_eq!(
        final_snapshot
            .players
            .iter()
            .map(|member| member.player)
            .collect::<Vec<_>>(),
        players
    );
    for (index, member) in final_snapshot.players.iter().enumerate() {
        let comparisons = member.competition.as_ref().unwrap();
        assert_eq!(comparisons.network.as_ref(), Some(&peers[index].1));
        assert_eq!(
            comparisons.ghosts,
            if index < 2 {
                vec![ghost.clone()]
            } else {
                vec![]
            }
        );
        assert_eq!(member.score.hits, if index == 0 { 1 } else { 0 });
        assert_eq!(member.score.misses, 0);
    }
    assert_eq!(final_snapshot.players[0].song_time, Some(Timestamp::ZERO));
    assert_eq!(final_snapshot.players[1].song_time, None);
    assert_eq!(
        players
            .iter()
            .map(|&player| runtime.member_judge(player).unwrap().stable_hash().unwrap())
            .collect::<Vec<_>>(),
        hashes
    );
    assert_eq!(capture.into_bytes().unwrap(), bytes);
}
