use crate::{
    native_converted_gameplay_fixtures::{native, point, rig},
    native_end::NativeEnd,
};
use beatkernel::{audio::*, time::ClockDomainId};

fn pair(basis: TargetFrameBasis, frame: u64) -> beatkernel::time::ClockPair {
    native(
        basis,
        frame,
        1_000_000_000 + (u128::from(frame) * 1_000_000_000 / 48_000) as i64,
    )
    .1
}

#[test]
fn finite_source_lookahead_and_coalesced_held_reports_wait_for_strict_exclusive_target_presentation(
) {
    let (_producer, mut output) = rig(44_100, 48_000, None, Some(3), 0);
    let basis = output.target_frame_basis();
    let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 44_100, 3)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
    assert_eq!(
        end.observe_target(7, basis, output.boundaries(), None, pair(basis, 0))
            .unwrap(),
        None
    );
    let report = output.render_pending(8).unwrap();
    assert!(report.source.unwrap().paused);
    assert_eq!(report.source.unwrap().playback_end_physical_frame, Some(3));
    assert!(output.pending_samples()[..4]
        .iter()
        .all(|sample| *sample > 0.0));
    assert_eq!(&output.pending_samples()[4..], &[0.; 4]);
    let facts = output.boundaries();
    assert_eq!(
        facts.end.unwrap().target_time,
        TargetTime::from_frames(4, 48_000).unwrap()
    );
    assert_eq!(
        end.observe_target(
            7,
            basis,
            facts,
            output.last_real_source_report(),
            pair(basis, 3)
        )
        .unwrap(),
        None
    );
    output.admit(8).unwrap();
    let real = output.last_real_source_report();
    output.render_held_pending(4).unwrap();
    assert_eq!(output.last_real_source_report(), real);
    assert_eq!(output.boundaries().end, facts.end);
    let boundary = end
        .observe_target(
            7,
            basis,
            output.boundaries(),
            output.last_real_source_report(),
            pair(basis, 4),
        )
        .unwrap()
        .unwrap();
    assert_eq!(boundary.physical_frame, 3);
    assert_eq!(boundary.playback_frame, 3);
    assert_eq!(boundary.output, point(2, 4 * 1_000_000_000 / 48_000));
    assert_eq!(
        boundary.host,
        point(1, 1_000_000_000 + 4 * 1_000_000_000 / 48_000)
    );
    assert_eq!(
        end.observe_target(
            7,
            basis,
            output.boundaries(),
            output.last_real_source_report(),
            pair(basis, 5)
        )
        .unwrap(),
        None
    );
    let before = format!("{end:?}");
    assert!(end
        .restart_for_target_output(8, &output, pair(basis, 5))
        .is_err());
    assert_eq!(format!("{end:?}"), before);
}

#[test]
fn wrong_target_epoch_domain_basis_or_changed_finite_marker_refuse_without_losing_native_lower() {
    let (_producer, mut output) = rig(44_100, 48_000, None, Some(3), 0);
    let basis = output.target_frame_basis();
    let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 44_100, 3)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
    end.observe_target(7, basis, output.boundaries(), None, pair(basis, 0))
        .unwrap();
    output.render_pending(8).unwrap();
    let facts = output.boundaries();
    let source = output.last_real_source_report();
    let before = format!("{end:?}");
    assert!(end
        .observe_target(6, basis, facts, source, pair(basis, 4))
        .is_err());
    assert_eq!(format!("{end:?}"), before);
    let altered = TargetFrameBasis::new(basis.origin(), basis.start_time(), 32_000).unwrap();
    assert!(end
        .observe_target(7, altered, facts, source, pair(basis, 4))
        .is_err());
    assert_eq!(format!("{end:?}"), before);
    let mut invalid = pair(basis, 4);
    invalid.source.domain = ClockDomainId(99);
    assert!(end
        .observe_target(7, basis, facts, source, invalid)
        .is_err());
    assert_eq!(format!("{end:?}"), before);
    let mut invalid = facts;
    invalid.end.as_mut().unwrap().source_frame = 4;
    assert!(end
        .observe_target(7, basis, invalid, source, pair(basis, 4))
        .is_err());
    assert_eq!(format!("{end:?}"), before);
    assert!(end
        .observe_target(7, basis, facts, source, pair(basis, 4))
        .unwrap()
        .is_some());
}

