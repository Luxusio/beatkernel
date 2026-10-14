//! Actual Mixer history plus explicit WASAPI DI evidence; never a device timing claim.
use super::{qualify_wasapi_source_observation_packet, qualify_wasapi_source_packet};
use crate::gameplay::output::ports::{SourcePlaybackIdentity, SourcePlaybackPacketError};
use crate::native_audio_presentation::NativeAudioSnapshot;
use crate::native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate};
use beatkernel::audio::*;
use beatkernel::time::{ClockDomainId, ClockMappingQuality, ClockPoint, Timestamp};
use beatkernel_platform::audio::{
    presentation::validation::OriginalNativePresentationEvidence, AudioClockReadingQuality,
    AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus, DeviceFormat, NativeOutputState,
    SampleEncoding, SourcePlaybackSnapshot, StreamCounters,
};
use std::cmp::Ordering;

fn at(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 3).unwrap(), 0, 0, 1, 0, 1).unwrap()
}
fn rig() -> (
    OutputFrameBasis,
    SourcePlaybackIdentity,
    SourcePlaybackSnapshot,
) {
    let format = AudioFormat::new(3, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(32, 64, 1).unwrap()).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::from_nanos(-123),
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0; 5]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 3]).unwrap();
    mixer.enable_source_playback_history(4).unwrap();
    let basis = mixer.output_frame_basis();
    let history = mixer.source_playback_history().unwrap();
    let identity = SourcePlaybackIdentity::new(
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
    )
    .unwrap();
    let render = mixer.render(&mut [0.0; 4]).unwrap();
    let history = mixer.source_playback_history().unwrap();
    assert_eq!(
        (basis.start_physical_frame(), identity.initial_playback()),
        (8, 5)
    );
    assert_eq!(history.pulled_end_frame(), 12);
    let snapshot = AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters {
            submitted_frames: 2,
            buffer_fills: 1,
            padding_frames: 1,
            ..StreamCounters::default()
        },
        render: Some(render),
        clock: Some(AudioClockSnapshot {
            position: 1,
            frequency: u64::MAX,
            qpc_100ns: 11,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(ClockPoint {
                domain: ClockDomainId(8),
                timestamp: Timestamp::from_nanos(1100),
            }),
            mapping_quality: ClockMappingQuality::Unknown,
        }),
    };
    (
        basis,
        identity,
        SourcePlaybackSnapshot {
            snapshot,
            playback: history.lookup_at_source_time(at(8)).unwrap(),
            pulled_source_end: history.pulled_end_frame(),
            revision: 4,
        },
    )
}
fn coordinate(
    epoch: u64,
    basis: OutputFrameBasis,
    raw: AudioStreamSnapshot,
) -> CoordinateNativeAudioSnapshot {
    let clock = raw.clock.unwrap();
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch,
            basis,
            evidence: OriginalNativePresentationEvidence::Wasapi {
                snapshot: raw,
                basis: Some(basis),
            },
        },
        NativeOutputCoordinate::Counter {
            position: clock.position,
            frequency: clock.frequency,
        },
    )
    .unwrap()
}

#[test]
fn real_paused_history_original_render_and_partial_native_submission_are_preserved() {
    let (basis, id, raw) = rig();
    let native = coordinate(23, basis, raw.snapshot);
    let query = native.source_query().unwrap();
    let packet = qualify_wasapi_source_packet(23, basis, id, query, raw).unwrap();
    assert_eq!(packet.facts().submitted_source_end, 10);
    assert_eq!(packet.facts().pulled_source_end, 12);
    assert_eq!(packet.facts().render, raw.snapshot.render);
    assert_eq!(packet.facts().playback, raw.playback);
    assert_eq!(packet.facts().playback.state(), SourcePlaybackState::Frozen);
    assert_eq!(packet.facts().revision, 4);
    assert_eq!(packet.native_evidence(), Some(&raw.snapshot));
    let observed = qualify_wasapi_source_observation_packet(native, id, packet)
        .unwrap()
        .unwrap();
    assert_eq!(observed.coordinate(), native);
    assert_eq!(
        observed.source().query().compare_interpolated_time(query),
        Ordering::Equal
    );
    assert_eq!(observed.source().native_evidence(), Some(&raw.snapshot));
}

#[test]
fn actual_submitted_frontier_is_inclusive_and_unsubmitted_pulls_do_not_qualify() {
    let (basis, id, raw) = rig();
    assert!(qualify_wasapi_source_packet(23, basis, id, at(10), raw).is_ok());
    for query in [
        at(7),
        at(11),
        InterpolatedTargetTime::new(
            TargetTime::from_frames(10, 3).unwrap(),
            1,
            1,
            u64::MAX,
            0,
            1,
        )
        .unwrap(),
    ] {
        assert!(qualify_wasapi_source_packet(23, basis, id, query, raw).is_err());
    }
    let fraction = InterpolatedTargetTime::new(
        TargetTime::from_frames(10, 3).unwrap(),
        1,
        1,
        u64::MAX,
        0,
        1,
    )
    .unwrap();
    assert_eq!(
        fraction.point(basis.origin()).unwrap(),
        at(10).point(basis.origin()).unwrap()
    );
}

