//! Original-coordinate epoch publication, with real acquired input and exact durations.
use super::*;
use beatkernel::input::{
    ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent,
};
use beatkernel::time::{ClockMapper, ClockMappingQuality};
use std::cmp::Ordering;

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}

fn host(nanos: i64) -> ClockPoint {
    point(1, nanos)
}

fn physical_origin() -> ClockPoint {
    point(2, 100)
}

fn config() -> AudioAuthorityConfig {
    AudioAuthorityConfig {
        history_capacity: 8,
        max_observation_age: Duration::from_nanos(1000),
        ..AudioAuthorityConfig::default()
    }
}

fn target(start: TargetTime, rate: u32) -> AudioAuthorityCoordinateBasis {
    AudioAuthorityCoordinateBasis::TargetFrame(
        TargetFrameBasis::new(physical_origin(), start, rate).unwrap(),
    )
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

fn observation(
    basis: AudioAuthorityCoordinateBasis,
    units: u64,
    at: i64,
) -> (ClockPair, NativeOutputCoordinate) {
    let original = coordinate(basis, units);
    (
        ClockPair {
            source: basis.point(original).unwrap(),
            target: host(at),
        },
        original,
    )
}

fn owner(basis: AudioAuthorityCoordinateBasis) -> AudioAuthority {
    AudioAuthority::new_with_coordinates(
        config(),
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

fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap()
}

fn input(at: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(at), sequence),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    })
}

fn observe(authority: &mut AudioAuthority, units: u64, at: i64) {
    let (pair, original) = observation(authority.coordinate_basis().unwrap(), units, at);
    assert_eq!(
        authority.observe_coordinate(authority.epoch().id, pair, original),
        Ok(ObservationAdmission::Retained)
    );
}

fn next_epoch(
    authority: &AudioAuthority,
    next_basis: AudioAuthorityCoordinateBasis,
) -> AudioAuthorityEpoch {
    let stream_origin = next_basis.origin().unwrap();
    AudioAuthorityEpoch {
        id: authority.epoch().id + 1,
        stream_origin,
        logical_origin: authority.epoch().logical_output(stream_origin).unwrap(),
        host_domain: authority.epoch().host_domain,
    }
}

fn snapshot(authority: &AudioAuthority) -> (String, usize) {
    (format!("{authority:?}"), authority.history.capacity())
}

fn watermarks(authority: &AudioAuthority) -> String {
    format!(
        "{:?}",
        (
            authority.acquired_prefix(),
            authority.closed_host_prefix(),
            authority.committed_input_host(),
            authority.committed_operation(),
            authority.committed_presentation(),
            authority.committed_coordinate_operation(),
            authority.committed_coordinate_presentation(),
        )
    )
}

fn seeded() -> (AudioAuthority, InputMerger) {
    let basis = target(TargetTime::new(0, 1, 3).unwrap(), 3);
    let mut authority = owner(basis);
    let mut merger = merger();
    observe(&mut authority, 0, 100);
    observe(&mut authority, 1, 200);
    let original = input(125, 1);
    merger.admit(original.clone(), host(250)).unwrap();
    authority.record_acquired_prefix(host(250)).unwrap();
    let prepared = authority
        .prepare_coordinate_input(host(125), host(250))
        .unwrap()
        .unwrap();
    assert_eq!(
        prepared
            .query()
            .compare_target_time(TargetTime::new(0, 5, 12).unwrap()),
        Ordering::Equal
    );
    assert_eq!(merger.pop_ready(host(250)).unwrap(), Some(original));
    authority
        .commit_coordinate_input(prepared, host(250))
        .unwrap();
    let frontier = authority
        .prepare_coordinate_frontier(host(250), &merger)
        .unwrap()
        .unwrap();
    authority
        .commit_coordinate_frontier(frontier, host(250), &mut merger)
        .unwrap();
    assert_eq!(authority.acquired_prefix(), Some(host(250)));
    assert_eq!(authority.closed_host_prefix(), Some(host(250)));
    assert_eq!(authority.committed_input_host(), Some(host(125)));
    assert_eq!(authority.committed_operation(), Some(point(3, 333_333_833)));
    assert_eq!(
        authority.committed_presentation(),
        Some(point(3, 333_333_833))
    );
    (authority, merger)
}

