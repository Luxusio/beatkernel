// Deferred real solo pump with virtual control time and disabled Runtime profiling.
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
    f.runtime
        .set_processing_clock(RuntimeProcessingClock::Disabled);
    run_gameplay_with_control(
        &mut f.device,
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
            seconds,
            pause_supported: false,
            logical_schedule: true,
        },
        control,
    )
}

#[test]
fn virtual_control_repeated_solo_runs_preserve_actual_pcm_capture_and_native_timestamps() {
    let mut previous = None;
    for timed in [false, true, false] {
        let mut f = Fixture::new(false, false);
        let mut control = if timed {
            Control::new([700, 700, 900, 950, 999])
        } else {
            Control::new([])
        };
        run(&mut f, timed.then_some(1), &mut control).unwrap();
        assert_eq!(
            f.device.step, 4,
            "the genuine device closure ends this run, not completion"
        );
        assert!(f.completion.is_none());
        assert_eq!(control.reads, if timed { 5 } else { 0 });
        assert_eq!(control.waits, [WaitDuration::from_millis(1); 3]);
        let mut expected = vec![0.0; 40];
        expected[20] = 0.25;
        expected[21] = 0.5;
        assert_eq!(f.device.pcm, expected);
        assert_eq!(f.device.mixer.counters().commands_applied, 1);
        assert_eq!(f.delivery.observed_events(), 1);
        let capture = f.capture.take().unwrap();
        assert_eq!(
            capture
                .records()
                .iter()
                .map(|record| record.song_time.as_nanos())
                .collect::<Vec<_>>(),
            [0, 20_000_000, 20_000_000]
        );
        let ReplayOperation::Input(input) = &capture.records()[1].operation else {
            panic!("actual input record");
        };
        assert_eq!(input.physical.meta().clock_domain, ClockDomainId(1));
        assert_eq!(
            input.physical.meta().timestamp,
            Timestamp::from_nanos(20_000_000)
        );
        let hash = f.runtime.judge().stable_hash().unwrap();
        let bytes = capture.into_bytes().unwrap();
        if let Some((old_hash, old_bytes)) = &previous {
            assert_eq!(*old_hash, hash);
            assert_eq!(old_bytes, &bytes);
        }
        previous = Some((hash, bytes));
    }
    let (publisher, viewer) = player::channel();
    viewer.cancel();
    let mut f = Fixture::new(false, false);
    let hash = f.runtime.judge().stable_hash().unwrap();
    let mut control = Control::new([]);
    player::with_publisher(publisher, || {
        run(&mut f, None, &mut control).map_err(|error| error.to_string())
    })
    .unwrap();
    assert_eq!(
        (f.device.step, control.reads, control.waits.len()),
        (0, 0, 0)
    );
    assert_eq!(f.runtime.judge().stable_hash().unwrap(), hash);
    assert!(f.capture.as_ref().unwrap().records().is_empty());
    let snapshot = viewer.take_latest().unwrap();
    assert!(snapshot.cancelled);
    assert!(snapshot.completed_end.is_none());
}

#[test]
fn solo_deadline_and_injected_faults_preserve_only_the_actual_committed_prefix() {
    for case in 0..6 {
        let mut f = Fixture::new(false, false);
        let mut control = match case {
            0 => Control::new([0, 0, 1_000_000_000]), // Exact deadline after one iteration.
            1 => Control::new([0, 0, 500, 499]),      // Regression before third device observation.
            2 => Control::new([0, 0, 500]),
            3 => Control::new([]),
            4 => Control::new([u64::MAX]),
            _ => Control::new([12, 12]), // Zero seconds never enters the body.
        };
        if case == 2 {
            control.readings.push_back(Err(Fault("read fault")));
        }
        if case == 3 {
            control.fail_wait = Some(2);
        }
        let seconds = if case == 3 {
            None
        } else {
            Some(if case == 5 { 0 } else { 1 })
        };
        let before = f.runtime.judge().stable_hash().unwrap();
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
        let steps = match case {
            0 => 1,
            1..=3 => 2,
            _ => 0,
        };
        assert_eq!(f.device.step, steps);
        assert_eq!(
            control.waits,
            vec![WaitDuration::from_millis(1); steps as usize]
        );
        let times = f
            .capture
            .as_ref()
            .unwrap()
            .records()
            .iter()
            .map(|record| record.song_time.as_nanos())
            .collect::<Vec<_>>();
        assert_eq!(
            times,
            match steps {
                0 => vec![],
                1 => vec![0],
                _ => vec![0, 20_000_000],
            }
        );
        assert!(f.device.pcm.iter().all(|&sample| sample == 0.0));
        assert_eq!(f.device.mixer.counters().commands_applied, 0);
        assert!(f.completion.is_none());
        if steps == 0 {
            assert_eq!(f.runtime.judge().stable_hash().unwrap(), before);
        }
        if steps == 2 {
            let mut pcm = [0.0; 2];
            f.device.mixer.render(&mut pcm).unwrap();
            assert_eq!(
                pcm,
                [0.25, 0.5],
                "a control failure cannot erase the accepted audio prefix"
            );
        }
    }
}
