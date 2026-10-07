// Deferred actual portable pump with populated competition, host and time ports.
use super::*;
use crate::{
    gameplay_competition::SoloCompetitionPort,
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost},
    native_pump_control::NativePumpControl,
};
use beatkernel::runtime::RuntimeProcessingClock;
use std::{cell::RefCell, rc::Rc, time::Duration as WaitDuration};

#[derive(Debug)]
struct Fault(&'static str);
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Fault {}
type Trace = Rc<RefCell<Vec<&'static str>>>;

#[derive(Default)]
struct Competition {
    reports: Vec<RuntimeReport>,
    marks: usize,
    reject: bool,
    trace: Trace,
}
impl SoloCompetitionPort for Competition {
    fn observe(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        self.trace.borrow_mut().push("competition");
        self.reports.push(report.clone());
        if self.reject {
            Err(Box::new(Fault("solo observer refused")))
        } else {
            Ok(())
        }
    }
    fn mark_native_completed(&mut self) {
        self.trace.borrow_mut().push("completed");
        self.marks += 1;
    }
}
#[derive(Default)]
struct Host {
    cancel: bool,
    reject: bool,
    publications: usize,
    ends: Vec<Timestamp>,
    trace: Trace,
}
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        self.cancel
    }
    fn pause_requested(&self) -> bool {
        false
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, _: PauseState) {}
    fn publish_section_end(&mut self, at: Timestamp) {
        self.ends.push(at);
    }
    fn publish_report(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        self.trace.borrow_mut().push("host");
        self.publications += 1;
        if self.reject {
            Err(Box::new(Fault("host refused")))
        } else {
            Ok(())
        }
    }
    fn publish_local_reports(
        &mut self,
        _: &[crate::local_runtime::PlayerReport],
    ) -> NativeGameplayResult<()> {
        panic!("solo publication")
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
}
#[derive(Default)]
struct Control {
    readings: VecDeque<u64>,
    reads: usize,
    waits: usize,
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        self.reads += 1;
        self.readings
            .pop_front()
            .ok_or_else(|| Box::new(Fault("unexpected clock read")) as Box<dyn std::error::Error>)
    }
    fn checked_add(moment: u64, duration: WaitDuration) -> Option<u64> {
        moment.checked_add(u64::try_from(duration.as_nanos()).ok()?)
    }
    fn wait(&mut self, duration: WaitDuration) -> NativeGameplayResult<()> {
        assert_eq!(duration, WaitDuration::from_millis(1));
        self.waits += 1;
        Ok(())
    }
}
struct Pump<'a> {
    device: &'a mut Device,
    allow_close: bool,
}
impl NativeGameplayDevice for Pump<'_> {
    fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
        self.device.observe(discipline)
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        self.device.render_report()
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        self.device.host_now()
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if self.device.step > 12 {
            return Err("missing genuine completion".into());
        }
        let mut batch = self.device.acquire(events)?;
        if !self.allow_close {
            batch.closed = false;
        }
        Ok(batch)
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &PresentationDiscipline,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        self.device.observe_end(end, discipline, report)
    }
    fn seed_resume(
        &mut self,
        discipline: &mut PresentationDiscipline,
        reference: ClockPair,
    ) -> NativeGameplayResult<()> {
        self.device.seed_resume(discipline, reference)
    }
    fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint> {
        self.device.fallback_schedule(rate)
    }
}
fn run(
    f: &mut Fixture,
    competition: &mut Option<Competition>,
    host: &mut Host,
    control: &mut Control,
    finite: bool,
    allow_close: bool,
    seconds: Option<u64>,
) -> NativeGameplayResult<()> {
    f.runtime
        .set_processing_clock(RuntimeProcessingClock::Disabled);
    run_gameplay_with_ports(
        &mut Pump {
            device: &mut f.device,
            allow_close,
        },
        GameplaySession {
            runtime: &mut f.runtime,
            gauge: &mut f.gauge,
            bgm: &mut f.bgm,
            discipline: &mut f.discipline,
            pause: &mut f.pause,
            end: &mut f.end,
            completion: &mut f.completion,
            capture: &mut f.capture,
            competition,
            delivery: &mut f.delivery,
            pre_origin_inputs: &mut f.pre,
        },
        NativeGameplayConfig {
            origin: point(1, 0),
            stream_origin: point(2, 0),
            playback_origin: point(2, 0),
            song_origin: Timestamp::ZERO,
            sample_rate: 1000,
            end_song: finite.then_some(Timestamp::from_nanos(10_000_000)),
            advance_lag: Duration::from_nanos(10_000_000),
            seconds,
            pause_supported: false,
            logical_schedule: true,
        },
        control,
        host,
    )
}
fn completion(f: &mut Fixture) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm_limits = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5], pcm_limits).unwrap(),
    )
    .unwrap();
    let prepared = crate::PreparedBms {
        source: f.source.clone(),
        compiled: f.source.compile().unwrap(),
        bank,
        sounds: Vec::new(),
        bgm_commands: vec![AudioCommand::Play {
            sample: SampleId(1),
            voice: VoiceId(99),
            at: Timestamp::from_nanos(50_000_000),
            gain: 0.5,
        }],
    };
    f.completion = Some(SongCompletion::prepare(&prepared, 0, 0, 0, ClockDomainId(2)).unwrap());
    f.bgm = BgmFeeder::new(
        prepared.bgm_commands,
        crate::bgm::BgmConfig {
            output_origin: point(2, 0),
            sample_rate: 1000,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(10_000_000),
            max_pending: 4,
        },
    )
    .unwrap();
}

