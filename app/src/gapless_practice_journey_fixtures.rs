// Genuine shared pump, retained Mixer, original acquisition and captures.
use super::*;
use crate::{
    gameplay_competition::NoopSoloCompetition,
    practice_control::{PracticeCapability, PracticeReply, PracticeRequest},
    practice_playback::{PracticeMember, PracticePlayback, PracticeRecordingPort},
    session_launch::SessionLaunch,
};
use beatkernel::audio::{
    practice_queue, CommandScope, PracticeCue, PracticeLimits, PracticeRegion,
    PreparedPracticeProgram,
};

struct PracticeHost {
    inner: Host,
    generations: Vec<u64>,
    commits: Vec<u64>,
    replies: Vec<PracticeReply>,
    requests: VecDeque<PracticeRequest>,
    refuse_visual: bool,
    prepared_launch: Option<SessionLaunch>,
    applied_launch: Option<SessionLaunch>,
    identities: Vec<u64>,
    stop_counts: Option<Rc<Cell<u64>>>,
    transition_stop_counts: Vec<u64>,
}
impl NativeGameplayHost for PracticeHost {
    fn prepare_policies(
        &mut self,
        policies: &[(PlayerId, &crate::gauge::GaugeProfile)],
    ) -> NativeGameplayResult<()> {
        NoopGameplayHost.prepare_policies(policies)
    }
    fn prepare_play_policies(
        &mut self,
        policies: &[(PlayerId, &ResolvedPlayPolicy)],
    ) -> NativeGameplayResult<()> {
        self.inner.prepare_play_policies(policies)
    }
    fn cancelled(&self) -> bool {
        self.inner.cancelled()
    }
    fn pause_requested(&self) -> bool {
        false
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, state: PauseState) {
        self.inner.publish_pause(state);
    }
    fn publish_section_end(&mut self, end: Timestamp) {
        self.inner.publish_section_end(end);
    }
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        self.inner.publish_report(report)
    }
    fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()> {
        self.inner.publish_local_reports(reports)
    }
    fn diagnostic(&mut self, diagnostic: NativeGameplayDiagnostic<'_>) {
        self.inner.diagnostic(diagnostic);
    }
    fn advertise_practice(&mut self, cap: Option<PracticeCapability>) -> NativeGameplayResult<()> {
        if let Some(cap) = cap {
            self.generations.push(cap.generation);
        }
        Ok(())
    }
    fn take_practice_request(&mut self) -> NativeGameplayResult<Option<PracticeRequest>> {
        // Requests become available only after bootstrap has committed.
        Ok(
            if self.generations.is_empty()
                || self
                    .stop_counts
                    .as_ref()
                    .is_some_and(|count| count.get() == 0)
            {
                None
            } else {
                self.requests.pop_front()
            },
        )
    }
    fn commit_practice_reply(&mut self, reply: &PracticeReply) -> NativeGameplayResult<()> {
        self.replies.push(reply.clone());
        Ok(())
    }
    fn prepare_practice_presentation(
        &mut self,
        generation: u64,
        attempts: &[(PlayerId, &crate::practice_session::PreparedPracticeAttempt)],
    ) -> NativeGameplayResult<crate::player::PreparedPracticePresentation> {
        if let Some(count) = &self.stop_counts {
            self.transition_stop_counts.push(count.get());
        }
        self.prepared_launch = attempts
            .first()
            .map(|(_, attempt)| attempt.next_launch.clone());
        crate::player::prepare_practice_presentation(generation, attempts)
    }
    fn apply_practice_identity(
        &mut self,
        prepared: &mut crate::player::PreparedPracticePresentation,
        generation: u64,
    ) {
        crate::player::apply_practice_identity(prepared, generation);
        self.applied_launch = self.prepared_launch.take();
        self.identities.push(generation);
    }
    fn commit_practice_presentation(
        &mut self,
        prepared: crate::player::PreparedPracticePresentation,
        generation: u64,
    ) -> NativeGameplayResult<()> {
        if self.refuse_visual {
            return Err("literal visual refusal after application".into());
        }
        crate::player::commit_practice_presentation(prepared, generation)?;
        self.commits.push(generation);
        Ok(())
    }
    fn publish_completed_solo(&mut self, result: CompletedPlayResult) -> NativeGameplayResult<()> {
        self.inner.publish_completed_solo(result)
    }
}
#[derive(Default)]
struct Recordings {
    paths: Vec<std::path::PathBuf>,
    captures: Vec<LiveReplayCapture>,
    refuse: bool,
    fail_at: Option<usize>,
    attempted_paths: Vec<std::path::PathBuf>,
}
impl PracticeRecordingPort for Recordings {
    fn archive(
        &mut self,
        player: PlayerId,
        path: &std::path::Path,
        capture: LiveReplayCapture,
    ) -> NativeGameplayResult<()> {
        assert_eq!(player, PlayerId(1));
        self.attempted_paths.push(path.to_owned());
        if self.paths.iter().any(|existing| existing == path) {
            return Err("exclusive recording path already exists".into());
        }
        if self.refuse || self.fail_at == Some(self.attempted_paths.len()) {
            return Err("literal recording refusal".into());
        }
        self.paths.push(path.to_owned());
        self.captures.push(capture);
        Ok(())
    }
}
fn practice_fixture(repeat: bool) -> (Solo, PracticePlayback, PracticeHost) {
    practice_fixture_end(repeat, 30_000_000)
}
fn practice_fixture_end(repeat: bool, end: i64) -> (Solo, PracticePlayback, PracticeHost) {
    let chart = source(false);
    practice_fixture_with_policy(repeat, end, chart, |chart| policy(chart, false))
}
fn practice_fixture_with_policy(
    repeat: bool,
    end: i64,
    chart: beatkernel_bms::BmsChart,
    select_policy: impl Fn(&beatkernel_bms::BmsChart) -> ResolvedPlayPolicy,
) -> (Solo, PracticePlayback, PracticeHost) {
    let selected = select_policy(&chart);
    let member_policy = select_policy(&chart);
    let mut solo = Solo::new(Mode::Normal, false, false);
    let (cold_device, producer, cold_host) =
        device(16, false, Mode::Normal, vec![DeviceId(u64::MAX)]);
    solo.device = cold_device;
    solo.host = cold_host;
    solo.runtime = SoloRuntime::new(
        ClockDomainId(3),
        ClockDomainId(2),
        Transport::new(logical(0).timestamp, Timestamp::ZERO, Rate::NORMAL),
        bindings(None, false),
        JudgeEngine::new(
            chart.compile().unwrap().chart,
            chart.rules(),
            selected.judge().clone(),
        )
        .unwrap(),
        producer,
        sounds(&chart, 0),
        8,
    )
    .unwrap();
    solo.runtime
        .set_processing_clock(RuntimeProcessingClock::Disabled);
    solo.gauge = BmsGauge::new(selected.gauge().try_copy().unwrap());
    solo.selected = selected;
    solo.device.stop_after = Some(16);
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 8], pcm).unwrap(),
    )
    .unwrap();
    let program = PreparedPracticeProgram::new(
        &bank,
        vec![PracticeCue {
            voice: VoiceId(800),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        }],
        PracticeLimits::new(4, 1, 64, 8, 64).unwrap(),
    )
    .unwrap();
    let (controller, endpoint) = practice_queue(&program).unwrap();
    let region = PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(end), repeat).unwrap();
    solo.device
        .mixer
        .install_practice(program, endpoint, region)
        .unwrap();
    solo.runtime.set_audio_scope(CommandScope(1));
    solo.runtime
        .set_song_end(Timestamp::from_nanos(end))
        .unwrap();
    solo.capture = Some(
        LiveReplayCapture::new_with_policy(
            solo.runtime.judge(),
            ClockDomainId(3),
            limits(),
            Timestamp::ZERO,
            0,
            Some(Timestamp::from_nanos(end)),
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            &solo.selected,
        )
        .unwrap(),
    );
    solo.presentation
        .admit(NativeAudioSnapshot {
            epoch: 0,
            basis: solo.device.creation_basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: raw(0),
                target: host(0),
            }),
        })
        .unwrap();
    let playback = PracticePlayback::new(
        controller,
        chart,
        vec![PracticeMember {
            player: PlayerId(1),
            policy: member_policy,
            launch: SessionLaunch::new(vec![
                "--chart".into(),
                "fixture.bms".into(),
                "--record-replay".into(),
                "practice.bkr".into(),
            ])
            .unwrap(),
            chart_seed: 0,
            capture_limits: Some(limits()),
        }],
        region,
        Timestamp::from_nanos(200_000_000),
        solo.device.creation_basis,
        2,
    )
    .unwrap();
    let (_, _, replacement_host) = device(16, false, Mode::Normal, vec![]);
    let inner = std::mem::replace(&mut solo.host, replacement_host);
    let host = PracticeHost {
        inner,
        generations: vec![],
        commits: vec![],
        replies: vec![],
        requests: VecDeque::new(),
        refuse_visual: false,
        prepared_launch: None,
        applied_launch: None,
        identities: Vec::new(),
        stop_counts: None,
        transition_stop_counts: Vec::new(),
    };
    (solo, playback, host)
}
fn run_practice(
    solo: &mut Solo,
    playback: &mut PracticePlayback,
    host: &mut PracticeHost,
    recording: &mut Recordings,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    let mut score = crate::competition::ScoreSummary::default();
    let outcome = run_practice_pump(solo, playback, host, recording, &mut score);
    let completed = outcome.as_ref().ok().copied().flatten();
    let path = playback.members()[0]
        .launch
        .args()
        .windows(2)
        .find(|pair| pair[0] == "--record-replay")
        .map(|pair| std::path::PathBuf::from(&pair[1]))
        .unwrap();
    // The launcher, rather than the practice pump, owns final replay and sidecar
    // saving. Exercise its actual finisher for completion, cancellation and error.
    if outcome.is_ok() || playback.generation() > 1 {
        assert!(
            solo.capture.is_some(),
            "pump must retain its final live capture"
        );
    }
    crate::native_finish::finish_solo_with_result_and_score(
        outcome,
        Ok(()),
        Ok(()),
        None,
        solo.capture.take(),
        solo.gauge.profile(),
        &score,
        Some(&path),
        |capture, actual, _failed| {
            assert_eq!(actual, Some(path.as_path()));
            match capture {
                Some(capture) => recording.archive(PlayerId(1), actual.unwrap(), capture),
                None => Ok(()), // A retired capture's failed effect already consumed it.
            }
        },
        |archive, actual| {
            assert_eq!(actual, Some(path.as_path()));
            assert_eq!(archive.entries().len(), 1);
            let entry = &archive.entries()[0];
            let result = completed.unwrap();
            assert_eq!(entry.result.scope, result.scope());
            assert_eq!(entry.result.outcome, result.outcome());
            assert_eq!(entry.result.gauge, result.gauge());
            assert_eq!(entry.profile, *solo.gauge.profile());
            assert_eq!(
                entry.score,
                Some(crate::result_archive::ArchivedScore::from_summary(&score)?)
            );
            Ok(())
        },
    )?;
    Ok(completed)
}
fn run_practice_pump(
    solo: &mut Solo,
    playback: &mut PracticePlayback,
    host: &mut PracticeHost,
    recording: &mut Recordings,
    score: &mut crate::competition::ScoreSummary,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    let mut competition = Some(NoopSoloCompetition);
    let mut config = audio_config(false, false);
    config.gameplay.end_song = playback.section_end();
    let session = AudioGameplaySession {
        session: GameplaySession {
            runtime: &mut solo.runtime,
            gauge: &mut solo.gauge,
            bgm: &mut solo.bgm,
            discipline: &mut solo.presentation,
            pause: &mut solo.pause,
            end: &mut solo.end,
            completion: &mut solo.completion,
            capture: &mut solo.capture,
            competition: &mut competition,
            delivery: &mut solo.delivery,
            pre_origin_inputs: &mut solo.pre,
        },
        merger: &mut solo.merger,
    };
    let stop_counts = host.stop_counts.clone();
    let mut score_host = crate::native_gameplay_host::NativeScoreHost::new(host, score);
    if let Some(count) = stop_counts {
        let mut device = ExclusiveEdgeDevice(&mut solo.device, false, None, Some(count));
        super::super::run_gameplay_audio_with_practice_and_ports(
            &mut device,
            session,
            config,
            &mut Control::default(),
            &mut score_host,
            playback,
            recording,
        )
    } else {
        super::super::run_gameplay_audio_with_practice_and_ports(
            &mut solo.device,
            session,
            config,
            &mut Control::default(),
            &mut score_host,
            playback,
            recording,
        )
    }
}
#[test]
fn retained_solo_actual_pump_loops_original_pcm_and_archives_each_attempt() {
    let (mut solo, mut playback, mut host) = practice_fixture(true);
    let basis = solo.device.creation_basis;
    let mut recording = Recordings::default();
    assert!(
        run_practice(&mut solo, &mut playback, &mut host, &mut recording)
            .unwrap()
            .is_none()
    );
    assert_eq!(solo.device.creation_basis, basis);
    assert_eq!(solo.presentation.authority().epoch().id, 0);
    assert!(playback.generation() >= 5);
    assert_eq!(host.commits.len(), (playback.generation() - 1) as usize);
    assert_eq!(recording.paths.len(), playback.generation() as usize);
    assert_eq!(recording.paths[0], std::path::Path::new("practice.bkr"));
    for (index, path) in recording.paths.iter().enumerate().skip(1) {
        assert_eq!(
            path,
            &std::path::PathBuf::from(format!("practice.retry{index}.bkr"))
        );
    }
    assert!(recording
        .captures
        .iter()
        .all(|capture| capture.header().normalized_clock == ClockDomainId(3)));
    assert!(solo.capture.is_none());
    // Original prepared BGM starts again every 30 source frames; no PCM copy or
    // new Mixer replaces the original physical grid.
    for frame in [0, 30, 60, 90, 120] {
        assert_eq!(solo.device.pcm[frame], 0.25);
    }
    assert_eq!(
        solo.runtime.audio_scope(),
        CommandScope(playback.generation())
    );
}

