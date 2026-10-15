//! Real cold native composition and actual retained PCM; no device proof is claimed.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::Rate,
};
use beatkernel_bms_runtime::{
    live_speed::LiveSpeedSetup,
    native_audio::{
        prepare_retained_audio_with_runtime_rates, prepare_retained_audio_with_speed,
        NativeAudioConfig, NativePracticeAudioConfig,
    },
    native_audio_startup::{
        initial_source_coordinate_runtime_transport, initial_source_coordinate_transport,
        next_runtime_program_id, prepare_source_coordinate_history,
    },
    replay_speed::ReplaySpeed,
};

const PROGRAM: u64 = 0x14102;
const LOGICAL: ClockDomainId = ClockDomainId(3);

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn speed(value: &str) -> ReplaySpeed {
    value.parse().unwrap()
}
fn bank() -> SampleBank {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(32, 64, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            format,
            vec![0., 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875],
            limits,
        )
        .unwrap(),
    )
    .unwrap();
    bank
}
fn config(start: i64, preroll: i64) -> NativeAudioConfig {
    NativeAudioConfig {
        output_origin: ClockPoint {
            domain: ClockDomainId(72),
            timestamp: ts(-123),
        },
        start: ts(start),
        preroll: Duration::from_nanos(preroll),
        lookahead: Duration::from_nanos(100_000_000),
        voices: 2,
        max_render_frames: 64,
        playback_end_frame: None,
        gated_start: false,
    }
}
fn practice(start: i64, end: i64) -> NativePracticeAudioConfig {
    NativePracticeAudioConfig {
        region: PracticeRegion::new(ts(start), ts(end), false).unwrap(),
        limits: PracticeLimits::new(8, 2, 64, 4, 16).unwrap(),
    }
}
fn play(at: i64) -> AudioCommand {
    AudioCommand::Play {
        at: ts(at),
        voice: VoiceId(1),
        sample: SampleId(1),
        gain: 1.0,
    }
}
fn started(controller: &mut PracticeController) -> PracticeReceipt {
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Boundary(boundary) => {
            assert_eq!(boundary.kind, PracticeBoundaryKind::Started);
            boundary
        }
        other => panic!("expected actual Started boundary, got {other:?}"),
    }
}

#[test]
fn actual_runtime_preparation_preserves_fractional_head_and_transport_phase_together() {
    let setup =
        LiveSpeedSetup::new(ts(0), Duration::ZERO, speed("1/2"), Some(ts(20_000_000))).unwrap();
    let mut prepared = prepare_retained_audio_with_runtime_rates(
        bank(),
        vec![play(0)],
        config(0, 0),
        practice(0, 20_000_000),
        setup.speed(),
        setup.end(),
        PROGRAM,
    )
    .unwrap();
    let identity = prepare_source_coordinate_history(&mut prepared.audio.mixer, 16).unwrap();
    let logical_origin = ClockPoint {
        domain: LOGICAL,
        timestamp: ts(500),
    };
    let mut transport =
        initial_source_coordinate_runtime_transport(91, identity, logical_origin, setup).unwrap();
    assert_eq!(transport.runtime_output_rate(), Some(speed("1/2").rate()));
    assert_eq!(prepared.audio.bgm.report().remaining, 0);
    let mut prefix = [99.; 3];
    prepared.audio.mixer.render(&mut prefix).unwrap();
    assert_eq!(prefix, [0., 0.0625, 0.125]);
    let first = started(&mut prepared.practice);
    let before = prepared.practice.metadata().unwrap();
    let token = prepared.practice.prepare_rate(speed("2/1").rate()).unwrap();
    prepared
        .practice
        .try_set_rate_next(17, first.generation, &token)
        .unwrap();
    assert_eq!(
        prepared.practice.metadata().unwrap(),
        before,
        "admission is not application"
    );
    let mut suffix = [99.; 4];
    let report = prepared.audio.mixer.render(&mut suffix).unwrap();
    assert_eq!(suffix, [0.1875, 0.4375, 0.6875, 0.875]);
    let PracticeEvent::Rate(event) = prepared.practice.try_pop_event().unwrap() else {
        panic!("actual retained rate event missing");
    };
    assert_eq!(event.outcome, PracticeRateOutcome::Applied);
    assert_eq!(
        (event.request_id, event.generation, event.iteration),
        (17, first.generation, first.iteration)
    );
    assert_eq!(
        (
            event.physical_frame,
            event.playback_frame,
            event.active_elapsed_nanos
        ),
        (3, 3, 3_000_000)
    );
    assert_eq!((event.request_revision, event.applied_revision), (0, 1));
    let phase = transport.output_phase_basis().unwrap();
    let cut = phase
        .point_from_elapsed_nanos(
            ClockPoint {
                domain: LOGICAL,
                timestamp: ts(3_000_500),
            },
            event.active_elapsed_nanos,
        )
        .unwrap();
    transport
        .set_rate_at_output_phase(cut, event.applied_rate)
        .unwrap();
    assert_eq!(
        transport.position_at_output_phase(cut).unwrap(),
        ts(1_500_000)
    );
    let after = phase
        .point_from_elapsed_nanos(
            ClockPoint {
                domain: LOGICAL,
                timestamp: ts(4_000_500),
            },
            4_000_000,
        )
        .unwrap();
    assert_eq!(
        transport.position_at_output_phase(after).unwrap(),
        ts(3_500_000)
    );
    let past = phase
        .point_from_elapsed_nanos(
            ClockPoint {
                domain: LOGICAL,
                timestamp: ts(1_000_500),
            },
            1_000_000,
        )
        .unwrap();
    assert_eq!(
        transport.position_at_output_phase(past).unwrap(),
        ts(500_000)
    );
    assert_eq!(report.counters.commands_consumed, 0);
    assert_eq!(report.counters.commands_applied, 0);
}

