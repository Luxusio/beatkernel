//! AC-015: publish original correlation pairs without advancing accepted play.
use super::*;
use beatkernel::input::{
    ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent,
};
use beatkernel::time::{ClockMapper, ClockMappingQuality};

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn host(nanos: i64) -> ClockPoint {
    point(1, nanos)
}
fn logical(nanos: i64) -> ClockPoint {
    point(3, nanos)
}
fn pair(domain: u32, raw: i64, host_ns: i64) -> ClockPair {
    ClockPair {
        source: point(domain, raw),
        target: host(host_ns),
    }
}
fn config() -> AudioAuthorityConfig {
    AudioAuthorityConfig {
        history_capacity: 4,
        max_observation_age: Duration::from_nanos(1000),
        input_extrapolation: ExtrapolationPolicy::Forbid,
        max_input_ahead: Duration::ZERO,
    }
}
fn original_epoch() -> AudioAuthorityEpoch {
    AudioAuthorityEpoch {
        id: 1,
        stream_origin: point(2, 1000),
        logical_origin: logical(5000),
        host_domain: ClockDomainId(1),
    }
}
fn replacement() -> AudioAuthorityEpoch {
    AudioAuthorityEpoch {
        id: 2,
        stream_origin: point(4, 0),
        logical_origin: logical(5300),
        host_domain: ClockDomainId(1),
    }
}
fn input(nanos: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(u64::MAX), host(nanos), sequence),
        control: PhysicalControlId::keyboard(7),
        state: ButtonState::Down,
    })
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(u64::MAX)], 8).unwrap()
}
fn watermarks(authority: &AudioAuthority) -> [Option<ClockPoint>; 5] {
    [
        authority.acquired_prefix(),
        authority.closed_host_prefix(),
        authority.committed_input_host(),
        authority.committed_operation(),
        authority.committed_presentation(),
    ]
}
fn state(authority: &AudioAuthority) -> (String, usize) {
    (format!("{authority:?}"), authority.history.capacity())
}
fn seeded() -> (AudioAuthority, InputMerger) {
    let mut authority = AudioAuthority::new(config(), original_epoch()).unwrap();
    let mut merger = merger();
    authority.observe(1, pair(2, 1100, 100)).unwrap();
    authority.observe(1, pair(2, 1300, 200)).unwrap();
    let original = input(125, 1);
    merger.admit(original.clone(), host(250)).unwrap();
    authority.record_acquired_prefix(host(250)).unwrap();
    let prepared = authority
        .prepare_input(host(125), host(250))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.output(), logical(5150));
    assert_eq!(merger.pop_ready(host(250)).unwrap(), Some(original));
    authority.commit_input(prepared).unwrap();
    let frontier = authority
        .prepare_frontier(host(250), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.output(), logical(5300));
    authority.commit_frontier(frontier, &mut merger).unwrap();
    assert_eq!(
        watermarks(&authority),
        [
            Some(host(250)),
            Some(host(250)),
            Some(host(125)),
            Some(logical(5300)),
            Some(logical(5300))
        ]
    );
    (authority, merger)
}
fn replacement_pairs() -> [ClockPair; 2] {
    [pair(4, 100, 180), pair(4, 200, 300)]
}

