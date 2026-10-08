// Deferred native cohort pump and admitted Stop evidence; portable output only.
use super::*;
use crate::{gauge::GaugeFailure, offline::OwnedStopEvidence};
use crate::native_gameplay::{InputBatch, retain_input};
const PLAYERS: [PlayerId; 3] = [PlayerId(7), PlayerId(101), PlayerId(u32::MAX)];
const SOURCES: [u64; 3] = [u64::MAX - 2, u64::MAX - 1, u64::MAX];
const TEXT: &str = "#BPM 3000\n#WAV01 head.wav\n#00011:01000100\n#000D1:00ZZ0001\n";

struct Pump<'a> {
    device: &'a mut Device,
    all_failed: bool,
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
        if self.device.step > 14 {
            return Err("fixture exhausted without natural cohort completion".into());
        }
        if self.device.step == 1 {
            for source in SOURCES {
                retain_input(events, button(source, 0, 1, ButtonState::Down))?;
            }
            if !self.all_failed {
                for source in &SOURCES[1..] {
                    retain_input(events, button(*source, 10_000_000, 2, ButtonState::Up))?;
                }
            }
        }
        if !self.all_failed {
            for source in &SOURCES[1..] {
                if self.device.step == 4 {
                    retain_input(events, button(*source, 40_000_000, 3, ButtonState::Down))?;
                }
                if self.device.step == 5 {
                    retain_input(events, button(*source, 50_000_000, 4, ButtonState::Up))?;
                }
            }
        }
        Ok(InputBatch {
            completed_through: Some(self.device.host_now()?),
            backlog: false,
            closed: false,
        })
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
fn run(f: &mut Fixture, finite: bool, all_failed: bool) -> NativeGameplayResult<()> {
    run_cohort(
        &mut Pump {
            device: &mut f.device,
            all_failed,
        },
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
            end_song: finite.then_some(Timestamp::from_nanos(80_000_000)),
            advance_lag: Duration::from_nanos(10_000_000),
            seconds: None,
            pause_supported: false,
            logical_schedule: false,
        },
    )
}
fn fixture(finite: bool) -> Fixture {
    let mut f = gauge_fence::cohort_fixture(TEXT, &[0x11], 16, 128);
    let compiled = f.source.compile().unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm_limits = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5], pcm_limits).unwrap(),
    )
    .unwrap();
    let sounds = f
        .source
        .notes
        .iter()
        .map(|note| SoundBinding {
            object: note.object,
            stage: beatkernel::judge::JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(1),
            gain: 1.0,
        })
        .collect::<Vec<_>>();
    let prepared = crate::PreparedBms {
        source: f.source.clone(),
        compiled,
        bank,
        sounds,
        bgm_commands: vec![AudioCommand::Play {
            sample: SampleId(1),
            voice: VoiceId(99),
            at: Timestamp::from_nanos(50_000_000),
            gain: 0.5,
        }],
    };
    for state in &mut f.states {
        state.completion =
            Some(SongCompletion::prepare(&prepared, 0, 0, 0, ClockDomainId(2)).unwrap());
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
    let configs = PLAYERS
        .into_iter()
        .zip(SOURCES)
        .enumerate()
        .map(|(index, (player, source))| {
            let judge = crate::native_judge::NativeJudgeConfig {
                early: 0,
                late: 0,
                offset: 0,
                preroll: 0,
                output: ClockDomainId(2),
                end: None,
            }
            .judge(&f.source, prepared.compiled.chart.clone())
            .unwrap();
            MemberConfig {
                player,
                device: Some(DeviceId(source)),
                judge,
                bindings: BindingMap::from_bindings([Binding {
                    device: DeviceSelector::Exact(DeviceId(source)),
                    physical: PhysicalControlId::keyboard(4u16),
                    game_control: GameControlId(0x11),
                }])
                .unwrap(),
                sounds: prepared
                    .sounds
                    .iter()
                    .map(|sound| SoundBinding {
                        voice: VoiceId(1 + index as u64 * 2),
                        ..*sound
                    })
                    .collect(),
            }
        })
        .collect();
    let (producer, consumer) = command_queue(16).unwrap();
    f.group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        producer,
        configs,
        0,
        &[VoiceId(99)],
    )
    .unwrap();
    let mut mixer_config = MixerConfig::new(
        format,
        ClockDomainId(2),
        Timestamp::ZERO,
        AudioLimits::new(16, 8, 32, 128, 16).unwrap(),
    );
    if finite {
        f.group
            .set_song_end(Timestamp::from_nanos(80_000_000))
            .unwrap();
        mixer_config = mixer_config.with_playback_end_frame(80);
        f.pause = NativePause::new(output(0), ClockDomainId(1), 1000)
            .unwrap()
            .with_playback_end_frame(80)
            .unwrap();
        let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 80).unwrap();
        end.observe(
            None,
            ClockPair {
                source: output(0),
                target: host(0),
            },
        )
        .unwrap();
        f.end = Some(end);
    }
    f.device.mixer = Mixer::new(mixer_config, prepared.bank, consumer).unwrap();
    f
}

