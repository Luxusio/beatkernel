//! Deferred actual ordered cohort pump with business/core-only presentation.
use crate::gameplay_presentation_port_fixtures::*;
use crate::{
    competition::ScoreSummary,
    local_input::InputMerger,
    local_players::PlayerId,
    local_runtime::{MemberConfig, RuntimeGroup},
    native_cohort::{CohortSession, GameplayPlayerState, run_cohort_with_ports},
};

struct Fixture {
    device: MemoryDevice,
    group: RuntimeGroup,
    states: Vec<GameplayPlayerState<Observer>>,
    merger: InputMerger,
    bgm: BgmFeeder,
    presentation: PresentationEstimator,
    pause: NativePause,
    end: Option<NativeEnd>,
    network: GroupObserver,
    delivery: InputDeliveryTelemetry,
    pre: u64,
}
impl Fixture {
    fn new(finite: bool) -> Self {
        let source = source();
        let (device, producer) = device(finite, vec![DeviceId(1), DeviceId(u64::MAX)]);
        let mut members = Vec::new();
        let mut states = Vec::new();
        for (device, player) in [
            (DeviceId(1), PlayerId(7)),
            (DeviceId(u64::MAX), PlayerId(u32::MAX)),
        ] {
            members.push(MemberConfig {
                player,
                device: Some(device),
                bindings: bindings(Some(device)),
                judge: judge(&source),
                sounds: vec![SoundBinding {
                    object: source.compile().unwrap().chart.objects()[0].id,
                    stage: beatkernel::judge::JudgeStage::Instant,
                    sample: SampleId(1),
                    voice: VoiceId(device.0),
                    gain: 1.0,
                }],
            });
            states.push(GameplayPlayerState {
                player,
                capture: None,
                competition: Some(Observer::default()),
                completion: None,
                score: ScoreSummary::default(),
                gauge: BmsGauge::default(),
                last_song: Timestamp::ZERO,
            });
        }
        let mut group = RuntimeGroup::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            producer,
            members,
            8,
            &[],
        )
        .unwrap();
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        if finite {
            group
                .set_song_end(Timestamp::from_nanos(10_000_000))
                .unwrap();
        }
        let (pause, end) = pause_end(finite);
        Self {
            device,
            group,
            states,
            merger: InputMerger::new(
                ClockDomainId(1),
                point(1, 0),
                vec![DeviceId(1), DeviceId(u64::MAX)],
                16,
            )
            .unwrap(),
            bgm: bgm(),
            presentation: estimator(),
            pause,
            end,
            network: GroupObserver::default(),
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
        run_cohort_with_ports(
            &mut self.device,
            CohortSession {
                group: &mut self.group,
                network: Some(&mut self.network),
                states: &mut self.states,
                merger: &mut self.merger,
                bgm: &mut self.bgm,
                discipline: &mut self.presentation,
                pause: &mut self.pause,
                end: &mut self.end,
                delivery: &mut self.delivery,
                pre_origin_inputs: &mut self.pre,
            },
            config(finite),
            control,
            host,
        )
    }
}

#[test]
fn actual_cohort_pure_presentation_orders_members_and_distinguishes_completion_from_cancel() {
    let mut f = Fixture::new(false);
    let mut host = Host::default();
    let mut control = Control::default();
    f.run(false, &mut host, &mut control).unwrap();
    // Input arrival is reversed; the merger processes source 1 first. Advances
    // publish the whole roster, while each input publishes its actual owner.
    assert_eq!(
        host.local,
        [
            vec![(PlayerId(7), 0, 0), (PlayerId(u32::MAX), 0, 0)],
            vec![(PlayerId(7), 20_000_000, 1)],
            vec![(PlayerId(u32::MAX), 20_000_000, 1)],
            vec![
                (PlayerId(7), 20_000_000, 0),
                (PlayerId(u32::MAX), 20_000_000, 0)
            ],
        ]
    );
    assert!(
        f.network
            .rows
            .iter()
            .all(|row| row == &[PlayerId(7), PlayerId(u32::MAX)])
    );
    assert_eq!(f.network.rows.len(), 4);
    assert_eq!(f.network.marks, 0);
    assert!(
        f.states
            .iter()
            .all(|s| s.competition.as_ref().unwrap().marks == 0)
    );
    assert_eq!(
        f.presentation.latest_pair(),
        Some(pair(40_000_000, 40_000_000))
    );
    // Backlogged input is admitted after the third block has rendered.
    let mut expected = vec![0.0; 40];
    expected[30] = 0.5;
    expected[31] = 1.0;
    assert_eq!(f.device.pcm, expected);
    assert_eq!(control.waits, 3);
    for cancel in [false, true] {
        let mut f = Fixture::new(true);
        let mut host = Host {
            cancel,
            ..Default::default()
        };
        f.run(true, &mut host, &mut Control::default()).unwrap();
        assert_eq!(f.network.marks, usize::from(!cancel));
        assert!(
            f.states
                .iter()
                .all(|s| s.competition.as_ref().unwrap().marks == usize::from(!cancel))
        );
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
fn actual_cohort_refuses_bad_or_stale_pairs_without_member_or_shared_completion() {
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
        assert!(host.local.is_empty());
        assert!(host.ends.is_empty());
        assert!(f.network.rows.is_empty());
        assert_eq!(f.network.marks, 0);
        assert!(
            f.states
                .iter()
                .all(|s| s.competition.as_ref().unwrap().times.is_empty()
                    && s.competition.as_ref().unwrap().marks == 0)
        );
        assert_eq!(control.waits, 0);
    }
}