#[test]
fn primed_replacement_stages_without_mutation_then_preserves_every_committed_watermark() {
    let (mut authority, mut merger) = seeded();
    let before = state(&authority);
    let accepted = watermarks(&authority);
    let capacity = authority.history.capacity();
    let revision = authority.revision;
    let staged = authority.prepare_epoch(replacement(), &merger).unwrap();
    let prepared = authority
        .prepare_primed_epoch(staged, replacement_pairs(), host(320), &merger)
        .unwrap();
    assert_eq!(state(&authority), before);
    authority
        .validate_primed_correlation(&prepared, host(325), &merger)
        .unwrap();
    assert_eq!(state(&authority), before);
    authority
        .commit_primed_correlation(prepared, host(325), &merger)
        .unwrap();
    assert_eq!(authority.epoch(), replacement());
    assert_eq!(watermarks(&authority), accepted);
    assert_eq!(authority.history_len(), 2);
    assert_eq!(authority.latest_observation(), Some(replacement_pairs()[1]));
    assert_eq!(authority.history.capacity(), capacity);
    assert_eq!(authority.revision, revision + 1);
    assert_eq!(merger.pending(), 0);
    assert!(matches!(
        authority.observe(1, pair(2, 1400, 320)),
        Err(AudioAuthorityError::WrongEpoch)
    ));
    assert_eq!(watermarks(&authority), accepted);
    // The merger keeps the original source sequence/closed prefix across publication.
    assert!(merger.admit(input(250, 2), host(320)).is_err());
    merger.admit(input(270, 2), host(320)).unwrap();
    authority.record_acquired_prefix(host(300)).unwrap();
    let mapped = authority
        .prepare_input(host(270), host(320))
        .unwrap()
        .unwrap();
    assert_eq!(mapped.output(), logical(5475));
    assert_eq!(mapped.mapper().quality(), ClockMappingQuality::Unknown);
    assert_eq!(mapped.mapper().uncertainty(), None);
    assert_eq!(
        authority.committed_operation(),
        accepted[3],
        "mapping preparation grants no gameplay advance"
    );
    assert_eq!(merger.pending(), 1);
}

#[test]
fn primed_restart_preserves_actual_presentation_behind_a_real_predicted_input_operation() {
    let ahead = AudioAuthorityConfig {
        input_extrapolation: ExtrapolationPolicy::Bounded {
            before: Duration::ZERO,
            after: Duration::from_nanos(100),
        },
        max_input_ahead: Duration::from_nanos(100),
        ..config()
    };
    let mut authority = AudioAuthority::new(ahead, original_epoch()).unwrap();
    let mut merger = merger();
    authority.observe(1, pair(2, 1100, 100)).unwrap();
    authority.observe(1, pair(2, 1300, 200)).unwrap();
    authority.record_acquired_prefix(host(200)).unwrap();
    let frontier = authority
        .prepare_frontier(host(200), &merger)
        .unwrap()
        .unwrap();
    authority.commit_frontier(frontier, &mut merger).unwrap();
    let event = input(225, 1);
    merger.admit(event.clone(), host(250)).unwrap();
    authority.record_acquired_prefix(host(225)).unwrap();
    let prepared = authority
        .prepare_input(host(225), host(250))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.output(), logical(5350));
    assert_eq!(merger.pop_ready(host(225)).unwrap(), Some(event));
    authority.commit_input(prepared).unwrap();
    let accepted = watermarks(&authority);
    let before = state(&authority);
    let pairs = [pair(2, 1300, 200), pair(2, 1320, 225)];
    let staged = authority.prepare_correlation_restart(&merger).unwrap();
    let candidate = authority
        .prepare_primed_restart(staged, pairs, host(250), &merger)
        .unwrap();
    assert_eq!(state(&authority), before);
    authority
        .commit_primed_correlation(candidate, host(250), &merger)
        .unwrap();
    assert_eq!(authority.epoch(), original_epoch());
    assert_eq!(authority.config(), ahead);
    assert_eq!(watermarks(&authority), accepted);
    assert_eq!(authority.committed_operation(), Some(logical(5350)));
    assert_eq!(authority.committed_presentation(), Some(logical(5300)));
    let frontier = authority
        .prepare_frontier(host(250), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.output(), logical(5320));
    assert_eq!(
        frontier.advance(),
        None,
        "fresh actual output cannot move the predicted operation backward"
    );
    authority.commit_frontier(frontier, &mut merger).unwrap();
    assert_eq!(authority.committed_operation(), Some(logical(5350)));
    assert_eq!(authority.committed_presentation(), Some(logical(5320)));
}

