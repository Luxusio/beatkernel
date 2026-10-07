//! Deferred application-boundary fixtures; no endpoint or credentials are opened.
use super::*;
use crate::competition_live::CompetitionOptions;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn explicit_room_selection_preserves_native_arguments_and_rejects_transport_ambiguity() {
    let valid = args(&[
        "--mp-room",
        "https://relay.example/room/team",
        "--mp-origin",
        "https://player.example",
        "--mp-ca",
        "trust/room.pem",
        "--mp-timeout-ms",
        "120000",
    ]);
    let mut supplied = valid.clone();
    supplied.extend(args(&[
        "--bind",
        "11:04",
        "--chart",
        "--mp-room",
        "--ghost-other",
        "recorded.bkr",
    ]));
    let (options, native) = CompetitionOptions::extract(&supplied).unwrap();
    assert_eq!(
        options.network,
        Some(NetworkRole::RoomWebTransport {
            url: "https://relay.example/room/team".into(),
            origin: "https://player.example".into(),
        })
    );
    assert_eq!(options.setup_timeout, std::time::Duration::from_secs(120));
    assert_eq!(native, args(&["--bind", "11:04", "--chart", "--mp-room"]));
    assert_eq!(options.ghosts.len(), 1);
    for extra in [
        args(&["--mp-room", "https://other.example/room/other"]),
        args(&["--mp-host", "127.0.0.1:4433"]),
        args(&["--mp-join", "127.0.0.1:4433"]),
        args(&["--mp-webtransport", "https://relay.example/room/pair"]),
        args(&["--mp-role", "host"]),
        args(&["--mp-role", "join"]),
        args(&["--mp-cert", "host.pem"]),
        args(&["--mp-key", "host.key"]),
        args(&["--mp-server-name", "relay.example"]),
    ] {
        let mut mixed = valid.clone();
        mixed.extend(extra);
        assert!(CompetitionOptions::extract(&mixed).is_err());
    }
    for invalid in [
        args(&[
            "--mp-room",
            "https://relay.example/room/team",
            "--mp-origin",
            "https://player.example",
        ]),
        args(&[
            "--mp-room",
            "https://relay.example/room/team",
            "--mp-ca",
            "room.pem",
        ]),
        args(&[
            "--mp-room",
            "",
            "--mp-origin",
            "https://player.example",
            "--mp-ca",
            "room.pem",
        ]),
        args(&[
            "--mp-room",
            "https://relay.example/room/\n",
            "--mp-origin",
            "https://player.example",
            "--mp-ca",
            "room.pem",
        ]),
    ] {
        assert!(CompetitionOptions::extract(&invalid).is_err());
    }
    let mut oversized = valid.clone();
    oversized[1] = "x".repeat(4097);
    assert!(CompetitionOptions::extract(&oversized).is_err());
    let bilateral = args(&[
        "--mp-webtransport",
        "https://relay.example/room/pair",
        "--mp-role",
        "join",
        "--mp-origin",
        "https://player.example",
        "--mp-ca",
        "pair.pem",
    ]);
    assert!(matches!(
        CompetitionOptions::extract(&bilateral).unwrap().0.network,
        Some(NetworkRole::WebTransport {
            role: StartRole::Join,
            ..
        })
    ));
    assert!(
        CompetitionOptions::extract(&[])
            .unwrap()
            .0
            .network
            .is_none()
    );
}

#[test]
fn room_application_requires_attached_lobby_before_credentials_or_endpoint_acquisition() {
    assert!(!crate::player::attached());
    let role = NetworkRole::RoomWebTransport {
        url: "https://relay.example/room/team".into(),
        origin: "https://player.example".into(),
    };
    for count in [1usize, 2, 3, 4, 64] {
        let players: Vec<PlayerId> = (0..count)
            .map(|index| PlayerId(u32::MAX - index as u32))
            .collect();
        // Missing trust configuration and an unusable path both stay behind
        // interactive admission. No real native owner is created here.
        for ca in [
            None,
            Some(std::path::PathBuf::from("missing/never-open-room-ca.pem")),
        ] {
            let mut options = MultiplayerOptions::default();
            options.quic.ca = ca;
            let failure =
                NativeCompetitionNetwork::new(&role, vec![0, 255, 17], players.clone(), options)
                    .err()
                    .unwrap();
            assert!(matches!(failure, MultiplayerError::Protocol(ref message)
                if message.contains("graphical player lobby")));
        }
    }
    #[cfg(not(feature = "webtransport"))]
    {
        let (publisher, _viewer) = crate::player::channel();
        crate::player::with_publisher(publisher, || {
            let mut options = MultiplayerOptions::default();
            options.quic.ca = Some("missing/never-open-room-ca.pem".into());
            let failure = NativeCompetitionNetwork::new(&role, vec![1], vec![PlayerId(7)], options)
                .err()
                .unwrap();
            assert!(matches!(failure, MultiplayerError::Io(_)));
            Ok(())
        })
        .unwrap();
        assert!(!crate::player::attached());
    }
}
