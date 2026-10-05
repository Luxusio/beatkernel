// Deferred actual cohort ownership with virtual pump effects, not a native driver.
use super::*;
use crate::native_pump_control::NativePumpControl;
use beatkernel::runtime::RuntimeProcessingClock;
use std::time::Duration as WaitDuration;

#[derive(Debug)]
struct Fault(&'static str);
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Fault {}
struct Control {
    readings: VecDeque<Result<u64, Fault>>,
    reads: usize,
    waits: Vec<WaitDuration>,
    fail_wait: Option<usize>,
}
impl Control {
    fn new(readings: impl IntoIterator<Item = u64>) -> Self {
        Self {
            readings: readings.into_iter().map(Ok).collect(),
            reads: 0,
            waits: Vec::new(),
            fail_wait: None,
        }
    }
}
impl NativePumpControl for Control {
    type Moment = u64;
    fn now(&mut self) -> NativeGameplayResult<u64> {
        self.reads += 1;
        Ok(self
            .readings
            .pop_front()
            .ok_or(Fault("unexpected control read"))??)
    }
    fn checked_add(moment: u64, duration: WaitDuration) -> Option<u64> {
        moment.checked_add(u64::try_from(duration.as_nanos()).ok()?)
    }
    fn wait(&mut self, duration: WaitDuration) -> NativeGameplayResult<()> {
        self.waits.push(duration);
        if self.fail_wait == Some(self.waits.len()) {
            Err(Box::new(Fault("wait fault")))
        } else {
            Ok(())
        }
    }
}
fn run(f: &mut Fixture, seconds: Option<u64>, control: &mut Control) -> NativeGameplayResult<()> {
    f.group
        .set_processing_clock(RuntimeProcessingClock::Disabled);
    run_cohort_with_control(
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
            seconds,
            pause_supported: false,
            logical_schedule: true,
        },
        control,
    )
}

#[test]
fn repeated_virtual_cohort_runs_preserve_each_capture_and_shared_pcm_without_reading_unlimited_clock()
 {
    let mut previous = None;
    for timed in [false, true, false] {
        let mut f = Fixture::new(false, 8);
        let mut control = if timed {
            Control::new([300, 400, 500, 600, 700])
        } else {
            Control::new([])
        };
        run(&mut f, timed.then_some(1), &mut control).unwrap();
        assert_eq!(
            f.device.step, 4,
            "the actual fixture device closes before any completion policy exists"
        );
        assert!(f.states.iter().all(|state| state.completion.is_none()));
        assert_eq!(control.reads, if timed { 5 } else { 0 });
        assert_eq!(control.waits, [WaitDuration::from_millis(1); 3]);
        let mut expected = vec![0.0; 40];
        expected[30] = 0.5;
        expected[31] = 1.0;
        assert_eq!(f.device.pcm, expected);
        assert_eq!(f.device.mixer.counters().commands_applied, 2);
        assert_eq!(f.merger.pending(), 0);
        assert_eq!(f.delivery.observed_events(), 2);
        let mut hashes = Vec::new();
        let mut bytes = Vec::new();
        for state in &mut f.states {
            assert_eq!(state.score.hits, 1);
            let capture = state.capture.take().unwrap();
            assert_eq!(
                capture
                    .records()
                    .iter()
                    .map(|record| record.song_time.as_nanos())
                    .collect::<Vec<_>>(),
                [0, 20_000_000, 20_000_000]
            );
            let ReplayOperation::Input(input) = &capture.records()[1].operation else {
                panic!("actual member input");
            };
            assert_eq!(input.physical.meta().clock_domain, ClockDomainId(1));
            assert_eq!(
                input.physical.meta().timestamp,
                Timestamp::from_nanos(20_000_000)
            );
            assert_eq!(
                input.physical.meta().source,
                DeviceId(if state.player == PlayerId(7) { 1 } else { 2 })
            );
            hashes.push(
                f.group
                    .member_judge(state.player)
                    .unwrap()
                    .stable_hash()
                    .unwrap(),
            );
            bytes.push(capture.into_bytes().unwrap());
        }
        if let Some((old_hashes, old_bytes)) = &previous {
            assert_eq!(old_hashes, &hashes);
            assert_eq!(old_bytes, &bytes);
        }
        previous = Some((hashes, bytes));
    }
    let (publisher, viewer) = player::channel();
    viewer.cancel();
    let mut f = Fixture::new(false, 8);
    let mut control = Control::new([500]);
    player::with_publisher(publisher, || {
        run(&mut f, Some(1), &mut control).map_err(|error| error.to_string())
    })
    .unwrap();
    assert_eq!(
        (f.device.step, control.reads, control.waits.len()),
        (0, 1, 0),
        "setup reads once, cancellation short-circuits the loop deadline read"
    );
    assert!(
        f.states
            .iter()
            .all(|state| state.capture.as_ref().unwrap().records().is_empty()
                && state.score.hits == 0)
    );
    let snapshot = viewer.take_latest().unwrap();
    assert!(snapshot.cancelled);
    assert!(snapshot.completed_end.is_none());
}

