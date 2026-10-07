//! Observed audio authority with real bounded acquired-input storage.
use crate::{
    audio_authority::*,
    local_input::{InputMerger, MergeError},
};
use beatkernel::{
    input::{ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent},
    time::{
        ClockDomainId, ClockMapper, ClockMappingQuality, ClockPair, ClockPoint, Duration,
        ExtrapolationPolicy, Timestamp, presentation::ObservationAdmission,
    },
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
fn logical(ns: i64) -> ClockPoint {
    point(3, ns)
}
fn observation(raw: i64, host_ns: i64) -> ClockPair {
    ClockPair {
        source: point(2, raw),
        target: host(host_ns),
    }
}
fn epoch() -> AudioAuthorityEpoch {
    AudioAuthorityEpoch {
        id: 1,
        stream_origin: point(2, 1000),
        logical_origin: logical(5000),
        host_domain: ClockDomainId(1),
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
fn owner() -> AudioAuthority {
    AudioAuthority::new(config(), epoch()).unwrap()
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap()
}
fn event(ns: i64, seq: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(ns), seq),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    })
}
fn paired(authority: &mut AudioAuthority) {
    authority.observe(1, observation(1100, 100)).unwrap();
    authority.observe(1, observation(1300, 200)).unwrap();
}
fn state(authority: &AudioAuthority) -> String {
    format!(
        "{:?}",
        (
            authority.config(),
            authority.epoch(),
            authority.latest_observation(),
            authority.acquired_prefix(),
            authority.closed_host_prefix(),
            authority.committed_input_host(),
            authority.committed_operation(),
            authority.committed_presentation(),
            authority.history_len()
        )
    )
}

#[test]
fn future_native_latency_association_keeps_past_covered_evidence_usable() {
    let mut authority = owner();
    let merger = merger();
    paired(&mut authority);
    authority.record_acquired_prefix(host(150)).unwrap();
    let before = state(&authority);
    let input = authority
        .prepare_input(host(125), host(150))
        .unwrap()
        .unwrap();
    assert_eq!(input.original(), host(125));
    assert_eq!(input.output(), logical(5150));
    assert_eq!(input.mapper().quality(), ClockMappingQuality::Unknown);
    let frontier = authority
        .prepare_frontier(host(150), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.observed_host(), host(100));
    assert_eq!(frontier.output(), logical(5100));
    assert_eq!(frontier.advance(), Some(logical(5100)));
    assert_eq!(
        state(&authority),
        before,
        "permission does not dispatch or commit"
    );
    assert_eq!(authority.latest_observation(), Some(observation(1300, 200)));
    authority.record_acquired_prefix(host(200)).unwrap();
    let future_prefix = state(&authority);
    assert!(authority
        .prepare_input(host(125), host(150))
        .unwrap()
        .is_none());
    assert_eq!(state(&authority), future_prefix);
    assert!(authority
        .prepare_frontier(host(150), &merger)
        .unwrap()
        .is_none());

    let mut all_future = owner();
    all_future.observe(1, observation(1100, 200)).unwrap();
    all_future.observe(1, observation(1300, 300)).unwrap();
    all_future.record_acquired_prefix(host(150)).unwrap();
    assert!(all_future
        .prepare_input(host(125), host(150))
        .unwrap()
        .is_none());
    assert!(all_future
        .prepare_frontier(host(150), &merger)
        .unwrap()
        .is_none());

    let mut stale_past = owner();
    stale_past.observe(1, observation(1100, 100)).unwrap();
    stale_past.observe(1, observation(1300, 5000)).unwrap();
    stale_past.record_acquired_prefix(host(1201)).unwrap();
    let stale_before = state(&stale_past);
    assert!(stale_past
        .prepare_input(host(125), host(1201))
        .unwrap()
        .is_none());
    assert!(stale_past
        .prepare_frontier(host(1201), &merger)
        .unwrap()
        .is_none());
    assert_eq!(state(&stale_past), stale_before);
}

