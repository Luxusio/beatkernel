//! Actual converted software state through the target-grid ALSA adapter.
use super::*;
use beatkernel::audio::Mixer;
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioError, AudioFormat, AudioLimits, CommandProducer,
        ConvertedOutputState, MixerConfig, PcmLimits, PcmSample, ResampleQuality, SampleBank,
        SampleId, TargetTime, VoiceId,
    },
    time::{ClockDomainId, Timestamp},
};
use beatkernel_platform::audio::{DeviceFormat, SampleEncoding};

fn rig() -> (CommandProducer, ConvertedNativeOutputState) {
    let source = AudioFormat::new(44_100, 1).unwrap();
    let limits = PcmLimits::new(256, 512, 1).unwrap();
    let mut bank = SampleBank::new(source, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            source,
            (0..64).map(|n| 0.125 + n as f32 / 512.0).collect(),
            limits,
        )
        .unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(-123),
            gain: 1.0,
        })
        .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            source,
            ClockDomainId(7),
            Timestamp::from_nanos(-123),
            AudioLimits::new(8, 2, 8, 256, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let format = DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap();
    let mut owner = ConvertedNativeOutputState::new(
        mixer,
        format,
        ChannelMatrix::new(1, 1, &[0.5]).unwrap(),
        ResampleQuality::Linear,
        8,
    )
    .unwrap_or_else(|_| panic!("valid actual converted owner"));
    owner.render_pending(8).unwrap();
    owner.admit(2).unwrap();
    (producer, owner)
}
fn request() -> ConvertedAlsaReplacementRequest {
    ConvertedAlsaReplacementRequest {
        native: AlsaRequest {
            device: "beatkernel-no-such-converted-device".into(),
            format: DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap(),
            buffer_frames: 32,
            period_frames: 8,
            allow_size_rounding: false,
            monotonic_domain: ClockDomainId(8),
        },
        matrix: None,
    }
}

#[test]
fn planned_target_basis_keeps_exact_first_unsent_fraction_and_actual_matrix_without_mutation() {
    let (_producer, mut owner) = rig();
    let backend = ConvertedAlsaReplacementBackend;
    let pcm = owner.pending_samples().to_vec();
    let phase = owner.converter_owner().source_position();
    let counters = owner.mixer().counters();
    let report = owner.pending_report();
    let planned = backend.planned_target_basis(&request(), &owner).unwrap();
    assert_eq!(planned, owner.target_frame_basis());
    assert_eq!(
        planned.start_time(),
        TargetTime::from_frames(2, 48_000).unwrap()
    );
    assert_eq!(planned.sample_rate(), 48_000);
    assert_eq!(planned.origin().domain, ClockDomainId(7));
    assert_eq!(planned.origin().timestamp, Timestamp::from_nanos(-123));
    assert_eq!(
        owner.converter_owner().converter().matrix().coefficients(),
        [0.5]
    );
    for (index, actual) in pcm.iter().enumerate() {
        let expected = 0.5 * (0.125 + (index + 2) as f64 * 147.0 / (160.0 * 512.0));
        assert!((f64::from(*actual) - expected).abs() < 2e-6);
    }
    assert_eq!(owner.pending_samples(), pcm);
    assert_eq!(owner.pending_report(), report);
    assert_eq!(owner.converter_owner().source_position(), phase);
    assert_eq!(owner.mixer().counters(), counters);
    assert_eq!(owner.admitted_frames(), 2);
    owner.admit(6).unwrap();
    let mut changed = request();
    changed.native.format = DeviceFormat::new(32_000, 1, SampleEncoding::Float32, None).unwrap();
    let next = backend.planned_target_basis(&changed, &owner).unwrap();
    assert_eq!(
        next.start_time(),
        TargetTime::from_frames(8, 48_000).unwrap()
    );
    assert_eq!(next.sample_rate(), 32_000);
    assert_eq!(owner.target_frame_basis().sample_rate(), 48_000);
    assert_eq!(owner.converter_owner().source_position(), phase);
}

