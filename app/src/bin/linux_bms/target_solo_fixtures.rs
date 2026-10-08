//! Actual Linux solo cold factory and original converted ALSA observations.
//! Optional null-plugin cases prove software/native ownership, not evdev or acoustics.
use super::*;
use beatkernel::{
    audio::{
        ConvertedOutputState, PcmLimits, PcmSample, SampleBank, SampleId, StoppedMixerSource,
        VoiceId,
    },
    time::{ClockPoint, Duration, Timestamp},
};
use beatkernel_bms_runtime::{
    native_audio::{prepare_audio, NativeAudioConfig, PreparedNativeAudio},
    native_audio_startup::new_target_audio_presentation,
    native_end::NativeEnd,
};
use beatkernel_platform::{
    audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding},
    linux::AlsaRequest,
};

const SOURCE_RATE: u32 = 44_100;
const TARGET_RATE: u32 = 48_000;
const SECTION_START: i64 = 604_800_000_000_000;

fn audio(end: Option<u64>) -> PreparedNativeAudio {
    let source = AudioFormat::new(SOURCE_RATE, 1).unwrap();
    let ramp_bytes = 256 * std::mem::size_of::<f32>();
    let limits = PcmLimits::new(ramp_bytes, ramp_bytes, 1).unwrap();
    let mut bank = SampleBank::new(source, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            source,
            (0..256).map(|n| 0.25 + n as f32 / 1024.0).collect(),
            limits,
        )
        .unwrap(),
    )
    .unwrap();
    prepare_audio(
        bank,
        vec![AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(SECTION_START),
            gain: 1.0,
        }],
        NativeAudioConfig {
            output_origin: native::output_origin(),
            start: Timestamp::from_nanos(SECTION_START),
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(1_000_000_000),
            voices: 2,
            max_render_frames: 64,
            playback_end_frame: end,
            gated_start: false,
        },
    )
    .unwrap()
}

fn request(device: String) -> AlsaRequest {
    AlsaRequest {
        device,
        format: DeviceFormat::new(TARGET_RATE, 2, SampleEncoding::Float32, None).unwrap(),
        period_frames: 64,
        buffer_frames: 256,
        allow_size_rounding: false,
        monotonic_domain: native::HOST,
    }
}

fn failure_diagnostics(
    stage: &str,
    output: &beatkernel_bms_runtime::native_alsa_output_ui::ConvertedAlsaOutputOwner,
    presentation: &beatkernel_bms_runtime::native_audio_presentation::NativeAudioPresentation,
    iterations: usize,
    converted_observations: usize,
) {
    let stream = output.current().unwrap().stream();
    eprintln!(
        "{stage}: iterations={iterations} converted_observations={converted_observations} \
         actual_snapshot={:?} actual_timing={:?} actual_converted={:?} \
         actual_source={:?} actual_boundaries={:?} owner_converted={:?} \
         owner_source={:?} owner_boundaries={:?} native_record={:?} target_basis={:?}",
        stream.snapshot(),
        stream.timing_snapshot(),
        stream.last_render_report(),
        stream.last_real_source_report(),
        stream.boundary_facts(),
        output.converted_report(),
        output.render_report(),
        output.target_boundary_facts(),
        presentation.latest_record(),
        presentation.target_basis(),
    );
}

#[test]
fn cold_solo_factory_refuses_invalid_native_request_without_requiring_input_devices() {
    for invalid in 0..3 {
        let prepared = audio(None);
        assert_eq!(prepared.mixer.config().format().sample_rate(), SOURCE_RATE);
        assert_eq!(prepared.bgm.config().sample_rate, SOURCE_RATE);
        let mut req = request("beatkernel-solo-must-not-acquire-invalid-request".into());
        match invalid {
            0 => req.period_frames = 0,
            1 => req.buffer_frames = req.period_frames,
            _ => req.monotonic_domain = native::OUTPUT,
        }
        let error = match native::open_target_output(req, prepared.mixer) {
            Ok(_) => panic!("invalid request acquired native output"),
            Err(error) => error,
        };
        if invalid == 0 {
            assert_eq!(
                error.downcast_ref::<beatkernel::audio::AudioError>(),
                Some(&beatkernel::audio::AudioError::InvalidCapacity)
            );
        } else {
            assert!(matches!(error.downcast_ref::<beatkernel_bms_runtime::native_alsa_replacement::AlsaReplacementError>(),
                Some(beatkernel_bms_runtime::native_alsa_replacement::AlsaReplacementError::Linux(
                    beatkernel_platform::linux::LinuxError::InvalidConfiguration(_)
                ))));
        }
    }
}