#[test]
fn startup_requires_two_real_pairs_then_maps_three_distinct_domains_analytically() {
    let mut authority = owner();
    let mut merger = merger();
    let original = event(125, 0);
    merger.admit(original.clone(), host(200)).unwrap();
    authority.record_acquired_prefix(host(200)).unwrap();
    assert!(
        authority
            .prepare_input(host(125), host(200))
            .unwrap()
            .is_none()
    );
    assert!(
        authority
            .prepare_frontier(host(200), &merger)
            .unwrap()
            .is_none()
    );
    authority.observe(1, observation(1100, 100)).unwrap();
    assert!(
        authority
            .prepare_input(host(125), host(200))
            .unwrap()
            .is_none()
    );
    assert_eq!(merger.pending(), 1);
    authority.observe(1, observation(1300, 200)).unwrap();
    let prepared = authority
        .prepare_input(host(125), host(200))
        .unwrap()
        .unwrap();
    // raw = 1100 + 2*(125-100) = 1150; logical = 5000 + (1150-1000).
    assert_eq!(prepared.original(), host(125));
    assert_eq!(prepared.output(), logical(5150));
    assert_eq!(prepared.epoch(), 1);
    assert_eq!(prepared.mapper().quality(), ClockMappingQuality::Unknown);
    assert_eq!(prepared.mapper().uncertainty(), None);
    assert_eq!(
        prepared.mapper().map(host(125), ClockDomainId(3)),
        Some(Timestamp::from_nanos(5150))
    );
    assert_eq!(
        merger.peek_ready(host(200)).unwrap().unwrap().meta(),
        original.meta()
    );
    assert_eq!(merger.pending(), 1);
    let delivered = merger.pop_ready(host(200)).unwrap().unwrap();
    assert_eq!(delivered, original);
    assert_eq!(delivered.meta().original_clock_point, None);
    authority.commit_input(prepared).unwrap();
    assert_eq!(authority.committed_input_host(), Some(host(125)));
    assert_eq!(authority.committed_operation(), Some(logical(5150)));
    assert_eq!(authority.committed_presentation(), None);
    let frontier = authority
        .prepare_frontier(host(200), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.host(), host(200));
    assert_eq!(frontier.observed_host(), host(200));
    assert_eq!(frontier.output(), logical(5300));
    assert_eq!(frontier.advance(), Some(logical(5300)));
    authority.commit_frontier(frontier, &mut merger).unwrap();
    assert_eq!(authority.committed_presentation(), Some(logical(5300)));
    assert_eq!(authority.committed_operation(), Some(logical(5300)));
}

#[test]
fn prefix_lag_and_equal_timestamp_pending_input_hold_deadline_until_drained() {
    let mut authority = owner();
    let mut merger = merger();
    paired(&mut authority);
    authority.record_acquired_prefix(host(99)).unwrap();
    assert!(
        authority
            .prepare_frontier(host(200), &merger)
            .unwrap()
            .is_none()
    );
    authority.record_acquired_prefix(host(199)).unwrap();
    // HOST100 is covered; HOST200 is not. Select the older real output point,
    // rather than invent output at HOST199 or wait forever for the newest pair.
    let frontier = authority
        .prepare_frontier(host(200), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.host(), host(199));
    assert_eq!(frontier.observed_host(), host(100));
    assert_eq!(frontier.output(), logical(5100));
    assert_eq!(frontier.advance(), Some(logical(5100)));
    let before = state(&authority);
    // The chosen covered pair is stale even while the unacquired newer pair
    // remains within the configured age. Freshness belongs to actual authority.
    assert!(
        authority
            .prepare_frontier(host(1101), &merger)
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&authority), before);
    authority.commit_frontier(frontier, &mut merger).unwrap();
    assert_eq!(authority.committed_presentation(), Some(logical(5100)));
    assert_eq!(authority.closed_host_prefix(), Some(host(199)));
    assert!(matches!(
        merger.admit(event(199, 0), host(200)),
        Err(MergeError::LateInput { .. })
    ));
    merger.admit(event(200, 0), host(200)).unwrap();
    authority.record_acquired_prefix(host(200)).unwrap();
    let before = state(&authority);
    assert!(
        authority
            .prepare_frontier(host(200), &merger)
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&authority), before);
    assert_eq!(merger.pending(), 1);
    let prepared = authority
        .prepare_input(host(200), host(200))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.output(), logical(5300));
    assert_eq!(
        merger
            .pop_ready(host(200))
            .unwrap()
            .unwrap()
            .meta()
            .timestamp,
        host(200).timestamp
    );
    authority.commit_input(prepared).unwrap();
    let frontier = authority
        .prepare_frontier(host(200), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.advance(), None);
    authority.commit_frontier(frontier, &mut merger).unwrap();
    assert_eq!(authority.closed_host_prefix(), Some(host(200)));
    assert!(matches!(
        merger.admit(event(200, 1), host(201)),
        Err(MergeError::LateInput { .. })
    ));
}

