//! Real cold Mixer/complete-state composition, executable without HAL.
//! Native-only bindings below are Rust typing evidence; they do not run IOHID,
//! open a CoreAudio device, or establish presentation/paused-publication proof.
use super::{initial_audio_config, parse, Options};
use beatkernel::{
    audio::{
        AudioCommand, AudioError, AudioLimits, CommandProducer, OutputFrameBasis, PcmLimits,
        PcmSample, PracticeController, PracticeLimits, PracticeRegion, SampleBank, SampleId,
        SourcePlaybackSpan, VoiceId,
    },
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use beatkernel_bms_runtime::{
    gameplay::output::ports::SourcePlaybackIdentity,
    native_audio::{
        prepare_audio_with_speed, prepare_retained_audio_with_speed, NativePracticeAudioConfig,
    },
    native_audio_startup::{
        cold_source_coordinate_identity, prepare_source_coordinate_history,
        source_coordinate_history_capacity,
    },
};
use beatkernel_platform::audio::{DeviceFormat, NativeOutputState, SampleEncoding};

fn options() -> Options {
    parse(
        &[
            "--chart",
            "cold-composition.bms",
            "--device",
            "42",
            "--keyboard-registry",
            "900",
            "--rate",
            "48000",
            "--channels",
            "1",
            "--buffer-frames",
            "64",
            "--preroll-ns",
            "0",
            "--voices",
            "2",
            "--end-ns",
            "1000000000",
            "--bind",
            "11:04",
        ]
        .map(str::to_owned),
    )
    .unwrap()
}

fn origin() -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(3),
        timestamp: Timestamp::ZERO,
    }
}

fn original_pcm(start: usize, frames: usize) -> Vec<f32> {
    (start..start + frames)
        .map(|frame| ((frame % 8) + 1) as f32 / 16.0)
        .collect()
}

fn bank(options: &Options) -> SampleBank {
    let limits = PcmLimits::new(4096, 8192, 1).unwrap();
    let mut bank = SampleBank::new(options.format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(options.format, original_pcm(0, 512), limits).unwrap(),
    )
    .unwrap();
    bank
}

fn cue() -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(1),
        sample: SampleId(1),
        at: Timestamp::ZERO,
        gain: 1.0,
    }
}

fn target(options: &Options) -> DeviceFormat {
    DeviceFormat::new(
        options.format.sample_rate(),
        options.format.channels(),
        SampleEncoding::Float32,
        None,
    )
    .unwrap()
}

fn retained_state(
    actual_maximum: usize,
) -> (
    NativeOutputState,
    PracticeController,
    SourcePlaybackIdentity,
    OutputFrameBasis,
    CommandProducer,
) {
    let options = options();
    // This is the same config builder consumed by the actual solo launcher.
    let config = initial_audio_config(&options, origin(), true, None, false);
    let mut prepared = prepare_retained_audio_with_speed(
        bank(&options),
        vec![cue()],
        config,
        NativePracticeAudioConfig {
            region: PracticeRegion::new(
                options.live_speed.start(),
                options.live_speed.end().unwrap(),
                false,
            )
            .unwrap(),
            limits: PracticeLimits::new(8, 1, 64, 8, 16).unwrap(),
        },
        options.live_speed.speed(),
        options.live_speed.end(),
    )
    .unwrap();
    assert_eq!(
        prepared
            .audio
            .mixer
            .configuration()
            .limits()
            .max_render_frames(),
        AudioLimits::MAX_RENDER_FRAMES
    );
    assert_eq!(prepared.audio.mixer.start_gate_frame(), None);
    assert_eq!(prepared.audio.mixer.config().playback_end_frame(), None);
    let identity = prepare_source_coordinate_history(
        &mut prepared.audio.mixer,
        source_coordinate_history_capacity(options.buffer).unwrap(),
    )
    .unwrap();
    let creation = prepared.audio.mixer.output_frame_basis();
    // Exercise the public complete-state cold PCM preparation at the actual
    // callback maximum. Native open's opt-in history growth is separately tested
    // inside the platform crate, whose preparation method is crate-private.
    let state =
        NativeOutputState::new(prepared.audio.mixer, target(&options), None, actual_maximum)
            .unwrap_or_else(|failure| {
                panic!("cold retained preparation refused: {:?}", failure.error())
            });
    (
        state,
        prepared.practice,
        identity,
        creation,
        prepared.audio.producer,
    )
}