#[test]
fn recovered_finite_suffix_restarts_on_exact_fractional_first_unsent_target_basis() {
    let (_producer, mut output) = rig(44_100, 48_000, None, Some(3), 0);
    let original = output.target_frame_basis();
    let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 44_100, 3)
        .unwrap()
        .with_target_basis(7, original)
        .unwrap();
    end.observe_target(7, original, output.boundaries(), None, pair(original, 0))
        .unwrap();
    output.render_pending(8).unwrap();
    output.admit(2).unwrap();
    let basis = output.target_frame_basis();
    assert_eq!(
        basis.start_time(),
        TargetTime::from_frames(2, 48_000).unwrap()
    );
    let before = format!("{end:?}");
    assert!(end
        .restart_for_target_output(7, &output, pair(basis, 0))
        .is_err());
    assert_eq!(format!("{end:?}"), before);
    let mut restarted = end
        .restart_for_target_output(8, &output, pair(basis, 0))
        .unwrap();
    assert_eq!(
        restarted
            .observe_target(
                8,
                basis,
                output.boundaries(),
                output.last_real_source_report(),
                pair(basis, 1)
            )
            .unwrap(),
        None
    );
    let boundary = restarted
        .observe_target(
            8,
            basis,
            output.boundaries(),
            output.last_real_source_report(),
            pair(basis, 2),
        )
        .unwrap()
        .unwrap();
    assert_eq!(boundary.output, point(2, 4 * 1_000_000_000 / 48_000));
    assert_eq!(boundary.physical_frame, 3);
}

#[test]
fn zero_length_finite_session_needs_real_endpoint_fact_then_emits_once_without_positive_pcm() {
    let (_producer, mut output) = rig(44_100, 48_000, None, Some(0), 0);
    let basis = output.target_frame_basis();
    let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 44_100, 0)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
    assert_eq!(
        end.observe_target(7, basis, output.boundaries(), None, pair(basis, 0))
            .unwrap(),
        None
    );
    let report = output.render_pending(8).unwrap();
    assert!(report.source.unwrap().frames > 0);
    assert_eq!(report.source.unwrap().playback_frames, 0);
    assert!(output.pending_samples().iter().all(|sample| *sample == 0.0));
    let boundary = end
        .observe_target(
            7,
            basis,
            output.boundaries(),
            output.last_real_source_report(),
            pair(basis, 0),
        )
        .unwrap()
        .unwrap();
    assert_eq!(boundary.physical_frame, 0);
    assert_eq!(boundary.playback_frame, 0);
    assert_eq!(boundary.output, point(2, 0));
    assert_eq!(
        end.observe_target(
            7,
            basis,
            output.boundaries(),
            output.last_real_source_report(),
            pair(basis, 1)
        )
        .unwrap(),
        None
    );
}

