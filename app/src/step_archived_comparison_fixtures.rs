//! Deferred actual Step/Mixer completion with borrowed display-only comparisons.
use super::{StepGameplay, StepLocalGameplay, StepGameplayConfig};
use crate::{
    PreparedBms,
    local_players::{PlayerId, ResolvedInputPlan},
    result_archive::{decode_archive, ArchivedScore},
    competition::OpponentKind,
    competition_presentation::{
        CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus,
    },
    multiplayer::Progress,
};
use beatkernel::{
    audio::{AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank, command_queue},
    input::{BindingMap, CodecLimits, DeviceId},
    replay::codec::{ReplayCodecLimits, decode_replay},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp, Duration},
};
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
struct Domains;
impl ClockMapper for Domains {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (point.domain == target).then_some(point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(8192, 32, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn setup() -> (PreparedBms, StepGameplayConfig) {
    let source = beatkernel_bms::parse("#BPM 120\n", Default::default()).unwrap();
    (
        PreparedBms {
            compiled: source.compile().unwrap(),
            source,
            bank: SampleBank::new(
                AudioFormat::new(1000, 1).unwrap(),
                PcmLimits::new(64, 256, 1).unwrap(),
            )
            .unwrap(),
            sounds: vec![],
            bgm_commands: vec![],
        },
        StepGameplayConfig {
            host_origin: point(1, 0),
            output_origin: point(2, 0),
            preroll: Duration::ZERO,
            early_ns: 0,
            late_ns: 0,
            offset_ns: 0,
            command_capacity: 8,
            bgm_pending: 4,
            bgm_lookahead: Duration::from_nanos(1_000_000_000),
            telemetry_capacity: 8,
        },
    )
}
fn output(bank: SampleBank, end: Option<i64>) -> (beatkernel::audio::CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(8).unwrap();
    let config = MixerConfig::new(
        AudioFormat::new(1000, 1).unwrap(),
        ClockDomainId(2),
        Timestamp::ZERO,
        AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
    );
    (
        producer,
        Mixer::new(
            match end {
                Some(end) => config.with_playback_end_frame(end as u64 / 1_000_000),
                None => config,
            },
            bank,
            consumer,
        )
        .unwrap(),
    )
}
fn selected() -> CompetitionSnapshot {
    // Borrowed port data; these prefix statistics convey no local completion.
    CompetitionSnapshot {
        ghosts: vec![GhostSnapshot {
            kind: OpponentKind::Own,
            label: "saved.bkr".into(),
            hits: 5,
            misses: 1,
            combo: 3,
            max_combo: 5,
            recorded_until: Some(Timestamp::from_nanos(604_800_000_000_000)),
        }],
        network: Some(NetworkSnapshot {
            status: NetworkStatus::Disconnected,
            progress: Some(Progress {
                song_ns: 9_007_199_254_740_993,
                hits: 3,
                misses: 1,
                combo: 0,
                max_combo: 3,
            }),
        }),
    }
}
#[test]
fn actual_solo_full_and_finite_completion_attach_borrowed_comparisons_before_one_shot_capture_take()
{
    for end in [None, Some(5_000_000)] {
        let (prepared, config) = setup();
        let (mut game, bank) = StepGameplay::new_section(
            prepared,
            config,
            BindingMap::from_bindings([]).unwrap(),
            Timestamp::ZERO,
            end.map(Timestamp::from_nanos),
        )
        .unwrap();
        game.configure_capture(limits(), u64::MAX).unwrap();
        let selected = selected();
        assert!(
            game.completed_archive_with_comparisons(&[(PlayerId(0), Some(&selected))])
                .unwrap()
                .is_none()
        );
        game.activate(point(1, 0)).unwrap();
        game.advance_to(point(1, end.unwrap_or(1_000_000)), &Domains, point(2, 0))
            .unwrap();
        assert!(
            game.completed_archive_with_comparisons(&[(PlayerId(1), Some(&selected))])
                .unwrap()
                .is_none()
        );
        let (_producer, mut mixer) = output(bank, end);
        for index in 1..=2 {
            let report = mixer.render(&mut [0.; 10]).unwrap();
            game.observe_completion(Some(report), Some(point(2, index * 10_000_000)))
                .unwrap();
        }
        assert!(game.completed_result().is_some());
        let old = game.completed_archive().unwrap().unwrap();
        assert_eq!(&old[8..12], &2u32.to_le_bytes());
        assert!(decode_archive(&old).unwrap().comparisons().is_none());
        let bytes = game
            .completed_archive_with_comparisons(&[(PlayerId(1), Some(&selected))])
            .unwrap()
            .unwrap();
        let archive = decode_archive(&bytes).unwrap();
        assert_eq!(&bytes[8..12], &3u32.to_le_bytes());
        assert_eq!(archive.comparison(PlayerId(1)), Some(&selected));
        assert_eq!(
            archive.entries()[0].score,
            Some(ArchivedScore::from_summary(game.score()).unwrap())
        );
        assert_eq!(
            archive.entries()[0].result.gauge,
            game.completed_result().unwrap().gauge()
        );
        game.fail();
        let replay = decode_replay(&game.take_replay().unwrap().unwrap(), limits()).unwrap();
        assert_eq!(archive.entries()[0].header, replay.header);
        assert!(
            game.completed_archive_with_comparisons(&[(PlayerId(1), Some(&selected))])
                .is_err()
        );
    }
}
#[test]
fn actual_local_completion_exports_whole_original_roster_and_validates_shuffled_none_or_selected_rows()
 {
    for count in [1u32, 2, 64] {
        let ids: Vec<_> = (0..count)
            .map(|index| PlayerId(u32::MAX - index * 17))
            .collect();
        let plan = ResolvedInputPlan::new(
            ids.iter()
                .enumerate()
                .map(|(index, id)| (*id, Some(DeviceId(index as u64))))
                .collect(),
        )
        .unwrap();
        let (prepared, config) = setup();
        let (mut game, bank) = StepLocalGameplay::new_section(
            prepared,
            config,
            plan,
            ids.iter()
                .map(|_| BindingMap::from_bindings([]).unwrap())
                .collect(),
            Timestamp::ZERO,
            None,
            beatkernel_bms::BmsInputMode::ButtonOnly,
        )
        .unwrap();
        for id in &ids {
            game.configure_capture(*id, limits(), id.0 as u64).unwrap();
        }
        let selected = selected();
        let rows: Vec<_> = ids
            .iter()
            .enumerate()
            .rev()
            .map(|(index, id)| {
                (
                    *id,
                    if index % 2 == 0 {
                        Some(&selected)
                    } else {
                        None
                    },
                )
            })
            .collect();
        assert!(
            game.completed_archive_with_comparisons(&rows)
                .unwrap()
                .is_none()
        );
        game.activate(point(1, 0)).unwrap();
        game.advance_to(point(1, 1_000_000), &Domains, point(2, 0))
            .unwrap();
        let (_producer, mut mixer) = output(bank, None);
        for index in 1..=2 {
            let report = mixer.render(&mut [0.; 10]).unwrap();
            game.observe_completion(Some(report), Some(point(2, index * 10_000_000)))
                .unwrap();
        }
        let bytes = game
            .completed_archive_with_comparisons(&rows)
            .unwrap()
            .unwrap();
        let archive = decode_archive(&bytes).unwrap();
        assert_eq!(
            archive
                .comparisons()
                .unwrap()
                .iter()
                .map(|row| row.0)
                .collect::<Vec<_>>(),
            ids
        );
        for (index, row) in archive.entries().iter().enumerate() {
            assert_eq!(
                archive.comparison(row.player),
                if index % 2 == 0 {
                    Some(&selected)
                } else {
                    None
                }
            );
            assert_eq!(
                row.score,
                Some(ArchivedScore::from_summary(game.score(row.player).unwrap()).unwrap())
            );
            assert_eq!(
                row.result.gauge,
                game.completed_result(row.player).unwrap().unwrap().gauge()
            );
        }
        assert!(
            game.completed_archive_with_comparisons(&[(PlayerId(0), None)])
                .is_err()
        );
        game.fail();
        game.take_replay(*ids.last().unwrap()).unwrap().unwrap();
        assert!(game.completed_archive_with_comparisons(&rows).is_err());
    }
}
#[test]
fn cancelled_or_disabled_owner_cannot_promote_comparison_data_and_invalid_completed_attachment_keeps_proof()
 {
    let selected = selected();
    for recording in [false, true] {
        let (prepared, config) = setup();
        let (mut game, _) = StepGameplay::new_section(
            prepared,
            config,
            BindingMap::from_bindings([]).unwrap(),
            Timestamp::ZERO,
            None,
        )
        .unwrap();
        if recording {
            game.configure_capture(limits(), 7).unwrap();
        }
        game.fail();
        assert!(game.completed_result().is_none());
        assert!(
            game.completed_archive_with_comparisons(&[(PlayerId(1), Some(&selected))])
                .unwrap()
                .is_none()
        );
    }
    {
        let (prepared, config) = setup();
        let (mut disabled, bank) = StepGameplay::new_section(
            prepared,
            config,
            BindingMap::from_bindings([]).unwrap(),
            Timestamp::ZERO,
            None,
        )
        .unwrap();
        disabled.activate(point(1, 0)).unwrap();
        disabled
            .advance_to(point(1, 1_000_000), &Domains, point(2, 0))
            .unwrap();
        let (_producer, mut mixer) = output(bank, None);
        for index in 1..=2 {
            let report = mixer.render(&mut [0.; 10]).unwrap();
            disabled
                .observe_completion(Some(report), Some(point(2, index * 10_000_000)))
                .unwrap();
        }
        assert!(disabled.completed_result().is_some());
        assert!(
            disabled
                .completed_archive_with_comparisons(&[(PlayerId(0), Some(&selected))])
                .unwrap()
                .is_none()
        );
    }
    let (prepared, config) = setup();
    let (mut game, bank) = StepGameplay::new_section(
        prepared,
        config,
        BindingMap::from_bindings([]).unwrap(),
        Timestamp::ZERO,
        None,
    )
    .unwrap();
    game.configure_capture(limits(), 7).unwrap();
    game.activate(point(1, 0)).unwrap();
    game.advance_to(point(1, 1_000_000), &Domains, point(2, 0))
        .unwrap();
    let (_producer, mut mixer) = output(bank, None);
    for index in 1..=2 {
        let report = mixer.render(&mut [0.; 10]).unwrap();
        game.observe_completion(Some(report), Some(point(2, index * 10_000_000)))
            .unwrap();
    }
    let proof = *game.completed_result().unwrap();
    let score = game.score().clone();
    let mut bad = selected.clone();
    bad.ghosts[0].label = "invalid\nlabel".into();
    assert!(
        game.completed_archive_with_comparisons(&[(PlayerId(1), Some(&bad))])
            .is_err()
    );
    assert_eq!(game.completed_result(), Some(&proof));
    assert_eq!(game.score(), &score);
    assert!(
        decode_archive(
            &game
                .completed_archive_with_comparisons(&[(PlayerId(1), None)])
                .unwrap()
                .unwrap()
        )
        .unwrap()
        .comparisons()
        .unwrap()[0]
            .1
            .is_none()
    );
}

const RECORD_START: i64 = 1_000_000_000;
const RECORD_SEED: u64 = 0x1234_5678_9abc_def0;
fn recording_setup() -> (PreparedBms, StepGameplayConfig) {
    use beatkernel::audio::{PcmSample, SampleId};
    let source = beatkernel_bms::parse_seeded(
        "#BPM 120\n#RANK 3\n#TOTAL 320\n#WAV01 tap.wav\n#00111:01\n#00131:01\n",
        Default::default(),
        RECORD_SEED,
    )
    .unwrap();
    let (_, config) = setup();
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 4], pcm).unwrap(),
    )
    .unwrap();
    (
        PreparedBms {
            compiled: source.compile().unwrap(),
            source,
            bank,
            sounds: vec![],
            bgm_commands: vec![],
        },
        config,
    )
}
fn recording_policy(source: &beatkernel_bms::BmsChart) -> crate::play_policy::ResolvedPlayPolicy {
    use beatkernel_bms::{BmsGaugeKind, BmsRankPrecedence, BmsTimingPreset};
    crate::play_policy::ResolvedPlayPolicy::bms_with_timing(
        source,
        BmsGaugeKind::Groove,
        crate::play_policy::TimingPresetSelection {
            preset: BmsTimingPreset::BeatorajaSevenKeys8320241dV1,
            precedence: BmsRankPrecedence::RankFirst,
        },
        0,
    )
    .unwrap()
}
fn recording_solo(
    end: Option<i64>,
    mode: beatkernel_bms::BmsInputMode,
    policy: bool,
) -> (
    StepGameplay,
    beatkernel_bms::BmsChart,
    Option<crate::play_policy::ResolvedPlayPolicy>,
) {
    let (prepared, config) = recording_setup();
    let source = prepared.source.clone();
    let policy = policy.then(|| recording_policy(&source));
    let (game, _) = match &policy {
        Some(_) => StepGameplay::new_section_with_policy(
            prepared,
            config,
            BindingMap::from_bindings([]).unwrap(),
            Timestamp::from_nanos(RECORD_START),
            end.map(Timestamp::from_nanos),
            mode,
            recording_policy(&source),
        ),
        None => StepGameplay::new_section_with_input_mode(
            prepared,
            config,
            BindingMap::from_bindings([]).unwrap(),
            Timestamp::from_nanos(RECORD_START),
            end.map(Timestamp::from_nanos),
            mode,
        ),
    }
    .unwrap();
    (game, source, policy)
}
fn recording_local(
    end: Option<i64>,
    mode: beatkernel_bms::BmsInputMode,
    policy: bool,
) -> (
    StepLocalGameplay,
    beatkernel_bms::BmsChart,
    Option<crate::play_policy::ResolvedPlayPolicy>,
) {
    let (prepared, config) = recording_setup();
    let source = prepared.source.clone();
    let policy = policy.then(|| recording_policy(&source));
    let plan = ResolvedInputPlan::new(vec![
        (PlayerId(7), Some(DeviceId(7))),
        (PlayerId(u32::MAX), Some(DeviceId(8))),
    ])
    .unwrap();
    let bindings = [DeviceId(7), DeviceId(8)]
        .into_iter()
        .map(|device| {
            use beatkernel::input::{Binding, DeviceSelector, GameControlId, PhysicalControlId};
            BindingMap::from_bindings([Binding {
                device: DeviceSelector::Exact(device),
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(0x11),
            }])
            .unwrap()
        })
        .collect();
    let (game, _) = match &policy {
        Some(_) => StepLocalGameplay::new_section_with_policy(
            prepared,
            config,
            plan,
            bindings,
            Timestamp::from_nanos(RECORD_START),
            end.map(Timestamp::from_nanos),
            mode,
            recording_policy(&source),
        ),
        None => StepLocalGameplay::new_section(
            prepared,
            config,
            plan,
            bindings,
            Timestamp::from_nanos(RECORD_START),
            end.map(Timestamp::from_nanos),
            mode,
        ),
    }
    .unwrap();
    (game, source, policy)
}
fn assert_recording_metadata(
    header: &beatkernel::replay::ReplayHeader,
    source: &beatkernel_bms::BmsChart,
    end: Option<i64>,
    mode: beatkernel_bms::BmsInputMode,
    policy: Option<&crate::play_policy::ResolvedPlayPolicy>,
) {
    use beatkernel_bms::BmsInputMode;
    let decoded = crate::replay_playback::decode_section_setup(&header.options).unwrap();
    assert_eq!(decoded.start, Timestamp::from_nanos(RECORD_START));
    assert_eq!(decoded.end, end.map(Timestamp::from_nanos));
    assert_eq!(decoded.chart_seed, RECORD_SEED);
    assert_eq!(decoded.input_mode, mode);
    assert_eq!(header.normalized_clock, ClockDomainId(1));
    let rules: &[u8] = match (policy.is_some(), mode) {
        (false, BmsInputMode::ButtonOnly) => b"beatkernel-bms/builtin-judge/v1",
        (false, BmsInputMode::ButtonOrContact) => b"beatkernel-bms/press-judge/v1",
        (true, BmsInputMode::ButtonOnly) => b"beatkernel-bms/profiled-builtin-judge/v1",
        (true, BmsInputMode::ButtonOrContact) => b"beatkernel-bms/profiled-press-judge/v1",
    };
    assert_eq!(header.rules_identity, rules);
    assert!(header.chart_identity.starts_with(b"bms-judge-setup/v2:"));
    let sound = crate::input_sounds::InputSoundIdentity::from_source(source)
        .unwrap()
        .unwrap();
    assert_eq!(
        &header.chart_identity[header.chart_identity.len() - 8..],
        &sound.fingerprint().to_le_bytes()
    );
    if let Some(policy) = policy {
        assert_eq!(decoded.profile, *policy.judge());
        assert_eq!(decoded.gauge, *policy.gauge());
        assert_eq!(decoded.judgments.as_ref(), policy.judgments());
        assert_eq!(decoded.timing.as_ref(), policy.timing());
    } else {
        assert_eq!(decoded.gauge, crate::gauge::GaugeProfile::default());
        assert_eq!(decoded.profile.input_offset(), Duration::ZERO);
        assert_eq!(decoded.profile.windows().len(), 1);
        assert_eq!(
            decoded.profile.windows()[0].grade,
            beatkernel::judge::JudgeGrade(1)
        );
        assert_eq!(decoded.profile.windows()[0].early, Duration::ZERO);
        assert_eq!(decoded.profile.windows()[0].late, Duration::ZERO);
        assert!(decoded.judgments.is_none() && decoded.timing.is_none());
    }
}
fn assert_genuine_admission(
    source: &beatkernel_bms::BmsChart,
    header: beatkernel::replay::ReplayHeader,
    bytes: &[u8],
) {
    let mut opponents =
        crate::saved_opponents::SavedOpponents::new(header, limits(), 8, 65536).unwrap();
    assert_eq!(
        opponents
            .add(source, bytes, OpponentKind::Own, "genuine.bkr")
            .unwrap(),
        0
    );
    opponents
        .advance_to(Timestamp::from_nanos(2_500_000_000))
        .unwrap();
    assert_eq!(opponents.count(), 1);
    assert_eq!(opponents.encoded_bytes(), bytes.len());
    assert_eq!(opponents.opponents()[0].score().misses, 1);
    assert_eq!(
        opponents.opponents()[0].recorded_until(),
        Some(Timestamp::from_nanos(2_500_000_000))
    );
}