#[test]
fn malformed_native_publication_facts_refuse_without_changing_original_payload() {
    let (basis, id, original) = rig();
    for case in 0..10 {
        let mut raw = original;
        match case {
            0 => raw.revision = 0,
            1 => raw.revision = 3,
            2 => raw.snapshot.telemetry_available = false,
            3 => raw.snapshot.counters.submitted_frames = 0,
            4 => raw.snapshot.counters.submitted_frames = 5,
            5 => raw.snapshot.counters.submitted_frames = u64::MAX,
            6 => raw.pulled_source_end = 7,
            7 => raw.snapshot.render.as_mut().unwrap().frames = 5,
            8 => raw.snapshot.render.as_mut().unwrap().playback_frames = 5,
            _ => {
                raw.playback =
                    SourcePlaybackSpan::new(8, None, 6, SourcePlaybackState::Frozen).unwrap()
            }
        }
        let saved = raw;
        assert!(
            qualify_wasapi_source_packet(23, basis, id, at(8), raw).is_err(),
            "case {case}"
        );
        assert_eq!(raw.snapshot, saved.snapshot);
        assert_eq!(raw.playback, saved.playback);
        assert_eq!(
            (raw.pulled_source_end, raw.revision),
            (saved.pulled_source_end, saved.revision)
        );
    }
}

#[test]
fn native_packet_rejects_wrong_original_source_header_and_half_open_span() {
    let (basis, id, mut raw) = rig();
    for wrong in [
        SourcePlaybackIdentity::new(4, 8, 5).unwrap(),
        SourcePlaybackIdentity::new(3, 9, 5).unwrap(),
        SourcePlaybackIdentity::new(3, 8, 6).unwrap(),
    ] {
        assert!(qualify_wasapi_source_packet(23, basis, wrong, at(8), raw).is_err());
    }
    raw.playback = SourcePlaybackSpan::new(8, Some(9), 5, SourcePlaybackState::Frozen).unwrap();
    assert!(qualify_wasapi_source_packet(23, basis, id, at(8), raw).is_ok());
    assert!(qualify_wasapi_source_packet(23, basis, id, at(9), raw).is_err());
}

#[test]
fn changed_complete_native_publication_waits_instead_of_stitching_evidence() {
    let (basis, id, original) = rig();
    let candidate = coordinate(23, basis, original.snapshot);
    let query = candidate.source_query().unwrap();
    for case in 0..8 {
        let mut raw = original;
        match case {
            0 => raw.snapshot.counters.padding_frames += 1,
            1 => raw.snapshot.counters.buffer_fills += 1,
            2 => raw.snapshot.counters.inferred_starvations += 1,
            3 => raw.snapshot.clock.as_mut().unwrap().qpc_100ns += 1,
            4 => {
                raw.snapshot
                    .clock
                    .as_mut()
                    .unwrap()
                    .host_point
                    .as_mut()
                    .unwrap()
                    .timestamp = Timestamp::from_nanos(1200)
            }
            5 => raw.snapshot.clock.as_mut().unwrap().mapping_quality = ClockMappingQuality::Exact,
            6 => raw.snapshot.status = AudioStreamStatus::Ready,
            _ => raw.snapshot.render = None,
        }
        let packet = qualify_wasapi_source_packet(23, basis, id, query, raw).unwrap();
        assert!(
            qualify_wasapi_source_observation_packet(candidate, id, packet)
                .unwrap()
                .is_none(),
            "case {case}"
        );
        assert_eq!(
            candidate.snapshot().evidence,
            OriginalNativePresentationEvidence::Wasapi {
                snapshot: original.snapshot,
                basis: Some(basis)
            }
        );
    }
}

#[test]
fn observation_header_and_subnanosecond_query_corruption_refuse_before_publication_matching() {
    let (basis, id, raw) = rig();
    let candidate = coordinate(23, basis, raw.snapshot);
    let exact = candidate.source_query().unwrap();
    let rounded = at(8);
    assert_eq!(
        exact.point(basis.origin()).unwrap(),
        rounded.point(basis.origin()).unwrap()
    );
    let packet = qualify_wasapi_source_packet(23, basis, id, rounded, raw).unwrap();
    assert!(qualify_wasapi_source_observation_packet(candidate, id, packet).is_err());
    let packet = qualify_wasapi_source_packet(24, basis, id, exact, raw).unwrap();
    assert!(qualify_wasapi_source_observation_packet(candidate, id, packet).is_err());
    let packet = qualify_wasapi_source_packet(23, basis, id, exact, raw).unwrap();
    assert!(qualify_wasapi_source_observation_packet(
        candidate,
        SourcePlaybackIdentity::new(3, 8, 4).unwrap(),
        packet
    )
    .is_err());
}

