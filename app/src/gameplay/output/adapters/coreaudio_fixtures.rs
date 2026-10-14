//! Deferred macOS-only failure mapping; no fabricated pending native stream.
use super::*;
use crate::gameplay_presentation_port_fixtures::{
    device, AudioCommand, DeviceId, SampleId, Timestamp, VoiceId,
};
#[test]
fn recovered_coreaudio_mapping_preserves_original_and_cleanup_errors_and_actual_paused_mixer() {
    let (output, mut producer) = device(false, vec![DeviceId(u64::MAX)]);
    let mut mixer = output.mixer;
    producer.request_pause(true);
    mixer.render(&mut [0.; 3]).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(u64::MAX),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.,
        })
        .unwrap();
    let basis = mixer.output_frame_basis();
    let counters = mixer.counters();
    let input: CoreAudioOpenFailure = OutputOpenFailure::recovered(
        CoreAudioError::Native {
            operation: "original registration",
            code: i32::MIN,
        },
        Some(mixer),
    )
    .with_cleanup_error(CoreAudioError::Native {
        operation: "original unregister",
        code: i32::MAX,
    });
    let mapped = map_open_failure(input, u64::MAX);
    assert!(matches!(
        mapped.error(),
        CoreAudioReplacementError::Native(CoreAudioError::Native {
            operation: "original registration",
            code: i32::MIN
        })
    ));
    assert!(matches!(
        mapped.cleanup_error(),
        Some(CoreAudioReplacementError::Native(CoreAudioError::Native {
            operation: "original unregister",
            code: i32::MAX
        }))
    ));
    assert!(mapped.pending_owner().is_none());
    assert_eq!(mapped.mixer().unwrap().output_frame_basis(), basis);
    assert_eq!(mapped.mixer().unwrap().counters(), counters);
    let (error, mixer, pending, cleanup) = mapped.into_parts();
    assert!(matches!(
        error,
        CoreAudioReplacementError::Native(CoreAudioError::Native { code: i32::MIN, .. })
    ));
    assert!(matches!(
        cleanup,
        Some(CoreAudioReplacementError::Native(CoreAudioError::Native {
            code: i32::MAX,
            ..
        }))
    ));
    assert!(pending.is_none());
    let mut mixer = mixer.unwrap();
    producer.request_pause(false);
    let mut pcm = [0.; 2];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.5]);
}
#[test]
fn unavailable_coreaudio_mapping_retains_cleanup_diagnostic_without_constructing_replacement_owner()
{
    let input: CoreAudioOpenFailure =
        OutputOpenFailure::recovered(CoreAudioError::ConfigurationChanged, None)
            .with_cleanup_error(CoreAudioError::RecoveryUnavailable);
    let mapped = map_open_failure(input, 7);
    assert!(mapped.mixer().is_none());
    assert!(mapped.pending_owner().is_none());
    assert!(matches!(
        mapped.error(),
        CoreAudioReplacementError::Native(CoreAudioError::ConfigurationChanged)
    ));
    assert!(matches!(
        mapped.cleanup_error(),
        Some(CoreAudioReplacementError::Native(
            CoreAudioError::RecoveryUnavailable
        ))
    ));
    let (_, mixer, pending, cleanup) = mapped.into_parts();
    assert!(mixer.is_none());
    assert!(pending.is_none());
    assert!(cleanup.is_some());
}

// These fixtures exercise complete software ownership through the actual
// adapter. They never open a HAL device: every open request below must refuse
// during validation. On the Linux coordinator these macOS fixtures are typed
// only; native execution and live-stream forwarding remain separate evidence.
mod complete {
    use super::*;
    use crate::gameplay::output::{
        adapters::remix::RemixedOutputBackend,
        ports::{
            ConditionalSourceRetirementNativeOutputBackend, OutputChannelRemixBackend,
            SourceHistoryRetirementNativeOutputBackend, SourcePlaybackNativeOutputBackend,
        },
    };
    use beatkernel::audio::{
        command_queue, AudioCounters, AudioFormat, AudioLimits, ChannelMatrix, CommandProducer,
        MixerConfig, PcmLimits, PcmSample, SampleBank, SourcePlaybackSpan,
    };
    use beatkernel_platform::{
        audio::{DeviceFormat, NativeOutputState, SampleEncoding},
        macos::audio::{CoreAudioStateOpenFailure, SourceRetirementTicket},
    };

    fn matrix() -> ChannelMatrix {
        ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()
    }

    fn owner() -> (CommandProducer, NativeOutputState) {
        let format = AudioFormat::new(1000, 1).unwrap();
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
        let mut first = [0.; 5];
        mixer.render(&mut first).unwrap();
        assert_eq!(first, [0.25; 5]);
        mixer.enable_source_playback_history(16).unwrap();
        let mut state = match NativeOutputState::new(
            mixer,
            DeviceFormat::new(1000, 2, SampleEncoding::Float32, None).unwrap(),
            Some(matrix()),
            8,
        ) {
            Ok(state) => state,
            Err(_) => panic!("valid complete owner"),
        };
        let report = state.render_pending(6).unwrap();
        assert_eq!((report.start_frame, report.frames), (5, 6));
        state.admit(2).unwrap();
        assert_eq!(state.output_frame_basis().start_physical_frame(), 7);
        assert_eq!(
            state.mixer().output_frame_basis().start_physical_frame(),
            11
        );
        assert_eq!(state.pending_samples(), &[0.25, -0.125].repeat(4));
        (producer, state)
    }

