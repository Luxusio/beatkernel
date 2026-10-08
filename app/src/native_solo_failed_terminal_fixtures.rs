// Deferred common native pump over real Runtime/queue/Mixer, never an OS device.
use super::*;
use crate::{gauge::GaugeFailure, offline::OwnedStopEvidence};
use beatkernel::interaction::InputOwner;

fn owner(f: &mut Fixture) -> NativeGameplaySession<'_> {
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
    }
}
fn settings(finite: bool) -> NativeGameplayConfig {
    NativeGameplayConfig {
        origin: point(1, 0),
        stream_origin: point(2, 0),
        playback_origin: point(2, 0),
        song_origin: Timestamp::ZERO,
        sample_rate: 1000,
        end_song: finite.then_some(Timestamp::from_nanos(80_000_000)),
        advance_lag: Duration::from_nanos(10_000_000),
        seconds: None,
        pause_supported: false,
        logical_schedule: false,
    }
}
struct Pump<'a> {
    device: &'a mut Device,
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
            return Err("fixture exhausted without natural completion".into());
        }
        if self.device.step == 2 {
            retain_input(events, input(20_000_000, 1, ButtonState::Down))?;
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
fn run(f: &mut Fixture, finite: bool) -> NativeGameplayResult<()> {
    run_gameplay(
        &mut Pump {
            device: &mut f.device,
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
        settings(finite),
    )
}
fn prepared(f: &Fixture) -> crate::PreparedBms {
    let compiled = f.source.compile().unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5], limits).unwrap(),
    )
    .unwrap();
    let sounds = f
        .source
        .notes
        .iter()
        .map(|note| SoundBinding {
            object: note.object,
            stage: if compiled
                .chart
                .objects()
                .iter()
                .find(|object| object.id == note.object)
                .unwrap()
                .time
                .end
                .is_some()
            {
                beatkernel::judge::JudgeStage::HoldHead
            } else {
                beatkernel::judge::JudgeStage::Instant
            },
            sample: SampleId(1),
            voice: VoiceId(u64::from(note.lane.control().0)),
            gain: 1.0,
        })
        .collect();
    crate::PreparedBms {
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
    }
}
fn fixture(finite: bool, recoverable: bool) -> Fixture {
    let mut f = gauge_fence::fatal_fixture(8, 128);
    if recoverable {
        f.source.mines[0].damage = beatkernel_bms::MineDamage::from_raw(50).unwrap();
    }
    let prepared = prepared(&f);
    f.completion =
        (!finite).then(|| SongCompletion::prepare(&prepared, 0, 0, 0, ClockDomainId(2)).unwrap());
    f.bgm = BgmFeeder::new(
        prepared.bgm_commands.clone(),
        crate::bgm::BgmConfig {
            output_origin: point(2, 0),
            sample_rate: 1000,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(10_000_000),
            max_pending: 4,
        },
    )
    .unwrap();
    let judge = crate::native_judge::NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(2),
        end: None,
    }
    .judge(&f.source, prepared.compiled.chart)
    .unwrap();
    f.capture = crate::native_judge::prepare_capture_for_source(
        &f.source,
        &judge,
        ClockDomainId(1),
        Timestamp::ZERO,
        0,
        Some(limits()),
    )
    .unwrap();
    let bindings = BindingMap::from_bindings([0x11, 0x12].map(|lane| Binding {
        device: DeviceSelector::Exact(DeviceId(1)),
        physical: PhysicalControlId::keyboard(4u16),
        game_control: GameControlId(lane),
    }))
    .unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    f.runtime = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        prepared.sounds,
        0,
    )
    .unwrap();
    let mut mixer_config = MixerConfig::new(
        prepared.bank.format(),
        ClockDomainId(2),
        Timestamp::ZERO,
        AudioLimits::new(8, 4, 16, 32, 8).unwrap(),
    );
    if finite {
        f.runtime
            .set_song_end(Timestamp::from_nanos(80_000_000))
            .unwrap();
        mixer_config = mixer_config.with_playback_end_frame(80);
        f.pause = NativePause::new(point(2, 0), ClockDomainId(1), 1000)
            .unwrap()
            .with_playback_end_frame(80)
            .unwrap();
        let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 1000, 80).unwrap();
        end.observe(
            None,
            ClockPair {
                source: point(2, 0),
                target: point(1, 0),
            },
        )
        .unwrap();
        f.end = Some(end);
    }
    f.device.mixer = Mixer::new(mixer_config, prepared.bank, consumer).unwrap();
    f
}