fn retained_solo_failed_attempt_keeps_rendered_stop_history(scrub: bool) {
    // At BPM 3000 a measure lasts 80 ms: the first head is at 10 ms.
    // Neither keysound is played, so the real failure-owned Stops must raise
    // the Mixer's cumulative unknown-stop count before this 100 ms section ends.
    let chart = beatkernel_bms::parse(
        "#BPM 3000\n#TOTAL 100\n#WAV01 key.wav\n#00011:0001000000000000\n#00112:01",
        Default::default(),
    )
    .unwrap();
    let select_policy = |chart: &beatkernel_bms::BmsChart| {
        ResolvedPlayPolicy::bms(
            chart,
            beatkernel_bms::BmsGaugeKind::Hazard,
            &[ClassifiedWindow {
                judgment: beatkernel_bms::BmsJudgment::PGreat,
                window: JudgeWindow {
                    grade: JudgeGrade(91),
                    early: Duration::ZERO,
                    late: Duration::ZERO,
                },
            }],
            0,
        )
        .unwrap()
    };
    let (mut solo, mut playback, mut host) =
        practice_fixture_with_policy(!scrub, 100_000_000, chart, select_policy);
    solo.device.sources.clear();
    assert!(solo.gauge.snapshot().failure.is_none());
    assert!(solo.gauge.snapshot().level_units > 0);
    assert_eq!(solo.device.mixer.counters().unknown_stops, 0);
    let basis = solo.device.creation_basis;
    let counts = Rc::new(Cell::new(0));
    host.stop_counts = Some(counts.clone());
    if scrub {
        // The host releases this request only after an actual render has
        // executed the failed attempt's Stops, never from admission alone.
        host.requests.push_back(PracticeRequest {
            id: 101,
            generation: 1,
            action: crate::practice_control::PracticeAction::Scrub {
                target: Timestamp::from_nanos(40_000_000),
            },
        });
    }
    let mut recording = Recordings::default();
    let result = run_practice(&mut solo, &mut playback, &mut host, &mut recording).unwrap();
    assert!(playback.generation() >= 2);
    assert!(!host.transition_stop_counts.is_empty());
    assert!(host.transition_stop_counts.iter().all(|count| *count > 0));
    assert!(counts.get() > 0);
    assert_eq!(counts.get(), solo.device.mixer.counters().unknown_stops);
    assert!(host.inner.reports.iter().any(|report| {
        report
            .judge_events
            .iter()
            .any(|event| matches!(event.outcome, JudgeOutcome::Miss { .. }))
    }));
    assert_eq!(solo.device.creation_basis, basis);
    assert_eq!(solo.presentation.authority().epoch().id, 0);
    // Ended advances audio receipt ownership but installs no fresh Runtime or
    // recording attempt. The scrub's sole fresh attempt remains generation 2.
    let attempt_generation = if scrub { 2 } else { playback.generation() };
    assert_eq!(solo.runtime.audio_scope(), CommandScope(attempt_generation));
    assert_eq!(recording.paths.len(), attempt_generation as usize);
    assert_eq!(recording.captures.len(), recording.paths.len());
    assert!(recording.captures.iter().all(|capture| {
        capture.header().normalized_clock == ClockDomainId(3)
            && capture
                .records()
                .iter()
                .any(|record| record.operation == ReplayOperation::Advance)
    }));
    assert_eq!(
        recording.paths.last().unwrap(),
        &std::path::PathBuf::from(format!("practice.retry{}.bkr", attempt_generation - 1))
    );
    // run_practice invokes the actual launcher finisher with the current capture.
    assert!(solo.capture.is_none());
    if scrub {
        assert_eq!(
            result.unwrap().scope(),
            crate::play_result::PlayResultScope::PracticeSection {
                start: Timestamp::from_nanos(40_000_000),
                end: Some(Timestamp::from_nanos(100_000_000)),
            }
        );
        assert!(playback.ended());
        assert_eq!(playback.generation(), 3);
        assert_eq!(host.identities, [2]);
        assert_eq!(host.commits, [2]);
        assert_eq!(host.replies.len(), 1);
        assert_eq!(host.replies[0].id, 101);
        assert_eq!(host.replies[0].result.as_ref().unwrap().generation, 2);
        assert_eq!(host.inner.completed.len(), 1);
    } else {
        assert!(result.is_none());
        assert_eq!(solo.device.pcm[100], 0.25);
    }
}

