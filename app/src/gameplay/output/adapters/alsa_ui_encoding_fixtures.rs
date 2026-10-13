//! Exact ALSA encoding through public settings, complete owners and real admission.
//! Null execution proves software lifecycle only, never physical presentation.
use super::*;
use crate::{
    gameplay::output::{adapters::alsa::AlsaReplacementError, ports::OutputReplacementBackend},
    settings::OutputEncoding,
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioError, AudioFormat, CommandProducer, Mixer, MixerConfig,
        PcmLimits, PcmSample, ResampleQuality, SampleBank, SampleId, StoppedMixerSource, VoiceId,
    },
    time::{ClockDomainId, Timestamp},
};
use beatkernel_platform::{
    audio::{encode_pcm, NativeOutputState, SampleEncoding},
    linux::LinuxError,
};

fn modes() -> [(OutputEncoding, SampleEncoding); 4] {
    [
        (OutputEncoding::Float32, SampleEncoding::Float32),
        (
            OutputEncoding::Pcm16,
            SampleEncoding::Pcm {
                container_bits: 16,
                valid_bits: 16,
            },
        ),
        (
            OutputEncoding::Pcm24,
            SampleEncoding::Pcm {
                container_bits: 24,
                valid_bits: 24,
            },
        ),
        (
            OutputEncoding::Pcm32,
            SampleEncoding::Pcm {
                container_bits: 32,
                valid_bits: 32,
            },
        ),
    ]
}
fn args(pairs: &[(&str, &str)]) -> Vec<String> {
    pairs
        .iter()
        .flat_map(|(flag, value)| [(*flag).to_owned(), (*value).to_owned()])
        .collect()
}
fn current(channels: u16) -> AlsaRequest {
    AlsaRequest {
        device: "beatkernel-no-such-encoding-device".into(),
        format: DeviceFormat::new(48_000, channels, modes()[2].1, None).unwrap(),
        period_frames: 8,
        buffer_frames: 32,
        allow_size_rounding: false,
        monotonic_domain: ClockDomainId(8),
    }
}
fn matrix() -> ChannelMatrix {
    ChannelMatrix::new(1, 2, &[1.0, 0.5]).unwrap()
}
fn applied(native: AlsaRequest) -> AlsaAppliedConfig {
    AlsaAppliedConfig {
        format: native.format,
        buffer_frames: native.buffer_frames,
        period_frames: native.period_frames,
        requested: native,
        sizing_adjusted: false,
        output_domain: ClockDomainId(7),
        output_origin: Timestamp::from_nanos(-123),
    }
}
fn field(cap: &OutputCapability, flag: &str) -> String {
    cap.settings()
        .unwrap()
        .fields()
        .iter()
        .find(|field| field.flag == flag)
        .unwrap()
        .value
        .clone()
}

#[test]
fn every_encoding_survives_native_remixed_and_converted_combined_reconstruction() {
    let mut original = current(1);
    // Explicit direct ordering is valid for any channel count and must survive mapping.
    original.format = DeviceFormat::new(48_000, 1, original.format.encoding(), Some(0)).unwrap();
    let retained = ChannelMatrix::default_mix(1, 1).unwrap();
    for (choice, encoding) in modes() {
        let edits = args(&[
            ("--encoding", choice.as_str()),
            ("--alsa", "null"),
            ("--period-frames", "16"),
            ("--buffer-frames", "64"),
        ]);
        let native = request_for_args(&original, &edits).unwrap();
        assert_eq!(
            native.format,
            DeviceFormat::new(48_000, 1, encoding, Some(0)).unwrap()
        );
        assert_eq!((native.period_frames, native.buffer_frames), (16, 64));
        assert_eq!(native.device, "null");
        assert_eq!(native.monotonic_domain, original.monotonic_domain);
        assert_eq!(native.allow_size_rounding, original.allow_size_rounding);

        let mut edits = edits;
        edits.extend(args(&[("--output-matrix", "0.25;0.75")]));
        let remixed = remixed_request_for_args(&original, Some(&retained), &edits).unwrap();
        assert_eq!(
            remixed.native.format,
            DeviceFormat::new(48_000, 2, encoding, Some(0)).unwrap()
        );
        assert_eq!(
            (remixed.native.period_frames, remixed.native.buffer_frames),
            (16, 64)
        );
        assert_eq!(
            matrix_text(remixed.matrix.as_ref().unwrap()).unwrap(),
            "0.25;0.75"
        );
        edits.extend(args(&[("--rate", "32000")]));
        let converted = converted_request_for_args(&original, &retained, &edits).unwrap();
        assert_eq!(
            converted.native.format,
            DeviceFormat::new(32_000, 2, encoding, Some(0)).unwrap()
        );
        assert_eq!(
            (
                converted.native.period_frames,
                converted.native.buffer_frames
            ),
            (16, 64)
        );
        assert_eq!(converted.matrix, remixed.matrix);
        assert_eq!(converted.native.device, "null");
        assert_eq!(converted.native.monotonic_domain, original.monotonic_domain);
        assert_eq!(
            converted.native.allow_size_rounding,
            original.allow_size_rounding
        );
    }
    assert_eq!(original.format.encoding(), modes()[2].1);
    assert_eq!(original.format.channels(), 1);
    assert_eq!(retained, ChannelMatrix::default_mix(1, 1).unwrap());
}

