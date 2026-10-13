//! Mathematical acquired-HOST preparation only: no native or gameplay permission.
use super::*;
use beatkernel::audio::{OutputFrameBasis, TargetFrameBasis, TargetTime};
use beatkernel::time::{ClockMapper, ClockMappingQuality};
use beatkernel::input::{
    ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent,
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
fn config() -> AudioAuthorityConfig {
    AudioAuthorityConfig {
        history_capacity: 8,
        max_observation_age: Duration::from_nanos(i64::MAX),
        ..AudioAuthorityConfig::default()
    }
}
fn source(frequency: u64) -> AudioAuthorityCoordinateBasis {
    AudioAuthorityCoordinateBasis::SourceCounter {
        basis: OutputFrameBasis::new(point(2, 0), 3, 0).unwrap(),
        frequency,
    }
}
fn coordinate(basis: AudioAuthorityCoordinateBasis, units: u64) -> NativeOutputCoordinate {
    match basis {
        AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. } => {
            NativeOutputCoordinate::Counter {
                position: units,
                frequency,
            }
        }
        _ => NativeOutputCoordinate::StreamFrame { frame: units },
    }
}
fn owner(cfg: AudioAuthorityConfig, basis: AudioAuthorityCoordinateBasis) -> AudioAuthority {
    AudioAuthority::new_with_coordinates(
        cfg,
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: basis.origin().unwrap(),
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
        basis,
    )
    .unwrap()
}
fn observe(owner: &mut AudioAuthority, units: u64, at: i64) {
    let basis = owner.coordinate_basis().unwrap();
    let coordinate = coordinate(basis, units);
    let pair = ClockPair {
        source: basis.point(coordinate).unwrap(),
        target: host(at),
    };
    assert_eq!(
        owner.observe_coordinate(7, pair, coordinate),
        Ok(ObservationAdmission::Retained)
    );
}
fn seeded(
    cfg: AudioAuthorityConfig,
    basis: AudioAuthorityCoordinateBasis,
    left: (u64, i64),
    right: (u64, i64),
    prefix: i64,
) -> AudioAuthority {
    let mut owner = owner(cfg, basis);
    observe(&mut owner, left.0, left.1);
    observe(&mut owner, right.0, right.1);
    owner.record_acquired_prefix(host(prefix)).unwrap();
    owner
}
fn prepare(owner: &AudioAuthority, original: i64, now: i64) -> PreparedCoordinateInput {
    owner
        .prepare_coordinate_input(host(original), host(now))
        .unwrap()
        .unwrap()
}
fn assert_output(owner: &AudioAuthority, original: i64, now: i64, raw: i64, logical: i64) {
    let state = owner.state();
    let entries = owner.history.clone();
    let token = prepare(owner, original, now);
    assert_eq!(token.original(), host(original));
    assert_eq!(token.epoch(), 7);
    assert_eq!(token.raw_output(), point(2, raw));
    assert_eq!(token.output(), point(3, logical));
    let origin = match owner.coordinate_basis().unwrap() {
        AudioAuthorityCoordinateBasis::SourceFrame(basis)
        | AudioAuthorityCoordinateBasis::SourceCounter { basis, .. } => basis.origin(),
        AudioAuthorityCoordinateBasis::TargetFrame(basis) => basis.origin(),
    };
    assert_eq!(token.query().point(origin).unwrap(), token.raw_output());
    assert_eq!(
        token.mapper().map(host(original), ClockDomainId(3)),
        Some(Timestamp::from_nanos(logical))
    );
    assert_eq!(token.mapper().quality(), ClockMappingQuality::Unknown);
    assert_eq!(owner.validate_coordinate_input(&token, host(now)), Ok(()));
    assert_eq!(owner.state(), state);
    assert_eq!(owner.history, entries);
}