fn history_spans(state: &NativeOutputState) -> Vec<SourcePlaybackSpan> {
    let history = state.mixer().source_playback_history().unwrap();
    (0..history.len())
        .map(|index| history.span(index).unwrap())
        .collect()
}

#[test]
fn solo_retained_nominal64_accepts_actual128_with_a_separate_render_ceiling() {
    let (mut state, practice, identity, creation, _producer) = retained_state(128);
    assert_eq!(state.max_frames(), 128);
    assert!(state.max_frames() < AudioLimits::MAX_RENDER_FRAMES);
    assert_eq!(state.output_frame_basis(), creation);
    assert_eq!(state.render_pending(129), Err(AudioError::RenderCapacity));
    assert_eq!(state.mixer().frame_cursor(), 0);
    assert!(state.mixer().source_playback_history().unwrap().is_empty());

    let report = state.render_pending(128).unwrap();
    assert_eq!((report.start_frame, report.frames), (0, 128));
    assert_eq!(report.playback_frames, 128);
    assert_eq!(state.pending_samples(), original_pcm(0, 128));
    assert_eq!(state.pending_frames(), 128);
    assert_eq!(state.output_frame_basis(), creation);
    assert_eq!(
        cold_source_coordinate_identity(state.mixer()).unwrap(),
        identity
    );
    assert_eq!(identity.initial_physical(), 0);
    assert_eq!(identity.initial_playback(), 0);
    assert_eq!(practice.history_retention_pins().0, Some(0));
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .pulled_end_frame(),
        128
    );

    state.admit(32).unwrap();
    assert_eq!(state.output_frame_basis().start_physical_frame(), 32);
    assert_eq!(state.mixer().frame_cursor(), 128);
    assert_eq!(state.pending_samples(), original_pcm(32, 96));
    assert_eq!(practice.history_retention_pins().0, Some(0));
    assert_eq!(
        cold_source_coordinate_identity(state.mixer()).unwrap(),
        identity
    );
}

#[test]
fn solo_retained_replacement64_to128_preserves_actual_suffix_history_and_creation_pin() {
    let options = options();
    let (mut state, practice, identity, creation, _producer) = retained_state(64);
    state.render_pending(64).unwrap();
    state.admit(16).unwrap();
    let report = state.pending_report();
    let basis = state.output_frame_basis();
    let spans = history_spans(&state);
    let pins = practice.history_retention_pins();
    assert_eq!(pins.0, Some(creation.start_physical_frame()));
    assert_eq!(state.pending_samples(), original_pcm(16, 48));

    state.reconfigure(target(&options), None, 128).unwrap();
    assert_eq!(state.max_frames(), 128);
    assert_eq!(state.pending_report(), report);
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(state.admitted_frames(), 16);
    assert_eq!(state.pending_samples(), original_pcm(16, 48));
    assert_eq!(history_spans(&state), spans);
    assert_eq!(practice.history_retention_pins(), pins);
    assert_eq!(
        cold_source_coordinate_identity(state.mixer()).unwrap(),
        identity
    );
    assert_eq!(state.render_pending(128), Err(AudioError::InvalidBuffer));

    state.admit(48).unwrap();
    let next = state.render_pending(128).unwrap();
    assert_eq!((next.start_frame, next.frames), (64, 128));
    assert_eq!(state.pending_samples(), original_pcm(64, 128));
    assert_eq!(state.output_frame_basis().start_physical_frame(), 64);
    assert_eq!(state.mixer().frame_cursor(), 192);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(
        history.retained_start_frame(),
        creation.start_physical_frame()
    );
    assert_eq!(history.retirement_revision(), 0);
    assert_eq!(history.pulled_end_frame(), 192);
    assert_eq!(practice.history_retention_pins(), pins);
    assert_eq!(
        cold_source_coordinate_identity(state.mixer()).unwrap(),
        identity
    );
}

