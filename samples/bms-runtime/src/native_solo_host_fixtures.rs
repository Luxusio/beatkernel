// Deferred explicit-host tests: portable Mixer and virtual waits, no competition.
use super::*;
use crate::native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost};
use crate::native_pump_control::NativePumpControl;
use beatkernel::runtime::RuntimeProcessingClock;
use std::{cell::Cell, rc::Rc, time::Duration as WaitDuration};

#[derive(Debug)]
struct PublicationFault(u32);
impl std::fmt::Display for PublicationFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "explicit solo publication fault {}", self.0)
    }
}
impl std::error::Error for PublicationFault {}

#[derive(Default)]
struct Control {
    reads: usize,
    waits: Vec<WaitDuration>,
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        self.reads += 1;
        Err("unlimited explicit host must not read the control clock".into())
    }
    fn checked_add(moment: u64, duration: WaitDuration) -> Option<u64> {
        moment.checked_add(u64::try_from(duration.as_nanos()).ok()?)
    }
    fn wait(&mut self, duration: WaitDuration) -> NativeGameplayResult<()> {
        self.waits.push(duration);
        Ok(())
    }
}

#[derive(Default)]
struct Host {
    cancelled: bool,
    reject: bool,
    pause_tick: Option<Rc<Cell<u64>>>,
    retries: usize,
    phases: Vec<PauseState>,
    ends: Vec<Timestamp>,
    reports: Vec<RuntimeReport>,
    diagnostic_reports: Vec<RuntimeReport>,
    pause_boundaries: Vec<(bool, LivePauseBoundary)>,
    progress: Vec<Timestamp>,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        self.cancelled
    }
    fn pause_requested(&self) -> bool {
        self.pause_tick
            .as_ref()
            .is_some_and(|tick| (2..=3).contains(&tick.get()))
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
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        self.reports.push(report.clone());
        if self.reject {
            Err(Box::new(PublicationFault(73)))
        } else {
            Ok(())
        }
    }
    fn publish_local_reports(
        &mut self,
        _: &[crate::local_runtime::PlayerReport],
    ) -> NativeGameplayResult<()> {
        panic!("solo cannot publish a local batch")
    }
    fn diagnostic(&mut self, diagnostic: NativeGameplayDiagnostic<'_>) {
        match diagnostic {
            NativeGameplayDiagnostic::SoloReport(report) => {
                self.diagnostic_reports.push(report.clone())
            }
            NativeGameplayDiagnostic::Pause { local, boundary } => {
                self.pause_boundaries.push((local, boundary))
            }
            NativeGameplayDiagnostic::SongProgress(song) => self.progress.push(song),
            NativeGameplayDiagnostic::Discipline { .. } => {}
            NativeGameplayDiagnostic::LocalReport { .. } => panic!("solo diagnostic provenance"),
        }
    }
}

// The parent device still renders the real Mixer. Only genuine acquisition and
// host pause intent are scripted; no player viewer is needed for pause/resume.
struct ScriptDevice<'a> {
    inner: &'a mut Device,
    tick: Rc<Cell<u64>>,
    pause_script: bool,
}
impl NativeGameplayDevice for ScriptDevice<'_> {
    fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
        self.inner.observe(discipline)?;
        self.tick.set(self.inner.step);
        Ok(())
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        self.inner.render_report()
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        self.inner.host_now()
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if !self.pause_script {
            return self.inner.acquire(events);
        }
        match self.inner.step {
            1 => retain_input(events, input(10_000_000, 1, ButtonState::Down))?,
            2 => retain_input(events, input(20_000_000, 2, ButtonState::Up))?,
            5 => retain_input(events, input(40_000_000, 3, ButtonState::Down))?,
            6 => retain_input(events, input(45_000_000, 4, ButtonState::Up))?,
            _ => {}
        }
        Ok(InputBatch {
            backlog: self.inner.step == 5,
            closed: self.inner.step >= 7,
        })
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &PresentationDiscipline,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        self.inner.observe_end(end, discipline, report)
    }
    fn seed_resume(
        &mut self,
        discipline: &mut PresentationDiscipline,
        reference: ClockPair,
    ) -> NativeGameplayResult<()> {
        self.inner.seed_resume(discipline, reference)
    }
    fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint> {
        self.inner.fallback_schedule(rate)
    }
}