#[test]
fn finite_startup_backward_mapping_and_forward_prediction_limits_are_explicit() {
    let mut cfg = config();
    cfg.input_extrapolation = ExtrapolationPolicy::Bounded {
        before: Duration::from_nanos(20),
        after: Duration::from_nanos(100),
    };
    cfg.max_input_ahead = Duration::from_nanos(60);
    let mut authority = AudioAuthority::new(cfg, epoch()).unwrap();
    paired(&mut authority);
    authority.record_acquired_prefix(host(300)).unwrap();
    let backward = authority
        .prepare_input(host(80), host(300))
        .unwrap()
        .unwrap();
    assert_eq!(backward.output(), logical(5060));
    assert_eq!(backward.mapper().quality(), ClockMappingQuality::Unknown);
    // An input before every retained/allowed interval cannot become coverable
    // through later increasing observations; surface expiry instead of dropping it.
    assert!(authority.prepare_input(host(79), host(300)).is_err());
    let ahead = authority
        .prepare_input(host(230), host(300))
        .unwrap()
        .unwrap();
    assert_eq!(ahead.output(), logical(5360));
    assert!(
        authority
            .prepare_input(host(231), host(300))
            .unwrap()
            .is_none()
    );
    let mut forbidden = owner();
    paired(&mut forbidden);
    forbidden.record_acquired_prefix(host(300)).unwrap();
    assert!(forbidden.prepare_input(host(80), host(300)).is_err());
    assert!(
        forbidden
            .prepare_input(host(201), host(300))
            .unwrap()
            .is_none()
    );
    // Independently check the finite HOST envelope with a looser output limit.
    cfg.max_input_ahead = Duration::from_nanos(1000);
    let mut finite = AudioAuthority::new(cfg, epoch()).unwrap();
    paired(&mut finite);
    finite.record_acquired_prefix(host(350)).unwrap();
    assert_eq!(
        finite
            .prepare_input(host(300), host(350))
            .unwrap()
            .unwrap()
            .output(),
        logical(5500)
    );
    assert!(
        finite
            .prepare_input(host(301), host(350))
            .unwrap()
            .is_none()
    );
}

#[test]
fn predicted_input_is_not_presentation_and_actual_catchup_never_advances_backward() {
    let mut cfg = config();
    cfg.input_extrapolation = ExtrapolationPolicy::Bounded {
        before: Duration::ZERO,
        after: Duration::from_nanos(100),
    };
    cfg.max_input_ahead = Duration::from_nanos(100);
    let mut authority = AudioAuthority::new(cfg, epoch()).unwrap();
    let mut merger = merger();
    paired(&mut authority);
    merger.admit(event(230, 0), host(240)).unwrap();
    authority.record_acquired_prefix(host(240)).unwrap();
    let input = authority
        .prepare_input(host(230), host(240))
        .unwrap()
        .unwrap();
    assert_eq!(input.output(), logical(5360));
    merger.pop_ready(host(240)).unwrap().unwrap();
    authority.commit_input(input).unwrap();
    assert_eq!(authority.committed_presentation(), None);
    let frontier = authority
        .prepare_frontier(host(240), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.host(), host(240));
    assert_eq!(frontier.observed_host(), host(200));
    assert_eq!(frontier.output(), logical(5300));
    assert_eq!(frontier.advance(), None);
    authority.commit_frontier(frontier, &mut merger).unwrap();
    assert_eq!(authority.committed_operation(), Some(logical(5360)));
    assert_eq!(authority.committed_presentation(), Some(logical(5300)));
    // Full acquisition prefix closes even though its observation HOST association is earlier.
    assert!(matches!(
        merger.admit(event(235, 1), host(250)),
        Err(MergeError::LateInput { .. })
    ));
    authority.observe(1, observation(1340, 220)).unwrap();
    let frontier = authority
        .prepare_frontier(host(240), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.output(), logical(5340));
    assert_eq!(frontier.advance(), None);
    authority.commit_frontier(frontier, &mut merger).unwrap();
    authority.observe(1, observation(1380, 240)).unwrap();
    let frontier = authority
        .prepare_frontier(host(240), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.advance(), Some(logical(5380)));
    authority.commit_frontier(frontier, &mut merger).unwrap();
    assert_eq!(authority.committed_operation(), Some(logical(5380)));
}