#[test]
fn counter_interpolation_preserves_literal_half_second_before_flooring() {
    let owner = seeded(config(), source(3), (1, 0), (2, 2), 2);
    assert_output(&owner, 1, 2, 500_000_000, 500_000_500);
    // Interpolating independently rounded endpoints would give 499999999.
    assert_ne!(
        prepare(&owner, 1, 2).raw_output().timestamp.as_nanos(),
        499_999_999
    );
    assert_output(&owner, 0, 2, 333_333_333, 333_333_833);
    assert_output(&owner, 2, 2, 666_666_666, 666_667_166);
}

#[test]
fn all_three_descriptors_add_nonzero_fractional_origin_once() {
    let source_basis = OutputFrameBasis::new(point(2, 100), 3, 1).unwrap();
    let target =
        TargetFrameBasis::new(point(2, 100), TargetTime::new(0, 1, 3).unwrap(), 3).unwrap();
    for basis in [
        AudioAuthorityCoordinateBasis::SourceFrame(source_basis),
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis: source_basis,
            frequency: 3,
        },
        AudioAuthorityCoordinateBasis::TargetFrame(target),
    ] {
        let owner = seeded(config(), basis, (1, 0), (2, 2), 2);
        assert_output(&owner, 1, 2, 833_333_433, 500_000_500);
        assert_output(&owner, 0, 2, 666_666_766, 333_333_833);
        assert_output(&owner, 2, 2, 1_000_000_100, 666_667_167);
    }
}

#[test]
fn flat_nanoseconds_still_support_exact_fractional_interpolation() {
    let basis = OutputFrameBasis::new(point(2, 100), 2_000_000_000, 0).unwrap();
    let target = TargetFrameBasis::new(
        point(2, 100),
        TargetTime::from_frames(0, 2_000_000_000).unwrap(),
        2_000_000_000,
    )
    .unwrap();
    for descriptor in [
        AudioAuthorityCoordinateBasis::SourceFrame(basis),
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis,
            frequency: 2_000_000_000,
        },
        AudioAuthorityCoordinateBasis::TargetFrame(target),
    ] {
        let owner = seeded(config(), descriptor, (0, 100), (1, 102), 102);
        assert_output(&owner, 101, 102, 100, 500);
        assert_eq!(
            prepare(&owner, 101, 102)
                .query()
                .compare_native_time_with_offset(
                    TargetTime::from_frames(0, 1).unwrap(),
                    0,
                    2_000_000_000,
                    0
                )
                .unwrap(),
            std::cmp::Ordering::Greater
        );
    }
}

#[test]
fn signed_before_origin_fraction_floors_to_negative_one() {
    let cfg = AudioAuthorityConfig {
        input_extrapolation: ExtrapolationPolicy::Bounded {
            before: Duration::from_nanos(1),
            after: Duration::ZERO,
        },
        ..config()
    };
    let owner = seeded(cfg, source(2_000_000_000), (0, 100), (1, 102), 102);
    assert_output(&owner, 99, 102, -1, 499);
    assert!(prepare(&owner, 99, 102).is_before_origin());
    assert_eq!(
        owner
            .prepare_coordinate_input(host(98), host(102))
            .unwrap_err(),
        AudioAuthorityError::HistoryExpired
    );
    assert_eq!(
        prepare(&owner, 99, 102)
            .mapper()
            .map(host(98), ClockDomainId(3)),
        None
    );
}

#[test]
fn exact_ahead_equality_is_allowed_and_same_floor_fraction_is_refused() {
    for ahead in [0, 1] {
        let cfg = AudioAuthorityConfig {
            input_extrapolation: ExtrapolationPolicy::Bounded {
                before: Duration::ZERO,
                after: Duration::from_nanos(8),
            },
            max_input_ahead: Duration::from_nanos(ahead),
            ..config()
        };
        let owner = seeded(cfg, source(2_000_000_000), (0, 100), (1, 102), 110);
        let boundary = 102 + ahead * 4;
        let token = prepare(&owner, boundary, 110);
        assert_eq!(token.raw_output(), point(2, ahead));
        assert!(owner
            .prepare_coordinate_input(host(boundary + 1), host(110))
            .unwrap()
            .is_none());
        assert_eq!(
            token.mapper().map(host(boundary), ClockDomainId(3)),
            Some(Timestamp::from_nanos(500 + ahead))
        );
        assert_eq!(
            token.mapper().map(host(boundary + 1), ClockDomainId(3)),
            None
        );
    }
}