fn run(
    f: &mut Fixture,
    host: &mut Host,
    control: &mut Control,
    pause: bool,
) -> NativeGameplayResult<()> {
    f.runtime
        .set_processing_clock(RuntimeProcessingClock::Disabled);
    let tick = Rc::new(Cell::new(0));
    if pause {
        host.pause_tick = Some(tick.clone());
    }
    run_gameplay_with_ports(
        &mut ScriptDevice {
            inner: &mut f.device,
            tick,
            pause_script: pause,
        },
        NativeGameplaySession {
            runtime: &mut f.runtime,
            gauge: &mut f.gauge,
            bgm: &mut f.bgm,
            discipline: &mut f.discipline,
            pause: &mut f.pause,
            end: &mut f.end,
            completion: &mut f.completion,
            capture: &mut f.capture,
            competition: &mut f.competition,
            delivery: &mut f.delivery,
            pre_origin_inputs: &mut f.pre,
        },
        NativeGameplayConfig {
            origin: point(1, 0),
            stream_origin: point(2, 0),
            playback_origin: point(2, 0),
            song_origin: Timestamp::ZERO,
            sample_rate: 1000,
            end_song: None,
            advance_lag: Duration::from_nanos(10_000_000),
            seconds: None,
            pause_supported: pause,
            logical_schedule: true,
        },
        control,
        host,
    )
}

fn publish_actual(
    f: &mut Fixture,
    report: RuntimeReport,
    evidence: &mut OwnedStopEvidence,
    host: &mut Host,
) -> NativeGameplayResult<()> {
    publish_with_host(
        &mut NativeGameplaySession {
            runtime: &mut f.runtime,
            gauge: &mut f.gauge,
            bgm: &mut f.bgm,
            discipline: &mut f.discipline,
            pause: &mut f.pause,
            end: &mut f.end,
            completion: &mut f.completion,
            capture: &mut f.capture,
            competition: &mut f.competition,
            delivery: &mut f.delivery,
            pre_origin_inputs: &mut f.pre,
        },
        report,
        evidence,
        host,
    )
}

#[test]
fn explicit_solo_host_ignores_ambient_commands_and_preserves_repeated_pcm_capture() {
    let mut previous = None;
    for ambient in [false, true] {
        let mut f = Fixture::new(false, false);
        let mut host = Host::default();
        let mut control = Control::default();
        if ambient {
            let (publisher, viewer) = player::channel();
            viewer.request_pause(true);
            viewer.cancel();
            player::with_publisher(publisher, || {
                run(&mut f, &mut host, &mut control, false).map_err(|e| e.to_string())
            })
            .unwrap();
            let untouched = viewer.take_latest().unwrap();
            assert_eq!(untouched.score.hits, 0);
            assert!(untouched.completed_end.is_none());
        } else {
            run(&mut f, &mut host, &mut control, false).unwrap();
        }
        assert_eq!((f.device.step, control.reads, host.retries), (4, 0, 4));
        assert_eq!(control.waits, [WaitDuration::from_millis(1); 3]);
        let mut expected = vec![0.0; 40];
        expected[20] = 0.25;
        expected[21] = 0.5;
        assert_eq!(f.device.pcm, expected);
        assert_eq!(f.device.mixer.counters().commands_applied, 1);
        assert_eq!(
            host.reports
                .iter()
                .map(|r| r.song_time.as_nanos())
                .collect::<Vec<_>>(),
            [0, 20_000_000, 20_000_000]
        );
        let acquired = host.reports[1].input.as_ref().unwrap();
        assert_eq!(acquired.meta().clock_domain, ClockDomainId(1));
        assert_eq!(acquired.meta().timestamp, Timestamp::from_nanos(20_000_000));
        assert_eq!(host.reports[1].judge_events.len(), 1);
        assert!(host.ends.is_empty() && f.completion.is_none());
        let hash = f.runtime.judge().stable_hash().unwrap();
        let bytes = f.capture.take().unwrap().into_bytes().unwrap();
        if let Some((prior_hash, prior_bytes)) = &previous {
            assert_eq!(*prior_hash, hash);
            assert_eq!(prior_bytes, &bytes);
        }
        previous = Some((hash, bytes));
    }
    let mut f = Fixture::new(false, false);
    let before = f.runtime.judge().stable_hash().unwrap();
    let mut host = Host {
        cancelled: true,
        ..Default::default()
    };
    let mut control = Control::default();
    run(&mut f, &mut host, &mut control, false).unwrap();
    assert_eq!(
        (
            f.device.step,
            control.reads,
            control.waits.len(),
            host.retries
        ),
        (0, 0, 0, 0)
    );
    assert!(host.reports.is_empty() && host.ends.is_empty());
    assert!(f.capture.as_ref().unwrap().records().is_empty());
    assert_eq!(f.runtime.judge().stable_hash().unwrap(), before);
}