#[test]
fn omitted_or_blank_encoding_keeps_pcm24_in_all_three_live_request_paths() {
    let native = current(2);
    let retained = matrix();
    for edits in [vec![], args(&[("--encoding", "")])] {
        assert_eq!(request_for_args(&native, &edits).unwrap(), native);
        let remixed = remixed_request_for_args(&native, Some(&retained), &edits).unwrap();
        assert_eq!(remixed.native, native);
        assert_eq!(remixed.matrix.as_ref(), Some(&retained));
        let converted = converted_request_for_args(&native, &retained, &edits).unwrap();
        assert_eq!(converted.native, native);
        assert_eq!(converted.matrix.as_ref(), Some(&retained));
    }
    let edits = args(&[("--encoding", ""), ("--period-frames", "4")]);
    assert_eq!(
        request_for_args(&native, &edits).unwrap().format,
        native.format
    );
    assert_eq!(
        remixed_request_for_args(&native, Some(&retained), &edits)
            .unwrap()
            .native
            .format,
        native.format
    );
    assert_eq!(
        converted_request_for_args(&native, &retained, &edits)
            .unwrap()
            .native
            .format,
        native.format
    );
}

#[test]
fn malformed_duplicate_and_foreign_schema_edits_refuse_and_legacy_rate_remains_forbidden() {
    let native = current(2);
    let retained = matrix();
    let invalid = [
        args(&[("--encoding", "PCM24")]),
        args(&[("--encoding", "pcm24 ")]),
        args(&[("--encoding", " pcm16")]),
        args(&[("--encoding", "pcm8")]),
        args(&[("--encoding", "pcm24\n")]),
        args(&[("--encoding", "pcm16"), ("--encoding", "pcm32")]),
        args(&[("--encoding", ""), ("--encoding", "pcm24")]),
        args(&[("--device", "42"), ("--encoding", "pcm16")]),
        args(&[("--backend", "wasapi"), ("--encoding", "float32")]),
        args(&[("--channels", "2"), ("--encoding", "pcm24")]),
        args(&[("--encoding", "pcm16"), ("--period-frames", "0")]),
        vec!["--encoding".into()],
    ];
    for edits in invalid {
        assert!(request_for_args(&native, &edits).is_err(), "{edits:?}");
        assert!(
            remixed_request_for_args(&native, Some(&retained), &edits).is_err(),
            "{edits:?}"
        );
        assert!(
            converted_request_for_args(&native, &retained, &edits).is_err(),
            "{edits:?}"
        );
        assert_eq!(native, current(2));
        assert_eq!(retained, matrix());
    }
    for value in ["", "32000"] {
        let edits = args(&[("--encoding", "pcm16"), ("--rate", value)]);
        assert!(request_for_args(&native, &edits).is_err());
        assert!(remixed_request_for_args(&native, Some(&retained), &edits).is_err());
    }
    for host in [SettingsHost::Windows, SettingsHost::Macos] {
        assert!(NativeSettings::output_only(&args(&[("--encoding", "pcm24")]), host).is_err());
    }
}