// Complete software ownership is real; the native clock below is explicit DI
// evidence passed through the same production qualifier as the Windows adapter.
// These portable checks do not execute a Windows stream or mint native tickets.
const COMPLETE_SOURCE_HZ: u32 = 48_000;

fn complete_at(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(
        TargetTime::from_frames(frame, COMPLETE_SOURCE_HZ).unwrap(),
        0,
        0,
        1,
        0,
        1,
    )
    .unwrap()
}

fn complete_source_state() -> (
    CommandProducer,
    NativeOutputState,
    OutputFrameBasis,
    SourcePlaybackIdentity,
) {
    let format = AudioFormat::new(COMPLETE_SOURCE_HZ, 1).unwrap();
    // Limits are bytes: this 32-frame mono float asset occupies 128 bytes.
    let limits = PcmLimits::new(128, 512, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 32], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(-123),
            gain: 1.0,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::from_nanos(-123),
            AudioLimits::new(16, 4, 16, 32, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut active = [0.0; 5];
    mixer.render(&mut active).unwrap();
    assert_eq!(active, [0.25; 5]);
    producer.request_pause(true);
    let mut held = [1.0; 3];
    assert!(mixer.render(&mut held).unwrap().paused);
    assert_eq!(held, [0.0; 3]);
    mixer.enable_source_playback_history(16).unwrap();
    let creation = mixer.output_frame_basis();
    let history = mixer.source_playback_history().unwrap();
    let identity = SourcePlaybackIdentity::new(
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
    )
    .unwrap();
    let mut state = match NativeOutputState::new(
        mixer,
        DeviceFormat::new(COMPLETE_SOURCE_HZ, 2, SampleEncoding::Float32, None).unwrap(),
        Some(ChannelMatrix::new(1, 2, &[1.0, -0.5]).unwrap()),
        8,
    ) {
        Ok(state) => state,
        Err(_) => panic!("valid complete Source setup"),
    };
    let report = state.render_pending(6).unwrap();
    assert_eq!((report.start_frame, report.frames), (8, 6));
    assert!(report.paused);
    assert_eq!(report.playback_frames, 0);
    state.admit(2).unwrap();
    assert_eq!(state.output_frame_basis().start_physical_frame(), 10);
    (producer, state, creation, identity)
}

fn complete_snapshot(
    state: &NativeOutputState,
    basis: OutputFrameBasis,
    submitted: u64,
    position: u64,
    frequency: u64,
) -> SourcePlaybackSnapshot {
    let query = InterpolatedTargetTime::new(
        TargetTime::from_frames(basis.start_physical_frame(), basis.sample_rate()).unwrap(),
        position,
        position,
        frequency,
        0,
        1,
    )
    .unwrap();
    let history = state.mixer().source_playback_history().unwrap();
    SourcePlaybackSnapshot {
        snapshot: AudioStreamSnapshot {
            telemetry_available: true,
            status: AudioStreamStatus::Running,
            counters: StreamCounters {
                submitted_frames: submitted,
                buffer_fills: 1,
                padding_frames: 1,
                ..StreamCounters::default()
            },
            render: state.pending_report(),
            clock: Some(AudioClockSnapshot {
                position,
                frequency,
                qpc_100ns: 11 + position,
                reading_quality: AudioClockReadingQuality::Accurate,
                host_point: Some(ClockPoint {
                    domain: ClockDomainId(8),
                    timestamp: Timestamp::from_nanos(1100 + position as i64 * 100),
                }),
                mapping_quality: ClockMappingQuality::Unknown,
            }),
        },
        playback: history.lookup_at_source_time(query).unwrap(),
        pulled_source_end: history.pulled_end_frame(),
        revision: 4,
    }
}

#[test]
fn complete_reopen_preserves_creation_history_pending_report_and_changed_native_frequency() {
    let (mut producer, mut state, creation, identity) = complete_source_state();
    let old = complete_snapshot(&state, creation, 2, 1, 96_000);
    let old_native = coordinate(23, creation, old.snapshot);
    let old_packet = qualify_wasapi_source_packet(
        23,
        creation,
        identity,
        old_native.source_query().unwrap(),
        old,
    )
    .unwrap();
    assert_eq!(old_packet.facts().submitted_source_end, 10);
    let basis = state.output_frame_basis();
    let pending = state.pending_samples().to_vec();
    let report = state.pending_report();
    let counters = state.mixer().counters();
    assert_eq!(pending, [0.0; 8]);
    assert_eq!(state.mixer().frame_cursor(), 14);
    assert_eq!(creation.start_physical_frame(), 8);
    state
        .reconfigure(
            state.format().unwrap(),
            Some(ChannelMatrix::new(1, 2, &[1.0, -0.5]).unwrap()),
            2,
        )
        .unwrap();
    assert_eq!(state.pending_samples(), pending);
    assert_eq!(state.pending_report(), report);
    assert_eq!(state.mixer().counters(), counters);
    assert_eq!(state.output_frame_basis(), basis);
    // This epoch successfully releases two frames from the preserved suffix.
    state.admit(2).unwrap();
    let raw = complete_snapshot(&state, basis, 2, 1, u64::MAX);
    let native = coordinate(24, basis, raw.snapshot);
    let query = native.source_query().unwrap();
    let packet = qualify_wasapi_source_packet(24, basis, identity, query, raw).unwrap();
    let observed = qualify_wasapi_source_observation_packet(native, identity, packet)
        .unwrap()
        .unwrap();
    assert_eq!(
        observed.coordinate().coordinate(),
        NativeOutputCoordinate::Counter {
            position: 1,
            frequency: u64::MAX
        }
    );
    assert_eq!(observed.source().identity(), identity);
    assert_eq!(
        identity,
        SourcePlaybackIdentity::new(COMPLETE_SOURCE_HZ, 8, 5).unwrap()
    );
    assert_eq!(packet.facts().submitted_source_end, 12);
    assert_eq!(packet.facts().pulled_source_end, 14);
    assert_eq!(packet.facts().render, report);
    assert_eq!(packet.facts().render.unwrap().start_frame, 8);
    assert_eq!(packet.facts().playback.physical_start(), 8);
    assert_eq!(packet.facts().playback.playback_start(), 5);
    assert_eq!(packet.native_evidence(), Some(&raw.snapshot));
    assert!(query.compare_interpolated_time(complete_at(10)).is_gt());
    assert!(query.compare_interpolated_time(complete_at(11)).is_lt());
    assert_eq!(
        query.point(basis.origin()).unwrap(),
        complete_at(10).point(basis.origin()).unwrap()
    );
    assert_eq!(state.pending_samples(), [0.0; 4]);
    assert_eq!(state.admitted_frames(), 4);
    assert_eq!(state.mixer().frame_cursor(), 14);

    // The original producer and prepared remix still address this same Mixer.
    state.admit(2).unwrap();
    producer.request_pause(false);
    let resumed = state.render_pending(2).unwrap();
    assert_eq!((resumed.start_frame, resumed.playback_start_frame), (14, 5));
    assert_eq!(resumed.playback_frames, 2);
    assert_eq!(state.pending_samples(), [0.25, -0.125, 0.25, -0.125]);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(
        history.initial_physical_frame(),
        creation.start_physical_frame()
    );
    assert_eq!(
        history.initial_playback_frame(),
        identity.initial_playback()
    );
    assert_eq!(history.source_rate(), COMPLETE_SOURCE_HZ);
    assert_eq!(history.span(0).unwrap().physical_start(), 8);
    assert_eq!(history.span(0).unwrap().physical_end(), Some(14));
}

#[test]
fn complete_counter_packets_keep_progress_and_coherence_below_one_nanosecond() {
    let (_, mut state, _, identity) = complete_source_state();
    let basis = state.output_frame_basis();
    state.admit(2).unwrap();
    let first = complete_snapshot(&state, basis, 2, 1, u64::MAX);
    let mut second = complete_snapshot(&state, basis, 2, 2, u64::MAX);
    second.revision = 6;
    let a = coordinate(24, basis, first.snapshot);
    let b = coordinate(24, basis, second.snapshot);
    let aq = a.source_query().unwrap();
    let bq = b.source_query().unwrap();
    assert_eq!(
        aq.point(basis.origin()).unwrap(),
        bq.point(basis.origin()).unwrap()
    );
    assert!(aq.compare_interpolated_time(bq).is_lt());
    let ap = qualify_wasapi_source_packet(24, basis, identity, aq, first).unwrap();
    let bp = qualify_wasapi_source_packet(24, basis, identity, bq, second).unwrap();
    assert!(qualify_wasapi_source_observation_packet(a, identity, ap)
        .unwrap()
        .is_some());
    assert!(qualify_wasapi_source_observation_packet(b, identity, bp)
        .unwrap()
        .is_some());
    assert_eq!(
        bp.native_evidence().unwrap().clock.unwrap().frequency,
        u64::MAX
    );
    // Both the rounded query and a packet from another native publication fail.
    let rounded =
        qualify_wasapi_source_packet(24, basis, identity, complete_at(10), first).unwrap();
    assert_eq!(
        qualify_wasapi_source_observation_packet(a, identity, rounded).unwrap_err(),
        SourcePlaybackPacketError::CoordinateMismatch
    );
    assert_eq!(
        qualify_wasapi_source_observation_packet(b, identity, ap).unwrap_err(),
        SourcePlaybackPacketError::CoordinateMismatch
    );
    let mut changed = second;
    changed.snapshot.counters.native_failures = 1;
    let changed = qualify_wasapi_source_packet(24, basis, identity, bq, changed).unwrap();
    assert!(
        qualify_wasapi_source_observation_packet(b, identity, changed)
            .unwrap()
            .is_none()
    );
}

#[test]
fn complete_pending_packet_refuses_missing_torn_unsubmitted_and_wrong_original_history_facts() {
    let (_, mut state, _, identity) = complete_source_state();
    let basis = state.output_frame_basis();
    state.admit(2).unwrap();
    let original = complete_snapshot(&state, basis, 2, 1, u64::MAX);
    let query = coordinate(24, basis, original.snapshot)
        .source_query()
        .unwrap();
    for (case, expected) in [
        SourcePlaybackPacketError::MissingNativeEvidence,
        SourcePlaybackPacketError::InvalidRevision,
        SourcePlaybackPacketError::InvalidRevision,
        SourcePlaybackPacketError::InvalidPrefix,
        SourcePlaybackPacketError::InvalidPrefix,
        SourcePlaybackPacketError::Overflow,
        SourcePlaybackPacketError::InvalidSpan,
        SourcePlaybackPacketError::InvalidReport,
    ]
    .into_iter()
    .enumerate()
    {
        let mut raw = original;
        match case {
            0 => raw.snapshot.telemetry_available = false,
            1 => raw.revision = 0,
            2 => raw.revision = 5,
            3 => raw.snapshot.counters.submitted_frames = 0,
            4 => raw.snapshot.counters.submitted_frames = 5,
            5 => raw.snapshot.counters.submitted_frames = u64::MAX,
            6 => {
                raw.playback =
                    SourcePlaybackSpan::new(8, None, 6, SourcePlaybackState::Frozen).unwrap()
            }
            _ => raw.snapshot.render.as_mut().unwrap().frames = 7,
        }
        assert_eq!(
            qualify_wasapi_source_packet(24, basis, identity, query, raw).unwrap_err(),
            expected,
            "case {case}"
        );
        assert_eq!(state.pending_report(), original.snapshot.render);
        assert_eq!(state.pending_samples(), [0.0; 4]);
        assert_eq!(state.admitted_frames(), 4);
    }
    for frame in [9, 13] {
        assert_eq!(
            qualify_wasapi_source_packet(24, basis, identity, complete_at(frame), original)
                .unwrap_err(),
            SourcePlaybackPacketError::QueryOutsidePrefix
        );
    }
    assert!(qualify_wasapi_source_packet(24, basis, identity, complete_at(12), original).is_ok());
    let beyond_submission = InterpolatedTargetTime::new(
        TargetTime::from_frames(12, COMPLETE_SOURCE_HZ).unwrap(),
        1,
        1,
        u64::MAX,
        0,
        1,
    )
    .unwrap();
    assert_eq!(
        beyond_submission.point(basis.origin()).unwrap(),
        complete_at(12).point(basis.origin()).unwrap()
    );
    assert_eq!(
        qualify_wasapi_source_packet(24, basis, identity, beyond_submission, original).unwrap_err(),
        SourcePlaybackPacketError::QueryOutsidePrefix
    );
    for wrong in [
        SourcePlaybackIdentity::new(44_100, 8, 5).unwrap(),
        SourcePlaybackIdentity::new(COMPLETE_SOURCE_HZ, 10, 5).unwrap(),
        SourcePlaybackIdentity::new(COMPLETE_SOURCE_HZ, 8, 6).unwrap(),
    ] {
        assert!(qualify_wasapi_source_packet(24, basis, wrong, query, original).is_err());
    }
    let history = state.mixer().source_playback_history().unwrap();
    assert!(history.lookup_at_source_time(complete_at(7)).is_err());
    assert!(history.lookup_at_source_time(complete_at(15)).is_err());
    assert_eq!(history.pulled_end_frame(), 14);
}

#[test]
fn complete_counter_evidence_rejects_missing_clock_and_frame_or_frequency_substitution() {
    use crate::native_output_coordinate::NativeOutputCoordinateError as Error;
    let (_, mut state, creation, _) = complete_source_state();
    let basis = state.output_frame_basis();
    state.admit(2).unwrap();
    let original = complete_snapshot(&state, basis, 2, 1, u64::MAX);
    for (case, expected) in [
        Error::MalformedEvidence,
        Error::MalformedEvidence,
        Error::MalformedEvidence,
        Error::MalformedEvidence,
        Error::MalformedEvidence,
        Error::InvalidFrequency,
        Error::CoordinateMismatch,
        Error::UnsupportedEvidence,
        Error::BasisMismatch,
    ]
    .into_iter()
    .enumerate()
    {
        let mut snapshot = original.snapshot;
        let mut native_basis = Some(basis);
        let mut counter = NativeOutputCoordinate::Counter {
            position: 1,
            frequency: u64::MAX,
        };
        match case {
            0 => snapshot.clock = None,
            1 => snapshot.clock.as_mut().unwrap().host_point = None,
            2 => {
                snapshot.clock.as_mut().unwrap().reading_quality =
                    AudioClockReadingQuality::Degraded
            }
            3 => snapshot.telemetry_available = false,
            4 => {
                snapshot.clock.as_mut().unwrap().position = 0;
                counter = NativeOutputCoordinate::Counter {
                    position: 0,
                    frequency: u64::MAX,
                };
            }
            5 => {
                snapshot.clock.as_mut().unwrap().frequency = 0;
                counter = NativeOutputCoordinate::Counter {
                    position: 1,
                    frequency: 0,
                };
            }
            6 => snapshot.clock.as_mut().unwrap().frequency = u64::from(COMPLETE_SOURCE_HZ),
            7 => counter = NativeOutputCoordinate::StreamFrame { frame: 1 },
            _ => native_basis = Some(creation),
        }
        assert_eq!(
            CoordinateNativeAudioSnapshot::new(
                NativeAudioSnapshot {
                    epoch: 24,
                    basis,
                    evidence: OriginalNativePresentationEvidence::Wasapi {
                        snapshot,
                        basis: native_basis
                    },
                },
                counter,
            )
            .unwrap_err(),
            expected,
            "case {case}"
        );
        assert_eq!(state.pending_report(), original.snapshot.render);
        assert_eq!(state.pending_samples(), [0.0; 4]);
    }
}

#[test]
fn complete_ready_prefill_and_original_terminal_codes_never_become_native_progress() {
    use super::{observe_wasapi, ReplacementObservationError};
    use beatkernel_platform::audio::presentation::discipline::{
        DisciplineConfig, PresentationDiscipline,
    };
    let (_, mut state, _, identity) = complete_source_state();
    let basis = state.output_frame_basis();
    state.admit(2).unwrap();
    let original = complete_snapshot(&state, basis, 2, 1, u64::MAX);
    let query = coordinate(24, basis, original.snapshot)
        .source_query()
        .unwrap();
    let origin = basis.point_at_stream_frame(0).unwrap();
    let mut presentation = PresentationDiscipline::new(
        DisciplineConfig::default(),
        origin,
        ClockDomainId(8),
        Timestamp::ZERO,
    )
    .unwrap();
    presentation
        .rebind_output(24, origin, origin, Timestamp::ZERO)
        .unwrap();
    let mut ready = original;
    ready.snapshot.status = AudioStreamStatus::Ready;
    let generated = qualify_wasapi_source_packet(24, basis, identity, query, ready).unwrap();
    assert_eq!(generated.native_evidence(), Some(&ready.snapshot));
    assert!(!observe_wasapi(&mut presentation, 24, ready.snapshot, basis).unwrap());
    assert_eq!(presentation.latest_pair(), None);
    assert_eq!(
        CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch: 24,
                basis,
                evidence: OriginalNativePresentationEvidence::Wasapi {
                    snapshot: ready.snapshot,
                    basis: Some(basis)
                }
            },
            NativeOutputCoordinate::Counter {
                position: 1,
                frequency: u64::MAX
            },
        )
        .unwrap_err(),
        crate::native_output_coordinate::NativeOutputCoordinateError::MalformedEvidence
    );
    for code in [i32::MIN, -1, i32::MAX] {
        let mut failed = original;
        failed.snapshot.status = AudioStreamStatus::Failed { hresult: code };
        failed.snapshot.counters.native_failures = 1;
        let packet = qualify_wasapi_source_packet(24, basis, identity, query, failed).unwrap();
        assert_eq!(packet.native_evidence(), Some(&failed.snapshot));
        assert_eq!(
            observe_wasapi(&mut presentation, 24, failed.snapshot, basis),
            Err(ReplacementObservationError::Status(
                AudioStreamStatus::Failed { hresult: code }
            ))
        );
        assert_eq!(presentation.latest_pair(), None);
        assert_eq!(state.pending_report(), original.snapshot.render);
        assert_eq!(state.pending_samples(), [0.0; 4]);
    }
}

