//! Deferred static presentation DI with no device acquisition or fake output IO.
use super::*;
use beatkernel::time::presentation::{EstimatorError, ObservationAdmission};
use beatkernel::transport::Rate;
use beatkernel_platform::audio::presentation::discipline::{PresentationDiscipline, DisciplineError};
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn rotate<P: GameplayPresentationPort>(owner: &mut P, epoch: u64) -> NativeGameplayResult<()> {
    owner.rebind_output(
        epoch,
        point(3, 100),
        point(3, 101),
        Timestamp::from_nanos(604_800_000_000_000),
    )
}
#[test]
fn actual_core_generic_port_rebinds_epoch_then_requires_fresh_tagged_warmup() {
    let mut owner = <PresentationEstimator as GameplayPresentationPort>::new_with_playback_origin(
        DisciplineConfig::default(),
        point(2, 0),
        point(2, 0),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap();
    owner
        .observe_clock_pair_in_epoch(
            0,
            ClockPair {
                source: point(2, 0),
                target: point(1, 0),
            },
        )
        .unwrap();
    assert_eq!(GameplayPresentationPort::epoch(&owner), Some(0));
    rotate(&mut owner, 1).unwrap();
    assert_eq!(GameplayPresentationPort::epoch(&owner), Some(1));
    assert!(GameplayPresentationPort::latest_pair(&owner).is_none());
    assert_eq!(
        owner.observe_clock_pair_in_epoch(
            0,
            ClockPair {
                source: point(2, 1),
                target: point(1, 1)
            }
        ),
        Err(EstimatorError::EpochMismatch)
    );
    assert_eq!(
        owner
            .observe_clock_pair_in_epoch(
                1,
                ClockPair {
                    source: point(3, 101),
                    target: point(1, 1)
                }
            )
            .unwrap(),
        ObservationAdmission::Retained
    );
    let mut transport = Transport::new(
        Timestamp::ZERO,
        Timestamp::from_nanos(604_800_000_000_000),
        Rate::NORMAL,
    );
    assert_eq!(
        GameplayPresentationPort::update(&mut owner, point(1, 1), &mut transport).unwrap(),
        DisciplineUpdate::Warmup { span_ns: 0 }
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(1))
            .unwrap()
            .as_nanos(),
        604_800_000_000_001
    );
}
#[test]
fn actual_native_generic_bridge_delegates_atomic_refusal_and_tagged_pair_admission() {
    let mut owner = <PresentationDiscipline as GameplayPresentationPort>::new_with_playback_origin(
        DisciplineConfig::default(),
        point(2, 0),
        point(2, 0),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap();
    let accepted = ClockPair {
        source: point(2, 0),
        target: point(1, 0),
    };
    owner.observe_clock_pair_in_epoch(0, accepted).unwrap();
    let error = rotate(&mut owner, 0).unwrap_err();
    assert_eq!(
        error.downcast_ref::<DisciplineError>(),
        Some(&DisciplineError::InvalidEpoch)
    );
    assert_eq!(
        GameplayPresentationPort::latest_pair(&owner),
        Some(accepted)
    );
    rotate(&mut owner, u64::MAX).unwrap();
    assert_eq!(GameplayPresentationPort::epoch(&owner), Some(u64::MAX));
    assert_eq!(
        owner.observe_clock_pair_in_epoch(0, accepted),
        Err(DisciplineError::EpochMismatch)
    );
    assert!(owner.latest_pair().is_none());
    owner
        .observe_clock_pair_in_epoch(
            u64::MAX,
            ClockPair {
                source: point(3, 101),
                target: point(1, 1),
            },
        )
        .unwrap();
    assert!(rotate(&mut owner, 1).is_err());
    assert_eq!(owner.retained_len(), 1);
}
struct Legacy(PresentationEstimator);
impl GameplayPresentationPort for Legacy {
    fn new_with_playback_origin(
        config: DisciplineConfig,
        output: ClockPoint,
        playback: ClockPoint,
        host: ClockDomainId,
        song: Timestamp,
    ) -> NativeGameplayResult<Self> {
        Ok(Self(PresentationEstimator::new_with_playback_origin(
            config, output, playback, host, song,
        )?))
    }
    fn latest_pair(&self) -> Option<ClockPair> {
        self.0.latest_pair()
    }
    fn quality(&self) -> ClockMappingQuality {
        self.0.quality()
    }
    fn validate_host(&self, point: ClockPoint) -> NativeGameplayResult<()> {
        Ok(self.0.validate_host(point)?)
    }
    fn update(
        &mut self,
        point: ClockPoint,
        transport: &mut Transport,
    ) -> NativeGameplayResult<DisciplineUpdate> {
        Ok(self.0.update(point, transport)?)
    }
}
#[test]
fn legacy_custom_port_compiles_without_new_overrides_and_explicitly_refuses_rebinding() {
    let mut owner = Legacy::new_with_playback_origin(
        DisciplineConfig::default(),
        point(2, 0),
        point(2, 0),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap();
    let accepted = ClockPair {
        source: point(2, 0),
        target: point(1, 0),
    };
    owner.0.observe_clock_pair(accepted).unwrap();
    assert_eq!(owner.epoch(), None);
    assert!(
        rotate(&mut owner, 1)
            .unwrap_err()
            .to_string()
            .contains("unsupported")
    );
    assert_eq!(owner.latest_pair(), Some(accepted));
    assert_eq!(owner.0.epoch(), 0);
}
