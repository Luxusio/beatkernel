//! Real Mixer adoption history with explicitly scripted native timing packets.
//! Scripted status fields prove adapter consistency, never device acquisition.
use super::*;
use crate::gameplay::output::ports::{SourcePlaybackIdentity, SourcePlaybackNativeOutputBackend};
use beatkernel::{
    audio::{
        command_queue, command_queue_with_start_gate, AudioFormat, AudioLimits, CommandProducer,
        InterpolatedTargetTime, Mixer, MixerConfig, PcmLimits, SampleBank, SourcePlaybackState,
        TargetTime,
    },
    time::{ClockDomainId, ClockMappingQuality, ClockPoint, Timestamp},
};
use beatkernel_platform::{
    audio::{DeviceFormat, SampleEncoding},
    linux::{AlsaNativeTimestamp, AlsaSourcePlaybackSnapshot, AlsaTimingSnapshot},
};

fn rig(gated: bool, hz: u32) -> (CommandProducer, NativeOutputState) {
    let format = AudioFormat::new(hz, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(256, 512, 1).unwrap()).unwrap();
    let (producer, consumer) = if gated {
        command_queue_with_start_gate(8)
    } else {
        command_queue(8)
    }
    .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::from_nanos(-123),
            AudioLimits::new(8, 2, 8, 256, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(16).unwrap();
    let native = DeviceFormat::new(hz, 1, SampleEncoding::Float32, None).unwrap();
    (
        producer,
        NativeOutputState::new(mixer, native, None, 64)
            .unwrap_or_else(|_| panic!("valid ordinary source owner")),
    )
}
fn at(frame: u64, hz: u32) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, hz).unwrap(), 0, 0, 1, 0, 1).unwrap()
}
fn native(submitted: u64, played: u64) -> AlsaTimingSnapshot {
    let point = |ns| ClockPoint {
        domain: ClockDomainId(8),
        timestamp: Timestamp::from_nanos(ns),
    };
    AlsaTimingSnapshot {
        native_state: 3,
        submitted_frames: submitted,
        delay_frames: i64::try_from(submitted - played).unwrap(),
        available_frames: 91,
        native_htstamp: AlsaNativeTimestamp {
            seconds: 1,
            nanoseconds: 123,
        },
        native_timestamp: Some(point(1_000_000_123)),
        query_started: point(1_000_000_100),
        query_finished: point(1_000_000_140),
        estimated_played_frames: Some(played),
        quality: ClockMappingQuality::Unknown,
        timestamp_mode: 1,
        timestamp_type: 1,
    }
}
fn actual_facts() -> (
    beatkernel::audio::OutputFrameBasis,
    SourcePlaybackIdentity,
    AlsaSourcePlaybackSnapshot,
) {
    let (mut producer, mut owner) = rig(false, 8);
    let basis = owner.output_frame_basis();
    owner.render_pending(4).unwrap();
    owner.admit(4).unwrap();
    producer.request_pause(true);
    let last = owner.render_pending(4).unwrap();
    owner.admit(4).unwrap();
    let history = owner.mixer().source_playback_history().unwrap();
    let id = SourcePlaybackIdentity::new(
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
    )
    .unwrap();
    (
        basis,
        id,
        AlsaSourcePlaybackSnapshot {
            playback: history.lookup_at_source_time(at(2, 8)).unwrap(),
            pulled_source_end: history.pulled_end_frame(),
            submitted_source_end: 8,
            render: Some(last),
            timing: Some(native(8, 2)),
            revision: 4,
        },
    )
}

