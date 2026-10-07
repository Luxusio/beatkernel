//! Recorded class scores are projections of actual judged stages and stored counts.
use crate::{
    competition::ScoreSummary,
    gauge::{BmsGauge, GaugeProfile},
    judgment_policy::BmsScoreSummary,
    local_players::PlayerId,
    play_policy::{ClassifiedWindow, ResolvedPlayPolicy},
    play_result::{CompletedPlayResult, PlayResultScope},
    replay_capture::LiveReplayCapture,
    result_archive::{ArchiveEntry, ArchivedScore, ResultArchive, decode_archive, encode_archive},
};
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeGrade, JudgeWindow},
    replay::codec::{ReplayCodecLimits, ReplayFile, decode_replay, encode_replay},
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsGaugeKind, BmsInputMode, BmsJudgment};

fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(ns),
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
fn actual_capture(finite: bool) -> (ReplayFile, ScoreSummary, CompletedPlayResult, GaugeProfile) {
    let source = beatkernel_bms::parse(
        "#BPM 60\n#TOTAL 320\n#WAV01 x.wav\n#00011:01010101\n#00111:01\n",
        Default::default(),
    )
    .unwrap();
    let classes = [
        (91, BmsJudgment::PGreat),
        (7, BmsJudgment::Great),
        (63, BmsJudgment::Good),
        (2, BmsJudgment::Bad),
    ];
    let windows: Vec<_> = classes
        .iter()
        .enumerate()
        .map(|(index, &(grade, judgment))| ClassifiedWindow {
            judgment,
            window: JudgeWindow {
                grade: JudgeGrade(grade),
                early: Duration::from_nanos(index as i64 + 1),
                late: Duration::from_nanos(index as i64 + 1),
            },
        })
        .collect();
    let policy = ResolvedPlayPolicy::bms(&source, BmsGaugeKind::Groove, &windows, 0).unwrap();
    let judge = crate::mine_plan::prepare_judge(
        &source,
        source.compile().unwrap().chart,
        policy.judge().clone(),
        BmsInputMode::ButtonOnly,
        1024,
    )
    .unwrap();
    let end = finite.then_some(Timestamp::from_nanos(4_000_000_010));
    let mut capture = LiveReplayCapture::new_with_policy(
        &judge,
        ClockDomainId(17),
        limits(),
        Timestamp::ZERO,
        0,
        end,
        BmsInputMode::ButtonOnly,
        None,
        &policy,
    )
    .unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(7u16),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let (producer, _consumer) = command_queue(8).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(17),
        ClockDomainId(17),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let mut gauge = BmsGauge::new(policy.gauge().clone());
    let mut score = ScoreSummary::default();
    for index in 0..4 {
        let ns = index * 1_000_000_000 + index + 1;
        for (offset, state) in [ButtonState::Down, ButtonState::Up].into_iter().enumerate() {
            let report = runtime
                .process_input(
                    PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(
                            DeviceId(9),
                            point(ns),
                            index as u64 * 2 + offset as u64,
                        ),
                        control: PhysicalControlId::keyboard(7u16),
                        state,
                    }),
                    &Identity,
                    point(ns),
                )
                .unwrap();
            assert!(report.judge_error.is_none());
            score.observe(&report.judge_events).unwrap();
            gauge
                .observe(&report.judge_events, &report.hazard_events)
                .unwrap();
            capture.record_report(&report).unwrap();
        }
    }
    let report = runtime
        .advance_to(point(4_000_000_010), &Identity, point(4_000_000_010))
        .unwrap();
    score.observe(&report.judge_events).unwrap();
    gauge
        .observe(&report.judge_events, &report.hazard_events)
        .unwrap();
    capture.record_report(&report).unwrap();
    assert_eq!((score.hits, score.misses), (4, 1));
    for object in source.compile().unwrap().chart.objects() {
        assert_eq!(
            runtime.judge().state(object.id),
            Some(beatkernel::interaction::InteractionState::Completed)
        );
    }
    let result = CompletedPlayResult::from_completed(Timestamp::ZERO, end, &gauge);
    let bytes = capture.into_bytes().unwrap();
    let file = decode_replay(&bytes, limits()).unwrap();
    assert_eq!(encode_replay(&file, limits()).unwrap(), bytes);
    (file, score, result, policy.gauge().clone())
}
fn archive(
    file: &ReplayFile,
    score: Option<&ScoreSummary>,
    result: CompletedPlayResult,
    profile: GaugeProfile,
) -> ResultArchive {
    let rows = [(PlayerId(1), result)];
    let identities = [(PlayerId(1), file.header.clone(), profile)];
    match score {
        Some(score) => {
            ResultArchive::from_completed_with_scores(&rows, &identities, &[(PlayerId(1), score)])
                .unwrap()
        }
        None => ResultArchive::from_completed(&rows, &identities).unwrap(),
    }
}
#[test]
fn actual_classified_full_and_finite_archives_roundtrip_without_new_score_fields() {
    for finite in [false, true] {
        let (file, score, result, profile) = actual_capture(finite);
        let archive = archive(&file, Some(&score), result, profile.clone());
        let bytes = encode_archive(&archive).unwrap();
        let decoded = decode_archive(&bytes).unwrap();
        assert_eq!(encode_archive(&decoded).unwrap(), bytes);
        assert_eq!(
            decoded.entries()[0].result.scope,
            if finite {
                PlayResultScope::PracticeSection {
                    start: Timestamp::ZERO,
                    end: Some(Timestamp::from_nanos(4_000_000_010)),
                }
            } else {
                PlayResultScope::FullSong
            }
        );
        assert_eq!(
            decoded.entries()[0].bms_score().unwrap(),
            Some(BmsScoreSummary {
                pgreat: 1,
                great: 1,
                good: 1,
                bad: 1,
                poor: 1,
                ex_score: 3,
            })
        );
        let no_score = archive_without_score(&file, result, profile);
        let bytes = encode_archive(&no_score).unwrap();
        let decoded = decode_archive(&bytes).unwrap();
        assert_eq!(decoded.entries()[0].bms_score().unwrap(), None);
        assert_eq!(encode_archive(&decoded).unwrap(), bytes);
    }
}
fn archive_without_score(
    file: &ReplayFile,
    result: CompletedPlayResult,
    profile: GaugeProfile,
) -> ResultArchive {
    archive(file, None, result, profile)
}
#[test]
fn archived_projection_uses_sorted_recorded_vector_and_refuses_unknown_or_overflowing_counts() {
    let (file, score, result, profile) = actual_capture(false);
    let archive = archive(&file, Some(&score), result, profile);
    let mut entry: ArchiveEntry = archive.entries()[0].clone();
    let stored = entry.score.as_mut().unwrap();
    stored.hits = 14;
    stored.misses = 3;
    stored.combo = 0;
    stored.max_combo = 0;
    stored.timing = Default::default();
    stored.grades = vec![(2, 5), (7, 4), (63, 3), (91, 2)];
    assert_eq!(
        entry.bms_score().unwrap(),
        Some(BmsScoreSummary {
            pgreat: 2,
            great: 4,
            good: 3,
            bad: 5,
            poor: 3,
            ex_score: 8,
        })
    );
    entry.score.as_mut().unwrap().grades[0].0 = 1;
    assert!(entry.bms_score().is_err());
    entry.score = Some(ArchivedScore {
        hits: u64::MAX / 2 + 1,
        misses: 0,
        combo: 0,
        max_combo: 0,
        grades: vec![(91, u64::MAX / 2 + 1)],
        timing: Default::default(),
    });
    assert!(entry.bms_score().is_err());
}
#[test]
fn constructors_and_wire_decode_refuse_classified_gauge_and_count_drift() {
    let (file, score, result, profile) = actual_capture(false);
    let rows = [(PlayerId(1), result)];
    assert!(
        ResultArchive::from_completed(
            &rows,
            &[(PlayerId(1), file.header.clone(), GaugeProfile::default())]
        )
        .is_err()
    );
    let mut wrong = score.clone();
    wrong.grades = [(999, 4)].into_iter().collect();
    assert!(
        ResultArchive::from_completed_with_scores(
            &rows,
            &[(PlayerId(1), file.header.clone(), profile.clone())],
            &[(PlayerId(1), &wrong)]
        )
        .is_err()
    );
    wrong.hits = u64::MAX / 2 + 1;
    wrong.misses = 0;
    wrong.grades = [(91, wrong.hits)].into_iter().collect();
    assert!(
        ResultArchive::from_completed_with_scores(
            &rows,
            &[(PlayerId(1), file.header.clone(), profile.clone())],
            &[(PlayerId(1), &wrong)]
        )
        .is_err()
    );
    let bytes = encode_archive(&archive(&file, Some(&score), result, profile)).unwrap();
    let profile_start = 24 + u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
    let mut corrupt = bytes.clone();
    let initial = u64::from_le_bytes(
        corrupt[profile_start..profile_start + 8]
            .try_into()
            .unwrap(),
    );
    corrupt[profile_start..profile_start + 8].copy_from_slice(&(initial + 1).to_le_bytes());
    assert!(decode_archive(&corrupt).is_err());
    // Score follows the full-song gauge result; timing has three populated optional values.
    let score_start = bytes.len() - (32 + 4 + 12 * 4 + 91);
    let mut corrupt = bytes.clone();
    corrupt[score_start + 36..score_start + 40].copy_from_slice(&1u32.to_le_bytes());
    assert!(decode_archive(&corrupt).is_err());
    let mut corrupt = bytes;
    let n = u64::MAX / 2 + 1;
    corrupt[score_start..score_start + 8].copy_from_slice(&(n + 3).to_le_bytes());
    corrupt[score_start + 36 + 3 * 12 + 4..score_start + 36 + 4 * 12]
        .copy_from_slice(&n.to_le_bytes());
    assert!(decode_archive(&corrupt).is_err());
}
#[test]
fn old_unclassified_archives_keep_same_bytes_and_do_not_guess_grade_meaning() {
    let (mut file, score, _, _) = actual_capture(false);
    let (inner, _) = crate::replay_judgment_policy::split_options(&file.header.options).unwrap();
    file.header.options = inner.to_vec();
    // Legacy compatibility intentionally allows a historical gauge distinct from setup metadata.
    let gauge = BmsGauge::default();
    let result = CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge);
    for legacy in [false, true] {
        if legacy {
            file.header = crate::result_archive::fixtures::header(0, None);
        }
        for score in [Some(&score), None] {
            let archive = archive(&file, score, result, GaugeProfile::default());
            let bytes = encode_archive(&archive).unwrap();
            let decoded = decode_archive(&bytes).unwrap();
            assert_eq!(decoded.entries()[0].bms_score().unwrap(), None);
            assert_eq!(encode_archive(&decoded).unwrap(), bytes);
        }
    }
}
