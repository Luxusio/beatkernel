// Deferred populated member/group ports over real shared Runtime and Mixer.
use super::*;
use crate::{
    gameplay_competition::{SoloCompetitionPort, GroupCompetitionPort},
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost},
    native_pump_control::NativePumpControl,
};
use beatkernel::runtime::{RuntimeProcessingClock, RuntimeReport};
use std::{cell::RefCell, rc::Rc, time::Duration as WaitDuration};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Seen {
    Member(PlayerId),
    Group,
    Host,
    MemberCompleted(PlayerId),
    GroupCompleted,
}
type Trace = Rc<RefCell<Vec<Seen>>>;
#[derive(Debug)]
struct Fault(&'static str);
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Fault {}
struct Member {
    player: PlayerId,
    reports: Vec<RuntimeReport>,
    marks: usize,
    reject: bool,
    trace: Trace,
}
impl SoloCompetitionPort for Member {
    fn observe(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        self.trace.borrow_mut().push(Seen::Member(self.player));
        self.reports.push(report.clone());
        if self.reject {
            Err(Box::new(Fault("member observer refused")))
        } else {
            Ok(())
        }
    }
    fn mark_native_completed(&mut self) {
        self.marks += 1;
        self.trace
            .borrow_mut()
            .push(Seen::MemberCompleted(self.player));
    }
}
#[derive(Default)]
struct Shared {
    prefixes: Vec<Vec<MemberProgress>>,
    marks: usize,
    reject: bool,
    trace: Trace,
}
impl GroupCompetitionPort for Shared {
    fn observe(&mut self, members: &[MemberProgress]) -> NativeGameplayResult<()> {
        self.trace.borrow_mut().push(Seen::Group);
        self.prefixes.push(members.to_vec());
        if self.reject {
            Err(Box::new(Fault("group observer refused")))
        } else {
            Ok(())
        }
    }
    fn mark_native_completed(&mut self) {
        self.marks += 1;
        self.trace.borrow_mut().push(Seen::GroupCompleted);
    }
}
#[derive(Default)]
struct Host {
    cancel: bool,
    reject: bool,
    ends: Vec<Timestamp>,
    batches: Vec<Vec<PlayerReport>>,
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
        panic!("whole cohort publication required")
    }
    fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()> {
        self.trace.borrow_mut().push(Seen::Host);
        self.batches.push(reports.to_vec());
        if self.reject {
            Err(Box::new(Fault("host observer refused")))
        } else {
            Ok(())
        }
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
fn states(f: &mut Fixture, trace: &Trace) -> Vec<GameplayPlayerState<Member>> {
    std::mem::take(&mut f.states)
        .into_iter()
        .map(|state| GameplayPlayerState {
            player: state.player,
            capture: state.capture,
            completion: state.completion,
            score: state.score,
            gauge: state.gauge,
            last_song: state.last_song,
            competition: Some(Member {
                player: state.player,
                reports: Vec::new(),
                marks: 0,
                reject: false,
                trace: trace.clone(),
            }),
        })
        .collect()
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
    ) -> NativeGameplayResult<crate::native_gameplay::InputBatch> {
        if self.device.step > 12 {
            return Err("missing genuine cohort completion".into());
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
    ) -> NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
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
    states: &mut [GameplayPlayerState<Member>],
    shared: &mut Shared,
    host_port: &mut Host,
    control: &mut Control,
    finite: bool,
    allow_close: bool,
    seconds: Option<u64>,
) -> NativeGameplayResult<()> {
    f.group
        .set_processing_clock(RuntimeProcessingClock::Disabled);
    run_cohort_with_ports(
        &mut Pump {
            device: &mut f.device,
            allow_close,
        },
        CohortSession {
            group: &mut f.group,
            states,
            network: Some(shared),
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
            end_song: finite.then_some(Timestamp::from_nanos(10_000_000)),
            advance_lag: Duration::from_nanos(10_000_000),
            seconds,
            pause_supported: false,
            logical_schedule: true,
        },
        control,
        host_port,
    )
}
fn completion(f: &mut Fixture, states: &mut [GameplayPlayerState<Member>]) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5], limits).unwrap(),
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
    for state in states {
        state.completion =
            Some(SongCompletion::prepare(&prepared, 10_000_000, 0, 0, ClockDomainId(2)).unwrap());
    }
    f.bgm = BgmFeeder::new(
        prepared.bgm_commands,
        crate::bgm::BgmConfig {
            output_origin: output(0),
            sample_rate: 1000,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(10_000_000),
            max_pending: 4,
        },
    )
    .unwrap();
}

