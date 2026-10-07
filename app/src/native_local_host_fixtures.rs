// Deferred explicit cohort host: real shared queue/Mixer, no network owners.
use super::*;
use crate::native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost};
use crate::native_pump_control::NativePumpControl;
use beatkernel::runtime::{RuntimeProcessingClock, RuntimeReport};
use std::time::Duration as WaitDuration;

#[derive(Default)]
struct Control {
    reads: usize,
    waits: Vec<WaitDuration>,
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        self.reads += 1;
        Err("unlimited cohort must not read control time".into())
    }
    fn checked_add(moment: u64, duration: WaitDuration) -> Option<u64> {
        moment.checked_add(u64::try_from(duration.as_nanos()).ok()?)
    }
    fn wait(&mut self, duration: WaitDuration) -> NativeGameplayResult<()> {
        self.waits.push(duration);
        Ok(())
    }
}

#[derive(Debug)]
struct PublicationFault;
impl std::fmt::Display for PublicationFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("explicit cohort publication refused")
    }
}
impl std::error::Error for PublicationFault {}

#[derive(Default)]
struct Host {
    cancelled: bool,
    reject: bool,
    retries: usize,
    phases: Vec<PauseState>,
    ends: Vec<Timestamp>,
    batches: Vec<Vec<PlayerReport>>,
    diagnostics: Vec<(PlayerId, RuntimeReport)>,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        self.cancelled
    }
    fn pause_requested(&self) -> bool {
        false
    }
    fn retry_pause_publication(&mut self) {
        self.retries += 1;
    }
    fn publish_pause(&mut self, state: PauseState) {
        self.phases.push(state);
    }
    fn publish_section_end(&mut self, end: Timestamp) {
        self.ends.push(end);
    }
    fn publish_report(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        panic!("cohort publication must retain whole batches")
    }
    fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()> {
        self.batches.push(reports.to_vec());
        if self.reject {
            Err(Box::new(PublicationFault))
        } else {
            Ok(())
        }
    }
    fn diagnostic(&mut self, diagnostic: NativeGameplayDiagnostic<'_>) {
        match diagnostic {
            NativeGameplayDiagnostic::LocalReport { player, report } => {
                self.diagnostics.push((player, report.clone()))
            }
            NativeGameplayDiagnostic::SoloReport(_) => {
                panic!("local diagnostic must retain player identity")
            }
            NativeGameplayDiagnostic::Pause { local, .. } => assert!(local),
            NativeGameplayDiagnostic::Discipline { .. }
            | NativeGameplayDiagnostic::SongProgress(_) => {}
        }
    }
}

fn run(f: &mut Fixture, host_port: &mut Host, control: &mut Control) -> NativeGameplayResult<()> {
    f.group
        .set_processing_clock(RuntimeProcessingClock::Disabled);
    run_cohort_with_ports(
        &mut f.device,
        NativeCohortSession {
            group: &mut f.group,
            states: &mut f.states,
            network: None,
            merger: &mut f.merger,
            bgm: &mut f.bgm,
            discipline: &mut f.discipline,
            pause: &mut f.pause,
            end: &mut f.end,
            delivery: &mut f.delivery,
            pre_origin_inputs: &mut f.pre,
        },
        NativeGameplayConfig {
            origin: host(0),
            stream_origin: output(0),
            playback_origin: output(0),
            song_origin: Timestamp::ZERO,
            sample_rate: 1000,
            end_song: None,
            advance_lag: Duration::from_nanos(10_000_000),
            seconds: None,
            pause_supported: false,
            logical_schedule: true,
        },
        control,
        host_port,
    )
}

fn observe(
    f: &mut Fixture,
    reports: &mut [PlayerReport],
    evidence: &mut OwnedStopEvidence,
    host_port: &mut Host,
) -> NativeGameplayResult<()> {
    observe_reports_with_host(
        reports,
        &mut f.states,
        &mut f.group,
        None,
        evidence,
        host_port,
    )
}