#[test]
#[ignore = "requires explicit BEATKERNEL_TEST_ALSA_DEVICE=null; no physical input/audio claim"]
fn actual_solo_factory_held_submission_recovers_original_source_bank_and_first_bgm_cue() {
    let device =
        std::env::var("BEATKERNEL_TEST_ALSA_DEVICE").expect("explicit ALSA endpoint required");
    let prepared = audio(None);
    assert_eq!(prepared.bgm.config().sample_rate, SOURCE_RATE);
    assert_eq!(prepared.bgm.config().output_origin, native::output_origin());
    assert_eq!(prepared.bgm.report().total_admitted, 1);
    let counters = prepared.mixer.counters();
    let mut output = native::open_target_output(request(device), prepared.mixer).unwrap();
    let initial = output.current().unwrap();
    let basis = initial.stream().frame_basis();
    assert_eq!(basis.sample_rate(), TARGET_RATE);
    assert_eq!(basis.origin(), native::output_origin());
    assert_eq!(initial.epoch(), 0);
    assert_eq!(initial.channel_matrix().source_channels(), 1);
    assert_eq!(initial.channel_matrix().target_channels(), 2);
    assert_eq!(
        initial.stream().configuration().format.sample_rate(),
        TARGET_RATE
    );
    let (config, timeout) = native::target_audio_timing_config(initial.stream()).unwrap();
    // Timing bounds derive from native target frames, not the PCM source rate.
    let expected_buffer_ns =
        i128::try_from((256u128 * 1_000_000_000).div_ceil(u128::from(TARGET_RATE))).unwrap();
    assert_eq!(
        timeout.as_nanos(),
        (expected_buffer_ns * 4 + 2_000_000_000) as i64
    );
    let mut presentation = new_target_audio_presentation(
        0,
        basis,
        native::HOST,
        ClockPoint {
            domain: native::LOGICAL,
            timestamp: Timestamp::ZERO,
        },
        config,
    )
    .unwrap();
    output.set_target_held(true).unwrap();
    output.current_mut().unwrap().stream_mut().start().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut first = None;
    let mut progress = false;
    let mut iterations = 0;
    let mut converted_observations = 0;
    while std::time::Instant::now() < deadline {
        iterations += 1;
        native::observe_target_output(&mut output, &mut presentation, None).unwrap();
        if let Some(report) = output.converted_report() {
            converted_observations += 1;
            assert_eq!(report.state, ConvertedOutputState::Held);
            assert_eq!(report.source_rate, SOURCE_RATE);
            assert_eq!(report.target_rate, TARGET_RATE);
            assert_eq!(report.source, None);
            assert_eq!(report.source_position.frame, 0);
            if first.is_some_and(|cursor| report.target_frame_cursor > cursor) {
                progress = true;
                break;
            }
            first.get_or_insert(report.target_frame_cursor);
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    if !progress {
        failure_diagnostics(
            "held progress missing",
            &output,
            &presentation,
            iterations,
            converted_observations,
        );
        eprintln!("held first_target_cursor={first:?}");
    }
    let stream = output.current().unwrap().stream();
    let native_snapshot = stream.snapshot();
    let timing = stream.timing_snapshot();
    let original_pair = timing
        .map(|timing| {
            beatkernel_platform::linux::alsa_presentation_pair_with_target_basis(timing, basis)
                .unwrap()
        })
        .flatten();
    assert!(native_snapshot.submitted_frames > 0);
    if let Some(record) = presentation.latest_record() {
        if let Some(pair) = original_pair {
            assert_eq!(pair.source.domain, native::OUTPUT);
            assert_eq!(pair.target.domain, native::HOST);
        }
        assert_eq!(record.pair().source.domain, native::OUTPUT);
        assert_eq!(record.pair().target.domain, native::HOST);
        assert_eq!(presentation.target_basis(), Some(basis));
        eprintln!(
            "held fixture evidence tier: native submissions/recovery plus original target clock"
        );
    } else {
        assert_eq!(original_pair, None, "available original native clock cannot be silently replaced with submission-only evidence");
        assert!(presentation.latest_record().is_none());
        assert_eq!(presentation.target_basis(), None);
        assert_eq!(
            timing.and_then(|timing| timing.estimated_played_frames),
            None
        );
        eprintln!("held fixture evidence tier: native submissions/recovery only; original target clock unavailable: {timing:?}");
    }
    output.stop().unwrap();
    assert!(
        progress,
        "bounded collection needs actual held target submission progress"
    );
    assert!(
        output.render_report().is_none(),
        "held PCM is not a source callback"
    );
    let current = output.current_mut().unwrap();
    assert!(current.stream().timing_snapshot().is_none());
    let mut recovered: ConvertedNativeOutputState = current.take_stopped_mixer().unwrap().unwrap();
    assert!(current.take_stopped_mixer().unwrap().is_none());
    assert_eq!(
        recovered.mixer().config().format().sample_rate(),
        SOURCE_RATE
    );
    assert_eq!(recovered.mixer().counters(), counters);
    assert_eq!(recovered.converter_owner().source_position().frame, 0);
    assert!(recovered
        .pending_samples()
        .iter()
        .all(|sample| *sample == 0.0));
    let pending = recovered.pending_frames();
    if pending > 0 {
        recovered.admit(pending).unwrap();
    }
    let report = recovered.render_pending(32).unwrap();
    assert_eq!(report.source_start_position.frame, 0);
    // Original BGM timestamp was mapped once from the section to OUTPUT zero.
    // A native held gap advances physical duration, never this source cue.
    for (frame, stereo) in recovered.pending_samples().chunks_exact(2).enumerate() {
        let expected = 0.25 + frame as f64 * 147.0 / (160.0 * 1024.0);
        for &sample in stereo {
            assert!((f64::from(sample) - expected).abs() < 2e-6);
        }
    }
    assert_eq!(recovered.mixer().counters().commands_applied, 1);
    assert_eq!(recovered.boundaries().source_rate, SOURCE_RATE);
}

#[test]
#[ignore = "requires BEATKERNEL_TEST_ALSA_CLOCK_DEVICE with original RUNNING played-frame evidence; null cannot prove crossing"]
fn actual_solo_target_finite_end_requires_source_endpoint_and_original_native_crossing() {
    let device = std::env::var("BEATKERNEL_TEST_ALSA_CLOCK_DEVICE").expect(
        "explicit clock-capable ALSA endpoint required; null submission proof cannot substitute",
    );
    let end_frame = u64::from(SOURCE_RATE);
    let prepared = audio(Some(end_frame));
    let mut output = native::open_target_output(request(device), prepared.mixer).unwrap();
    let initial = output.current().unwrap();
    let basis = initial.stream().frame_basis();
    let (config, _) = native::target_audio_timing_config(initial.stream()).unwrap();
    let mut presentation = new_target_audio_presentation(
        0,
        basis,
        native::HOST,
        ClockPoint {
            domain: native::LOGICAL,
            timestamp: Timestamp::ZERO,
        },
        config,
    )
    .unwrap();
    let mut end = NativeEnd::new(
        native::output_origin(),
        native::HOST,
        SOURCE_RATE,
        end_frame,
    )
    .unwrap()
    .with_target_basis(0, basis)
    .unwrap();
    output.set_target_held(true).unwrap();
    output.current_mut().unwrap().stream_mut().start().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut iterations = 0;
    let mut converted_observations = 0;
    while presentation.latest_record().is_none() && std::time::Instant::now() < deadline {
        iterations += 1;
        native::observe_target_output(&mut output, &mut presentation, Some(&mut end)).unwrap();
        converted_observations += usize::from(output.converted_report().is_some());
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    if presentation.latest_record().is_none() {
        failure_diagnostics(
            "finite native lower bracket missing",
            &output,
            &presentation,
            iterations,
            converted_observations,
        );
        output.stop().unwrap();
    }
    assert!(
        presentation.latest_record().is_some(),
        "need an actual native lower bracket"
    );
    assert!(output.target_boundary_facts().unwrap().end.is_none());
    output.set_target_held(false).unwrap();
    let mut crossing = None;
    while std::time::Instant::now() < deadline {
        iterations += 1;
        native::observe_target_output(&mut output, &mut presentation, None).unwrap();
        converted_observations += usize::from(output.converted_report().is_some());
        if let Some(boundary) = output
            .observe_target_audio_end(&mut end, &presentation)
            .unwrap()
        {
            crossing = Some(boundary);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    if crossing.is_none() {
        failure_diagnostics(
            "finite native crossing missing",
            &output,
            &presentation,
            iterations,
            converted_observations,
        );
    }
    output.stop().unwrap();
    let crossing = crossing.expect("bounded actual finite target/native crossing");
    let facts = output.target_boundary_facts().unwrap();
    let mapped = facts.end.expect("actual immutable target endpoint");
    assert_eq!(crossing.playback_frame, end_frame);
    assert_eq!(crossing.physical_frame, mapped.source_frame);
    assert_eq!(
        crossing.output,
        mapped.target_time.point(native::output_origin()).unwrap()
    );
    assert_eq!(crossing.host.domain, native::HOST);
    assert!(
        presentation
            .latest_record()
            .unwrap()
            .pair()
            .source
            .timestamp
            >= crossing.output.timestamp
    );
    let recovered: ConvertedNativeOutputState = output
        .current_mut()
        .unwrap()
        .take_stopped_mixer()
        .unwrap()
        .unwrap();
    assert_eq!(
        recovered.mixer().config().playback_end_frame(),
        Some(end_frame)
    );
    assert_eq!(recovered.mixer().playback_frame_cursor(), end_frame);
    assert!(recovered.mixer().is_paused());
    assert_eq!(recovered.boundaries().end, facts.end);
    assert_eq!(recovered.mixer().counters().commands_applied, 1);
}
