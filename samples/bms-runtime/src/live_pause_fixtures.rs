//! Real mixer, transport and runtime fixtures for the shared live pause policy.
use super::*;
use crate::{
    native_start::{HostStartWindow, interval::StartInterval},
    playback_pause::{
        NativePause, PauseError, PauseIntervalObservation, PauseKeyboard, PausePhase,
    },
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig, PcmLimits,
        PcmSample, RenderReport, SampleBank, SampleId, VoiceId, command_queue,
    },
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId,
        DeviceSelector, EventMeta, GameControlId, NativeEventMeta, PhysicalControlId,
        PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeWindow},
    replay::{ReplayOperation, codec::ReplayCodecLimits},
    runtime::Runtime,
    time::{
        ClockDomainId, ClockMapper, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp,
    },
    transport::{Rate, Transport},
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn mixer(rate: u32) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(rate, 1).unwrap();
    let limits = AudioLimits::new(8, 2, 8, 32, 8).unwrap();
    let pcm = PcmLimits::new(128, 512, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 64], pcm).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    (
        producer,
        Mixer::new(
            MixerConfig::new(format, ClockDomainId(11), Timestamp::ZERO, limits),
            bank,
            consumer,
        )
        .unwrap(),
    )
}
fn observed(render: RenderReport, rate: u32, before: i64, after: i64) -> PauseIntervalObservation {
    PauseIntervalObservation {
        output_origin: point(11, 0),
        sample_rate: rate,
        render,
        clock: StartInterval::new(
            point(
                11,
                (i128::from(render.start_frame) * 1_000_000_000 / i128::from(rate)) as i64,
            ),
            point(22, before),
            point(22, after),
        )
        .unwrap(),
    }
}
fn interval(observation: Option<PauseIntervalObservation>, now: i64) -> LivePauseObservation {
    LivePauseObservation::Interval {
        observation,
        now: point(22, now),
    }
}
fn update(
    pause: &mut NativePause,
    evidence: LivePauseObservation,
    rendered: Option<RenderReport>,
    desired: Option<bool>,
) -> Result<LivePauseUpdate, PauseError> {
    update_live_pause(pause, evidence, rendered, desired, Timestamp::ZERO, 1000)
}
fn boundary(paused: bool, before: i64, after: i64, song: i64, frame: u64) -> LivePauseBoundary {
    LivePauseBoundary {
        paused,
        window: HostStartWindow::new(point(22, before), point(22, after)).unwrap(),
        at: point(22, if paused { before } else { after }),
        playback_frame: frame,
        song: Timestamp::from_nanos(song),
    }
}