#[test]
fn solo_retained_above_ceiling_refuses_without_changing_pcm_or_source_identity() {
    let options = options();
    let (mut state, practice, identity, _, _producer) = retained_state(64);
    state.render_pending(64).unwrap();
    state.admit(16).unwrap();
    let report = state.pending_report();
    let basis = state.output_frame_basis();
    let spans = history_spans(&state);
    let pins = practice.history_retention_pins();

    assert_eq!(
        state.reconfigure(target(&options), None, AudioLimits::MAX_RENDER_FRAMES + 1),
        Err(AudioError::RenderCapacity)
    );
    assert_eq!(state.max_frames(), 64);
    assert_eq!(state.pending_report(), report);
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(state.pending_samples(), original_pcm(16, 48));
    assert_eq!(history_spans(&state), spans);
    assert_eq!(practice.history_retention_pins(), pins);
    assert_eq!(
        cold_source_coordinate_identity(state.mixer()).unwrap(),
        identity
    );
}

#[test]
fn solo_scalar_network_preparation_keeps_nominal_limit_finite_fence_and_start_gate() {
    let options = options();
    let config = initial_audio_config(&options, origin(), false, Some(512), true);
    let prepared = prepare_audio_with_speed(
        bank(&options),
        vec![cue()],
        config,
        options.live_speed.speed(),
    )
    .unwrap();
    assert_eq!(
        prepared.mixer.configuration().limits().max_render_frames(),
        64
    );
    assert_eq!(prepared.mixer.config().playback_end_frame(), Some(512));
    assert_eq!(prepared.mixer.start_gate_frame(), Some(None));
    assert!(prepared.mixer.source_playback_history().is_none());
    let failure = match NativeOutputState::new(prepared.mixer, target(&options), None, 128) {
        Ok(_) => panic!("scalar/network preparation must keep its original render bound"),
        Err(failure) => failure,
    };
    assert_eq!(failure.error(), &AudioError::RenderCapacity);
    let mixer = failure.mixer().unwrap();
    assert_eq!(mixer.frame_cursor(), 0);
    assert_eq!(mixer.playback_frame_cursor(), 0);
    assert_eq!(mixer.start_gate_frame(), Some(None));
    assert_eq!(mixer.config().playback_end_frame(), Some(512));
}