#[test]
fn explicit_cohort_host_is_independent_of_ambient_cancel_and_repeats_actual_member_captures() {
    let mut previous = None;
    for ambient in [false, true] {
        let mut f = Fixture::new(false, 8);
        let mut host_port = Host::default();
        let mut control = Control::default();
        if ambient {
            let (publisher, viewer) = player::channel();
            viewer.request_pause(true);
            viewer.cancel();
            player::with_publisher(publisher, || {
                run(&mut f, &mut host_port, &mut control).map_err(|e| e.to_string())
            })
            .unwrap();
            let untouched = viewer.take_latest().unwrap();
            assert_eq!(untouched.score.hits, 0);
            assert!(untouched.players.is_empty() && untouched.completed_end.is_none());
        } else {
            run(&mut f, &mut host_port, &mut control).unwrap();
        }
        assert_eq!((f.device.step, control.reads, host_port.retries), (4, 0, 4));
        assert_eq!(control.waits, [WaitDuration::from_millis(1); 3]);
        let mut expected = vec![0.0; 40];
        expected[30] = 0.5;
        expected[31] = 1.0;
        assert_eq!(f.device.pcm, expected);
        assert_eq!(f.device.mixer.counters().commands_applied, 2);
        assert_eq!((f.merger.pending(), f.delivery.observed_events()), (0, 2));
        assert!(host_port.ends.is_empty());
        assert!(host_port.batches.iter().any(|batch| batch.len() == 2));
        let mut hashes = Vec::new();
        let mut captures = Vec::new();
        for state in &mut f.states {
            assert_eq!(state.score.hits, 1);
            assert!(state.completion.is_none());
            let reports = host_port
                .batches
                .iter()
                .flatten()
                .filter(|tagged| tagged.player == state.player)
                .map(|tagged| &tagged.report)
                .collect::<Vec<_>>();
            assert_eq!(
                reports
                    .iter()
                    .map(|report| report.song_time.as_nanos())
                    .collect::<Vec<_>>(),
                [0, 20_000_000, 20_000_000]
            );
            let meta = reports[1].input.as_ref().unwrap().meta();
            assert_eq!(
                meta.source,
                DeviceId(if state.player == PlayerId(7) { 1 } else { 2 })
            );
            assert_eq!(meta.clock_domain, ClockDomainId(1));
            assert_eq!(meta.timestamp, Timestamp::from_nanos(20_000_000));
            assert_eq!(reports[1].judge_events.len(), 1);
            hashes.push(
                f.group
                    .member_judge(state.player)
                    .unwrap()
                    .stable_hash()
                    .unwrap(),
            );
            captures.push(state.capture.take().unwrap().into_bytes().unwrap());
        }
        if let Some((prior_hashes, prior_captures)) = &previous {
            assert_eq!(prior_hashes, &hashes);
            assert_eq!(prior_captures, &captures);
        }
        previous = Some((hashes, captures));
    }
    let mut f = Fixture::new(false, 8);
    let before = f
        .group
        .member_judge(PlayerId(7))
        .unwrap()
        .stable_hash()
        .unwrap();
    let mut host_port = Host {
        cancelled: true,
        ..Default::default()
    };
    let mut control = Control::default();
    run(&mut f, &mut host_port, &mut control).unwrap();
    assert_eq!(
        (
            f.device.step,
            control.reads,
            control.waits.len(),
            host_port.retries
        ),
        (0, 0, 0, 0)
    );
    assert!(host_port.batches.is_empty() && host_port.ends.is_empty());
    assert!(
        f.states
            .iter()
            .all(|state| state.capture.as_ref().unwrap().records().is_empty())
    );
    assert_eq!(
        f.group
            .member_judge(PlayerId(7))
            .unwrap()
            .stable_hash()
            .unwrap(),
        before
    );
}

