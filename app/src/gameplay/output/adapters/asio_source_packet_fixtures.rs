//! Genuine Mixer source/render facts with explicitly supplied callback metadata.
//! These pure adapter tests do not prove SDK acquisition or acoustic latency.
use super::*;
use crate::{
    gameplay::output::ports::*,
    native_audio_presentation::NativeAudioSnapshot,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_platform::{
    audio::asio::MultimediaHostInterval,
    audio::presentation::validation::OriginalNativePresentationEvidence,
    windows::asio::stream::{
        AsioBufferObservation, AsioCallbackEvent, AsioDiagnostics, AsioFaults,
        AsioPlaybackPhaseSnapshot, AsioStreamSnapshot,
    },
};

fn at(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 4).unwrap(), 0, 0, 1, 0, 1).unwrap()
}
fn fixture() -> (
    OutputFrameBasis,
    SourcePlaybackIdentity,
    AsioPlaybackPhaseSnapshot,
    CoordinateNativeAudioSnapshot,
) {
    let format = AudioFormat::new(4, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(128, 128, 1).unwrap()).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::from_nanos(-123),
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(false);
    mixer.enable_source_playback_history(8).unwrap();
    let basis = mixer.output_frame_basis();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    let report = mixer.render(&mut [0.; 2]).unwrap();
    let history = mixer.source_playback_history().unwrap();
    let identity = SourcePlaybackIdentity::new(
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
    )
    .unwrap();
    let event = AsioCallbackEvent {
        buffer_index: 1,
        direct_process: -7,
        flags: 7,
        sample_position: 1002,
        system_nanoseconds: 8013,
        sample_rate: 4.0,
    };
    let view = AsioPlaybackPhaseSnapshot {
        snapshot: AsioStreamSnapshot {
            phase: AsioStreamPhase::Running,
            telemetry_available: true,
            prepared_frames: 4,
            buffer_fills: 2,
            render: Some(report),
            buffer_observation: Some(AsioBufferObservation {
                event,
                render: report,
                delivered_first_frame: report.start_frame,
                delivered_frames: u32::try_from(report.frames).unwrap(),
            }),
            native: AsioDiagnostics::default(),
        },
        playback: history.lookup_at_source_time(at(6)).unwrap(),
        pulled_source_end: history.pulled_end_frame(),
        revision: 4,
    };
    let point = |ns| ClockPoint {
        domain: ClockDomainId(8),
        timestamp: Timestamp::from_nanos(ns),
    };
    let observation = AsioPresentationObservation::from_render(
        report,
        4,
        MultimediaHostInterval {
            before: point(1_000_000_000),
            after: point(1_000_000_013),
        },
        1,
        7,
        basis.origin(),
    )
    .unwrap();
    let candidate = CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 23,
            basis,
            evidence: OriginalNativePresentationEvidence::Asio {
                observation,
                basis: Some(basis),
            },
        },
        NativeOutputCoordinate::StreamFrame { frame: 2 },
    )
    .unwrap();
    (basis, identity, view, candidate)
}

#[test]
fn nonzero_original_history_and_full_callback_survive_packet_and_observation_qualification() {
    let (basis, id, view, candidate) = fixture();
    assert_eq!(
        (id.hz(), id.initial_physical(), id.initial_playback()),
        (4, 4, 2)
    );
    assert_eq!(
        view.playback,
        SourcePlaybackSpan::new(6, None, 4, SourcePlaybackState::Frozen).unwrap()
    );
    let query = candidate.source_query().unwrap();
    assert_eq!(
        query.compare_target_time(TargetTime::from_frames(6, 4).unwrap()),
        std::cmp::Ordering::Equal
    );
    let packet = qualify_asio_source_packet(23, basis, id, query, view).unwrap();
    assert_eq!(packet.native_evidence(), Some(&view));
    assert_eq!(packet.facts().submitted_source_end, 8);
    let observation = qualify_asio_source_observation_packet(
        candidate,
        view.snapshot.buffer_observation.unwrap(),
        packet,
    )
    .unwrap()
    .unwrap();
    assert_eq!(observation.coordinate(), candidate);
    assert_eq!(observation.source().facts().playback, view.playback);
    assert_eq!(observation.source().native_evidence(), Some(&view));
}

#[test]
fn every_original_callback_field_must_match_even_if_render_and_native_frame_are_equal() {
    let (basis, id, view, candidate) = fixture();
    for case in 0..6 {
        let mut changed = view.snapshot.buffer_observation.unwrap();
        match case {
            0 => changed.event.buffer_index = 0,
            1 => changed.event.direct_process += 1,
            2 => changed.event.flags ^= 4,
            3 => changed.event.sample_position += 1,
            4 => changed.event.system_nanoseconds += 1,
            _ => changed.event.sample_rate += 0.25,
        }
        let packet = qualify_asio_source_packet(23, basis, id, at(6), view).unwrap();
        assert!(
            qualify_asio_source_observation_packet(candidate, changed, packet)
                .unwrap()
                .is_none(),
            "case {case}"
        );
    }
}