// These tests are typed only by the cached Windows check on a Linux host.
// They construct no stream/ticket and claim no native forwarding execution.
#[cfg(target_os = "windows")]
#[test]
fn complete_native_adapter_and_remix_retain_the_opaque_native_ticket_type() {
    use crate::gameplay::output::{
        adapters::{remix::RemixedOutputBackend, wasapi::WasapiCompleteReplacementBackend},
        ports::ConditionalSourceRetirementNativeOutputBackend,
    };
    use beatkernel_platform::windows::audio::SourceRetirementTicket;
    fn has_native_ticket<
        B: ConditionalSourceRetirementNativeOutputBackend<
            NativeOutputState,
            Ticket = SourceRetirementTicket,
        >,
    >() {
    }
    has_native_ticket::<WasapiCompleteReplacementBackend>();
    has_native_ticket::<RemixedOutputBackend<WasapiCompleteReplacementBackend>>();
}

#[cfg(target_os = "windows")]
#[test]
fn complete_adapter_error_keeps_primary_cleanup_and_raw_native_error_chain() {
    use crate::gameplay::output::adapters::wasapi::WasapiCompleteReplacementError;
    use beatkernel_platform::{
        audio::AudioPlatformError,
        windows::audio::{
            WasapiCompleteError, WasapiCompleteFailure, WasapiCompleteFailureReport,
            WasapiCompletePhase,
        },
    };
    use std::error::Error;
    let native = WasapiCompleteError {
        error: AudioPlatformError::Native { code: i32::MIN },
        report: WasapiCompleteFailureReport {
            primary: Some(WasapiCompleteFailure {
                phase: WasapiCompletePhase::Encode,
                code: i32::MIN,
            }),
            release: Some(-2),
            stop: Some(-3),
            start_cleanup: Some(-4),
            join: Some(-5),
            recovery: Some(-6),
        },
    };
    let saved = native.clone();
    let wrapped = WasapiCompleteReplacementError::Native(native);
    let recovered = wrapped
        .source()
        .unwrap()
        .downcast_ref::<WasapiCompleteError>()
        .unwrap();
    assert_eq!(recovered, &saved);
    assert_eq!(
        recovered
            .source()
            .unwrap()
            .downcast_ref::<AudioPlatformError>(),
        Some(&saved.error)
    );
}