#[test]
fn refused_whole_cohort_publication_observes_every_member_and_preserves_stop_prefix() {
    const PLAYERS: [PlayerId; 3] = [PlayerId(7), PlayerId(101), PlayerId(u32::MAX)];
    const SOURCES: [u64; 3] = [u64::MAX - 2, u64::MAX - 1, u64::MAX];
    for capacity in [8, 4] {
        let mut f = gauge_fence::cohort_fixture(
            "#BPM 3000\n#WAV01 head.wav\n#00011:01000100\n#000D1:00ZZ0001\n",
            &[0x11],
            capacity,
            128,
        );
        f.group
            .set_processing_clock(RuntimeProcessingClock::Disabled);
        let mut host_port = Host::default();
        let mut evidence = OwnedStopEvidence::default();
        for (index, source) in SOURCES.into_iter().enumerate() {
            let InputResult::Processed(mut reports) = f
                .group
                .process_input(
                    button(source, 0, 1, ButtonState::Down),
                    &ExplicitDomains,
                    output(40_000_000),
                )
                .unwrap()
            else {
                panic!("assigned source");
            };
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].player, PLAYERS[index]);
            observe(&mut f, &mut reports, &mut evidence, &mut host_port).unwrap();
        }
        assert_eq!(evidence.admitted_stops(), 0);
        host_port.batches.clear();
        host_port.diagnostics.clear();
        host_port.reject = true;
        let mut actual = f
            .group
            .advance_to(host(20_000_000), &ExplicitDomains, output(20_000_000))
            .unwrap();
        let error = observe(&mut f, &mut actual, &mut evidence, &mut host_port).unwrap_err();
        let error = error
            .downcast_ref::<NativeCohortObservationError>()
            .unwrap();
        assert!(
            error
                .failures
                .iter()
                .any(|message| message.contains("explicit cohort publication refused"))
        );
        assert_eq!(host_port.batches.len(), 1);
        assert_eq!(
            host_port.batches[0]
                .iter()
                .map(|tagged| tagged.player)
                .collect::<Vec<_>>(),
            PLAYERS
        );
        assert_eq!(error.reports.len(), 3);
        assert_eq!(
            host_port
                .diagnostics
                .iter()
                .map(|(player, _)| *player)
                .collect::<Vec<_>>(),
            PLAYERS
        );
        for (index, state) in f.states.iter().enumerate() {
            assert_eq!(state.score.hits, 1);
            assert_eq!(
                state.gauge.snapshot().failure,
                Some(crate::gauge::GaugeFailure::InstantDeath)
            );
            assert_eq!(
                f.group.player_gameplay_fence(state.player),
                Some(Timestamp::from_nanos(20_000_000))
            );
            assert_eq!(
                f.group
                    .member_judge(state.player)
                    .unwrap()
                    .remaining_hazards(),
                1
            );
            assert_eq!(state.capture.as_ref().unwrap().records().len(), 2);
            assert_eq!(error.reports[index].report.hazard_events.len(), 1);
            assert_eq!(
                error.reports[index].report.hazard_events[0].outcome,
                beatkernel::judge::HazardOutcome::Triggered
            );
            if capacity == 8 || index == 0 {
                assert_eq!(
                    actual[index].report.audio_commands,
                    [AudioCommand::Stop {
                        voice: VoiceId(1 + 2 * index as u64),
                        at: Timestamp::from_nanos(40_000_000),
                    }]
                );
                assert!(actual[index].report.audio_failures.is_empty());
            } else {
                assert!(actual[index].report.audio_commands.is_empty());
                assert_eq!(actual[index].report.audio_failures.len(), 1);
                assert_eq!(
                    actual[index].report.audio_failures[0].command,
                    AudioCommand::Stop {
                        voice: VoiceId(1 + 2 * index as u64),
                        at: Timestamp::from_nanos(40_000_000),
                    }
                );
            }
            assert_eq!(
                error.reports[index].report.audio_commands,
                actual[index].report.audio_commands
            );
            assert_eq!(
                host_port.diagnostics[index].1.audio_commands,
                actual[index].report.audio_commands
            );
            let capture = state.capture.as_ref().unwrap();
            let file = beatkernel::replay::codec::ReplayFile::new(
                capture.header().clone(),
                capture.records().to_vec(),
            );
            let mut replay =
                crate::replay_playback::reconstruct(&f.source, file, limits()).unwrap();
            replay.seek_cursor(2).unwrap();
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                f.group
                    .member_judge(state.player)
                    .unwrap()
                    .stable_hash()
                    .unwrap()
            );
        }
        assert_eq!(evidence.admitted_stops(), if capacity == 8 { 3 } else { 1 });
        assert!(
            !f.group.poisoned(),
            "publication/Stop refusal must not rewrite successful judge ownership"
        );
        host_port.reject = false;
        let mut later = f
            .group
            .advance_to(host(60_000_000), &ExplicitDomains, output(60_000_000))
            .unwrap();
        observe(&mut f, &mut later, &mut evidence, &mut host_port).unwrap();
        assert!(
            later
                .iter()
                .all(|tagged| tagged.report.judge_events.is_empty()
                    && tagged.report.hazard_events.is_empty()
                    && tagged.report.audio_commands.is_empty())
        );
        assert!(
            f.states
                .iter()
                .all(|state| state.capture.as_ref().unwrap().records().len() == 2)
        );
        assert_eq!(evidence.admitted_stops(), if capacity == 8 { 3 } else { 1 });
        let mut pcm = [0.0; 42];
        let rendered = f.device.mixer.render(&mut pcm).unwrap();
        assert_eq!(&pcm[..40], &[0.0; 40]);
        assert_eq!(
            &pcm[40..],
            if capacity == 8 {
                &[0.0, 0.0]
            } else {
                &[0.5, 1.0]
            }
        );
        assert_eq!(
            rendered.counters.commands_applied,
            if capacity == 8 { 6 } else { 4 }
        );
        assert_eq!(rendered.counters.unknown_stops, 0);
        assert!(host_port.ends.is_empty());
    }
}