fn replacement_basis() -> AudioAuthorityCoordinateBasis {
    target(TargetTime::new(0, 2, 3).unwrap(), 7)
}

fn replacement_observations() -> [(ClockPair, NativeOutputCoordinate); 2] {
    [
        observation(replacement_basis(), 0, 260),
        observation(replacement_basis(), 1, 300),
    ]
}

fn prepared(authority: &AudioAuthority, merger: &InputMerger) -> PreparedPrimedCoordinateEpoch {
    let basis = replacement_basis();
    let staged = authority
        .prepare_coordinate_epoch(next_epoch(authority, basis), basis, merger)
        .unwrap();
    authority
        .prepare_primed_coordinate_epoch(staged, replacement_observations(), host(320), merger)
        .unwrap()
}

#[test]
fn mixed_rate_commit_preserves_all_watermarks_and_original_input_correspondence() {
    let (mut authority, mut merger) = seeded();
    let before = snapshot(&authority);
    let accepted = watermarks(&authority);
    let merger_before = format!("{merger:?}");
    let revision = authority.revision;
    let next = next_epoch(&authority, replacement_basis());
    let staged = authority
        .prepare_coordinate_epoch(next, replacement_basis(), &merger)
        .unwrap();
    assert_eq!(staged.next_epoch(), next);
    assert_eq!(staged.next_basis(), replacement_basis());
    let token = prepared(&authority, &merger);
    assert_eq!(token.next_epoch(), next);
    assert_eq!(token.next_basis(), replacement_basis());
    assert_eq!(token.observations(), replacement_observations());
    assert_eq!(token.prepared_at(), host(320));
    authority
        .validate_primed_coordinate_epoch(&token, host(325), &merger)
        .unwrap();
    assert_eq!(snapshot(&authority), before);
    assert_eq!(format!("{merger:?}"), merger_before);
    authority
        .commit_primed_coordinate_epoch(token, host(325), &merger)
        .unwrap();
    assert_eq!(authority.epoch(), next);
    assert_eq!(authority.coordinate_basis(), Some(replacement_basis()));
    assert_eq!(authority.revision, revision + 1);
    assert_eq!(authority.history.capacity(), before.1);
    assert_eq!(watermarks(&authority), accepted);
    assert_eq!(format!("{merger:?}"), merger_before);
    assert_eq!(authority.history_len(), 2);
    for (entry, (pair, original)) in authority.history.iter().zip(replacement_observations()) {
        assert_eq!(entry.pair(), pair);
        assert_eq!(entry.coordinate(), Some(original));
    }

    // The exact midpoint is 2/3 + 1/14 = 31/42 seconds. Floor only once.
    let original = input(280, 2);
    merger.admit(original.clone(), host(325)).unwrap();
    authority.record_acquired_prefix(host(300)).unwrap();
    let mapped = authority
        .prepare_coordinate_input(host(280), host(325))
        .unwrap()
        .unwrap();
    let expected_raw = 100 + (31u128 * 1_000_000_000 / 42) as i64;
    assert_eq!(mapped.original(), host(280));
    assert_eq!(mapped.epoch(), next.id);
    assert_eq!(mapped.raw_output(), point(2, expected_raw));
    assert_eq!(mapped.output(), point(3, 404_762_405));
    assert_eq!(
        mapped
            .query()
            .compare_target_time(TargetTime::new(0, 31, 42).unwrap()),
        Ordering::Equal
    );
    assert_eq!(mapped.mapper().quality(), ClockMappingQuality::Unknown);
    assert_eq!(
        mapped.mapper().map(host(280), ClockDomainId(3)),
        Some(Timestamp::from_nanos(404_762_405))
    );
    assert_eq!(merger.peek_ready(host(300)).unwrap(), Some(&original));
    assert_eq!(merger.pop_ready(host(300)).unwrap(), Some(original));
    authority
        .commit_coordinate_input(mapped, host(325))
        .unwrap();
    assert_eq!(authority.committed_input_host(), Some(host(280)));
}