#[test]
fn actual_applied_encoding_and_rounded_sizing_roundtrip_without_approximating_pcm_widths() {
    for (choice, encoding) in modes() {
        let mut native = current(2);
        native.format = DeviceFormat::new(48_000, 2, encoding, None).unwrap();
        native.allow_size_rounding = true;
        let mut metadata = applied(native);
        metadata.period_frames = 16;
        metadata.buffer_frames = 64;
        metadata.sizing_adjusted = true;
        let cap = capability(&metadata).unwrap();
        assert_eq!(field(&cap, "--encoding"), choice.as_str());
        let next = request_for_args(&metadata.requested, &cap.current_args).unwrap();
        assert_eq!(next.format, metadata.format);
        assert_eq!((next.period_frames, next.buffer_frames), (16, 64));
        let remixed =
            remixed_request_for_args(&metadata.requested, Some(&matrix()), &cap.current_args)
                .unwrap();
        assert_eq!(remixed.native, next);
        assert_eq!(remixed.matrix, Some(matrix()));
        let cap = converted_capability(&metadata, &matrix()).unwrap();
        assert_eq!(field(&cap, "--encoding"), choice.as_str());
        let next =
            converted_request_for_args(&metadata.requested, &matrix(), &cap.current_args).unwrap();
        assert_eq!(next.native.format, metadata.format);
        assert_eq!(
            (next.native.period_frames, next.native.buffer_frames),
            (16, 64)
        );
        assert_eq!(next.matrix, Some(matrix()));
    }
    for (container_bits, valid_bits) in [(16, 12), (24, 16), (32, 24)] {
        let mut native = current(2);
        native.format = DeviceFormat::new(
            48_000,
            2,
            SampleEncoding::Pcm {
                container_bits,
                valid_bits,
            },
            None,
        )
        .unwrap();
        let metadata = applied(native);
        assert!(capability(&metadata).is_err());
        assert!(converted_capability(&metadata, &matrix()).is_err());
    }
    for case in 0..5 {
        let mut metadata = applied(current(2));
        match case {
            0 => {
                metadata.format =
                    DeviceFormat::new(48_000, 2, SampleEncoding::Float32, None).unwrap()
            }
            1 => metadata.period_frames = 0,
            2 => metadata.period_frames = metadata.buffer_frames,
            3 => metadata.sizing_adjusted = true,
            _ => {
                metadata.period_frames *= 2;
                metadata.sizing_adjusted = true;
            }
        }
        assert!(capability(&metadata).is_err(), "case {case}");
        assert!(
            converted_capability(&metadata, &matrix()).is_err(),
            "case {case}"
        );
    }
    assert!(converted_capability(
        &applied(current(2)),
        &ChannelMatrix::default_mix(1, 1).unwrap()
    )
    .is_err());
}