#[test]
fn invalid_original_pairs_cannot_prime_or_change_active_epoch_history_or_watermarks() {
    let (authority, merger) = seeded();
    let staged = authority.prepare_epoch(replacement(), &merger).unwrap();
    let before = state(&authority);
    let cases = [
        ([pair(4, 100, 180), pair(4, 100, 300)], host(320)),
        ([pair(4, 100, 180), pair(4, 90, 300)], host(320)),
        ([pair(4, 100, 180), pair(4, 200, 180)], host(320)),
        ([pair(4, 100, 180), pair(4, 200, 179)], host(320)),
        ([pair(4, 100, 180), pair(4, 200, 240)], host(320)),
        ([pair(4, 100, 180), pair(4, 200, 330)], host(320)),
        ([pair(2, 100, 180), pair(4, 200, 300)], host(320)),
        (
            [
                ClockPair {
                    source: point(4, 100),
                    target: point(7, 180),
                },
                pair(4, 200, 300),
            ],
            host(320),
        ),
        ([pair(4, -1, 180), pair(4, 200, 300)], host(320)),
        (replacement_pairs(), host(1301)),
        (replacement_pairs(), point(7, 320)),
    ];
    for (pairs, now) in cases {
        assert!(
            authority
                .prepare_primed_epoch(staged, pairs, now, &merger)
                .is_err(),
            "invalid original pairs {pairs:?} at {now:?}"
        );
        assert_eq!(state(&authority), before);
        assert_eq!(merger.pending(), 0);
    }
    let restart = authority.prepare_correlation_restart(&merger).unwrap();
    assert!(authority
        .prepare_primed_restart(
            restart,
            [pair(2, 1299, 180), pair(2, 1400, 300)],
            host(320),
            &merger
        )
        .is_err());
    assert!(authority
        .prepare_primed_restart(
            restart,
            [pair(2, 1300, 180), pair(2, 1300, 300)],
            host(320),
            &merger
        )
        .is_err());
    assert_eq!(state(&authority), before);
}