#[test]
fn publication_preserves_predicted_operation_ahead_of_actual_presentation_verbatim() {
    let basis = target(TargetTime::new(0, 1, 3).unwrap(), 3);
    let mut authority = AudioAuthority::new_with_coordinates(
        AudioAuthorityConfig {
            input_extrapolation: ExtrapolationPolicy::Bounded {
                before: Duration::ZERO,
                after: Duration::from_nanos(100),
            },
            max_input_ahead: Duration::from_nanos(1_000_000_000),
            ..config()
        },
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: basis.origin().unwrap(),
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
        basis,
    )
    .unwrap();
    let mut merger = merger();
    observe(&mut authority, 0, 100);
    observe(&mut authority, 1, 200);
    authority.record_acquired_prefix(host(200)).unwrap();
    let frontier = authority
        .prepare_coordinate_frontier(host(250), &merger)
        .unwrap()
        .unwrap();
    authority
        .commit_coordinate_frontier(frontier, host(250), &mut merger)
        .unwrap();
    let original = input(225, 1);
    merger.admit(original.clone(), host(250)).unwrap();
    authority.record_acquired_prefix(host(225)).unwrap();
    let predicted = authority
        .prepare_coordinate_input(host(225), host(250))
        .unwrap()
        .unwrap();
    assert_eq!(
        predicted
            .query()
            .compare_target_time(TargetTime::new(0, 3, 4).unwrap()),
        Ordering::Equal
    );
    assert_eq!(merger.pop_ready(host(225)).unwrap(), Some(original));
    authority
        .commit_coordinate_input(predicted, host(250))
        .unwrap();
    assert_eq!(
        authority
            .committed_coordinate_operation()
            .unwrap()
            .compare_interpolated_time(authority.committed_coordinate_presentation().unwrap()),
        Ordering::Greater
    );
    let accepted = watermarks(&authority);
    let basis = target(TargetTime::new(0, 3, 4).unwrap(), 7);
    let staged = authority
        .prepare_coordinate_epoch(next_epoch(&authority, basis), basis, &merger)
        .unwrap();
    let token = authority
        .prepare_primed_coordinate_epoch(
            staged,
            [observation(basis, 0, 260), observation(basis, 1, 300)],
            host(320),
            &merger,
        )
        .unwrap();
    authority
        .commit_primed_coordinate_epoch(token, host(320), &merger)
        .unwrap();
    assert_eq!(watermarks(&authority), accepted);
}

#[test]
fn all_coordinate_classes_accept_exact_progress_hidden_by_equal_nanoseconds() {
    let source = OutputFrameBasis::new(physical_origin(), 4_000_000_000, 0).unwrap();
    let next_source = OutputFrameBasis::new(physical_origin(), 4_000_000_000, 1).unwrap();
    for (old, next) in [
        (
            AudioAuthorityCoordinateBasis::SourceFrame(source),
            AudioAuthorityCoordinateBasis::SourceFrame(next_source),
        ),
        (
            AudioAuthorityCoordinateBasis::SourceCounter {
                basis: source,
                frequency: 4_000_000_000,
            },
            AudioAuthorityCoordinateBasis::SourceCounter {
                basis: next_source,
                frequency: 3_000_000_000,
            },
        ),
        (
            target(TargetTime::from_frames(0, 1).unwrap(), 4_000_000_000),
            target(TargetTime::new(0, 1, 4_000_000_000).unwrap(), 4_000_000_000),
        ),
    ] {
        let mut authority = owner(old);
        let merger = merger();
        observe(&mut authority, 0, 100);
        observe(&mut authority, 1, 200);
        let originals = [observation(next, 0, 260), observation(next, 1, 300)];
        assert_eq!(originals[0].0.source, originals[1].0.source);
        assert_ne!(originals[0].1, originals[1].1);
        let staged = authority
            .prepare_coordinate_epoch(next_epoch(&authority, next), next, &merger)
            .unwrap();
        let token = authority
            .prepare_primed_coordinate_epoch(staged, originals, host(320), &merger)
            .unwrap();
        authority
            .commit_primed_coordinate_epoch(token, host(320), &merger)
            .unwrap();
        assert_eq!(authority.coordinate_basis(), Some(next));
        for (entry, (pair, original)) in authority.history.iter().zip(originals) {
            assert_eq!(entry.pair(), pair);
            assert_eq!(entry.coordinate(), Some(original));
        }
    }
}