#[test]
fn solo_publication_refusal_retains_fatal_capture_and_actual_stop_admission_prefix() {
    for capacity in [8, 1] {
        let mut f = gauge_fence::fatal_fixture(capacity, 128);
        f.runtime
            .set_processing_clock(RuntimeProcessingClock::Disabled);
        let actual = f
            .runtime
            .process_input(
                input(20_000_000, 1, ButtonState::Down),
                &ExplicitDomains,
                point(2, 40_000_000),
            )
            .unwrap();
        assert_eq!(
            (actual.judge_events.len(), actual.hazard_events.len()),
            (2, 1)
        );
        let mut host = Host {
            reject: true,
            ..Default::default()
        };
        let mut evidence = OwnedStopEvidence::default();
        let error = publish_actual(&mut f, actual, &mut evidence, &mut host).unwrap_err();
        let error = error
            .downcast_ref::<NativeReportObservationError>()
            .unwrap();
        assert_eq!(
            error
                .presentation_error
                .as_ref()
                .unwrap()
                .downcast_ref::<PublicationFault>()
                .unwrap()
                .0,
            73
        );
        assert!(
            error.gauge_error.is_none()
                && error.capture_error.is_none()
                && error.competition_error.is_none()
        );
        assert_eq!(
            (
                error.report.judge_events.len(),
                error.report.hazard_events.len()
            ),
            (2, 1)
        );
        assert_eq!(host.reports.len(), 1);
        assert_eq!(host.diagnostic_reports.len(), 1);
        assert_eq!(
            host.diagnostic_reports[0].audio_commands,
            error.report.audio_commands
        );
        assert_eq!(
            host.diagnostic_reports[0].audio_failures.len(),
            error.report.audio_failures.len()
        );
        assert_eq!(
            f.gauge.snapshot().failure,
            Some(crate::gauge::GaugeFailure::InstantDeath)
        );
        assert_eq!(
            f.runtime.gameplay_fence(),
            Some(Timestamp::from_nanos(20_000_000))
        );
        assert_eq!(f.runtime.judge().remaining_hazards(), 1);
        assert_eq!(f.capture.as_ref().unwrap().records().len(), 2);
        if capacity == 8 {
            assert_eq!(
                &error.report.audio_commands[2..],
                &[
                    AudioCommand::Stop {
                        voice: VoiceId(17),
                        at: Timestamp::from_nanos(40_000_000)
                    },
                    AudioCommand::Stop {
                        voice: VoiceId(18),
                        at: Timestamp::from_nanos(40_000_000)
                    },
                ]
            );
            assert!(error.report.audio_failures.is_empty());
            assert_eq!(evidence.admitted_stops(), 2);
        } else {
            assert_eq!(error.report.audio_commands.len(), 1);
            assert_eq!(error.report.audio_failures.len(), 3); // Play18, Stop17, Stop18.
            assert_eq!(
                error.report.audio_failures[1].command,
                AudioCommand::Stop {
                    voice: VoiceId(17),
                    at: Timestamp::from_nanos(40_000_000)
                }
            );
            assert_eq!(
                error.report.audio_failures[2].command,
                AudioCommand::Stop {
                    voice: VoiceId(18),
                    at: Timestamp::from_nanos(40_000_000)
                }
            );
            assert_eq!(evidence.admitted_stops(), 0);
        }
        let hash = f.runtime.judge().stable_hash().unwrap();
        let captured = f.capture.as_ref().unwrap().records().to_vec();
        host.reject = false;
        let later = f
            .runtime
            .process_input(
                input(60_000_000, 2, ButtonState::Up),
                &ExplicitDomains,
                point(2, 60_000_000),
            )
            .unwrap();
        publish_actual(&mut f, later, &mut evidence, &mut host).unwrap();
        assert!(host.reports.last().unwrap().judge_events.is_empty());
        assert_eq!(f.capture.as_ref().unwrap().records(), captured);
        assert_eq!(f.runtime.judge().stable_hash().unwrap(), hash);
        let mut pcm = [0.0; 42];
        let rendered = f.device.mixer.render(&mut pcm).unwrap();
        assert_eq!(&pcm[..40], &[0.0; 40]);
        assert_eq!(
            &pcm[40..],
            if capacity == 8 {
                &[0.0, 0.0]
            } else {
                &[0.25, 0.5]
            }
        );
        assert_eq!(
            rendered.counters.commands_applied,
            if capacity == 8 { 4 } else { 1 }
        );
        let file = f.capture.take().unwrap().into_file();
        let mut replay = crate::replay_playback::reconstruct(&f.source, file, limits()).unwrap();
        replay.seek_cursor(captured.len()).unwrap();
        assert_eq!(replay.engine().stable_hash().unwrap(), hash);
    }
}