#[test]
fn interval_ack_uses_coherent_mixer_grid_and_distinct_pause_and_resume_cutoffs() {
    let (mut producer, mut mixer) = mixer(1000);
    let first = observed(mixer.render(&mut [0.0; 4]).unwrap(), 1000, 0, 100_000);
    let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 1000).unwrap();
    let missing = update(
        &mut pause,
        interval(None, 0),
        Some(first.render),
        Some(true),
    )
    .unwrap();
    assert!(!missing.observed && missing.requested.is_none() && missing.boundary.is_none());
    assert_eq!(pause.phase(), PausePhase::Running);
    let requested = update(
        &mut pause,
        interval(Some(first), 100_000),
        Some(first.render),
        Some(true),
    )
    .unwrap();
    assert!(requested.observed);
    assert_eq!(requested.requested, Some(true));
    producer.request_pause(requested.requested.unwrap());
    let repeated = update(
        &mut pause,
        interval(Some(first), 100_000),
        Some(first.render),
        Some(true),
    )
    .unwrap();
    assert_eq!(repeated.requested, None);
    let mut silence = [99.0; 3];
    let frozen = observed(
        mixer.render(&mut silence).unwrap(),
        1000,
        3_900_000,
        4_100_000,
    );
    assert_eq!(silence, [0.0; 3]);
    let later = observed(
        mixer.render(&mut [0.0; 2]).unwrap(),
        1000,
        6_900_000,
        7_100_000,
    );
    let waiting = update(
        &mut pause,
        interval(Some(frozen), 4_099_999),
        Some(later.render),
        Some(true),
    )
    .unwrap();
    assert!(waiting.observed && waiting.boundary.is_none());
    assert_eq!(pause.last_render_report(), Some(frozen.render));
    let ack = update(
        &mut pause,
        interval(None, 4_100_000),
        Some(later.render),
        None,
    )
    .unwrap();
    assert!(!ack.observed && ack.requested.is_none());
    let paused = ack.boundary.unwrap();
    assert!(paused.paused);
    assert_eq!(paused.at, point(22, 3_900_000));
    assert_eq!(paused.window.earliest(), point(22, 3_900_000));
    assert_eq!(paused.window.latest(), point(22, 4_100_000));
    assert_eq!(
        (paused.playback_frame, paused.song.as_nanos()),
        (4, 4_000_000)
    );
    let transport = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
    let frozen_transport =
        prepare_live_transport(&transport, paused, Timestamp::from_nanos(3_000_000)).unwrap();
    assert_eq!(
        frozen_transport
            .position_at(Timestamp::from_nanos(3_899_999))
            .unwrap()
            .as_nanos(),
        3_899_999
    );
    assert_eq!(
        frozen_transport.position_at(paused.at.timestamp).unwrap(),
        paused.song
    );
    assert!(frozen_transport.is_paused());

    let requested = update(
        &mut pause,
        interval(Some(later), 7_100_000),
        Some(later.render),
        Some(false),
    )
    .unwrap();
    assert_eq!(requested.requested, Some(false));
    producer.request_pause(false);
    let resumed = observed(
        mixer.render(&mut [0.0; 2]).unwrap(),
        1000,
        8_900_000,
        9_100_000,
    );
    let waiting = update(&mut pause, interval(Some(resumed), 9_099_999), None, None).unwrap();
    assert!(waiting.boundary.is_none());
    let resumed = update(&mut pause, interval(None, 9_100_000), None, None)
        .unwrap()
        .boundary
        .unwrap();
    assert!(!resumed.paused);
    assert_eq!(resumed.at, point(22, 9_100_000));
    assert_eq!(resumed.window.earliest(), point(22, 8_900_000));
    assert_eq!((resumed.playback_frame, resumed.song), (4, paused.song));
    let running = prepare_live_transport(&frozen_transport, resumed, paused.song).unwrap();
    assert!(!running.is_paused());
    assert_eq!(
        running
            .position_at(Timestamp::from_nanos(9_099_999))
            .unwrap(),
        paused.song
    );
    assert_eq!(
        running.position_at(resumed.at.timestamp).unwrap(),
        paused.song
    );
    assert_eq!(
        running
            .position_at(Timestamp::from_nanos(10_100_000))
            .unwrap()
            .as_nanos(),
        5_000_000
    );
    assert_eq!(
        pause
            .song_origin_after_pause(Timestamp::ZERO)
            .unwrap()
            .as_nanos(),
        -5_000_000
    );
}

#[test]
fn coalesced_interval_and_point_backends_share_exact_logical_song_without_sharing_cutoffs() {
    let (mut producer, mut mixer) = mixer(1000);
    let first = observed(mixer.render(&mut [0.0; 4]).unwrap(), 1000, 0, 100_000);
    let mut bounded = NativePause::new(point(11, 0), ClockDomainId(22), 1000).unwrap();
    update(
        &mut bounded,
        interval(Some(first), 100_000),
        Some(first.render),
        Some(true),
    )
    .unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 3]).unwrap();
    let coalesced = observed(
        mixer.render(&mut [0.0; 2]).unwrap(),
        1000,
        6_900_000,
        7_100_000,
    );
    let ack = update_live_pause(
        &mut bounded,
        interval(Some(coalesced), 7_100_000),
        Some(first.render),
        None,
        Timestamp::from_nanos(47_000_000),
        1000,
    )
    .unwrap()
    .boundary
    .unwrap();
    assert_eq!(ack.at, first.clock.before);
    assert_eq!(ack.window.latest(), coalesced.clock.after);
    assert_eq!((ack.playback_frame, ack.song.as_nanos()), (4, 51_000_000));

    let mut point_pause = NativePause::new(point(11, 0), ClockDomainId(22), 1000).unwrap();
    let first_pair = ClockPair {
        source: point(11, 0),
        target: point(22, 0),
    };
    let request = update_live_pause(
        &mut point_pause,
        LivePauseObservation::Point(first_pair),
        Some(first.render),
        Some(true),
        Timestamp::from_nanos(47_000_000),
        1000,
    )
    .unwrap();
    assert!(request.observed && request.requested == Some(true));
    let crossing = ClockPair {
        source: point(11, 7_000_000),
        target: point(22, 7_000_000),
    };
    let ack = update_live_pause(
        &mut point_pause,
        LivePauseObservation::Point(crossing),
        Some(coalesced.render),
        None,
        Timestamp::from_nanos(47_000_000),
        1000,
    )
    .unwrap()
    .boundary
    .unwrap();
    assert_eq!(ack.at, point(22, 4_000_000));
    assert_eq!(ack.window.earliest(), ack.at);
    assert_eq!(ack.window.latest(), ack.at);
    assert_eq!((ack.playback_frame, ack.song.as_nanos()), (4, 51_000_000));
}

