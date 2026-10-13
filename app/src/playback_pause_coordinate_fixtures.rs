//! Real software mixer/adoption and exact authority contracts, not device timing proof.
use super::*;
use crate::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
        PreparedCoordinateControlCutoff,
    },
    gameplay::output::ports::SourcePlaybackIdentity,
    live_pause::prepare_coordinate_live_transport,
    local_input::InputMerger,
    native_output_coordinate::NativeOutputCoordinate,
};
use beatkernel::{
    audio::*,
    input::EventMeta,
    time::{Duration, ExtrapolationPolicy},
    transport::{OutputPhaseBasis, Rate, Transport},
};
use std::cmp::Ordering;

fn p(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn h(ns: i64) -> ClockPoint {
    p(1, ns)
}
fn identity(hz: u32) -> SourcePlaybackIdentity {
    SourcePlaybackIdentity::new(hz, 0, 0).unwrap()
}
fn descriptor() -> AudioAuthorityCoordinateBasis {
    AudioAuthorityCoordinateBasis::SourceCounter {
        basis: OutputFrameBasis::new(p(2, 0), 3, 0).unwrap(),
        frequency: 12_000_000_000,
    }
}
fn mixer(hz: u32, gate: Option<u64>, end: Option<u64>) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(hz, 1).unwrap();
    let pcm = PcmLimits::new(256, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 64], pcm).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = if gate.is_some() {
        command_queue_with_start_gate(8).unwrap()
    } else {
        command_queue(8).unwrap()
    };
    if let Some(frame) = gate {
        producer.schedule_start_at(frame).unwrap();
    }
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mut config = MixerConfig::new(
        format,
        ClockDomainId(2),
        Timestamp::ZERO,
        AudioLimits::new(8, 4, 8, 64, 8).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn authority(basis: AudioAuthorityCoordinateBasis) -> AudioAuthority {
    AudioAuthority::new_with_coordinates(
        AudioAuthorityConfig {
            history_capacity: 16,
            max_observation_age: Duration::from_nanos(1_000_000_000),
            input_extrapolation: ExtrapolationPolicy::Forbid,
            max_input_ahead: Duration::ZERO,
        },
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: basis.origin().unwrap(),
            logical_origin: p(3, 500),
            host_domain: ClockDomainId(1),
        },
        basis,
    )
    .unwrap()
}
fn coordinate_point(
    basis: AudioAuthorityCoordinateBasis,
    coordinate: NativeOutputCoordinate,
) -> ClockPoint {
    match (basis, coordinate) {
        (
            AudioAuthorityCoordinateBasis::SourceFrame(basis),
            NativeOutputCoordinate::StreamFrame { frame },
        ) => basis.point_at_stream_frame(frame).unwrap(),
        (
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
            NativeOutputCoordinate::StreamFrame { frame },
        ) => basis.point_at_stream_frame(frame).unwrap(),
        (
            AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency },
            NativeOutputCoordinate::Counter {
                position,
                frequency: actual,
            },
        ) => {
            assert_ne!(frequency, 0);
            assert_eq!(frequency, actual);
            basis.point_at_native_counter(position, actual).unwrap()
        }
        _ => panic!("native coordinate must match its original creation descriptor"),
    }
}
fn observe(a: &mut AudioAuthority, units: u64, host: i64) {
    let basis = a.coordinate_basis().unwrap();
    let coordinate = match basis {
        AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. } => {
            NativeOutputCoordinate::Counter {
                position: units,
                frequency,
            }
        }
        _ => NativeOutputCoordinate::StreamFrame { frame: units },
    };
    a.observe_coordinate(
        a.epoch().id,
        ClockPair {
            source: coordinate_point(basis, coordinate),
            target: h(host),
        },
        coordinate,
    )
    .unwrap();
}
fn request(pause: &mut NativePause, desired: bool, units: u64, host: i64) -> bool {
    let mut a = authority(pause.coordinate_binding.unwrap().basis);
    if pause.epoch() != a.epoch().id {
        let mut epoch = a.epoch();
        epoch.id = pause.epoch();
        a = AudioAuthority::new_with_coordinates(
            AudioAuthorityConfig::default(),
            epoch,
            pause.coordinate_binding.unwrap().basis,
        )
        .unwrap();
    }
    observe(&mut a, units, host);
    pause.request_coordinate(desired, &a).unwrap()
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), h(i64::MIN), vec![DeviceId(9)], 8).unwrap()
}
fn event(host: i64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), h(host), 0),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    })
}
fn query(t: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(t, 0, 0, 1, 0, 1).unwrap()
}
fn token(
    a: &AudioAuthority,
    cut: InterpolatedTargetTime,
    resume: bool,
    now: ClockPoint,
    m: &InputMerger,
) -> PreparedCoordinateControlCutoff {
    let host = a
        .presented_coordinate_boundary_host(&cut, now)
        .unwrap()
        .unwrap();
    if resume {
        a.prepare_coordinate_resume_control_cutoff(7, cut, host, now, m)
    } else {
        a.prepare_coordinate_control_cutoff(7, cut, host, now, m)
    }
    .unwrap()
    .unwrap()
}
fn staged() -> (
    NativePause,
    PreparedCoordinatePauseAdoption,
    CommandProducer,
    Mixer,
) {
    let (mut producer, mut mixer) = mixer(3, None, None);
    assert_eq!(mixer.render(&mut [0.0; 1]).unwrap().playback_frames, 1);
    let mut pause = NativePause::new(p(2, 0), ClockDomainId(1), 3)
        .unwrap()
        .with_coordinate_binding(7, descriptor(), identity(3))
        .unwrap();
    assert!(request(&mut pause, true, 0, 0));
    producer.request_pause(true);
    let report = mixer.render(&mut [0.0; 2]).unwrap();
    assert!(report.paused);
    assert_eq!(
        (
            report.start_frame,
            report.playback_start_frame,
            report.playback_frames
        ),
        (1, 1, 0)
    );
    let before = pause.clone();
    let prepared = pause
        .prepare_coordinate_adoption(Some(report), None)
        .unwrap()
        .unwrap();
    assert_eq!(pause, before);
    (pause, prepared, producer, mixer)
}
fn crossed() -> AudioAuthority {
    let mut a = authority(descriptor());
    observe(&mut a, 3_999_999_998, 100);
    observe(&mut a, 4_000_000_002, 104);
    a.record_acquired_prefix(h(104)).unwrap();
    a
}

