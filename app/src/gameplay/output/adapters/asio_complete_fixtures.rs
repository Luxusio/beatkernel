//! Actual complete-state adapter mapping and static Source contracts.
//! No trusted opener, native stream, SDK or playback authority is fabricated.
//! These ASIO-gated fixtures are typed only by the Linux coordinator.
use super::*;
use crate::gameplay::output::{
    adapters::remix::RemixedOutputBackend,
    ports::{
        ConditionalSourceRetirementNativeOutputBackend, OriginalCoordinateNativeOutputBackend,
        OutputChannelRemixBackend, SourceHistoryRetirementNativeOutputBackend,
        SourcePlaybackNativeOutputBackend, SourcePlaybackPacketError,
    },
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioCounters, AudioFormat, AudioLimits, ChannelMatrix,
        CommandProducer, MixerConfig, PcmLimits, PcmSample, SampleBank, SampleId,
        SourcePlaybackSpan, VoiceId,
    },
    time::{ClockDomainId, Timestamp},
};
use beatkernel_platform::{
    audio::{asio::AsioPcmEncoding, AsioPlanarLayout, NativeOutputLayout, NativeOutputState},
    windows::asio::{
        control::{AsioControlError, AsioControlErrorDomain},
        stream::{
            AsioDiagnostics, AsioPlaybackPhaseSnapshot, AsioStatePrepareFailure,
            SourceRetirementTicket,
        },
    },
};
use std::{error::Error, sync::Arc};

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
    let mut prefix = [0.; 5];
    mixer.render(&mut prefix).unwrap();
    assert_eq!(prefix, [0.25; 5]);
    mixer.enable_source_playback_history(16).unwrap();
    let mut state = NativeOutputState::from_mixer(mixer);
    state
        .reconfigure_asio(
            AsioPlanarLayout::new(
                AudioFormat::new(1000, 2).unwrap(),
                &[
                    (2, AsioPcmEncoding::Int16Lsb),
                    (5, AsioPcmEncoding::Float32Msb),
                ],
            )
            .unwrap(),
            Some(matrix()),
            8,
        )
        .unwrap();
    let report = state.render_pending(6).unwrap();
    assert_eq!((report.start_frame, report.frames), (5, 6));
    state.admit(2).unwrap();
    assert_eq!(state.output_frame_basis().start_physical_frame(), 7);
    assert_eq!(state.pending_samples(), &[0.25, -0.125].repeat(4));
    (producer, state)
}