#[test]
fn source_math_keeps_cached_faults_but_absent_callback_never_substitutes_cached_native_event() {
    let (basis, id, mut view, candidate) = fixture();
    let callback = view.snapshot.buffer_observation.take().unwrap();
    view.snapshot.native = AsioDiagnostics {
        faults: AsioFaults(8),
        render_error: 9,
        event: Some(callback.event),
    };
    let packet = qualify_asio_source_packet(23, basis, id, at(6), view).unwrap();
    assert_eq!(
        packet.native_evidence().unwrap().snapshot.native,
        view.snapshot.native
    );
    assert!(
        qualify_asio_source_observation_packet(candidate, callback, packet)
            .unwrap()
            .is_none()
    );
}

#[test]
fn malformed_publication_delivery_and_report_metadata_refuse_without_observation() {
    let (basis, id, view, _) = fixture();
    for case in 0..7 {
        let mut bad = view;
        match case {
            0 => bad.revision = 0,
            1 => bad.revision = 3,
            2 => bad.snapshot.telemetry_available = false,
            3 => bad.snapshot.prepared_frames = 0,
            4 => bad.snapshot.prepared_frames = 5,
            5 => bad.snapshot.prepared_frames = u64::MAX,
            _ => {
                bad.snapshot
                    .buffer_observation
                    .as_mut()
                    .unwrap()
                    .render
                    .start_frame += 1
            }
        }
        assert!(
            qualify_asio_source_packet(23, basis, id, at(6), bad).is_err(),
            "case {case}"
        );
    }
    for identity in [
        SourcePlaybackIdentity::new(5, 4, 2).unwrap(),
        SourcePlaybackIdentity::new(4, 5, 2).unwrap(),
    ] {
        assert!(qualify_asio_source_packet(23, basis, identity, at(6), view).is_err());
    }
}

#[test]
fn exact_partial_delivery_prefix_and_original_epoch_query_are_enforced() {
    let (basis, id, mut view, whole_candidate) = fixture();
    // Explicit supplied partial-write metadata: no Mixer history is rewritten.
    view.snapshot.prepared_frames = 3;
    view.snapshot
        .buffer_observation
        .as_mut()
        .unwrap()
        .delivered_frames = 1;
    let candidate = delivery_candidate(whole_candidate, 6, 1);
    let edge = qualify_asio_source_packet(23, basis, id, at(7), view).unwrap();
    assert_eq!(edge.facts().submitted_source_end, 7);
    assert_eq!(edge.facts().pulled_source_end, 8);
    let fraction = InterpolatedTargetTime::new(
        TargetTime::from_frames(7, 4).unwrap(),
        0,
        1,
        u64::MAX,
        1,
        u64::MAX - 1,
    )
    .unwrap();
    assert!(qualify_asio_source_packet(23, basis, id, fraction, view).is_err());
    let wrong_epoch = qualify_asio_source_packet(24, basis, id, at(6), view).unwrap();
    assert!(matches!(
        qualify_asio_source_observation_packet(
            candidate,
            view.snapshot.buffer_observation.unwrap(),
            wrong_epoch
        ),
        Err(SourcePlaybackPacketError::EpochMismatch)
    ));
    let wrong_query = qualify_asio_source_packet(23, basis, id, at(7), view).unwrap();
    assert!(matches!(
        qualify_asio_source_observation_packet(
            candidate,
            view.snapshot.buffer_observation.unwrap(),
            wrong_query
        ),
        Err(SourcePlaybackPacketError::CoordinateMismatch)
    ));
}

fn delivery_candidate(
    whole: CoordinateNativeAudioSnapshot,
    first: u64,
    frames: usize,
) -> CoordinateNativeAudioSnapshot {
    let mut snapshot = whole.snapshot();
    let OriginalNativePresentationEvidence::Asio { observation, basis } = snapshot.evidence else {
        panic!("ASIO fixture must retain its original evidence family");
    };
    snapshot.evidence = OriginalNativePresentationEvidence::Asio {
        observation: AsioPresentationObservation::from_delivery(
            observation.render,
            first,
            frames,
            observation.sample_rate,
            observation.host,
            0,
            0,
            observation.output_origin,
        )
        .unwrap(),
        basis,
    };
    CoordinateNativeAudioSnapshot::new(
        snapshot,
        NativeOutputCoordinate::StreamFrame {
            frame: first - snapshot.basis.start_physical_frame(),
        },
    )
    .unwrap()
}