#[test]
fn real_pause_staging_is_readonly_and_missing_adoption_cannot_ack() {
    let (pause, prepared, _, _) = staged();
    assert_eq!(pause.phase(), PausePhase::Pausing);
    assert_eq!(
        (prepared.physical_frame(), prepared.playback_frame()),
        (1, 1)
    );
    assert_eq!(
        prepared
            .exact_cut()
            .compare_interpolated_time(query(TargetTime::from_frames(1, 3).unwrap())),
        Ordering::Equal
    );
    assert!(pause
        .prepare_coordinate_adoption(None, None)
        .unwrap()
        .is_none());
}

#[test]
fn fractional_counter_before_equal_after_share_floor_but_only_crossed_cut_qualifies() {
    let (pause, prepared, _, _) = staged();
    let mut a = authority(descriptor());
    observe(&mut a, 3_999_999_999, 101);
    a.record_acquired_prefix(h(101)).unwrap();
    assert!(a
        .presented_coordinate_boundary_host(&prepared.exact_cut(), h(101))
        .unwrap()
        .is_none());
    observe(&mut a, 4_000_000_000, 102);
    observe(&mut a, 4_000_000_001, 103);
    a.record_acquired_prefix(h(103)).unwrap();
    for (at, ordering) in [
        (101, Ordering::Less),
        (102, Ordering::Equal),
        (103, Ordering::Greater),
    ] {
        let input = a.prepare_coordinate_input(h(at), h(103)).unwrap().unwrap();
        assert_eq!(input.raw_output(), p(2, 333_333_333));
        assert_eq!(
            input
                .query()
                .compare_interpolated_time(prepared.exact_cut()),
            ordering
        );
    }
    let cutoff = token(&a, prepared.exact_cut(), false, h(103), &merger());
    assert_eq!(cutoff.host(), h(102));
    assert_eq!(
        pause
            .validate_coordinate_adoption(&prepared, &a, &cutoff, h(103), &merger())
            .unwrap()
            .host,
        h(102)
    );
    assert_eq!(pause.phase(), PausePhase::Pausing);
}

#[test]
fn pause_equality_must_drain_and_native_ack_precedes_authority_commit() {
    let (mut pause, prepared, _, _) = staged();
    let mut a = crossed();
    let mut m = merger();
    let cutoff = token(&a, prepared.exact_cut(), false, h(104), &m);
    let payload = event(102);
    m.admit(payload.clone(), h(104)).unwrap();
    let before = pause.clone();
    assert!(pause
        .commit_coordinate_adoption(prepared.clone(), &a, &cutoff, h(104), &m)
        .is_err());
    assert_eq!(pause, before);
    assert_eq!(m.pop_ready(h(104)).unwrap(), Some(payload));
    let boundary = pause
        .commit_coordinate_adoption(prepared.clone(), &a, &cutoff, h(104), &m)
        .unwrap();
    assert!(boundary.paused);
    assert_eq!(boundary.raw_output, p(2, 333_333_333));
    assert_eq!(boundary.output, p(3, 333_333_833));
    assert_eq!(a.committed_operation(), None);
    a.commit_coordinate_control_cutoff(cutoff, h(104), &m)
        .unwrap();
    assert_eq!(pause.phase(), PausePhase::Paused);
    assert!(pause
        .commit_coordinate_adoption(prepared, &a, &cutoff, h(104), &m)
        .is_err());
}

#[test]
fn committing_authority_first_invalidates_native_ack_atomically() {
    let (mut pause, prepared, _, _) = staged();
    let mut a = crossed();
    let m = merger();
    let cutoff = token(&a, prepared.exact_cut(), false, h(104), &m);
    a.commit_coordinate_control_cutoff(cutoff, h(104), &m)
        .unwrap();
    let before = pause.clone();
    assert!(pause
        .commit_coordinate_adoption(prepared, &a, &cutoff, h(104), &m)
        .is_err());
    assert_eq!(pause, before);
}

#[test]
fn distinct_cut_with_identical_host_and_floor_cannot_substitute_actual_adoption() {
    let (mut pause, prepared, _, _) = staged();
    let a = crossed();
    let m = merger();
    let distinct = InterpolatedTargetTime::new(
        TargetTime::from_frames(0, 1).unwrap(),
        3_999_999_999,
        4_000_000_000,
        12_000_000_000,
        1,
        2,
    )
    .unwrap();
    let actual = token(&a, prepared.exact_cut(), false, h(104), &m);
    let wrong = token(&a, distinct, false, h(104), &m);
    assert_eq!(actual.host(), wrong.host());
    assert_eq!(actual.raw_output(), wrong.raw_output());
    assert_eq!(actual.output(), wrong.output());
    let before = pause.clone();
    assert!(pause
        .commit_coordinate_adoption(prepared, &a, &wrong, h(104), &m)
        .is_err());
    assert_eq!(pause, before);
}

