//! Actual Mixer/converter pause adoption and retained coordinate epoch contracts.
//! Controlled original native snapshots prove software policy, not device timing.
use super::*;
use crate::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
        PreparedCoordinateControlCutoff,
    },
    gameplay::output::ports::{SourcePlaybackIdentity, TargetOutputTelemetry},
    local_input::InputMerger,
    native_audio_presentation::TargetNativeAudioSnapshot,
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
};
use beatkernel::{
    audio::{
        ChannelMatrix, CommandProducer, ConvertedOutputState, ConvertedRenderReport,
        OutputFrameBasis, TargetFrameBasis, TargetTime,
    },
    input::EventMeta,
    time::{Duration, ExtrapolationPolicy},
};
use beatkernel_platform::audio::{
    ConvertedNativeOutputState, DeviceFormat, SampleEncoding,
    presentation::validation::OriginalNativePresentationEvidence,
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
fn authority(epoch: u64, basis: TargetFrameBasis) -> AudioAuthority {
    AudioAuthority::new_with_coordinates(
        AudioAuthorityConfig {
            history_capacity: 64,
            max_observation_age: Duration::from_nanos(1_000_000_000),
            input_extrapolation: ExtrapolationPolicy::Forbid,
            max_input_ahead: Duration::ZERO,
        },
        AudioAuthorityEpoch {
            id: epoch,
            stream_origin: basis.point_at_stream_frame(0).unwrap(),
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
        AudioAuthorityCoordinateBasis::TargetFrame(basis),
    )
    .unwrap()
}
fn observe(a: &mut AudioAuthority, frame: u64, ns: i64) {
    let AudioAuthorityCoordinateBasis::TargetFrame(basis) = a.coordinate_basis().unwrap() else {
        panic!("fixture retains the actual Target descriptor")
    };
    a.observe_coordinate(
        a.epoch().id,
        ClockPair {
            source: basis.point_at_stream_frame(frame).unwrap(),
            target: host(ns),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap();
}
fn snapshot(
    epoch: u64,
    basis: TargetFrameBasis,
    frame: u64,
    ns: i64,
) -> CoordinateTargetNativeAudioSnapshot {
    CoordinateTargetNativeAudioSnapshot::new(
        TargetNativeAudioSnapshot {
            epoch,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: host(ns),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}
fn telemetry(
    state: &ConvertedNativeOutputState,
    report: ConvertedRenderReport,
) -> TargetOutputTelemetry {
    TargetOutputTelemetry {
        source: state.last_real_source_report(),
        converted: Some(report),
        facts: state.boundaries(),
    }
}
fn cutoff(
    a: &AudioAuthority,
    prepared: &PreparedCoordinatePauseAdoption,
    now: ClockPoint,
    inputs: &InputMerger,
) -> PreparedCoordinateControlCutoff {
    let at = a
        .presented_coordinate_boundary_host(&prepared.exact_cut(), now)
        .unwrap()
        .unwrap();
    if prepared.paused() {
        a.prepare_coordinate_control_cutoff(a.epoch().id, prepared.exact_cut(), at, now, inputs)
    } else {
        a.prepare_coordinate_resume_control_cutoff(
            a.epoch().id,
            prepared.exact_cut(),
            at,
            now,
            inputs,
        )
    }
    .unwrap()
    .unwrap()
}
struct Rig {
    producer: CommandProducer,
    state: ConvertedNativeOutputState,
    original_source: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    pause: NativePause,
    authority: AudioAuthority,
    inputs: InputMerger,
    old_adoption: PreparedCoordinatePauseAdoption,
    old_cutoff: PreparedCoordinateControlCutoff,
}
fn rig(source_rate: u32, gate: Option<u64>) -> Rig {
    rig_with_history(source_rate, gate, true)
}
fn rig_with_history(source_rate: u32, gate: Option<u64>, keep_source_history: bool) -> Rig {
    let (mut producer, mut state) =
        crate::native_converted_test_support::rig(source_rate, 48_000, gate, None, 0);
    let original_source = state.mixer().output_frame_basis();
    if keep_source_history {
        state.enable_source_playback_history(32).unwrap();
    }
    state.enable_phase_history(64).unwrap();
    let identity = SourcePlaybackIdentity::new(
        original_source.sample_rate(),
        original_source.start_physical_frame(),
        state.mixer().playback_frame_cursor(),
    )
    .unwrap();
    let basis = state.target_frame_basis();
    let mut pause = NativePause::new(basis.origin(), ClockDomainId(1), source_rate).unwrap();
    if let Some(gate) = gate {
        pause = pause.with_start_frame(gate).unwrap();
    }
    pause = pause
        .with_target_basis(7, basis)
        .unwrap()
        .with_coordinate_binding(
            7,
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
            identity,
        )
        .unwrap();
    let active = state.render_pending(8).unwrap();
    assert!(active.source.unwrap().playback_frames > 0);
    state.admit(8).unwrap();
    let mut a = authority(7, basis);
    observe(&mut a, 0, 100);
    observe(&mut a, 8, 200);
    assert!(pause.request_coordinate(true, &a).unwrap());
    producer.request_pause(true);
    state.render_pending(32).unwrap();
    assert!(state.last_real_source_report().unwrap().paused);
    let prepared = pause
        .prepare_coordinate_adoption(state.last_real_source_report(), Some(state.boundaries()))
        .unwrap()
        .unwrap();
    observe(&mut a, state.converter_owner().target_frame_cursor(), 1000);
    let mut inputs = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap();
    // Genuine acquired payload is admitted, returned and mapped before the pause
    // cutoff; a fabricated acquired watermark alone does not stand in for input.
    let original = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(200), 1),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    });
    inputs.admit(original.clone(), host(1000)).unwrap();
    a.record_acquired_prefix(host(1000)).unwrap();
    let mapped = a
        .prepare_coordinate_input(host(200), host(1000))
        .unwrap()
        .unwrap();
    assert_eq!(inputs.pop_ready(host(1000)).unwrap(), Some(original));
    a.commit_coordinate_input(mapped, host(1000)).unwrap();
    inputs.commit(host(200)).unwrap();
    let cut = cutoff(&a, &prepared, host(1000), &inputs);
    let acknowledged = pause
        .commit_coordinate_adoption(prepared.clone(), &a, &cut, host(1000), &inputs)
        .unwrap();
    assert!(acknowledged.paused);
    assert_eq!(acknowledged.source_identity, identity);
    a.commit_coordinate_control_cutoff(cut, host(1000), &inputs)
        .unwrap();
    state.admit(32).unwrap();
    assert_eq!(pause.phase(), PausePhase::Paused);
    assert_eq!(state.mixer().playback_frame_cursor(), pause.frozen);
    assert_ne!(state.mixer().output_frame_basis(), original_source);
    Rig {
        producer,
        state,
        original_source,
        identity,
        pause,
        authority: a,
        inputs,
        old_adoption: prepared,
        old_cutoff: cut,
    }
}
fn planned(state: &ConvertedNativeOutputState, rate: u32) -> TargetFrameBasis {
    let actual = state.target_frame_basis();
    TargetFrameBasis::new(actual.origin(), actual.start_time(), rate).unwrap()
}
fn reconfigure(state: &mut ConvertedNativeOutputState, rate: u32) {
    state
        .reconfigure(
            DeviceFormat::new(rate, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            128,
        )
        .unwrap();
}
fn invariant(
    pause: &NativePause,
) -> (
    u64,
    u64,
    u64,
    Option<TargetTime>,
    TargetTime,
    Option<TargetTime>,
) {
    let target = pause.target.unwrap();
    (
        pause.frozen,
        pause.gap,
        pause.start_frame,
        target.paused_at,
        target.total_gap,
        target.startup,
    )
}

#[test]
fn original_mixer_grid_startup_and_frozen_adoption_survive_same_and_mixed_target_rebinds() {
    for (source_rate, gate, target_rate) in [
        (24_000, None, 48_000),
        (44_100, Some(2), 44_100),
        (44_100, Some(2), 96_000),
    ] {
        let mut r = rig(source_rate, gate);
        let basis = planned(&r.state, target_rate);
        let old = r.pause.clone();
        let original_cursor = r.state.mixer().frame_cursor();
        let original_position = r.state.converter_owner().source_position();
        let original_history_end = r
            .state
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .pulled_end_frame();
        let config = r.state.mixer().config();
        let mut candidate = old.clone();
        candidate
            .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
            .unwrap();
        assert_eq!(candidate.phase(), PausePhase::Paused);
        assert_eq!(candidate.epoch(), 8);
        assert_eq!(candidate.target_basis(), Some(basis));
        assert_eq!(invariant(&candidate), invariant(&old));
        assert_eq!(
            candidate.coordinate_binding.unwrap().source_identity,
            r.identity
        );
        assert_eq!(candidate.coordinate_binding.unwrap().epoch, 8);
        assert_eq!(
            candidate.coordinate_binding.unwrap().basis,
            AudioAuthorityCoordinateBasis::TargetFrame(basis)
        );
        assert_eq!(
            candidate.min_host,
            old.last_pair.map(|pair| pair.target).or(old.min_host)
        );
        assert!(candidate.last_pair.is_none());
        assert!(candidate.reference.is_none());
        assert!(candidate.coordinate_reference.is_none());
        assert!(candidate.boundary.is_none());
        assert!(candidate.last_interval.is_none());
        assert!(candidate.interval_reference.is_none());
        assert!(candidate.last_transition_physical.is_none());
        assert!(candidate.target.unwrap().transition.is_none());
        assert_eq!(
            candidate.target.unwrap().generation_floor,
            Some(r.state.converter_owner().target_time())
        );
        assert_eq!(r.state.mixer().frame_cursor(), original_cursor);
        assert_eq!(
            r.state.converter_owner().source_position(),
            original_position
        );
        assert_eq!(r.state.mixer().config(), config);
        assert_eq!(
            r.state
                .converter_owner()
                .source_playback_history()
                .unwrap()
                .pulled_end_frame(),
            original_history_end
        );
        assert_eq!(
            r.original_source.start_physical_frame(),
            r.identity.initial_physical()
        );
        // The retained cold source pin remains the original creation grid even
        // though Mixer.output_frame_basis() now describes its advanced next frame.
        assert_eq!(r.original_source.origin(), basis.origin());
        assert_ne!(
            r.original_source.start_physical_frame(),
            r.state.mixer().output_frame_basis().start_physical_frame()
        );
        reconfigure(&mut r.state, target_rate);
        assert_eq!(r.state.target_frame_basis(), basis);
        let held = r.state.render_held_pending(4).unwrap();
        let tuple = telemetry(&r.state, held);
        let first = snapshot(8, basis, 0, 2000);
        let after = snapshot(8, basis, 1, 2001);
        assert!(
            candidate
                .coordinate_target_replacement_observation_ready(first, tuple)
                .unwrap()
        );
        assert!(
            old.validate_coordinate_target_replacement(&candidate, first, tuple)
                .is_err()
        );
        assert!(
            candidate
                .observe_coordinate_target_replacement(first, tuple)
                .unwrap()
        );
        old.validate_coordinate_target_replacement(&candidate, first, tuple)
            .unwrap();
        assert!(
            candidate
                .observe_coordinate_target_replacement(after, tuple)
                .unwrap()
        );
        old.validate_coordinate_target_replacement(&candidate, after, tuple)
            .unwrap();
        assert_eq!(invariant(&candidate), invariant(&old));
        assert_eq!(r.state.mixer().frame_cursor(), original_cursor);
        assert_eq!(r.state.mixer().playback_frame_cursor(), old.frozen);
        assert_eq!(r.state.pending_samples(), &[0.0; 4]);
    }
}

#[test]
fn pending_held_suffix_keeps_original_interpretation_and_cannot_certify_fresh_generation() {
    let mut r = rig(24_000, None);
    let old_held = r.state.render_held_pending(8).unwrap();
    r.state.admit(2).unwrap();
    let basis = r.state.target_frame_basis();
    let original_suffix = r.state.pending_samples().to_vec();
    let original_position = r.state.converter_owner().source_position();
    let cursor = r.state.mixer().frame_cursor();
    let mut candidate = r.pause.clone();
    candidate
        .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
        .unwrap();
    assert_eq!(r.state.pending_frames(), 6);
    assert_eq!(r.state.pending_samples(), original_suffix);
    assert_eq!(r.state.pending_report(), Some(old_held));
    assert_eq!(r.state.admitted_frames(), 2);
    assert!(
        !candidate
            .coordinate_target_replacement_observation_ready(
                snapshot(8, basis, 6, 2000),
                telemetry(&r.state, old_held)
            )
            .unwrap()
    );
    r.state.admit(6).unwrap();
    let fresh = r.state.render_held_pending(4).unwrap();
    let tuple = telemetry(&r.state, fresh);
    assert_eq!(fresh.target_start_time, old_held.target_end_time);
    assert!(
        !candidate
            .coordinate_target_replacement_observation_ready(snapshot(8, basis, 5, 2000), tuple)
            .unwrap()
    );
    assert!(
        candidate
            .coordinate_target_replacement_observation_ready(snapshot(8, basis, 6, 2000), tuple)
            .unwrap()
    );
    assert!(
        candidate
            .coordinate_target_replacement_observation_ready(snapshot(8, basis, 7, 2001), tuple)
            .unwrap()
    );
    assert_eq!(
        r.state.converter_owner().source_position(),
        original_position
    );
    assert_eq!(r.state.mixer().frame_cursor(), cursor);
    assert_eq!(r.state.mixer().playback_frame_cursor(), r.pause.frozen);
}

#[test]
fn incompatible_pending_rate_and_active_tail_refuse_without_changing_pause_or_suffix() {
    let mut r = rig(24_000, None);
    let held = r.state.render_held_pending(8).unwrap();
    r.state.admit(3).unwrap();
    let suffix = r.state.pending_samples().to_vec();
    let basis = planned(&r.state, 44_100);
    let mut candidate = r.pause.clone();
    let before = candidate.clone();
    assert!(
        candidate
            .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
            .is_err()
    );
    assert_eq!(candidate, before);
    assert_eq!(r.state.pending_report(), Some(held));
    assert_eq!(r.state.pending_samples(), suffix);
    assert!(
        r.state
            .validate_reconfigure(
                DeviceFormat::new(44_100, 1, SampleEncoding::Float32, None).unwrap(),
                &ChannelMatrix::default_mix(1, 1).unwrap(),
                128
            )
            .is_err()
    );
    r.state.admit(5).unwrap();
    let active = r.state.render_pending(8).unwrap();
    assert!(active.source.unwrap().paused);
    assert_eq!(active.state, ConvertedOutputState::Active);
    let basis = r.state.target_frame_basis();
    assert!(
        candidate
            .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
            .is_err()
    );
    assert_eq!(candidate, before);
    assert_eq!(r.state.pending_report(), Some(active));
}

#[test]
fn actual_paused_source_pull_progresses_without_changing_frozen_playback_or_history_identity() {
    let mut r = rig(24_000, None);
    let old_cursor = r.state.mixer().frame_cursor();
    let frozen = r.state.mixer().playback_frame_cursor();
    let paused = r.state.render_pending(16).unwrap();
    assert!(paused.source.unwrap().paused);
    assert_eq!(paused.source.unwrap().playback_frames, 0);
    assert!(r.state.mixer().frame_cursor() > old_cursor);
    assert_eq!(r.state.mixer().playback_frame_cursor(), frozen);
    r.state.admit(16).unwrap();
    let basis = r.state.target_frame_basis();
    let history = r.state.converter_owner().source_playback_history().unwrap();
    assert_eq!(
        history.initial_physical_frame(),
        r.identity.initial_physical()
    );
    assert_eq!(
        history.initial_playback_frame(),
        r.identity.initial_playback()
    );
    assert_eq!(history.source_rate(), r.identity.hz());
    assert_eq!(history.pulled_end_frame(), r.state.mixer().frame_cursor());
    let mut candidate = r.pause.clone();
    candidate
        .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
        .unwrap();
    let held = r.state.render_held_pending(4).unwrap();
    let native = snapshot(8, basis, 1, 2000);
    assert!(
        candidate
            .observe_coordinate_target_replacement(native, telemetry(&r.state, held))
            .unwrap()
    );
    r.pause
        .validate_coordinate_target_replacement(&candidate, native, telemetry(&r.state, held))
        .unwrap();
    assert_eq!(invariant(&candidate), invariant(&r.pause));
}

#[test]
fn old_coordinate_epoch_adoption_tokens_and_host_brackets_do_not_authorize_new_owner() {
    let mut r = rig(44_100, Some(2));
    let basis = planned(&r.state, 48_000);
    let mut candidate = r.pause.clone();
    candidate
        .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
        .unwrap();
    let before = candidate.clone();
    assert!(
        candidate
            .commit_coordinate_adoption(
                r.old_adoption.clone(),
                &r.authority,
                &r.old_cutoff,
                host(1000),
                &r.inputs
            )
            .is_err()
    );
    assert_eq!(candidate, before);
    assert!(candidate.request_coordinate(false, &r.authority).is_err());
    assert_eq!(candidate, before);
    let held = r.state.render_held_pending(4).unwrap();
    let tuple = telemetry(&r.state, held);
    for native in [
        snapshot(7, basis, 1, 2000),
        snapshot(
            8,
            basis,
            1,
            candidate.min_host.unwrap().timestamp.as_nanos() - 1,
        ),
    ] {
        assert!(
            candidate
                .observe_coordinate_target_replacement(native, tuple)
                .is_err()
        );
        assert_eq!(candidate, before);
    }
    let accepted = snapshot(8, basis, 1, 2000);
    assert!(
        candidate
            .observe_coordinate_target_replacement(accepted, tuple)
            .unwrap()
    );
    let after = candidate.clone();
    assert!(
        r.pause
            .validate_coordinate_target_replacement(&candidate, snapshot(8, basis, 2, 2001), tuple)
            .is_err()
    );
    assert_eq!(candidate, after);
    r.pause
        .validate_coordinate_target_replacement(&candidate, accepted, tuple)
        .unwrap();
}

#[test]
fn public_scalar_rebind_observation_and_publication_guards_remain_closed_to_coordinate_owner() {
    let mut r = rig(24_000, None);
    let basis = r.state.target_frame_basis();
    let mut candidate = r.pause.clone();
    let before = candidate.clone();
    assert!(
        candidate
            .rebind_target_output_with_basis(8, &r.state, basis)
            .is_err()
    );
    assert!(candidate.rebind_output(8, r.state.mixer()).is_err());
    assert_eq!(candidate, before);
    candidate
        .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
        .unwrap();
    let held = r.state.render_held_pending(4).unwrap();
    let native = snapshot(8, basis, 1, 2000);
    let pair = match native.snapshot().evidence {
        OriginalNativePresentationEvidence::SuppliedPair(pair) => pair,
        _ => unreachable!(),
    };
    let before = candidate.clone();
    assert!(
        candidate
            .observe_target(
                8,
                basis,
                r.state.boundaries(),
                r.state.last_real_source_report(),
                pair
            )
            .is_err()
    );
    assert_eq!(candidate, before);
    assert!(
        r.pause
            .validate_target_replacement(&candidate, basis, held, pair)
            .is_err()
    );
    assert_eq!(candidate, before);
}

#[test]
fn malformed_or_torn_held_telemetry_is_rejected_without_accepting_a_native_coordinate() {
    let mut r = rig(44_100, Some(2));
    let basis = r.state.target_frame_basis();
    let mut candidate = r.pause.clone();
    candidate
        .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
        .unwrap();
    let held = r.state.render_held_pending(4).unwrap();
    let original = telemetry(&r.state, held);
    let native = snapshot(8, basis, 1, 2000);
    let before = candidate.clone();
    for mutation in 0..21 {
        let mut torn = original;
        match mutation {
            0 => torn.facts.origin = Some(point(2, 1)),
            1 => torn.facts.source_rate += 1,
            2 => torn.facts.pause = None,
            3 => torn.facts.pause.as_mut().unwrap().source_frame += 1,
            4 => torn.facts.pause.as_mut().unwrap().target_time = held.target_start_time,
            5 => torn.facts.resume = torn.facts.pause,
            6 => torn.facts.startup.as_mut().unwrap().source_frame += 1,
            7 => torn.facts.startup.as_mut().unwrap().target_time = held.target_start_time,
            8 => torn.converted.as_mut().unwrap().target_rate += 1,
            9 => torn.converted.as_mut().unwrap().source_rate += 1,
            10 => torn.converted.as_mut().unwrap().state = ConvertedOutputState::Active,
            11 => torn.converted.as_mut().unwrap().source = original.source,
            12 => torn.converted.as_mut().unwrap().source_position.denominator = 0,
            13 => torn.converted.as_mut().unwrap().source_start_position.frame += 1,
            14 => torn.converted.as_mut().unwrap().target_end_time = held.target_start_time,
            15 => torn.converted.as_mut().unwrap().pulled_source_frame_cursor -= 1,
            16 => torn.converted.as_mut().unwrap().pause_source_frame = None,
            17 => torn.source.as_mut().unwrap().paused = false,
            18 => torn.source.as_mut().unwrap().playback_start_frame += 1,
            19 => {
                let source = torn.source.as_mut().unwrap();
                source.start_frame = u64::MAX;
                source.frames = 1;
            }
            _ => {
                torn.converted.as_mut().unwrap().target_start_time =
                    TargetTime::new(u64::MAX, 47_999, 48_000).unwrap()
            }
        }
        assert!(
            candidate
                .coordinate_target_replacement_observation_ready(native, torn)
                .is_err(),
            "mutation {mutation}"
        );
        assert!(
            candidate
                .observe_coordinate_target_replacement(native, torn)
                .is_err(),
            "mutation {mutation}"
        );
        assert_eq!(candidate, before, "mutation {mutation}");
        assert!(
            r.pause
                .validate_coordinate_target_replacement(&candidate, native, torn)
                .is_err(),
            "mutation {mutation}"
        );
        assert_eq!(candidate, before, "mutation {mutation}");
    }
    let absent = TargetOutputTelemetry {
        converted: None,
        ..original
    };
    assert!(
        !candidate
            .coordinate_target_replacement_observation_ready(native, absent)
            .unwrap()
    );
    assert!(
        !candidate
            .observe_coordinate_target_replacement(native, absent)
            .unwrap()
    );
    assert_eq!(candidate, before);
    let absent_source = TargetOutputTelemetry {
        source: None,
        ..original
    };
    assert!(
        !candidate
            .coordinate_target_replacement_observation_ready(native, absent_source)
            .unwrap()
    );
    assert!(
        !candidate
            .observe_coordinate_target_replacement(native, absent_source)
            .unwrap()
    );
    assert_eq!(candidate, before);
    let empty = TargetOutputTelemetry {
        converted: Some(ConvertedRenderReport {
            target_frames: 0,
            ..held
        }),
        ..original
    };
    assert!(
        !candidate
            .coordinate_target_replacement_observation_ready(native, empty)
            .unwrap()
    );
    assert!(
        !candidate
            .observe_coordinate_target_replacement(native, empty)
            .unwrap()
    );
    assert_eq!(candidate, before);
    assert!(
        candidate
            .observe_coordinate_target_replacement(native, original)
            .unwrap()
    );
    r.pause
        .validate_coordinate_target_replacement(&candidate, native, original)
        .unwrap();
}

#[test]
fn coordinate_kind_basis_epoch_and_native_host_progress_stay_bound_to_one_publication() {
    let mut r = rig(24_000, None);
    let basis = r.state.target_frame_basis();
    let mut candidate = r.pause.clone();
    candidate
        .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
        .unwrap();
    let held = r.state.render_held_pending(4).unwrap();
    let tuple = telemetry(&r.state, held);
    let before = candidate.clone();
    let wrong_basis = TargetFrameBasis::new(basis.origin(), basis.start_time(), 44_100).unwrap();
    for invalid in [
        snapshot(7, basis, 1, 2000),
        snapshot(8, wrong_basis, 1, 2000),
    ] {
        assert!(
            candidate
                .observe_coordinate_target_replacement(invalid, tuple)
                .is_err()
        );
        assert_eq!(candidate, before);
    }
    // Target native coordinates are original stream frames, never a counter
    // relabelled merely because its rounded point is the same.
    assert!(
        CoordinateTargetNativeAudioSnapshot::new(
            snapshot(8, basis, 1, 2000).snapshot(),
            NativeOutputCoordinate::Counter {
                position: 1,
                frequency: 48_000
            }
        )
        .is_err()
    );
    let native = snapshot(8, basis, 2, 2000);
    assert!(
        candidate
            .observe_coordinate_target_replacement(native, tuple)
            .unwrap()
    );
    let accepted = candidate.clone();
    for invalid in [snapshot(8, basis, 1, 2001), snapshot(8, basis, 3, 1999)] {
        assert!(
            candidate
                .observe_coordinate_target_replacement(invalid, tuple)
                .is_err()
        );
        assert_eq!(candidate, accepted);
    }
    r.pause
        .validate_coordinate_target_replacement(&candidate, native, tuple)
        .unwrap();
    assert!(
        r.pause
            .validate_coordinate_target_replacement(&candidate, snapshot(8, basis, 3, 2001), tuple)
            .is_err()
    );
    assert_eq!(candidate, accepted);
}

#[test]
fn cold_rebind_rejects_unacknowledged_resume_and_changed_source_identity_atomically() {
    let mut r = rig(44_100, Some(2));
    let basis = r.state.target_frame_basis();
    let mut pending = r.old_adoption.prior.clone();
    let before = pending.clone();
    assert_eq!(pending.phase(), PausePhase::Pausing);
    assert!(
        pending
            .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
            .is_err()
    );
    assert_eq!(pending, before);
    let mut resuming = r.pause.clone();
    assert!(resuming.request_coordinate(false, &r.authority).unwrap());
    let before = resuming.clone();
    assert!(
        resuming
            .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
            .is_err()
    );
    assert_eq!(resuming, before);
    for mutation in 0..7 {
        let mut pause = r.pause.clone();
        let epoch = if mutation == 0 { 7 } else { 8 };
        match mutation {
            1 => pause.frozen += 1,
            2 => pause.rate += 1,
            3 => pause.origin = point(2, 1),
            4 => pause.start_frame += 1,
            5 => {
                pause.coordinate_binding.as_mut().unwrap().source_identity =
                    SourcePlaybackIdentity::new(44_100, 1, 0).unwrap()
            }
            6 => {
                pause.coordinate_binding.as_mut().unwrap().basis =
                    AudioAuthorityCoordinateBasis::SourceFrame(r.original_source)
            }
            _ => {}
        }
        let before = pause.clone();
        assert!(
            pause
                .rebind_coordinate_target_output_with_basis(epoch, &r.state, basis)
                .is_err(),
            "mutation {mutation}"
        );
        assert_eq!(pause, before, "mutation {mutation}");
    }
    // Actual producer resumption is a cold refusal even before the source has
    // rendered again; toggling a paused owner flag cannot preserve that hold.
    r.producer.request_pause(false);
    assert!(r.state.mixer().is_paused());
    assert!(!r.state.mixer().pause_requested());
    let mut candidate = r.pause.clone();
    let before = candidate.clone();
    assert!(
        candidate
            .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
            .is_err()
    );
    assert_eq!(candidate, before);
}

#[test]
fn changed_frozen_gap_startup_binding_and_prior_pause_cannot_publish_ready_candidate() {
    let mut r = rig(44_100, Some(2));
    let basis = r.state.target_frame_basis();
    let mut candidate = r.pause.clone();
    candidate
        .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
        .unwrap();
    let held = r.state.render_held_pending(4).unwrap();
    let tuple = telemetry(&r.state, held);
    let native = snapshot(8, basis, 1, 2000);
    assert!(
        candidate
            .observe_coordinate_target_replacement(native, tuple)
            .unwrap()
    );
    r.pause
        .validate_coordinate_target_replacement(&candidate, native, tuple)
        .unwrap();
    for mutation in 0..10 {
        let mut changed = candidate.clone();
        match mutation {
            0 => changed.frozen += 1,
            1 => changed.gap += 1,
            2 => changed.start_frame += 1,
            3 => changed.start_configured = !changed.start_configured,
            4 => changed.target.as_mut().unwrap().paused_at = Some(held.target_start_time),
            5 => {
                changed.target.as_mut().unwrap().total_gap =
                    TargetTime::from_frames(1, 44_100).unwrap()
            }
            6 => changed.target.as_mut().unwrap().startup = Some(held.target_start_time),
            7 => {
                changed.coordinate_binding.as_mut().unwrap().source_identity =
                    SourcePlaybackIdentity::new(44_100, 1, 0).unwrap()
            }
            8 => changed.phase = PausePhase::Running,
            _ => changed.epoch = 7,
        }
        let before = changed.clone();
        assert!(
            r.pause
                .validate_coordinate_target_replacement(&changed, native, tuple)
                .is_err(),
            "mutation {mutation}"
        );
        assert_eq!(changed, before, "mutation {mutation}");
    }
    let mut changed_old = r.pause.clone();
    changed_old.target.as_mut().unwrap().paused_at = Some(held.target_start_time);
    let before = changed_old.clone();
    assert!(
        changed_old
            .validate_coordinate_target_replacement(&candidate, native, tuple)
            .is_err()
    );
    assert_eq!(changed_old, before);
    assert_eq!(invariant(&candidate), invariant(&r.pause));
}

#[test]
fn flat_nanosecond_points_still_compare_original_target_frames_against_exact_floor_math_only() {
    // Only the descriptor, exact held-report arithmetic and private test floor
    // use 4 GHz. The unique actual source/PCM owner is a supported 24k→48k rig;
    // this is not evidence of high-rate AudioFormat/device/submission support.
    let mut r = rig(24_000, None);
    let actual = r.state.target_frame_basis();
    let mut candidate = r.pause.clone();
    candidate
        .rebind_coordinate_target_output_with_basis(8, &r.state, actual)
        .unwrap();
    let held = r.state.render_held_pending(1).unwrap();
    let basis = TargetFrameBasis::new(actual.origin(), actual.start_time(), 4_000_000_000).unwrap();
    let floor = basis.time_at_stream_frame(1).unwrap();
    candidate.target.as_mut().unwrap().basis = basis;
    candidate.target.as_mut().unwrap().generation_floor = Some(floor);
    candidate.coordinate_binding.as_mut().unwrap().basis =
        AudioAuthorityCoordinateBasis::TargetFrame(basis);
    let report = ConvertedRenderReport {
        target_start_time: floor,
        target_end_time: basis.time_at_stream_frame(2).unwrap(),
        target_rate: 4_000_000_000,
        target_frames: 1,
        ..held
    };
    let tuple = telemetry(&r.state, report);
    let before = snapshot(8, basis, 0, 2000);
    let equal = snapshot(8, basis, 1, 2001);
    let after = snapshot(8, basis, 2, 2002);
    assert_eq!(
        basis.point_at_stream_frame(0).unwrap(),
        basis.point_at_stream_frame(1).unwrap()
    );
    assert_eq!(
        basis.point_at_stream_frame(1).unwrap(),
        basis.point_at_stream_frame(2).unwrap()
    );
    let original = candidate.clone();
    assert!(
        !candidate
            .coordinate_target_replacement_observation_ready(before, tuple)
            .unwrap()
    );
    assert!(
        !candidate
            .observe_coordinate_target_replacement(before, tuple)
            .unwrap()
    );
    assert_eq!(candidate, original);
    assert!(
        candidate
            .coordinate_target_replacement_observation_ready(equal, tuple)
            .unwrap()
    );
    assert!(
        candidate
            .observe_coordinate_target_replacement(equal, tuple)
            .unwrap()
    );
    assert!(
        candidate
            .observe_coordinate_target_replacement(after, tuple)
            .unwrap()
    );
    assert_eq!(candidate.target.unwrap().generation_floor, Some(floor));
    assert_eq!(candidate.frozen, r.pause.frozen);
    let accepted = candidate.clone();
    assert!(
        candidate
            .observe_coordinate_target_replacement(equal, tuple)
            .is_err()
    );
    assert_eq!(candidate, accepted);
}

fn actual_transition(
    pause: &mut NativePause,
    a: &mut AudioAuthority,
    inputs: &InputMerger,
    producer: &mut CommandProducer,
    state: &mut ConvertedNativeOutputState,
    epoch_start_target_cursor: u64,
    paused: bool,
    now_ns: i64,
) -> CoordinatePauseBoundary {
    assert!(pause.request_coordinate(paused, a).unwrap());
    producer.request_pause(paused);
    let mut found = None;
    let mut audible_resume = false;
    for _ in 0..8 {
        state.render_pending(16).unwrap();
        audible_resume |= state.pending_samples().iter().any(|sample| *sample > 0.0);
        let prepared = pause
            .prepare_coordinate_adoption(state.last_real_source_report(), Some(state.boundaries()))
            .unwrap();
        state.admit(16).unwrap();
        if let Some(prepared) = prepared {
            found = Some(prepared);
            break;
        }
    }
    let prepared = found.expect("actual converted source transition crosses a target block");
    assert_eq!(prepared.paused(), paused);
    let native_frame = state.converter_owner().target_frame_cursor() - epoch_start_target_cursor;
    observe(a, native_frame, now_ns);
    a.record_acquired_prefix(host(now_ns)).unwrap();
    let cut = cutoff(a, &prepared, host(now_ns), inputs);
    let boundary = pause
        .commit_coordinate_adoption(prepared, a, &cut, host(now_ns), inputs)
        .unwrap();
    a.commit_coordinate_control_cutoff(cut, host(now_ns), inputs)
        .unwrap();
    assert_eq!(
        pause.phase(),
        if paused {
            PausePhase::Paused
        } else {
            PausePhase::Running
        }
    );
    if !paused {
        assert!(
            audible_resume,
            "the original sample resumes through the retained converter"
        );
    }
    boundary
}

fn assert_exact_gap(
    old_gap: TargetTime,
    paused_at: TargetTime,
    resumed_at: TargetTime,
    actual: TargetTime,
) {
    // Independent rational oracle: compare cross products without invoking the
    // production pause duration/combine helper or rounding either interval.
    let numerator = |time: TargetTime| {
        u128::from(time.seconds()) * u128::from(time.denominator()) + u128::from(time.numerator())
    };
    let pause_den = u128::from(paused_at.denominator());
    let resume_den = u128::from(resumed_at.denominator());
    let gap_den = u128::from(old_gap.denominator());
    let interval_num = numerator(resumed_at) * pause_den - numerator(paused_at) * resume_den;
    let expected_num = interval_num * gap_den + numerator(old_gap) * pause_den * resume_den;
    let expected_den = pause_den * resume_den * gap_den;
    assert_eq!(
        numerator(actual) * expected_den,
        expected_num * u128::from(actual.denominator())
    );
}

#[test]
fn actual_mixed_rate_resume_and_repause_preserve_exact_nonzero_gap_across_two_replacements() {
    let mut r = rig(44_100, Some(2));
    let original_config = r.state.mixer().config();
    let original_startup = r.pause.target.unwrap().startup;
    let mut previous = r.pause.clone();
    for (index, target_rate) in [44_100, 48_000].into_iter().enumerate() {
        let epoch = 8 + index as u64;
        let basis = planned(&r.state, target_rate);
        let old = previous.clone();
        let mut candidate = old.clone();
        candidate
            .rebind_coordinate_target_output_with_basis(epoch, &r.state, basis)
            .unwrap();
        assert_eq!(invariant(&candidate), invariant(&old));
        let old_gap = old.target.unwrap().total_gap;
        let paused_at = old.target.unwrap().paused_at.unwrap();
        if index == 1 {
            assert_ne!(old_gap, TargetTime::from_frames(0, 1).unwrap());
        }
        reconfigure(&mut r.state, target_rate);
        let epoch_start = r.state.converter_owner().target_frame_cursor();
        let held = r.state.render_held_pending(4).unwrap();
        let tuple = telemetry(&r.state, held);
        let start_host = 2000 + index as i64 * 3000;
        let native = snapshot(epoch, basis, 1, start_host + 100);
        assert!(
            candidate
                .observe_coordinate_target_replacement(native, tuple)
                .unwrap()
        );
        old.validate_coordinate_target_replacement(&candidate, native, tuple)
            .unwrap();
        r.state.admit(4).unwrap();
        let mut a = authority(epoch, basis);
        observe(&mut a, 0, start_host);
        observe(&mut a, 1, start_host + 100);
        let frozen = candidate.frozen;
        let resumed = actual_transition(
            &mut candidate,
            &mut a,
            &r.inputs,
            &mut r.producer,
            &mut r.state,
            epoch_start,
            false,
            start_host + 1000,
        );
        assert_eq!(resumed.epoch, epoch);
        assert_eq!(resumed.source_identity, r.identity);
        assert_eq!(resumed.playback_frame, frozen);
        assert_eq!(
            candidate.gap,
            resumed.physical_frame - resumed.playback_frame
        );
        assert_exact_gap(
            old_gap,
            paused_at,
            resumed.target_projection.unwrap().target_time,
            candidate.target.unwrap().total_gap,
        );
        assert_eq!(candidate.target.unwrap().startup, original_startup);
        let gap_after_resume = candidate.target.unwrap().total_gap;
        let paused = actual_transition(
            &mut candidate,
            &mut a,
            &r.inputs,
            &mut r.producer,
            &mut r.state,
            epoch_start,
            true,
            start_host + 2000,
        );
        assert!(paused.paused);
        assert_eq!(paused.source_identity, r.identity);
        assert_eq!(candidate.target.unwrap().total_gap, gap_after_resume);
        assert_eq!(
            candidate.target.unwrap().paused_at,
            Some(paused.target_projection.unwrap().target_time)
        );
        assert_eq!(r.state.mixer().playback_frame_cursor(), candidate.frozen);
        assert_eq!(r.state.boundaries().resume, None);
        assert_eq!(r.state.pending_frames(), 0);
        assert_eq!(r.state.mixer().config(), original_config);
        let history = r.state.converter_owner().source_playback_history().unwrap();
        assert_eq!(history.source_rate(), r.identity.hz());
        assert_eq!(
            history.initial_physical_frame(),
            r.original_source.start_physical_frame()
        );
        assert_eq!(
            history.initial_playback_frame(),
            r.identity.initial_playback()
        );
        assert_eq!(history.pulled_end_frame(), r.state.mixer().frame_cursor());
        previous = candidate;
    }
}

#[test]
fn source_history_is_required_from_cold_enabling_and_cannot_be_reinvented_at_current_cursor() {
    let no_history = rig_with_history(24_000, None, false);
    assert!(
        no_history
            .state
            .converter_owner()
            .source_playback_history()
            .is_none()
    );
    let mut candidate = no_history.pause.clone();
    let before = candidate.clone();
    assert!(
        candidate
            .rebind_coordinate_target_output_with_basis(
                8,
                &no_history.state,
                no_history.state.target_frame_basis()
            )
            .is_err()
    );
    assert_eq!(candidate, before);
    let r = rig(24_000, None);
    let basis = r.state.target_frame_basis();
    // The original source history starts at cold setup. A descriptor pretending
    // that its identity starts at the now-advanced Mixer cursor is a different
    // source identity, despite equal immutable rate/origin and a held owner.
    let mut candidate = r.pause.clone();
    candidate
        .coordinate_binding
        .as_mut()
        .unwrap()
        .source_identity = SourcePlaybackIdentity::new(
        r.identity.hz(),
        r.state.mixer().frame_cursor(),
        r.state.mixer().playback_frame_cursor(),
    )
    .unwrap();
    let before = candidate.clone();
    assert!(
        candidate
            .rebind_coordinate_target_output_with_basis(8, &r.state, basis)
            .is_err()
    );
    assert_eq!(candidate, before);
    assert_eq!(
        r.state
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .initial_physical_frame(),
        r.original_source.start_physical_frame()
    );
}