#[test]
fn stationary_absent_and_stale_output_gain_no_authority_from_host_time() {
    let mut authority = owner();
    let mut merger = merger();
    paired(&mut authority);
    authority.record_acquired_prefix(host(200)).unwrap();
    let frontier = authority
        .prepare_frontier(host(200), &merger)
        .unwrap()
        .unwrap();
    authority.commit_frontier(frontier, &mut merger).unwrap();
    let before = state(&authority);
    assert_eq!(
        authority.observe(1, observation(1300, 200)).unwrap(),
        ObservationAdmission::Unchanged
    );
    assert_eq!(
        authority.observe(1, observation(1300, 250)).unwrap(),
        ObservationAdmission::Unchanged
    );
    assert_eq!(state(&authority), before);
    assert!(
        authority
            .prepare_frontier(host(250), &merger)
            .unwrap()
            .is_none()
    );
    assert!(
        authority
            .prepare_frontier(host(1201), &merger)
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&authority), before);
    let mut unclosed = owner();
    paired(&mut unclosed);
    unclosed.record_acquired_prefix(host(200)).unwrap();
    let before = state(&unclosed);
    assert!(
        unclosed
            .prepare_input(host(150), host(1201))
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&unclosed), before);
}

#[test]
fn malformed_observations_prefixes_config_and_rebase_overflow_reject_atomically() {
    let mut authority = owner();
    paired(&mut authority);
    authority.record_acquired_prefix(host(200)).unwrap();
    for (id, pair) in [
        (0, observation(1400, 250)),
        (2, observation(1400, 250)),
        (
            1,
            ClockPair {
                source: point(4, 1400),
                target: host(250),
            },
        ),
        (
            1,
            ClockPair {
                source: point(2, 1400),
                target: point(4, 250),
            },
        ),
        (1, observation(1299, 250)),
        (1, observation(1400, 199)),
    ] {
        let before = state(&authority);
        assert!(authority.observe(id, pair).is_err());
        assert_eq!(state(&authority), before);
    }
    for prefix in [host(199), point(4, 300)] {
        let before = state(&authority);
        assert!(authority.record_acquired_prefix(prefix).is_err());
        assert_eq!(state(&authority), before);
    }
    let before = state(&authority);
    assert!(authority.prepare_input(point(4, 150), host(200)).is_err());
    assert!(authority.prepare_input(host(150), point(4, 200)).is_err());
    assert_eq!(state(&authority), before);
    for capacity in [0, 1, 1025] {
        let mut cfg = config();
        cfg.history_capacity = capacity;
        assert!(AudioAuthority::new(cfg, epoch()).is_err());
    }
    for age in [0, -1] {
        let mut cfg = config();
        cfg.max_observation_age = Duration::from_nanos(age);
        assert!(AudioAuthority::new(cfg, epoch()).is_err());
    }
    let mut cfg = config();
    cfg.max_input_ahead = Duration::from_nanos(-1);
    assert!(AudioAuthority::new(cfg, epoch()).is_err());
    for (before, after) in [(-1, 0), (0, -1)] {
        let mut cfg = config();
        cfg.input_extrapolation = ExtrapolationPolicy::Bounded {
            before: Duration::from_nanos(before),
            after: Duration::from_nanos(after),
        };
        assert!(AudioAuthority::new(cfg, epoch()).is_err());
    }
    let bad = AudioAuthorityEpoch {
        stream_origin: host(1000),
        ..epoch()
    };
    assert!(AudioAuthority::new(config(), bad).is_err());
    for domain in [1, 2] {
        let mut bad = epoch();
        bad.logical_origin = point(domain, 5000);
        assert!(AudioAuthority::new(config(), bad).is_err());
    }
    let extremes = AudioAuthorityEpoch {
        id: 1,
        stream_origin: point(2, i64::MIN),
        logical_origin: logical(i64::MAX),
        host_domain: ClockDomainId(1),
    };
    let mut overflow = AudioAuthority::new(config(), extremes).unwrap();
    let before = state(&overflow);
    assert!(overflow.observe(1, observation(i64::MIN + 1, 100)).is_err());
    assert_eq!(state(&overflow), before);
}