#[test]
fn recording_headers_equal_genuine_solo_and_full_width_local_captures_and_admit_same_sections() {
    use beatkernel_bms::BmsInputMode;
    for end in [None, Some(3_000_000_000)] {
        for mode in [BmsInputMode::ButtonOnly, BmsInputMode::ButtonOrContact] {
            for selected in [false, true] {
                let (mut solo, source, policy) = recording_solo(end, mode, selected);
                let score = solo.score().clone();
                let hash = solo.judge().stable_hash().unwrap();
                let header = solo.recording_header(limits(), RECORD_SEED).unwrap();
                assert_recording_metadata(&header, &source, end, mode, policy.as_ref());
                assert_eq!(solo.score(), &score);
                assert_eq!(solo.judge().stable_hash().unwrap(), hash);
                solo.configure_capture(limits(), RECORD_SEED).unwrap();
                assert_eq!(
                    solo.recording_header(limits(), RECORD_SEED).unwrap(),
                    header
                );
                solo.activate(point(1, 0)).unwrap();
                solo.advance_to(point(1, 1_500_000_000), &Domains, point(2, 0))
                    .unwrap();
                assert!(solo.recording_header(limits(), RECORD_SEED).is_err());
                solo.fail();
                let bytes = solo.take_replay().unwrap().unwrap();
                let replay = decode_replay(&bytes, limits()).unwrap();
                assert_eq!(replay.header, header);
                assert!(!replay.records.is_empty());
                assert_genuine_admission(&source, header, &bytes);
                assert!(solo.take_replay().unwrap().is_none());

                let (mut local, source, policy) = recording_local(end, mode, selected);
                let ids = [PlayerId(7), PlayerId(u32::MAX)];
                let headers: Vec<_> = ids
                    .iter()
                    .map(|id| {
                        let score = local.score(*id).unwrap().clone();
                        let hash = local.judge(*id).unwrap().stable_hash().unwrap();
                        let header = local.recording_header(*id, limits(), RECORD_SEED).unwrap();
                        assert_recording_metadata(&header, &source, end, mode, policy.as_ref());
                        assert_eq!(local.score(*id), Some(&score));
                        assert_eq!(local.judge(*id).unwrap().stable_hash().unwrap(), hash);
                        header
                    })
                    .collect();
                for (id, header) in ids.iter().zip(&headers) {
                    local.configure_capture(*id, limits(), RECORD_SEED).unwrap();
                    assert_eq!(
                        local.recording_header(*id, limits(), RECORD_SEED).unwrap(),
                        *header
                    );
                }
                local.activate(point(1, 0)).unwrap();
                local
                    .advance_to(point(1, 1_500_000_000), &Domains, point(2, 0))
                    .unwrap();
                for id in ids {
                    assert!(local.recording_header(id, limits(), RECORD_SEED).is_err());
                }
                local.fail();
                for (id, header) in ids.iter().zip(headers) {
                    let bytes = local.take_replay(*id).unwrap().unwrap();
                    assert_eq!(decode_replay(&bytes, limits()).unwrap().header, header);
                    assert_genuine_admission(&source, header, &bytes);
                    assert!(local.take_replay(*id).unwrap().is_none());
                }
            }
        }
    }
}

