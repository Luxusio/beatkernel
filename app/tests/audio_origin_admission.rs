//! Original observation pairs, checked frozen admissions, and no Runtime substitutes.
use beatkernel::time::{
    ClockDomainId, ClockPair, ClockPoint, Duration, ExtrapolationPolicy, Timestamp,
};
use beatkernel_bms_runtime::audio_authority::{
    AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch, AudioAuthorityError,
};

fn p(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn owner(origin: i64) -> AudioAuthority {
    AudioAuthority::new(
        AudioAuthorityConfig {
            history_capacity: 16,
            max_observation_age: Duration::from_nanos(1_000_000_000),
            input_extrapolation: ExtrapolationPolicy::Bounded {
                before: Duration::from_nanos(100_000_000),
                after: Duration::ZERO,
            },
            max_input_ahead: Duration::ZERO,
        },
        AudioAuthorityEpoch {
            id: 1,
            stream_origin: p(2, 0),
            logical_origin: p(3, origin),
            host_domain: ClockDomainId(1),
        },
    )
    .unwrap()
}
fn real_pairs() -> AudioAuthority {
    let mut a = owner(0);
    a.observe(
        1,
        ClockPair {
            source: p(2, 9_816_157),
            target: p(1, 1_760_030_000),
        },
    )
    .unwrap();
    a.observe(
        1,
        ClockPair {
            source: p(2, 22_571_157),
            target: p(1, 1_772_805_000),
        },
    )
    .unwrap();
    a.record_acquired_prefix(p(1, 1_772_805_000)).unwrap();
    a
}
fn linear() -> AudioAuthority {
    let mut a = owner(5_000_000_000);
    a.observe(
        1,
        ClockPair {
            source: p(2, 10),
            target: p(1, 110),
        },
    )
    .unwrap();
    a.observe(
        1,
        ClockPair {
            source: p(2, 30),
            target: p(1, 130),
        },
    )
    .unwrap();
    a.record_acquired_prefix(p(1, 130)).unwrap();
    a
}

#[test]
fn actual_browser_pairs_classify_before_estimated_start_without_clamping() {
    let mut a = real_pairs();
    let now = p(1, 1_772_805_000);
    let input = a.prepare_input(p(1, 1_719_645_000), now).unwrap().unwrap();
    assert!(
        1_719_645_000 > 1_717_990_000,
        "HOST start heuristic would have admitted this event"
    );
    assert_eq!(input.original(), p(1, 1_719_645_000));
    assert_eq!(input.output(), p(3, -30_505_617));
    assert!(input.is_before_origin());
    assert_eq!(
        a.commit_input(input),
        Err(AudioAuthorityError::OperationRegression)
    );
    assert_eq!(a.committed_input_host(), None);
    assert_eq!(a.committed_operation(), None);
    assert_eq!(a.closed_host_prefix(), None);
    a.commit_discarded_input(input).unwrap();
    assert_eq!(a.committed_input_host(), Some(input.original()));
    assert_eq!(
        a.committed_operation(),
        None,
        "discard cannot grant song progress"
    );
    assert_eq!(
        a.closed_host_prefix(),
        None,
        "discard cannot close presentation coverage"
    );
    assert_eq!(
        a.commit_discarded_input(input),
        Err(AudioAuthorityError::StalePreparation)
    );
}

#[test]
fn exact_origin_and_postorigin_use_normal_commit_in_nonzero_logical_domain() {
    let mut a = linear();
    let now = p(1, 130);
    let before = a.prepare_input(p(1, 99), now).unwrap().unwrap();
    assert_eq!(before.output(), p(3, 4_999_999_999));
    assert!(before.is_before_origin());
    a.commit_discarded_input(before).unwrap();
    let exact = a.prepare_input(p(1, 100), now).unwrap().unwrap();
    assert_eq!(exact.output(), p(3, 5_000_000_000));
    assert!(!exact.is_before_origin());
    assert!(a.commit_discarded_input(exact).is_err());
    a.commit_input(exact).unwrap();
    let positive = a.prepare_input(p(1, 111), now).unwrap().unwrap();
    assert_eq!(positive.output(), p(3, 5_000_000_011));
    assert!(!positive.is_before_origin());
    a.commit_input(positive).unwrap();
    assert_eq!(a.committed_operation(), Some(p(3, 5_000_000_011)));
}

#[test]
fn discard_retains_original_chronology_and_does_not_make_older_input_legal() {
    let mut a = linear();
    let now = p(1, 130);
    let input = a.prepare_input(p(1, 99), now).unwrap().unwrap();
    a.commit_discarded_input(input).unwrap();
    assert_eq!(
        a.prepare_input(p(1, 98), now),
        Err(AudioAuthorityError::OperationRegression)
    );
    assert_eq!(a.committed_operation(), None);
    assert!(
        a.prepare_input(p(1, 99), now)
            .unwrap()
            .unwrap()
            .is_before_origin(),
        "equal-HOST distinct inputs remain admissible"
    );
}

#[test]
fn discard_tokens_are_revision_and_owner_bound_and_rejection_is_atomic() {
    let mut a = linear();
    let mut foreign = owner(0);
    foreign
        .observe(
            1,
            ClockPair {
                source: p(2, 10),
                target: p(1, 110),
            },
        )
        .unwrap();
    foreign
        .observe(
            1,
            ClockPair {
                source: p(2, 30),
                target: p(1, 130),
            },
        )
        .unwrap();
    foreign.record_acquired_prefix(p(1, 130)).unwrap();
    let token = a.prepare_input(p(1, 99), p(1, 130)).unwrap().unwrap();
    assert_eq!(
        foreign.commit_discarded_input(token),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(foreign.committed_input_host(), None);
    a.record_acquired_prefix(p(1, 131)).unwrap();
    assert_eq!(
        a.commit_discarded_input(token),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(a.committed_input_host(), None);
    assert_eq!(a.committed_operation(), None);
}

#[test]
fn negative_classification_preserves_domain_history_prefix_and_freshness_guards() {
    let a = linear();
    assert_eq!(
        a.prepare_input(p(9, 99), p(1, 130)),
        Err(AudioAuthorityError::DomainMismatch)
    );
    assert_eq!(
        a.prepare_input(p(1, 99), p(9, 130)),
        Err(AudioAuthorityError::DomainMismatch)
    );
    assert_eq!(
        a.prepare_input(p(1, -100_000_000), p(1, 130)),
        Err(AudioAuthorityError::HistoryExpired)
    );
    assert_eq!(
        a.prepare_input(p(1, 99), p(1, 1_000_000_131)).unwrap(),
        None
    );
    assert_eq!(
        a.prepare_input(p(1, 131), p(1, 131)).unwrap(),
        None,
        "unacquired input remains pending"
    );
    assert_eq!(
        a.prepare_input(p(1, 99), p(1, 98)).unwrap(),
        None,
        "future acquired input cannot be discarded"
    );
}

#[test]
fn unknown_correlation_and_unrepresentable_origin_keep_original_authority_refusals() {
    let mut unknown = owner(0);
    unknown.record_acquired_prefix(p(1, 130)).unwrap();
    assert_eq!(unknown.prepare_input(p(1, 99), p(1, 130)).unwrap(), None);
    let mut forbidden = AudioAuthority::new(
        AudioAuthorityConfig {
            input_extrapolation: ExtrapolationPolicy::Forbid,
            ..owner(0).config()
        },
        owner(0).epoch(),
    )
    .unwrap();
    forbidden
        .observe(
            1,
            ClockPair {
                source: p(2, 10),
                target: p(1, 110),
            },
        )
        .unwrap();
    forbidden
        .observe(
            1,
            ClockPair {
                source: p(2, 30),
                target: p(1, 130),
            },
        )
        .unwrap();
    forbidden.record_acquired_prefix(p(1, 130)).unwrap();
    assert_eq!(
        forbidden.prepare_input(p(1, 99), p(1, 130)),
        Err(AudioAuthorityError::HistoryExpired)
    );
    let mut overflowing = owner(i64::MAX);
    assert!(overflowing
        .observe(
            1,
            ClockPair {
                source: p(2, 10),
                target: p(1, 110)
            }
        )
        .is_err());
    assert_eq!(overflowing.history_len(), 0);
    assert_eq!(overflowing.committed_input_host(), None);
    assert_eq!(overflowing.committed_operation(), None);
}