    #[derive(Debug, PartialEq)]
    struct OwnerFacts {
        first_unsent: OutputFrameBasis,
        pulled_basis: OutputFrameBasis,
        counters: AudioCounters,
        format: Option<DeviceFormat>,
        capacity: usize,
        report: Option<RenderReport>,
        admitted: usize,
        pcm_address: usize,
        pcm: Vec<f32>,
        history_identity: (u32, u64, u64),
        history_end: u64,
        history_capacity: usize,
        history_revision: u64,
        spans: Vec<SourcePlaybackSpan>,
        uncertain: bool,
    }

    fn facts(state: &NativeOutputState) -> OwnerFacts {
        let history = state.mixer().source_playback_history().unwrap();
        OwnerFacts {
            first_unsent: state.output_frame_basis(),
            pulled_basis: state.mixer().output_frame_basis(),
            counters: state.mixer().counters(),
            format: state.format(),
            capacity: state.max_frames(),
            report: state.pending_report(),
            admitted: state.admitted_frames(),
            pcm_address: state.pending_samples().as_ptr() as usize,
            pcm: state.pending_samples().to_vec(),
            history_identity: (
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
            uncertain: state.admission_is_uncertain(),
        }
    }

    fn backend() -> CoreAudioCompleteReplacementBackend {
        let clock = MachClock::new(ClockDomainId(8), ClockDomainId(9)).unwrap();
        CoreAudioCompleteReplacementBackend::new(clock, ClockDomainId(9), 8)
    }

    fn recovered(
        failure: OutputOpenFailure<
            CoreAudioReplacementError,
            CoreAudioCompleteReplacementOutput,
            NativeOutputState,
        >,
    ) -> NativeOutputState {
        let (_, state, pending, _) = failure.into_parts();
        assert!(
            pending.is_none(),
            "cold validation cannot create a native owner"
        );
        state.expect("cold refusal must return the complete software owner")
    }

    #[test]
    fn recovered_state_failure_preserves_original_diagnostics_pcm_history_and_matrix() {
        let (mut producer, state) = owner();
        let before = facts(&state);
        let input: CoreAudioStateOpenFailure = OutputOpenFailure::recovered_state(
            CoreAudioError::Native {
                operation: "original complete registration",
                code: i32::MIN,
            },
            Some(state),
        )
        .with_cleanup_error(CoreAudioError::Native {
            operation: "original complete unregister",
            code: i32::MAX,
        });
        let mapped = map_state_open_failure(input, u64::MAX, 32, Some(matrix()));
        assert!(matches!(
            mapped.error(),
            CoreAudioReplacementError::Native(CoreAudioError::Native {
                operation: "original complete registration",
                code: i32::MIN
            })
        ));
        assert!(matches!(
            mapped.cleanup_error(),
            Some(CoreAudioReplacementError::Native(CoreAudioError::Native {
                operation: "original complete unregister",
                code: i32::MAX
            }))
        ));
        let mut state = recovered(mapped);
        assert_eq!(facts(&state), before);
        // The original producer and voice are still live. Draining the exact
        // suffix and rendering again also observes the original remix gains.
        state.admit(4).unwrap();
        producer.request_pause(true);
        assert!(state.render_pending(2).unwrap().paused);
        assert_eq!(state.pending_samples(), [0.; 4]);
        state.admit(2).unwrap();
        producer.request_pause(false);
        assert!(!state.render_pending(2).unwrap().paused);
        assert_eq!(state.pending_samples(), [0.25, -0.125, 0.25, -0.125]);
    }

    #[test]
    fn unavailable_complete_failure_does_not_invent_a_mixer_or_native_owner() {
        let input: CoreAudioStateOpenFailure =
            OutputOpenFailure::recovered_state(CoreAudioError::ConfigurationChanged, None)
                .with_cleanup_error(CoreAudioError::RecoveryUnavailable);
        let mapped = map_state_open_failure(input, 19, 32, Some(matrix()));
        let (error, state, pending, cleanup) = mapped.into_parts();
        assert!(matches!(
            error,
            CoreAudioReplacementError::Native(CoreAudioError::ConfigurationChanged)
        ));
        assert!(matches!(
            cleanup,
            Some(CoreAudioReplacementError::Native(
                CoreAudioError::RecoveryUnavailable
            ))
        ));
        assert!(state.is_none());
        assert!(pending.is_none());
    }

    #[test]
    fn real_complete_backend_cold_refusals_return_the_entire_unmodified_owner() {
        let mut backend = backend();
        for (device, rate, channels, frames, expected_capacity) in [
            (0, 1000, 2, 4, false),
            (1, 1000, 2, 0, false),
            (1, 999, 2, 4, false),
            (1, 1000, 1, 4, false),
            (1, 1000, 2, 33, true),
        ] {
            let (_, state) = owner();
            let before = facts(&state);
            let request = CoreAudioRequest {
                device,
                format: AudioFormat::new(rate, channels).unwrap(),
                buffer_frames: frames,
            };
            let failure = match backend.open_remixed(request, state, u64::MAX, matrix()) {
                Ok(_) => panic!("invalid request must refuse before HAL setup"),
                Err(failure) => failure,
            };
            assert!(
                matches!(failure.error(), CoreAudioReplacementError::Native(error)
                if if expected_capacity { matches!(error, CoreAudioError::Capacity) }
                else { matches!(error, CoreAudioError::InvalidRequest) })
            );
            assert!(failure.cleanup_error().is_none());
            assert_eq!(facts(&recovered(failure)), before);
        }
    }

    #[test]
    fn changed_pending_interpretation_and_plain_open_refuse_before_native_acquisition() {
        let mut backend = backend();
        let request = CoreAudioRequest {
            device: 1,
            format: AudioFormat::new(1000, 2).unwrap(),
            buffer_frames: 4,
        };
        let (_, state) = owner();
        let before = facts(&state);
        let changed = ChannelMatrix::new(1, 2, &[-0.25, 1.]).unwrap();
        let failure = match backend.open_remixed(request, state, 11, changed) {
            Ok(_) => panic!("a pending remix change must refuse before HAL setup"),
            Err(failure) => failure,
        };
        assert!(matches!(
            failure.error(),
            CoreAudioReplacementError::Native(CoreAudioError::InvalidRequest)
        ));
        let state = recovered(failure);
        assert_eq!(facts(&state), before);
        // Omitting the required mono-to-stereo interpretation is also a cold
        // refusal; it cannot flatten the complete owner into its pulled Mixer.
        let failure = match backend.open(request, state, 12) {
            Ok(_) => panic!("plain open cannot discard the retained channel matrix"),
            Err(failure) => failure,
        };
        assert!(matches!(
            failure.error(),
            CoreAudioReplacementError::Native(CoreAudioError::InvalidRequest)
        ));
        assert_eq!(facts(&recovered(failure)), before);
    }

    #[test]
    fn quarantined_complete_owner_cannot_be_reopened_or_replayed_by_the_backend() {
        let (_, mut state) = owner();
        state.mark_admission_uncertain();
        let before = facts(&state);
        let request = CoreAudioRequest {
            device: 1,
            format: AudioFormat::new(1000, 2).unwrap(),
            buffer_frames: 4,
        };
        let failure = match backend().open_remixed(request, state, 31, matrix()) {
            Ok(_) => panic!("uncertain native admission cannot reopen"),
            Err(failure) => failure,
        };
        let mut state = recovered(failure);
        assert_eq!(facts(&state), before);
        assert!(state.admit(1).is_err());
        assert!(state.render_pending(1).is_err());
        assert_eq!(facts(&state), before);
    }

    #[test]
    fn complete_backend_and_remix_keep_the_complete_owner_and_original_ticket_contract() {
        fn accepts_existing_source_ports<B>()
        where
            B: OutputChannelRemixBackend<NativeOutputState>
                + SourceHistoryRetirementNativeOutputBackend<NativeOutputState>
                + SourcePlaybackNativeOutputBackend<
                    NativeOutputState,
                    NativeEvidence = CoreAudioSourcePlaybackSnapshot,
                > + ConditionalSourceRetirementNativeOutputBackend<
                    NativeOutputState,
                    Ticket = SourceRetirementTicket,
                >,
        {
        }
        accepts_existing_source_ports::<CoreAudioCompleteReplacementBackend>();
        // The existing remix adapter intentionally exposes lifecycle/source
        // ports, rather than itself promising another nested remix layer.
        fn accepts_remixed_source_ports<B>()
        where
            B: SourceHistoryRetirementNativeOutputBackend<NativeOutputState>
                + ConditionalSourceRetirementNativeOutputBackend<
                    NativeOutputState,
                    Ticket = SourceRetirementTicket,
                >,
        {
        }
        accepts_remixed_source_ports::<RemixedOutputBackend<CoreAudioCompleteReplacementBackend>>();
    }

    #[test]
    fn callback_packet_and_epoch_errors_are_not_raw_native_io_permission() {
        use crate::gameplay::output::ports::SourcePlaybackPacketError;
        let backend = backend();
        for error in [
            CoreAudioReplacementError::CallbackFailure(1),
            CoreAudioReplacementError::CallbackFailure(u64::MAX),
            CoreAudioReplacementError::EpochMismatch,
            CoreAudioReplacementError::Playback(SourcePlaybackPacketError::BasisMismatch),
            CoreAudioReplacementError::Playback(SourcePlaybackPacketError::InvalidPrefix),
            CoreAudioReplacementError::Native(CoreAudioError::ConfigurationChanged),
            CoreAudioReplacementError::Native(CoreAudioError::InvalidRequest),
        ] {
            assert!(!backend.is_source_native_io_error(&error), "{error}");
        }
    }
}