#[test]
fn invalid_native_metadata_pending_interpretation_and_shape_refuse_with_full_owner_unchanged() {
    let (mut producer, mut owner) = rig();
    let pcm = owner.pending_samples().to_vec();
    let phase = owner.converter_owner().source_position();
    let basis = owner.target_frame_basis();
    let report = owner.pending_report();
    let counters = owner.mixer().counters();
    let facts = owner.boundaries();
    let source_report = owner.last_real_source_report();
    let mut backend = ConvertedAlsaReplacementBackend;
    for case in 0..10 {
        let mut invalid = request();
        match case {
            0 => invalid.native.device.clear(),
            1 => invalid.native.device = "bad\0endpoint".into(),
            2 => invalid.native.period_frames = 0,
            3 => invalid.native.buffer_frames = invalid.native.period_frames,
            4 => invalid.native.monotonic_domain = ClockDomainId(7),
            5 => {
                invalid.native.format =
                    DeviceFormat::new(48_000, 1, SampleEncoding::Float32, Some(1)).unwrap()
            }
            6 => {
                invalid.native.format =
                    DeviceFormat::new(32_000, 1, SampleEncoding::Float32, None).unwrap()
            }
            7 => {
                invalid.native.format =
                    DeviceFormat::new(48_000, 2, SampleEncoding::Float32, None).unwrap()
            }
            8 => invalid.matrix = Some(ChannelMatrix::new(1, 1, &[0.25]).unwrap()),
            _ => {
                invalid.native.period_frames = 512;
                invalid.native.buffer_frames = 1024;
            }
        }
        assert!(backend.planned_target_basis(&invalid, &owner).is_err());
        let failure = match backend.open(invalid, owner, 19) {
            Err(failure) => failure,
            Ok(_) => panic!("invalid native metadata must refuse before endpoint acquisition"),
        };
        assert!(matches!(
            failure.error(),
            AlsaReplacementError::Linux(
                LinuxError::InvalidConfiguration(_)
                    | LinuxError::Mixer(AudioError::InvalidFormat | AudioError::RenderCapacity)
            )
        ));
        let (_, recovered, pending, cleanup) = failure.into_parts();
        assert!(pending.is_none());
        assert!(cleanup.is_none());
        owner = recovered.expect("original complete software owner");
        assert_eq!(owner.pending_samples(), pcm);
        assert_eq!(owner.pending_report(), report);
        assert_eq!(owner.converter_owner().source_position(), phase);
        assert_eq!(owner.target_frame_basis(), basis);
        assert_eq!(owner.mixer().counters(), counters);
        assert_eq!(owner.boundaries(), facts);
        assert_eq!(owner.last_real_source_report(), source_report);
        assert_eq!(owner.admitted_frames(), 2);
        assert_eq!(owner.max_frames(), 8);
        assert_eq!(
            owner.converter_owner().converter().matrix().coefficients(),
            [0.5]
        );
    }
    // Recovery retains the original live queue, rather than only its PCM tail.
    producer
        .try_push(AudioCommand::Stop {
            voice: VoiceId(1),
            at: Timestamp::from_nanos(1_000_000_000),
        })
        .unwrap();
    assert_eq!(owner.mixer().counters(), counters);
}

#[test]
fn actual_invalid_endpoint_open_returns_concrete_native_error_and_complete_target_state() {
    let (_producer, owner) = rig();
    let pcm = owner.pending_samples().to_vec();
    let phase = owner.converter_owner().source_position();
    let basis = owner.target_frame_basis();
    let report = owner.pending_report();
    let counters = owner.mixer().counters();
    let mut backend = ConvertedAlsaReplacementBackend;
    let failure = match backend.open(request(), owner, 23) {
        Err(failure) => failure,
        Ok(_) => panic!("deliberately absent native endpoint must refuse"),
    };
    assert!(
        matches!(failure.error(), AlsaReplacementError::Linux(LinuxError::Alsa { operation: "snd_pcm_open", code }) if *code < 0)
    );
    let (_, owner, pending, cleanup) = failure.into_parts();
    assert!(pending.is_none());
    assert!(cleanup.is_none());
    let owner = owner.unwrap();
    assert_eq!(owner.pending_samples(), pcm);
    assert_eq!(owner.pending_report(), report);
    assert_eq!(owner.admitted_frames(), 2);
    assert_eq!(owner.target_frame_basis(), basis);
    assert_eq!(owner.converter_owner().source_position(), phase);
    assert_eq!(owner.mixer().counters(), counters);
    assert_eq!(
        owner.converter_owner().converter().matrix().coefficients(),
        [0.5]
    );
}