#[test]
fn populated_cohort_ports_keep_actual_member_order_progress_and_repeatable_pcm_capture() {
    let mut previous = None;
    for _ in 0..2 {
        let mut f = Fixture::new(false, 8);
        let trace = Trace::default();
        let mut states = states(&mut f, &trace);
        let mut shared = Shared {
            trace: trace.clone(),
            ..Default::default()
        };
        let mut host_port = Host {
            trace: trace.clone(),
            ..Default::default()
        };
        let mut control = Control::default();
        run(
            &mut f,
            &mut states,
            &mut shared,
            &mut host_port,
            &mut control,
            false,
            true,
            None,
        )
        .unwrap();
        let a = Seen::Member(PlayerId(7));
        let b = Seen::Member(PlayerId(u32::MAX));
        assert_eq!(
            &*trace.borrow(),
            &[
                a.clone(),
                b.clone(),
                Seen::Group,
                Seen::Host,
                a.clone(),
                Seen::Group,
                Seen::Host,
                b.clone(),
                Seen::Group,
                Seen::Host,
                a,
                b,
                Seen::Group,
                Seen::Host,
            ]
        );
        assert_eq!(
            shared
                .prefixes
                .iter()
                .map(|rows| rows
                    .iter()
                    .map(|row| (row.player, row.progress.song_ns, row.progress.hits))
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            vec![
                vec![(PlayerId(7), 0, 0), (PlayerId(u32::MAX), 0, 0)],
                vec![(PlayerId(7), 20_000_000, 1), (PlayerId(u32::MAX), 0, 0)],
                vec![
                    (PlayerId(7), 20_000_000, 1),
                    (PlayerId(u32::MAX), 20_000_000, 1)
                ],
                vec![
                    (PlayerId(7), 20_000_000, 1),
                    (PlayerId(u32::MAX), 20_000_000, 1)
                ],
            ]
        );
        assert_eq!((control.reads, control.waits, shared.marks), (0, 3, 0));
        assert!(host_port.ends.is_empty());
        let mut expected = vec![0.0; 40];
        expected[30] = 0.5;
        expected[31] = 1.0;
        assert_eq!(f.device.pcm, expected);
        let mut captured = Vec::new();
        let mut hashes = Vec::new();
        for state in &mut states {
            let observer = state.competition.as_ref().unwrap();
            assert_eq!(observer.marks, 0);
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
                DeviceId(if state.player == PlayerId(7) { 1 } else { 2 })
            );
            assert_eq!(state.score.hits, 1);
            hashes.push(
                f.group
                    .member_judge(state.player)
                    .unwrap()
                    .stable_hash()
                    .unwrap(),
            );
            captured.push(state.capture.take().unwrap().into_bytes().unwrap());
        }
        if let Some((old_hashes, old_captured)) = &previous {
            assert_eq!(old_hashes, &hashes);
            assert_eq!(old_captured, &captured);
        }
        previous = Some((hashes, captured));
    }
}

#[test]
fn every_member_and_group_refusal_preserves_all_committed_fatal_reports_and_partial_stops() {
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
        let trace = Trace::default();
        let mut states = states(&mut f, &trace);
        let mut shared = Shared {
            trace: trace.clone(),
            ..Default::default()
        };
        let mut host_port = Host {
            trace: trace.clone(),
            ..Default::default()
        };
        let mut evidence = OwnedStopEvidence::default();
        for source in SOURCES {
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
            observe_reports_with_competition(
                &mut reports,
                &mut states,
                &mut f.group,
                Some(&mut shared),
                &mut evidence,
                &mut host_port,
            )
            .unwrap();
        }
        for state in &mut states {
            state.competition.as_mut().unwrap().reject = true;
        }
        shared.reject = true;
        host_port.reject = true;
        trace.borrow_mut().clear();
        let mut actual = f
            .group
            .advance_to(host(20_000_000), &ExplicitDomains, output(20_000_000))
            .unwrap();
        let error = observe_reports_with_competition(
            &mut actual,
            &mut states,
            &mut f.group,
            Some(&mut shared),
            &mut evidence,
            &mut host_port,
        )
        .unwrap_err();
        let error = error
            .downcast_ref::<NativeCohortObservationError>()
            .unwrap();
        assert_eq!(
            &*trace.borrow(),
            &[
                Seen::Member(PLAYERS[0]),
                Seen::Member(PLAYERS[1]),
                Seen::Member(PLAYERS[2]),
                Seen::Group,
                Seen::Host
            ]
        );
        assert_eq!(
            error
                .failures
                .iter()
                .filter(|message| message.contains("member observer refused"))
                .count(),
            3
        );
        assert!(
            error
                .failures
                .iter()
                .any(|message| message.contains("group observer refused"))
        );
        assert!(
            error
                .failures
                .iter()
                .any(|message| message.contains("host observer refused"))
        );
        assert_eq!(error.reports.len(), 3);
        assert_eq!(
            shared
                .prefixes
                .last()
                .unwrap()
                .iter()
                .map(|row| (row.player, row.progress.song_ns, row.progress.hits))
                .collect::<Vec<_>>(),
            PLAYERS.map(|player| (player, 20_000_000, 1))
        );
        for (index, state) in states.iter_mut().enumerate() {
            assert_eq!(state.score.hits, 1);
            assert_eq!(
                state.gauge.snapshot().failure,
                Some(crate::gauge::GaugeFailure::InstantDeath)
            );
            assert_eq!(
                f.group.player_gameplay_fence(state.player),
                Some(Timestamp::from_nanos(20_000_000))
            );
            assert_eq!(state.competition.as_ref().unwrap().reports.len(), 2);
            assert_eq!(state.competition.as_ref().unwrap().marks, 0);
            assert_eq!(
                error.reports[index].report.hazard_events,
                actual[index].report.hazard_events
            );
            assert_eq!(
                error.reports[index].report.audio_commands,
                actual[index].report.audio_commands
            );
            assert_eq!(
                error.reports[index].report.audio_failures,
                actual[index].report.audio_failures
            );
            let stop = AudioCommand::Stop {
                voice: VoiceId(1 + 2 * index as u64),
                at: Timestamp::from_nanos(40_000_000),
            };
            if capacity == 8 || index == 0 {
                assert_eq!(actual[index].report.audio_commands, [stop]);
            } else {
                assert_eq!(actual[index].report.audio_failures[0].command, stop);
            }
            let file = state.capture.take().unwrap().into_file();
            assert_eq!(file.records.len(), 2);
            let rebuilt = crate::replay_playback::reconstruct(&f.source, file, limits()).unwrap();
            assert_eq!(
                rebuilt.engine().stable_hash().unwrap(),
                f.group
                    .member_judge(state.player)
                    .unwrap()
                    .stable_hash()
                    .unwrap()
            );
        }
        assert_eq!(shared.marks, 0);
        assert_eq!(evidence.admitted_stops(), if capacity == 8 { 3 } else { 1 });
        assert!(!f.group.poisoned());
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
    }
}