#[test]
fn explicit_pause_host_receives_typed_boundaries_and_preserves_reconciled_input_provenance() {
    let mut f = Fixture::new(false, true);
    let mut host = Host::default();
    let mut control = Control::default();
    run(&mut f, &mut host, &mut control, true).unwrap();
    assert_eq!(
        host.phases,
        [
            PauseState::Running,
            PauseState::Pausing,
            PauseState::Paused,
            PauseState::Resuming,
            PauseState::Running
        ]
    );
    assert_eq!(
        host.pause_boundaries
            .iter()
            .map(|(local, boundary)| (*local, boundary.paused))
            .collect::<Vec<_>>(),
        [(false, true), (false, false)]
    );
    assert_eq!((control.reads, f.device.step, f.device.seeded), (0, 7, 1));
    assert_eq!(f.pause.phase(), PausePhase::Running);
    assert_eq!(f.delivery.observed_events(), 4);
    assert!(host.ends.is_empty());
    let releases = f
        .capture
        .as_ref()
        .unwrap()
        .records()
        .iter()
        .filter_map(|record| {
            let ReplayOperation::Input(input) = &record.operation else {
                return None;
            };
            let PhysicalInputEvent::Button(event) = &input.physical else {
                return None;
            };
            (event.state == ButtonState::Up).then_some((
                event.meta.timestamp.as_nanos(),
                event.meta.original_clock_point,
            ))
        })
        .collect::<Vec<_>>();
    assert_eq!(releases.len(), 2);
    assert_eq!(releases[0], (40_000_000, Some(point(1, 20_000_000))));
    assert_eq!(releases[1].0, 45_000_000);
}