#[test]
fn live_update_validates_before_publishing_request_state_and_checked_song() {
    let (mut producer, mut mixer) = mixer(44_100);
    let first = observed(mixer.render(&mut [0.0; 1]).unwrap(), 44_100, 100, 120);
    let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 44_100).unwrap();
    for rate in [0, 1_000_000_001] {
        assert!(
            update_live_pause(
                &mut pause,
                interval(Some(first), 120),
                Some(first.render),
                Some(true),
                Timestamp::ZERO,
                rate
            )
            .is_err()
        );
        assert_eq!(pause.phase(), PausePhase::Running);
        assert_eq!(pause.last_render_report(), None);
    }
    let wrong_now = LivePauseObservation::Interval {
        observation: Some(first),
        now: point(99, 120),
    };
    assert!(
        update_live_pause(
            &mut pause,
            wrong_now,
            Some(first.render),
            Some(true),
            Timestamp::ZERO,
            44_100
        )
        .is_err()
    );
    assert_eq!(pause.phase(), PausePhase::Running);
    assert_eq!(pause.last_render_report(), None);
    let update = update_live_pause(
        &mut pause,
        interval(Some(first), 120),
        None,
        Some(true),
        Timestamp::ZERO,
        44_100,
    )
    .unwrap();
    producer.request_pause(update.requested.unwrap());
    let frozen = observed(mixer.render(&mut [0.0; 1]).unwrap(), 44_100, 200, 240);
    let before = pause.last_render_report();
    assert!(
        update_live_pause(
            &mut pause,
            interval(Some(frozen), 240),
            None,
            None,
            Timestamp::from_nanos(i64::MAX - 22_674),
            44_100
        )
        .is_err()
    );
    assert_eq!(pause.phase(), PausePhase::Pausing);
    assert_eq!(pause.last_render_report(), before);
    let waiting = update_live_pause(
        &mut pause,
        interval(Some(frozen), 239),
        None,
        None,
        Timestamp::from_nanos(i64::MAX - 22_675),
        44_100,
    )
    .unwrap();
    assert!(waiting.boundary.is_none());
    let ack = update_live_pause(
        &mut pause,
        interval(None, 240),
        None,
        None,
        Timestamp::from_nanos(i64::MAX - 22_675),
        44_100,
    )
    .unwrap()
    .boundary
    .unwrap();
    assert_eq!(ack.song, Timestamp::from_nanos(i64::MAX));
    assert_eq!(ack.playback_frame, 1);
    assert_eq!(ack.at, point(22, 200));
}

#[test]
fn exact_transport_freeze_preserves_prior_rate_history_and_rejects_unsafe_input_prefixes() {
    let mut original = Transport::new(
        Timestamp::ZERO,
        Timestamp::from_nanos(100_000_000),
        Rate::NORMAL,
    );
    original
        .set_rate(Timestamp::from_nanos(2_000_000), Rate::new(3, 2).unwrap())
        .unwrap();
    let before = original.clone();
    let paused = boundary(true, 4_000_000, 4_200_000, 104_000_000, 4);
    let last = Timestamp::from_nanos(103_500_000);
    let candidate = prepare_live_transport(&original, paused, last).unwrap();
    assert_eq!(original, before);
    for ns in [0, 1_000_000, 2_000_000, 3_000_000, 3_999_999] {
        assert_eq!(
            candidate.position_at(Timestamp::from_nanos(ns)),
            original.position_at(Timestamp::from_nanos(ns))
        );
    }
    assert_eq!(
        original
            .position_at(paused.at.timestamp)
            .unwrap()
            .as_nanos(),
        105_000_000
    );
    assert_eq!(
        candidate.position_at(paused.at.timestamp).unwrap(),
        paused.song
    );
    assert_eq!(
        candidate
            .position_at(Timestamp::from_nanos(9_000_000))
            .unwrap(),
        paused.song
    );
    assert!(validate_pre_pause_input(&candidate, paused, point(22, 3_000_000)).is_ok());
    for host in [
        point(22, 3_900_000),
        point(22, 4_000_000),
        point(22, 4_100_000),
        point(99, 3_000_000),
    ] {
        assert!(validate_pre_pause_input(&candidate, paused, host).is_err());
    }
    assert!(prepare_live_transport(&original, paused, Timestamp::from_nanos(104_000_001)).is_err());
    assert_eq!(original, before);
    let resumed = boundary(false, 7_000_000, 7_200_000, 104_000_000, 4);
    let running = prepare_live_transport(&candidate, resumed, paused.song).unwrap();
    assert_eq!(
        running.position_at(resumed.at.timestamp).unwrap(),
        paused.song
    );
    assert_eq!(
        running
            .position_at(Timestamp::from_nanos(8_200_000))
            .unwrap()
            .as_nanos(),
        105_500_000
    );
    assert!(validate_pre_pause_input(&running, resumed, point(22, 7_000_000)).is_err());
}

