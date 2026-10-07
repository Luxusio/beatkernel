//! Actual preparation, pristine judge, native capture and stepped identity; no I/O.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    multiplayer::competition_identity as native_identity,
    multiplayer_protocol::{MultiplayerError, Session},
    multiplayer_start::{StartPolicy, StartRole},
    native_judge::{NativeJudgeConfig, prepare_capture},
    prepare_from_source,
    replay_capture::LiveReplayCapture,
    step_gameplay::{StepGameplay, StepGameplayConfig, StepGameplayError},
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits},
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::JudgeEngine,
    replay::codec::{ReplayCodecLimits, decode_replay},
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
};

const CHART: &str = "#BPM 60\n#WAV01 key.wav\n#00011:01\n#00111:01\n";

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}

fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(11, 1_000_000_000),
        output_origin: point(22, 0),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 8,
        bgm_pending: 2,
        bgm_lookahead: Duration::from_nanos(500_000_000),
        telemetry_capacity: 8,
    }
}

fn limits(bytes: usize, header: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(bytes, 16, header, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}

fn prepared(chart: &str, sample_rate: u32, seed: u64) -> PreparedBms {
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&40_u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    for value in [1_u16, 1] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    for value in [4_u32, 8] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    for value in [2_u16, 16] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&4_u32.to_le_bytes());
    for value in [8192_i16, -4096] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("pack/chart.bms", chart.as_bytes().to_vec())
        .unwrap();
    files.insert("pack/key.wav", wav).unwrap();
    let source = files.scope("pack/chart.bms").unwrap();
    prepare_from_source(
        chart.as_bytes(),
        &source,
        AudioFormat::new(sample_rate, 1).unwrap(),
        PcmLimits::new(256, 1024, 8).unwrap(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        seed,
        None,
    )
    .unwrap()
}

fn bindings(key: u16) -> BindingMap {
    BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(key),
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}

fn game(chosen: StepGameplayConfig, chart: &str, rate: u32, seed: u64, key: u16) -> StepGameplay {
    StepGameplay::new(prepared(chart, rate, seed), chosen, bindings(key))
        .unwrap()
        .0
}

fn mapper(chosen: StepGameplayConfig) -> AffineClockMapper {
    AffineClockMapper::exact_offset(
        ClockPair {
            source: chosen.host_origin,
            target: chosen.output_origin,
        },
        ClockInterval {
            start: chosen.host_origin.timestamp,
            end: chosen
                .host_origin
                .timestamp
                .checked_add(Duration::from_nanos(10_000_000_000))
                .unwrap(),
        },
    )
    .unwrap()
}

fn hit(game: &mut StepGameplay, chosen: StepGameplayConfig) {
    let event = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(77), chosen.host_origin, 0),
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    });
    game.process_input(event, &mapper(chosen), chosen.output_origin)
        .unwrap();
    assert_eq!(game.score().hits, 1);
}

#[test]
fn stepped_identity_matches_independently_prepared_native_capture_exactly() {
    let bounds = limits(65_536, 4096);
    for seed in [0, u64::MAX] {
        let chosen = config();
        let mut stepped = game(chosen, CHART, 4, seed, 4);
        let native = prepared(CHART, 48_000, seed);
        let profile = NativeJudgeConfig {
            early: chosen.early_ns,
            late: chosen.late_ns,
            offset: chosen.offset_ns,
            preroll: 123_000_000,
            output: ClockDomainId(900),
            end: None,
        }
        .profile()
        .unwrap();
        let judge =
            JudgeEngine::new(native.compiled.chart, native.source.rules(), profile).unwrap();
        let capture = prepare_capture(
            &judge,
            ClockDomainId(700),
            Timestamp::ZERO,
            seed,
            Some(bounds),
        )
        .unwrap()
        .unwrap();
        let expected =
            native_identity(capture.header(), env!("CARGO_PKG_VERSION"), bounds).unwrap();
        assert_eq!(
            stepped.competition_identity(bounds, seed).unwrap(),
            expected
        );
        let decoded = decode_replay(&expected, bounds).unwrap();
        assert_eq!(decoded.header.normalized_clock, ClockDomainId(0));
        assert!(decoded.records.is_empty());
        assert!(stepped.judge().effective_song_time().is_none());
        stepped.fail();
        assert!(
            stepped.take_replay().unwrap().is_none(),
            "identity alone must not enable capture"
        );
    }
}