#[test]
fn native_solo_pump_finishes_failed_output_but_preserves_hold_future_hazards_and_capture() {
    for finite in [false, true] {
        let mut f = fixture(finite, false);
        run(&mut f, finite).unwrap();
        assert!(f.device.step >= if finite { 9 } else { 7 });
        assert!(
            f.device.step <= 12,
            "the device never returned closed or cancellation"
        );
        assert_eq!(
            f.runtime.gameplay_fence(),
            Some(Timestamp::from_nanos(20_000_000))
        );
        assert_eq!(f.gauge.snapshot().failure, Some(GaugeFailure::InstantDeath));
        assert!(!f.gauge.can_clear());
        assert_eq!(f.runtime.judge().remaining_hazards(), 1);
        assert!(f.runtime.judge().is_held(InputOwner {
            source: DeviceId(1),
            physical: PhysicalControlId::keyboard(4u16),
            game_control: GameControlId(0x11)
        }));
        assert_eq!(&f.device.pcm[20..23], &[0.0; 3]);
        assert_eq!(&f.device.pcm[50..53], &[0.125, 0.25, 0.0]);
        assert!(f.device.pcm[53..].iter().all(|&sample| sample == 0.0));
        assert_eq!(f.device.mixer.counters().commands_applied, 5);
        assert_eq!(f.bgm.report().remaining, 0);
        assert_eq!(f.bgm.report().outstanding, 0);
        let hash = f.runtime.judge().stable_hash().unwrap();
        let file = f.capture.take().unwrap().into_file();
        assert_eq!(file.records.len(), 3);
        assert_eq!(
            file.records.last().unwrap().song_time,
            Timestamp::from_nanos(20_000_000)
        );
        let rebuilt = crate::replay_playback::reconstruct(&f.source, file, limits()).unwrap();
        assert_eq!(rebuilt.engine().stable_hash().unwrap(), hash);
        assert_eq!(rebuilt.engine().remaining_hazards(), 1);
    }
    let mut zero = fixture(false, true);
    run(&mut zero, false).unwrap();
    assert!(
        zero.device.step >= 9,
        "recoverable depletion cannot bypass the real last deadline"
    );
    assert_eq!(zero.runtime.gameplay_fence(), None);
    assert_eq!(zero.gauge.snapshot().failure, None);
    assert_eq!(zero.runtime.judge().remaining_hazards(), 0);
    let mut manual = fixture(false, false);
    manual
        .runtime
        .advance_to(point(1, 0), &ExplicitDomains, point(2, 0))
        .unwrap();
    manual.runtime.fence_gameplay();
    let hash = manual.runtime.judge().stable_hash().unwrap();
    assert!(
        run(&mut manual, false)
            .unwrap_err()
            .to_string()
            .contains("fixture exhausted")
    );
    assert_eq!(manual.gauge, BmsGauge::default());
    assert_eq!(manual.runtime.judge().stable_hash().unwrap(), hash);
}