#[test]
fn transport_preparation_rejects_chronology_state_and_frozen_song_mismatches_atomically() {
    let pause = boundary(true, 4_000_000, 4_200_000, 4_000_000, 4);
    let mut original = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
    original
        .set_rate(Timestamp::from_nanos(5_000_000), Rate::NORMAL)
        .unwrap();
    let before = original.clone();
    // Even a same-rate command commits chronology without adding an anchor.
    assert!(prepare_live_transport(&original, pause, Timestamp::from_nanos(3_000_000)).is_err());
    assert_eq!(original, before);
    let pristine = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
    let frozen =
        prepare_live_transport(&pristine, pause, Timestamp::from_nanos(3_000_000)).unwrap();
    let before = frozen.clone();
    assert!(prepare_live_transport(&frozen, pause, pause.song).is_err());
    let resume = boundary(false, 7_000_000, 7_200_000, 4_000_000, 4);
    assert!(prepare_live_transport(&pristine, resume, pause.song).is_err());
    assert!(prepare_live_transport(&frozen, resume, Timestamp::from_nanos(3_999_999)).is_err());
    assert!(
        prepare_live_transport(
            &frozen,
            LivePauseBoundary {
                song: Timestamp::from_nanos(4_000_001),
                ..resume
            },
            Timestamp::from_nanos(4_000_001)
        )
        .is_err()
    );
    assert_eq!(frozen, before);
    let wrong_cutoff = LivePauseBoundary {
        at: point(22, 4_200_000),
        ..pause
    };
    assert!(prepare_live_transport(&pristine, wrong_cutoff, Timestamp::ZERO).is_err());
}

struct Identity;
impl ClockMapper for Identity {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn input(device: u64, ns: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(device), point(22, ns), sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(7),
        code: Some(4),
        timestamp: Some(point(99, ns - 17)),
    });
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(4),
        state,
    })
}

