struct SoloIntervalDevice<'a> {
    inner: &'a mut Device,
    conflicting: bool,
}
impl NativeGameplayDevice for SoloIntervalDevice<'_> {
    fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
        self.inner.observe(discipline)
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        self.inner.render_report()
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        self.inner.host_now()
    }
    fn pause_observation(
        &mut self,
        _: ClockPair,
    ) -> NativeGameplayResult<crate::live_pause::LivePauseObservation> {
        let render = self.inner.report.unwrap();
        let ns = render.start_frame as i64 * 1_000_000;
        let actual = beatkernel_platform::audio::asio::AsioPresentationObservation::from_render(
            render,
            1000,
            beatkernel_platform::audio::asio::MultimediaHostInterval {
                before: point(1, ns + 2_000_000),
                after: point(1, ns + 4_000_000),
            },
            0,
            0,
            point(2, 0),
        )?;
        Ok(crate::live_pause::LivePauseObservation::Interval {
            observation: Some(crate::playback_pause::PauseIntervalObservation {
                output_origin: actual.output_origin,
                sample_rate: actual.sample_rate,
                render: actual.render,
                clock: crate::native_start::StartInterval::new(
                    actual.output,
                    actual.host.before,
                    actual.host.after,
                )?,
            }),
            now: self.inner.host_now()?,
        })
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if !self.inner.pause_scenario {
            return self.inner.acquire(events);
        }
        match self.inner.step {
            1 => {
                retain_input(events, input(10_000_000, 1, ButtonState::Down))?;
                self.inner.viewer.as_ref().unwrap().request_pause(true);
            }
            3 => {
                if self.conflicting {
                    retain_input(events, input(21_000_000, 2, ButtonState::Down))?;
                }
                retain_input(
                    events,
                    input(
                        24_000_000,
                        if self.conflicting { 3 } else { 2 },
                        ButtonState::Up,
                    ),
                )?;
                self.inner.viewer.as_ref().unwrap().request_pause(false);
            }
            5 => retain_input(events, input(46_000_000, 3, ButtonState::Down))?,
            6 => retain_input(events, input(47_000_000, 4, ButtonState::Up))?,
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
fn run_solo_interval(f: &mut Fixture, finite: bool, conflicting: bool) -> NativeGameplayResult<()> {
    let pause_supported = f.device.pause_scenario;
    run_gameplay(
        &mut SoloIntervalDevice {
            inner: &mut f.device,
            conflicting,
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
            end_song: finite.then_some(Timestamp::from_nanos(10_000_000)),
            advance_lag: Duration::from_nanos(10_000_000),
            seconds: None,
            pause_supported,
            logical_schedule: true,
        },
    )
}
#[test]
fn interval_solo_exact_freeze_and_backlogged_resume_preserve_original_provenance() {
    let (publisher, viewer) = player::channel();
    let mut fixture = Fixture::new(false, true);
    fixture.device.pause_scenario = true;
    fixture.device.viewer = Some(viewer);
    player::with_publisher(publisher, || {
        run_solo_interval(&mut fixture, false, false).map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(fixture.device.seeded, 1);
    assert_eq!(fixture.device.fallback, 0);
    assert_eq!(fixture.delivery.observed_events(), 4);
    let transport = fixture.runtime.transport_mut();
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(21_000_000))
            .unwrap(),
        Timestamp::from_nanos(21_000_000)
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(22_000_000))
            .unwrap(),
        Timestamp::from_nanos(20_000_000)
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(44_000_000))
            .unwrap(),
        Timestamp::from_nanos(20_000_000)
    );
    let records = fixture.capture.as_ref().unwrap().records();
    let buttons = records
        .iter()
        .filter_map(|r| match &r.operation {
            ReplayOperation::Input(input) => match &input.physical {
                PhysicalInputEvent::Button(b) => Some((
                    b.state,
                    b.meta.timestamp,
                    b.meta.sequence,
                    b.meta.original_clock_point,
                    r.song_time,
                )),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(buttons.len(), 4);
    assert_eq!(buttons[0].1, Timestamp::from_nanos(10_000_000));
    assert_eq!(
        buttons[1],
        (
            ButtonState::Up,
            Timestamp::from_nanos(44_000_000),
            2,
            Some(point(1, 24_000_000)),
            Timestamp::from_nanos(20_000_000)
        )
    );
    assert_eq!(
        buttons[2],
        (
            ButtonState::Down,
            Timestamp::from_nanos(46_000_000),
            3,
            None,
            Timestamp::from_nanos(22_000_000)
        )
    );
    assert_eq!(buttons[3].1, Timestamp::from_nanos(47_000_000));
    assert!(
        records
            .iter()
            .any(|r| matches!(r.operation, ReplayOperation::Advance)
                && r.song_time == Timestamp::from_nanos(20_000_000))
    );
    let file = beatkernel::replay::codec::ReplayFile::new(
        fixture.capture.as_ref().unwrap().header().clone(),
        records.to_vec(),
    );
    crate::replay_playback::reconstruct(&fixture.source, file, limits()).unwrap();
}
#[test]
fn interval_solo_conflicting_queued_prefix_retains_actual_accepted_capture() {
    let (publisher, viewer) = player::channel();
    let mut fixture = Fixture::new(false, true);
    fixture.device.pause_scenario = true;
    fixture.device.viewer = Some(viewer);
    let error = player::with_publisher(publisher, || {
        run_solo_interval(&mut fixture, false, true).map_err(|e| e.to_string())
    })
    .unwrap_err();
    assert!(error.contains("queued input exceeds the exact pause song prefix"));
    let inputs = fixture
        .capture
        .as_ref()
        .unwrap()
        .records()
        .iter()
        .filter(|r| matches!(r.operation, ReplayOperation::Input(_)))
        .count();
    assert_eq!(inputs, 1);
    assert_eq!(fixture.runtime.telemetry().counters().inputs, 1);
}
#[test]
fn interval_solo_finite_marker_wins_without_manual_pause_or_extra_judgement() {
    let mut fixture = Fixture::new(true, false);
    run_solo_interval(&mut fixture, true, false).unwrap();
    assert_eq!(fixture.pause.phase(), PausePhase::Running);
    assert_eq!(fixture.device.seeded, 0);
    assert_eq!(fixture.runtime.telemetry().counters().inputs, 0);
    assert_eq!(
        fixture
            .capture
            .as_ref()
            .unwrap()
            .records()
            .last()
            .unwrap()
            .song_time,
        Timestamp::from_nanos(10_000_000)
    );
}