#[test]
fn preparation_rejects_class_origin_epoch_domain_and_projection_changes_atomically() {
    let (authority, merger) = seeded();
    let before = snapshot(&authority);
    let basis = replacement_basis();
    let next = next_epoch(&authority, basis);
    let altered_origin = AudioAuthorityCoordinateBasis::TargetFrame(
        TargetFrameBasis::new(point(2, 101), TargetTime::new(0, 2, 3).unwrap(), 7).unwrap(),
    );
    let altered_domain = AudioAuthorityCoordinateBasis::TargetFrame(
        TargetFrameBasis::new(point(4, 100), TargetTime::new(0, 2, 3).unwrap(), 7).unwrap(),
    );
    let source_class = AudioAuthorityCoordinateBasis::SourceFrame(
        OutputFrameBasis::new(physical_origin(), 3, 2).unwrap(),
    );
    let cases = [
        (AudioAuthorityEpoch { id: 7, ..next }, basis),
        (AudioAuthorityEpoch { id: 6, ..next }, basis),
        (next, source_class),
        (next, altered_origin),
        (next, altered_domain),
        (
            AudioAuthorityEpoch {
                stream_origin: point(2, 666_666_767),
                ..next
            },
            basis,
        ),
        (
            AudioAuthorityEpoch {
                logical_origin: point(3, 333_333_834),
                ..next
            },
            basis,
        ),
        (
            AudioAuthorityEpoch {
                logical_origin: point(4, 333_333_833),
                ..next
            },
            basis,
        ),
        (
            AudioAuthorityEpoch {
                host_domain: ClockDomainId(4),
                ..next
            },
            basis,
        ),
    ];
    for (next, basis) in cases {
        assert!(
            authority
                .prepare_coordinate_epoch(next, basis, &merger)
                .is_err()
        );
        assert_eq!(snapshot(&authority), before);
    }
}

#[test]
fn exact_descriptor_regression_is_refused_even_when_both_rounded_origins_match() {
    let old = target(TargetTime::from_frames(0, 1).unwrap(), 4_000_000_000);
    let mut authority = owner(old);
    observe(&mut authority, 0, 100);
    observe(&mut authority, 3, 200); // 3/4 ns, still rounded to the same origin.
    let next_basis = target(TargetTime::new(0, 1, 2_000_000_000).unwrap(), 4_000_000_000);
    let next = next_epoch(&authority, next_basis);
    assert_eq!(
        next.stream_origin,
        authority.latest_observation().unwrap().source
    );
    let before = snapshot(&authority);
    assert!(
        authority
            .prepare_coordinate_epoch(next, next_basis, &merger())
            .is_err()
    );
    assert_eq!(snapshot(&authority), before);
}

#[test]
fn invalid_original_coordinate_pairs_are_refused_without_staged_or_active_effects() {
    let (authority, merger) = seeded();
    let staged = authority
        .prepare_coordinate_epoch(
            next_epoch(&authority, replacement_basis()),
            replacement_basis(),
            &merger,
        )
        .unwrap();
    let originals = replacement_observations();
    let before = snapshot(&authority);
    let merger_before = format!("{merger:?}");
    let mut cases = Vec::new();
    let mut duplicate = originals;
    duplicate[1].1 = duplicate[0].1;
    duplicate[1].0.source = duplicate[0].0.source;
    cases.push((duplicate, host(320)));
    let mut native_backtrack = [
        observation(replacement_basis(), 2, 260),
        observation(replacement_basis(), 1, 300),
    ];
    cases.push((native_backtrack, host(320)));
    native_backtrack[1].0.target = host(260);
    cases.push((native_backtrack, host(320)));
    let mut host_backtrack = originals;
    host_backtrack[1].0.target = host(259);
    cases.push((host_backtrack, host(320)));
    let mut future = originals;
    future[1].0.target = host(321);
    cases.push((future, host(320)));
    let mut behind_acquired = originals;
    behind_acquired[0].0.target = host(210);
    behind_acquired[1].0.target = host(240);
    cases.push((behind_acquired, host(320)));
    let mut torn_point = originals;
    torn_point[1].0.source = point(2, 809_523_910);
    cases.push((torn_point, host(320)));
    let mut wrong_kind = originals;
    wrong_kind[1].1 = NativeOutputCoordinate::Counter {
        position: 1,
        frequency: 7,
    };
    cases.push((wrong_kind, host(320)));
    let mut wrong_native_domain = originals;
    wrong_native_domain[0].0.source.domain = ClockDomainId(4);
    cases.push((wrong_native_domain, host(320)));
    let mut wrong_host_domain = originals;
    wrong_host_domain[1].0.target.domain = ClockDomainId(4);
    cases.push((wrong_host_domain, host(320)));
    cases.push((originals, point(4, 320)));
    cases.push((originals, host(1301)));
    // The second is fresh while the first is stale: both originals must be fresh.
    cases.push((
        [
            observation(replacement_basis(), 0, 100),
            observation(replacement_basis(), 1, 300),
        ],
        host(1201),
    ));
    for (observations, now) in cases {
        assert!(
            authority
                .prepare_primed_coordinate_epoch(staged.clone(), observations, now, &merger)
                .is_err(),
            "{observations:?} at {now:?}"
        );
        assert_eq!(snapshot(&authority), before);
        assert_eq!(format!("{merger:?}"), merger_before);
    }
}

