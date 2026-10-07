use crate::{
    competition::{Competition, OpponentKind},
    competition_live::{CompetitionOptions, LiveCompetition, replay_limits},
    native_judge::NativeJudgeConfig,
    play_policy::{GaugeSelection, OriginalGaugeContext, ResolvedPlayPolicy},
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    audio::command_queue,
    input::BindingMap,
    judge::JudgeEngine,
    replay::{
        ReplayOperation, ReplayRecord,
        codec::{ReplayFile, encode_replay},
    },
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockPoint, ClockMapper, ClockMappingQuality, Timestamp},
    transport::{Transport, Rate},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
fn source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse("#BPM 60\n#WAV01 note.wav\n#00011:01", Default::default()).unwrap()
}
fn policy(
    source: &beatkernel_bms::BmsChart,
    kind: beatkernel_bms::BmsGaugeKind,
) -> ResolvedPlayPolicy {
    cfg()
        .resolve_play_policy(
            &OriginalGaugeContext::from_source(source),
            GaugeSelection::Bms(kind),
        )
        .unwrap()
}
fn cfg() -> NativeJudgeConfig {
    NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(17),
        end: None,
    }
}
fn judge(source: &beatkernel_bms::BmsChart, policy: &ResolvedPlayPolicy) -> JudgeEngine {
    cfg()
        .judge_with_policy(source, source.compile().unwrap().chart, policy)
        .unwrap()
}
fn file(
    source: &beatkernel_bms::BmsChart,
    policy: &ResolvedPlayPolicy,
    end: Option<Timestamp>,
) -> ReplayFile {
    let judge = judge(source, policy);
    let mut file = LiveReplayCapture::new_with_policy(
        &judge,
        ClockDomainId(17),
        replay_limits().unwrap(),
        Timestamp::ZERO,
        0,
        end,
        beatkernel_bms::BmsInputMode::ButtonOnly,
        None,
        policy,
    )
    .unwrap()
    .into_file();
    file.records.push(ReplayRecord {
        ordinal: 0,
        song_time: Timestamp::from_nanos(1),
        operation: ReplayOperation::Advance,
    });
    file
}
struct Saved(PathBuf);
impl Saved {
    fn new(file: &ReplayFile) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-policy-ghost-{}-{}.bkr",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            &path,
            encode_replay(file, replay_limits().unwrap()).unwrap(),
        )
        .unwrap();
        Self(path)
    }
}
impl Drop for Saved {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (point.domain == target).then_some(point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
#[test]
fn actual_saved_policy_ghosts_prepare_and_follow_committed_runtime_prefixes_for_all_six_kinds() {
    let source = source();
    for kind in beatkernel_bms::BmsGaugeKind::ALL {
        for end in [None, Some(Timestamp::from_nanos(10))] {
            let policy = policy(&source, kind);
            let file = file(&source, &policy, end);
            let saved = Saved::new(&file);
            let mut options = CompetitionOptions::default();
            options.ghosts.push((OpponentKind::Own, saved.0.clone()));
            let engine = judge(&source, &policy);
            let mut live = LiveCompetition::prepare_native_section_with_policy(
                &options,
                &source,
                &engine,
                &policy,
                ClockDomainId(17),
                Timestamp::ZERO,
                0,
                end,
                0,
            )
            .unwrap()
            .unwrap();
            assert_eq!(live.native_policy_header(), &file.header);
            let (producer, _consumer) = command_queue(8).unwrap();
            let mut runtime = Runtime::new(
                ClockDomainId(17),
                ClockDomainId(17),
                Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
                BindingMap::from_bindings([]).unwrap(),
                engine,
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
            live.observe(&report).unwrap();
            let snapshot = live.archive_snapshot().unwrap();
            assert_eq!(snapshot.ghosts.len(), 1);
            assert_eq!(snapshot.ghosts[0].misses, 1);
            assert_eq!(
                snapshot.ghosts[0].recorded_until,
                Some(Timestamp::from_nanos(1))
            );
        }
    }
}
#[test]
fn mismatched_policy_or_endpoint_refuses_without_changing_existing_opponents() {
    let source = source();
    let hard = policy(&source, beatkernel_bms::BmsGaugeKind::Hard);
    let other = policy(&source, beatkernel_bms::BmsGaugeKind::Groove);
    let recorded = file(&source, &hard, Some(Timestamp::from_nanos(10)));
    let mut comparison = Competition::new(recorded.header.clone(), 4).unwrap();
    comparison
        .add_replay(
            &source,
            recorded.clone(),
            replay_limits().unwrap(),
            OpponentKind::Own,
            "same",
        )
        .unwrap();
    assert!(
        comparison
            .add_replay(
                &source,
                file(&source, &other, Some(Timestamp::from_nanos(10))),
                replay_limits().unwrap(),
                OpponentKind::Other,
                "different"
            )
            .is_err()
    );
    assert!(
        comparison
            .add_replay(
                &source,
                file(&source, &hard, Some(Timestamp::from_nanos(11))),
                replay_limits().unwrap(),
                OpponentKind::Other,
                "end"
            )
            .is_err()
    );
    assert_eq!(comparison.opponents().len(), 1);
    let mut truncated = recorded;
    truncated.records[0].song_time = Timestamp::from_nanos(12);
    assert!(
        comparison
            .add_replay(
                &source,
                truncated,
                replay_limits().unwrap(),
                OpponentKind::Other,
                "outside"
            )
            .is_err()
    );
    assert_eq!(comparison.opponents().len(), 1);
}
#[test]
fn original_clock_domains_can_differ_and_local_member_headers_keep_original_ids() {
    let source = source();
    let policy = policy(&source, beatkernel_bms::BmsGaugeKind::Hard);
    let recorded = file(&source, &policy, None);
    let saved = Saved::new(&recorded);
    let mut options = CompetitionOptions::default();
    options.ghosts.push((OpponentKind::Other, saved.0.clone()));
    for id in [
        crate::local_players::PlayerId(7),
        crate::local_players::PlayerId(u32::MAX),
    ] {
        let engine = judge(&source, &policy);
        let live = LiveCompetition::prepare_member_section_with_policy(
            id,
            &options,
            &source,
            &engine,
            &policy,
            ClockDomainId(99),
            Timestamp::ZERO,
            0,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            live.native_policy_header().normalized_clock,
            ClockDomainId(99)
        );
        assert_eq!(live.native_policy_header().options, recorded.header.options);
        assert_eq!(live.archive_snapshot().unwrap().ghosts.len(), 1);
    }
    let builtin = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
    let engine = judge(&source, &builtin);
    assert!(
        LiveCompetition::prepare_member_section_with_policy(
            crate::local_players::PlayerId(1),
            &options,
            &source,
            &engine,
            &builtin,
            ClockDomainId(17),
            Timestamp::ZERO,
            0,
            None
        )
        .is_err()
    );
}

#[test]
fn selected_cohort_saved_ghosts_share_each_members_actual_capture_identity() {
    use crate::{
        native_cohort_setup::{CohortPreparation, prepare_cohort_with_policy},
        local_players::PlayerId,
        PreparedBms,
    };
    use beatkernel::{
        audio::{AudioFormat, PcmLimits, SampleBank},
        input::DeviceId,
    };
    let source = source();
    let policy = policy(&source, beatkernel_bms::BmsGaugeKind::Hard);
    let saved = Saved::new(&file(&source, &policy, Some(Timestamp::from_nanos(10))));
    let prepared = PreparedBms {
        compiled: source.compile().unwrap(),
        source,
        bank: SampleBank::new(
            AudioFormat::new(1000, 1).unwrap(),
            PcmLimits::new(64, 256, 4).unwrap(),
        )
        .unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    };
    let bindings = std::collections::BTreeMap::from([(0x11, 4)]);
    let config = CohortPreparation {
        host: ClockDomainId(99),
        output: ClockDomainId(2),
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        start: Timestamp::ZERO,
        end: Some(Timestamp::from_nanos(10)),
        chart_seed: 0,
        bindings: &bindings,
        record_replay: Some(std::path::Path::new("not-written.bkr")),
        replay_max_bytes: 65536,
        replay_max_records: 128,
    };
    let mut options = CompetitionOptions::default();
    options.ghosts.push((OpponentKind::Own, saved.0.clone()));
    let cohort = prepare_cohort_with_policy(
        &prepared,
        &[
            (PlayerId(7), DeviceId(1)),
            (PlayerId(u32::MAX), DeviceId(2)),
        ],
        &options,
        &config,
        &policy,
    )
    .unwrap();
    for state in &cohort.states {
        assert_eq!(
            state.competition.as_ref().unwrap().native_policy_header(),
            state.capture.as_ref().unwrap().header()
        );
        assert_eq!(
            state
                .competition
                .as_ref()
                .unwrap()
                .archive_snapshot()
                .unwrap()
                .ghosts
                .len(),
            1
        );
    }
}