#[test]
fn mapper_has_only_frozen_host_to_logical_relation() {
    let owner = seeded(config(), source(3), (1, 0), (2, 2), 2);
    let token = prepare(&owner, 1, 2);
    for (from, to) in [
        (point(4, 1), ClockDomainId(3)),
        (host(1), ClockDomainId(2)),
        (point(3, 500_000_500), ClockDomainId(1)),
        (host(1), ClockDomainId(1)),
        (host(-1), ClockDomainId(3)),
        (host(3), ClockDomainId(3)),
    ] {
        assert_eq!(token.mapper().map(from, to), None);
    }
}

#[test]
fn future_right_association_keeps_fresh_already_due_evidence_usable() {
    let cfg = AudioAuthorityConfig {
        max_observation_age: Duration::from_nanos(100),
        ..config()
    };
    let mut owner = seeded(cfg, source(1_000_000_000), (100, 100), (200, 200), 150);
    assert_output(&owner, 125, 150, 125, 625);
    owner.record_acquired_prefix(host(200)).unwrap();
    assert!(owner
        .prepare_coordinate_input(host(125), host(150))
        .unwrap()
        .is_none());
    let future = seeded(cfg, source(1_000_000_000), (100, 200), (200, 300), 150);
    assert!(future
        .prepare_coordinate_input(host(125), host(150))
        .unwrap()
        .is_none());
    let stale = seeded(cfg, source(1_000_000_000), (100, 100), (200, 5000), 201);
    assert!(stale
        .prepare_coordinate_input(host(125), host(201))
        .unwrap()
        .is_none());
}

#[test]
fn acquisition_domain_age_and_order_restrictions_preserve_owner() {
    let basis = source(1_000_000_000);
    let mut owner = owner(config(), basis);
    observe(&mut owner, 100, 100);
    assert!(owner
        .prepare_coordinate_input(host(100), host(100))
        .unwrap()
        .is_none());
    observe(&mut owner, 200, 200);
    assert!(owner
        .prepare_coordinate_input(host(125), host(200))
        .unwrap()
        .is_none());
    owner.record_acquired_prefix(host(120)).unwrap();
    assert!(owner
        .prepare_coordinate_input(host(125), host(200))
        .unwrap()
        .is_none());
    owner.record_acquired_prefix(host(200)).unwrap();
    let state = owner.state();
    for (original, now) in [(point(4, 125), host(200)), (host(125), point(4, 200))] {
        assert_eq!(
            owner.prepare_coordinate_input(original, now).unwrap_err(),
            AudioAuthorityError::DomainMismatch
        );
    }
    assert!(owner
        .prepare_coordinate_input(host(201), host(200))
        .unwrap()
        .is_none());
    assert_eq!(
        owner
            .prepare_coordinate_input(host(99), host(200))
            .unwrap_err(),
        AudioAuthorityError::HistoryExpired
    );
    assert!(owner
        .prepare_coordinate_input(host(201), host(201))
        .unwrap()
        .is_none());
    assert_eq!(owner.state(), state);
    // Synthetic accepted restrictions only; these fields grant no native evidence.
    owner.closed = Some(host(125));
    assert_eq!(
        owner
            .prepare_coordinate_input(host(125), host(200))
            .unwrap_err(),
        AudioAuthorityError::OperationRegression
    );
    owner.closed = None;
    owner.input_host = Some(host(126));
    assert_eq!(
        owner
            .prepare_coordinate_input(host(125), host(200))
            .unwrap_err(),
        AudioAuthorityError::OperationRegression
    );
    owner.input_host = None;
    let exact_operation = prepare(&owner, 126, 200).query();
    owner.operation = Some(point(3, 626));
    owner.coordinate_operation = Some(exact_operation);
    assert_eq!(
        owner
            .prepare_coordinate_input(host(125), host(200))
            .unwrap_err(),
        AudioAuthorityError::OperationRegression
    );
    owner.operation = None;
    owner.coordinate_operation = None;
    owner.revision = u64::MAX;
    assert_eq!(
        owner
            .prepare_coordinate_input(host(125), host(200))
            .unwrap_err(),
        AudioAuthorityError::Overflow
    );
}