#[test]
fn primed_publication_rechecks_freshness_time_and_domain_without_any_commit_effect() {
    let (mut authority, merger) = seeded();
    let staged = authority.prepare_epoch(replacement(), &merger).unwrap();
    let prepared = authority
        .prepare_primed_epoch(staged, replacement_pairs(), host(320), &merger)
        .unwrap();
    let before = state(&authority);
    for (now, error) in [
        (host(319), AudioAuthorityError::ObservationRegression),
        (host(1301), AudioAuthorityError::HistoryExpired),
        (point(7, 320), AudioAuthorityError::DomainMismatch),
    ] {
        assert_eq!(
            authority.validate_primed_correlation(&prepared, now, &merger),
            Err(error)
        );
        assert_eq!(state(&authority), before);
        assert_eq!(
            authority.commit_primed_correlation(prepared, now, &merger),
            Err(error)
        );
        assert_eq!(state(&authority), before);
    }
    authority
        .commit_primed_correlation(prepared, host(325), &merger)
        .unwrap();
    let published = state(&authority);
    assert_eq!(
        authority.commit_primed_correlation(prepared, host(325), &merger),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(state(&authority), published);
}

#[test]
fn pending_input_and_stale_staging_are_rechecked_before_publication_or_preparation() {
    let (mut authority, mut merger) = seeded();
    let staged = authority.prepare_epoch(replacement(), &merger).unwrap();
    let prepared = authority
        .prepare_primed_epoch(staged, replacement_pairs(), host(320), &merger)
        .unwrap();
    let event = input(260, 2);
    merger.admit(event.clone(), host(320)).unwrap();
    let before = state(&authority);
    assert!(matches!(
        authority.prepare_primed_epoch(staged, replacement_pairs(), host(320), &merger),
        Err(AudioAuthorityError::PendingInputs)
    ));
    assert_eq!(
        authority.validate_primed_correlation(&prepared, host(320), &merger),
        Err(AudioAuthorityError::PendingInputs)
    );
    assert_eq!(
        authority.commit_primed_correlation(prepared, host(320), &merger),
        Err(AudioAuthorityError::PendingInputs)
    );
    assert_eq!(state(&authority), before);
    assert_eq!(merger.pending(), 1);
    assert_eq!(merger.peek_ready(host(320)).unwrap(), Some(&event));
    assert_eq!(merger.pop_ready(host(320)).unwrap(), Some(event));
    authority.record_acquired_prefix(host(260)).unwrap();
    let changed = state(&authority);
    assert_eq!(
        authority.validate_primed_correlation(&prepared, host(320), &merger),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert!(matches!(
        authority.prepare_primed_epoch(staged, replacement_pairs(), host(320), &merger),
        Err(AudioAuthorityError::StalePreparation)
    ));
    assert_eq!(
        authority.commit_primed_correlation(prepared, host(320), &merger),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(state(&authority), changed);
}

#[test]
fn primed_tokens_use_semantic_state_and_refuse_a_different_owner_configuration() {
    let (left, merger) = seeded();
    let staged = left.prepare_epoch(replacement(), &merger).unwrap();
    let prepared = left
        .prepare_primed_epoch(staged, replacement_pairs(), host(320), &merger)
        .unwrap();
    let (mut identical, identical_merger) = seeded();
    identical
        .validate_primed_correlation(&prepared, host(320), &identical_merger)
        .unwrap();
    identical
        .commit_primed_correlation(prepared, host(320), &identical_merger)
        .unwrap();
    assert_eq!(identical.epoch(), replacement());
    let different = AudioAuthority::new(
        AudioAuthorityConfig {
            history_capacity: 2,
            ..config()
        },
        original_epoch(),
    )
    .unwrap();
    let before = state(&different);
    assert_eq!(
        different.validate_primed_correlation(&prepared, host(320), &merger),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(state(&different), before);
}

#[test]
fn priming_checks_rebase_overflow_and_revision_limit_before_publication_permission() {
    let (authority, merger) = seeded();
    let extreme = AudioAuthorityEpoch {
        logical_origin: logical(i64::MAX - 1),
        ..replacement()
    };
    let staged = authority.prepare_epoch(extreme, &merger).unwrap();
    let before = state(&authority);
    assert!(matches!(
        authority.prepare_primed_epoch(
            staged,
            [pair(4, 2, 250), pair(4, 3, 300)],
            host(320),
            &merger
        ),
        Err(AudioAuthorityError::Overflow)
    ));
    assert_eq!(state(&authority), before);
    let (mut boundary, merger) = seeded();
    boundary.revision = u64::MAX - 1;
    let staged = boundary.prepare_epoch(replacement(), &merger).unwrap();
    let prepared = boundary
        .prepare_primed_epoch(staged, replacement_pairs(), host(320), &merger)
        .unwrap();
    let accepted = watermarks(&boundary);
    boundary
        .commit_primed_correlation(prepared, host(320), &merger)
        .unwrap();
    assert_eq!(
        boundary.revision,
        u64::MAX,
        "primed publication consumes exactly one revision"
    );
    assert_eq!(watermarks(&boundary), accepted);
    let exhausted = state(&boundary);
    assert!(matches!(
        boundary.prepare_correlation_restart(&merger),
        Err(AudioAuthorityError::Overflow)
    ));
    assert_eq!(state(&boundary), exhausted);
    // Private child access exercises exhausted staging without a production test seam.
    let (mut exhausted_owner, merger) = seeded();
    let mut epoch_token = exhausted_owner
        .prepare_epoch(replacement(), &merger)
        .unwrap();
    let mut restart_token = exhausted_owner
        .prepare_correlation_restart(&merger)
        .unwrap();
    exhausted_owner.revision = u64::MAX;
    epoch_token.state.revision = u64::MAX;
    restart_token.state.revision = u64::MAX;
    let before = state(&exhausted_owner);
    assert!(matches!(
        exhausted_owner.prepare_primed_epoch(epoch_token, replacement_pairs(), host(320), &merger),
        Err(AudioAuthorityError::Overflow)
    ));
    assert!(matches!(
        exhausted_owner.prepare_primed_restart(
            restart_token,
            [pair(2, 1300, 180), pair(2, 1400, 300)],
            host(320),
            &merger
        ),
        Err(AudioAuthorityError::Overflow)
    ));
    assert_eq!(state(&exhausted_owner), before);
}