#[test]
fn actual_runtime_capture_preserves_committed_prefix_and_source_owned_resume_releases() {
    let source = beatkernel_bms::parse(
        "#BPM 60000\n#WAV01 original.wav\n#00011:0001\n",
        Default::default(),
    )
    .unwrap();
    let compiled = source.compile().unwrap();
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::ZERO,
    )
    .unwrap();
    let judge = JudgeEngine::new(compiled.chart, source.rules(), profile).unwrap();
    let limits =
        ReplayCodecLimits::new(65_536, 128, 4096, CodecLimits::new(4096, 4096).unwrap()).unwrap();
    let mut capture = LiveReplayCapture::new(&judge, ClockDomainId(22), limits).unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let (producer, mut mixer) = mixer(1000);
    let mut runtime = Runtime::new(
        ClockDomainId(22),
        ClockDomainId(11),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        Vec::new(),
        8,
    )
    .unwrap();
    let first = observed(mixer.render(&mut [0.0; 4]).unwrap(), 1000, 0, 100_000);
    let mut keyboard = PauseKeyboard::new();
    let down = input(1, 2_000_000, 1, ButtonState::Down);
    assert!(keyboard.accept(&down).unwrap());
    let report = runtime
        .process_input(down, &Identity, point(11, 4_000_000))
        .unwrap();
    assert!(report.judge_error.is_none());
    assert_eq!(report.judge_events.len(), 1);
    assert!(matches!(
        report.judge_events[0].outcome,
        JudgeOutcome::Hit { .. }
    ));
    capture.record_report(&report).unwrap();
    let committed = capture.records().to_vec();
    let hash = runtime.judge_mut().stable_hash().unwrap();
    let previous_transport = runtime.transport_mut().clone();
    let rewind = boundary(true, 3_900_000, 4_100_000, 1_000_000, 1);
    assert!(prepare_live_transport(runtime.transport_mut(), rewind, report.song_time).is_err());
    assert_eq!(&*runtime.transport_mut(), &previous_transport);
    assert_eq!(runtime.judge_mut().stable_hash().unwrap(), hash);
    assert_eq!(capture.records(), committed);

    let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 1000).unwrap();
    let requested = update(
        &mut pause,
        interval(Some(first), 100_000),
        Some(first.render),
        Some(true),
    )
    .unwrap();
    runtime.request_audio_pause(requested.requested.unwrap());
    let frozen = observed(
        mixer.render(&mut [0.0; 3]).unwrap(),
        1000,
        3_900_000,
        4_100_000,
    );
    assert!(
        update(&mut pause, interval(Some(frozen), 4_099_999), None, None)
            .unwrap()
            .boundary
            .is_none()
    );
    let paused = update(&mut pause, interval(None, 4_100_000), None, None)
        .unwrap()
        .boundary
        .unwrap();
    let candidate =
        prepare_live_transport(runtime.transport_mut(), paused, report.song_time).unwrap();
    *runtime.transport_mut() = candidate;
    let advanced = runtime
        .advance_to(paused.at, &Identity, point(11, 4_000_000))
        .unwrap();
    assert_eq!(advanced.song_time, paused.song);
    capture.record_report(&advanced).unwrap();
    let prefix = capture.records().to_vec();
    let release = input(1, 4_000_000, 2, ButtonState::Up);
    keyboard.observe_paused(release.clone()).unwrap();
    keyboard
        .observe_paused(input(2, 4_050_000, 1, ButtonState::Down))
        .unwrap();
    assert_eq!(capture.records(), prefix);

    let requested = update(
        &mut pause,
        interval(Some(frozen), 5_000_000),
        None,
        Some(false),
    )
    .unwrap();
    runtime.request_audio_pause(requested.requested.unwrap());
    let resumed = observed(
        mixer.render(&mut [0.0; 2]).unwrap(),
        1000,
        7_000_000,
        7_200_000,
    );
    let resumed = update(&mut pause, interval(Some(resumed), 7_200_000), None, None)
        .unwrap()
        .boundary
        .unwrap();
    let candidate =
        prepare_live_transport(runtime.transport_mut(), resumed, advanced.song_time).unwrap();
    *runtime.transport_mut() = candidate;
    let releases = keyboard.resume(resumed.at).unwrap();
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].meta().source, DeviceId(1));
    assert_eq!(releases[0].meta().timestamp, resumed.at.timestamp);
    assert_eq!(
        releases[0].meta().original_clock_point,
        Some(point(22, 4_000_000))
    );
    assert_eq!(releases[0].meta().native, release.meta().native);
    let report = runtime
        .process_input(releases[0].clone(), &Identity, point(11, 4_000_000))
        .unwrap();
    assert_eq!(report.song_time, paused.song);
    capture.record_report(&report).unwrap();
    assert!(
        !keyboard
            .accept(&input(2, 7_200_000, 2, ButtonState::Repeat))
            .unwrap()
    );
    assert!(
        !keyboard
            .accept(&input(2, 7_200_000, 3, ButtonState::Up))
            .unwrap()
    );
    let down = input(1, 7_200_000, 3, ButtonState::Down);
    assert!(keyboard.accept(&down).unwrap());
    let report = runtime
        .process_input(down, &Identity, point(11, 4_000_000))
        .unwrap();
    capture.record_report(&report).unwrap();
    assert_eq!(capture.records().len(), 4);
    assert_eq!(
        capture.records()[2].song_time,
        capture.records()[3].song_time
    );
    for (index, state) in [(2, ButtonState::Up), (3, ButtonState::Down)] {
        let ReplayOperation::Input(input) = &capture.records()[index].operation else {
            panic!("expected retained input");
        };
        let PhysicalInputEvent::Button(button) = &input.physical else {
            panic!("expected original button");
        };
        assert_eq!(button.meta.source, DeviceId(1));
        assert_eq!(button.state, state);
    }
    assert_eq!(&capture.records()[..2], prefix);
}