#[test]
fn runtime_cold_odd_preroll_still_uses_original_pcm_and_original_started_receipt() {
    let mut prepared = prepare_retained_audio_with_runtime_rates(
        bank(),
        vec![play(10_000_000)],
        config(10_000_000, 1_500_001),
        practice(10_000_000, 20_000_000),
        speed("6/4"),
        Some(ts(20_000_000)),
        PROGRAM,
    )
    .unwrap();
    let mut pcm = [99.; 5];
    let report = prepared.audio.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0., 0., 0., 0.1875, 0.375]);
    let boundary = started(&mut prepared.practice);
    assert_eq!(boundary.requested_song_time, ts(10_000_000));
    assert_eq!(boundary.applied_song_time, ts(7_749_999));
    assert_eq!(boundary.correction_nanos, -2_250_001);
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (0, 0));
    assert_eq!(prepared.practice.metadata().unwrap().revision, 0);
    assert_eq!(report.counters.commands_applied, 0);
}

#[test]
fn gated_runtime_program_has_no_applied_event_until_actual_active_output() {
    let mut cfg = config(0, 0);
    cfg.gated_start = true;
    let mut prepared = prepare_retained_audio_with_runtime_rates(
        bank(),
        vec![play(0)],
        cfg,
        practice(0, 20_000_000),
        speed("1/2"),
        Some(ts(20_000_000)),
        PROGRAM,
    )
    .unwrap();
    let mut held = [99.; 3];
    prepared.audio.mixer.render(&mut held).unwrap();
    assert_eq!(held, [0.; 3]);
    assert_eq!(prepared.practice.try_pop_event(), Err(PracticeError::Empty));
    assert!(!prepared.practice.metadata().unwrap().started);
    assert_eq!(prepared.practice.metadata().unwrap().revision, 0);
    prepared.audio.producer.schedule_start_at(5).unwrap();
    let mut pcm = [99.; 5];
    prepared.audio.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0., 0., 0., 0.0625, 0.125]);
    let boundary = started(&mut prepared.practice);
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (5, 0));
    assert_eq!(prepared.practice.try_pop_event(), Err(PracticeError::Empty));
    let token = prepared.practice.prepare_rate(speed("2/1").rate()).unwrap();
    prepared
        .practice
        .try_set_rate_next(18, boundary.generation, &token)
        .unwrap();
    let mut after_start = [99.];
    prepared.audio.mixer.render(&mut after_start).unwrap();
    assert_eq!(after_start, [0.1875]);
    let PracticeEvent::Rate(event) = prepared.practice.try_pop_event().unwrap() else {
        panic!("rate was not applied at actual gated start");
    };
    assert_eq!(event.outcome, PracticeRateOutcome::Applied);
    assert_eq!((event.physical_frame, event.playback_frame), (8, 3));
    assert_eq!(event.active_elapsed_nanos, 3_000_000);
    assert_eq!(prepared.audio.producer.applied_start_frame(), Some(5));
}

