//! Actual portable Windows planning and retained software ownership.
//! Supplied HOST intervals check geometry only; they do not grant native authority.
use super::{output_settings as settings, AsioView, Backend};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, ChannelMatrix, CommandProducer,
        Mixer, MixerConfig, PcmLimits, PcmSample, SampleBank, SampleId, SourcePlaybackSpan,
        VoiceId,
    },
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use beatkernel_bms_runtime::gameplay::output::{
    domain::control::{OutputCapability, OutputControls, OutputReply},
    ports::SourcePlaybackIdentity,
};
use beatkernel_platform::audio::{
    asio::{
        encode_asio_channel, AsioBufferRequest, AsioPcmEncoding, AsioPresentationObservation,
        MultimediaHostInterval,
    },
    AsioPlanarLayout, AudioStreamMode, NativeOutputLayout, NativeOutputState, SharedPeriodPolicy,
};

const DRIVER: &str = "{12345678-9ABC-DEF0-1234-56789ABCDEF0}";
const OTHER_DRIVER: &str = "{11111111-2222-3333-4444-555555555555}";
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
fn source() -> AudioFormat {
    AudioFormat::new(48_000, 2).unwrap()
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn bounds() -> settings::AsioClockBounds {
    settings::AsioClockBounds {
        timer: 11,
        drift: 22,
        latency: 33,
        age: 1_000_000_000,
    }
}
fn capability(
    channels: &[u32],
    matrix: Option<&ChannelMatrix>,
    clock: settings::AsioClockBounds,
) -> OutputCapability {
    settings::switch_capability(
        settings::asio_clock_capability(
            settings::asio_capability(DRIVER, AsioBufferRequest::Frames(4), channels, matrix)
                .unwrap(),
            clock,
        )
        .unwrap(),
        Backend::Asio,
        Some(AsioView::Bits64),
    )
    .unwrap()
}
fn plan(
    current: &OutputCapability,
    matrix: Option<&ChannelMatrix>,
    args: &[String],
) -> Result<settings::AsioTargetPlan, String> {
    super::plan_retained_asio_source_output(current, source(), matrix, args, true)
}
fn layout() -> AsioPlanarLayout {
    AsioPlanarLayout::new(
        source(),
        &[
            (7, AsioPcmEncoding::Float32Lsb),
            (3, AsioPcmEncoding::Int16Msb),
        ],
    )
    .unwrap()
}
fn mixer() -> (CommandProducer, Mixer) {
    let pcm = PcmLimits::new(64, 512, 1).unwrap();
    let mut bank = SampleBank::new(source(), pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            source(),
            vec![
                0.125, -0.25, 0.375, -0.5, 0.625, -0.75, 0.875, -1.0, 0.25, -0.125, 0.5, -0.375,
                0.75, -0.625, 1.0, -0.875,
            ],
            pcm,
        )
        .unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            source(),
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(16, 4, 16, AudioLimits::MAX_RENDER_FRAMES, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(16).unwrap();
    (producer, mixer)
}
fn prepared() -> (CommandProducer, NativeOutputState) {
    let (producer, mixer) = mixer();
    let mut state = NativeOutputState::from_mixer(mixer);
    state.reconfigure_asio(layout(), None, 4).unwrap();
    (producer, state)
}
fn history(
    state: &NativeOutputState,
) -> (SourcePlaybackIdentity, u64, u64, Vec<SourcePlaybackSpan>) {
    let history = state.mixer().source_playback_history().unwrap();
    (
        SourcePlaybackIdentity::new(
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
        )
        .unwrap(),
        history.pulled_end_frame(),
        history.retirement_revision(),
        (0..history.len())
            .map(|index| history.span(index).unwrap())
            .collect(),
    )
}

#[test]
fn retained_asio_same_family_plan_keeps_driver_channel_order_width_and_source_rate() {
    let cap = capability(&[7, 3], None, bounds());
    let original = cap.clone();
    for args in [Vec::new(), cap.current_args.clone()] {
        let requested = plan(&cap, None, &args).unwrap();
        assert_eq!(requested.device, DRIVER);
        assert_eq!(requested.view, AsioView::Bits64);
        assert_eq!(requested.channels, [7, 3]);
        assert_eq!(requested.buffer, AsioBufferRequest::Frames(4));
        assert_eq!(requested.bounds, bounds());
        assert_eq!(requested.sample_rate, 48_000);
        assert!(requested.matrix.is_none());
    }
    assert_eq!(cap, original);
}

#[test]
fn retained_asio_driver_buffer_and_channel_changes_use_the_real_source_matrix() {
    let cap = capability(&[7, 3], None, bounds());
    let requested = plan(
        &cap,
        None,
        &strings(&[
            "--device",
            OTHER_DRIVER,
            "--buffer",
            "frames:8",
            "--output-channels",
            "9,2,5",
            "--output-matrix",
            "1,0;0,1;0.5,0.25",
        ]),
    )
    .unwrap();
    let expected = ChannelMatrix::new(2, 3, &[1., 0., 0., 1., 0.5, 0.25]).unwrap();
    assert_eq!(requested.device, OTHER_DRIVER);
    assert_eq!(requested.channels, [9, 2, 5]);
    assert_eq!(requested.buffer, AsioBufferRequest::Frames(8));
    assert_eq!(requested.matrix.as_ref(), Some(&expected));
    assert_eq!(requested.sample_rate, source().sample_rate());
    let next_cap = capability(
        &requested.channels,
        requested.matrix.as_ref(),
        requested.bounds,
    );
    let next = plan(
        &next_cap,
        requested.matrix.as_ref(),
        &strings(&["--buffer", "default"]),
    )
    .unwrap();
    assert_eq!(next.buffer, AsioBufferRequest::DriverPreferred);
    assert_eq!(next.channels, [9, 2, 5]);
    assert_eq!(next.matrix, Some(expected));
    assert!(plan(
        &next_cap,
        requested.matrix.as_ref(),
        &strings(&["--output-matrix", "exact"])
    )
    .is_err());
    let reset = plan(
        &next_cap,
        requested.matrix.as_ref(),
        &strings(&["--output-channels", "2,9", "--output-matrix", "exact"]),
    )
    .unwrap();
    assert_eq!(reset.channels, [2, 9]);
    assert!(reset.matrix.is_none());
}

#[test]
fn retained_asio_per_output_clock_bounds_do_not_inherit_other_member_drafts() {
    let first = capability(&[7, 3], None, bounds());
    let second_bounds = settings::AsioClockBounds {
        timer: 101,
        drift: 202,
        latency: 303,
        age: 2_000_000_000,
    };
    let second = capability(&[11, 8], None, second_bounds);
    let first_copy = first.clone();
    let second_copy = second.clone();
    let changed = plan(
        &first,
        None,
        &strings(&[
            "--asio-timer-error-ns",
            "0",
            "--asio-anchor-age-ns",
            "1000000",
        ]),
    )
    .unwrap();
    assert_eq!(
        changed.bounds,
        settings::AsioClockBounds {
            timer: 0,
            age: 1_000_000,
            ..bounds()
        }
    );
    assert_eq!(plan(&second, None, &[]).unwrap().bounds, second_bounds);
    assert_eq!(plan(&second, None, &[]).unwrap().channels, [11, 8]);
    assert_eq!(plan(&first, None, &[]).unwrap().bounds, bounds());
    assert_eq!(first, first_copy);
    assert_eq!(second, second_copy);
}

#[test]
fn retained_asio_invalid_geometry_or_source_family_refuses_before_owner_mutation() {
    let cap = capability(&[7, 3], None, bounds());
    let cap_copy = cap.clone();
    let (_producer, mut state) = prepared();
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let report = state.pending_report();
    let pcm = state.pending_samples().to_vec();
    let basis = state.output_frame_basis();
    let original_history = history(&state);
    for args in [
        strings(&["--backend", "wasapi"]),
        strings(&["--rate", "44100"]),
        strings(&["--channels", "1"]),
        strings(&["--encoding", "float64"]),
        strings(&["--buffer", "frames:0"]),
        strings(&["--buffer", "ns:1000000"]),
        strings(&[
            "--buffer",
            &format!("frames:{}", AudioLimits::MAX_RENDER_FRAMES + 1),
        ]),
        strings(&["--output-channels", "7,7"]),
        strings(&["--output-channels", "2147483648,3"]),
        strings(&["--output-channels", "7"]),
        strings(&["--output-matrix", "1,0,0;0,1,0"]),
        strings(&["--asio-anchor-age-ns", "0"]),
        strings(&["--asio-latency-error-ns", "-1"]),
    ] {
        assert!(plan(&cap, None, &args).is_err(), "draft {args:?}");
        assert_eq!(cap, cap_copy);
        assert_eq!(state.pending_report(), report);
        assert_eq!(state.pending_samples(), pcm);
        assert_eq!(state.output_frame_basis(), basis);
        assert_eq!(history(&state), original_history);
    }
    let mut wasapi = cap.clone();
    let backend = wasapi
        .current_args
        .iter()
        .position(|arg| arg == "--backend")
        .unwrap();
    wasapi.current_args[backend + 1] = "wasapi".into();
    assert!(plan(&wasapi, None, &strings(&["--backend", "asio"])).is_err());
}

#[test]
fn retained_asio_cold_resize_keeps_nonzero_original_pcm_report_history_and_identity() {
    let (_producer, mut state) = prepared();
    let report = state.render_pending(4).unwrap();
    assert_eq!(
        state.pending_samples(),
        [0.125, -0.25, 0.375, -0.5, 0.625, -0.75, 0.875, -1.]
    );
    state.admit(1).unwrap();
    let samples = state.pending_samples().to_vec();
    let old_history = history(&state);
    let basis = state.output_frame_basis();
    let config = state.mixer().config();
    assert_eq!(
        config.limits().max_render_frames(),
        AudioLimits::MAX_RENDER_FRAMES
    );
    assert_eq!(basis.start_physical_frame(), 1);
    assert_eq!(state.mixer().frame_cursor(), 4);
    for frames in [8, 1, 16] {
        state
            .validate_reconfigure_asio(&layout(), None, frames)
            .unwrap();
        state.reconfigure_asio(layout(), None, frames).unwrap();
        assert_eq!(state.max_frames(), frames);
        assert_eq!(state.mixer().config(), config);
        assert_eq!(
            state.layout(),
            Some(&NativeOutputLayout::AsioPlanar(layout()))
        );
        assert!(state.format().is_none());
        assert_eq!(state.target_pcm(), Some(source()));
        assert_eq!(state.pending_report(), Some(report));
        assert_eq!(state.pending_samples(), samples);
        assert_eq!(state.admitted_frames(), 1);
        assert_eq!(state.output_frame_basis(), basis);
        assert_eq!(history(&state), old_history);
    }
    assert!(state
        .reconfigure_asio(layout(), None, AudioLimits::MAX_RENDER_FRAMES + 1)
        .is_err());
    assert_eq!(state.max_frames(), 16);
    state.admit(3).unwrap();
    let next = state.render_pending(2).unwrap();
    assert_eq!(next.start_frame, 4);
    assert_eq!(state.pending_samples(), [0.25, -0.125, 0.5, -0.375]);
    assert_eq!(history(&state).0, old_history.0);
    assert_eq!(state.mixer().config(), config);
}

#[test]
fn retained_asio_pending_interpretation_rejects_reorder_encoding_matrix_and_rate_changes() {
    let (_producer, mut state) = prepared();
    state.render_pending(4).unwrap();
    state.admit(2).unwrap();
    let original = (
        state.pending_report(),
        state.output_frame_basis(),
        history(&state),
    );
    let pcm = state.pending_samples().to_vec();
    for next in [
        AsioPlanarLayout::new(
            source(),
            &[
                (3, AsioPcmEncoding::Int16Msb),
                (7, AsioPcmEncoding::Float32Lsb),
            ],
        )
        .unwrap(),
        AsioPlanarLayout::new(
            source(),
            &[
                (7, AsioPcmEncoding::Float32Msb),
                (3, AsioPcmEncoding::Int16Msb),
            ],
        )
        .unwrap(),
        AsioPlanarLayout::new(AudioFormat::new(44_100, 2).unwrap(), layout().channels()).unwrap(),
    ] {
        assert!(state.reconfigure_asio(next, None, 8).is_err());
        assert_eq!(state.pending_samples(), pcm);
        assert_eq!(
            (
                state.pending_report(),
                state.output_frame_basis(),
                history(&state)
            ),
            original
        );
        assert_eq!(
            state.layout(),
            Some(&NativeOutputLayout::AsioPlanar(layout()))
        );
    }
    let matrix = ChannelMatrix::new(2, 2, &[0., 1., 1., 0.]).unwrap();
    assert!(state.reconfigure_asio(layout(), Some(matrix), 8).is_err());
    assert_eq!(state.pending_samples(), pcm);
    assert_eq!(state.admitted_frames(), 2);
    assert_eq!(state.max_frames(), 4);
}

#[test]
fn retained_asio_prefix_uses_nonzero_heterogeneous_pcm_and_explicit_delivery_geometry() {
    use beatkernel_bms_runtime::{
        gameplay::output::adapters::observation::checked_asio_pause_observation,
        native_audio_presentation::NativeAudioSnapshot,
        native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
    };
    use beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence;
    let (_producer, mut state) = prepared();
    let report = state.render_pending(4).unwrap();
    let mut left = [0u8; 8];
    let mut right = [0u8; 4];
    encode_asio_channel(
        AsioPcmEncoding::Float32Lsb,
        &state.pending_samples()[..4],
        2,
        0,
        &mut left,
    )
    .unwrap();
    encode_asio_channel(
        AsioPcmEncoding::Int16Msb,
        &state.pending_samples()[..4],
        2,
        1,
        &mut right,
    )
    .unwrap();
    assert_eq!(
        left.as_slice(),
        [0.125f32.to_le_bytes(), 0.375f32.to_le_bytes()].concat()
    );
    assert_eq!(
        right.as_slice(),
        [(-8192i16).to_be_bytes(), (-16384i16).to_be_bytes()].concat()
    );
    state.admit(2).unwrap();
    let basis = state.output_frame_basis();
    let prefix = AsioPresentationObservation::from_delivery(
        report,
        2,
        1,
        48_000,
        MultimediaHostInterval {
            before: point(1, 1_000_000),
            after: point(1, 1_000_100),
        },
        0,
        0,
        basis.origin(),
    )
    .unwrap();
    let original = OriginalNativePresentationEvidence::Asio {
        observation: prefix,
        basis: Some(basis),
    };
    let coordinate = CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 7,
            basis,
            evidence: original,
        },
        NativeOutputCoordinate::StreamFrame { frame: 0 },
    )
    .unwrap();
    assert_eq!(coordinate.snapshot().evidence, original);
    assert_eq!(prefix.render, report);
    assert_eq!(prefix.delivered_first_frame, 2);
    assert_eq!(prefix.delivered_end_frame().unwrap(), 3);
    assert_eq!(state.pending_report(), Some(report));
    assert!(checked_asio_pause_observation(Some(prefix), point(1, 1_000_100)).is_err());
    let whole =
        AsioPresentationObservation::from_render(report, 48_000, prefix.host, 0, 0, basis.origin())
            .unwrap();
    assert!(checked_asio_pause_observation(Some(whole), point(1, 1_000_100)).is_ok());
}