#[test]
fn pinned_history_refuses_capacity_then_closed_prefix_retires_only_unneeded_segment() {
    let mut cfg = config();
    cfg.history_capacity = 2;
    let mut authority = AudioAuthority::new(cfg, epoch()).unwrap();
    let mut merger = merger();
    paired(&mut authority);
    merger.admit(event(125, 0), host(200)).unwrap();
    authority.record_acquired_prefix(host(200)).unwrap();
    let before = state(&authority);
    assert!(authority.observe(1, observation(1500, 300)).is_err());
    assert_eq!(state(&authority), before);
    assert_eq!(authority.history_len(), 2);
    assert_eq!(merger.pending(), 1);
    let prepared = authority
        .prepare_input(host(125), host(200))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.output(), logical(5150));
    merger.pop_ready(host(200)).unwrap().unwrap();
    authority.commit_input(prepared).unwrap();
    let frontier = authority
        .prepare_frontier(host(200), &merger)
        .unwrap()
        .unwrap();
    authority.commit_frontier(frontier, &mut merger).unwrap();
    authority.observe(1, observation(1500, 300)).unwrap();
    assert_eq!(authority.history_len(), 2);
    merger.admit(event(250, 1), host(300)).unwrap();
    authority.record_acquired_prefix(host(300)).unwrap();
    assert_eq!(
        merger
            .peek_ready(host(300))
            .unwrap()
            .unwrap()
            .meta()
            .timestamp,
        host(250).timestamp
    );
    let prepared = authority
        .prepare_input(host(250), host(300))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.output(), logical(5400));
    merger.pop_ready(host(300)).unwrap().unwrap();
    authority.commit_input(prepared).unwrap();
    assert!(authority.prepare_input(host(125), host(300)).is_err());
}

#[test]
fn future_observations_acquisition_prefix_and_event_occurrence_cannot_authorize_now() {
    let mut authority = owner();
    paired(&mut authority);
    authority.record_acquired_prefix(host(200)).unwrap();
    let mut merger = merger();
    let original = event(150, 0);
    merger.admit(original.clone(), host(200)).unwrap();
    let before = state(&authority);
    // The second anchor is still in this query's future; it cannot supply a
    // correlation for an otherwise already occurred HOST150 event.
    assert!(
        authority
            .prepare_input(host(150), host(175))
            .unwrap()
            .is_none()
    );
    assert!(
        authority
            .prepare_frontier(host(199), &merger)
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&authority), before);
    assert_eq!(merger.peek_ready(host(200)).unwrap(), Some(&original));
    assert_eq!(merger.pending(), 1);
    let prepared = authority
        .prepare_input(host(150), host(200))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.output(), logical(5200));
    merger.pop_ready(host(200)).unwrap().unwrap();
    authority.commit_input(prepared).unwrap();
    let mut cfg = config();
    cfg.input_extrapolation = ExtrapolationPolicy::Bounded {
        before: Duration::ZERO,
        after: Duration::from_nanos(100),
    };
    cfg.max_input_ahead = Duration::from_nanos(100);
    let mut predicted = AudioAuthority::new(cfg, epoch()).unwrap();
    paired(&mut predicted);
    let mut future_merger =
        InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap();
    future_merger.admit(event(230, 0), host(240)).unwrap();
    predicted.record_acquired_prefix(host(240)).unwrap();
    let before = state(&predicted);
    assert!(
        predicted
            .prepare_input(host(230), host(220))
            .unwrap()
            .is_none()
    );
    assert!(
        predicted
            .prepare_frontier(host(220), &future_merger)
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&predicted), before);
    assert_eq!(future_merger.pending(), 1);
    let prepared = predicted
        .prepare_input(host(230), host(240))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.output(), logical(5360));
    future_merger.pop_ready(host(240)).unwrap().unwrap();
    predicted.commit_input(prepared).unwrap();
    let before = state(&predicted);
    // Even after input was dispatched, a full prefix acquired at HOST240 may
    // not close through a declared HOST220 query time.
    assert!(
        predicted
            .prepare_frontier(host(220), &future_merger)
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&predicted), before);
}

#[test]
fn prepared_inputs_reject_stale_and_cross_owner_mapping_but_accept_identical_semantics() {
    let mut a = owner();
    paired(&mut a);
    a.record_acquired_prefix(host(200)).unwrap();
    let prepared = a.prepare_input(host(125), host(200)).unwrap().unwrap();
    let mut different = owner();
    different.observe(1, observation(1200, 100)).unwrap();
    different.observe(1, observation(1400, 200)).unwrap();
    different.record_acquired_prefix(host(200)).unwrap();
    let before = state(&different);
    assert!(different.commit_input(prepared.clone()).is_err());
    assert_eq!(state(&different), before);
    let mut same = owner();
    paired(&mut same);
    same.record_acquired_prefix(host(200)).unwrap();
    same.commit_input(prepared.clone()).unwrap();
    assert_eq!(same.committed_operation(), Some(logical(5150)));
    a.record_acquired_prefix(host(201)).unwrap();
    let before = state(&a);
    assert!(a.commit_input(prepared).is_err());
    assert_eq!(state(&a), before);
}