#[test]
fn native_solo_stop_evidence_is_new_admission_only_and_finite_boundary_guards_remain_required() {
    for capacity in [2, 3, 8] {
        let mut f = gauge_fence::fatal_fixture(capacity, 128);
        let report = f
            .runtime
            .process_input(
                input(20_000_000, 1, ButtonState::Down),
                &ExplicitDomains,
                point(2, 20_000_000),
            )
            .unwrap();
        let original = report.clone();
        let mut evidence = OwnedStopEvidence::default();
        let result = publish_with_stops(&mut owner(&mut f), report, &mut evidence);
        if capacity < 4 {
            let failure = result.unwrap_err();
            let failure = failure
                .downcast_ref::<NativeReportObservationError>()
                .unwrap();
            assert_eq!(failure.report.audio_commands.len(), capacity);
            assert_eq!(failure.report.audio_failures.len(), 4 - capacity);
            assert_eq!(failure.report.bound_inputs, original.bound_inputs);
            assert_eq!(failure.report.judge_events, original.judge_events);
            assert!(
                failure
                    .report
                    .audio_failures
                    .iter()
                    .all(|error| error.reason == QueuePushError::Full)
            );
        } else {
            result.unwrap();
        }
        assert_eq!(
            evidence.admitted_stops(),
            capacity.min(4).saturating_sub(2) as u64
        );
        let retained = evidence.admitted_stops();
        // The original report may be observed again, but its old command list
        // never becomes another admission receipt and the Runtime stop is once-only.
        publish_with_stops(&mut owner(&mut f), original, &mut evidence).unwrap();
        assert_eq!(evidence.admitted_stops(), retained);
        assert!(f.runtime.fence_gameplay_sounds(Timestamp::ZERO).is_none());
    }
    let mut f = fixture(true, false);
    run(&mut f, true).unwrap();
    let rendered = f.device.report.unwrap();
    let mut actual_end = NativeEnd::new(point(2, 0), ClockDomainId(1), 1000, 80).unwrap();
    actual_end
        .observe(
            None,
            ClockPair {
                source: point(2, 0),
                target: point(1, 0),
            },
        )
        .unwrap();
    let boundary = actual_end
        .observe(
            Some(rendered),
            ClockPair {
                source: point(2, 90_000_000),
                target: point(1, 90_000_000),
            },
        )
        .unwrap()
        .unwrap();
    let config = settings(true);
    let bgm = f.bgm.report();
    let frozen = Timestamp::from_nanos(20_000_000);
    let admitted = f.runtime.admitted_audio_commands();
    assert_eq!(
        admitted, 5,
        "two heads, two Stops and the independent BGM share one producer"
    );
    assert_eq!(
        (
            rendered.counters.commands_consumed,
            rendered.counters.commands_applied
        ),
        (5, 5)
    );
    assert!(!finite_done(
        config,
        Some(boundary),
        boundary.host,
        frozen,
        false,
        false
    ));
    assert!(finite_done_with_terminal(
        config,
        Some(boundary),
        boundary.host,
        frozen,
        false,
        false,
        true,
        bgm,
        Some(rendered),
        admitted
    ));
    for (edge, last, backlog, resuming, proof) in [
        (None, boundary.host, false, false, true),
        (
            Some(boundary),
            point(1, boundary.host.timestamp.as_nanos() - 1),
            false,
            false,
            true,
        ),
        (Some(boundary), boundary.host, true, false, true),
        (Some(boundary), boundary.host, false, true, true),
        (Some(boundary), boundary.host, false, false, false),
    ] {
        assert!(!finite_done_with_terminal(
            config,
            edge,
            last,
            frozen,
            backlog,
            resuming,
            proof,
            bgm,
            Some(rendered),
            admitted
        ));
    }
    assert!(!finite_done_with_terminal(
        config,
        Some(boundary),
        boundary.host,
        frozen,
        false,
        false,
        true,
        crate::bgm::BgmFeedReport {
            outstanding: 1,
            ..bgm
        },
        Some(rendered),
        admitted
    ));
    assert!(!finite_done_with_terminal(
        config,
        Some(boundary),
        boundary.host,
        frozen,
        false,
        false,
        true,
        bgm,
        Some(RenderReport {
            paused: false,
            ..rendered
        }),
        admitted
    ));
    assert!(!finite_done_with_terminal(
        config,
        Some(boundary),
        boundary.host,
        frozen,
        false,
        false,
        true,
        bgm,
        Some(RenderReport {
            playback_end_physical_frame: None,
            ..rendered
        }),
        admitted
    ));
    // An endpoint freezes the consumer, not the producer. A new real accepted
    // Stop is stranded even though the Mixer's pending heap remains empty.
    f.runtime
        .enqueue_audio(AudioCommand::Stop {
            voice: VoiceId(u64::MAX),
            at: Timestamp::from_nanos(80_000_000),
        })
        .unwrap();
    let stranded = f.device.mixer.render(&mut [0.0]).unwrap();
    assert_eq!(f.runtime.admitted_audio_commands(), 6);
    assert_eq!(
        (
            stranded.counters.commands_consumed,
            stranded.counters.commands_applied,
            stranded.pending_commands
        ),
        (5, 5, 0)
    );
    assert!(!finite_done_with_terminal(
        config,
        Some(boundary),
        boundary.host,
        frozen,
        false,
        false,
        true,
        bgm,
        Some(stranded),
        f.runtime.admitted_audio_commands()
    ));
}