#[test]
fn retained_asio_remix_renders_actual_original_source_columns_after_cold_plan() {
    let cap = capability(&[7, 3], None, bounds());
    let requested = plan(&cap, None, &strings(&["--output-matrix", "0,1;1,0"])).unwrap();
    let (_producer, mixer) = mixer();
    let mut state = NativeOutputState::from_mixer(mixer);
    state
        .reconfigure_asio(layout(), requested.matrix, 4)
        .unwrap();
    let report = state.render_pending(4).unwrap();
    assert_eq!(report.playback_frames, 4);
    assert_eq!(state.mixer().config().format(), source());
    assert_eq!(
        state.pending_samples(),
        [-0.25, 0.125, -0.5, 0.375, -0.75, 0.625, -1., 0.875]
    );
    assert_eq!(state.channel_matrix().unwrap().source_channels(), 2);
}

#[test]
fn retained_asio_ui_sdk_refusal_is_correlated_once_and_keeps_original_capability_and_pcm() {
    let cap = capability(&[7, 3], None, bounds());
    let mut controls = OutputControls::new();
    controls.advertise(Some(cap.clone())).unwrap();
    let (_producer, mut state) = prepared();
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let original = (
        state.pending_report(),
        state.output_frame_basis(),
        history(&state),
    );
    let pcm = state.pending_samples().to_vec();
    let id = controls
        .request(strings(&["--buffer", "frames:8"]))
        .unwrap();
    assert!(controls.request(Vec::new()).is_err());
    let request = controls.take_request().unwrap();
    assert_eq!(request.id, id);
    assert!(controls.take_request().is_none());
    let error = super::plan_retained_asio_source_output(&cap, source(), None, &request.args, false)
        .err()
        .expect("SDK-disabled replacement must refuse before native open");
    assert!(error.contains("asio-sdk"));
    let reply = OutputReply {
        id,
        result: Err(error),
    };
    assert!(controls
        .reply(&OutputReply {
            id: id + 1,
            result: reply.result.clone()
        })
        .is_err());
    controls.reply(&reply).unwrap();
    assert!(controls.reply(&reply).is_err());
    assert_eq!(controls.capability(), Some(&cap));
    assert_eq!(controls.take_reply(), Some(reply));
    assert!(controls.take_reply().is_none());
    assert!(!controls.pending());
    assert_eq!(state.pending_samples(), pcm);
    assert_eq!(
        (
            state.pending_report(),
            state.output_frame_basis(),
            history(&state)
        ),
        original
    );
    assert!(controls.request(strings(&["--buffer", "default"])).unwrap() > id);
}