#[cfg(target_os = "windows")]
mod complete_native_io_classifier {
    use super::NativeOutputState;
    use crate::gameplay::output::{
        adapters::{
            observation::ReplacementObservationError,
            wasapi::{WasapiCompleteReplacementBackend, WasapiCompleteReplacementError},
        },
        ports::{ConditionalSourceRetirementNativeOutputBackend, SourcePlaybackPacketError},
    };
    use crate::native_output_coordinate::NativeOutputCoordinateError;
    use beatkernel::time::ClockDomainId;
    use beatkernel_platform::{
        audio::{
            presentation::discipline::DisciplineError, AudioPlatformError, AudioStreamStatus,
            ConfigurationConstraint, PeriodConstraints,
        },
        windows::{
            audio::{
                WasapiCompleteError, WasapiCompleteFailure, WasapiCompleteFailureReport,
                WasapiCompletePhase, WasapiOptions,
            },
            clock::QpcClock,
        },
    };
    use std::error::Error;

    // An actual canonical backend, without opening an audio stream. On Linux
    // this Windows-gated test is foreign typing evidence only.
    fn backend() -> WasapiCompleteReplacementBackend {
        WasapiCompleteReplacementBackend::new(
            QpcClock::new(ClockDomainId(8)).unwrap(),
            WasapiOptions::default(),
            8,
        )
    }