#[cfg(target_os = "macos")]
#[test]
fn solo_native_complete_owner_and_actual_device_hooks_are_statically_composable() {
    use super::native::{AudioSeedDevice, GameplayDevice};
    use beatkernel::{
        audio::{InterpolatedTargetTime, Mixer},
        time::ClockDomainId,
    };
    use beatkernel_bms_runtime::{
        gameplay::output::{
            adapters::coreaudio_ui::{self, NativeCoreAudioOutputOwner, NativeCoreAudioOutputUi},
            ports::NativeSourcePlaybackQuery,
        },
        gameplay_presentation::{GameplayAudioOutputContext, GameplayDevice as GameplayDevicePort},
        native_audio_presentation::NativeAudioPresentation,
        native_audio_startup::NativeAudioSeedPort,
        native_coordinate_control::CoordinateControlFacts,
        native_gameplay::NativeGameplayResult,
        native_playback_phase::NativePlaybackMapping,
    };
    use beatkernel_platform::macos::{
        audio::{CoreAudioRequest, CoreAudioSourcePlaybackSnapshot, CoreAudioStream},
        clock::MachClock,
    };

    let _: fn(
        CoreAudioRequest,
        Mixer,
        MachClock,
        ClockDomainId,
    ) -> NativeGameplayResult<NativeCoreAudioOutputOwner> = coreaudio_ui::complete_owner;
    let _: fn(
        &mut NativeCoreAudioOutputOwner,
        &mut NativeAudioPresentation,
        SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> =
        NativeCoreAudioOutputOwner::observe_native_control;
    let _: fn(
        &mut NativeCoreAudioOutputOwner,
        &NativeAudioPresentation,
        &InterpolatedTargetTime,
        SourcePlaybackIdentity,
    ) -> NativeGameplayResult<
        Option<NativeSourcePlaybackQuery<CoreAudioSourcePlaybackSnapshot>>,
    > = NativeCoreAudioOutputOwner::conditional_source_playback_at;
    let _: fn(
        &mut NativeCoreAudioOutputOwner,
        &NativeAudioPresentation,
        &InterpolatedTargetTime,
        SourcePlaybackIdentity,
        Option<u64>,
    ) -> NativeGameplayResult<Option<u64>> =
        NativeCoreAudioOutputOwner::request_source_history_retirement_with_source_pin;
    let _: fn(
        &mut NativeCoreAudioOutputUi,
        &mut NativeCoreAudioOutputOwner,
        GameplayAudioOutputContext<'_>,
        ClockPoint,
        SourcePlaybackIdentity,
        bool,
    ) -> NativeGameplayResult<bool> = NativeCoreAudioOutputUi::service_coordinate_source_audio;

    fn current_stream(owner: &NativeCoreAudioOutputOwner) -> Option<&CoreAudioStream> {
        owner.current().map(|output| output.stream())
    }
    let _: fn(&NativeCoreAudioOutputOwner) -> Option<&CoreAudioStream> = current_stream;

    // Bind the actual executable's device and seed types. Trait typing alone
    // cannot prove that overrides call the conditional provider or that a real
    // paused native transaction publishes; common pump execution and macOS
    // acceptance remain separate evidence obligations.
    fn actual_device_hooks<'device>() {
        let _: fn(
            &mut GameplayDevice<'device>,
            &NativeAudioPresentation,
            &InterpolatedTargetTime,
            SourcePlaybackIdentity,
        ) -> NativeGameplayResult<Option<NativePlaybackMapping>> =
            <GameplayDevice<'device> as GameplayDevicePort>::conditional_playback_mapping_at;
        let _: fn(
            &mut GameplayDevice<'device>,
            &mut NativeAudioPresentation,
            SourcePlaybackIdentity,
        ) -> NativeGameplayResult<Option<CoordinateControlFacts>> =
            <GameplayDevice<'device> as GameplayDevicePort>::observe_coordinate_control;
        let _: fn(
            &mut GameplayDevice<'device>,
            &NativeAudioPresentation,
            &InterpolatedTargetTime,
            SourcePlaybackIdentity,
            Option<u64>,
        ) -> NativeGameplayResult<Option<u64>> =
            <GameplayDevice<'device> as GameplayDevicePort>::request_history_retirement;
        let _: fn(
            &mut GameplayDevice<'device>,
            GameplayAudioOutputContext<'_>,
            ClockPoint,
            SourcePlaybackIdentity,
            bool,
        ) -> NativeGameplayResult<bool> =
            <GameplayDevice<'device> as GameplayDevicePort>::publish_paused_coordinate_audio_output;
        let _: fn(
            &mut AudioSeedDevice<'device>,
            &mut NativeAudioPresentation,
            SourcePlaybackIdentity,
        ) -> NativeGameplayResult<Option<CoordinateControlFacts>> =
            <AudioSeedDevice<'device> as NativeAudioSeedPort>::observe_coordinate_audio;
    }
    actual_device_hooks();
    let _: fn(&CoreAudioStream) -> u32 = CoreAudioStream::max_callback_frames;
}