#[test]
fn retained_asio_sdk_disabled_keeps_legacy_wasapi_selector_and_portable_asio_parsing() {
    for mode in [
        AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
        AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
        AudioStreamMode::Exclusive,
    ] {
        assert!(super::retained_complete_startup(
            true,
            Backend::Wasapi,
            mode
        ));
        assert!(!super::retained_complete_startup(
            false,
            Backend::Wasapi,
            mode
        ));
        assert!(!super::retained_complete_startup(true, Backend::Asio, mode));
    }
    let mut args = strings(&[
        "--chart",
        "unopened.bms",
        "--bind",
        "11:04",
        "--seconds",
        "2",
    ]);
    args.extend(strings(&[
        "--backend",
        "asio",
        "--device",
        DRIVER,
        "--asio-view",
        "64",
        "--output-channels",
        "7,3",
        "--buffer",
        "frames:4",
        "--asio-system-clock",
        "multimedia",
        "--asio-timer-error-ns",
        "11",
        "--asio-drift-error-ns",
        "22",
        "--asio-latency-error-ns",
        "33",
        "--asio-anchor-age-ns",
        "1000000000",
    ]));
    let parsed = super::parse(&args).unwrap();
    assert_eq!(parsed.backend, Backend::Asio);
    assert_eq!(parsed.output_channels, Some(vec![7, 3]));
    let cap = capability(&[7, 3], None, bounds());
    assert!(super::plan_retained_asio_source_output(&cap, source(), None, &[], false).is_err());
}

