//! Deferred same-stream identity reconstruction via the actual business port.
use super::*;
use beatkernel::time::{Duration, presentation::EstimatorError};
use beatkernel::transport::Rate;
use beatkernel_platform::audio::presentation::discipline::PresentationDiscipline;
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn config() -> DisciplineConfig {
    DisciplineConfig {
        capacity: 8,
        min_span: Duration::from_nanos(500_000_000),
        correction_horizon: Duration::from_nanos(9_000_000_000),
        ..Default::default()
    }
}
fn evidence(output: i64, host: i64) -> ClockPair {
    ClockPair {
        source: point(2, output),
        target: point(1, host),
    }
}
#[test]
fn actual_core_resume_preserves_configuration_and_max_epoch_but_requires_fresh_warmup_and_origins()
{
    for epoch in [7, u64::MAX] {
        let mut old =
            PresentationEstimator::new(config(), point(2, 0), ClockDomainId(1), Timestamp::ZERO)
                .unwrap();
        old.rebind_output(epoch, point(2, 0), point(2, 0), Timestamp::ZERO)
            .unwrap();
        old.observe_clock_pair_in_epoch(epoch, evidence(0, 0))
            .unwrap();
        old.observe_clock_pair_in_epoch(epoch, evidence(1_000_000_000, 1_000_000_000))
            .unwrap();
        let old_pair = old.latest_pair();
        let old_len = old.retained_len();
        let song = Timestamp::from_nanos(604_800_000_000_000);
        let mut fresh = GameplayPresentationPort::restart_for_resume(
            &old,
            point(2, -1_000_000_000),
            point(2, 2_000_000_000),
            ClockDomainId(1),
            song,
        )
        .unwrap();
        assert_eq!(GameplayPresentationPort::config(&fresh), config());
        assert_eq!(fresh.epoch(), epoch);
        assert_eq!(fresh.retained_len(), 0);
        assert_eq!(fresh.latest_pair(), None);
        assert_eq!(
            (old.latest_pair(), old.retained_len(), old.epoch()),
            (old_pair, old_len, epoch)
        );
        assert_eq!(
            fresh.observe_clock_pair_in_epoch(0, evidence(9, 9)),
            Err(EstimatorError::EpochMismatch)
        );
        fresh
            .observe_clock_pair_in_epoch(epoch, evidence(2_000_000_000, 0))
            .unwrap();
        let mut transport = Transport::new(Timestamp::ZERO, song, Rate::NORMAL);
        let original = transport.clone();
        assert_eq!(
            fresh.update(point(1, 0), &mut transport).unwrap(),
            DisciplineUpdate::Warmup { span_ns: 0 }
        );
        assert_eq!(transport, original);
        fresh
            .observe_clock_pair_in_epoch(epoch, evidence(3_000_000_000, 1_000_000_000))
            .unwrap();
        assert_eq!(
            fresh
                .update(point(1, 1_000_000_000), &mut transport)
                .unwrap(),
            DisciplineUpdate::Applied {
                base_rate_ppm: 0,
                correction_ppm: 0,
                applied_rate_ppm: 0,
                phase_error_ns: 0,
                limited: false
            }
        );
        assert_eq!(
            transport
                .position_at(Timestamp::from_nanos(2_000_000_000))
                .unwrap(),
            Timestamp::from_nanos(604_802_000_000_000)
        );
    }
}
#[test]
fn actual_native_adapter_resume_preserves_settings_and_identity_and_refused_origins_leave_old_evidence()
 {
    for epoch in [11, u64::MAX] {
        let mut old =
            PresentationDiscipline::new(config(), point(2, 0), ClockDomainId(1), Timestamp::ZERO)
                .unwrap();
        old.rebind_output(epoch, point(2, 0), point(2, 0), Timestamp::ZERO)
            .unwrap();
        old.observe_clock_pair_in_epoch(epoch, evidence(0, 0))
            .unwrap();
        let before = old.latest_pair();
        assert!(
            GameplayPresentationPort::restart_for_resume(
                &old,
                point(2, 2),
                point(3, 3),
                ClockDomainId(1),
                Timestamp::ZERO
            )
            .is_err()
        );
        assert_eq!(old.latest_pair(), before);
        assert_eq!(old.epoch(), epoch);
        let mut fresh = GameplayPresentationPort::restart_for_resume(
            &old,
            point(2, 0),
            point(2, 40),
            ClockDomainId(1),
            Timestamp::from_nanos(72_000_000_000_000),
        )
        .unwrap();
        assert_eq!(GameplayPresentationPort::config(&fresh), config());
        assert_eq!(fresh.epoch(), epoch);
        assert_eq!(fresh.retained_len(), 0);
        assert!(
            fresh
                .observe_clock_pair_in_epoch(0, evidence(1, 1))
                .is_err()
        );
        fresh
            .observe_clock_pair_in_epoch(epoch, evidence(40, 40))
            .unwrap();
        assert_eq!(fresh.latest_pair(), Some(evidence(40, 40)));
        assert_eq!(old.latest_pair(), before);
    }
}
// The default constructor still supports legacy untagged ports. Tagged ports
// cannot silently discard their identity when the cold owner cannot restore it.
struct Custom<const TAGGED: bool, const CLAIMS: bool = false>(PresentationEstimator, u64);
impl<const TAGGED: bool, const CLAIMS: bool> GameplayPresentationPort for Custom<TAGGED, CLAIMS> {
    fn new_with_playback_origin(
        c: DisciplineConfig,
        o: ClockPoint,
        p: ClockPoint,
        h: ClockDomainId,
        s: Timestamp,
    ) -> NativeGameplayResult<Self> {
        Ok(Self(
            PresentationEstimator::new_with_playback_origin(c, o, p, h, s)?,
            0,
        ))
    }
    fn epoch(&self) -> Option<u64> {
        TAGGED.then_some(self.1)
    }
    fn rebind_output(
        &mut self,
        _epoch: u64,
        _o: ClockPoint,
        _p: ClockPoint,
        _s: Timestamp,
    ) -> NativeGameplayResult<()> {
        if CLAIMS {
            Ok(())
        } else {
            Err("custom identity restoration unsupported".into())
        }
    }
    fn latest_pair(&self) -> Option<ClockPair> {
        self.0.latest_pair()
    }
    fn quality(&self) -> ClockMappingQuality {
        self.0.quality()
    }
    fn validate_host(&self, p: ClockPoint) -> NativeGameplayResult<()> {
        Ok(self.0.validate_host(p)?)
    }
    fn update(
        &mut self,
        p: ClockPoint,
        t: &mut Transport,
    ) -> NativeGameplayResult<DisciplineUpdate> {
        Ok(self.0.update(p, t)?)
    }
}
#[test]
fn legacy_defaults_construct_fresh_owner_while_tagged_unsupported_restoration_refuses_atomically() {
    let mut legacy = Custom::<false>::new_with_playback_origin(
        config(),
        point(2, 0),
        point(2, 0),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap();
    legacy.0.observe_clock_pair(evidence(0, 0)).unwrap();
    let fresh = legacy
        .restart_for_resume(point(2, 0), point(2, 1), ClockDomainId(1), Timestamp::ZERO)
        .unwrap();
    assert_eq!(fresh.epoch(), None);
    assert_eq!(fresh.0.config(), DisciplineConfig::default());
    assert_eq!(fresh.latest_pair(), None);
    assert_eq!(legacy.latest_pair(), Some(evidence(0, 0)));
    let mut tagged = Custom::<true>::new_with_playback_origin(
        config(),
        point(2, 0),
        point(2, 0),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap();
    tagged.1 = 7;
    tagged.0.observe_clock_pair(evidence(0, 0)).unwrap();
    assert!(
        tagged
            .restart_for_resume(point(2, 0), point(2, 1), ClockDomainId(1), Timestamp::ZERO)
            .is_err()
    );
    assert_eq!(tagged.epoch(), Some(7));
    assert_eq!(tagged.latest_pair(), Some(evidence(0, 0)));
    assert_eq!(tagged.0.config(), config());
}
#[test]
fn custom_restore_success_without_exact_epoch_is_rejected_and_invalid_core_construction_preserves_old_ring()
 {
    let mut mismatch = Custom::<true, true>::new_with_playback_origin(
        config(),
        point(2, 0),
        point(2, 0),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap();
    mismatch.1 = u64::MAX;
    mismatch.0.observe_clock_pair(evidence(0, 0)).unwrap();
    assert!(
        mismatch
            .restart_for_resume(point(2, 0), point(2, 1), ClockDomainId(1), Timestamp::ZERO)
            .is_err()
    );
    assert_eq!(mismatch.epoch(), Some(u64::MAX));
    assert_eq!(mismatch.latest_pair(), Some(evidence(0, 0)));
    let mut old =
        PresentationEstimator::new(config(), point(2, 0), ClockDomainId(1), Timestamp::ZERO)
            .unwrap();
    old.rebind_output(19, point(2, 0), point(2, 0), Timestamp::ZERO)
        .unwrap();
    old.observe_clock_pair_in_epoch(19, evidence(0, 0)).unwrap();
    for playback in [point(3, 1), point(2, -1)] {
        assert!(
            old.restart_for_resume(point(2, 0), playback, ClockDomainId(1), Timestamp::ZERO)
                .is_err()
        );
        assert_eq!(old.epoch(), 19);
        assert_eq!(old.latest_pair(), Some(evidence(0, 0)));
        assert_eq!(old.retained_len(), 1);
        assert_eq!(old.config(), config());
    }
}
