//! Deferred actual report observation and explicit host delegation.
use crate::{
    competition::{ScoreSummary, CompetitionError},
    gameplay_presentation_port_fixtures::*,
    local_runtime::{SoloRuntime, PlayerReport},
    local_players::PlayerId,
    native_gameplay_host::NativeScoreHost,
    play_result::CompletedPlayResult,
    timing::TimingSummary,
};
use std::{error::Error, fmt, sync::Arc};
#[derive(Debug)]
struct Refused(Arc<u64>);
impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("injected host refusal")
    }
}
impl Error for Refused {}
#[derive(Default)]
struct HostSpy {
    calls: Vec<&'static str>,
    refusal: Option<Box<dyn Error>>,
    report: Option<*const RuntimeReport>,
    local: Vec<PlayerId>,
    completed: Vec<CompletedPlayResult>,
}
impl NativeGameplayHost for HostSpy {
    fn cancelled(&self) -> bool {
        true
    }
    fn pause_requested(&self) -> bool {
        true
    }
    fn retry_pause_publication(&mut self) {
        self.calls.push("retry");
    }
    fn publish_pause(&mut self, _: PauseState) {
        self.calls.push("pause");
    }
    fn publish_section_end(&mut self, _: Timestamp) {
        self.calls.push("section");
    }
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        self.calls.push("report");
        self.report = Some(report as *const _);
        match self.refusal.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()> {
        self.calls.push("local");
        self.local = reports.iter().map(|row| row.player).collect();
        Ok(())
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {
        self.calls.push("diagnostic");
    }
    fn publish_completed_solo(&mut self, result: CompletedPlayResult) -> NativeGameplayResult<()> {
        self.calls.push("completed-solo");
        self.completed.push(result);
        Ok(())
    }
    fn publish_completed_local(
        &mut self,
        rows: &[(PlayerId, CompletedPlayResult)],
    ) -> NativeGameplayResult<()> {
        self.calls.push("completed-local");
        self.local = rows.iter().map(|row| row.0).collect();
        Ok(())
    }
}
struct SameDomain;
impl beatkernel::time::ClockMapper for SameDomain {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (point.domain == target).then_some(point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn report_for_grade(grade: u32) -> RuntimeReport {
    let source = source();
    let (_, producer) = device(false, vec![]);
    let mut runtime = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings(None),
        JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
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
        producer,
        vec![],
        8,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let report = runtime
        .process_input(
            PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(u64::MAX), point(1, 20_000_000), u64::MAX),
                control: PhysicalControlId::keyboard(4),
                state: ButtonState::Down,
            }),
            &SameDomain,
            point(2, 0),
        )
        .unwrap();
    assert_eq!(report.judge_events.len(), 1);
    report
}
fn committed_report() -> RuntimeReport {
    report_for_grade(1)
}
#[test]
fn genuine_committed_runtime_report_scores_once_before_original_borrowed_publication() {
    let report = committed_report();
    let mut score = ScoreSummary::default();
    let mut host = HostSpy::default();
    NativeScoreHost::new(&mut host, &mut score)
        .publish_report(&report)
        .unwrap();
    assert_eq!(host.calls, ["report"]);
    assert_eq!(host.report, Some(&report as *const _));
    assert_eq!(
        (score.hits, score.misses, score.combo, score.max_combo),
        (1, 0, 1, 1)
    );
    assert_eq!(score.grades.get(&1), Some(&1));
    assert_eq!(
        (
            score.timing.count(),
            score.timing.exact(),
            score.timing.last_ns()
        ),
        (1, 1, Some(0))
    );
}
#[test]
fn successful_score_retains_original_boxed_publication_error_without_rollback() {
    let report = committed_report();
    let token = Arc::new(93);
    let boxed = Box::new(Refused(token.clone()));
    let pointer = boxed.as_ref() as *const Refused;
    let mut host = HostSpy {
        refusal: Some(boxed),
        ..Default::default()
    };
    let mut score = ScoreSummary::default();
    let error = NativeScoreHost::new(&mut host, &mut score)
        .publish_report(&report)
        .unwrap_err();
    let original = error.downcast_ref::<Refused>().unwrap();
    assert_eq!(original as *const Refused, pointer);
    assert!(Arc::ptr_eq(&original.0, &token));
    assert_eq!(score.hits, 1);
    assert_eq!(host.calls, ["report"]);
}
#[test]
fn numeric_score_fault_is_atomic_and_wins_even_when_underlying_host_also_refuses() {
    let report = committed_report();
    let mut score = ScoreSummary {
        hits: u64::MAX,
        ..Default::default()
    };
    let before = score.clone();
    let mut host = HostSpy {
        refusal: Some(Box::new(Refused(Arc::new(7)))),
        ..Default::default()
    };
    let error = NativeScoreHost::new(&mut host, &mut score)
        .publish_report(&report)
        .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<CompetitionError>(),
        Some(CompetitionError::ScoreOverflow)
    ));
    assert_eq!(score, before);
    assert_eq!(host.calls, ["report"]);
    assert!(host.refusal.is_none());
}
#[test]
fn timing_refusal_precedes_score_fault_and_empty_report_still_reaches_host() {
    let mut report = committed_report();
    let mut score = ScoreSummary {
        hits: u64::MAX,
        timing: TimingSummary::exhausted_for_fixture(),
        ..Default::default()
    };
    let before = score.clone();
    let mut host = HostSpy::default();
    let error = NativeScoreHost::new(&mut host, &mut score)
        .publish_report(&report)
        .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<CompetitionError>(),
        Some(CompetitionError::TimingOverflow)
    ));
    assert_eq!(score, before);
    report.judge_events.clear();
    NativeScoreHost::new(&mut host, &mut score)
        .publish_report(&report)
        .unwrap();
    assert_eq!(score, before);
    assert_eq!(host.calls, ["report", "report"]);
}
#[test]
fn all_commands_and_typed_results_delegate_while_local_reports_never_merge_solo_score() {
    let report = committed_report();
    let rows = [PlayerReport {
        player: PlayerId(u32::MAX),
        report,
    }];
    let result = CompletedPlayResult::from_completed(Timestamp::ZERO, None, &BmsGauge::default());
    let mut score = ScoreSummary::default();
    let mut host = HostSpy::default();
    {
        let mut observer = NativeScoreHost::new(&mut host, &mut score);
        assert!(observer.cancelled());
        assert!(observer.pause_requested());
        observer.retry_pause_publication();
        observer.publish_pause(PauseState::Paused);
        observer.publish_section_end(Timestamp::from_nanos(604_800_000_000_000));
        observer.diagnostic(NativeGameplayDiagnostic::SongProgress(
            Timestamp::from_nanos(9_007_199_254_740_993),
        ));
        observer.publish_local_reports(&rows).unwrap();
        observer.publish_completed_solo(result).unwrap();
        observer
            .publish_completed_local(&[(PlayerId(u32::MAX), result), (PlayerId(7), result)])
            .unwrap();
    }
    assert_eq!(score, ScoreSummary::default());
    assert_eq!(
        host.calls,
        [
            "retry",
            "pause",
            "section",
            "diagnostic",
            "local",
            "completed-solo",
            "completed-local"
        ]
    );
    assert_eq!(host.local, [PlayerId(u32::MAX), PlayerId(7)]);
    assert_eq!(host.completed, [result]);
}

#[test]
fn genuine_custom_judge_grade_ids_zero_and_maximum_remain_opaque_in_score_host() {
    for grade in [0, u32::MAX] {
        let report = report_for_grade(grade);
        let mut host = HostSpy::default();
        let mut score = ScoreSummary::default();
        NativeScoreHost::new(&mut host, &mut score)
            .publish_report(&report)
            .unwrap();
        assert_eq!(
            score
                .grades
                .iter()
                .map(|(&id, &count)| (id, count))
                .collect::<Vec<_>>(),
            [(grade, 1)]
        );
        assert_eq!(
            (score.hits, score.timing.count(), score.timing.exact()),
            (1, 1, 1)
        );
    }
}