#[test]
fn mathematically_equivalent_cut_encoding_is_accepted() {
    let (mut pause, prepared, _, _) = staged();
    let a = crossed();
    let m = merger();
    let equivalent = InterpolatedTargetTime::new(
        TargetTime::from_frames(0, 1).unwrap(),
        8_000_000_000,
        8_000_000_000,
        24_000_000_000,
        0,
        1,
    )
    .unwrap();
    assert_eq!(
        equivalent.compare_interpolated_time(prepared.exact_cut()),
        Ordering::Equal
    );
    let cutoff = token(&a, equivalent, false, h(104), &m);
    pause
        .commit_coordinate_adoption(prepared, &a, &cutoff, h(104), &m)
        .unwrap();
    assert_eq!(pause.phase(), PausePhase::Paused);
}

#[test]
fn changed_full_owner_snapshot_and_changed_binding_refuse_old_preparation() {
    let (mut pause, prepared, _, _) = staged();
    let a = crossed();
    let m = merger();
    let cutoff = token(&a, prepared.exact_cut(), false, h(104), &m);
    // A prior semantic field outside phase/frame/gap must also invalidate it.
    pause.last_now = Some(h(1));
    let before = pause.clone();
    assert!(pause
        .commit_coordinate_adoption(prepared.clone(), &a, &cutoff, h(104), &m)
        .is_err());
    assert_eq!(pause, before);
    let mut other = NativePause::new(p(2, 0), ClockDomainId(1), 3)
        .unwrap()
        .with_coordinate_binding(8, descriptor(), identity(3))
        .unwrap();
    assert!(request(&mut other, true, 0, 0));
    let before = other.clone();
    assert!(other
        .commit_coordinate_adoption(prepared, &a, &cutoff, h(104), &m)
        .is_err());
    assert_eq!(other, before);
}

#[test]
fn pending_earlier_input_and_stale_authority_freshness_cannot_ack() {
    for mode in 0..3 {
        let (mut pause, prepared, _, _) = staged();
        let mut a = crossed();
        let mut m = merger();
        let cutoff = token(&a, prepared.exact_cut(), false, h(104), &m);
        let now = match mode {
            0 => {
                m.admit(event(101), h(104)).unwrap();
                h(104)
            }
            1 => {
                a.record_acquired_prefix(h(105)).unwrap();
                h(105)
            }
            _ => h(1_000_000_105),
        };
        let before = pause.clone();
        assert!(pause
            .commit_coordinate_adoption(prepared, &a, &cutoff, now, &m)
            .is_err());
        assert_eq!(pause, before);
    }
}

#[test]
fn coordinate_binding_is_cold_explicit_and_source_scope_checked() {
    for (basis, id) in [
        (descriptor(), identity(4)),
        (
            AudioAuthorityCoordinateBasis::SourceFrame(
                OutputFrameBasis::new(p(2, 1), 3, 0).unwrap(),
            ),
            identity(3),
        ),
        (descriptor(), SourcePlaybackIdentity::new(3, 2, 0).unwrap()),
    ] {
        assert!(NativePause::new(p(2, 0), ClockDomainId(1), 3)
            .unwrap()
            .with_coordinate_binding(7, basis, id)
            .is_err());
    }
    let mut cold = NativePause::new(p(2, 0), ClockDomainId(1), 3).unwrap();
    cold.request(
        true,
        ClockPair {
            source: p(2, 0),
            target: h(0),
        },
    )
    .unwrap();
    assert!(cold
        .with_coordinate_binding(7, descriptor(), identity(3))
        .is_err());
}

#[test]
fn legacy_scalar_observation_cannot_emit_coordinate_bound_ack() {
    let (mut pause, _, _, mut mixer) = staged();
    let report = mixer.render(&mut [0.0; 1]).unwrap();
    let before = pause.clone();
    assert!(pause
        .observe(
            Some(report),
            ClockPair {
                source: p(2, 1_000_000_000),
                target: h(104)
            }
        )
        .is_err());
    assert_eq!(pause, before);
    let interval = PauseIntervalObservation {
        output_origin: p(2, 0),
        sample_rate: 3,
        render: report,
        clock: crate::native_start::StartInterval::new(p(2, 1_000_000_000), h(104), h(105))
            .unwrap(),
    };
    assert!(pause.observe_interval(Some(interval), h(105)).is_err());
    assert_eq!(pause, before);
    assert!(pause.rebind_output(8, &mixer).is_err());
    assert_eq!(pause, before);
    assert!(pause
        .request_in_epoch(
            7,
            true,
            ClockPair {
                source: p(2, 0),
                target: h(0)
            }
        )
        .is_err());
    assert_eq!(pause, before);
}