fn capture(seed: u64) -> beatkernel_bms_runtime::replay_capture::LiveReplayCapture {
    use beatkernel_bms_runtime::native_judge::{
        capture_limits, prepare_section_capture_for_source, NativeJudgeConfig,
    };
    let source = beatkernel_bms::parse(
        "#BPM 3000\n#WAV01 original.wav\n#00011:00010000\n",
        Default::default(),
    )
    .unwrap();
    let judge = NativeJudgeConfig {
        early: 5,
        late: 7,
        offset: -2,
        preroll: 0,
        output: ClockDomainId(2),
        end: Some(Timestamp::from_nanos(50_000_000)),
    }
    .judge(&source, source.compile().unwrap().chart)
    .unwrap();
    prepare_section_capture_for_source(
        &source,
        &judge,
        ClockDomainId(1),
        Timestamp::ZERO,
        seed,
        Some(Timestamp::from_nanos(50_000_000)),
        capture_limits(true, 8192, 8).unwrap(),
    )
    .unwrap()
    .unwrap()
}

#[test]
fn retained_asio_cold_plan_failures_preserve_pristine_capture_source_and_section() {
    let capture = capture(u64::MAX);
    let header = capture.header().clone();
    let cap = capability(&[7, 3], None, bounds());
    assert!(plan(&cap, None, &strings(&["--output-channels", "0,0"])).is_err());
    assert!(super::plan_retained_asio_source_output(&cap, source(), None, &[], false).is_err());
    assert_eq!(capture.header(), &header);
    assert!(capture.records().is_empty());
    let section =
        beatkernel_bms_runtime::replay_playback::decode_section_setup(&header.options).unwrap();
    assert_eq!(section.chart_seed, u64::MAX);
    assert_eq!(section.start, Timestamp::ZERO);
    assert_eq!(section.end, Some(Timestamp::from_nanos(50_000_000)));
    assert_eq!(header.normalized_clock, ClockDomainId(1));
}