#[test]
fn native_cohort_pump_drains_all_failed_or_waits_for_actual_survivors_without_rewriting_prefixes() {
    for (finite, all_failed) in [(false, true), (false, false), (true, true), (true, false)] {
        let mut f = fixture(finite);
        run(&mut f, finite, all_failed).unwrap();
        assert!(f.device.step >= if finite || !all_failed { 9 } else { 7 });
        assert!(
            f.device.step <= 14,
            "the fixture never closes or cancels the pump"
        );
        assert_eq!(f.bgm.report().remaining, 0);
        assert_eq!(f.bgm.report().outstanding, 0);
        assert_eq!(
            f.device.mixer.counters().unknown_stops,
            if all_failed { 3 } else { 1 }
        );
        assert_eq!(f.device.mixer.counters().commands_applied, 7);
        assert_eq!(&f.device.pcm[10..13], &[0.75, 1.0, 0.0]);
        assert_eq!(
            &f.device.pcm[50..53],
            if all_failed {
                &[0.125, 0.25, 0.0]
            } else {
                &[0.625, 1.0, 0.0]
            }
        );
        assert_eq!(f.states[0].last_song, Timestamp::from_nanos(20_000_000));
        for (index, state) in f.states.iter_mut().enumerate() {
            let failed = all_failed || index == 0;
            assert_eq!(
                state.gauge.snapshot().failure,
                failed.then_some(GaugeFailure::InstantDeath)
            );
            assert_eq!(
                f.group.player_gameplay_fence(state.player),
                failed.then_some(Timestamp::from_nanos(20_000_000))
            );
            let judge = f.group.member_judge(state.player).unwrap();
            assert_eq!(judge.remaining_hazards(), usize::from(failed));
            assert_eq!(state.score.hits, if failed { 1 } else { 2 });
            if !failed {
                assert!(state.last_song.as_nanos() >= if finite { 80_000_000 } else { 60_000_001 });
            }
            let hash = judge.stable_hash().unwrap();
            let file = state.capture.take().unwrap().into_file();
            if failed {
                assert_eq!(
                    file.records.last().unwrap().song_time,
                    Timestamp::from_nanos(20_000_000)
                );
            }
            let first = file
                .records
                .iter()
                .find_map(|record| match &record.operation {
                    ReplayOperation::Input(input) => Some(input),
                    _ => None,
                })
                .unwrap();
            assert_eq!(first.physical.meta().source, DeviceId(SOURCES[index]));
            let rebuilt = crate::replay_playback::reconstruct(&f.source, file, limits()).unwrap();
            assert_eq!(rebuilt.engine().stable_hash().unwrap(), hash);
        }
        assert!(!f.group.poisoned());
    }
}