    fn assert_native_classification(
        backend: &WasapiCompleteReplacementBackend,
        native: WasapiCompleteError,
        expected: bool,
    ) {
        let saved = native.clone();
        let error = WasapiCompleteReplacementError::Native(native);
        assert_eq!(
            <WasapiCompleteReplacementBackend as ConditionalSourceRetirementNativeOutputBackend<
                NativeOutputState,
            >>::is_source_native_io_error(backend, &error),
            expected,
            "{saved:?}"
        );
        // Classification borrows the complete error; it cannot replace the
        // original primary/report or flatten simultaneous cleanup failures.
        let original = error
            .source()
            .unwrap()
            .downcast_ref::<WasapiCompleteError>()
            .unwrap();
        assert_eq!(original, &saved);
        assert_eq!(
            original
                .source()
                .unwrap()
                .downcast_ref::<AudioPlatformError>(),
            Some(&saved.error)
        );
    }

    #[test]
    fn only_original_raw_io_and_liveness_are_eligible_for_authorization_reprobe() {
        let backend = backend();
        for error in [
            AudioPlatformError::Native { code: i32::MIN },
            AudioPlatformError::WorkerFailure,
            AudioPlatformError::DeviceInvalidated,
            AudioPlatformError::EndpointBusy,
            AudioPlatformError::ExclusiveDisabled,
        ] {
            for phase in [
                None,
                Some(WasapiCompletePhase::Padding),
                Some(WasapiCompletePhase::Acquire),
                Some(WasapiCompletePhase::Clock),
            ] {
                assert_native_classification(
                    &backend,
                    WasapiCompleteError {
                        error: error.clone(),
                        report: WasapiCompleteFailureReport {
                            primary: phase.map(|phase| WasapiCompleteFailure {
                                phase,
                                code: i32::MIN,
                            }),
                            ..WasapiCompleteFailureReport::default()
                        },
                    },
                    true,
                );
            }
        }
    }