fn mixer(source_rate: u32) -> (CommandProducer, Mixer) {
    let source = AudioFormat::new(source_rate, 1).unwrap();
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
    let (mut producer, consumer) = command_queue(16).unwrap();
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
            AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn encoded(format: DeviceFormat, samples: &[f32]) -> Vec<u8> {
    let mut bytes = vec![0; samples.len() * usize::from(format.encoding().bytes_per_sample())];
    encode_pcm(format, samples, &mut bytes).unwrap();
    bytes
}
fn assert_pending_refusal(error: &AlsaReplacementError) {
    assert!(
        matches!(
            error,
            AlsaReplacementError::Linux(LinuxError::Mixer(AudioError::InvalidFormat))
        ),
        "must refuse before snd_pcm_open: {error}"
    );
}
#[test]
fn native_backend_refuses_pending_encoding_before_open_and_recovers_exact_owner_for_retry() {
    let (mut producer, source) = mixer(48_000);
    let native = current(1);
    let mut owner = NativeOutputState::new(source, native.format, None, 8)
        .unwrap_or_else(|_| panic!("valid complete native state"));
    owner.render_pending(8).unwrap();
    owner.admit(2).unwrap();
    let pcm = owner.pending_samples().to_vec();
    let bytes = encoded(native.format, &pcm);
    let basis = owner.output_frame_basis();
    let report = owner.pending_report();
    let counters = owner.mixer().counters();
    let source_format = owner.mixer().config().format();
    let mut backend = AlsaReplacementBackend;
    for (choice, _) in modes()
        .into_iter()
        .filter(|(choice, _)| *choice != OutputEncoding::Pcm24)
    {
        let changed = request_for_args(&native, &args(&[("--encoding", choice.as_str())])).unwrap();
        let failure = match backend.open(changed, owner, 3) {
            Ok(_) => panic!("pending encoding change must refuse"),
            Err(failure) => failure,
        };
        assert_pending_refusal(failure.error());
        let (_, recovered, pending, cleanup) = failure.into_parts();
        assert!(pending.is_none());
        assert!(cleanup.is_none());
        owner = recovered.expect("the original complete native state");
        assert_eq!(owner.format(), Some(native.format));
        assert_eq!(owner.pending_samples(), pcm);
        assert_eq!(encoded(native.format, owner.pending_samples()), bytes);
        assert_eq!(owner.pending_report(), report);
        assert_eq!(owner.output_frame_basis(), basis);
        assert_eq!(owner.admitted_frames(), 2);
        assert_eq!(owner.max_frames(), 8);
        assert_eq!(owner.mixer().counters(), counters);
        assert_eq!(owner.mixer().config().format(), source_format);
        assert_eq!(owner.mixer().frame_cursor(), 8);
        // Same-format retry passes the production owner's admission without
        // requiring an ALSA library. Native acquisition is exercised by null.
        owner.validate_reconfigure(native.format, None, 8).unwrap();
        assert_eq!(owner.pending_samples(), pcm);
        assert_eq!(owner.pending_report(), report);
        assert_eq!(owner.output_frame_basis(), basis);
        assert_eq!(owner.mixer().counters(), counters);
    }
    // The retained Mixer also keeps its original command consumer and voice cursor.
    owner.admit(6).unwrap();
    owner.render_pending(1).unwrap();
    assert_eq!(owner.pending_samples(), [0.125 + 8.0 / 512.0]);
    producer.request_pause(true);
    owner.admit(1).unwrap();
    assert!(owner.render_pending(1).unwrap().paused);
    assert_eq!(owner.pending_samples(), [0.0]);
}

fn converted_owner() -> (CommandProducer, ConvertedNativeOutputState) {
    let (producer, source) = mixer(44_100);
    let owner = ConvertedNativeOutputState::new(
        source,
        current(2).format,
        matrix(),
        ResampleQuality::Linear,
        8,
    )
    .unwrap_or_else(|_| panic!("valid complete converted state"));
    (producer, owner)
}

#[test]
fn converted_backend_refuses_pending_encoding_before_open_and_recovers_exact_converter_for_retry() {
    let (mut producer, mut owner) = converted_owner();
    let native = current(2);
    let retained = matrix();
    owner.render_pending(8).unwrap();
    owner.admit(2).unwrap();
    let pcm = owner.pending_samples().to_vec();
    let bytes = encoded(native.format, &pcm);
    let basis = owner.target_frame_basis();
    let report = owner.pending_report();
    let counters = owner.mixer().counters();
    let source_cursor = owner.mixer().frame_cursor();
    let phase = owner.converter_owner().source_position();
    let facts = owner.boundaries();
    let source_report = owner.last_real_source_report();
    let admitted_time = owner.admitted_target_time();
    let mut backend = ConvertedAlsaReplacementBackend;
    for (choice, _) in modes()
        .into_iter()
        .filter(|(choice, _)| *choice != OutputEncoding::Pcm24)
    {
        let changed = converted_request_for_args(
            &native,
            &retained,
            &args(&[("--encoding", choice.as_str())]),
        )
        .unwrap();
        let failure = match backend.open(changed, owner, 7) {
            Ok(_) => panic!("pending encoding change must refuse"),
            Err(failure) => failure,
        };
        assert_pending_refusal(failure.error());
        let (_, recovered, pending, cleanup) = failure.into_parts();
        assert!(pending.is_none());
        assert!(cleanup.is_none());
        owner = recovered.expect("the original complete converter");
        assert_eq!(owner.format(), native.format);
        assert_eq!(owner.pending_samples(), pcm);
        assert_eq!(encoded(native.format, owner.pending_samples()), bytes);
        assert_eq!(owner.pending_report(), report);
        assert_eq!(owner.target_frame_basis(), basis);
        assert_eq!(owner.admitted_frames(), 2);
        assert_eq!(owner.admitted_target_time(), admitted_time);
        assert_eq!(owner.max_frames(), 8);
        assert_eq!(owner.mixer().counters(), counters);
        assert_eq!(
            owner.mixer().config().format(),
            AudioFormat::new(44_100, 1).unwrap()
        );
        assert_eq!(owner.mixer().frame_cursor(), source_cursor);
        assert_eq!(owner.converter_owner().source_position(), phase);
        assert_eq!(owner.converter_owner().converter().matrix(), &retained);
        assert_eq!(owner.boundaries(), facts);
        assert_eq!(owner.last_real_source_report(), source_report);
        let same = converted_request_for_args(&native, &retained, &[]).unwrap();
        owner
            .validate_reconfigure(same.native.format, same.matrix.as_ref().unwrap(), 8)
            .unwrap();
        assert_eq!(owner.pending_samples(), pcm);
        assert_eq!(owner.pending_report(), report);
        assert_eq!(owner.target_frame_basis(), basis);
        assert_eq!(owner.converter_owner().source_position(), phase);
        assert_eq!(owner.mixer().counters(), counters);
        assert_eq!(owner.boundaries(), facts);
        assert_eq!(owner.last_real_source_report(), source_report);
    }
    producer.request_pause(true);
    owner.admit(6).unwrap();
    // Drain bounded converter lookahead until the original queue's pause is adopted.
    for _ in 0..4 {
        owner.render_pending(8).unwrap();
        if owner.mixer().is_paused() {
            return;
        }
        owner.admit(8).unwrap();
    }
    panic!("recovered converter must retain the original Mixer command consumer");
}

#[test]
#[ignore = "requires explicit BEATKERNEL_TEST_ALSA_DEVICE=null; software lifecycle only, no acoustic proof"]
fn actual_null_all_encodings_open_start_retire_and_retry_both_complete_owner_types() {
    assert_eq!(
        std::env::var("BEATKERNEL_TEST_ALSA_DEVICE").unwrap(),
        "null"
    );
    for (choice, encoding) in modes() {
        let mut native = current(2);
        native.device = "null".into();
        native.format = DeviceFormat::new(48_000, 2, encoding, None).unwrap();
        native.period_frames = 64;
        native.buffer_frames = 256;
        let (_producer, source) = mixer(48_000);
        let request = RemixedOutputRequest {
            native: native.clone(),
            matrix: Some(matrix()),
        };
        let mut backend = RemixedOutputBackend::new(AlsaReplacementBackend);
        let mut output = backend
            .open(request.clone(), NativeOutputState::from_mixer(source), 20)
            .unwrap_or_else(|failure| panic!("{}", failure.error()));
        assert_eq!(output.stream().configuration().format, native.format);
        assert_eq!(
            field(&capability_for_output(&output).unwrap(), "--encoding"),
            choice.as_str()
        );
        assert_eq!(
            field(&capability_for_output(&output).unwrap(), "--output-matrix"),
            "1;0.5"
        );
        backend.start(&mut output).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while output.stream().snapshot().submitted_frames < 64 {
            assert!(std::time::Instant::now() < deadline, "native null progress");
            std::thread::yield_now();
        }
        backend.retire(&mut output).unwrap();
        let owner = output.take_stopped_mixer().unwrap().unwrap();
        assert_eq!(
            owner.mixer().config().format(),
            AudioFormat::new(48_000, 1).unwrap()
        );
        assert!(output.take_stopped_mixer().unwrap().is_none());
        let mut output = backend
            .open(request, owner, 21)
            .unwrap_or_else(|failure| panic!("native same-format retry: {}", failure.error()));
        backend.start(&mut output).unwrap();
        backend.retire(&mut output).unwrap();
        assert!(output.take_stopped_mixer().unwrap().is_some());

        let (_producer, source) = mixer(44_100);
        let owner = ConvertedNativeOutputState::new(
            source,
            native.format,
            matrix(),
            ResampleQuality::Linear,
            64,
        )
        .unwrap_or_else(|_| panic!("valid converter"));
        let mut backend = ConvertedAlsaReplacementBackend;
        let mut output = backend
            .open(
                ConvertedAlsaReplacementRequest {
                    native: native.clone(),
                    matrix: Some(matrix()),
                },
                owner,
                22,
            )
            .unwrap_or_else(|failure| panic!("{}", failure.error()));
        assert_eq!(output.stream().configuration().format, native.format);
        assert_eq!(
            field(
                &capability_for_converted_output(&output).unwrap(),
                "--encoding"
            ),
            choice.as_str()
        );
        backend.start(&mut output).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while output.stream().snapshot().submitted_frames < 64 {
            assert!(
                std::time::Instant::now() < deadline,
                "converted null progress"
            );
            std::thread::yield_now();
        }
        backend.retire(&mut output).unwrap();
        let owner = output.take_stopped_mixer().unwrap().unwrap();
        assert_eq!(
            owner.mixer().config().format(),
            AudioFormat::new(44_100, 1).unwrap()
        );
        assert_eq!(owner.converter_owner().converter().matrix(), &matrix());
        assert!(output.take_stopped_mixer().unwrap().is_none());
        let mut output = backend
            .open(
                ConvertedAlsaReplacementRequest {
                    native,
                    matrix: Some(matrix()),
                },
                owner,
                23,
            )
            .unwrap_or_else(|failure| panic!("converted same-format retry: {}", failure.error()));
        backend.start(&mut output).unwrap();
        backend.retire(&mut output).unwrap();
        assert!(output.take_stopped_mixer().unwrap().is_some());
    }
}