#[test]
fn cohort_control_expiration_and_failures_preserve_the_real_committed_member_prefix() {
    for case in 0..6 {
        let mut f = Fixture::new(false, 8);
        let mut control = match case {
            0 => Control::new([0, 0, 100, 200, 1_000_000_000]),
            1 => Control::new([0, 0, 100, 200, 199]),
            2 => Control::new([0, 0, 100, 200]),
            3 => Control::new([]),
            4 => Control::new([u64::MAX]),
            _ => Control::new([8, 8]),
        };
        if case == 2 {
            control.readings.push_back(Err(Fault("read fault")));
        }
        if case == 3 {
            control.fail_wait = Some(3);
        }
        let seconds = if case == 3 {
            None
        } else {
            Some(if case == 5 { 0 } else { 1 })
        };
        let result = run(&mut f, seconds, &mut control);
        match case {
            0 | 5 => result.unwrap(),
            2 => assert_eq!(
                result.unwrap_err().downcast_ref::<Fault>().unwrap().0,
                "read fault"
            ),
            3 => assert_eq!(
                result.unwrap_err().downcast_ref::<Fault>().unwrap().0,
                "wait fault"
            ),
            _ => assert!(result.is_err()),
        }
        let committed = case < 4;
        assert_eq!(f.device.step, if committed { 3 } else { 0 });
        assert_eq!(
            control.waits,
            vec![WaitDuration::from_millis(1); if committed { 3 } else { 0 }]
        );
        assert!(f.device.pcm.iter().all(|&sample| sample == 0.0));
        assert_eq!(f.device.mixer.counters().commands_applied, 0);
        assert!(
            !f.group.poisoned(),
            "an outer control failure does not rewrite RuntimeGroup ownership"
        );
        for state in &f.states {
            let capture = state.capture.as_ref().unwrap();
            assert_eq!(
                capture
                    .records()
                    .iter()
                    .map(|record| record.song_time.as_nanos())
                    .collect::<Vec<_>>(),
                if committed {
                    vec![0, 20_000_000, 20_000_000]
                } else {
                    Vec::new()
                }
            );
            assert_eq!(state.score.hits, u64::from(committed));
            assert!(state.completion.is_none());
            let file = beatkernel::replay::codec::ReplayFile::new(
                capture.header().clone(),
                capture.records().to_vec(),
            );
            let restored = crate::replay_playback::reconstruct(&f.source, file, limits()).unwrap();
            assert_eq!(
                restored.engine().stable_hash().unwrap(),
                f.group
                    .member_judge(state.player)
                    .unwrap()
                    .stable_hash()
                    .unwrap()
            );
        }
        if committed {
            let mut pcm = [0.0; 2];
            f.device.mixer.render(&mut pcm).unwrap();
            assert_eq!(
                pcm,
                [0.5, 1.0],
                "accepted shared-queue heads survive timeout or wait/read failure"
            );
        }
    }
}
