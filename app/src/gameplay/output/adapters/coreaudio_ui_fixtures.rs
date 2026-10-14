//! Actual CoreAudio settings and cold ownership validation, without a HAL stream.
//! These macOS fixtures are typed on Linux; native execution remains separate.
use super::*;
use crate::gameplay::output::adapters::coreaudio::{
    CoreAudioCompleteReplacementOutput, CoreAudioReplacementError,
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioCounters, CommandProducer, Mixer, MixerConfig,
        OutputFrameBasis, OutputOpenFailure, PcmLimits, PcmSample, SampleBank, SampleId,
        SourcePlaybackSpan, VoiceId,
    },
    time::Timestamp,
};
use beatkernel_platform::{
    audio::{DeviceFormat, NativeOutputState, SampleEncoding},
    macos::audio::{CoreAudioApplied, CoreAudioError},
};

fn args(pairs: &[(&str, &str)]) -> Vec<String> {
    pairs
        .iter()
        .flat_map(|(flag, value)| [(*flag).to_owned(), (*value).to_owned()])
        .collect()
}

fn current() -> CoreAudioRequest {
    CoreAudioRequest {
        device: 7,
        format: AudioFormat::new(44_100, 2).unwrap(),
        buffer_frames: 64,
    }
}

fn matrix() -> ChannelMatrix {
    ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()
}

fn applied() -> CoreAudioApplied {
    let request = current();
    CoreAudioApplied {
        request,
        format: request.format,
        buffer_frames: request.buffer_frames,
        buffer_channels: vec![1, 1],
        output_domain: ClockDomainId(7),
        output_origin: Timestamp::from_nanos(-123),
        native_clock: ClockDomainId(8),
    }
}

fn field(capability: &OutputCapability, flag: &str) -> String {
    capability
        .settings()
        .unwrap()
        .fields()
        .iter()
        .find(|field| field.flag == flag)
        .unwrap()
        .value
        .clone()
}

#[test]
fn complete_matrix_edits_keep_original_source_width_and_rate_through_native_changes() {
    let native = current();
    let retained = matrix();
    let request = request_for_args(
        native,
        Some(&retained),
        &args(&[
            ("--device", "4294967295"),
            ("--buffer-frames", "128"),
            ("--output-matrix", "0.25;0.75;1"),
        ]),
    )
    .unwrap();
    assert_eq!(request.native.device, u32::MAX);
    assert_eq!(request.native.buffer_frames, 128);
    assert_eq!(request.native.format.sample_rate(), 44_100);
    assert_eq!(request.native.format.channels(), 3);
    let selected = request.matrix.as_ref().unwrap();
    assert_eq!(
        (selected.source_channels(), selected.target_channels()),
        (1, 3)
    );
    assert_eq!(matrix_text(selected).unwrap(), "0.25;0.75;1");

    for edits in [vec![], args(&[("--output-matrix", "")])] {
        let unchanged = request_for_args(request.native, Some(selected), &edits).unwrap();
        assert_eq!(unchanged, request);
    }
    let exact = request_for_args(
        request.native,
        Some(selected),
        &args(&[("--output-matrix", "exact")]),
    )
    .unwrap();
    assert_eq!(exact.native.format, AudioFormat::new(44_100, 1).unwrap());
    assert_eq!(exact.native.device, u32::MAX);
    assert_eq!(exact.native.buffer_frames, 128);
    assert!(exact.matrix.is_none());
    assert_eq!(native, current());
    assert_eq!(retained, matrix());
}

#[test]
fn decimal_geometry_parser_accepts_bounds_and_refuses_zero_malformed_and_overflow_edits() {
    let native = current();
    let retained = matrix();
    let maximum = AudioLimits::MAX_RENDER_FRAMES.to_string();
    let accepted = request_for_args(
        native,
        Some(&retained),
        &args(&[("--device", "1"), ("--buffer-frames", &maximum)]),
    )
    .unwrap();
    assert_eq!(accepted.native.device, 1);
    assert_eq!(
        accepted.native.buffer_frames as usize,
        AudioLimits::MAX_RENDER_FRAMES
    );
    assert_eq!(accepted.matrix.as_ref(), Some(&retained));

    for flag in ["--device", "--buffer-frames"] {
        for value in ["0", "-1", "+1", "1.0", " 1", "1 ", "1\n", "4294967296"] {
            assert!(
                request_for_args(native, Some(&retained), &args(&[(flag, value)])).is_err(),
                "{flag}={value:?}"
            );
            assert_eq!(native, current());
            assert_eq!(retained, matrix());
        }
    }
    let excessive = (AudioLimits::MAX_RENDER_FRAMES + 1).to_string();
    assert!(request_for_args(
        native,
        Some(&retained),
        &args(&[("--buffer-frames", &excessive)])
    )
    .is_err());
    let unchanged = request_for_args(
        native,
        Some(&retained),
        &args(&[("--device", ""), ("--buffer-frames", "")]),
    )
    .unwrap();
    assert_eq!(unchanged.native, native);
    assert_eq!(unchanged.matrix.as_ref(), Some(&retained));
}