#[test]
fn genuine_resume_preserves_frozen_frame_gap_and_equal_pending_payload() {
    let (mut pause, prepared, mut producer, mut mixer) = staged();
    let mut a = crossed();
    let mut m = merger();
    let cutoff = token(&a, prepared.exact_cut(), false, h(104), &m);
    pause
        .commit_coordinate_adoption(prepared, &a, &cutoff, h(104), &m)
        .unwrap();
    a.commit_coordinate_control_cutoff(cutoff, h(104), &m)
        .unwrap();
    assert!(pause.request_coordinate(false, &a).unwrap());
    producer.request_pause(false);
    let report = mixer.render(&mut [0.0; 1]).unwrap();
    assert_eq!(
        (
            report.start_frame,
            report.playback_start_frame,
            report.playback_frames
        ),
        (3, 1, 1)
    );
    let prepared = pause
        .prepare_coordinate_adoption(Some(report), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        (prepared.physical_frame(), prepared.playback_frame()),
        (3, 1)
    );
    observe(&mut a, 12_000_000_000, 200);
    a.record_acquired_prefix(h(200)).unwrap();
    let cutoff = token(&a, prepared.exact_cut(), true, h(200), &m);
    let equality = event(200);
    m.admit(equality.clone(), h(200)).unwrap();
    let boundary = pause
        .commit_coordinate_adoption(prepared, &a, &cutoff, h(200), &m)
        .unwrap();
    assert!(!boundary.paused);
    assert_eq!(boundary.playback_frame, 1);
    a.commit_coordinate_control_cutoff(cutoff, h(200), &m)
        .unwrap();
    assert_eq!(m.peek_ready(h(200)).unwrap(), Some(&equality));
    assert_eq!(pause.phase(), PausePhase::Running);
    assert_eq!(
        pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
        Timestamp::from_nanos(-666_666_666)
    );
}

#[test]
fn nonzero_attempt_origin_preserves_original_preroll_phase_for_all_selected_rates() {
    for rate in [
        Rate::NORMAL,
        Rate::new(1, 2).unwrap(),
        Rate::new(3, 2).unwrap(),
        Rate::new(2, 1).unwrap(),
    ] {
        let (mut producer, mut mixer) = mixer(3, None, None);
        mixer.render(&mut [0.0; 2]).unwrap();
        let mut pause = NativePause::new(p(2, 0), ClockDomainId(1), 3)
            .unwrap()
            .with_coordinate_binding(7, descriptor(), identity(3))
            .unwrap();
        assert!(request(&mut pause, true, 0, 0));
        producer.request_pause(true);
        let prepared = pause
            .prepare_coordinate_adoption(Some(mixer.render(&mut [0.0; 2]).unwrap()), None)
            .unwrap()
            .unwrap();
        let mut a = authority(descriptor());
        observe(&mut a, 0, 100);
        observe(&mut a, 8_000_000_000, 200);
        a.record_acquired_prefix(h(200)).unwrap();
        let m = merger();
        let cutoff = token(&a, prepared.exact_cut(), false, h(200), &m);
        let phase = OutputPhaseBasis::new(ClockDomainId(3), 91, 113, 1, 3).unwrap();
        let transport = Transport::new_with_output_phase(
            phase,
            p(3, 500),
            Timestamp::from_nanos(100_001),
            Duration::from_nanos(333_333_334),
            rate,
            None,
        )
        .unwrap();
        let point = phase
            .point_from_elapsed_nanos(cutoff.output(), 333_333_333)
            .unwrap();
        let song = transport.position_at_output_phase(point).unwrap();
        let expected = match (rate.numerator(), rate.denominator()) {
            (1, 1) => 100_000,
            (1, 2) => 100_001,
            (3, 2) => 100_000,
            (2, 1) => 99_999,
            _ => unreachable!(),
        };
        assert_eq!(song, Timestamp::from_nanos(expected));
        let paused =
            prepare_coordinate_live_transport(&transport, &prepared, &cutoff, phase, song).unwrap();
        assert!(!transport.is_paused());
        assert!(paused.is_paused());
        assert_eq!(paused.position_at_output_phase(point).unwrap(), song);
        assert_eq!(paused.preroll_setup().unwrap().positive_rate, rate);
        pause
            .commit_coordinate_adoption(prepared, &a, &cutoff, h(200), &m)
            .unwrap();
        a.commit_coordinate_control_cutoff(cutoff, h(200), &m)
            .unwrap();
        assert!(pause.request_coordinate(false, &a).unwrap());
        producer.request_pause(false);
        let resume = pause
            .prepare_coordinate_adoption(Some(mixer.render(&mut [0.0; 1]).unwrap()), None)
            .unwrap()
            .unwrap();
        observe(&mut a, 16_000_000_000, 300);
        a.record_acquired_prefix(h(300)).unwrap();
        let cutoff = token(&a, resume.exact_cut(), true, h(300), &m);
        let resumed =
            prepare_coordinate_live_transport(&paused, &resume, &cutoff, phase, song).unwrap();
        assert!(!resumed.is_paused());
        assert_eq!(resumed.output_phase_basis(), Some(phase));
        assert_eq!(resumed.preroll_setup(), transport.preroll_setup());
        let at = phase
            .point_from_elapsed_nanos(cutoff.output(), 333_333_333)
            .unwrap();
        assert_eq!(resumed.position_at_output_phase(at).unwrap(), song);
        let later = phase
            .point_from_elapsed_nanos(p(3, cutoff.output().timestamp.as_nanos() + 1), 333_333_336)
            .unwrap();
        let literal = 100_001 + 2 * rate.numerator() as i64 / rate.denominator() as i64;
        assert_eq!(
            resumed.position_at_output_phase(later).unwrap(),
            Timestamp::from_nanos(literal)
        );
    }
}