#[test]
fn two_deliveries_of_one_original_render_preserve_report_history_and_advance_coordinates() {
    let (basis, id, view, whole) = fixture();
    let original_report = view.snapshot.render.unwrap();
    for (first, prepared, relative) in [(6, 3, 2), (7, 4, 3)] {
        let mut prefix_view = view;
        prefix_view.snapshot.prepared_frames = prepared;
        let callback = prefix_view.snapshot.buffer_observation.as_mut().unwrap();
        callback.delivered_first_frame = first;
        callback.delivered_frames = 1;
        let candidate = delivery_candidate(whole, first, 1);
        let query = candidate.source_query().unwrap();
        let packet = qualify_asio_source_packet(23, basis, id, query, prefix_view).unwrap();
        let qualified = qualify_asio_source_observation_packet(
            candidate,
            prefix_view.snapshot.buffer_observation.unwrap(),
            packet,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            qualified.coordinate().coordinate(),
            NativeOutputCoordinate::StreamFrame { frame: relative }
        );
        assert_eq!(qualified.source().facts().render, Some(original_report));
        assert_eq!(qualified.source().facts().submitted_source_end, first + 1);
        assert_eq!(qualified.source().facts().pulled_source_end, 8);
        assert_eq!(qualified.source().facts().playback, view.playback);
        assert_eq!(qualified.source().native_evidence(), Some(&prefix_view));
    }
}

#[test]
fn callback_and_native_observation_must_agree_on_both_delivery_fields_in_both_directions() {
    let (basis, id, view, whole) = fixture();
    // Matching the report/event alone cannot authenticate a different prefix.
    for (callback_first, callback_frames, candidate_first, candidate_frames) in
        [(6, 2, 6, 1), (6, 1, 6, 2), (6, 1, 7, 1), (7, 1, 6, 1)]
    {
        let mut current = view;
        current.snapshot.prepared_frames =
            callback_first + u64::from(callback_frames) - basis.start_physical_frame();
        let callback = current.snapshot.buffer_observation.as_mut().unwrap();
        callback.delivered_first_frame = callback_first;
        callback.delivered_frames = callback_frames;
        let callback = *callback;
        let candidate = delivery_candidate(whole, candidate_first, candidate_frames);
        let packet =
            qualify_asio_source_packet(23, basis, id, candidate.source_query().unwrap(), current)
                .unwrap();
        assert!(qualify_asio_source_observation_packet(candidate, callback, packet)
            .unwrap()
            .is_none(), "callback {callback_first}+{callback_frames}, candidate {candidate_first}+{candidate_frames}");
    }
    // A changed expected callback is independently refused as well.
    for field in 0..2 {
        let mut changed = view.snapshot.buffer_observation.unwrap();
        if field == 0 {
            changed.delivered_first_frame += 1;
        } else {
            changed.delivered_frames -= 1;
        }
        let packet = qualify_asio_source_packet(23, basis, id, at(6), view).unwrap();
        assert!(
            qualify_asio_source_observation_packet(whole, changed, packet)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn invalid_delivered_interval_never_authenticates_a_report_or_submitted_prefix() {
    let (basis, id, view, _) = fixture();
    for (first, count, prepared) in [
        (6, 0, 4),
        (5, 1, 4),
        (7, 2, 4),
        (u64::MAX, 1, 4),
        (6, u32::MAX, 4),
        (6, 2, 3),
    ] {
        let mut malformed = view;
        malformed.snapshot.prepared_frames = prepared;
        let callback = malformed.snapshot.buffer_observation.as_mut().unwrap();
        callback.delivered_first_frame = first;
        callback.delivered_frames = count;
        assert!(
            qualify_asio_source_packet(23, basis, id, at(6), malformed).is_err(),
            "delivery {first}+{count}, prepared {prepared}"
        );
    }
}

#[test]
fn packet_rejections_keep_existing_typed_revision_report_and_prefix_errors() {
    let (basis, id, view, _) = fixture();
    for revision in [0, 1, 3, u64::MAX] {
        let mut malformed = view;
        malformed.revision = revision;
        assert!(matches!(
            qualify_asio_source_packet(23, basis, id, at(6), malformed),
            Err(SourcePlaybackPacketError::InvalidRevision)
        ));
    }
    let mut malformed = view;
    malformed
        .snapshot
        .buffer_observation
        .as_mut()
        .unwrap()
        .render
        .start_frame += 1;
    assert!(matches!(
        qualify_asio_source_packet(23, basis, id, at(6), malformed),
        Err(SourcePlaybackPacketError::InvalidReport)
    ));
    for prepared in [0, 5] {
        let mut malformed = view;
        malformed.snapshot.prepared_frames = prepared;
        assert!(matches!(
            qualify_asio_source_packet(23, basis, id, at(6), malformed),
            Err(SourcePlaybackPacketError::InvalidPrefix)
        ));
    }
}