#[test]
fn genuine_wrong_finite_and_open_sections_preserve_accepted_opponent_bytes_prefix_and_score() {
    use crate::{
        competition::CompetitionError,
        saved_opponents::{SavedOpponents, SavedOpponentsError},
    };
    use beatkernel_bms::BmsInputMode;
    let mut recordings = Vec::new();
    for end in [None, Some(3_000_000_000), Some(4_000_000_000)] {
        let (mut game, source, _) = recording_solo(end, BmsInputMode::ButtonOrContact, true);
        let header = game.recording_header(limits(), RECORD_SEED).unwrap();
        game.configure_capture(limits(), RECORD_SEED).unwrap();
        game.activate(point(1, 0)).unwrap();
        game.advance_to(point(1, 1_500_000_000), &Domains, point(2, 0))
            .unwrap();
        game.fail();
        recordings.push((source, header, game.take_replay().unwrap().unwrap()));
    }
    for (index, (source, header, bytes)) in recordings.iter().enumerate() {
        let mut opponents = SavedOpponents::new(header.clone(), limits(), 8, 65536).unwrap();
        opponents
            .add(source, bytes, OpponentKind::Own, "accepted.bkr")
            .unwrap();
        opponents
            .advance_to(Timestamp::from_nanos(2_500_000_000))
            .unwrap();
        let before = (
            opponents.count(),
            opponents.encoded_bytes(),
            opponents.song_time(),
            opponents.opponents()[0].score().clone(),
            opponents.opponents()[0].recorded_until(),
            opponents.opponents()[0].song_time(),
            opponents.opponents()[0].label().to_owned(),
        );
        for (other, (source, _, wrong)) in recordings.iter().enumerate() {
            if index == other {
                continue;
            }
            assert!(matches!(
                opponents.add(source, wrong, OpponentKind::Other, "wrong.bkr"),
                Err(SavedOpponentsError::Competition(
                    CompetitionError::IncompatibleSetup
                ))
            ));
            assert_eq!(
                (
                    opponents.count(),
                    opponents.encoded_bytes(),
                    opponents.song_time(),
                    opponents.opponents()[0].score().clone(),
                    opponents.opponents()[0].recorded_until(),
                    opponents.opponents()[0].song_time(),
                    opponents.opponents()[0].label().to_owned()
                ),
                before
            );
        }
        assert_eq!(
            opponents
                .add(source, bytes, OpponentKind::Other, "recovery.bkr")
                .unwrap(),
            1
        );
        assert_eq!(opponents.encoded_bytes(), bytes.len() * 2);
    }
}