#[test]
fn delayed_original_input_uses_its_retained_segment_after_newer_rates_arrive() {
    let mut authority = owner();
    let mut merger = merger();
    paired(&mut authority);
    merger.admit(event(125, 0), host(400)).unwrap();
    authority.record_acquired_prefix(host(400)).unwrap();
    authority.observe(1, observation(1600, 300)).unwrap();
    authority.observe(1, observation(1800, 400)).unwrap();
    assert_eq!(authority.history_len(), 4);
    let prepared = authority
        .prepare_input(host(125), host(400))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.original(), host(125));
    assert_eq!(prepared.output(), logical(5150));
    assert_eq!(prepared.mapper().quality(), ClockMappingQuality::Unknown);
    let before = state(&authority);
    assert!(authority.observe(1, observation(2000, 500)).is_err());
    assert_eq!(state(&authority), before);
    assert_eq!(merger.pending(), 1);
    assert_eq!(
        merger
            .peek_ready(host(400))
            .unwrap()
            .unwrap()
            .meta()
            .timestamp,
        host(125).timestamp
    );
    merger.pop_ready(host(400)).unwrap().unwrap();
    authority.commit_input(prepared).unwrap();
    let frontier = authority
        .prepare_frontier(host(400), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.output(), logical(5800));
    authority.commit_frontier(frontier, &mut merger).unwrap();
    authority.observe(1, observation(2000, 500)).unwrap();
    assert!(authority.history_len() <= 4);
    assert_eq!(authority.committed_input_host(), Some(host(125)));
}

#[test]
fn prepared_frontiers_recheck_pending_input_and_owner_semantics_before_atomic_commit() {
    let mut authority = owner();
    paired(&mut authority);
    authority.record_acquired_prefix(host(200)).unwrap();
    let mut merger = merger();
    let prepared = authority
        .prepare_frontier(host(200), &merger)
        .unwrap()
        .unwrap();
    merger.admit(event(150, 0), host(200)).unwrap();
    let before = state(&authority);
    assert!(authority.commit_frontier(prepared, &mut merger).is_err());
    assert_eq!(state(&authority), before);
    assert_eq!(merger.pending(), 1);
    assert_eq!(
        merger
            .peek_ready(host(200))
            .unwrap()
            .unwrap()
            .meta()
            .timestamp,
        host(150).timestamp
    );
    let prepared = authority
        .prepare_input(host(150), host(200))
        .unwrap()
        .unwrap();
    merger.pop_ready(host(200)).unwrap();
    authority.commit_input(prepared).unwrap();
    let frontier = authority
        .prepare_frontier(host(200), &merger)
        .unwrap()
        .unwrap();
    authority.record_acquired_prefix(host(201)).unwrap();
    let before = state(&authority);
    assert!(authority.commit_frontier(frontier, &mut merger).is_err());
    assert_eq!(state(&authority), before);
    merger.admit(event(201, 1), host(201)).unwrap();
}

#[test]
fn same_epoch_correlation_restart_preserves_watermarks_and_requires_fresh_two_anchors() {
    let mut authority = owner();
    let mut merger = merger();
    paired(&mut authority);
    authority.record_acquired_prefix(host(200)).unwrap();
    merger.admit(event(150, 0), host(200)).unwrap();
    let input = authority
        .prepare_input(host(150), host(200))
        .unwrap()
        .unwrap();
    assert_eq!(
        merger
            .peek_ready(host(200))
            .unwrap()
            .unwrap()
            .meta()
            .timestamp,
        host(150).timestamp
    );
    merger.pop_ready(host(200)).unwrap().unwrap();
    authority.commit_input(input).unwrap();
    let frontier = authority
        .prepare_frontier(host(200), &merger)
        .unwrap()
        .unwrap();
    authority.commit_frontier(frontier, &mut merger).unwrap();
    let retained = (
        authority.epoch(),
        authority.config(),
        authority.acquired_prefix(),
        authority.closed_host_prefix(),
        authority.committed_input_host(),
        authority.committed_operation(),
        authority.committed_presentation(),
    );
    let restart = authority.prepare_correlation_restart(&merger).unwrap();
    authority
        .commit_correlation_restart(restart, &merger)
        .unwrap();
    assert_eq!(authority.history_len(), 0);
    assert_eq!(authority.latest_observation(), None);
    assert_eq!(
        (
            authority.epoch(),
            authority.config(),
            authority.acquired_prefix(),
            authority.closed_host_prefix(),
            authority.committed_input_host(),
            authority.committed_operation(),
            authority.committed_presentation()
        ),
        retained
    );
    authority.record_acquired_prefix(host(350)).unwrap();
    assert!(
        authority
            .prepare_input(host(300), host(350))
            .unwrap()
            .is_none()
    );
    authority.observe(1, observation(1400, 250)).unwrap();
    assert!(
        authority
            .prepare_input(host(300), host(350))
            .unwrap()
            .is_none()
    );
    authority.observe(1, observation(1600, 350)).unwrap();
    assert_eq!(
        authority
            .prepare_input(host(300), host(350))
            .unwrap()
            .unwrap()
            .output(),
        logical(5500)
    );
    assert!(matches!(
        merger.admit(event(190, 0), host(350)),
        Err(MergeError::LateInput { .. })
    ));
}