#[test]
fn populated_solo_port_observes_actual_order_and_repeatable_pcm_capture_without_completing_on_close()
 {
    let mut previous = None;
    for _ in 0..2 {
        let mut f = Fixture::new(false, false);
        let trace = Trace::default();
        let mut competition = Some(Competition {
            trace: trace.clone(),
            ..Default::default()
        });
        let mut host = Host {
            trace: trace.clone(),
            ..Default::default()
        };
        let mut control = Control::default();
        run(
            &mut f,
            &mut competition,
            &mut host,
            &mut control,
            false,
            true,
            None,
        )
        .unwrap();
        assert_eq!(
            &*trace.borrow(),
            &[
                "competition",
                "host",
                "competition",
                "host",
                "competition",
                "host"
            ]
        );
        let observer = competition.as_ref().unwrap();
        assert_eq!(
            observer
                .reports
                .iter()
                .map(|r| r.song_time.as_nanos())
                .collect::<Vec<_>>(),
            [0, 20_000_000, 20_000_000]
        );
        assert_eq!(
            observer.reports[1].input.as_ref().unwrap().meta().source,
            DeviceId(1)
        );
        assert_eq!(observer.reports[1].judge_events.len(), 1);
        assert_eq!(
            (
                observer.marks,
                host.publications,
                control.reads,
                control.waits
            ),
            (0, 3, 0, 3)
        );
        assert!(host.ends.is_empty());
        let mut expected = vec![0.0; 40];
        expected[20] = 0.25;
        expected[21] = 0.5;
        assert_eq!(f.device.pcm, expected);
        let hash = f.runtime.judge().stable_hash().unwrap();
        let capture = f.capture.take().unwrap().into_bytes().unwrap();
        if let Some((old_hash, old_capture)) = &previous {
            assert_eq!(*old_hash, hash);
            assert_eq!(old_capture, &capture);
        }
        previous = Some((hash, capture));
    }
}

#[test]
fn solo_competition_refusal_keeps_original_error_fatal_capture_and_full_or_partial_stops() {
    for capacity in [8, 3] {
        let mut f = gauge_fence::fatal_fixture(capacity, 128);
        f.runtime
            .set_processing_clock(RuntimeProcessingClock::Disabled);
        let report = f
            .runtime
            .process_input(
                input(20_000_000, 1, ButtonState::Down),
                &ExplicitDomains,
                point(2, 40_000_000),
            )
            .unwrap();
        let original = report.clone();
        let trace = Trace::default();
        let mut competition = Some(Competition {
            reject: true,
            trace: trace.clone(),
            ..Default::default()
        });
        let mut host = Host {
            reject: true,
            trace: trace.clone(),
            ..Default::default()
        };
        let mut evidence = OwnedStopEvidence::default();
        let error = publish_with_host(
            &mut GameplaySession {
                runtime: &mut f.runtime,
                gauge: &mut f.gauge,
                bgm: &mut f.bgm,
                discipline: &mut f.discipline,
                pause: &mut f.pause,
                end: &mut f.end,
                completion: &mut f.completion,
                capture: &mut f.capture,
                competition: &mut competition,
                delivery: &mut f.delivery,
                pre_origin_inputs: &mut f.pre,
            },
            report,
            &mut evidence,
            &mut host,
        )
        .unwrap_err();
        let error = error
            .downcast_ref::<NativeReportObservationError>()
            .unwrap();
        assert_eq!(
            error
                .competition_error
                .as_ref()
                .unwrap()
                .downcast_ref::<Fault>()
                .unwrap()
                .0,
            "solo observer refused"
        );
        assert_eq!(
            error
                .presentation_error
                .as_ref()
                .unwrap()
                .downcast_ref::<Fault>()
                .unwrap()
                .0,
            "host refused"
        );
        assert_eq!(&*trace.borrow(), &["competition", "host"]);
        assert_eq!(error.report.judge_events, original.judge_events);
        assert_eq!(error.report.hazard_events, original.hazard_events);
        assert_eq!(error.report.bound_inputs, original.bound_inputs);
        assert_eq!(&error.report.audio_commands[..2], &original.audio_commands);
        assert_eq!(
            error.report.audio_commands[2],
            AudioCommand::Stop {
                voice: VoiceId(17),
                at: Timestamp::from_nanos(40_000_000)
            }
        );
        if capacity == 8 {
            assert_eq!(
                error.report.audio_commands[3],
                AudioCommand::Stop {
                    voice: VoiceId(18),
                    at: Timestamp::from_nanos(40_000_000)
                }
            );
            assert!(error.report.audio_failures.is_empty());
        } else {
            assert_eq!(error.report.audio_failures.len(), 1);
            assert_eq!(
                error.report.audio_failures[0].command,
                AudioCommand::Stop {
                    voice: VoiceId(18),
                    at: Timestamp::from_nanos(40_000_000)
                }
            );
        }
        assert_eq!(evidence.admitted_stops(), if capacity == 8 { 2 } else { 1 });
        assert_eq!(
            f.runtime.gameplay_fence(),
            Some(Timestamp::from_nanos(20_000_000))
        );
        assert_eq!(
            f.gauge.snapshot().failure,
            Some(crate::gauge::GaugeFailure::InstantDeath)
        );
        assert_eq!(competition.as_ref().unwrap().marks, 0);
        assert_eq!(f.capture.as_ref().unwrap().records().len(), 2);
        let rebuilt = crate::replay_playback::reconstruct(
            &f.source,
            f.capture.take().unwrap().into_file(),
            limits(),
        )
        .unwrap();
        assert_eq!(
            rebuilt.engine().stable_hash().unwrap(),
            f.runtime.judge().stable_hash().unwrap()
        );
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
            if capacity == 8 { 4 } else { 3 }
        );
    }
}