    #[test]
    fn typed_packet_coordinate_policy_and_timing_errors_remain_visible() {
        let backend = backend();
        for error in [
            WasapiCompleteReplacementError::Playback(SourcePlaybackPacketError::BasisMismatch),
            WasapiCompleteReplacementError::Playback(SourcePlaybackPacketError::QueryOutsidePrefix),
            WasapiCompleteReplacementError::Playback(SourcePlaybackPacketError::InvalidSpan),
            WasapiCompleteReplacementError::Playback(SourcePlaybackPacketError::InvalidRevision),
            WasapiCompleteReplacementError::Coordinate(NativeOutputCoordinateError::BasisMismatch),
            WasapiCompleteReplacementError::Coordinate(
                NativeOutputCoordinateError::InvalidFrequency,
            ),
            WasapiCompleteReplacementError::Observation(ReplacementObservationError::EpochMismatch),
            WasapiCompleteReplacementError::Observation(ReplacementObservationError::Discipline(
                DisciplineError::FrequencyChanged,
            )),
            WasapiCompleteReplacementError::Observation(ReplacementObservationError::Status(
                AudioStreamStatus::Failed { hresult: i32::MIN },
            )),
        ] {
            assert!(
                !backend.is_source_native_io_error(&error),
                "typed error must remain visible: {error:?}"
            );
        }
        for error in [
            AudioPlatformError::InvalidRequest,
            AudioPlatformError::InvalidFormat,
            AudioPlatformError::RecoveryUnavailable,
            AudioPlatformError::DeviceUnavailable,
            AudioPlatformError::Capacity,
            AudioPlatformError::FormatUnsupported { closest: None },
            AudioPlatformError::ConfigurationUnsupported {
                constraint: ConfigurationConstraint::PeriodBounds,
                constraints: PeriodConstraints::default(),
                suggested_buffer_frames: Some(128),
                suggested_period_frames: Some(64),
            },
        ] {
            assert_native_classification(
                &backend,
                WasapiCompleteError {
                    error,
                    report: WasapiCompleteFailureReport::default(),
                },
                false,
            );
        }
    }

