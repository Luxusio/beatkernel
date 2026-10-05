//! Deferred scored portable pumps backed by actual Mixer completion evidence.
use crate::{
    gameplay_presentation_port_fixtures::*,
    local_runtime::SoloRuntime,
    native_gameplay::{GameplaySession, run_gameplay_with_result_and_score_and_ports},
    native_solo_result_fixtures::{
        ResultHost, ResultObserver, result_source, result_judge, completion,
    },
    competition::ScoreSummary,
    completion::SongCompletion,
    play_result::{CompletedPlayResult, CompletedSoloPublicationError, PlayResultScope},
};
struct Play {
    device: MemoryDevice,
    runtime: SoloRuntime,
    gauge: BmsGauge,
    bgm: BgmFeeder,
    presentation: PresentationEstimator,
    pause: NativePause,
    end: Option<NativeEnd>,
    completion: Option<SongCompletion>,
    capture: Option<crate::replay_capture::LiveReplayCapture>,
    competition: Option<ResultObserver>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
}
impl Play {
    fn new(finite: bool) -> Self {
        let source = result_source(false);
        let (mut device, producer) = device(finite, vec![DeviceId(u64::MAX)]);
        device.close = false;
        let judge = result_judge(&source, false);
        let capture = if finite {
            None
        } else {
            let limits = beatkernel::replay::codec::ReplayCodecLimits::new(
                8192,
                32,
                4096,
                beatkernel::input::CodecLimits::new(4096, 1024).unwrap(),
            )
            .unwrap();
            Some(
                crate::replay_capture::LiveReplayCapture::new(&judge, ClockDomainId(1), limits)
                    .unwrap(),
            )
        };
        let mut runtime = SoloRuntime::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            bindings(None),
            judge,
            producer,
            vec![],
            8,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        if finite {
            runtime
                .set_song_end(Timestamp::from_nanos(10_000_000))
                .unwrap();
        }
        let (pause, end) = pause_end(finite);
        Self {
            device,
            runtime,
            gauge: BmsGauge::default(),
            bgm: bgm(),
            presentation: estimator(),
            pause,
            end,
            completion: (!finite).then(|| completion(&source)),
            capture,
            competition: Some(ResultObserver::default()),
            delivery: InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap(),
            pre: 0,
        }
    }
    fn run(
        &mut self,
        finite: bool,
        host: &mut ResultHost,
        score: &mut ScoreSummary,
        control: &mut Control,
    ) -> NativeGameplayResult<Option<CompletedPlayResult>> {
        run_gameplay_with_result_and_score_and_ports(
            &mut self.device,
            GameplaySession {
                runtime: &mut self.runtime,
                gauge: &mut self.gauge,
                bgm: &mut self.bgm,
                discipline: &mut self.presentation,
                pause: &mut self.pause,
                end: &mut self.end,
                completion: &mut self.completion,
                capture: &mut self.capture,
                competition: &mut self.competition,
                delivery: &mut self.delivery,
                pre_origin_inputs: &mut self.pre,
            },
            config(finite),
            control,
            host,
            score,
        )
    }
}
#[test]
fn real_full_and_finite_pumps_return_actual_typed_completion_and_exact_observed_score() {
    for finite in [false, true] {
        let mut play = Play::new(finite);
        let mut host = ResultHost::default();
        let mut score = ScoreSummary::default();
        let result = play
            .run(finite, &mut host, &mut score, &mut Control::default())
            .unwrap()
            .unwrap();
        assert_eq!(
            result.scope(),
            if finite {
                PlayResultScope::PracticeSection {
                    start: Timestamp::ZERO,
                    end: Some(Timestamp::from_nanos(10_000_000)),
                }
            } else {
                PlayResultScope::FullSong
            }
        );
        assert_eq!(result.gauge(), *play.gauge.snapshot());
        assert_eq!(host.results, [result]);
        assert_eq!(play.competition.as_ref().unwrap().marks, 1);
        assert!(play.device.step >= 3);
        let committed_hits = host
            .base
            .solo
            .iter()
            .flat_map(|report| &report.judge_events)
            .filter(|event| matches!(event.outcome, beatkernel::judge::JudgeOutcome::Hit { .. }))
            .count() as u64;
        assert_eq!(score.hits, committed_hits);
        assert_eq!(score.misses, 0);
        assert_eq!(score.hits, if finite { 0 } else { 1 });
        assert_eq!(score.timing.count(), score.hits);
        if !finite {
            assert_eq!(score.grades.get(&1), Some(&1));
            assert_eq!(score.timing.exact(), 1);
            let archive = crate::native_completed_save::solo_archive_with_score(
                &Ok(Some(result)),
                play.capture.as_ref(),
                play.gauge.profile(),
                &score,
            )
            .unwrap()
            .unwrap();
            let bytes = crate::result_archive::encode_archive(&archive).unwrap();
            let decoded = crate::result_archive::decode_archive(&bytes).unwrap();
            assert_eq!(
                decoded.entries()[0].score,
                Some(crate::result_archive::ArchivedScore::from_summary(&score).unwrap())
            );
            assert_eq!(
                decoded.entries()[0].header,
                *play.capture.as_ref().unwrap().header()
            );
            assert_eq!(decoded.entries()[0].result.gauge, result.gauge());
        }
        assert_eq!(play.device.report.unwrap().pending_commands, 0);
    }
}
#[test]
fn cancel_retains_committed_prefix_without_completion_and_publication_failure_keeps_scored_proof() {
    for cancel_after_hit in [false, true] {
        let mut play = Play::new(false);
        let mut host = ResultHost::default();
        if cancel_after_hit {
            host.cancel_at = Some(Timestamp::from_nanos(20_000_000));
        } else {
            host.base.cancel = true;
        }
        let mut score = ScoreSummary::default();
        assert!(
            play.run(false, &mut host, &mut score, &mut Control::default())
                .unwrap()
                .is_none()
        );
        assert_eq!(score.hits, u64::from(cancel_after_hit));
        assert!(host.results.is_empty());
        assert_eq!(play.competition.as_ref().unwrap().marks, 0);
    }
    let mut play = Play::new(false);
    let mut host = ResultHost {
        reject: true,
        ..Default::default()
    };
    let mut score = ScoreSummary::default();
    let error = play
        .run(false, &mut host, &mut score, &mut Control::default())
        .unwrap_err();
    let refusal = error
        .downcast_ref::<CompletedSoloPublicationError>()
        .unwrap();
    assert_eq!(host.results, [refusal.result]);
    assert_eq!(refusal.result.gauge(), *play.gauge.snapshot());
    assert_eq!(score.hits, 1);
    assert_eq!(score.grades.get(&1), Some(&1));
    assert_eq!(play.competition.as_ref().unwrap().marks, 1);
}
#[test]
fn every_nondefault_initial_score_is_refused_before_device_control_host_or_judge_effects() {
    for field in 0..4 {
        let mut play = Play::new(false);
        let mut score = ScoreSummary::default();
        match field {
            0 => score.hits = 1,
            1 => score.misses = 1,
            2 => {
                score.grades.insert(u32::MAX, 0);
            }
            _ => score.max_combo = 1,
        }
        let before = score.clone();
        let hash = play.runtime.judge().stable_hash().unwrap();
        let mut host = ResultHost::default();
        let mut control = Control::default();
        assert!(
            play.run(false, &mut host, &mut score, &mut control)
                .is_err()
        );
        assert_eq!(score, before);
        assert_eq!(play.runtime.judge().stable_hash().unwrap(), hash);
        assert_eq!(play.device.step, 0);
        assert!(play.device.report.is_none());
        assert_eq!(control.waits, 0);
        assert!(host.base.solo.is_empty());
        assert!(host.results.is_empty());
        assert!(host.base.pause_states.is_empty());
        assert_eq!(play.competition.as_ref().unwrap().marks, 0);
    }
}