#[test]
fn solo_completion_mark_requires_real_endpoint_or_bgm_idle_presentation_not_cancel_or_timeout() {
    for finite in [true, false] {
        let mut f = Fixture::new(finite, false);
        if !finite {
            completion(&mut f);
        }
        let mut competition = Some(Competition::default());
        let mut host = Host::default();
        let mut control = Control::default();
        run(
            &mut f,
            &mut competition,
            &mut host,
            &mut control,
            finite,
            false,
            None,
        )
        .unwrap();
        assert_eq!(competition.as_ref().unwrap().marks, 1);
        assert_eq!(control.reads, 0);
        if finite {
            assert_eq!(f.device.step, 3);
            assert_eq!(host.ends, [Timestamp::from_nanos(10_000_000)]);
            assert!(
                f.device
                    .report
                    .unwrap()
                    .playback_end_physical_frame
                    .is_some()
            );
            assert_eq!(
                f.runtime.judge().state(beatkernel::chart::ObjectId(1)),
                Some(beatkernel::interaction::InteractionState::Pending)
            );
            assert!(f.device.pcm.iter().all(|&v| v == 0.0));
        } else {
            assert!((7..=12).contains(&f.device.step));
            assert!(host.ends.is_empty());
            assert_eq!(&f.device.pcm[20..23], &[0.25, 0.5, 0.0]);
            assert_eq!(&f.device.pcm[50..53], &[0.125, 0.25, 0.0]);
            assert_eq!(
                (f.bgm.report().remaining, f.bgm.report().outstanding),
                (0, 0)
            );
            let rendered = f.device.report.unwrap();
            assert_eq!((rendered.active_voices, rendered.pending_commands), (0, 0));
            assert_eq!(rendered.counters.commands_applied, 2);
        }
    }
    for case in 0..3 {
        let mut f = Fixture::new(false, false);
        let mut competition = Some(Competition {
            reject: case == 2,
            ..Default::default()
        });
        let mut host = Host {
            cancel: case == 0,
            ..Default::default()
        };
        let mut control = Control::default();
        let seconds = if case == 1 {
            control.readings.extend([0, 0, 1_000_000_000]);
            Some(1)
        } else {
            None
        };
        let result = run(
            &mut f,
            &mut competition,
            &mut host,
            &mut control,
            false,
            false,
            seconds,
        );
        if case == 2 {
            let error = result.unwrap_err();
            let error = error
                .downcast_ref::<NativeReportObservationError>()
                .unwrap();
            assert_eq!(
                error
                    .competition_error
                    .as_ref()
                    .unwrap()
                    .downcast_ref::<Fault>()
                    .unwrap()
                    .0,
                "solo observer refused"
            );
            assert_eq!(f.capture.as_ref().unwrap().records().len(), 1);
            assert_eq!(host.publications, 1);
        } else {
            result.unwrap();
        }
        assert_eq!(competition.as_ref().unwrap().marks, 0);
        assert_eq!(f.device.step, if case == 0 { 0 } else { 1 });
        assert_eq!(control.reads, if case == 1 { 3 } else { 0 });
        assert!(host.ends.is_empty());
    }
}