#[test]
fn cohort_completion_marks_members_then_group_only_after_genuine_output_guards() {
    for finite in [true, false] {
        let mut f = Fixture::new(finite, 8);
        let trace = Trace::default();
        let mut states = states(&mut f, &trace);
        if !finite {
            completion(&mut f, &mut states);
        }
        let mut shared = Shared {
            trace: trace.clone(),
            ..Default::default()
        };
        let mut host_port = Host {
            trace: trace.clone(),
            ..Default::default()
        };
        let mut control = Control::default();
        run(
            &mut f,
            &mut states,
            &mut shared,
            &mut host_port,
            &mut control,
            finite,
            false,
            None,
        )
        .unwrap();
        assert_eq!(shared.marks, 1);
        assert!(
            states
                .iter()
                .all(|state| state.competition.as_ref().unwrap().marks == 1)
        );
        let observed = trace.borrow();
        assert_eq!(
            &observed[observed.len() - 3..],
            &[
                Seen::MemberCompleted(PlayerId(7)),
                Seen::MemberCompleted(PlayerId(u32::MAX)),
                Seen::GroupCompleted
            ]
        );
        assert_eq!(control.reads, 0);
        if finite {
            assert_eq!(f.device.step, 3);
            assert_eq!(host_port.ends, [Timestamp::from_nanos(10_000_000)]);
            assert!(
                states
                    .iter()
                    .all(|state| state.last_song == Timestamp::from_nanos(10_000_000)
                        && state.score.hits == 0)
            );
            assert!(
                f.device
                    .report
                    .unwrap()
                    .playback_end_physical_frame
                    .is_some()
            );
            assert!(f.device.pcm.iter().all(|&v| v == 0.0));
        } else {
            assert!((7..=12).contains(&f.device.step));
            assert!(host_port.ends.is_empty());
            assert!(
                states
                    .iter()
                    .all(|state| state.score.hits == 1 && state.last_song.as_nanos() >= 30_000_001)
            );
            assert_eq!(&f.device.pcm[30..33], &[0.5, 1.0, 0.0]);
            assert_eq!(&f.device.pcm[50..53], &[0.125, 0.25, 0.0]);
            assert_eq!(
                (f.bgm.report().remaining, f.bgm.report().outstanding),
                (0, 0)
            );
            let rendered = f.device.report.unwrap();
            assert_eq!((rendered.active_voices, rendered.pending_commands), (0, 0));
            assert_eq!(rendered.counters.commands_applied, 3);
        }
    }
    for case in 0..3 {
        let mut f = Fixture::new(false, 8);
        let trace = Trace::default();
        let mut states = states(&mut f, &trace);
        let mut shared = Shared {
            reject: case == 2,
            ..Default::default()
        };
        let mut host_port = Host {
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
            &mut states,
            &mut shared,
            &mut host_port,
            &mut control,
            false,
            false,
            seconds,
        );
        if case == 2 {
            let error = result.unwrap_err();
            let error = error
                .downcast_ref::<NativeCohortObservationError>()
                .unwrap();
            assert_eq!(error.reports.len(), 2);
            assert!(
                error
                    .failures
                    .iter()
                    .any(|message| message.contains("group observer refused"))
            );
            assert!(
                states
                    .iter()
                    .all(|state| state.capture.as_ref().unwrap().records().len() == 1)
            );
            assert_eq!(host_port.batches.len(), 1);
        } else {
            result.unwrap();
        }
        assert_eq!(shared.marks, 0);
        assert!(
            states
                .iter()
                .all(|state| state.competition.as_ref().unwrap().marks == 0)
        );
        assert_eq!(f.device.step, if case == 0 { 0 } else { 1 });
        assert_eq!(control.reads, if case == 1 { 3 } else { 0 });
        assert!(host_port.ends.is_empty());
    }
}