#[test]
fn counter_frequency_is_original_metadata_and_cannot_be_torn_during_priming() {
    let old_source = OutputFrameBasis::new(physical_origin(), 3, 0).unwrap();
    let old = AudioAuthorityCoordinateBasis::SourceCounter {
        basis: old_source,
        frequency: 3,
    };
    let mut authority = owner(old);
    observe(&mut authority, 0, 100);
    observe(&mut authority, 1, 200);
    let next = AudioAuthorityCoordinateBasis::SourceCounter {
        basis: OutputFrameBasis::new(physical_origin(), 3, 1).unwrap(),
        frequency: u64::MAX,
    };
    let merger = merger();
    let staged = authority
        .prepare_coordinate_epoch(next_epoch(&authority, next), next, &merger)
        .unwrap();
    let mut originals = [observation(next, 0, 260), observation(next, 1, 300)];
    originals[1].1 = NativeOutputCoordinate::Counter {
        position: 1,
        frequency: u64::MAX - 1,
    };
    let before = snapshot(&authority);
    assert_eq!(
        authority.prepare_primed_coordinate_epoch(staged, originals, host(320), &merger),
        Err(AudioAuthorityError::CoordinateMismatch)
    );
    assert_eq!(snapshot(&authority), before);
}

#[test]
fn publication_rechecks_freshness_time_domain_and_duplicate_use_atomically() {
    let (mut authority, merger) = seeded();
    let token = prepared(&authority, &merger);
    let before = snapshot(&authority);
    for now in [host(319), host(1301), point(4, 320)] {
        assert!(
            authority
                .validate_primed_coordinate_epoch(&token, now, &merger)
                .is_err()
        );
        assert!(
            authority
                .commit_primed_coordinate_epoch(token.clone(), now, &merger)
                .is_err()
        );
        assert_eq!(snapshot(&authority), before);
    }
    authority
        .commit_primed_coordinate_epoch(token.clone(), host(325), &merger)
        .unwrap();
    let published = snapshot(&authority);
    assert_eq!(
        authority.commit_primed_coordinate_epoch(token, host(325), &merger),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(snapshot(&authority), published);
}

#[test]
fn pending_input_is_preserved_and_refuses_every_coordinate_stage() {
    let (mut authority, mut merger) = seeded();
    let token = prepared(&authority, &merger);
    let original = input(280, 2);
    merger.admit(original.clone(), host(320)).unwrap();
    let before = snapshot(&authority);
    let merger_before = format!("{merger:?}");
    assert_eq!(
        authority.prepare_coordinate_epoch(token.next_epoch(), token.next_basis(), &merger),
        Err(AudioAuthorityError::PendingInputs)
    );
    assert_eq!(
        authority.validate_primed_coordinate_epoch(&token, host(320), &merger),
        Err(AudioAuthorityError::PendingInputs)
    );
    assert_eq!(
        authority.commit_primed_coordinate_epoch(token, host(320), &merger),
        Err(AudioAuthorityError::PendingInputs)
    );
    assert_eq!(snapshot(&authority), before);
    assert_eq!(format!("{merger:?}"), merger_before);
    assert_eq!(merger.peek_ready(host(320)).unwrap(), Some(&original));
}

#[test]
fn changed_authority_and_drained_merger_both_invalidate_prepared_publication() {
    for change in 0..3 {
        let (mut authority, mut merger) = seeded();
        let token = prepared(&authority, &merger);
        match change {
            0 => authority.record_acquired_prefix(host(251)).unwrap(),
            1 => {
                let event = input(280, 2);
                merger.admit(event.clone(), host(320)).unwrap();
                assert_eq!(merger.pop_ready(host(320)).unwrap(), Some(event));
                assert_eq!(merger.pending(), 0);
            }
            _ => merger.commit(host(350)).unwrap(),
        }
        let before = snapshot(&authority);
        let merger_before = format!("{merger:?}");
        assert!(
            authority
                .validate_primed_coordinate_epoch(&token, host(360), &merger)
                .is_err()
        );
        assert!(
            authority
                .commit_primed_coordinate_epoch(token, host(360), &merger)
                .is_err()
        );
        assert_eq!(snapshot(&authority), before);
        assert_eq!(format!("{merger:?}"), merger_before);
    }
}

fn reconstructed_merger(
    device: DeviceId,
    domain: ClockDomainId,
    origin: i64,
    capacity: usize,
) -> InputMerger {
    let mut merger =
        InputMerger::new(domain, point(domain.0, origin), vec![device], capacity).unwrap();
    let event = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(device, point(domain.0, 125), 1),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    });
    merger.admit(event.clone(), point(domain.0, 250)).unwrap();
    assert_eq!(merger.pop_ready(point(domain.0, 250)).unwrap(), Some(event));
    merger.commit(point(domain.0, 250)).unwrap();
    merger
}