#[test]
fn recording_header_queries_remain_optional_pristine_member_checked_and_limit_checked() {
    use beatkernel_bms::BmsInputMode;
    let tiny = ReplayCodecLimits::new(8192, 32, 1, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    let (mut solo, _, _) = recording_solo(Some(3_000_000_000), BmsInputMode::ButtonOnly, false);
    assert!(solo.recording_header(tiny, RECORD_SEED).is_err());
    let header = solo.recording_header(limits(), RECORD_SEED).unwrap();
    assert_eq!(
        solo.recording_header(limits(), RECORD_SEED).unwrap(),
        header
    );
    solo.fail();
    assert!(solo.recording_header(limits(), RECORD_SEED).is_err());
    assert!(solo.take_replay().unwrap().is_none());
    let (mut local, _, _) = recording_local(None, BmsInputMode::ButtonOrContact, false);
    assert!(matches!(
        local.recording_header(PlayerId(0), limits(), RECORD_SEED),
        Err(super::StepLocalGameplayError::UnknownPlayer(PlayerId(0)))
    ));
    assert!(local
        .recording_header(PlayerId(u32::MAX), tiny, RECORD_SEED)
        .is_err());
    let header = local
        .recording_header(PlayerId(u32::MAX), limits(), RECORD_SEED)
        .unwrap();
    local
        .configure_capture(PlayerId(u32::MAX), limits(), RECORD_SEED)
        .unwrap();
    assert_eq!(
        local
            .recording_header(PlayerId(u32::MAX), limits(), RECORD_SEED)
            .unwrap(),
        header
    );
    local.fail();
    assert!(local
        .recording_header(PlayerId(u32::MAX), limits(), RECORD_SEED)
        .is_err());
    assert!(local.take_replay(PlayerId(7)).unwrap().is_none());
    let replay = local.take_replay(PlayerId(u32::MAX)).unwrap().unwrap();
    assert_eq!(decode_replay(&replay, limits()).unwrap().header, header);
}