#[derive(Debug, PartialEq)]
struct OwnerFacts {
    first_unsent: OutputFrameBasis,
    pulled_basis: OutputFrameBasis,
    counters: AudioCounters,
    layout: Option<NativeOutputLayout>,
    matrix: Option<(u16, u16, Vec<u32>)>,
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
        layout: state.layout().copied(),
        matrix: state.channel_matrix().map(|matrix| {
            (
                matrix.source_channels(),
                matrix.target_channels(),
                matrix
                    .coefficients()
                    .iter()
                    .map(|gain| gain.to_bits())
                    .collect(),
            )
        }),
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

fn host(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(8),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn anchor() -> MultimediaClockAnchor {
    MultimediaClockAnchor::new(123, host(100), host(113), 1_000_000, 17, 29).unwrap()
}
fn backend() -> AsioCompleteReplacementBackend {
    let clock = QpcClock::new(ClockDomainId(8)).unwrap();
    AsioCompleteReplacementBackend::new(clock, anchor(), 31, ClockDomainId(8), 8)
}
fn recovered(
    failure: OutputOpenFailure<
        AsioReplacementError,
        AsioCompleteReplacementOutput,
        NativeOutputState,
    >,
) -> NativeOutputState {
    let (_, state, pending, _) = failure.into_parts();
    assert!(
        pending.is_none(),
        "mapping cannot invent a pending native stream"
    );
    state.expect("the original complete software state must be returned")
}

#[test]
fn driver_open_failure_returns_original_planar_pcm_remix_history_and_live_producer() {
    let (mut producer, state) = owner();
    let before = facts(&state);
    let mapped = map_state_driver_open_error(
        AsioControlError::Native {
            operation: "original complete driver open",
            domain: AsioControlErrorDomain::Com,
            code: i32::MIN,
        },
        state,
    );
    assert!(matches!(
        mapped.error(),
        AsioReplacementError::Native(AsioStreamError::Control(AsioControlError::Native {
            operation: "original complete driver open",
            domain: AsioControlErrorDomain::Com,
            code: i32::MIN
        }))
    ));
    assert!(mapped.cleanup_error().is_none());
    let mut state = recovered(mapped);
    assert_eq!(facts(&state), before);
    assert!(
        state.format().is_none(),
        "heterogeneous planes are not a DeviceFormat"
    );
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
fn complete_prepare_failure_keeps_primary_latest_cleanup_allocation_and_exact_owner() {
    let (_, state) = owner();
    let before = facts(&state);
    let cleanup = Arc::new(AsioControlError::Native {
        operation: "original latest complete close",
        domain: AsioControlErrorDomain::Bridge,
        code: i32::MAX,
    });
    let input: AsioStatePrepareFailure = OutputOpenFailure::recovered_state(
        AsioStreamError::RateMismatch {
            expected: 1000,
            actual: 999.5,
        },
        Some(state),
    )
    .with_cleanup_error(AsioStreamError::Cleanup(Arc::clone(&cleanup)));
    let mapped = map_state_prepare_failure(input, u64::MAX, anchor(), 31);
    assert!(
        matches!(mapped.error(), AsioReplacementError::Native(AsioStreamError::RateMismatch {
        expected: 1000, actual
    }) if *actual == 999.5)
    );
    let Some(AsioReplacementError::Native(AsioStreamError::Cleanup(retained))) =
        mapped.cleanup_error()
    else {
        panic!("the latest typed cleanup diagnostic must survive mapping");
    };
    assert!(Arc::ptr_eq(retained, &cleanup));
    assert_eq!(facts(&recovered(mapped)), before);
}

#[test]
fn unavailable_complete_prepare_failure_invents_neither_mixer_nor_pending_stream() {
    let input: AsioStatePrepareFailure =
        OutputOpenFailure::recovered_state(AsioStreamError::InvalidChannels, None)
            .with_cleanup_error(AsioStreamError::RecoveryUnavailable);
    let mapped = map_state_prepare_failure(input, 19, anchor(), 31);
    let (error, state, pending, cleanup) = mapped.into_parts();
    assert!(matches!(
        error,
        AsioReplacementError::Native(AsioStreamError::InvalidChannels)
    ));
    assert!(matches!(
        cleanup,
        Some(AsioReplacementError::Native(
            AsioStreamError::RecoveryUnavailable
        ))
    ));
    assert!(state.is_none());
    assert!(pending.is_none());
}

#[test]
fn failure_mapping_never_clears_uncertain_native_admission_or_replays_diagnostic_suffix() {
    let (_, mut state) = owner();
    state.mark_admission_uncertain();
    let before = facts(&state);
    let input: AsioStatePrepareFailure =
        OutputOpenFailure::recovered_state(AsioStreamError::RecoveryUnavailable, Some(state));
    let mut state = recovered(map_state_prepare_failure(input, 23, anchor(), 31));
    assert_eq!(facts(&state), before);
    assert!(state.admit(1).is_err());
    assert!(state.render_pending(1).is_err());
    assert_eq!(facts(&state), before);
}

#[test]
fn complete_and_remixed_backends_keep_exact_source_state_evidence_and_native_ticket_contracts() {
    fn accepts_complete<B>()
    where
        B: OutputChannelRemixBackend<NativeOutputState>
            + OriginalCoordinateNativeOutputBackend<NativeOutputState>
            + SourceHistoryRetirementNativeOutputBackend<NativeOutputState>
            + SourcePlaybackNativeOutputBackend<
                NativeOutputState,
                NativeEvidence = AsioPlaybackPhaseSnapshot,
            > + ConditionalSourceRetirementNativeOutputBackend<
                NativeOutputState,
                Ticket = SourceRetirementTicket,
            >,
        B::Output: StoppedMixerSource<NativeOutputState, Error = AsioReplacementError>,
    {
    }
    fn accepts_remixed<B>()
    where
        B: OriginalCoordinateNativeOutputBackend<NativeOutputState>
            + SourceHistoryRetirementNativeOutputBackend<NativeOutputState>
            + SourcePlaybackNativeOutputBackend<
                NativeOutputState,
                NativeEvidence = AsioPlaybackPhaseSnapshot,
            > + ConditionalSourceRetirementNativeOutputBackend<
                NativeOutputState,
                Ticket = SourceRetirementTicket,
            >,
        B::Output: StoppedMixerSource<NativeOutputState, Error = AsioReplacementError>,
    {
    }
    accepts_complete::<AsioCompleteReplacementBackend>();
    accepts_remixed::<RemixedOutputBackend<AsioCompleteReplacementBackend>>();
}

#[test]
fn only_exact_recovery_unavailable_permits_the_existing_owner_authorization_reprobe() {
    let backend = backend();
    let remixed = RemixedOutputBackend::new(self::backend());
    let error = AsioReplacementError::Native(AsioStreamError::RecoveryUnavailable);
    assert!(backend.is_source_native_io_error(&error));
    assert!(remixed.is_source_native_io_error(&error));
    assert!(matches!(
        error.source().unwrap().downcast_ref::<AsioStreamError>(),
        Some(AsioStreamError::RecoveryUnavailable)
    ));
}

#[test]
fn unrelated_native_cleanup_config_and_render_failures_never_become_reprobe_permission() {
    let backend = backend();
    let remixed = RemixedOutputBackend::new(self::backend());
    for native in [
        AsioStreamError::Control(AsioControlError::Native {
            operation: "unrelated driver failure",
            domain: AsioControlErrorDomain::Asio,
            code: -777,
        }),
        AsioStreamError::Cleanup(Arc::new(AsioControlError::Native {
            operation: "unrelated release failure",
            domain: AsioControlErrorDomain::Bridge,
            code: -778,
        })),
        AsioStreamError::InvalidChannels,
        AsioStreamError::InvalidState,
        AsioStreamError::RateMismatch {
            expected: 1000,
            actual: 999.5,
        },
        AsioStreamError::Render(beatkernel_platform::audio::asio::AsioRenderError::Capacity),
        AsioStreamError::ReopenRequired(AsioDiagnostics::default()),
    ] {
        let error = AsioReplacementError::Native(native);
        let original = error.to_string();
        assert!(!backend.is_source_native_io_error(&error), "{error}");
        assert!(!remixed.is_source_native_io_error(&error), "{error}");
        assert_eq!(error.to_string(), original);
        assert!(error
            .source()
            .unwrap()
            .downcast_ref::<AsioStreamError>()
            .is_some());
    }
}

#[test]
fn packet_coordinate_host_status_and_timing_errors_keep_their_original_types_through_remix() {
    use crate::native_output_coordinate::NativeOutputCoordinateError;
    let backend = backend();
    let remixed = RemixedOutputBackend::new(self::backend());
    for error in [
        AsioReplacementError::Playback(SourcePlaybackPacketError::InvalidIdentity),
        AsioReplacementError::Playback(SourcePlaybackPacketError::BasisMismatch),
        AsioReplacementError::Playback(SourcePlaybackPacketError::EpochMismatch),
        AsioReplacementError::Playback(SourcePlaybackPacketError::InvalidRevision),
        AsioReplacementError::Playback(SourcePlaybackPacketError::InvalidPrefix),
        AsioReplacementError::Playback(SourcePlaybackPacketError::InvalidSpan),
        AsioReplacementError::Playback(SourcePlaybackPacketError::InvalidReport),
        AsioReplacementError::Playback(SourcePlaybackPacketError::QueryOutsidePrefix),
        AsioReplacementError::Playback(SourcePlaybackPacketError::QueryOutsideSpan),
        AsioReplacementError::Playback(SourcePlaybackPacketError::MissingNativeEvidence),
        AsioReplacementError::Playback(SourcePlaybackPacketError::CoordinateMismatch),
        AsioReplacementError::Playback(SourcePlaybackPacketError::Overflow),
        AsioReplacementError::Coordinate(NativeOutputCoordinateError::BasisMismatch),
        AsioReplacementError::Coordinate(NativeOutputCoordinateError::Overflow),
        AsioReplacementError::Host(std::io::Error::from_raw_os_error(5)),
        AsioReplacementError::Status(AsioStreamPhase::Stopped),
        AsioReplacementError::Presentation(AsioPresentationError::Unavailable),
        AsioReplacementError::Presentation(AsioPresentationError::Malformed),
        AsioReplacementError::Presentation(AsioPresentationError::Overflow),
        AsioReplacementError::Presentation(AsioPresentationError::Clock(
            beatkernel_platform::audio::asio::MultimediaClockError::Expired,
        )),
        AsioReplacementError::Observation(ReplacementObservationError::EpochMismatch),
    ] {
        let original = error.to_string();
        assert!(!backend.is_source_native_io_error(&error), "{error}");
        assert!(!remixed.is_source_native_io_error(&error), "{error}");
        assert_eq!(error.to_string(), original);
    }
}