#[test]
fn merger_snapshot_binds_exact_registry_domain_origin_and_capacity() {
    let (mut authority, merger) = seeded();
    let token = prepared(&authority, &merger);
    let before = snapshot(&authority);
    for different in [
        reconstructed_merger(DeviceId(10), ClockDomainId(1), 0, 8),
        reconstructed_merger(DeviceId(9), ClockDomainId(4), 0, 8),
        reconstructed_merger(DeviceId(9), ClockDomainId(1), 1, 8),
        reconstructed_merger(DeviceId(9), ClockDomainId(1), 0, 4),
    ] {
        assert_eq!(different.pending(), 0);
        assert_eq!(different.source_count(), merger.source_count());
        let merger_before = format!("{different:?}");
        assert!(
            authority
                .validate_primed_coordinate_epoch(&token, host(320), &different)
                .is_err()
        );
        assert!(
            authority
                .commit_primed_coordinate_epoch(token.clone(), host(320), &different)
                .is_err()
        );
        assert_eq!(snapshot(&authority), before);
        assert_eq!(format!("{different:?}"), merger_before);
    }
}

#[test]
fn semantic_equivalence_allows_an_identical_authority_and_merger_without_pointer_identity() {
    let (left, left_merger) = seeded();
    let token = prepared(&left, &left_merger);
    let (mut equivalent, equivalent_merger) = seeded();
    assert_eq!(snapshot(&left), snapshot(&equivalent));
    assert_eq!(format!("{left_merger:?}"), format!("{equivalent_merger:?}"));
    equivalent
        .validate_primed_coordinate_epoch(&token, host(320), &equivalent_merger)
        .unwrap();
    equivalent
        .commit_primed_coordinate_epoch(token, host(320), &equivalent_merger)
        .unwrap();
    assert_eq!(equivalent.coordinate_basis(), Some(replacement_basis()));
    assert_eq!(left.epoch().id, 7);
    assert_eq!(equivalent.epoch().id, 8);
    assert_eq!(watermarks(&equivalent), watermarks(&left));
}