#[test]
fn cold_finite_restart_prepares_before_worker_move_defers_old_tail_then_uses_fresh_native_endpoint_facts(
) {
    use beatkernel_platform::audio::{DeviceFormat, SampleEncoding};
    let (mut producer, mut output) = rig(24_000, 48_000, None, Some(100), 0);
    let initial = output.target_frame_basis();
    let relation = |basis: TargetFrameBasis, frame| {
        native(
            basis,
            frame,
            1_000_000_000
                + basis
                    .point_at_stream_frame(frame)
                    .unwrap()
                    .timestamp
                    .as_nanos(),
        )
        .1
    };
    let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 24_000, 100)
        .unwrap()
        .with_target_basis(7, initial)
        .unwrap();
    end.observe_target(7, initial, output.boundaries(), None, relation(initial, 0))
        .unwrap();
    output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    producer.request_pause(true);
    output.render_pending(8).unwrap();
    output.admit(8).unwrap();
    output
        .reconfigure(
            DeviceFormat::new(32_000, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            128,
        )
        .unwrap();
    let retained = output.render_held_pending(3).unwrap();
    output.admit(1).unwrap();
    let planned = output.target_frame_basis();
    let original = format!("{end:?}");
    let mut prepared = end
        .prepare_restart_for_target_output(8, &output, planned)
        .unwrap();
    assert_eq!(format!("{end:?}"), original);
    let cold = format!("{prepared:?}");
    assert!(!prepared
        .target_replacement_observation_ready(Some(retained), relation(planned, 0))
        .unwrap());
    assert_eq!(format!("{prepared:?}"), cold);
    assert!(!prepared
        .target_replacement_observation_ready(Some(retained), relation(planned, 2))
        .unwrap());
    assert_eq!(format!("{prepared:?}"), cold);
    // The end candidate is prepared while software ownership is still cold;
    // only then does the complete converter/pending suffix enter a worker.
    let worker = std::thread::spawn(move || {
        output.admit(2).unwrap();
        let fresh = output.render_held_pending(2).unwrap();
        (output, fresh)
    });
    let (mut output, fresh) = worker.join().unwrap();
    assert!(prepared
        .target_replacement_observation_ready(Some(fresh), relation(planned, 2))
        .unwrap());
    assert_eq!(
        prepared
            .observe_target(
                8,
                planned,
                output.boundaries(),
                output.last_real_source_report(),
                relation(planned, 2)
            )
            .unwrap(),
        None
    );
    output.admit(2).unwrap();
    producer.request_pause(false);
    output.render_pending(128).unwrap();
    output.admit(128).unwrap();
    assert_eq!(
        prepared
            .observe_target(
                8,
                planned,
                output.boundaries(),
                output.last_real_source_report(),
                relation(planned, 3)
            )
            .unwrap(),
        None
    );
    output.render_pending(8).unwrap();
    let facts = output.boundaries();
    let actual = output.last_real_source_report();
    assert_eq!(facts.end.unwrap().source_frame, 104);
    assert_eq!(
        facts.end.unwrap().target_time,
        TargetTime::from_frames(144, 32_000).unwrap()
    );
    // Consumed source position is 100.5 at this block's start. Its exclusive
    // endpoint 104 is ceil((104-100.5)/(24000/32000))=5 target frames later.
    assert!(output.pending_samples()[..5]
        .iter()
        .all(|sample| *sample > 0.0));
    assert_eq!(&output.pending_samples()[5..], &[0.; 3]);
    assert_eq!(
        prepared
            .observe_target(8, planned, facts, actual, relation(planned, 136))
            .unwrap(),
        None
    );
    let boundary = prepared
        .observe_target(8, planned, facts, actual, relation(planned, 137))
        .unwrap()
        .unwrap();
    assert_eq!(boundary.physical_frame, 104);
    assert_eq!(boundary.playback_frame, 100);
    assert_eq!(boundary.output, point(2, 4_500_000));
    assert_eq!(
        prepared
            .observe_target(8, planned, facts, actual, relation(planned, 138))
            .unwrap(),
        None
    );
}