#[test]
fn native_local_partial_stop_reports_keep_exact_evidence_and_finite_completion_requires_real_boundary()
 {
    let mut f = gauge_fence::cohort_fixture(
        "#BPM 3000\n#WAV01 future.wav\n#00011:00000100\n#000D1:00ZZ0001\n",
        &[0x11],
        2,
        128,
    );
    let mut evidence = OwnedStopEvidence::default();
    for source in SOURCES {
        let InputResult::Processed(mut reports) = f
            .group
            .process_input(
                button(source, 0, 1, ButtonState::Down),
                &ExplicitDomains,
                output(0),
            )
            .unwrap()
        else {
            panic!("assigned source");
        };
        observe_reports_with_stops(
            &mut reports,
            &mut f.states,
            &mut f.group,
            None,
            &mut evidence,
        )
        .unwrap();
    }
    let mut reports = f
        .group
        .advance_to(host(20_000_000), &ExplicitDomains, output(20_000_000))
        .unwrap();
    let error = observe_reports_with_stops(
        &mut reports,
        &mut f.states,
        &mut f.group,
        None,
        &mut evidence,
    )
    .unwrap_err();
    let error = error
        .downcast_ref::<NativeCohortObservationError>()
        .unwrap();
    assert_eq!(error.reports.len(), 3);
    assert_eq!(evidence.admitted_stops(), 2);
    for (index, player) in PLAYERS.into_iter().enumerate() {
        assert_eq!(
            reports[index].report.audio_commands.len(),
            usize::from(index < 2)
        );
        assert_eq!(
            reports[index].report.audio_failures.len(),
            usize::from(index == 2)
        );
        assert_eq!(
            error.reports[index].report.audio_commands,
            reports[index].report.audio_commands
        );
        assert_eq!(
            error.reports[index].report.audio_failures,
            reports[index].report.audio_failures
        );
        assert_eq!(
            f.group.player_gameplay_fence(player),
            Some(Timestamp::from_nanos(20_000_000))
        );
        assert_eq!(f.states[index].capture.as_ref().unwrap().records().len(), 2);
    }
    assert_eq!(
        reports[2].report.audio_failures[0].reason,
        QueuePushError::Full
    );
    assert!(
        observe_reports_with_stops(
            &mut reports,
            &mut f.states,
            &mut f.group,
            None,
            &mut evidence
        )
        .is_err()
    );
    assert_eq!(
        evidence.admitted_stops(),
        2,
        "republication is not another producer admission"
    );
    let raw = f.device.mixer.render(&mut [0.0; 32]).unwrap();
    assert_eq!(
        (raw.counters.commands_applied, raw.counters.unknown_stops),
        (2, 2)
    );
    crate::native_audio::validate_stop_evidence(Some(raw), &evidence).unwrap();

    let mut finite = fixture(true);
    run(&mut finite, true, true).unwrap();
    let raw = finite.device.report.unwrap();
    let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 80).unwrap();
    end.observe(
        None,
        ClockPair {
            source: output(0),
            target: host(0),
        },
    )
    .unwrap();
    let boundary = end
        .observe(
            Some(raw),
            ClockPair {
                source: output(90_000_000),
                target: host(90_000_000),
            },
        )
        .unwrap()
        .unwrap();
    let bgm = finite.bgm.report();
    assert_eq!(
        finite.group.admitted_audio_commands(),
        7,
        "three heads and three Stops plus shared BGM are counted once"
    );
    assert_eq!(
        (
            raw.counters.commands_consumed,
            raw.counters.commands_applied
        ),
        (7, 7)
    );
    assert!(!finite_cohort_done(
        Some(80_000_000),
        Some(boundary.host),
        Some(boundary.host),
        &finite.states,
        false,
        false
    ));
    assert!(finite_cohort_done_with_terminal(
        Some(80_000_000),
        Some(boundary.host),
        Some(boundary.host),
        &finite.states,
        &finite.group,
        false,
        false,
        bgm,
        Some(raw)
    ));
    for (presented, committed, backlog, resuming) in [
        (None, Some(boundary.host), false, false),
        (
            Some(boundary.host),
            Some(host(boundary.host.timestamp.as_nanos() - 1)),
            false,
            false,
        ),
        (Some(boundary.host), Some(boundary.host), true, false),
        (Some(boundary.host), Some(boundary.host), false, true),
    ] {
        assert!(!finite_cohort_done_with_terminal(
            Some(80_000_000),
            presented,
            committed,
            &finite.states,
            &finite.group,
            backlog,
            resuming,
            bgm,
            Some(raw)
        ));
    }
    assert!(!finite_cohort_done_with_terminal(
        Some(80_000_000),
        Some(boundary.host),
        Some(boundary.host),
        &finite.states,
        &finite.group,
        false,
        false,
        crate::bgm::BgmFeedReport {
            outstanding: 1,
            ..bgm
        },
        Some(raw)
    ));
    assert!(!finite_cohort_done_with_terminal(
        Some(80_000_000),
        Some(boundary.host),
        Some(boundary.host),
        &finite.states,
        &finite.group,
        false,
        false,
        bgm,
        Some(RenderReport {
            pending_commands: 1,
            ..raw
        })
    ));
    finite
        .group
        .enqueue_audio(AudioCommand::Stop {
            voice: VoiceId(u64::MAX),
            at: Timestamp::from_nanos(80_000_000),
        })
        .unwrap();
    let stranded = finite.device.mixer.render(&mut [0.0]).unwrap();
    assert_eq!(finite.group.admitted_audio_commands(), 8);
    assert_eq!(
        (
            stranded.counters.commands_consumed,
            stranded.counters.commands_applied,
            stranded.pending_commands
        ),
        (7, 7, 0)
    );
    assert!(!finite_cohort_done_with_terminal(
        Some(80_000_000),
        Some(boundary.host),
        Some(boundary.host),
        &finite.states,
        &finite.group,
        false,
        false,
        bgm,
        Some(stranded)
    ));
}
