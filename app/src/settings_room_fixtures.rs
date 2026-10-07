//! Deferred settings/profile source fixtures; no profile file or endpoint opens.
use super::*;
use crate::{
    competition_live::{CompetitionOptions, NetworkRole},
    presentation_settings::PresentationSettings,
    settings_profile::{
        PlayerProfile, decode_player_profile, decode_profile, encode_player_profile, encode_profile,
    },
};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.chunks_exact(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

#[test]
fn room_selection_roundtrips_native_and_combined_profiles_on_all_three_hosts() {
    for host in [
        SettingsHost::Windows,
        SettingsHost::Linux,
        SettingsHost::Macos,
    ] {
        let supplied = args(&[
            "--mp-room",
            "https://relay.example/room/team-A",
            "--mp-origin",
            "https://player.example",
            "--mp-ca",
            "trust directory/room.pem",
            "--bind",
            "11:04",
            "--start-ns",
            "604800000000001",
        ]);
        let model = NativeSettings::from_args(&supplied, host).unwrap();
        let expected = model.native_args();
        assert_eq!(
            value(&expected, "--mp-room"),
            Some("https://relay.example/room/team-A")
        );
        assert_eq!(value(&expected, "--mp-role"), None);
        let restored = decode_profile(&encode_profile(&model, host).unwrap(), host).unwrap();
        assert_eq!(restored.native_args(), expected);
        let profile = PlayerProfile {
            native: restored,
            presentation: PresentationSettings::default(),
        };
        let bytes = encode_player_profile(&profile, host).unwrap();
        let restored = decode_player_profile(&bytes, host).unwrap();
        assert_eq!(restored.native.native_args(), expected);
        let (competition, remainder) = CompetitionOptions::extract(&expected).unwrap();
        assert_eq!(
            competition.network,
            Some(NetworkRole::RoomWebTransport {
                url: "https://relay.example/room/team-A".into(),
                origin: "https://player.example".into(),
            })
        );
        assert_eq!(
            competition.quic.ca.as_deref(),
            Some(std::path::Path::new("trust directory/room.pem"))
        );
        assert_eq!(value(&remainder, "--start-ns"), Some("604800000000001"));
        assert_eq!(value(&remainder, "--bind"), Some("11:04"));
        let mut repeated = supplied.clone();
        repeated.extend(args(&["--mp-room", "https://other.example/room/x"]));
        assert!(NativeSettings::from_args(&repeated, host).is_err());
    }
}

#[test]
fn transport_overrides_replace_the_entire_room_bilateral_and_raw_credential_family() {
    for host in [
        SettingsHost::Windows,
        SettingsHost::Linux,
        SettingsHost::Macos,
    ] {
        let common = args(&["--bind", "11:04", "--ghost-self", "saved.bkr"]);
        let modes = [
            args(&[
                "--mp-room",
                "https://relay.example/room/team",
                "--mp-origin",
                "https://player.example",
                "--mp-ca",
                "room.pem",
            ]),
            args(&[
                "--mp-webtransport",
                "https://relay.example/room/pair",
                "--mp-role",
                "join",
                "--mp-origin",
                "https://pair.example",
                "--mp-ca",
                "pair.pem",
            ]),
            args(&[
                "--mp-join",
                "127.0.0.1:4433",
                "--mp-ca",
                "raw.pem",
                "--mp-server-name",
                "raw.example",
            ]),
            args(&[
                "--mp-host",
                "127.0.0.1:4434",
                "--mp-cert",
                "host.pem",
                "--mp-key",
                "host.key",
            ]),
        ];
        let family = [
            "--mp-room",
            "--mp-webtransport",
            "--mp-role",
            "--mp-origin",
            "--mp-host",
            "--mp-join",
            "--mp-cert",
            "--mp-key",
            "--mp-ca",
            "--mp-server-name",
        ];
        for from in &modes {
            let mut base = common.clone();
            base.extend(from.clone());
            for to in &modes {
                let merged = overlay_native_args(&base, to, host).unwrap();
                for flag in family {
                    assert_eq!(value(&merged, flag), value(to, flag), "{flag}");
                }
                assert_eq!(value(&merged, "--bind"), Some("11:04"));
                assert_eq!(value(&merged, "--ghost-self"), Some("saved.bkr"));
                let (actual, _) = CompetitionOptions::extract(&merged).unwrap();
                let (expected, _) = CompetitionOptions::extract(to).unwrap();
                assert_eq!(actual.network, expected.network);
            }
            let cleared = overlay_native_args(&base, &args(&["--mp-room", ""]), host).unwrap();
            for flag in family {
                assert_eq!(value(&cleared, flag), None);
            }
            assert!(
                CompetitionOptions::extract(&cleared)
                    .unwrap()
                    .0
                    .network
                    .is_none()
            );
            assert_eq!(value(&cleared, "--ghost-self"), Some("saved.bkr"));
        }
    }
}