#[test]
fn unsupported_schema_duplicate_fields_and_wrong_source_matrix_refuse_without_mutation() {
    let native = current();
    let retained = matrix();
    for edits in [
        args(&[("--rate", "32000")]),
        args(&[("--rate", "")]),
        args(&[("--channels", "2")]),
        args(&[("--encoding", "float32")]),
        args(&[("--period-frames", "32")]),
        args(&[("--backend", "coreaudio")]),
        args(&[("--device", "1"), ("--device", "2")]),
        args(&[("--buffer-frames", "32"), ("--buffer-frames", "64")]),
        args(&[("--output-matrix", "1,0")]),
        args(&[("--output-matrix", "NaN")]),
        args(&[("--output-matrix", "1;")]),
        vec!["--buffer-frames".into()],
    ] {
        assert!(
            request_for_args(native, Some(&retained), &edits).is_err(),
            "{edits:?}"
        );
        assert_eq!(native, current());
        assert_eq!(retained, matrix());
    }
}

#[test]
fn capability_uses_actual_callback_maximum_but_advertises_exact_nominal_geometry() {
    let metadata = applied();
    let retained = matrix();
    for maximum in [64, 128, AudioLimits::MAX_RENDER_FRAMES as u32] {
        let cap = capability_for_configuration(&metadata, maximum, Some(&retained)).unwrap();
        assert_eq!(cap.host, SettingsHost::Macos);
        assert_eq!(field(&cap, "--device"), "7");
        assert_eq!(field(&cap, "--buffer-frames"), "64");
        assert_eq!(field(&cap, "--output-matrix"), "1;-0.5");
        let reconstructed =
            request_for_args(metadata.request, Some(&retained), &cap.current_args).unwrap();
        assert_eq!(reconstructed.native, metadata.request);
        assert_eq!(reconstructed.matrix.as_ref(), Some(&retained));
        assert!(!cap
            .settings()
            .unwrap()
            .fields()
            .iter()
            .any(|field| field.flag == "--rate"));
    }
    let exact = capability_for_configuration(&metadata, 128, None).unwrap();
    assert_eq!(field(&exact, "--output-matrix"), "exact");
    assert_eq!(
        request_for_args(metadata.request, None, &exact.current_args)
            .unwrap()
            .native,
        metadata.request
    );
}

#[test]
fn inconsistent_capability_geometry_format_and_matrix_are_not_advertised() {
    let metadata = applied();
    for maximum in [0, 63, AudioLimits::MAX_RENDER_FRAMES as u32 + 1, u32::MAX] {
        assert!(capability_for_configuration(&metadata, maximum, Some(&matrix())).is_err());
    }
    let mut wrong_rate = metadata.clone();
    wrong_rate.format = AudioFormat::new(48_000, 2).unwrap();
    let mut wrong_channels = metadata.clone();
    wrong_channels.format = AudioFormat::new(44_100, 1).unwrap();
    let mut wrong_nominal = metadata.clone();
    wrong_nominal.buffer_frames = 128;
    let mut zero = metadata.clone();
    zero.request.buffer_frames = 0;
    zero.buffer_frames = 0;
    let mut excessive = metadata.clone();
    excessive.request.buffer_frames = AudioLimits::MAX_RENDER_FRAMES as u32 + 1;
    excessive.buffer_frames = excessive.request.buffer_frames;
    for invalid in [wrong_rate, wrong_channels, wrong_nominal, zero, excessive] {
        assert!(capability_for_configuration(&invalid, 128, Some(&matrix())).is_err());
    }
    assert!(capability_for_configuration(
        &metadata,
        128,
        Some(&ChannelMatrix::default_mix(1, 1).unwrap())
    )
    .is_err());
    assert_eq!(metadata, applied());
}

fn active_mixer() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm_limits = PcmLimits::new(128, 512, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 32], pcm_limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(-123),
            gain: 1.,
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
    mixer.render(&mut [0.; 3]).unwrap();
    mixer.enable_source_playback_history(16).unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    let mut paused = [1.; 2];
    assert!(mixer.render(&mut paused).unwrap().paused);
    assert_eq!(paused, [0.; 2]);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(-123),
            gain: 1.,
        })
        .unwrap();
    (producer, mixer)
}

#[derive(Debug, PartialEq)]
struct MixerFacts {
    basis: OutputFrameBasis,
    playback: u64,
    counters: AudioCounters,
    identity: (u32, u64, u64),
    history_end: u64,
    history_capacity: usize,
    history_revision: u64,
    spans: Vec<SourcePlaybackSpan>,
}

fn facts(mixer: &Mixer) -> MixerFacts {
    let history = mixer.source_playback_history().unwrap();
    MixerFacts {
        basis: mixer.output_frame_basis(),
        playback: mixer.playback_frame_cursor(),
        counters: mixer.counters(),
        identity: (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
        ),
        history_end: history.pulled_end_frame(),
        history_capacity: history.capacity(),
        history_revision: history.retirement_revision(),
        spans: (0..history.len())
            .map(|index| history.span(index).unwrap())
            .collect(),
    }
}