#[test]
fn actual_mixer_pause_history_and_original_native_fields_survive_qualification() {
    let (basis, id, facts) = actual_facts();
    assert_eq!(facts.playback.state(), SourcePlaybackState::Running);
    assert_eq!(facts.playback.physical_end(), Some(4));
    assert!(facts.render.unwrap().paused);
    let candidate = facts.timing.unwrap();
    let packet = qualify_alsa_source_packet(23, basis, id, at(2, 8), facts).unwrap();
    let observation = qualify_alsa_observation_packet(23, basis, id, candidate, packet)
        .unwrap()
        .unwrap();
    assert_eq!(observation.source().native_evidence(), Some(&candidate));
    assert_eq!(observation.source().facts().render, facts.render);
    assert_eq!(
        observation.coordinate().coordinate(),
        crate::native_output_coordinate::NativeOutputCoordinate::StreamFrame { frame: 2 }
    );
    assert_eq!(
        observation.source().native_evidence().unwrap().quality,
        ClockMappingQuality::Unknown
    );
    assert_eq!(observation.coordinate().snapshot().basis, basis);
}
#[test]
fn absent_timing_preserves_source_facts_without_substituting_candidate_clock() {
    let (basis, id, mut facts) = actual_facts();
    let candidate = facts.timing.take().unwrap();
    let packet = qualify_alsa_source_packet(23, basis, id, at(2, 8), facts).unwrap();
    assert!(packet.native_evidence().is_none());
    assert_eq!(packet.facts().submitted_source_end, 8);
    assert!(
        qualify_alsa_observation_packet(23, basis, id, candidate, packet)
            .unwrap()
            .is_none()
    );
}
#[test]
fn complete_native_candidate_equality_includes_raw_fields_not_only_played_frame() {
    let (basis, id, facts) = actual_facts();
    for case in 0..14 {
        let mut candidate = facts.timing.unwrap();
        match case {
            0 => candidate.native_state = 4,
            1 => candidate.submitted_frames += 1,
            2 => candidate.delay_frames = -1,
            3 => candidate.available_frames += 1,
            4 => candidate.native_htstamp.nanoseconds += 1,
            5 => candidate.native_timestamp = None,
            6 => candidate.query_started.timestamp = Timestamp::from_nanos(1_000_000_101),
            7 => candidate.query_finished.timestamp = Timestamp::from_nanos(1_000_000_141),
            8 => candidate.estimated_played_frames = Some(3),
            9 => candidate.timestamp_mode = 0,
            10 => candidate.timestamp_type = 0,
            11 => candidate.query_finished.domain = ClockDomainId(9),
            12 => candidate.quality = ClockMappingQuality::Exact,
            _ => candidate.native_htstamp.seconds += 1,
        }
        let packet = qualify_alsa_source_packet(23, basis, id, at(2, 8), facts).unwrap();
        assert!(
            qualify_alsa_observation_packet(23, basis, id, candidate, packet)
                .unwrap()
                .is_none(),
            "case {case}"
        );
    }
}
#[test]
fn matching_but_malformed_native_packets_are_rejected_without_new_observation() {
    let (basis, id, facts) = actual_facts();
    for case in 0..10 {
        let mut invalid = facts;
        let mut raw = invalid.timing.unwrap();
        match case {
            0 => raw.native_state = 4,
            1 => raw.delay_frames = -1,
            2 => raw.native_htstamp.nanoseconds = 1_000_000_000,
            3 => raw.native_htstamp.seconds = -1,
            4 => {
                raw.native_timestamp.as_mut().unwrap().timestamp =
                    Timestamp::from_nanos(1_000_000_124)
            }
            5 => raw.query_started.timestamp = Timestamp::from_nanos(1_000_000_141),
            6 => raw.query_finished.domain = ClockDomainId(9),
            7 => raw.timestamp_mode = 0,
            8 => raw.quality = ClockMappingQuality::Exact,
            _ => raw.timestamp_type = 0,
        }
        invalid.timing = Some(raw);
        let packet = qualify_alsa_source_packet(23, basis, id, at(2, 8), invalid).unwrap();
        assert!(
            qualify_alsa_observation_packet(23, basis, id, raw, packet).is_err(),
            "case {case}"
        );
    }
}
#[test]
fn timing_submission_and_exact_query_must_match_real_positive_prefix() {
    let (basis, id, facts) = actual_facts();
    let mut mismatch = facts;
    mismatch.timing.as_mut().unwrap().submitted_frames = 7;
    assert!(qualify_alsa_source_packet(23, basis, id, at(2, 8), mismatch).is_err());
    let future =
        InterpolatedTargetTime::new(TargetTime::from_frames(8, 8).unwrap(), 0, 1, u64::MAX, 1, 1)
            .unwrap();
    let mut frozen = facts;
    frozen.playback =
        beatkernel::audio::SourcePlaybackSpan::new(4, None, 4, SourcePlaybackState::Frozen)
            .unwrap();
    assert!(qualify_alsa_source_packet(23, basis, id, future, frozen).is_err());
    let packet = qualify_alsa_source_packet(23, basis, id, at(3, 8), facts).unwrap();
    assert!(qualify_alsa_observation_packet(23, basis, id, facts.timing.unwrap(), packet).is_err());
}
#[test]
fn actual_start_gate_and_pending_reopen_basis_keep_history_origin_distinct() {
    let (mut producer, mut owner) = rig(true, 8);
    producer.schedule_start_at(2).unwrap();
    let report = owner.render_pending(4).unwrap();
    owner.admit(2).unwrap();
    let basis = owner.output_frame_basis();
    assert_eq!(basis.start_physical_frame(), 2);
    let history = owner.mixer().source_playback_history().unwrap();
    let id = SourcePlaybackIdentity::new(
        8,
        history.initial_physical_frame(),
        history.initial_playback_frame(),
    )
    .unwrap();
    assert_eq!(id.initial_physical(), 0);
    let query = at(3, 8);
    let facts = AlsaSourcePlaybackSnapshot {
        playback: history.lookup_at_source_time(query).unwrap(),
        pulled_source_end: 4,
        submitted_source_end: 4,
        render: Some(report),
        timing: Some(native(2, 1)),
        revision: 6,
    };
    let packet = qualify_alsa_source_packet(31, basis, id, query, facts).unwrap();
    assert_eq!(packet.facts().playback.playback_start(), 0);
    let observation = qualify_alsa_observation_packet(31, basis, id, facts.timing.unwrap(), packet)
        .unwrap()
        .unwrap();
    assert_eq!(
        observation.coordinate().coordinate(),
        crate::native_output_coordinate::NativeOutputCoordinate::StreamFrame { frame: 1 }
    );
    assert_eq!(owner.pending_frames(), 2);
    assert_eq!(producer.applied_start_frame(), Some(2));
}