#[test]
fn retained_asio_typed_start_cleanup_and_destination_failures_attempt_every_member_capture() {
    use beatkernel_bms_runtime::{
        competition::ScoreSummary, gauge::BmsGauge, local_players::PlayerId,
        native_cohort::PlayerState,
        native_cohort_setup::finish_cohort_with_results_and_network_and_cleanup,
    };
    use std::{
        error::Error,
        fmt,
        path::{Path, PathBuf},
    };
    #[derive(Debug)]
    struct Fault(&'static str);
    impl fmt::Display for Fault {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }
    impl Error for Fault {}
    let ids = [PlayerId(99), PlayerId(7), PlayerId(u32::MAX)];
    for primary_failed in [false, true] {
        let primary = Box::new(Fault("ASIO start failure marker"));
        let primary_pointer = primary.as_ref() as *const Fault;
        let cleanup = Box::new(Fault("ASIO cleanup failure marker"));
        let cleanup_pointer = cleanup.as_ref() as *const Fault;
        let states = ids
            .iter()
            .map(|&player| PlayerState {
                player,
                capture: Some(capture(u64::from(player.0))),
                competition: None,
                completion: None,
                score: ScoreSummary::default(),
                gauge: BmsGauge::default(),
                last_song: Timestamp::ZERO,
            })
            .collect();
        let destinations = ids
            .iter()
            .rev()
            .map(|player| (*player, Some(PathBuf::from(format!("{}.bkr", player.0)))))
            .collect();
        let mut calls = Vec::new();
        let error = finish_cohort_with_results_and_network_and_cleanup(
            if primary_failed {
                Err(primary)
            } else {
                Ok(None)
            },
            Err(cleanup),
            states,
            None,
            destinations,
            vec!["additional practice destination preparation failure".into()],
            Some(Path::new("not-completed.bkr")),
            |recording, destination, failed| {
                assert!(failed);
                let recording = recording.unwrap();
                let player = PlayerId(
                    u32::try_from(
                        beatkernel_bms_runtime::replay_playback::decode_section_setup(
                            &recording.header().options,
                        )
                        .unwrap()
                        .chart_seed,
                    )
                    .unwrap(),
                );
                assert_eq!(destination, Some(Path::new(&format!("{}.bkr", player.0))));
                calls.push(player);
                Err("independent member save refusal".into())
            },
            |_, _| panic!("failed start or uncompleted cancellation cannot invent completion"),
        )
        .unwrap_err();
        assert_eq!(calls, ids);
        let retained = error
            .downcast_ref::<Fault>()
            .expect("native error type must survive finalization");
        assert_eq!(
            retained as *const _,
            if primary_failed {
                primary_pointer
            } else {
                cleanup_pointer
            }
        );
    }
}

// The actual cold error mapper retains typed probe/close failures. This guard is
// only a destruction counter; it cannot stand in for a native handle or window.
mod setup_probe_failure {
    use super::super::{retain_asio_setup_probe_failure, NativeOutputError};
    use beatkernel::audio::OutputOpenFailure;
    use std::{cell::Cell, error::Error, fmt, io, rc::Rc};

    struct DropGuard {
        drops: Rc<Cell<usize>>,
        label: &'static str,
    }
    impl Drop for DropGuard {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    #[derive(Debug)]
    struct ProbeFailure {
        operation: &'static str,
        code: i32,
        cause: io::Error,
    }
    impl fmt::Display for ProbeFailure {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{} refused with code {}", self.operation, self.code)
        }
    }
    impl Error for ProbeFailure {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.cause)
        }
    }
    type Failure = OutputOpenFailure<NativeOutputError, DropGuard, ()>;

    #[test]
    fn retained_asio_probe_and_close_failures_keep_both_typed_boxes_and_pending_guard() {
        let primary = Box::new(ProbeFailure {
            operation: "sample-rate probe",
            code: -998,
            cause: io::Error::from_raw_os_error(5),
        });
        let primary_pointer = primary.as_ref() as *const ProbeFailure;
        let cleanup = Box::new(io::Error::from_raw_os_error(16));
        let cleanup_pointer = cleanup.as_ref() as *const io::Error;
        let drops = Rc::new(Cell::new(0));
        let guard = DropGuard {
            drops: Rc::clone(&drops),
            label: "initial probe owner",
        };
        let error = retain_asio_setup_probe_failure(primary, Err(cleanup), guard);
        let failure = error
            .downcast_ref::<Failure>()
            .expect("failed close must keep the existing pending-owner carrier");
        assert!(failure.mixer().is_none());
        assert_eq!(
            failure.pending_owner().unwrap().label,
            "initial probe owner"
        );
        assert_eq!(drops.get(), 0, "the error still owns its pending guard");
        let retained_primary = failure.error().0.downcast_ref::<ProbeFailure>().unwrap();
        assert_eq!(retained_primary as *const _, primary_pointer);
        assert_eq!(retained_primary.operation, "sample-rate probe");
        assert_eq!(retained_primary.code, -998);
        let retained_cleanup = failure
            .cleanup_error()
            .unwrap()
            .0
            .downcast_ref::<io::Error>()
            .unwrap();
        assert_eq!(retained_cleanup as *const _, cleanup_pointer);
        assert_eq!(retained_cleanup.raw_os_error(), Some(16));
        assert_eq!(
            error.to_string(),
            "sample-rate probe refused with code -998"
        );
        let wrapper = error
            .source()
            .unwrap()
            .downcast_ref::<NativeOutputError>()
            .unwrap();
        let source = wrapper
            .source()
            .unwrap()
            .downcast_ref::<ProbeFailure>()
            .unwrap();
        assert_eq!(source as *const _, primary_pointer);
        assert_eq!(
            source
                .source()
                .unwrap()
                .downcast_ref::<io::Error>()
                .unwrap()
                .raw_os_error(),
            Some(5)
        );
        assert_eq!(
            failure
                .cleanup_error()
                .unwrap()
                .source()
                .unwrap()
                .downcast_ref::<io::Error>()
                .unwrap() as *const _,
            cleanup_pointer
        );
        drop(error);
        assert_eq!(drops.get(), 1);
        assert_eq!(Rc::strong_count(&drops), 1);
    }

    #[test]
    fn retained_asio_probe_io_and_typed_cleanup_survive_carrier_extraction_without_guard_drop() {
        let primary = Box::new(io::Error::from_raw_os_error(22));
        let primary_pointer = primary.as_ref() as *const io::Error;
        let cleanup = Box::new(ProbeFailure {
            operation: "control release",
            code: -999,
            cause: io::Error::from_raw_os_error(16),
        });
        let cleanup_pointer = cleanup.as_ref() as *const ProbeFailure;
        let drops = Rc::new(Cell::new(0));
        let error = retain_asio_setup_probe_failure(
            primary,
            Err(cleanup),
            DropGuard {
                drops: Rc::clone(&drops),
                label: "channel-count probe owner",
            },
        );
        let failure = error.downcast::<Failure>().expect("typed pending failure");
        let (primary, recovered, pending, cleanup) = failure.into_parts();
        assert!(recovered.is_none());
        let primary_io = primary.0.downcast_ref::<io::Error>().unwrap();
        assert_eq!(primary_io as *const _, primary_pointer);
        assert_eq!(primary_io.raw_os_error(), Some(22));
        let cleanup = cleanup.unwrap();
        let retained_cleanup = cleanup.0.downcast_ref::<ProbeFailure>().unwrap();
        assert_eq!(retained_cleanup as *const _, cleanup_pointer);
        assert_eq!(retained_cleanup.code, -999);
        assert_eq!(
            cleanup.to_string(),
            "control release refused with code -999"
        );
        assert_eq!(
            retained_cleanup
                .source()
                .unwrap()
                .downcast_ref::<io::Error>()
                .unwrap()
                .raw_os_error(),
            Some(16)
        );
        assert_eq!(pending.as_ref().unwrap().label, "channel-count probe owner");
        drop(primary);
        drop(cleanup);
        assert_eq!(
            drops.get(),
            0,
            "moving and dropping diagnostics does not drop the guard"
        );
        drop(pending);
        assert_eq!(drops.get(), 1);
        assert_eq!(Rc::strong_count(&drops), 1);
    }

    #[test]
    fn retained_asio_probe_close_success_returns_exact_original_box_and_drops_guard_once() {
        let primary = Box::new(ProbeFailure {
            operation: "initial output channels",
            code: -997,
            cause: io::Error::from_raw_os_error(5),
        });
        let primary_pointer = primary.as_ref() as *const ProbeFailure;
        let drops = Rc::new(Cell::new(0));
        let error = retain_asio_setup_probe_failure(
            primary,
            Ok(()),
            DropGuard {
                drops: Rc::clone(&drops),
                label: "successfully closed probe owner",
            },
        );
        let retained = error
            .downcast_ref::<ProbeFailure>()
            .expect("successful close must return the original error without another carrier");
        assert_eq!(retained as *const _, primary_pointer);
        assert!(!error.is::<Failure>());
        assert!(!error.is::<NativeOutputError>());
        assert_eq!(
            error.to_string(),
            "initial output channels refused with code -997"
        );
        assert_eq!(
            error
                .source()
                .unwrap()
                .downcast_ref::<io::Error>()
                .unwrap()
                .raw_os_error(),
            Some(5)
        );
        assert_eq!(
            drops.get(),
            1,
            "successful close releases its guard immediately"
        );
        drop(error);
        assert_eq!(drops.get(), 1);
        assert_eq!(Rc::strong_count(&drops), 1);
    }
}