#[test]
fn validation_accepts_later_now_but_rechecks_age_and_domains() {
    let cfg = AudioAuthorityConfig {
        max_observation_age: Duration::from_nanos(10),
        ..config()
    };
    let owner = seeded(cfg, source(1_000_000_000), (100, 100), (102, 102), 102);
    let token = prepare(&owner, 101, 102);
    assert_eq!(owner.validate_coordinate_input(&token, host(112)), Ok(()));
    assert_eq!(
        owner.validate_coordinate_input(&token, host(113)),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert!(owner.validate_coordinate_input(&token, host(101)).is_err());
    assert!(owner
        .validate_coordinate_input(&token, point(4, 102))
        .is_err());
}

#[test]
fn observation_prefix_restart_and_revision_changes_stale_tokens() {
    for mutation in 0..4 {
        let mut owner = seeded(config(), source(1_000_000_000), (100, 100), (102, 102), 102);
        let token = prepare(&owner, 101, 102);
        match mutation {
            0 => observe(&mut owner, 103, 103),
            1 => owner.record_acquired_prefix(host(103)).unwrap(),
            2 => {
                let merger =
                    InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap();
                let restart = owner.prepare_correlation_restart(&merger).unwrap();
                owner.commit_correlation_restart(restart, &merger).unwrap();
            }
            _ => owner.revision += 1,
        }
        let state = owner.state();
        assert_eq!(
            owner.validate_coordinate_input(&token, host(103)),
            Err(AudioAuthorityError::StalePreparation)
        );
        assert_eq!(owner.state(), state);
    }
}

#[test]
fn unchanged_coordinate_preserves_token_and_does_not_refresh_freshness() {
    let cfg = AudioAuthorityConfig {
        max_observation_age: Duration::from_nanos(10),
        ..config()
    };
    let mut owner = seeded(cfg, source(1_000_000_000), (100, 100), (102, 102), 102);
    let token = prepare(&owner, 101, 102);
    let state = owner.state();
    assert_eq!(
        owner.observe_coordinate(
            7,
            ClockPair {
                source: point(2, 102),
                target: host(10_000)
            },
            coordinate(source(1_000_000_000), 102)
        ),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(owner.state(), state);
    assert_eq!(owner.validate_coordinate_input(&token, host(112)), Ok(()));
    assert_eq!(
        owner.validate_coordinate_input(&token, host(113)),
        Err(AudioAuthorityError::StalePreparation)
    );
}

#[test]
fn full_signed_host_interval_and_large_frequency_are_exact() {
    let owner = seeded(
        config(),
        source(u64::MAX),
        (0, i64::MIN),
        (u64::MAX, i64::MAX),
        i64::MAX,
    );
    // At HOST 0, fraction is 2^63/(2^64-1) seconds, just above 1/2.
    assert_output(&owner, 0, i64::MAX, 500_000_000, 500_000_500);
    assert_output(&owner, i64::MIN, i64::MAX, 0, 500);
    assert_output(&owner, i64::MAX, i64::MAX, 1_000_000_000, 1_000_000_500);
}

#[test]
fn denominator_product_larger_than_u64_does_not_overflow() {
    let owner = seeded(config(), source(u64::MAX), (1, 0), (2, 3), 3);
    assert_output(&owner, 1, 3, 0, 500);
    assert_eq!(
        prepare(&owner, 1, 3)
            .query()
            .compare_native_time_with_offset(TargetTime::from_frames(0, 1).unwrap(), 1, u64::MAX, 0)
            .unwrap(),
        std::cmp::Ordering::Greater
    );
}

#[test]
fn unrepresentable_logical_output_refuses_without_mutating_owner() {
    let basis = source(1_000_000_000);
    let cfg = AudioAuthorityConfig {
        input_extrapolation: ExtrapolationPolicy::Bounded {
            before: Duration::ZERO,
            after: Duration::from_nanos(1),
        },
        max_input_ahead: Duration::from_nanos(1),
        ..config()
    };
    let mut owner = AudioAuthority::new_with_coordinates(
        cfg,
        AudioAuthorityEpoch {
            logical_origin: point(3, i64::MAX - 2),
            ..AudioAuthorityEpoch {
                id: 7,
                stream_origin: point(2, 0),
                logical_origin: point(3, 500),
                host_domain: ClockDomainId(1),
            }
        },
        basis,
    )
    .unwrap();
    observe(&mut owner, 0, 100);
    observe(&mut owner, 2, 102);
    owner.record_acquired_prefix(host(103)).unwrap();
    let state = owner.state();
    assert_eq!(
        owner
            .prepare_coordinate_input(host(103), host(103))
            .unwrap_err(),
        AudioAuthorityError::Overflow
    );
    let token = prepare(&owner, 102, 103);
    assert_eq!(token.output(), point(3, i64::MAX));
    assert_eq!(token.mapper().map(host(103), ClockDomainId(3)), None);
    assert_eq!(owner.state(), state);
}

#[test]
fn preparation_and_validation_preserve_original_event_and_merger_payload() {
    let owner = seeded(config(), source(3), (1, 0), (2, 2), 2);
    let mut merger = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap();
    let event = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(1), 42),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    });
    merger.admit(event.clone(), host(2)).unwrap();
    let before = merger.peek_ready(host(2)).unwrap();
    let state = owner.state();
    let token = prepare(&owner, 1, 2);
    assert_eq!(owner.validate_coordinate_input(&token, host(2)), Ok(()));
    assert_eq!(merger.peek_ready(host(2)).unwrap(), before);
    assert_eq!(merger.pending(), 1);
    assert_eq!(
        token.original(),
        ClockPoint {
            domain: event.meta().clock_domain,
            timestamp: event.meta().timestamp
        }
    );
    assert_eq!(owner.state(), state);
}