#[test]
#[ignore = "requires explicit BEATKERNEL_TEST_ALSA_DEVICE=null or native endpoint; software lifecycle evidence only"]
fn actual_native_source_history_positive_submission_and_stop_recover_complete_owner() {
    let device =
        std::env::var("BEATKERNEL_TEST_ALSA_DEVICE").expect("explicit native endpoint required");
    let (mut producer, owner) = rig(false, 48_000);
    let identity = SourcePlaybackIdentity::new(48_000, 0, 0).unwrap();
    let request = AlsaRequest {
        device,
        format: DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap(),
        buffer_frames: 256,
        period_frames: 64,
        allow_size_rounding: false,
        monotonic_domain: ClockDomainId(8),
    };
    let mut backend = AlsaReplacementBackend;
    let mut output = backend
        .open(request, owner, 41)
        .unwrap_or_else(|failure| panic!("native open refused: {}", failure.error()));
    assert!(backend
        .observe_native_playback(&mut output, identity)
        .unwrap()
        .is_none());
    backend.start(&mut output).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut submitted = None;
    while std::time::Instant::now() < deadline {
        // These independent atomic diagnostics prove positive native writes,
        // without demanding a bounded compound read beat an unpaced null writer.
        let snapshot = output.stream().snapshot();
        if snapshot.submitted_frames > 0 {
            submitted = Some(snapshot.submitted_frames);
            break;
        }
        assert!(matches!(
            snapshot.status,
            AlsaStatus::Ready | AlsaStatus::Running
        ));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // Some native null implementations expose no usable played timestamp.
    // A missing observation is legitimate; no synthetic association replaces it.
    if let Some(observation) = backend
        .observe_native_playback(&mut output, identity)
        .unwrap()
    {
        assert_eq!(observation.coordinate().snapshot().epoch, 41);
        assert!(observation.source().native_evidence().is_some());
    }
    backend.retire(&mut output).unwrap();
    assert!(
        submitted.is_some(),
        "real worker must positively submit within bounded collection"
    );
    let packet = backend
        .source_playback_at(&output, &at(0, 48_000), identity)
        .unwrap()
        .expect("joined writer must expose stable real source/admission facts");
    assert_eq!(packet.epoch(), 41);
    assert!(packet.facts().submitted_source_end > 0);
    assert!(packet.facts().submitted_source_end >= submitted.unwrap());
    assert!(packet.facts().submitted_source_end <= packet.facts().pulled_source_end);
    assert!(packet.native_evidence().is_none());
    let recovered = output.take_stopped_mixer().unwrap().unwrap();
    let history = recovered.mixer().source_playback_history().unwrap();
    assert!(history.pulled_end_frame() >= submitted.unwrap());
    assert_eq!(history.pulled_end_frame(), packet.facts().pulled_source_end);
    assert_eq!(history.initial_physical_frame(), 0);
    assert_eq!(history.source_rate(), 48_000);
    assert_eq!(recovered.format().unwrap().sample_rate(), 48_000);
    assert!(output.take_stopped_mixer().unwrap().is_none());
    producer.request_pause(true);
    assert!(output.stream().timing_snapshot().is_none());
}