#[test]
fn phase_preflight_rejects_wrong_attempt_source_rate_prefix_and_control_kind_readonly() {
    let (_, prepared, _, _) = staged();
    let a = crossed();
    let m = merger();
    let cutoff = token(&a, prepared.exact_cut(), false, h(104), &m);
    let phase = OutputPhaseBasis::new(ClockDomainId(3), 91, 113, 0, 3).unwrap();
    let t = Transport::new_with_output_phase(
        phase,
        p(3, 500),
        Timestamp::ZERO,
        Duration::ZERO,
        Rate::NORMAL,
        None,
    )
    .unwrap();
    for wrong in [
        OutputPhaseBasis::new(ClockDomainId(3), 92, 113, 0, 3).unwrap(),
        OutputPhaseBasis::new(ClockDomainId(3), 91, 114, 0, 3).unwrap(),
        OutputPhaseBasis::new(ClockDomainId(3), 91, 113, 0, 4).unwrap(),
    ] {
        assert!(
            prepare_coordinate_live_transport(&t, &prepared, &cutoff, wrong, Timestamp::ZERO)
                .is_err()
        );
    }
    assert!(prepare_coordinate_live_transport(
        &t,
        &prepared,
        &cutoff,
        phase,
        Timestamp::from_nanos(333_333_334)
    )
    .is_err());
    let resume_cutoff = token(&a, prepared.exact_cut(), true, h(104), &m);
    assert!(prepare_coordinate_live_transport(
        &t,
        &prepared,
        &resume_cutoff,
        phase,
        Timestamp::ZERO
    )
    .is_err());
    assert!(!t.is_paused());
    assert_eq!(t.output_phase_basis(), Some(phase));
}

#[test]
fn actual_start_gate_is_not_manual_pause_and_eventual_pause_uses_original_physical_gap() {
    let (mut producer, mut mixer) = mixer(3, Some(2), None);
    let mut pause = NativePause::new(p(2, 0), ClockDomainId(1), 3)
        .unwrap()
        .with_start_frame(2)
        .unwrap()
        .with_coordinate_binding(7, descriptor(), identity(3))
        .unwrap();
    assert!(request(&mut pause, true, 0, 0));
    let held = mixer.render(&mut [0.0; 1]).unwrap();
    assert!(held.paused);
    assert_eq!(held.playback_frames, 0);
    assert!(pause
        .prepare_coordinate_adoption(Some(held), None)
        .unwrap()
        .is_none());
    let active = mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!(active.playback_frames, 1);
    assert!(pause
        .prepare_coordinate_adoption(Some(active), None)
        .unwrap()
        .is_none());
    producer.request_pause(true);
    let report = mixer.render(&mut [0.0; 2]).unwrap();
    let adoption = pause
        .prepare_coordinate_adoption(Some(report), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        (adoption.physical_frame(), adoption.playback_frame()),
        (3, 1)
    );
    assert_eq!(
        adoption
            .exact_cut()
            .compare_target_time(TargetTime::from_frames(3, 3).unwrap()),
        Ordering::Equal
    );
}

#[test]
fn genuine_finite_marker_is_retained_and_reached_endpoint_cannot_resume() {
    let (mut producer, mut mixer) = mixer(3, None, Some(2));
    let mut pause = NativePause::new(p(2, 0), ClockDomainId(1), 3)
        .unwrap()
        .with_playback_end_frame(2)
        .unwrap()
        .with_coordinate_binding(7, descriptor(), identity(3))
        .unwrap();
    mixer.render(&mut [0.0; 1]).unwrap();
    assert!(request(&mut pause, true, 0, 0));
    // No requested mixer pause: the actual exclusive endpoint stops this block.
    let report = mixer.render(&mut [0.0; 3]).unwrap();
    assert_eq!(report.playback_end_physical_frame, Some(2));
    let adoption = pause
        .prepare_coordinate_adoption(Some(report), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        (adoption.physical_frame(), adoption.playback_frame()),
        (2, 2)
    );
    let mut a = authority(descriptor());
    observe(&mut a, 0, 100);
    observe(&mut a, 8_000_000_000, 200);
    a.record_acquired_prefix(h(200)).unwrap();
    let m = merger();
    let cutoff = token(&a, adoption.exact_cut(), false, h(200), &m);
    pause
        .commit_coordinate_adoption(adoption, &a, &cutoff, h(200), &m)
        .unwrap();
    let before = pause.clone();
    assert!(!pause.request_coordinate(false, &a).unwrap());
    assert_eq!(pause, before);
    producer.request_pause(false);
    let ended = mixer.render(&mut [0.0; 1]).unwrap();
    assert!(ended.paused);
    assert_eq!(ended.playback_frames, 0);
    assert!(pause
        .prepare_coordinate_adoption(Some(ended), None)
        .unwrap()
        .is_none());
}

fn target_staged() -> (
    NativePause,
    CommandProducer,
    beatkernel_platform::audio::ConvertedNativeOutputState,
) {
    let (mut producer, mut output) =
        crate::native_converted_test_support::rig(44_100, 48_000, None, None, 0);
    let basis = output.target_frame_basis();
    let mut pause = NativePause::new(p(2, 0), ClockDomainId(1), 44_100)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap()
        .with_coordinate_binding(
            7,
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
            identity(44_100),
        )
        .unwrap();
    output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    assert!(request(&mut pause, true, 1, 101));
    producer.request_pause(true);
    output.render_pending(8).unwrap();
    (pause, producer, output)
}