#[test]
fn complete_owner_cold_refusal_keeps_structured_failure_original_history_and_live_commands() {
    type Failure = OutputOpenFailure<
        CoreAudioReplacementError,
        CoreAudioCompleteReplacementOutput,
        NativeOutputState,
    >;
    let valid = CoreAudioRequest {
        device: 7,
        format: AudioFormat::new(1000, 1).unwrap(),
        buffer_frames: 8,
    };
    for (request, expected) in [
        (
            CoreAudioRequest { device: 0, ..valid },
            CoreAudioError::InvalidRequest,
        ),
        (
            CoreAudioRequest {
                buffer_frames: 0,
                ..valid
            },
            CoreAudioError::InvalidRequest,
        ),
        (
            CoreAudioRequest {
                format: AudioFormat::new(2000, 1).unwrap(),
                ..valid
            },
            CoreAudioError::InvalidRequest,
        ),
        (
            CoreAudioRequest {
                format: AudioFormat::new(1000, 2).unwrap(),
                ..valid
            },
            CoreAudioError::InvalidRequest,
        ),
        (
            CoreAudioRequest {
                buffer_frames: 33,
                ..valid
            },
            CoreAudioError::Capacity,
        ),
    ] {
        let (mut producer, mixer) = active_mixer();
        let before = facts(&mixer);
        let clock = MachClock::new(ClockDomainId(8), ClockDomainId(9)).unwrap();
        let error = match complete_owner(request, mixer, clock, ClockDomainId(9)) {
            Ok(_) => panic!("invalid cold request must refuse before HAL setup"),
            Err(error) => error,
        };
        let failure = error
            .downcast::<Failure>()
            .expect("the UI constructor must preserve its typed complete-open failure");
        match failure.error() {
            CoreAudioReplacementError::Native(error) => assert_eq!(error, &expected),
            error => panic!("unexpected cold refusal: {error:?}"),
        }
        assert!(failure.pending_owner().is_none());
        assert!(failure.cleanup_error().is_none());
        let (_, state, pending, cleanup) = (*failure).into_parts();
        assert!(pending.is_none());
        assert!(cleanup.is_none());
        let mut state = state.expect("the unique complete software owner must be returned");
        assert_eq!(facts(state.mixer()), before);
        assert_eq!(state.output_frame_basis(), before.basis);
        assert!(state.format().is_none());
        assert_eq!(state.max_frames(), 0);
        assert_eq!(state.pending_report(), None);
        assert_eq!(state.pending_frames(), 0);
        assert_eq!(state.admitted_frames(), 0);
        assert!(state.pending_samples().is_empty());
        assert!(!state.admission_is_uncertain());

        state
            .reconfigure(
                DeviceFormat::new(1000, 1, SampleEncoding::Float32, None).unwrap(),
                None,
                8,
            )
            .unwrap();
        producer.request_pause(false);
        let report = state.render_pending(2).unwrap();
        assert!(!report.paused);
        assert_eq!(report.start_frame, before.basis.start_physical_frame());
        assert_eq!(state.pending_samples(), [0.5; 2]);
    }
}

#[test]
fn legacy_and_complete_owner_ui_apis_compose_without_constructing_native_handles() {
    // Function-item typing checks the real public composition only. It does not
    // establish request, epoch, publication, or device lifecycle execution.
    let _: fn(CoreAudioStream, MachClock, ClockDomainId) -> NativeCoreAudioOutputOwner = owner;
    let _: fn(
        CoreAudioRequest,
        Mixer,
        MachClock,
        ClockDomainId,
    ) -> Result<NativeCoreAudioOutputOwner, Box<dyn std::error::Error>> = complete_owner;
    let _: fn(
        &NativeCoreAudioOutputOwner,
        bool,
    ) -> Result<NativeCoreAudioOutputUi, Box<dyn std::error::Error>> = NativeCoreAudioOutputUi::new;
    let _: fn(&NativeCoreAudioOutputUi) -> bool = NativeCoreAudioOutputUi::pending;
    let _: for<'a> fn(
        &mut NativeCoreAudioOutputUi,
        &mut NativeCoreAudioOutputOwner,
        GameplayOutputContext<'a, PresentationDiscipline>,
        ClockPoint,
    ) -> Result<bool, Box<dyn std::error::Error>> = NativeCoreAudioOutputUi::service;
    let _: for<'a> fn(
        &mut NativeCoreAudioOutputUi,
        &mut NativeCoreAudioOutputOwner,
        GameplayAudioOutputContext<'a>,
        ClockPoint,
    ) -> Result<bool, Box<dyn std::error::Error>> = NativeCoreAudioOutputUi::service_audio;
    let _: for<'a> fn(
        &mut NativeCoreAudioOutputUi,
        &mut NativeCoreAudioOutputOwner,
        GameplayAudioOutputContext<'a>,
        ClockPoint,
        crate::gameplay::output::ports::SourcePlaybackIdentity,
        bool,
    ) -> Result<bool, Box<dyn std::error::Error>> =
        NativeCoreAudioOutputUi::service_coordinate_source_audio;
}