#[test]
fn epoch_replacement_rechecks_pending_and_origins_then_retires_old_observations() {
    let mut cfg = config();
    cfg.input_extrapolation = ExtrapolationPolicy::Bounded {
        before: Duration::ZERO,
        after: Duration::from_nanos(100),
    };
    cfg.max_input_ahead = Duration::from_nanos(100);
    let mut authority = AudioAuthority::new(cfg, epoch()).unwrap();
    let mut merger = merger();
    paired(&mut authority);
    authority.record_acquired_prefix(host(200)).unwrap();
    let frontier = authority
        .prepare_frontier(host(200), &merger)
        .unwrap()
        .unwrap();
    authority.commit_frontier(frontier, &mut merger).unwrap();
    let next = AudioAuthorityEpoch {
        id: 2,
        stream_origin: point(4, 0),
        logical_origin: logical(5400),
        host_domain: ClockDomainId(1),
    };
    for invalid in [
        AudioAuthorityEpoch { id: 1, ..next },
        AudioAuthorityEpoch {
            logical_origin: logical(5299),
            ..next
        },
        AudioAuthorityEpoch {
            logical_origin: point(5, 5300),
            ..next
        },
    ] {
        let before = state(&authority);
        assert!(authority.prepare_epoch(invalid, &merger).is_err());
        assert_eq!(state(&authority), before);
    }
    let prepared = authority.prepare_epoch(next, &merger).unwrap();
    merger.admit(event(250, 0), host(250)).unwrap();
    let before = state(&authority);
    assert!(authority.commit_epoch(prepared, &merger).is_err());
    assert_eq!(state(&authority), before);
    assert!(authority.prepare_epoch(next, &merger).is_err());
    assert_eq!(merger.pending(), 1);
    authority.record_acquired_prefix(host(250)).unwrap();
    let input = authority
        .prepare_input(host(250), host(250))
        .unwrap()
        .unwrap();
    assert_eq!(input.output(), logical(5400));
    merger.pop_ready(host(250)).unwrap().unwrap();
    authority.commit_input(input).unwrap();
    let prepared = authority.prepare_epoch(next, &merger).unwrap();
    authority.commit_epoch(prepared, &merger).unwrap();
    assert_eq!(authority.epoch(), next);
    assert_eq!(authority.history_len(), 0);
    let before = state(&authority);
    assert!(authority.observe(1, observation(1700, 400)).is_err());
    assert_eq!(state(&authority), before);
    authority.record_acquired_prefix(host(400)).unwrap();
    authority
        .observe(
            2,
            ClockPair {
                source: point(4, 10),
                target: host(300),
            },
        )
        .unwrap();
    assert!(
        authority
            .prepare_input(host(350), host(400))
            .unwrap()
            .is_none()
    );
    authority
        .observe(
            2,
            ClockPair {
                source: point(4, 110),
                target: host(400),
            },
        )
        .unwrap();
    assert_eq!(
        authority
            .prepare_input(host(350), host(400))
            .unwrap()
            .unwrap()
            .output(),
        logical(5460)
    );
}

#[test]
fn merger_source_chronology_capacity_and_peek_faults_preserve_exact_pending_events() {
    let authority = owner();
    let before = state(&authority);
    let mut merger = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 2).unwrap();
    let first = event(125, 1);
    let second = event(150, 2);
    merger.admit(first.clone(), host(200)).unwrap();
    merger.admit(second.clone(), host(200)).unwrap();
    let mut unknown = event(160, 3);
    unknown.meta_mut().source = DeviceId(10);
    let mut wrong_domain = event(160, 3);
    wrong_domain.meta_mut().clock_domain = ClockDomainId(4);
    for rejected in [
        unknown,
        wrong_domain,
        event(124, 3),
        event(160, 1),
        event(160, 3),
    ] {
        assert!(merger.admit(rejected, host(200)).is_err());
        assert_eq!(merger.pending(), 2);
        assert_eq!(merger.peek_ready(host(200)).unwrap(), Some(&first));
    }
    assert!(merger.peek_ready(point(4, 200)).is_err());
    assert_eq!(merger.pending(), 2);
    assert_eq!(merger.pop_ready(host(200)).unwrap(), Some(first));
    assert_eq!(merger.pop_ready(host(200)).unwrap(), Some(second));
    assert_eq!(state(&authority), before);
}