    #[test]
    fn local_range_uncertain_admission_and_every_cleanup_failure_remain_visible_intact() {
        let backend = backend();
        let raw = AudioPlatformError::Native { code: i32::MIN };
        for phase in [
            WasapiCompletePhase::Quarantined,
            WasapiCompletePhase::History,
            WasapiCompletePhase::Render,
            WasapiCompletePhase::Encode,
            WasapiCompletePhase::Release,
            WasapiCompletePhase::Admission,
            WasapiCompletePhase::Start,
            WasapiCompletePhase::Stop,
            WasapiCompletePhase::Join,
            WasapiCompletePhase::Recovery,
        ] {
            assert_native_classification(
                &backend,
                WasapiCompleteError {
                    error: raw.clone(),
                    report: WasapiCompleteFailureReport {
                        primary: Some(WasapiCompleteFailure {
                            phase,
                            code: i32::MIN,
                        }),
                        ..WasapiCompleteFailureReport::default()
                    },
                },
                false,
            );
        }
        const INVALID_ARGUMENT: i32 = 0x8007_0057u32 as i32;
        for native_code in [false, true] {
            assert_native_classification(
                &backend,
                WasapiCompleteError {
                    error: if native_code {
                        AudioPlatformError::Native {
                            code: INVALID_ARGUMENT,
                        }
                    } else {
                        raw.clone()
                    },
                    report: WasapiCompleteFailureReport {
                        primary: Some(WasapiCompleteFailure {
                            phase: WasapiCompletePhase::Clock,
                            code: if native_code {
                                i32::MIN
                            } else {
                                INVALID_ARGUMENT
                            },
                        }),
                        ..WasapiCompleteFailureReport::default()
                    },
                },
                false,
            );
        }
        for cleanup in 0..6 {
            let mut report = WasapiCompleteFailureReport {
                primary: Some(WasapiCompleteFailure {
                    phase: WasapiCompletePhase::Clock,
                    code: i32::MIN,
                }),
                ..WasapiCompleteFailureReport::default()
            };
            match cleanup {
                0 => report.release = Some(-2),
                1 => report.stop = Some(-3),
                2 => report.start_cleanup = Some(-4),
                3 => report.join = Some(-5),
                4 => report.recovery = Some(-6),
                _ => {
                    report.primary.as_mut().unwrap().phase = WasapiCompletePhase::Quarantined;
                    report.release = Some(-2);
                    report.stop = Some(-3);
                    report.start_cleanup = Some(-4);
                    report.join = Some(-5);
                    report.recovery = Some(-6);
                }
            }
            assert_native_classification(
                &backend,
                WasapiCompleteError {
                    error: raw.clone(),
                    report,
                },
                false,
            );
        }
    }
}