#[test]
fn target_staging_requires_actual_source_adoption_and_matching_original_projection() {
    let (pause, _, output) = target_staged();
    let facts = output.boundaries();
    let report = output.last_real_source_report();
    let before = pause.clone();
    assert!(facts.pause.is_some());
    assert!(pause
        .prepare_coordinate_adoption(report, None)
        .unwrap()
        .is_none());
    assert!(pause
        .prepare_coordinate_adoption(None, Some(facts))
        .unwrap()
        .is_none());
    let mut absent = facts;
    absent.pause = None;
    assert!(pause
        .prepare_coordinate_adoption(report, Some(absent))
        .unwrap()
        .is_none());
    for mutation in 0..3 {
        let mut wrong = facts;
        match mutation {
            0 => wrong.pause.as_mut().unwrap().source_frame += 1,
            1 => wrong.origin = Some(p(2, 1)),
            _ => wrong.source_rate = 24_000,
        }
        assert!(pause
            .prepare_coordinate_adoption(report, Some(wrong))
            .is_err());
        assert_eq!(pause, before);
    }
    let adoption = pause
        .prepare_coordinate_adoption(report, Some(facts))
        .unwrap()
        .unwrap();
    assert_eq!(adoption.physical_frame(), facts.pause.unwrap().source_frame);
    assert_eq!(adoption.target_projection(), facts.pause);
    assert_eq!(
        adoption
            .exact_cut()
            .compare_target_time(facts.pause.unwrap().target_time),
        Ordering::Equal
    );
    assert_eq!(pause, before);
}

#[test]
fn actual_target_resume_overshoot_is_subsequent_mapping_and_never_control_elapsed() {
    let (mut pause, mut producer, mut output) = target_staged();
    let basis = pause.target_basis().unwrap();
    let mut a = authority(AudioAuthorityCoordinateBasis::TargetFrame(basis));
    observe(&mut a, 0, 100);
    observe(&mut a, 1, 101);
    observe(&mut a, 9, 109);
    a.record_acquired_prefix(h(109)).unwrap();
    let m = merger();
    let adoption = pause
        .prepare_coordinate_adoption(output.last_real_source_report(), Some(output.boundaries()))
        .unwrap()
        .unwrap();
    let frozen = adoption.playback_frame();
    assert!(frozen > 1);
    let phase = OutputPhaseBasis::new(ClockDomainId(3), 91, 113, 1, 44_100).unwrap();
    let t = Transport::new_with_output_phase(
        phase,
        p(3, 500),
        Timestamp::from_nanos(100_001),
        Duration::from_nanos(22_676),
        Rate::new(3, 2).unwrap(),
        None,
    )
    .unwrap();
    let cutoff = token(&a, adoption.exact_cut(), false, h(109), &m);
    let elapsed = i128::from(frozen - 1) * 1_000_000_000 / 44_100;
    let point = phase
        .point_from_elapsed_nanos(cutoff.output(), elapsed)
        .unwrap();
    let song = t.position_at_output_phase(point).unwrap();
    let paused = prepare_coordinate_live_transport(&t, &adoption, &cutoff, phase, song).unwrap();
    pause
        .commit_coordinate_adoption(adoption, &a, &cutoff, h(109), &m)
        .unwrap();
    a.commit_coordinate_control_cutoff(cutoff, h(109), &m)
        .unwrap();
    output.admit(8).unwrap();
    assert!(pause.request_coordinate(false, &a).unwrap());
    producer.request_pause(false);
    let held = output.render_held_pending(4).unwrap();
    assert!(held.source.is_none());
    output.admit(4).unwrap();
    assert!(pause
        .prepare_coordinate_adoption(output.last_real_source_report(), Some(output.boundaries()))
        .unwrap()
        .is_none());
    let mut adoption = None;
    for _ in 0..32 {
        output.render_pending(1).unwrap();
        output.admit(1).unwrap();
        if output.boundaries().resume.is_some() {
            adoption = pause
                .prepare_coordinate_adoption(
                    output.last_real_source_report(),
                    Some(output.boundaries()),
                )
                .unwrap();
            if adoption.is_some() {
                break;
            }
        }
    }
    let adoption = adoption.expect("real nonempty resume source and projected target crossing");
    observe(&mut a, 128, 228);
    a.record_acquired_prefix(h(228)).unwrap();
    let mapped = output.boundaries().resume.unwrap();
    assert_eq!(adoption.playback_frame(), frozen);
    // This rate ratio projects to a target sample strictly after the original source boundary.
    assert_eq!(
        query(mapped.target_time)
            .compare_target_time(TargetTime::from_frames(mapped.source_frame, 44_100).unwrap()),
        Ordering::Greater
    );
    let cutoff = token(&a, adoption.exact_cut(), true, h(228), &m);
    let resumed =
        prepare_coordinate_live_transport(&paused, &adoption, &cutoff, phase, song).unwrap();
    let exact = phase
        .point_from_elapsed_nanos(cutoff.output(), elapsed)
        .unwrap();
    assert_eq!(resumed.position_at_output_phase(exact).unwrap(), song);
    assert_eq!(resumed.preroll_setup(), t.preroll_setup());
    let fractional = output.converter_owner().source_position();
    assert!(fractional.numerator > 0);
    // Remove the actual source silent gap from the converter's real consumed
    // position, retaining its fractional phase for subsequent playback only.
    let silent_gap = mapped.source_frame.checked_sub(frozen).unwrap();
    let active = SourcePosition {
        frame: fractional.frame.checked_sub(silent_gap).unwrap(),
        numerator: fractional.numerator,
        denominator: fractional.denominator,
    };
    let future = phase
        .point(p(3, cutoff.output().timestamp.as_nanos() + 1), active)
        .unwrap();
    assert!(future.elapsed_nanos() > elapsed);
    assert!(resumed.position_at_output_phase(future).unwrap() > song);
    pause
        .commit_coordinate_adoption(adoption, &a, &cutoff, h(228), &m)
        .unwrap();
    assert_eq!(pause.phase(), PausePhase::Running);
}