#[test]
fn restart_and_epoch_descriptors_reject_pending_stale_and_cross_owner_state() {
    let mut authority = owner();
    paired(&mut authority);
    authority.record_acquired_prefix(host(200)).unwrap();
    let mut merger = merger();
    let restart = authority.prepare_correlation_restart(&merger).unwrap();
    merger.admit(event(150, 0), host(200)).unwrap();
    let before = state(&authority);
    assert!(
        authority
            .commit_correlation_restart(restart, &merger)
            .is_err()
    );
    assert_eq!(state(&authority), before);
    assert_eq!(merger.pending(), 1);
    assert!(authority.prepare_correlation_restart(&merger).is_err());
    let input = authority
        .prepare_input(host(150), host(200))
        .unwrap()
        .unwrap();
    merger.pop_ready(host(200)).unwrap().unwrap();
    authority.commit_input(input).unwrap();
    let restart = authority.prepare_correlation_restart(&merger).unwrap();
    authority.record_acquired_prefix(host(201)).unwrap();
    let before = state(&authority);
    assert!(
        authority
            .commit_correlation_restart(restart, &merger)
            .is_err()
    );
    assert_eq!(state(&authority), before);
    let mut different = owner();
    different.observe(1, observation(1200, 100)).unwrap();
    different.observe(1, observation(1400, 200)).unwrap();
    different.record_acquired_prefix(host(201)).unwrap();
    let restart = authority.prepare_correlation_restart(&merger).unwrap();
    let before = state(&different);
    assert!(
        different
            .commit_correlation_restart(restart, &merger)
            .is_err()
    );
    assert_eq!(state(&different), before);
    let next = AudioAuthorityEpoch {
        id: 2,
        stream_origin: point(4, 0),
        logical_origin: logical(6000),
        host_domain: ClockDomainId(1),
    };
    let replacement = authority.prepare_epoch(next, &merger).unwrap();
    let before = state(&different);
    assert!(different.commit_epoch(replacement, &merger).is_err());
    assert_eq!(state(&different), before);
    let frontier = different
        .prepare_frontier(host(201), &merger)
        .unwrap()
        .unwrap();
    let before = state(&authority);
    assert!(authority.commit_frontier(frontier, &mut merger).is_err());
    assert_eq!(state(&authority), before);
    // A refused cross-owner frontier must not close the shared merger.
    merger.admit(event(151, 1), host(201)).unwrap();
    assert_eq!(merger.pending(), 1);
}

#[test]
fn exhausted_revision_refuses_input_and_frontier_permission_before_caller_effects() {
    let mut input_owner = owner();
    paired(&mut input_owner);
    input_owner.record_acquired_prefix(host(200)).unwrap();
    let mut queued = merger();
    let original = event(150, 0);
    queued.admit(original.clone(), host(200)).unwrap();
    // Fault injection of the finite transaction counter after genuine cold
    // setup. No permission may escape if its eventual commit cannot advance.
    input_owner.revision = u64::MAX;
    let before = state(&input_owner);
    assert!(matches!(
        input_owner.prepare_input(host(150), host(200)),
        Err(AudioAuthorityError::Overflow)
    ));
    assert_eq!(state(&input_owner), before);
    assert_eq!(input_owner.revision, u64::MAX);
    assert_eq!(queued.pending(), 1);
    assert_eq!(queued.peek_ready(host(200)).unwrap(), Some(&original));
    assert_eq!(input_owner.committed_operation(), None);
    let mut frontier_owner = owner();
    paired(&mut frontier_owner);
    frontier_owner.record_acquired_prefix(host(200)).unwrap();
    let mut drained = merger();
    frontier_owner.revision = u64::MAX;
    let before = state(&frontier_owner);
    assert!(matches!(
        frontier_owner.prepare_frontier(host(200), &drained),
        Err(AudioAuthorityError::Overflow)
    ));
    assert_eq!(state(&frontier_owner), before);
    assert_eq!(frontier_owner.revision, u64::MAX);
    assert_eq!(frontier_owner.committed_presentation(), None);
    assert_eq!(frontier_owner.closed_host_prefix(), None);
    // Failed preparation must not close the real merger or lose equal-prefix input.
    drained.admit(event(200, 0), host(200)).unwrap();
    assert_eq!(drained.pending(), 1);
}