#[test]
fn finite_runtime_end_can_restart_on_same_mixer_without_immutable_end() {
    let mut prepared = prepare_retained_audio_with_runtime_rates(
        bank(),
        vec![play(0)],
        config(0, 0),
        practice(0, 4_000_000),
        speed("2/1"),
        Some(ts(4_000_000)),
        PROGRAM,
    )
    .unwrap();
    let mut pcm = [99.; 4];
    prepared.audio.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0., 0.25, 0., 0.]);
    started(&mut prepared.practice);
    let PracticeEvent::Boundary(ended) = prepared.practice.try_pop_event().unwrap() else {
        panic!("finite end event missing");
    };
    assert_eq!(ended.kind, PracticeBoundaryKind::Ended);
    prepared
        .practice
        .try_request(PracticeRequest {
            id: 19,
            expected_generation: ended.generation,
            at_playback_frame: 4,
            region: practice(0, 4_000_000).region,
        })
        .unwrap();
    let mut replayed = [99.; 4];
    prepared.audio.mixer.render(&mut replayed).unwrap();
    assert_eq!(replayed, pcm);
    let PracticeEvent::Boundary(restarted) = prepared.practice.try_pop_event().unwrap() else {
        panic!("finite restart event missing");
    };
    assert_eq!(restarted.kind, PracticeBoundaryKind::Requested);
    assert_eq!(restarted.physical_frame, 4);
    assert_eq!(restarted.generation, ended.generation + 1);
}

#[test]
fn runtime_cold_refuses_zero_identity_mismatched_extent_and_invalid_commands() {
    for invalid in 0..6 {
        let mut cfg = config(0, 0);
        let mut commands = vec![play(0)];
        let mut end = Some(ts(20_000_000));
        let mut program = PROGRAM;
        match invalid {
            0 => program = 0,
            1 => cfg.start = ts(1),
            2 => cfg.playback_end_frame = Some(20),
            3 => end = Some(ts(20_000_001)),
            4 => end = None,
            _ => commands = vec![play(-1)],
        }
        assert!(
            prepare_retained_audio_with_runtime_rates(
                bank(),
                commands,
                cfg,
                practice(0, 20_000_000),
                speed("3/2"),
                end,
                program,
            )
            .is_err(),
            "invalid cold case {invalid} was accepted"
        );
    }
}

#[test]
fn runtime_program_tokens_cannot_cross_actual_installed_programs() {
    let mut first = prepare_retained_audio_with_runtime_rates(
        bank(),
        vec![play(0)],
        config(0, 0),
        practice(0, 20_000_000),
        speed("1/1"),
        Some(ts(20_000_000)),
        PROGRAM,
    )
    .unwrap();
    let second = prepare_retained_audio_with_runtime_rates(
        bank(),
        vec![play(0)],
        config(0, 0),
        practice(0, 20_000_000),
        speed("1/1"),
        Some(ts(20_000_000)),
        PROGRAM,
    )
    .unwrap();
    let foreign = second.practice.prepare_rate(speed("2/1").rate()).unwrap();
    let before = first.practice.metadata().unwrap();
    assert!(first.practice.try_set_rate_next(20, 1, &foreign).is_err());
    assert_eq!(first.practice.metadata().unwrap(), before);
    let mut pcm = [99.; 4];
    first.audio.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0., 0.125, 0.25, 0.375]);
    started(&mut first.practice);
    assert_eq!(first.practice.try_pop_event(), Err(PracticeError::Empty));
}