#[test]
fn primed_target_completion_survives_invalid_observations_and_delivers_original_boundary_once() {
    let (_producer, mut output) = rig(44_100, 48_000, None, Some(3), 0);
    let basis = output.target_frame_basis();
    let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 44_100, 3)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
    end.prime_target(7, basis, output.boundaries(), None, pair(basis, 0))
        .unwrap();
    let generated = output.render_pending(8).unwrap();
    assert_eq!(generated.source.unwrap().playback_frames, 3);
    let facts = output.boundaries();
    let source = output.last_real_source_report();
    assert_eq!(
        facts.end.unwrap().target_time,
        TargetTime::from_frames(4, 48_000).unwrap()
    );
    output.admit(8).unwrap();
    end.prime_target(7, basis, facts, source, pair(basis, 6))
        .unwrap();
    let primed = end.clone();
    assert!(end
        .prime_target(7, basis, facts, source, pair(basis, 7))
        .is_err());
    assert_eq!(end, primed);

    assert!(end
        .observe_target(8, basis, facts, source, pair(basis, 8))
        .is_err());
    assert_eq!(end, primed);
    let changed_basis = TargetFrameBasis::new(basis.origin(), basis.start_time(), 32_000).unwrap();
    assert!(end
        .observe_target(7, changed_basis, facts, source, pair(basis, 8))
        .is_err());
    assert_eq!(end, primed);
    for source_domain in [true, false] {
        let mut invalid = pair(basis, 8);
        if source_domain {
            invalid.source.domain = ClockDomainId(99);
        } else {
            invalid.target.domain = ClockDomainId(99);
        }
        assert!(end
            .observe_target(7, basis, facts, source, invalid)
            .is_err());
        assert_eq!(end, primed);
    }
    let mut changed = facts;
    changed.source_rate = 24_000;
    assert!(end
        .observe_target(7, basis, changed, source, pair(basis, 8))
        .is_err());
    assert_eq!(end, primed);
    changed = facts;
    changed.end.as_mut().unwrap().target_time = TargetTime::from_frames(5, 48_000).unwrap();
    assert!(end
        .observe_target(7, basis, changed, source, pair(basis, 8))
        .is_err());
    assert_eq!(end, primed);
    changed = facts;
    changed.end.as_mut().unwrap().source_frame = 4;
    assert!(end
        .observe_target(7, basis, changed, source, pair(basis, 8))
        .is_err());
    assert_eq!(end, primed);
    let mut changed_source = source.unwrap();
    changed_source.playback_end_physical_frame = Some(4);
    assert!(end
        .observe_target(7, basis, facts, Some(changed_source), pair(basis, 8))
        .is_err());
    assert_eq!(end, primed);

    // A coalesced held callback supplies no replacement source evidence. The
    // deferred startup result retains the original frame-0/frame-6 bracket.
    output.render_held_pending(4).unwrap();
    assert_eq!(output.last_real_source_report(), source);
    let boundary = end
        .observe_target(7, basis, output.boundaries(), source, pair(basis, 8))
        .unwrap()
        .unwrap();
    assert_eq!(
        boundary,
        crate::native_end::EndBoundary {
            physical_frame: 3,
            playback_frame: 3,
            output: point(2, 4 * 1_000_000_000 / 48_000),
            host: point(1, 1_000_000_000 + 4 * 1_000_000_000 / 48_000),
        }
    );
    assert_eq!(
        end.observe_target(7, basis, output.boundaries(), source, pair(basis, 9))
            .unwrap(),
        None
    );
    let delivered = end.clone();
    assert!(end
        .prime_target(7, basis, facts, source, pair(basis, 10))
        .is_err());
    assert_eq!(end, delivered);
}

#[test]
fn primed_zero_and_single_source_frame_completion_remain_pending_until_valid_delivery() {
    for source_end in [0, 1] {
        let (_producer, mut output) = rig(44_100, 48_000, None, Some(source_end), 0);
        let basis = output.target_frame_basis();
        let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 44_100, source_end)
            .unwrap()
            .with_target_basis(7, basis)
            .unwrap();
        end.prime_target(7, basis, output.boundaries(), None, pair(basis, 0))
            .unwrap();
        let rendered = output.render_pending(8).unwrap();
        assert_eq!(
            rendered.source.unwrap().playback_frames,
            source_end as usize
        );
        let facts = output.boundaries();
        let source = output.last_real_source_report();
        let endpoint_frame = if source_end == 0 { 0 } else { 2 };
        assert_eq!(
            facts.end.unwrap().target_time,
            TargetTime::from_frames(endpoint_frame, 48_000).unwrap()
        );
        if source_end == 0 {
            assert!(output.pending_samples().iter().all(|sample| *sample == 0.0));
        }
        output.admit(8).unwrap();
        end.prime_target(7, basis, facts, source, pair(basis, endpoint_frame))
            .unwrap();
        let pending = end.clone();
        assert!(end
            .prime_target(7, basis, facts, source, pair(basis, endpoint_frame + 1))
            .is_err());
        assert_eq!(end, pending);
        assert!(end
            .observe_target(8, basis, facts, source, pair(basis, endpoint_frame + 1))
            .is_err());
        assert_eq!(end, pending);
        let boundary = end
            .observe_target(7, basis, facts, source, pair(basis, endpoint_frame + 1))
            .unwrap()
            .unwrap();
        assert_eq!(boundary.physical_frame, source_end);
        assert_eq!(boundary.playback_frame, source_end);
        assert_eq!(boundary.output, pair(basis, endpoint_frame).source);
        assert_eq!(boundary.host, pair(basis, endpoint_frame).target);
        assert_eq!(
            end.observe_target(7, basis, facts, source, pair(basis, endpoint_frame + 2))
                .unwrap(),
            None
        );
    }
}

