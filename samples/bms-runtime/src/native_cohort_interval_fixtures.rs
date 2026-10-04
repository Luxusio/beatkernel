struct CohortIntervalDevice<'a> {
    inner: &'a mut Device,
    conflicting: bool,
}
impl NativeGameplayDevice for CohortIntervalDevice<'_> {
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
                before: host(ns + 2_000_000),
                after: host(ns + 4_000_000),
            },
            0,
            0,
            output(0),
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
    ) -> NativeGameplayResult<crate::native_gameplay::InputBatch> {
        use crate::native_gameplay::{InputBatch, retain_input};
        if !self.inner.pause_flow {
            return self.inner.acquire(events);
        }
        match self.inner.step {
            1 => {
                for device in [2, 1] {
                    retain_input(events, button(device, 10_000_000, 1, ButtonState::Down))?;
                }
                self.inner.viewer.as_ref().unwrap().request_pause(true);
            }
            3 => {
                for device in [2, 1] {
                    if self.conflicting {
                        retain_input(events, button(device, 21_000_000, 2, ButtonState::Down))?;
                    }
                    retain_input(
                        events,
                        button(
                            device,
                            24_000_000,
                            if self.conflicting { 3 } else { 2 },
                            ButtonState::Up,
                        ),
                    )?;
                }
                self.inner.viewer.as_ref().unwrap().request_pause(false);
            }
            6 => {
                for device in [2, 1] {
                    retain_input(events, button(device, 56_000_000, 3, ButtonState::Down))?;
                }
            }
            7 => {
                for device in [2, 1] {
                    retain_input(events, button(device, 57_000_000, 4, ButtonState::Up))?;
                }
            }
            _ => {}
        }
        Ok(InputBatch {
            backlog: self.inner.step == 6,
            closed: self.inner.step >= 8,
        })
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &PresentationDiscipline,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
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
fn run_cohort_interval(
    f: &mut Fixture,
    finite: bool,
    conflicting: bool,
) -> NativeGameplayResult<()> {
    let pause_supported = f.device.pause_flow;
    run_cohort(
        &mut CohortIntervalDevice {
            inner: &mut f.device,
            conflicting,
        },
        NativeCohortSession {
            network: None,
            group: &mut f.group,
            states: &mut f.states,
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
            seconds: None,
            pause_supported,
            logical_schedule: true,
        },
    )
}
#[test]
fn interval_cohort_sparse_members_reconcile_before_backlogged_originals_at_exact_song() {
    let (publisher, viewer) = player::channel();
    let mut fixture = Fixture::new(false, 16);
    fixture.device.pause_flow = true;
    fixture.device.viewer = Some(viewer);
    player::with_publisher(publisher, || {
        run_cohort_interval(&mut fixture, false, false).map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(fixture.device.seeded, 1);
    assert_eq!(fixture.delivery.observed_events(), 8);
    assert_eq!(
        fixture.states.iter().map(|s| s.player).collect::<Vec<_>>(),
        vec![PlayerId(7), PlayerId(u32::MAX)]
    );
    assert_eq!(
        fixture
            .group
            .transport_mut()
            .position_at(host(22_000_000).timestamp)
            .unwrap(),
        Timestamp::from_nanos(20_000_000)
    );
    assert_eq!(
        fixture
            .group
            .transport_mut()
            .position_at(host(54_000_000).timestamp)
            .unwrap(),
        Timestamp::from_nanos(20_000_000)
    );
    for (state, device) in fixture.states.iter().zip([1, 2]) {
        let capture = state.capture.as_ref().unwrap();
        let buttons = capture
            .records()
            .iter()
            .filter_map(|r| match &r.operation {
                ReplayOperation::Input(input) => match &input.physical {
                    PhysicalInputEvent::Button(b) => Some((
                        b.meta.source,
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
        assert!(buttons.iter().all(|b| b.0 == DeviceId(device)));
        assert_eq!(
            buttons[1],
            (
                DeviceId(device),
                ButtonState::Up,
                host(54_000_000).timestamp,
                2,
                Some(host(24_000_000)),
                Timestamp::from_nanos(20_000_000)
            )
        );
        assert_eq!(
            buttons[2],
            (
                DeviceId(device),
                ButtonState::Down,
                host(56_000_000).timestamp,
                3,
                None,
                Timestamp::from_nanos(22_000_000)
            )
        );
        assert_eq!(buttons[3].2, host(57_000_000).timestamp);
        let file = beatkernel::replay::codec::ReplayFile::new(
            capture.header().clone(),
            capture.records().to_vec(),
        );
        crate::replay_playback::reconstruct(&fixture.source, file, limits()).unwrap();
    }
}
#[test]
fn interval_cohort_conflicting_original_does_not_invent_a_rewound_member_prefix() {
    let (publisher, viewer) = player::channel();
    let mut fixture = Fixture::new(false, 16);
    fixture.device.pause_flow = true;
    fixture.device.viewer = Some(viewer);
    let error = player::with_publisher(publisher, || {
        run_cohort_interval(&mut fixture, false, true).map_err(|e| e.to_string())
    })
    .unwrap_err();
    assert!(error.contains("queued input exceeds the exact pause song prefix"));
    for state in &fixture.states {
        assert_eq!(
            state
                .capture
                .as_ref()
                .unwrap()
                .records()
                .iter()
                .filter(|r| matches!(r.operation, ReplayOperation::Input(_)))
                .count(),
            1
        );
        assert_eq!(state.last_song, Timestamp::from_nanos(10_000_000));
    }
}
#[test]
fn interval_cohort_terminal_marker_keeps_every_member_exact_and_unpaused_logically() {
    let mut fixture = Fixture::new(true, 8);
    run_cohort_interval(&mut fixture, true, false).unwrap();
    assert_eq!(fixture.pause.phase(), PausePhase::Running);
    assert_eq!(fixture.device.seeded, 0);
    assert!(
        fixture
            .states
            .iter()
            .all(|state| state.last_song == Timestamp::from_nanos(10_000_000)
                && state.score.hits == 0)
    );
}