#[test]
fn clocks_output_bindings_and_optional_capture_do_not_change_setup_identity() {
    let bounds = limits(65_536, 4096);
    let seed = 73;
    let first = game(config(), CHART, 4, seed, 4);
    let expected = first.competition_identity(bounds, seed).unwrap();
    let chosen = StepGameplayConfig {
        host_origin: point(400, 9_000_000_000),
        output_origin: point(500, 111),
        preroll: Duration::from_nanos(250_000_001),
        ..config()
    };
    let mut captured = game(chosen, CHART, 44_100, seed, 9);
    let untouched_bytes = LiveReplayCapture::new_at_with_chart_seed(
        captured.judge(),
        chosen.host_origin.domain,
        bounds,
        Timestamp::ZERO,
        seed,
    )
    .unwrap()
    .into_bytes()
    .unwrap();
    captured.configure_capture(bounds, seed).unwrap();
    captured.activate(point(400, 10_000_000_000)).unwrap();
    for _ in 0..3 {
        assert_eq!(
            captured.competition_identity(bounds, seed).unwrap(),
            expected
        );
    }
    captured.fail();
    assert_eq!(captured.take_replay().unwrap().unwrap(), untouched_bytes);
    assert!(captured.take_replay().unwrap().is_none());
}

#[test]
fn compiled_chart_profile_and_branch_provenance_change_actual_peer_compatibility() {
    let bounds = limits(65_536, 4096);
    let baseline = game(config(), CHART, 4, 7, 4)
        .competition_identity(bounds, 7)
        .unwrap();
    let changed_chart = CHART.replace("#00111:01", "#00112:01");
    let variants = [
        game(config(), &changed_chart, 4, 7, 4)
            .competition_identity(bounds, 7)
            .unwrap(),
        game(
            StepGameplayConfig {
                early_ns: 1,
                ..config()
            },
            CHART,
            4,
            7,
            4,
        )
        .competition_identity(bounds, 7)
        .unwrap(),
        game(
            StepGameplayConfig {
                late_ns: 1,
                ..config()
            },
            CHART,
            4,
            7,
            4,
        )
        .competition_identity(bounds, 7)
        .unwrap(),
        game(
            StepGameplayConfig {
                offset_ns: -1,
                ..config()
            },
            CHART,
            4,
            7,
            4,
        )
        .competition_identity(bounds, 7)
        .unwrap(),
        game(config(), CHART, 4, 8, 4)
            .competition_identity(bounds, 8)
            .unwrap(),
    ];
    for changed in variants {
        assert_ne!(changed, baseline);
        let mut session =
            Session::new(baseline.clone(), StartRole::Host, StartPolicy::default(), 0).unwrap();
        assert_eq!(
            session.receive(1, &changed, 0),
            Err(MultiplayerError::IncompatibleSetup)
        );
        assert!(!session.setup_complete());
    }
    let mut compatible =
        Session::new(baseline.clone(), StartRole::Join, StartPolicy::default(), 0).unwrap();
    compatible.receive(1, &baseline, 0).unwrap();
    assert!(compatible.setup_complete());
}

#[test]
fn refused_limits_leave_pristine_game_and_capture_configuration_usable() {
    let chosen = config();
    let mut game = game(chosen, CHART, 4, 0, 4);
    let bounds = limits(65_536, 4096);
    let expected = game.competition_identity(bounds, 0).unwrap();
    let judge = game.judge().stable_hash().unwrap();
    let song = game.song_time();
    for rejected in [limits(65_536, 1), limits(64, 64)] {
        assert!(game.competition_identity(rejected, 0).is_err());
        assert!(!game.failed());
        assert_eq!(game.judge().stable_hash().unwrap(), judge);
        assert_eq!(game.song_time(), song);
        assert!(game.judge().effective_song_time().is_none());
        assert_eq!(game.competition_identity(bounds, 0).unwrap(), expected);
    }
    game.configure_capture(bounds, 0).unwrap();
    hit(&mut game, chosen);
    game.fail();
    let bytes = game.take_replay().unwrap().unwrap();
    assert_eq!(decode_replay(&bytes, bounds).unwrap().records.len(), 1);
}

#[test]
fn started_or_fenced_identity_is_refused_without_erasing_actual_judge_progress() {
    let chosen = config();
    let bounds = limits(65_536, 4096);
    let mut game = game(chosen, CHART, 4, 0, 4);
    game.advance_to(chosen.host_origin, &mapper(chosen), chosen.output_origin)
        .unwrap();
    assert!(matches!(
        game.competition_identity(bounds, 0),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    assert!(!game.failed());
    hit(&mut game, chosen);
    let score = game.score().clone();
    let hash = game.judge().stable_hash().unwrap();
    assert!(matches!(
        game.competition_identity(bounds, 0),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    assert_eq!(game.score(), &score);
    assert_eq!(game.judge().stable_hash().unwrap(), hash);
    game.fail();
    assert!(matches!(
        game.competition_identity(bounds, 0),
        Err(StepGameplayError::Failed)
    ));
    assert_eq!(game.score(), &score);
}