#[test]
fn clock_only_target_priming_preserves_first_native_lower_until_delayed_endpoint_metadata() {
    for (unavailable_frame, crossing_frame) in [(2, 4), (5, 6)] {
        let (_producer, mut output) = rig(44_100, 48_000, None, Some(3), 0);
        let basis = output.target_frame_basis();
        let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 44_100, 3)
            .unwrap()
            .with_target_basis(7, basis)
            .unwrap();
        assert!(output.last_real_source_report().is_none());
        assert!(output.boundaries().end.is_none());
        end.prime_target_clock(7, basis, pair(basis, 0)).unwrap();
        // Auxiliary source/endpoint telemetry is still unavailable. Even when
        // its native clock passes the eventual endpoint, retain the first lower.
        end.prime_target_clock(7, basis, pair(basis, unavailable_frame))
            .unwrap();
        output.render_pending(8).unwrap();
        let facts = output.boundaries();
        let source = output.last_real_source_report();
        assert_eq!(facts.end.unwrap().source_frame, 3);
        assert_eq!(
            facts.end.unwrap().target_time,
            TargetTime::from_frames(4, 48_000).unwrap()
        );
        output.admit(8).unwrap();
        end.prime_target(7, basis, facts, source, pair(basis, crossing_frame))
            .unwrap();
        let pending = end.clone();
        assert!(end
            .prime_target_clock(7, basis, pair(basis, crossing_frame + 1))
            .is_err());
        assert_eq!(end, pending);
        let boundary = end
            .observe_target(7, basis, facts, source, pair(basis, crossing_frame + 2))
            .unwrap()
            .unwrap();
        assert_eq!(boundary.physical_frame, 3);
        assert_eq!(boundary.playback_frame, 3);
        assert_eq!(boundary.output, pair(basis, 4).source);
        assert_eq!(boundary.host, pair(basis, 4).target);
        assert_eq!(
            end.observe_target(7, basis, facts, source, pair(basis, crossing_frame + 3))
                .unwrap(),
            None
        );
        let completed = end.clone();
        assert!(end
            .prime_target_clock(7, basis, pair(basis, crossing_frame + 4))
            .is_err());
        assert_eq!(end, completed);
    }
}

#[test]
fn clock_only_target_priming_rejects_changed_identity_and_regressed_pairs_atomically() {
    let (_producer, output) = rig(44_100, 48_000, None, Some(3), 0);
    let basis = output.target_frame_basis();
    let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 44_100, 3)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
    end.prime_target_clock(7, basis, pair(basis, 0)).unwrap();
    end.prime_target_clock(7, basis, pair(basis, 2)).unwrap();
    let before = end.clone();
    assert!(end.prime_target_clock(8, basis, pair(basis, 3)).is_err());
    assert_eq!(end, before);
    let changed = TargetFrameBasis::new(basis.origin(), basis.start_time(), 32_000).unwrap();
    assert!(end.prime_target_clock(7, changed, pair(basis, 3)).is_err());
    assert_eq!(end, before);
    for source_domain in [true, false] {
        let mut invalid = pair(basis, 3);
        if source_domain {
            invalid.source.domain = ClockDomainId(99);
        } else {
            invalid.target.domain = ClockDomainId(99);
        }
        assert!(end.prime_target_clock(7, basis, invalid).is_err());
        assert_eq!(end, before);
    }
    assert!(end.prime_target_clock(7, basis, pair(basis, 1)).is_err());
    assert_eq!(end, before);
    let mut negative = pair(basis, 3);
    negative.source.timestamp = beatkernel::time::Timestamp::from_nanos(-1);
    assert!(end.prime_target_clock(7, basis, negative).is_err());
    assert_eq!(end, before);
    // Refusal cannot install an endpoint, render report or fake boundary fact.
    assert!(output.last_real_source_report().is_none());
    assert!(output.boundaries().end.is_none());
    end.prime_target_clock(7, basis, pair(basis, 3)).unwrap();
}