#[test]
fn actual_target_cut_distinguishes_fractional_before_equal_after_in_same_floor() {
    let (mut producer, mut output) =
        crate::native_converted_test_support::rig(44_100, 48_001, None, None, 0);
    let basis = output.target_frame_basis();
    let mut pause = NativePause::new(p(2, 0), ClockDomainId(1), 44_100)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap()
        .with_coordinate_binding(
            7,
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
            identity(44_100),
        )
        .unwrap();
    output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    assert!(request(&mut pause, true, 1, 1));
    producer.request_pause(true);
    output.render_pending(8).unwrap();
    let actual = output.boundaries().pause.unwrap();
    assert_eq!(
        actual.target_time,
        TargetTime::from_frames(3, 48_001).unwrap()
    );
    let prepared = pause
        .prepare_coordinate_adoption(output.last_real_source_report(), Some(output.boundaries()))
        .unwrap()
        .unwrap();
    let mut a = authority(AudioAuthorityCoordinateBasis::TargetFrame(basis));
    observe(&mut a, 2, 100);
    observe(&mut a, 4, 1_000_000_100);
    let now = h(1_000_000_100);
    a.record_acquired_prefix(now).unwrap();
    let m = merger();
    let cutoff = token(&a, prepared.exact_cut(), false, now, &m);
    assert_eq!(cutoff.host(), h(500_000_100));
    for (at, ordering) in [
        (500_000_099, Ordering::Less),
        (500_000_100, Ordering::Equal),
        (500_000_101, Ordering::Greater),
    ] {
        let input = a.prepare_coordinate_input(h(at), now).unwrap().unwrap();
        assert_eq!(input.raw_output(), cutoff.raw_output());
        assert_eq!(
            input
                .query()
                .compare_interpolated_time(prepared.exact_cut()),
            ordering
        );
    }
    assert_eq!(pause.phase(), PausePhase::Pausing);
    pause
        .commit_coordinate_adoption(prepared, &a, &cutoff, now, &m)
        .unwrap();
    assert_eq!(pause.phase(), PausePhase::Paused);
}

#[test]
fn wrong_authority_descriptor_epoch_host_domain_and_control_kind_refuse_ack() {
    let (mut pause, prepared, _, _) = staged();
    let m = merger();
    let base = OutputFrameBasis::new(p(2, 0), 4, 0).unwrap();
    let mut different = authority(AudioAuthorityCoordinateBasis::SourceCounter {
        basis: base,
        frequency: 12_000_000_000,
    });
    observe(&mut different, 3_999_999_998, 100);
    observe(&mut different, 4_000_000_002, 104);
    different.record_acquired_prefix(h(104)).unwrap();
    let cutoff = token(&different, prepared.exact_cut(), false, h(104), &m);
    let before = pause.clone();
    assert!(pause
        .commit_coordinate_adoption(prepared.clone(), &different, &cutoff, h(104), &m)
        .is_err());
    assert_eq!(pause, before);
    let a = crossed();
    let resume = token(&a, prepared.exact_cut(), true, h(104), &m);
    assert!(pause
        .commit_coordinate_adoption(prepared.clone(), &a, &resume, h(104), &m)
        .is_err());
    let cutoff = token(&a, prepared.exact_cut(), false, h(104), &m);
    assert!(pause
        .commit_coordinate_adoption(prepared.clone(), &a, &cutoff, p(99, 104), &m)
        .is_err());
    assert_eq!(pause, before);
    let mut epoch = a.epoch();
    epoch.id = 8;
    let mut other =
        AudioAuthority::new_with_coordinates(AudioAuthorityConfig::default(), epoch, descriptor())
            .unwrap();
    let basis = descriptor();
    for (position, host) in [(3_999_999_998, 100), (4_000_000_002, 104)] {
        let c = NativeOutputCoordinate::Counter {
            position,
            frequency: 12_000_000_000,
        };
        other
            .observe_coordinate(
                8,
                ClockPair {
                    source: coordinate_point(basis, c),
                    target: h(host),
                },
                c,
            )
            .unwrap();
    }
    other.record_acquired_prefix(h(104)).unwrap();
    assert!(pause
        .commit_coordinate_adoption(prepared, &other, &cutoff, h(104), &m)
        .is_err());
    assert_eq!(pause, before);
}

#[test]
fn inconsistent_actual_report_extent_gap_and_playback_are_refused_without_staging_mutation() {
    let (pause, _, _, mut mixer) = staged();
    let report = mixer.render(&mut [0.0; 1]).unwrap();
    let before = pause.clone();
    for mutation in 0..3 {
        let mut wrong = report;
        match mutation {
            0 => wrong.playback_frames = wrong.frames + 1,
            1 => wrong.playback_start_frame = wrong.start_frame + 1,
            _ => wrong.playback_end_physical_frame = Some(wrong.start_frame),
        }
        assert!(pause
            .prepare_coordinate_adoption(Some(wrong), None)
            .is_err());
        assert_eq!(pause, before);
    }
    let empty = mixer.render(&mut []).unwrap();
    assert_eq!(empty.frames, 0);
    assert!(pause
        .prepare_coordinate_adoption(Some(empty), None)
        .unwrap()
        .is_none());
    assert_eq!(pause, before);
}