#[test]
fn retained_solo_loop_preserves_actual_failed_attempt_stop_evidence() {
    retained_solo_failed_attempt_keeps_rendered_stop_history(false);
}

#[test]
fn retained_solo_scrub_preserves_actual_failed_attempt_stop_evidence() {
    retained_solo_failed_attempt_keeps_rendered_stop_history(true);
}

#[test]
fn retained_solo_terminal_receipt_finishes_without_legacy_finite_mixer_marker() {
    let (mut solo, mut playback, mut host) = practice_fixture(false);
    let mut recording = Recordings::default();
    assert!(
        run_practice(&mut solo, &mut playback, &mut host, &mut recording)
            .unwrap()
            .is_some()
    );
    assert!(playback.ended());
    assert!(solo
        .device
        .report
        .unwrap()
        .playback_end_physical_frame
        .is_none());
    assert_eq!(host.inner.completed.len(), 1);
    assert_eq!(recording.paths, [std::path::PathBuf::from("practice.bkr")]);
}
#[test]
fn retained_solo_completed_pump_preserves_final_capture_for_actual_replay_and_score_sidecar() {
    let (mut solo, mut playback, mut host) = practice_fixture(false);
    let mut recording = Recordings::default();
    let mut score = crate::competition::ScoreSummary::default();
    let outcome = run_practice_pump(
        &mut solo,
        &mut playback,
        &mut host,
        &mut recording,
        &mut score,
    );
    let completed = outcome.as_ref().unwrap().unwrap();
    assert!(playback.ended());
    assert_eq!(host.inner.completed, [completed]);
    assert!(recording.paths.is_empty());
    assert!(recording.attempted_paths.is_empty());
    let capture = solo
        .capture
        .take()
        .expect("completed pump must retain final capture");
    assert!(capture
        .records()
        .iter()
        .any(|record| record.operation == ReplayOperation::Advance));
    let header = capture.header().clone();
    let expected_score = crate::result_archive::ArchivedScore::from_summary(&score).unwrap();
    assert!(score.hits + score.misses > 0);
    let path = playback.members()[0]
        .launch
        .args()
        .windows(2)
        .find(|pair| pair[0] == "--record-replay")
        .map(|pair| std::path::PathBuf::from(&pair[1]))
        .unwrap();
    assert_eq!(path, std::path::Path::new("practice.bkr"));
    let saved = std::cell::RefCell::new(Vec::new());
    crate::native_finish::finish_solo_with_result_and_score(
        outcome,
        Ok(()),
        Ok(()),
        None,
        Some(capture),
        solo.gauge.profile(),
        &score,
        Some(&path),
        |capture, actual, failed| {
            assert!(!failed);
            assert_eq!(actual, Some(path.as_path()));
            let file = capture.unwrap().into_file();
            assert_eq!(file.header, header);
            let bytes = beatkernel::replay::codec::encode_replay(&file, limits())?;
            let decoded = beatkernel::replay::codec::decode_replay(&bytes, limits())?;
            assert_eq!(decoded.header, file.header);
            assert_eq!(decoded.records, file.records);
            saved.borrow_mut().push("replay");
            Ok(())
        },
        |archive, actual| {
            assert_eq!(actual, Some(path.as_path()));
            assert_eq!(&*saved.borrow(), &["replay"]);
            let entry = &archive.entries()[0];
            assert_eq!(entry.player, PlayerId(1));
            assert_eq!(entry.header, header);
            assert_eq!(entry.profile, *solo.gauge.profile());
            assert_eq!(entry.result.scope, completed.scope());
            assert_eq!(entry.result.outcome, completed.outcome());
            assert_eq!(entry.result.gauge, completed.gauge());
            assert_eq!(entry.score, Some(expected_score));
            let decoded = crate::result_archive::decode_archive(
                &crate::result_archive::encode_archive(archive)?,
            )?;
            assert_eq!(decoded.entries(), archive.entries());
            saved.borrow_mut().push("sidecar");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(&*saved.borrow(), &["replay", "sidecar"]);
}
#[test]
fn retained_solo_recording_failure_stops_before_installing_next_attempt() {
    let (mut solo, mut playback, mut host) = practice_fixture(true);
    let mut recording = Recordings {
        refuse: true,
        ..Default::default()
    };
    let error = run_practice(&mut solo, &mut playback, &mut host, &mut recording).unwrap_err();
    assert_eq!(error.to_string(), "literal recording refusal");
    assert_eq!(playback.generation(), 1);
    assert!(host.commits.is_empty());
    assert_eq!(solo.runtime.audio_scope(), CommandScope(1));
}

#[test]
fn retained_solo_requested_scrub_uses_actual_cut_and_exact_original_anchor() {
    let (mut solo, mut playback, mut host) = practice_fixture_end(false, 100_000_000);
    host.requests.push_back(PracticeRequest {
        id: 71,
        generation: 1,
        action: crate::practice_control::PracticeAction::Scrub {
            target: Timestamp::from_nanos(10_000_001),
        },
    });
    let mut recording = Recordings::default();
    run_practice(&mut solo, &mut playback, &mut host, &mut recording).unwrap();
    assert_eq!(host.replies.len(), 1);
    assert_eq!(host.replies[0].id, 71);
    let applied = host.replies[0].result.as_ref().unwrap();
    assert_eq!(applied.generation, 2);
    assert_eq!(applied.applied_target, Timestamp::from_nanos(10_000_001));
    assert_eq!(applied.requested_target, applied.applied_target);
    assert_eq!(
        solo.runtime.transport_mut().anchor().song_time,
        applied.applied_target
    );
    assert_eq!(
        recording.paths,
        [
            std::path::PathBuf::from("practice.bkr"),
            std::path::PathBuf::from("practice.retry1.bkr")
        ]
    );
}
#[test]
fn retained_solo_disable_loop_preserves_attempt_and_recording_identity() {
    let (mut solo, mut playback, mut host) = practice_fixture_end(true, 100_000_000);
    host.requests.push_back(PracticeRequest {
        id: 81,
        generation: 1,
        action: crate::practice_control::PracticeAction::DisableLoop,
    });
    let mut recording = Recordings::default();
    assert!(
        run_practice(&mut solo, &mut playback, &mut host, &mut recording)
            .unwrap()
            .is_some()
    );
    assert!(host.commits.is_empty());
    assert_eq!(host.replies.len(), 1);
    assert_eq!(host.replies[0].id, 81);
    assert_eq!(host.replies[0].result.as_ref().unwrap().generation, 1);
    assert_eq!(recording.paths, [std::path::PathBuf::from("practice.bkr")]);
}
#[test]
fn retained_solo_delayed_acquired_cut_keeps_original_input_in_retired_capture() {
    let (mut solo, mut playback, mut host) = practice_fixture(true);
    solo.device.collector_cuts = Some(
        (1..=16)
            .map(|step| (step >= 7).then(|| super::host(step * 5_000_000)))
            .collect(),
    );
    let mut recording = Recordings::default();
    run_practice(&mut solo, &mut playback, &mut host, &mut recording).unwrap();
    let old = &recording.captures[0];
    let original: Vec<_> = old
        .records()
        .iter()
        .filter_map(|record| {
            if let ReplayOperation::Input(event) = &record.operation {
                Some(event.physical.meta().sequence)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(original, [0, 1]);
    assert!(recording.captures.iter().skip(1).all(|capture| {
        !capture
            .records()
            .iter()
            .any(|record| matches!(record.operation, ReplayOperation::Input(_)))
    }));
    assert!(host.commits.len() >= 3);
}

/// Retains the same real Device/Mixer and shifts only the scripted original Up
/// acquisition to the exact native HOST association of source frame 30.
struct ExclusiveEdgeDevice<'a>(
    &'a mut Device,
    bool,
    Option<PhysicalInputEvent>,
    Option<Rc<Cell<u64>>>,
);
impl GameplayDevice for ExclusiveEdgeDevice<'_> {
    type Presentation = PresentationEstimator;
    fn observe(&mut self, p: &mut Self::Presentation) -> NativeGameplayResult<()> {
        self.0.observe(p)
    }
    fn observe_audio(&mut self, p: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        self.0.observe_audio(p)?;
        if let Some(count) = &self.3 {
            count.set(self.0.mixer.counters().unknown_stops);
        }
        Ok(())
    }
    fn audio_pause_observation(
        &mut self,
        p: &NativeAudioPresentation,
        now: ClockPoint,
    ) -> NativeGameplayResult<LivePauseObservation> {
        self.0.audio_pause_observation(p, now)
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        self.0.render_report()
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        self.0.host_now()
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if self.3.is_some() {
            return self.0.acquire(events);
        }
        let old_len = events.len();
        let batch = self.0.acquire(events)?;
        if self.0.step == 3 {
            assert_eq!(self.0.report.unwrap().playback_start_frame, 20);
            assert_eq!(self.0.report.unwrap().playback_frames, 10);
            if self.1 {
                // The original collector owns a later Up; it cannot publish
                // that event or an acquisition cut covering it before step 4.
                assert_eq!(events.len(), old_len + 1);
                let mut event = events.pop_back().expect("original native Up");
                assert_eq!(event.meta().sequence, 1);
                event.meta_mut().timestamp = host(15_250_000).timestamp;
                self.2 = Some(event);
            } else {
                for event in events.iter_mut().skip(old_len) {
                    assert_eq!(event.meta().sequence, 1);
                    event.meta_mut().timestamp = host(15_000_000).timestamp;
                }
            }
        }
        if self.1 && self.0.step == 4 {
            events.push_back(self.2.take().expect("original pre-cut Up"));
            events.push_back(input(
                15_500_000,
                DeviceId(u64::MAX),
                7,
                2,
                ButtonState::Down,
            ));
        }
        Ok(batch)
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        p: &Self::Presentation,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        self.0.observe_end(end, p, report)
    }
    fn seed_resume(
        &mut self,
        p: &mut Self::Presentation,
        reference: ClockPair,
    ) -> NativeGameplayResult<()> {
        self.0.seed_resume(p, reference)
    }
    fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint> {
        self.0.fallback_schedule(rate)
    }
}
#[test]
fn retained_solo_exclusive_block_endpoint_preserves_equal_input_for_new_attempt() {
    let (mut solo, mut playback, mut host_port) = practice_fixture(true);
    let end = Timestamp::from_nanos(30_000_000);
    assert_eq!(solo.runtime.song_end(), Some(end));
    solo.capture = Some(
        LiveReplayCapture::new_with_policy(
            solo.runtime.judge(),
            ClockDomainId(3),
            limits(),
            Timestamp::ZERO,
            0,
            Some(end),
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            &solo.selected,
        )
        .unwrap(),
    );
    let mut config = audio_config(false, false);
    config.gameplay.end_song = Some(end);
    config.gameplay.advance_lag = Duration::ZERO;
    let mut recordings = Recordings::default();
    let mut competition = Some(NoopSoloCompetition);
    let mut device = ExclusiveEdgeDevice(&mut solo.device, false, None, None);
    super::super::run_gameplay_audio_with_practice_and_ports(
        &mut device,
        AudioGameplaySession {
            session: GameplaySession {
                runtime: &mut solo.runtime,
                gauge: &mut solo.gauge,
                bgm: &mut solo.bgm,
                discipline: &mut solo.presentation,
                pause: &mut solo.pause,
                end: &mut solo.end,
                completion: &mut solo.completion,
                capture: &mut solo.capture,
                competition: &mut competition,
                delivery: &mut solo.delivery,
                pre_origin_inputs: &mut solo.pre,
            },
            merger: &mut solo.merger,
        },
        config,
        &mut Control::default(),
        &mut host_port,
        &mut playback,
        &mut recordings,
    )
    .unwrap();
    let sequences = |capture: &LiveReplayCapture| -> Vec<_> {
        capture
            .records()
            .iter()
            .filter_map(|record| match &record.operation {
                ReplayOperation::Input(input) => {
                    Some((input.physical.meta().sequence, record.song_time))
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        sequences(&recordings.captures[0]),
        [(0, Timestamp::from_nanos(20_000_000))]
    );
    assert_eq!(sequences(&recordings.captures[1]), [(1, Timestamp::ZERO)]);
    assert!(recordings
        .captures
        .iter()
        .skip(2)
        .all(|capture| sequences(capture).is_empty()));
    assert_eq!(solo.device.creation_basis.origin(), raw(0));
    assert_eq!(solo.presentation.authority().epoch().id, 0);
}

#[test]
fn retained_solo_subframe_end_keeps_literal_cut_and_original_input_provenance() {
    let literal_end = Timestamp::from_nanos(30_000_001);
    let (mut solo, mut playback, mut host_port) =
        practice_fixture_end(true, literal_end.as_nanos());
    let mut config = audio_config(false, true);
    config.gameplay.end_song = Some(literal_end);
    config.gameplay.advance_lag = Duration::ZERO;
    let mut recordings = Recordings::default();
    let mut competition = Some(NoopSoloCompetition);
    let mut device = ExclusiveEdgeDevice(&mut solo.device, true, None, None);
    super::super::run_gameplay_audio_with_practice_and_ports(
        &mut device,
        AudioGameplaySession {
            session: GameplaySession {
                runtime: &mut solo.runtime,
                gauge: &mut solo.gauge,
                bgm: &mut solo.bgm,
                discipline: &mut solo.presentation,
                pause: &mut solo.pause,
                end: &mut solo.end,
                completion: &mut solo.completion,
                capture: &mut solo.capture,
                competition: &mut competition,
                delivery: &mut solo.delivery,
                pre_origin_inputs: &mut solo.pre,
            },
            merger: &mut solo.merger,
        },
        config,
        &mut Control::default(),
        &mut host_port,
        &mut playback,
        &mut recordings,
    )
    .unwrap();
    let old = &recordings.captures[0];
    let old_inputs: Vec<_> = old
        .records()
        .iter()
        .filter_map(|record| match &record.operation {
            ReplayOperation::Input(input) => Some(input.physical.meta().sequence),
            _ => None,
        })
        .collect();
    assert_eq!(old_inputs, [0]);
    assert!(old.records().iter().any(|record| {
        record.song_time == literal_end && record.operation == ReplayOperation::Advance
    }));
    assert!(old
        .records()
        .iter()
        .all(|record| record.song_time <= literal_end));
    let new = &recordings.captures[1];
    let original_down: Vec<_> = new
        .records()
        .iter()
        .filter_map(|record| match &record.operation {
            ReplayOperation::Input(input) => Some((record.song_time, input)),
            _ => None,
        })
        .collect();
    assert_eq!(original_down.len(), 1);
    let (at, input) = original_down[0];
    assert_eq!(at, Timestamp::ZERO);
    assert_eq!(input.physical.meta().sequence, 2);
    assert_eq!(input.physical.meta().source, DeviceId(u64::MAX));
    assert_eq!(
        input.physical.meta().original_clock_point,
        Some(host(15_500_000))
    );
    assert_eq!(
        input.physical.meta().timestamp,
        logical(31_000_000).timestamp
    );
    assert_eq!(input.physical.meta().clock_domain, ClockDomainId(3));
    assert!(matches!(&input.physical, PhysicalInputEvent::Button(button)
        if button.state == ButtonState::Down && button.control == PhysicalControlId::keyboard(7)));
    assert_eq!(solo.device.pcm[31], 0.25);
    assert_eq!(solo.presentation.authority().epoch().id, 0);
    assert_eq!(solo.device.creation_basis.origin(), raw(0));
}
#[test]
fn retained_solo_mismatched_literal_end_refuses_before_native_pump_or_advertisement() {
    let (mut solo, mut playback, mut host_port) = practice_fixture(true);
    let mut config = audio_config(false, false);
    config.gameplay.end_song = Some(Timestamp::from_nanos(30_000_001));
    let mut competition = Some(NoopSoloCompetition);
    let mut recordings = Recordings::default();
    let result = super::super::run_gameplay_audio_with_practice_and_ports(
        &mut solo.device,
        AudioGameplaySession {
            session: GameplaySession {
                runtime: &mut solo.runtime,
                gauge: &mut solo.gauge,
                bgm: &mut solo.bgm,
                discipline: &mut solo.presentation,
                pause: &mut solo.pause,
                end: &mut solo.end,
                completion: &mut solo.completion,
                capture: &mut solo.capture,
                competition: &mut competition,
                delivery: &mut solo.delivery,
                pre_origin_inputs: &mut solo.pre,
            },
            merger: &mut solo.merger,
        },
        config,
        &mut Control::default(),
        &mut host_port,
        &mut playback,
        &mut recordings,
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("section ends differ"));
    assert_eq!(solo.device.step, 0);
    assert!(recordings.paths.is_empty());
    assert!(solo.capture.is_some());
    assert!(host_port.generations.is_empty());
    assert!(host_port.commits.is_empty());
    assert_eq!(playback.generation(), 1);
}

#[test]
fn retained_solo_visual_failure_preserves_applied_attempt_recording_and_retry_identity() {
    let (mut solo, mut playback, mut host) = practice_fixture(true);
    host.refuse_visual = true;
    let mut recording = Recordings::default();
    let error = run_practice(&mut solo, &mut playback, &mut host, &mut recording).unwrap_err();
    assert_eq!(
        error.to_string(),
        "literal visual refusal after application"
    );
    assert_eq!(
        recording.paths,
        [
            std::path::PathBuf::from("practice.bkr"),
            std::path::PathBuf::from("practice.retry1.bkr")
        ]
    );
    assert_eq!(recording.attempted_paths, recording.paths);
    assert!(recording.captures[0].records().iter().any(|record|
        matches!(&record.operation, ReplayOperation::Input(input)
            if input.physical.meta().sequence == 0 && input.physical.meta().source == DeviceId(u64::MAX))));
    assert!(recording.captures[1].records().is_empty());
    assert_eq!(
        recording.captures[1].header().normalized_clock,
        ClockDomainId(3)
    );
    assert_eq!(solo.runtime.audio_scope(), CommandScope(2));
    assert_eq!(playback.generation(), 2);
    assert_eq!(playback.members()[0].launch.attempt(), 1);
    assert_eq!(host.identities, [2]);
    assert_eq!(host.generations, [1]);
    assert!(host.commits.is_empty());
    assert_eq!(host.applied_launch.as_ref().unwrap().attempt(), 1);
    let next = host.applied_launch.as_ref().unwrap().retry().unwrap();
    assert_eq!(next.attempt(), 2);
    assert!(next
        .args()
        .windows(2)
        .any(|pair| pair == ["--record-replay", "practice.retry2.bkr"]));
    assert_eq!(solo.presentation.authority().epoch().id, 0);
    assert_eq!(solo.device.creation_basis.origin(), raw(0));
    assert!(solo.capture.is_none());
}
#[test]
fn retained_solo_visual_failure_stays_primary_when_fresh_final_archive_also_refuses() {
    let (mut solo, mut playback, mut host) = practice_fixture(true);
    host.refuse_visual = true;
    let mut recording = Recordings {
        fail_at: Some(2),
        ..Default::default()
    };
    let error = run_practice(&mut solo, &mut playback, &mut host, &mut recording).unwrap_err();
    assert_eq!(
        error.to_string(),
        "literal visual refusal after application"
    );
    assert_eq!(recording.paths, [std::path::PathBuf::from("practice.bkr")]);
    assert_eq!(
        recording.attempted_paths,
        [
            std::path::PathBuf::from("practice.bkr"),
            std::path::PathBuf::from("practice.retry1.bkr")
        ]
    );
    assert_eq!(playback.generation(), 2);
    assert_eq!(solo.runtime.audio_scope(), CommandScope(2));
    assert_eq!(host.applied_launch.as_ref().unwrap().attempt(), 1);
    assert_eq!(solo.presentation.authority().epoch().id, 0);
}

#[test]
fn retained_solo_requested_visual_failure_does_not_publish_a_successful_ack() {
    let (mut solo, mut playback, mut host) = practice_fixture_end(false, 100_000_000);
    host.requests.push_back(PracticeRequest {
        id: 91,
        generation: 1,
        action: crate::practice_control::PracticeAction::Scrub {
            target: Timestamp::from_nanos(10_000_001),
        },
    });
    host.refuse_visual = true;
    let mut recording = Recordings::default();
    let error = run_practice(&mut solo, &mut playback, &mut host, &mut recording).unwrap_err();
    assert_eq!(
        error.to_string(),
        "literal visual refusal after application"
    );
    assert_eq!(playback.generation(), 2);
    assert_eq!(solo.runtime.audio_scope(), CommandScope(2));
    assert_eq!(
        solo.runtime.transport_mut().anchor().song_time,
        Timestamp::from_nanos(10_000_001)
    );
    assert_eq!(
        recording.paths,
        [
            std::path::PathBuf::from("practice.bkr"),
            std::path::PathBuf::from("practice.retry1.bkr")
        ]
    );
    assert!(host.replies.is_empty());
    assert_eq!(host.generations, [1]);
    assert_eq!(host.applied_launch.as_ref().unwrap().attempt(), 1);
}