#[test]
fn scalar_owner_cannot_prepare_coordinate_input() {
    let owner = AudioAuthority::new(
        config(),
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: point(2, 0),
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
    )
    .unwrap();
    assert_eq!(
        owner
            .prepare_coordinate_input(host(0), host(0))
            .unwrap_err(),
        AudioAuthorityError::CoordinateModeMismatch
    );
}

#[test]
fn same_rounded_values_do_not_make_different_original_descriptors_interchangeable() {
    let first = seeded(config(), source(u64::MAX), (1, 0), (2, 2), 2);
    let second = seeded(config(), source(u64::MAX - 1), (1, 0), (2, 2), 2);
    let token = prepare(&first, 1, 2);
    assert_eq!(token.raw_output(), prepare(&second, 1, 2).raw_output());
    assert_eq!(token.output(), prepare(&second, 1, 2).output());
    assert_eq!(
        second.validate_coordinate_input(&token, host(2)),
        Err(AudioAuthorityError::StalePreparation)
    );
}

#[test]
fn unrepresentable_frozen_host_validity_refuses_without_mutation() {
    let cfg = AudioAuthorityConfig {
        input_extrapolation: ExtrapolationPolicy::Bounded {
            before: Duration::from_nanos(i64::MAX),
            after: Duration::from_nanos(i64::MAX),
        },
        max_input_ahead: Duration::from_nanos(i64::MAX),
        ..config()
    };
    let owner = seeded(cfg, source(1_000_000_000), (0, -2), (2, 2), 3);
    let state = owner.state();
    assert_eq!(
        owner
            .prepare_coordinate_input(host(3), host(3))
            .unwrap_err(),
        AudioAuthorityError::Overflow
    );
    assert_eq!(
        owner
            .prepare_coordinate_input(host(-3), host(3))
            .unwrap_err(),
        AudioAuthorityError::Overflow
    );
    assert_eq!(owner.state(), state);
}