#[test]
fn qualified_native_cut_cannot_precede_real_request_host_or_raw_output() {
    let (_, _, _, mut mixer) = staged();
    let actual = mixer.render(&mut [0.0; 1]).unwrap();
    let a = crossed();
    let m = merger();
    for (units, host) in [(0, 103), (4_000_000_008, 0)] {
        let mut pause = NativePause::new(p(2, 0), ClockDomainId(1), 3)
            .unwrap()
            .with_coordinate_binding(7, descriptor(), identity(3))
            .unwrap();
        assert!(request(&mut pause, true, units, host));
        let prepared = pause
            .prepare_coordinate_adoption(Some(actual), None)
            .unwrap()
            .unwrap();
        let cutoff = token(&a, prepared.exact_cut(), false, h(104), &m);
        let before = pause.clone();
        assert!(pause
            .commit_coordinate_adoption(prepared, &a, &cutoff, h(104), &m)
            .is_err());
        assert_eq!(pause, before);
    }
}

#[test]
fn exact_original_request_counter_refuses_preceding_adoption_despite_equal_floor_and_host() {
    let basis = AudioAuthorityCoordinateBasis::SourceCounter {
        basis: OutputFrameBasis::new(p(2, 0), 1_000_000_000, 0).unwrap(),
        frequency: 4_000_000_000,
    };
    let mut a = authority(basis);
    observe(&mut a, 3, 4);
    observe(&mut a, 5, 5);
    a.record_acquired_prefix(h(5)).unwrap();
    let mut pause = NativePause::new(p(2, 0), ClockDomainId(1), 1_000_000_000)
        .unwrap()
        .with_coordinate_binding(7, basis, identity(1_000_000_000))
        .unwrap();
    assert!(pause.request_coordinate(true, &a).unwrap());
    let (mut producer, mut mixer) = mixer(1_000_000_000, None, None);
    mixer.render(&mut [0.0; 1]).unwrap();
    producer.request_pause(true);
    let adoption = pause
        .prepare_coordinate_adoption(Some(mixer.render(&mut [0.0; 1]).unwrap()), None)
        .unwrap()
        .unwrap();
    let m = merger();
    let cutoff = token(&a, adoption.exact_cut(), false, h(5), &m);
    let original = a.latest_coordinate_observation().unwrap();
    assert_eq!(cutoff.raw_output(), original.pair().source);
    assert_eq!(cutoff.host(), original.pair().target);
    assert_eq!(cutoff.raw_output(), p(2, 1));
    assert_eq!(cutoff.host(), h(5));
    let before = pause.clone();
    assert!(pause
        .commit_coordinate_adoption(adoption, &a, &cutoff, h(5), &m)
        .is_err());
    assert_eq!(pause, before);
}

#[test]
fn nonzero_source_creation_frame_accepts_matching_authority_creation_origin() {
    let (mut producer, mut mixer) = mixer(3, None, None);
    mixer.render(&mut [0.0; 3]).unwrap();
    let basis =
        AudioAuthorityCoordinateBasis::SourceFrame(OutputFrameBasis::new(p(2, 0), 3, 2).unwrap());
    let mut pause = NativePause::new(p(2, 0), ClockDomainId(1), 3)
        .unwrap()
        .with_coordinate_binding(7, basis, SourcePlaybackIdentity::new(3, 2, 2).unwrap())
        .unwrap();
    let mut a = authority(basis);
    observe(&mut a, 0, 100);
    assert_ne!(a.epoch().stream_origin, p(2, 0));
    assert!(pause.request_coordinate(true, &a).unwrap());
    producer.request_pause(true);
    let report = mixer.render(&mut [0.0; 1]).unwrap();
    let prepared = pause
        .prepare_coordinate_adoption(Some(report), None)
        .unwrap()
        .unwrap();
    observe(&mut a, 1, 101);
    a.record_acquired_prefix(h(101)).unwrap();
    let m = merger();
    let cutoff = token(&a, prepared.exact_cut(), false, h(101), &m);
    let boundary = pause
        .commit_coordinate_adoption(prepared, &a, &cutoff, h(101), &m)
        .unwrap();
    assert_eq!(boundary.raw_output, p(2, 1_000_000_000));
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (3, 3));
}

#[test]
fn actual_nonzero_target_creation_time_accepts_matching_authority_creation_origin() {
    let (mut producer, mut output) =
        crate::native_converted_test_support::rig(44_100, 48_000, None, None, 0);
    output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    let basis = output.target_frame_basis();
    assert_ne!(basis.start_time(), TargetTime::from_frames(0, 1).unwrap());
    let source = SourcePlaybackIdentity::new(
        44_100,
        output.mixer().frame_cursor(),
        output.mixer().playback_frame_cursor(),
    )
    .unwrap();
    let mut pause = NativePause::new(p(2, 0), ClockDomainId(1), 44_100)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap()
        .with_coordinate_binding(7, AudioAuthorityCoordinateBasis::TargetFrame(basis), source)
        .unwrap();
    let mut a = authority(AudioAuthorityCoordinateBasis::TargetFrame(basis));
    observe(&mut a, 0, 100);
    assert_ne!(a.epoch().stream_origin, p(2, 0));
    assert!(pause.request_coordinate(true, &a).unwrap());
    producer.request_pause(true);
    output.render_pending(8).unwrap();
    let facts = output.boundaries();
    let prepared = pause
        .prepare_coordinate_adoption(output.last_real_source_report(), Some(facts))
        .unwrap()
        .unwrap();
    observe(&mut a, 8, 108);
    a.record_acquired_prefix(h(108)).unwrap();
    let m = merger();
    let cutoff = token(&a, prepared.exact_cut(), false, h(108), &m);
    let boundary = pause
        .commit_coordinate_adoption(prepared, &a, &cutoff, h(108), &m)
        .unwrap();
    assert_eq!(boundary.target_projection, facts.pause);
    assert_eq!(boundary.playback_frame, source.initial_playback());
}
