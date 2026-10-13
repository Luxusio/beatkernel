//! Pure desktop journeys use the production validators and ALSA mapper. Applied
//! metadata is supplied, not hardware evidence; channel refusals are UI evidence.
#![cfg(target_os = "linux")]

use super::*;
use beatkernel::{
    audio::ChannelMatrix,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_bms_runtime::{
    gameplay::output::adapters::{alsa_ui, encoding::native_encoding},
    live_output_control::{OutputCapability, OutputReply},
    settings::{OutputEncoding, SettingsHost},
    settings_profile::{
        PlayerProfile, decode_player_profile, decode_profile, encode_player_profile, encode_profile,
    },
    ui::live_audio::LiveAudioFrame,
};
use beatkernel_platform::{
    audio::DeviceFormat,
    linux::{AlsaAppliedConfig, AlsaRequest},
};

const CHOICES: [(OutputEncoding, &str); 4] = [
    (OutputEncoding::Float32, "float32"),
    (OutputEncoding::Pcm16, "pcm16"),
    (OutputEncoding::Pcm24, "pcm24"),
    (OutputEncoding::Pcm32, "pcm32"),
];

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn value<'a>(values: &'a [String], flag: &str) -> Option<&'a str> {
    values
        .chunks_exact(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn native_request(choice: OutputEncoding) -> AlsaRequest {
    AlsaRequest {
        device: "supplied-applied-device".into(),
        format: DeviceFormat::new(48_000, 1, native_encoding(choice), None).unwrap(),
        buffer_frames: 256,
        period_frames: 128,
        allow_size_rounding: true,
        monotonic_domain: ClockDomainId(37),
    }
}

fn applied(request: &AlsaRequest) -> AlsaAppliedConfig {
    AlsaAppliedConfig {
        requested: request.clone(),
        format: request.format,
        buffer_frames: request.buffer_frames,
        period_frames: request.period_frames,
        sizing_adjusted: false,
        output_domain: ClockDomainId(38),
        output_origin: Timestamp::ZERO,
    }
}

fn live_fixture(capability: OutputCapability) -> (Desktop, player::PlayerPublisher) {
    let mut app = super::tests::lifecycle_fixture();
    app.validate = crate::app::validate_native;
    app.validate_replay = crate::app::validate_replay;
    let next = app
        .prepare_route(ScreenRoute::Play { replay: false })
        .unwrap();
    app.commit_route(next);
    let (publisher, viewer) = player::channel();
    publisher.advertise_output(Some(capability)).unwrap();
    viewer.request_pause(true);
    // A cold channel starts with an unavailable placeholder. Consume that
    // initial publication before installing this acknowledged paused UI state,
    // so later reply polling cannot overwrite it with stale startup metadata.
    let startup = viewer.take_latest().expect("cold channel publication");
    assert_eq!(startup.pause, player::PauseState::Unavailable);
    let mut game = super::tests::retry_fixture();
    game.viewer = viewer;
    game.snapshot = Some(player::PlayerSnapshot {
        status: player::PlayerStatus::Playing,
        pause: player::PauseState::Paused,
        ..Default::default()
    });
    app.game = Some(game);
    (app, publisher)
}

fn compose_live(app: &mut Desktop) {
    let draft = app.live_audio.as_ref().unwrap();
    let view = app.live_audio_view.as_ref().unwrap();
    view.update(LiveAudioFrame {
        fields: draft.values.fields(),
        selected: draft.selected,
        editor: &draft.editor,
        message: draft.message.as_deref(),
        pending: app.game.as_ref().unwrap().viewer.output_pending(),
        hovered: None,
        armed: None,
    })
    .unwrap();
    app.scene.clear();
    app.hits.clear();
    view.compose(&mut app.scene, &mut app.hits).unwrap();
}

fn edit_live(app: &mut Desktop, flag: &str, text: &str) -> usize {
    let draft = app.live_audio.as_mut().unwrap();
    let index = draft
        .values
        .fields()
        .iter()
        .position(|field| field.flag == flag)
        .unwrap();
    draft.select(index).unwrap();
    draft.editor.select_all();
    draft.edit(None, Some(text));
    assert_eq!(draft.editor.value(), text);
    index
}

fn assert_paused_owner(app: &Desktop, play: ScreenInstanceId, launch: &[String]) {
    assert!(app.navigator.retains(play));
    let game = app.game.as_ref().unwrap();
    assert_eq!(game.launch.args(), launch);
    assert_eq!(
        game.snapshot.as_ref().unwrap().pause,
        player::PauseState::Paused
    );
    assert!(game.viewer.pause_requested());
    assert!(!game.cancelling);
    assert!(!game.joined);
    assert!(game.worker.is_none());
}

#[test]
fn output_encoding_settings_invalid_apply_preserves_draft_then_valid_profile_apply_and_reopen() {
    let mut app = super::tests::lifecycle_fixture();
    app.validate = crate::app::validate_native;
    app.validate_replay = crate::app::validate_replay;
    app.open_settings();
    let scope = app.navigator.active_id();
    let original = app.options.native.clone();
    let initial = app.settings.as_ref().unwrap();
    let flags: Vec<_> = initial
        .values
        .fields()
        .iter()
        .map(|field| field.flag)
        .collect();
    let encoding = flags.iter().position(|flag| *flag == "--encoding").unwrap();
    assert_eq!(encoding, flags.len() - 1, "append after the existing rows");
    assert_eq!(flags[encoding - 1], "--speed");
    assert_eq!(
        &flags[..9],
        &[
            "--local-player",
            "--local-input",
            "--evdev",
            "--alsa",
            "--rate",
            "--channels",
            "--period-frames",
            "--buffer-frames",
            "--advance-lag-ns",
        ]
    );
    assert_eq!(initial.values.fields()[encoding].value, "");
    assert_eq!(value(&initial.values.native_args(), "--encoding"), None);
    assert_eq!(initial.profile.value(), "");
    app.settings.as_mut().unwrap().select(encoding - 1).unwrap();
    app.key(KeyCode::Tab, false);
    let draft = app.settings.as_ref().unwrap();
    assert_eq!(draft.selected, encoding);
    assert_eq!(draft.values.fields()[draft.selected].flag, "--encoding");
    assert!(!draft.profile_focused);
    app.key(KeyCode::Tab, false);
    let draft = app.settings.as_ref().unwrap();
    assert_eq!(draft.selected, encoding);
    assert!(draft.profile_focused);
    app.key(KeyCode::Tab, false);
    let draft = app.settings.as_ref().unwrap();
    assert_eq!(draft.selected, 0);
    assert!(!draft.profile_focused);
    let draft = app.settings.as_mut().unwrap();
    draft.select(encoding).unwrap();
    draft.editor.select_all();
    draft.edit(None, Some("PCM16"));
    app.apply_settings();
    assert_eq!(app.navigator.active_id(), scope);
    assert_eq!(app.options.native, original);
    let draft = app.settings.as_ref().unwrap();
    assert_eq!(draft.editor.value(), "PCM16");
    assert_eq!(draft.values.fields()[encoding].value, "PCM16");
    assert!(draft
        .error
        .as_ref()
        .is_some_and(|error| error.contains("encoding")));

    for (_, token) in CHOICES {
        let draft = app.settings.as_mut().unwrap();
        draft.editor.select_all();
        draft.edit(None, Some(token));
        draft.profile_focused = true;
        draft.profile.select_all();
        draft.edit(None, Some("linux-output.profile"));
        let native = draft.values.native_args();
        let v1 = encode_profile(&draft.values, SettingsHost::Linux).unwrap();
        assert_eq!(
            decode_profile(&v1, SettingsHost::Linux)
                .unwrap()
                .native_args(),
            native
        );
        let v2 = encode_player_profile(
            &PlayerProfile {
                native: draft.values.clone(),
                presentation: draft.presentation,
            },
            SettingsHost::Linux,
        )
        .unwrap();
        let restored = decode_player_profile(&v2, SettingsHost::Linux).unwrap();
        assert_eq!(restored.native.native_args(), native);
        assert_eq!(restored.presentation, draft.presentation);
        app.apply_settings();
        assert_eq!(app.navigator.route(), ScreenRoute::Selection);
        assert!(app.settings.is_none());
        assert_eq!(value(&app.options.native, "--encoding"), Some(token));
        assert_eq!(
            app.options.profile,
            Some(PathBuf::from("linux-output.profile"))
        );
        let mut play_args = app.options.native.clone();
        play_args.extend(args(&[
            "--chart",
            "fixture.bms",
            "--evdev",
            "fixture-evdev",
            "--alsa",
            "null",
            "--rate",
            "48000",
            "--channels",
            "1",
            "--period-frames",
            "128",
            "--buffer-frames",
            "256",
            "--bind",
            "11:04",
        ]));
        let prepared = crate::app::prepare_native_invocation(&play_args, false).unwrap();
        assert_eq!(value(&prepared, "--encoding"), Some(token));
        crate::app::validate_native(&prepared).unwrap();
        // Watch validation uses the real replay projection and strict consumer.
        let mut replay = app.options.native.clone();
        replay.extend(args(&["--chart", "fixture.bms", "--replay", "fixture.bkr"]));
        crate::app::validate_replay(&replay).unwrap();
        let projected = crate::auto_native::syntax_replay(&replay).unwrap();
        assert_eq!(value(&projected, "--encoding"), Some(token));
        app.open_settings();
        let draft = app.settings.as_mut().unwrap();
        // Supplied pairs keep their original identities; the newly populated
        // encoding pair becomes the first supplied row, without losing tokens.
        let index = draft
            .values
            .fields()
            .iter()
            .position(|field| field.flag == "--encoding")
            .unwrap();
        draft.select(index).unwrap();
        assert_eq!(draft.editor.value(), token);
        assert_eq!(draft.profile.value(), "linux-output.profile");
        app.back();
        assert_eq!(app.navigator.route(), ScreenRoute::Selection);
        app.open_settings();
        let draft = app.settings.as_mut().unwrap();
        let index = draft
            .values
            .fields()
            .iter()
            .position(|field| field.flag == "--encoding")
            .unwrap();
        draft.select(index).unwrap();
        assert_eq!(draft.editor.value(), token);
    }
}

#[test]
fn output_encoding_live_native_applied_capability_maps_each_edit_and_refreshes_same_scope() {
    for (choice, token) in CHOICES {
        let current = native_request(OutputEncoding::Pcm24);
        let initial = alsa_ui::capability(&applied(&current)).unwrap();
        let (mut app, publisher) = live_fixture(initial.clone());
        app.options.native = args(&["--encoding", "pcm16"]);
        let cold_args = app.options.native.clone();
        let launch = app.game.as_ref().unwrap().launch.args().to_vec();
        let play = app.navigator.active_id().unwrap();
        app.key(KeyCode::F2, false);
        let scope = app.navigator.active_id().unwrap();
        assert_eq!(
            value(
                &app.live_audio.as_ref().unwrap().values.native_args(),
                "--encoding"
            ),
            Some("pcm24")
        );
        let encoding = app
            .live_audio
            .as_ref()
            .unwrap()
            .values
            .fields()
            .iter()
            .position(|field| field.flag == "--encoding")
            .unwrap();
        for _ in 0..encoding {
            app.key(KeyCode::Tab, false);
        }
        let draft = app.live_audio.as_ref().unwrap();
        assert_eq!(draft.selected, encoding);
        assert_eq!(draft.editor.value(), "pcm24");
        assert_eq!(app.navigator.active_id(), Some(scope));
        assert_paused_owner(&app, play, &launch);
        let index = edit_live(&mut app, "--encoding", token);
        compose_live(&mut app);
        let hit = app
            .hits
            .iter()
            .find(|(id, _)| *id == ControlId(1000 + index as u64))
            .unwrap()
            .1;
        assert!(hit.y + hit.height < HEIGHT as i64);
        app.apply_live_audio();
        let request = publisher.take_output_request().unwrap().unwrap();
        assert_eq!(value(&request.args, "--encoding"), Some(token));
        let mapped = alsa_ui::request_for_args(&current, &request.args).unwrap();
        assert_eq!(mapped.format.encoding(), native_encoding(choice));
        assert_eq!(mapped.format.pcm(), current.format.pcm());
        // The owner may authorize buffer rounding; refresh must use applied,
        // canonical values rather than echoing the submitted draft.
        let mut metadata = applied(&mapped);
        metadata.buffer_frames = 512;
        metadata.sizing_adjusted = true;
        let canonical = alsa_ui::capability(&metadata).unwrap();
        publisher
            .reply_output(&OutputReply {
                id: request.id,
                result: Ok(canonical.clone()),
            })
            .unwrap();
        app.collect_game();
        compose_live(&mut app);
        let draft = app.live_audio.as_ref().unwrap();
        assert_eq!(draft.selected, index);
        assert_eq!(draft.editor.value(), token);
        assert_eq!(
            value(&draft.values.native_args(), "--buffer-frames"),
            Some("512")
        );
        assert!(draft
            .message
            .as_ref()
            .unwrap()
            .contains("Playback remains paused"));
        assert_eq!(app.navigator.active_id(), Some(scope));
        assert_eq!(
            app.game
                .as_ref()
                .unwrap()
                .viewer
                .output_capability()
                .unwrap(),
            Some(canonical.clone())
        );
        assert_eq!(app.options.native, cold_args);
        assert_paused_owner(&app, play, &launch);
        app.activate(ControlId(91));
        assert_eq!(app.navigator.active_id(), Some(play));
        assert!(app.live_audio.is_none() && app.live_audio_view.is_none());
        app.open_live_audio();
        assert_ne!(app.navigator.active_id(), Some(scope));
        assert_eq!(
            app.live_audio.as_ref().unwrap().values.native_args(),
            canonical.settings().unwrap().native_args()
        );
        assert_paused_owner(&app, play, &launch);
    }
}

#[test]
fn output_encoding_live_converted_combined_edit_preserves_source_and_refreshes_canonical_matrix() {
    let current = native_request(OutputEncoding::Float32);
    let matrix = ChannelMatrix::default_mix(1, 1).unwrap();
    let initial = alsa_ui::converted_capability(&applied(&current), &matrix).unwrap();
    let (mut app, publisher) = live_fixture(initial);
    let play = app.navigator.active_id().unwrap();
    let launch = app.game.as_ref().unwrap().launch.args().to_vec();
    app.open_live_audio();
    let scope = app.navigator.active_id();
    for (flag, text) in [
        ("--encoding", "pcm32"),
        ("--rate", "44100"),
        ("--buffer-frames", "512"),
        ("--period-frames", "64"),
    ] {
        edit_live(&mut app, flag, text);
    }
    let index = edit_live(&mut app, "--output-matrix", "1 ; 0.5");
    compose_live(&mut app);
    app.apply_live_audio();
    let request = publisher.take_output_request().unwrap().unwrap();
    let mapped = alsa_ui::converted_request_for_args(&current, &matrix, &request.args).unwrap();
    assert_eq!(mapped.native.format.sample_rate(), 44_100);
    assert_eq!(mapped.native.format.channels(), 2);
    assert_eq!(
        mapped.native.format.encoding(),
        native_encoding(OutputEncoding::Pcm32)
    );
    assert_eq!(
        (mapped.native.buffer_frames, mapped.native.period_frames),
        (512, 64)
    );
    let converted_matrix = mapped.matrix.as_ref().unwrap();
    assert_eq!(converted_matrix.source_channels(), 1);
    assert_eq!(converted_matrix.target_channels(), 2);
    assert_eq!(current, native_request(OutputEncoding::Float32));
    assert_eq!(matrix, ChannelMatrix::default_mix(1, 1).unwrap());
    let canonical =
        alsa_ui::converted_capability(&applied(&mapped.native), converted_matrix).unwrap();
    publisher
        .reply_output(&OutputReply {
            id: request.id,
            result: Ok(canonical.clone()),
        })
        .unwrap();
    app.collect_game();
    compose_live(&mut app);
    assert_eq!(app.navigator.active_id(), scope);
    let draft = app.live_audio.as_ref().unwrap();
    assert_eq!(draft.selected, index);
    assert_eq!(draft.editor.value(), "1;0.5");
    assert_eq!(
        value(&draft.values.native_args(), "--encoding"),
        Some("pcm32")
    );
    assert_eq!(value(&draft.values.native_args(), "--rate"), Some("44100"));
    assert_eq!(
        app.game
            .as_ref()
            .unwrap()
            .viewer
            .output_capability()
            .unwrap(),
        Some(canonical)
    );
    assert_paused_owner(&app, play, &launch);
}

#[test]
fn output_encoding_live_mapper_error_and_invalid_reply_preserve_edit_and_accepted_capability() {
    let current = native_request(OutputEncoding::Pcm16);
    let initial = alsa_ui::capability(&applied(&current)).unwrap();
    let (mut app, publisher) = live_fixture(initial.clone());
    let play = app.navigator.active_id().unwrap();
    let launch = app.game.as_ref().unwrap().launch.args().to_vec();
    app.open_live_audio();
    let scope = app.navigator.active_id();
    let index = edit_live(&mut app, "--encoding", "pcm8");
    app.apply_live_audio();
    let request = publisher.take_output_request().unwrap().unwrap();
    let error = alsa_ui::request_for_args(&current, &request.args).unwrap_err();
    assert!(error.contains("encoding"));
    publisher
        .reply_output(&OutputReply {
            id: request.id,
            result: Err(error.clone()),
        })
        .unwrap();
    app.collect_game();
    assert_eq!(app.live_audio.as_ref().unwrap().editor.value(), "pcm8");
    assert_eq!(
        app.live_audio.as_ref().unwrap().message.as_deref(),
        Some(error.as_str())
    );
    assert_eq!(
        app.game
            .as_ref()
            .unwrap()
            .viewer
            .output_capability()
            .unwrap(),
        Some(initial.clone())
    );

    edit_live(&mut app, "--encoding", "pcm32");
    app.apply_live_audio();
    let request = publisher.take_output_request().unwrap().unwrap();
    let mut invalid = initial.clone();
    invalid.current_args.extend(args(&["--encoding", "pcm32"]));
    assert!(publisher
        .reply_output(&OutputReply {
            id: request.id,
            result: Ok(invalid)
        })
        .is_err());
    app.collect_game();
    assert!(app.game.as_ref().unwrap().viewer.output_pending());
    assert_eq!(app.live_audio.as_ref().unwrap().request, Some(request.id));
    assert_eq!(app.live_audio.as_ref().unwrap().editor.value(), "pcm32");
    assert_eq!(app.live_audio.as_ref().unwrap().selected, index);
    assert_eq!(
        app.game
            .as_ref()
            .unwrap()
            .viewer
            .output_capability()
            .unwrap(),
        Some(initial.clone())
    );

    // This explicit reply verifies channel feedback only. The actual retained
    // owner guard is separately covered by target_replacement_fixtures and
    // audio_publication_fixtures; injecting it cannot prove native refusal.
    let refusal = "output replacement is unavailable during retained practice";
    publisher
        .reply_output(&OutputReply {
            id: request.id,
            result: Err(refusal.into()),
        })
        .unwrap();
    app.collect_game();
    compose_live(&mut app);
    assert!(!app.game.as_ref().unwrap().viewer.output_pending());
    assert_eq!(app.live_audio.as_ref().unwrap().editor.value(), "pcm32");
    assert_eq!(
        app.live_audio.as_ref().unwrap().message.as_deref(),
        Some(refusal)
    );
    assert_eq!(app.navigator.active_id(), scope);
    assert_eq!(
        app.game
            .as_ref()
            .unwrap()
            .viewer
            .output_capability()
            .unwrap(),
        Some(initial.clone())
    );
    assert_paused_owner(&app, play, &launch);
    app.back();
    app.open_live_audio();
    assert_ne!(app.navigator.active_id(), scope);
    assert_eq!(
        app.live_audio.as_ref().unwrap().values.native_args(),
        initial.settings().unwrap().native_args()
    );
    assert_eq!(
        value(
            &app.live_audio.as_ref().unwrap().values.native_args(),
            "--encoding"
        ),
        Some("pcm16")
    );
    assert_paused_owner(&app, play, &launch);
}

#[test]
fn output_encoding_live_blank_edit_preserves_applied_encoding_on_real_mapper() {
    let current = native_request(OutputEncoding::Pcm24);
    let initial = alsa_ui::capability(&applied(&current)).unwrap();
    let (mut app, publisher) = live_fixture(initial);
    app.open_live_audio();
    let index = edit_live(&mut app, "--encoding", "");
    app.apply_live_audio();
    let request = publisher.take_output_request().unwrap().unwrap();
    assert_eq!(value(&request.args, "--encoding"), None);
    let mapped = alsa_ui::request_for_args(&current, &request.args).unwrap();
    assert_eq!(mapped.format, current.format);
    let canonical = alsa_ui::capability(&applied(&mapped)).unwrap();
    publisher
        .reply_output(&OutputReply {
            id: request.id,
            result: Ok(canonical),
        })
        .unwrap();
    app.collect_game();
    assert_eq!(app.live_audio.as_ref().unwrap().selected, index);
    assert_eq!(app.live_audio.as_ref().unwrap().editor.value(), "pcm24");
}