// Separate Windows typing tier: no stream, HWND, native ticket or authority token is constructed.
#[cfg(all(target_os = "windows", feature = "asio-sdk"))]
#[test]
fn retained_asio_windows_composition_has_actual_complete_owner_and_conditional_ports() {
    use super::{
        live_output::AsioSetup,
        owned_output::{self, WindowsOutputOwner},
    };
    use beatkernel::audio::InterpolatedTargetTime;
    use beatkernel_bms_runtime::gameplay::output::{
        adapters::remix::RemixedOutputBackend,
        ports::{
            ConditionalSourceRetirementNativeOutputBackend, OutputChannelRemixBackend,
            SourceHistoryRetirementNativeOutputBackend,
        },
    };
    use beatkernel_bms_runtime::{
        audio_authority::AudioAuthorityCoordinateBasis,
        gameplay_presentation::GameplayAudioOutputContext,
        native_audio_presentation::NativeAudioPresentation,
        native_coordinate_control::CoordinateControlFacts,
        native_playback_phase::NativePlaybackMapping,
    };
    use beatkernel_platform::windows::clock::QpcClock;
    fn complete_backend<
        B: OutputChannelRemixBackend<NativeOutputState>
            + ConditionalSourceRetirementNativeOutputBackend<NativeOutputState>
            + SourceHistoryRetirementNativeOutputBackend<NativeOutputState>,
    >() {
    }
    fn remixed_source_backend<
        B: ConditionalSourceRetirementNativeOutputBackend<NativeOutputState>
            + SourceHistoryRetirementNativeOutputBackend<NativeOutputState>,
    >() {
    }
    complete_backend::<owned_output::WindowsAsioCompleteBackend>();
    remixed_source_backend::<RemixedOutputBackend<owned_output::WindowsAsioCompleteBackend>>();
    let _: fn(
        AsioSetup,
        AudioFormat,
        Mixer,
        &super::Options,
        QpcClock,
    ) -> super::Result<WindowsOutputOwner> = owned_output::complete_asio_owner;
    let _: fn(&mut WindowsOutputOwner) -> super::Result<Option<AudioAuthorityCoordinateBasis>> =
        WindowsOutputOwner::source_coordinate_basis;
    let _: fn(
        &mut WindowsOutputOwner,
        &mut NativeAudioPresentation,
        SourcePlaybackIdentity,
    ) -> super::Result<Option<CoordinateControlFacts>> = WindowsOutputOwner::observe_native_control;
    let _: fn(
        &mut WindowsOutputOwner,
        &NativeAudioPresentation,
        &InterpolatedTargetTime,
        SourcePlaybackIdentity,
    ) -> super::Result<Option<NativePlaybackMapping>> =
        WindowsOutputOwner::conditional_playback_mapping_at;
    let _: fn(
        &mut WindowsOutputOwner,
        &NativeAudioPresentation,
        &InterpolatedTargetTime,
        SourcePlaybackIdentity,
        Option<u64>,
    ) -> super::Result<Option<u64>> =
        WindowsOutputOwner::request_source_history_retirement_with_source_pin;
    let _: fn(
        &mut owned_output::WindowsOutputUi,
        &mut WindowsOutputOwner,
        GameplayAudioOutputContext<'_>,
        ClockPoint,
        SourcePlaybackIdentity,
        bool,
    ) -> super::Result<bool> = owned_output::WindowsOutputUi::service_coordinate_source_audio;
}