#[test]
#[ignore = "requires explicit BEATKERNEL_TEST_ALSA_DEVICE=null or an available native endpoint; no acoustic proof"]
fn actual_native_held_before_start_reports_and_stop_join_recover_complete_converter() {
    let device =
        std::env::var("BEATKERNEL_TEST_ALSA_DEVICE").expect("explicit native endpoint required");
    let (_producer, owner) = rig();
    let phase = owner.converter_owner().source_position();
    let counters = owner.mixer().counters();
    let source_report = owner.last_real_source_report();
    let basis = owner.target_frame_basis();
    let mut request = request();
    request.native.device = device;
    request.native.period_frames = 64;
    request.native.buffer_frames = 256;
    let mut backend = ConvertedAlsaReplacementBackend;
    let mut output = backend
        .open(request, owner, 29)
        .unwrap_or_else(|failure| panic!("native open refused: {}", failure.error()));
    assert_eq!(output.epoch(), 29);
    assert_eq!(backend.epoch(&output), 29);
    assert_eq!(backend.basis(&output), basis);
    assert_eq!(output.channel_matrix().coefficients(), [0.5]);
    assert!(backend
        .observe_native_target(&mut output)
        .unwrap()
        .is_none());
    assert!(backend
        .pause_observation(
            &output,
            beatkernel::time::ClockPair {
                source: basis.point_at_stream_frame(0).unwrap(),
                target: beatkernel::time::ClockPoint {
                    domain: ClockDomainId(8),
                    timestamp: Timestamp::ZERO
                },
            },
            beatkernel::time::ClockPoint {
                domain: ClockDomainId(8),
                timestamp: Timestamp::ZERO
            }
        )
        .is_err());
    backend.prepare_replacement_start(&mut output).unwrap();
    backend.start(&mut output).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut first = None;
    let mut progressed = None;
    while std::time::Instant::now() < deadline {
        if let Some(report) = backend.converted_report(&output).unwrap() {
            assert_eq!(report.state, ConvertedOutputState::Held);
            assert_eq!(report.source, None);
            assert!(report.target_frames > 0);
            if let Some(initial) = first {
                if report.target_frame_cursor > initial {
                    progressed = Some(report);
                    break;
                }
            } else {
                first = Some(report.target_frame_cursor);
            }
        }
        assert!(
            matches!(
                output.stream().snapshot().status,
                AlsaStatus::Ready | AlsaStatus::Running
            ),
            "actual worker must remain ready/running during bounded report collection"
        );
        if let Some(snapshot) = backend.observe_native_target(&mut output).unwrap() {
            assert_eq!(snapshot.epoch, 29);
            assert_eq!(snapshot.basis, basis);
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    backend.retire(&mut output).unwrap();
    assert!(
        progressed.is_some(),
        "bounded collection must observe real fresh held target progress"
    );
    assert_eq!(backend.render_report(&output).unwrap(), source_report);
    let owner = output.take_stopped_mixer().unwrap().unwrap();
    assert_eq!(owner.converter_owner().source_position(), phase);
    assert_eq!(owner.mixer().counters(), counters);
    assert_eq!(owner.last_real_source_report(), source_report);
    assert_eq!(
        owner.converter_owner().converter().matrix().coefficients(),
        [0.5]
    );
    assert!(owner.pending_samples().iter().all(|sample| *sample == 0.0));
    assert_eq!(owner.pending_is_held(), owner.pending_frames() > 0);
    assert!(output.take_stopped_mixer().unwrap().is_none());
    assert!(output.stream().timing_snapshot().is_none());
}
