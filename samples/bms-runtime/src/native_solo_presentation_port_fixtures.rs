//! Deferred real solo pump using only business contracts and core evidence.
use crate::gameplay_presentation_port_fixtures::*;
use crate::{
    local_runtime::SoloRuntime,
    native_gameplay::{GameplaySession, run_gameplay_with_ports},
};

struct Fixture {
    device: MemoryDevice,
    runtime: SoloRuntime,
    gauge: BmsGauge,
    bgm: BgmFeeder,
    presentation: PresentationEstimator,
    pause: NativePause,
    end: Option<NativeEnd>,
    completion: Option<crate::completion::SongCompletion>,
    capture: Option<crate::replay_capture::LiveReplayCapture>,
    observer: Option<Observer>,
    delivery: InputDeliveryTelemetry,
    pre: u64,
}
impl Fixture {
    fn new(finite: bool) -> Self {
        let source = source();
        let (device, producer) = device(finite, vec![DeviceId(u64::MAX)]);
        let mut runtime = SoloRuntime::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            bindings(None),
            judge(&source),
            producer,
            vec![SoundBinding {
                object: source.compile().unwrap().chart.objects()[0].id,
                stage: beatkernel::judge::JudgeStage::Instant,
                sample: SampleId(1),
                voice: VoiceId(1),
                gain: 1.0,
            }],
            8,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        if finite {
            runtime
                .set_song_end(Timestamp::from_nanos(10_000_000))
                .unwrap();
        }
        let (pause, end) = pause_end(finite);
        Self {
            device,
            runtime,
            gauge: BmsGauge::default(),
            bgm: bgm(),
            presentation: estimator(),
            pause,
            end,
            completion: None,
            capture: None,
            observer: Some(Observer::default()),
            delivery: InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap(),
            pre: 0,
        }
    }
    fn run(
        &mut self,
        finite: bool,
        host: &mut Host,
        control: &mut Control,
    ) -> NativeGameplayResult<()> {
        let config = NativeGameplayConfig {
            pause_supported: self.device.pause_command.is_some(),
            ..config(finite)
        };
        run_gameplay_with_ports(
            &mut self.device,
            GameplaySession {
                runtime: &mut self.runtime,
                gauge: &mut self.gauge,
                bgm: &mut self.bgm,
                discipline: &mut self.presentation,
                pause: &mut self.pause,
                end: &mut self.end,
                completion: &mut self.completion,
                capture: &mut self.capture,
                competition: &mut self.observer,
                delivery: &mut self.delivery,
                pre_origin_inputs: &mut self.pre,
            },
            config,
            control,
            host,
        )
    }
}

#[test]
fn actual_solo_pump_uses_pure_pairs_original_input_and_only_marks_genuine_completion() {
    let mut f = Fixture::new(false);
    let mut host = Host::default();
    let mut control = Control::default();
    f.run(false, &mut host, &mut control).unwrap();
    assert_eq!(
        host.solo
            .iter()
            .map(|r| r.song_time.as_nanos())
            .collect::<Vec<_>>(),
        [0, 20_000_000, 20_000_000]
    );
    let input = host.solo[1].input.as_ref().unwrap().meta();
    assert_eq!(input.source, DeviceId(u64::MAX));
    assert_eq!(input.sequence, u64::MAX - 1);
    assert_eq!(input.timestamp, Timestamp::from_nanos(20_000_000));
    assert_eq!(host.solo[1].judge_events.len(), 1);
    assert_eq!(
        f.observer.as_ref().unwrap().times,
        [0, 20_000_000, 20_000_000]
    );
    assert_eq!(f.observer.as_ref().unwrap().marks, 0);
    assert_eq!(
        f.presentation.latest_pair(),
        Some(pair(40_000_000, 40_000_000))
    );
    let mut expected = vec![0.0; 40];
    expected[20] = 0.25;
    expected[21] = 0.5;
    assert_eq!(f.device.pcm, expected);
    assert_eq!(control.waits, 3);
    for cancel in [false, true] {
        let mut f = Fixture::new(true);
        let mut host = Host {
            cancel,
            ..Default::default()
        };
        f.run(true, &mut host, &mut Control::default()).unwrap();
        assert_eq!(f.observer.as_ref().unwrap().marks, usize::from(!cancel));
        assert_eq!(f.device.step, if cancel { 0 } else { 3 });
        assert_eq!(
            host.ends,
            if cancel {
                vec![]
            } else {
                vec![Timestamp::from_nanos(10_000_000)]
            }
        );
    }
}

#[test]
fn actual_solo_pump_rejects_bad_or_stale_evidence_before_publication_or_completion() {
    for stale in [false, true] {
        let mut f = Fixture::new(false);
        f.device.stale = stale;
        f.device.reject_at = (!stale).then_some(1);
        let mut host = Host::default();
        let mut control = Control::default();
        let error = f.run(false, &mut host, &mut control).unwrap_err();
        assert_eq!(
            error.downcast_ref::<EstimatorError>(),
            Some(&if stale {
                EstimatorError::Stale
            } else {
                EstimatorError::NonIncreasing
            })
        );
        assert_eq!(f.presentation.latest_pair(), Some(pair(0, 0)));
        assert!(host.solo.is_empty());
        assert!(host.ends.is_empty());
        assert!(f.observer.as_ref().unwrap().times.is_empty());
        assert_eq!(f.observer.as_ref().unwrap().marks, 0);
        assert_eq!(control.waits, 0);
    }
}

#[test]
fn actual_solo_pause_resume_reconstructs_pure_port_and_reconciles_original_release_before_backlog()
{
    let mut f = Fixture::new(false);
    let command = std::rc::Rc::new(std::cell::Cell::new(false));
    f.device.pause_command = Some(command.clone());
    let mut host = Host {
        pause_command: Some(command),
        ..Default::default()
    };
    f.run(false, &mut host, &mut Control::default()).unwrap();
    assert_eq!(f.device.seeds, [pair(40_000_000, 40_000_000)]);
    assert_eq!(f.pause.phase(), crate::playback_pause::PausePhase::Running);
    assert_eq!(f.delivery.observed_events(), 4);
    let releases = host
        .solo
        .iter()
        .filter_map(|r| {
            if let Some(PhysicalInputEvent::Button(event)) = &r.input {
                (event.state == ButtonState::Up)
                    .then_some((event.meta.timestamp, event.meta.original_clock_point))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        releases,
        [
            (
                Timestamp::from_nanos(40_000_000),
                Some(point(1, 20_000_000))
            ),
            (Timestamp::from_nanos(45_000_000), None),
        ]
    );
    assert!(host.pause_states.contains(&PauseState::Pausing));
    assert!(host.pause_states.contains(&PauseState::Resuming));
    assert_eq!(f.observer.as_ref().unwrap().marks, 0);
}

mod resume_clock {
    include!("native_solo_resume_clock_fixtures.rs");
}