#[test]
fn actual_runtime_capacity_refusal_preserves_original_rate_pcm_and_revision() {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(4, 16, 1).unwrap();
    let mut samples = SampleBank::new(format, limits).unwrap();
    samples
        .insert(
            SampleId(1),
            PcmSample::new(format, vec![0.125], limits).unwrap(),
        )
        .unwrap();
    let commands = [0, 2_000_000, 4_000_000]
        .into_iter()
        .enumerate()
        .map(|(index, at)| AudioCommand::Play {
            at: ts(at),
            voice: VoiceId(index as u64 + 1),
            sample: SampleId(1),
            gain: 1.0,
        })
        .collect();
    let mut cfg = config(0, 0);
    cfg.voices = 16;
    let mut bounds = practice(0, 20_000_000);
    bounds.limits = PracticeLimits::new(8, 1, 64, 4, 16).unwrap();
    let mut prepared = prepare_retained_audio_with_runtime_rates(
        samples,
        commands,
        cfg,
        bounds,
        speed("1/1"),
        Some(ts(20_000_000)),
        PROGRAM,
    )
    .unwrap();
    let mut first = [99.];
    prepared.audio.mixer.render(&mut first).unwrap();
    assert_eq!(first, [0.125]);
    let start = started(&mut prepared.practice);
    let token = prepared.practice.prepare_rate(speed("4/1").rate()).unwrap();
    prepared
        .practice
        .try_set_rate_next(21, start.generation, &token)
        .unwrap();
    let mut continued = [99.; 5];
    let report = prepared.audio.mixer.render(&mut continued).unwrap();
    assert_eq!(continued, [0., 0.125, 0., 0.125, 0.]);
    let PracticeEvent::Rate(event) = prepared.practice.try_pop_event().unwrap() else {
        panic!("actual capacity refusal event missing");
    };
    assert_eq!(
        event.outcome,
        PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity)
    );
    assert_eq!(event.request_id, 21);
    assert_eq!(event.generation, start.generation);
    assert_eq!((event.request_revision, event.applied_revision), (0, 0));
    assert_eq!(
        (event.previous_rate, event.applied_rate),
        (Rate::NORMAL, Rate::NORMAL)
    );
    assert_eq!(prepared.practice.metadata().unwrap().revision, 0);
    assert_eq!(prepared.audio.mixer.rate(), Rate::NORMAL);
    assert_eq!(report.counters.voice_full, 0);
    assert_eq!(report.counters.commands_applied, 0);
}

#[test]
fn native_cold_program_ids_are_nonzero_and_do_not_reuse_a_prior_allocation() {
    let first = next_runtime_program_id().unwrap();
    let second = next_runtime_program_id().unwrap();
    assert_ne!(first, 0);
    assert!(
        second > first,
        "runtime program custody identity was reused"
    );
}

#[test]
fn fixed_cold_program_and_transport_remain_fixed_only_with_identical_initial_pcm() {
    let mut fixed = prepare_retained_audio_with_speed(
        bank(),
        vec![play(0)],
        config(0, 0),
        practice(0, 20_000_000),
        speed("3/2"),
        Some(ts(20_000_000)),
    )
    .unwrap();
    assert_eq!(
        fixed.practice.prepare_rate(Rate::NORMAL),
        Err(PracticeError::RuntimeRateRequired)
    );
    let identity = prepare_source_coordinate_history(&mut fixed.audio.mixer, 8).unwrap();
    let setup =
        LiveSpeedSetup::new(ts(0), Duration::ZERO, speed("3/2"), Some(ts(20_000_000))).unwrap();
    let origin = ClockPoint {
        domain: LOGICAL,
        timestamp: ts(500),
    };
    let old = initial_source_coordinate_transport(91, identity, origin, setup).unwrap();
    let runtime = initial_source_coordinate_runtime_transport(91, identity, origin, setup).unwrap();
    assert_eq!(old.runtime_output_rate(), None);
    assert_eq!(runtime.runtime_output_rate(), Some(speed("3/2").rate()));
    assert_eq!(old.output_phase_basis(), runtime.output_phase_basis());
    let phase = runtime.output_phase_basis().unwrap();
    for (elapsed, expected) in [(0, 0), (1_000_000, 1_500_000), (20_000_000, 20_000_000)] {
        let point = phase
            .point_from_elapsed_nanos(
                ClockPoint {
                    domain: LOGICAL,
                    timestamp: ts(500 + elapsed as i64),
                },
                elapsed,
            )
            .unwrap();
        assert_eq!(
            old.position_at_output_phase(point),
            runtime.position_at_output_phase(point)
        );
        assert_eq!(
            runtime.position_at_output_phase(point).unwrap(),
            ts(expected)
        );
    }
    let mut pcm = [99.; 4];
    fixed.audio.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0., 0.1875, 0.375, 0.5625]);
}