#[test]
fn staged_coordinate_epoch_also_becomes_stale_before_original_anchors_are_primed() {
    let (mut authority, merger) = seeded();
    let staged = authority
        .prepare_coordinate_epoch(
            next_epoch(&authority, replacement_basis()),
            replacement_basis(),
            &merger,
        )
        .unwrap();
    authority.record_acquired_prefix(host(251)).unwrap();
    let before = snapshot(&authority);
    assert_eq!(
        authority.prepare_primed_coordinate_epoch(
            staged,
            replacement_observations(),
            host(320),
            &merger
        ),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(snapshot(&authority), before);
}

#[test]
fn coordinate_preparation_keeps_scalar_admission_guards_intact() {
    let (mut authority, merger) = seeded();
    let before = snapshot(&authority);
    assert_eq!(
        authority.prepare_epoch(next_epoch(&authority, replacement_basis()), &merger),
        Err(AudioAuthorityError::CoordinateMappingUnavailable)
    );
    assert_eq!(
        authority.observe(7, authority.latest_observation().unwrap()),
        Err(AudioAuthorityError::CoordinateModeMismatch)
    );
    assert_eq!(snapshot(&authority), before);
    let scalar = AudioAuthority::new(
        config(),
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: physical_origin(),
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
    )
    .unwrap();
    let before = snapshot(&scalar);
    assert_eq!(
        scalar.prepare_coordinate_epoch(
            next_epoch(&scalar, replacement_basis()),
            replacement_basis(),
            &merger
        ),
        Err(AudioAuthorityError::CoordinateModeMismatch)
    );
    assert_eq!(snapshot(&scalar), before);
}

#[test]
fn timestamp_projection_native_coordinate_and_revision_overflow_are_atomic() {
    let physical = point(2, i64::MIN);
    let old_basis =
        AudioAuthorityCoordinateBasis::SourceFrame(OutputFrameBasis::new(physical, 1, 0).unwrap());
    let authority = AudioAuthority::new_with_coordinates(
        config(),
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: physical,
            logical_origin: point(3, i64::MAX - 1),
            host_domain: ClockDomainId(1),
        },
        old_basis,
    )
    .unwrap();
    let next_basis =
        AudioAuthorityCoordinateBasis::SourceFrame(OutputFrameBasis::new(physical, 1, 1).unwrap());
    let next = AudioAuthorityEpoch {
        id: 8,
        stream_origin: point(2, i64::MIN + 1_000_000_000),
        logical_origin: point(3, i64::MAX),
        host_domain: ClockDomainId(1),
    };
    let before = snapshot(&authority);
    assert_eq!(
        authority.prepare_coordinate_epoch(next, next_basis, &merger()),
        Err(AudioAuthorityError::Overflow)
    );
    assert_eq!(snapshot(&authority), before);

    let extreme = AudioAuthorityCoordinateBasis::SourceFrame(
        OutputFrameBasis::new(physical_origin(), 1, u64::MAX).unwrap(),
    );
    let normal = owner(AudioAuthorityCoordinateBasis::SourceFrame(
        OutputFrameBasis::new(physical_origin(), 1, 0).unwrap(),
    ));
    let before = snapshot(&normal);
    assert_eq!(
        normal.prepare_coordinate_epoch(
            AudioAuthorityEpoch {
                id: 8,
                ..normal.epoch()
            },
            extreme,
            &merger()
        ),
        Err(AudioAuthorityError::Overflow)
    );
    assert_eq!(snapshot(&normal), before);

    let mut authority = owner(AudioAuthorityCoordinateBasis::TargetFrame(
        TargetFrameBasis::new(
            point(2, i64::MAX - 1),
            TargetTime::from_frames(0, 1).unwrap(),
            1_000_000_000,
        )
        .unwrap(),
    ));
    observe(&mut authority, 0, 100);
    observe(&mut authority, 1, 200);
    let next_basis = AudioAuthorityCoordinateBasis::TargetFrame(
        TargetFrameBasis::new(
            point(2, i64::MAX - 1),
            TargetTime::from_frames(1, 1_000_000_000).unwrap(),
            1_000_000_000,
        )
        .unwrap(),
    );
    let merger = merger();
    let staged = authority
        .prepare_coordinate_epoch(next_epoch(&authority, next_basis), next_basis, &merger)
        .unwrap();
    let observations = [
        observation(next_basis, 0, 260),
        (
            ClockPair {
                source: point(2, i64::MAX),
                target: host(300),
            },
            NativeOutputCoordinate::StreamFrame { frame: 1 },
        ),
    ];
    let before = snapshot(&authority);
    assert_eq!(
        authority.prepare_primed_coordinate_epoch(staged, observations, host(320), &merger),
        Err(AudioAuthorityError::Overflow)
    );
    assert_eq!(snapshot(&authority), before);

    let (mut exhausted, merger) = seeded();
    exhausted.revision = u64::MAX;
    let before = snapshot(&exhausted);
    assert_eq!(
        exhausted.prepare_coordinate_epoch(
            next_epoch(&exhausted, replacement_basis()),
            replacement_basis(),
            &merger
        ),
        Err(AudioAuthorityError::Overflow)
    );
    assert_eq!(snapshot(&exhausted), before);
}
