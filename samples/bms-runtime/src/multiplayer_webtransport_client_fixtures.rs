//! Native client boundary fixtures. No fixture connects, binds, or reads credentials.
use crate::{
    competition::OpponentKind,
    competition_live::{CompetitionOptions, NetworkRole},
    multiplayer_start::StartRole,
    multiplayer_webtransport_client::WebTransportOptions,
    settings::{overlay_native_args, NativeSettings, SettingsHost},
};
use std::{path::PathBuf, time::Duration};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn relay_args(role: &str) -> Vec<String> {
    args(&[
        "--mp-webtransport",
        "https://relay.example:4433/rooms/A_-9",
        "--mp-role",
        role,
        "--mp-origin",
        "http://localhost:8080",
        "--mp-ca",
        "信頼/root CA.pem",
    ])
}

fn connection(role: StartRole) -> WebTransportOptions {
    WebTransportOptions {
        url: "https://relay.example:4433/rooms/A_-9".into(),
        origin: "http://localhost:8080".into(),
        ca: PathBuf::from("信頼/root CA.pem"),
        role,
    }
}

// Public historical certificate bundled in x509-parser 0.18.1, assets/certificate.pem
// (https://github.com/rusticata/x509-parser). Only trust-store construction is
// exercised: this expired public leaf is never used for a handshake or deployment.
const PUBLIC_CERTIFICATE: &[u8] = b"-----BEGIN CERTIFICATE-----
MIIFWzCCBEOgAwIBAgISAyBIAwu7NBD5CTxX8suDCMgFMA0GCSqGSIb3DQEBCwUA
MEoxCzAJBgNVBAYTAlVTMRYwFAYDVQQKEw1MZXQncyBFbmNyeXB0MSMwIQYDVQQD
ExpMZXQncyBFbmNyeXB0IEF1dGhvcml0eSBYMzAeFw0xOTA3MTIxMTEyMzBaFw0x
OTEwMTAxMTEyMzBaMB0xGzAZBgNVBAMTEmxpc3RzLmZvci1vdXIuaW5mbzCCASIw
DQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAMVoti34X46DaI2nX24C+aZ2Ofkm
hKbidiXiRTon1MLSMGl1oNW9MyRyYYCzP4j6DNKChJnr8ZnVShh2oZD+yHWP9lpn
XMGkbsUxejRMU9hnaAB50pXRIDAzavkVFCguFlJ8nKkv/Y1Avlw7tc2aZOd3lOZB
Er8gJ8mRDGqqsNU+Z12I6slEstzGMpsq6AewCVw4lMjdWWgugzUrxQTRAsG87on6
gOiQH2cMODN3L7Fq4KOLQIjb3/luQhAQhpdKmEGFLin3c+f5or3thCDuwwDtOU1l
Zf+8t9S8pZPLrZrIs6H2xjXqCRuUY7iRNbO18Ukc6rlDYhBj9LT+cpmBbHECAwEA
AaOCAmYwggJiMA4GA1UdDwEB/wQEAwIFoDAdBgNVHSUEFjAUBggrBgEFBQcDAQYI
KwYBBQUHAwIwDAYDVR0TAQH/BAIwADAdBgNVHQ4EFgQUJj2pvRtl3GloH3He6FX1
ds3X0VEwHwYDVR0jBBgwFoAUqEpqYwR93brm0Tm3pkVl7/Oo7KEwbwYIKwYBBQUH
AQEEYzBhMC4GCCsGAQUFBzABhiJodHRwOi8vb2NzcC5pbnQteDMubGV0c2VuY3J5
cHQub3JnMC8GCCsGAQUFBzAChiNodHRwOi8vY2VydC5pbnQteDMubGV0c2VuY3J5
cHQub3JnLzAdBgNVHREEFjAUghJsaXN0cy5mb3Itb3VyLmluZm8wTAYDVR0gBEUw
QzAIBgZngQwBAgEwNwYLKwYBBAGC3xMBAQEwKDAmBggrBgEFBQcCARYaaHR0cDov
L2Nwcy5sZXRzZW5jcnlwdC5vcmcwggEDBgorBgEEAdZ5AgQCBIH0BIHxAO8AdgAp
PFGWVMg5ZbqqUPxYB9S3b79Yeily3KTDDPTlRUf0eAAAAWvmGV7yAAAEAwBHMEUC
ICQL2Sm14aCMLxX9a9RbySgyBfichMRdbu6QA2Mbrl4eAiEA1vgJ7snqUWCgoqEE
3SEfK3ioMopzWBsPvG6LdCuCMRAAdQBvU3asMfAxGdiZAKRRFf93FRwR2QLBACkG
jbIImjfZEwAAAWvmGV9oAAAEAwBGMEQCIExGqw3Lo0nSCyUuTRf92FgGASwWYji5
UGnXuYnpJrAvAiBw8AWVag8fzZ4ogAhY9EFRNdLrUcBjStipL888vyuxKzANBgkq
hkiG9w0BAQsFAAOCAQEAF8BBLDvSWZg57B6aDtzfUTSGetCYs3k0vJqCJlL+Pz7/
UruCSsojQzp5R6jvvgYQ83MaIdwe2mgt+OCQB5v7ylctyBzBmYIw9nPnxEC7HlcJ
L2K/k5ZjJFRnv4kV1Si8+TIpEAV0ksf39KGKemG8kGi4GXV1v03zSv0p8aCarpuo
SKBJ4qlB0CvmS2MqV4KnzO0O2h0c/ZQ4jg7l53eiN7VPdRMMO1DRw+MaW6I/hEZp
+oZQ7hhKXgKUBvF4IGwyrfyIZ8AeWKG4IP98COgyRbz7qtrAVevRKCM0ZC2t04A2
Fcix40FKEeiE093Aj3cweMYxNLPgwgQP8Xu3kA5QEw==
-----END CERTIFICATE-----
";

#[test]
fn competition_extraction_keeps_native_token_order_and_separates_start_role_from_relay_url() {
    for (word, role) in [("host", StartRole::Host), ("join", StartRole::Join)] {
        let mut supplied = args(&["--chart", "--mp-role", "--rate", "44100"]);
        supplied.extend(relay_args(word));
        supplied.extend(args(&[
            "--ghost-self",
            "自分/replay.bkr",
            "--bind",
            "11:04",
            "--ghost-other",
            "他人/replay.bkr",
            "--mp-timeout-ms",
            "1500",
            "--mp-start-lead-ms",
            "2500",
            "--help",
        ]));
        let (parsed, rest) = CompetitionOptions::extract(&supplied).unwrap();
        assert_eq!(
            rest,
            args(&[
                "--chart",
                "--mp-role",
                "--rate",
                "44100",
                "--bind",
                "11:04",
                "--help"
            ])
        );
        assert_eq!(
            parsed.network,
            Some(NetworkRole::WebTransport {
                url: "https://relay.example:4433/rooms/A_-9".into(),
                role,
                origin: "http://localhost:8080".into(),
            })
        );
        assert_eq!(parsed.quic.ca, Some(PathBuf::from("信頼/root CA.pem")));
        assert!(
            parsed.quic.cert.is_none()
                && parsed.quic.key.is_none()
                && parsed.quic.server_name.is_none()
        );
        assert_eq!(parsed.setup_timeout, Duration::from_millis(1500));
        assert_eq!(parsed.start_policy.lead_ns, 2_500_000_000);
        assert_eq!(
            parsed.ghosts,
            vec![
                (OpponentKind::Own, PathBuf::from("自分/replay.bkr")),
                (OpponentKind::Other, PathBuf::from("他人/replay.bkr"))
            ]
        );
    }
}

#[test]
fn competition_mode_refusals_leave_missing_ca_as_a_draft_without_mixing_raw_credentials() {
    let mut draft = relay_args("host");
    draft.truncate(6);
    let (parsed, rest) = CompetitionOptions::extract(&draft).unwrap();
    assert!(rest.is_empty());
    assert!(parsed.quic.ca.is_none());
    assert!(matches!(
        parsed.network,
        Some(NetworkRole::WebTransport {
            role: StartRole::Host,
            ..
        })
    ));
    for flag in ["--mp-webtransport", "--mp-role", "--mp-origin"] {
        let mut missing = relay_args("host");
        let index = missing.iter().position(|arg| arg == flag).unwrap();
        missing.drain(index..index + 2);
        assert!(
            CompetitionOptions::extract(&missing).is_err(),
            "missing {flag}"
        );
    }
    for extra in [
        args(&["--mp-webtransport", "https://other.example/rooms/a"]),
        args(&["--mp-role", "join"]),
        args(&["--mp-origin", "https://other.example"]),
        args(&["--mp-ca", "other.pem"]),
        args(&["--mp-host", "127.0.0.1:4433"]),
        args(&["--mp-join", "127.0.0.1:4433"]),
        args(&["--mp-cert", "cert.pem"]),
        args(&["--mp-key", "key.pem"]),
        args(&["--mp-server-name", "relay.example"]),
    ] {
        let mut supplied = relay_args("host");
        supplied.extend(extra);
        assert!(CompetitionOptions::extract(&supplied).is_err());
    }
    for role in ["", "Host", "server", "client", " host", "join\n"] {
        assert!(
            CompetitionOptions::extract(&relay_args(role)).is_err(),
            "{role:?}"
        );
    }
    for flag in ["--mp-role", "--mp-origin", "--mp-webtransport"] {
        assert!(CompetitionOptions::extract(&args(&[flag])).is_err());
        let mut supplied = relay_args("join");
        let index = supplied.iter().position(|arg| arg == flag).unwrap();
        supplied[index + 1] = "x".repeat(4097);
        assert!(CompetitionOptions::extract(&supplied).is_err());
    }
    for raw in ["--mp-host", "--mp-join"] {
        for extra in ["--mp-role", "--mp-origin"] {
            assert!(
                CompetitionOptions::extract(&args(&[raw, "127.0.0.1:4433", extra, "host"]))
                    .is_err()
            );
        }
    }
}

#[test]
fn raw_quic_drafts_and_explicit_root_configuration_keep_their_existing_contract() {
    use crate::multiplayer_quic::{client_config, client_tls};
    let (host, _) = CompetitionOptions::extract(&args(&[
        "--mp-host",
        "127.0.0.1:4433",
        "--mp-cert",
        "証明/server.pem",
        "--mp-key",
        "秘密/server.pem",
    ]))
    .unwrap();
    assert_eq!(
        host.network,
        Some(NetworkRole::Host("127.0.0.1:4433".parse().unwrap()))
    );
    assert!(host.quic.validate_for_role(true).is_ok());
    let (join, _) = CompetitionOptions::extract(&args(&[
        "--mp-join",
        "[::1]:4433",
        "--mp-ca",
        "信頼/root.pem",
        "--mp-server-name",
        "relay.example",
    ]))
    .unwrap();
    assert_eq!(
        join.network,
        Some(NetworkRole::Join("[::1]:4433".parse().unwrap()))
    );
    assert!(join.quic.validate_for_role(false).is_ok());
    let (incomplete, _) =
        CompetitionOptions::extract(&args(&["--mp-join", "127.0.0.1:4433"])).unwrap();
    assert!(incomplete.quic.validate_for_role(false).is_err());
    let tls = client_tls(PUBLIC_CERTIFICATE, b"beatkernel-multiplayer/6").unwrap();
    assert_eq!(tls.alpn_protocols, [b"beatkernel-multiplayer/6".to_vec()]);
    assert!(!tls.enable_early_data);
    assert!(client_config(PUBLIC_CERTIFICATE, "relay.example").is_ok());
    assert!(client_config(PUBLIC_CERTIFICATE, "[invalid-name").is_err());
    assert!(client_config(&[], "relay.example").is_err());
}

#[test]
fn all_native_settings_hosts_retain_drafts_and_replace_the_entire_network_family_on_switch() {
    for (host, audio_flag, audio_value) in [
        (SettingsHost::Windows, "--device", "endpoint-id"),
        (SettingsHost::Linux, "--alsa", "hw:1"),
        (SettingsHost::Macos, "--device", "17"),
    ] {
        let kept = args(&[
            audio_flag,
            audio_value,
            "--chart-seed",
            "18446744073709551615",
            "--mp-timeout-ms",
            "1200",
            "--bind",
            "11:04",
        ]);
        let mut base = kept.clone();
        base.extend(args(&[
            "--mp-join",
            "127.0.0.1:4433",
            "--mp-ca",
            "old-trust.pem",
            "--mp-server-name",
            "old.example",
        ]));
        let mut web = relay_args("host");
        web.truncate(6); // No inherited raw-QUIC trust anchor is authorized.
        let merged = overlay_native_args(&base, &web, host).unwrap();
        let mut expected = kept.clone();
        expected.extend(web);
        assert_eq!(merged, expected);
        let mut draft = NativeSettings::from_args(&merged, host).unwrap();
        assert_eq!(draft.native_args(), merged);
        for flag in ["--mp-webtransport", "--mp-role", "--mp-origin", "--mp-ca"] {
            assert_eq!(
                draft.fields().iter().filter(|row| row.flag == flag).count(),
                1
            );
        }
        let url_row = draft
            .fields()
            .iter()
            .position(|row| row.flag == "--mp-webtransport")
            .unwrap();
        draft.set_value(url_row, "https://incomplete/").unwrap();
        assert_eq!(draft.fields()[url_row].value, "https://incomplete/");
        let (parsed, _) = CompetitionOptions::extract(&merged).unwrap();
        assert!(parsed.quic.ca.is_none());
        let trusted =
            overlay_native_args(&merged, &args(&["--mp-ca", "new-trust.pem"]), host).unwrap();
        let (parsed, _) = CompetitionOptions::extract(&trusted).unwrap();
        assert!(matches!(
            parsed.network,
            Some(NetworkRole::WebTransport { .. })
        ));
        assert_eq!(parsed.quic.ca, Some(PathBuf::from("new-trust.pem")));

        let raw_host = args(&[
            "--mp-host",
            "[::1]:4444",
            "--mp-cert",
            "host.pem",
            "--mp-key",
            "host.key",
        ]);
        let switched = overlay_native_args(&trusted, &raw_host, host).unwrap();
        let mut expected = kept.clone();
        expected.extend(raw_host);
        assert_eq!(switched, expected);
        let (parsed, _) = CompetitionOptions::extract(&switched).unwrap();
        assert!(parsed.quic.validate_for_role(true).is_ok());
        let raw_join = args(&["--mp-join", "127.0.0.1:5555"]);
        let switched = overlay_native_args(&switched, &raw_join, host).unwrap();
        let mut expected = kept;
        expected.extend(raw_join);
        assert_eq!(switched, expected);
        let (parsed, _) = CompetitionOptions::extract(&switched).unwrap();
        assert!(
            parsed.quic.cert.is_none()
                && parsed.quic.key.is_none()
                && parsed.quic.ca.is_none()
                && parsed.quic.server_name.is_none()
        );
    }
}

#[cfg(not(feature = "webtransport"))]
#[test]
fn disabled_feature_refuses_actual_native_admission_before_credential_or_socket_acquisition() {
    use crate::multiplayer::{Multiplayer, MultiplayerError, MultiplayerOptions};
    for role in [StartRole::Host, StartRole::Join] {
        let mut options = connection(role);
        options.ca = PathBuf::from("must-not-open-this-certificate.pem");
        assert_eq!(
            options.validate().unwrap_err().kind(),
            std::io::ErrorKind::Unsupported
        );
        let error = Multiplayer::webtransport(
            options,
            b"fixture identity".to_vec(),
            MultiplayerOptions::default(),
        )
        .err()
        .expect("disabled feature must not create a worker");
        match error {
            MultiplayerError::Io(message) => assert!(
                message.to_ascii_lowercase().contains("webtransport"),
                "{message}"
            ),
            other => panic!("expected explicit feature refusal, got {other:?}"),
        }
    }
}

#[cfg(feature = "webtransport")]
#[test]
fn client_url_admission_is_canonical_https_and_exact_bounded_room_identity() {
    use crate::multiplayer_webtransport_client::validate_url;
    for text in [
        "https://relay.example/rooms/A_-9",
        "https://127.0.0.1:4433/rooms/a",
        "https://[::1]:4433/rooms/a",
    ] {
        let parsed = validate_url(text).unwrap();
        assert_eq!(parsed.as_str(), text);
        assert_eq!(parsed.scheme(), "https");
        assert!(parsed.port_or_known_default().unwrap() > 0);
    }
    let longest_key = format!("https://relay.example/rooms/{}", "A".repeat(1024));
    assert!(validate_url(&longest_key).is_ok());
    assert!(validate_url(&(longest_key + "A")).is_err());
    for text in [
        "",
        "http://localhost:4433/rooms/a",
        "wss://relay.example/rooms/a",
        "https://relay.example:0/rooms/a",
        "https://relay.example:65536/rooms/a",
        "https://user:pass@relay.example/rooms/a",
        "https://relay.example/rooms/a?",
        "https://relay.example/rooms/a#",
        "https://relay.example/rooms/a/",
        "https://relay.example/rooms/a/b",
        "https://relay.example/rooms/",
        "https://relay.example/Rooms/a",
        "https://relay.example/rooms/a.b",
        "https://relay.example/rooms/한",
        "https://relay.example/rooms/%41",
        "https://relay.example/rooms/%2e%2e/a",
        "https://relay.example/rooms/../rooms/a",
        "https://RELAY.example/rooms/a",
        "https://relay.example:443/rooms/a",
        " https://relay.example/rooms/a",
        "https://relay.example/rooms/a\n",
        "https://relay.example\\rooms\\a",
    ] {
        assert!(validate_url(text).is_err(), "{text:?}");
    }
    assert!(validate_url(&format!("https://relay.example/rooms/{}", "a".repeat(4096))).is_err());
}

#[cfg(feature = "webtransport")]
#[test]
fn client_metadata_requires_explicit_origin_and_trust_for_either_start_role() {
    for role in [StartRole::Host, StartRole::Join] {
        let options = connection(role);
        assert!(options.validate().is_ok());
        assert_eq!(options.role, role);
        assert_eq!(options.url, connection(StartRole::Host).url);
        for origin in [
            "https://rhythm.example",
            "http://localhost:8080",
            "http://127.9.8.7:8000",
            "http://[::1]:8080",
        ] {
            let mut candidate = options.clone();
            candidate.origin = origin.into();
            assert!(candidate.validate().is_ok());
        }
        for origin in [
            "",
            "null",
            "http://rhythm.example",
            "http://localhost.example",
            "https://RHYTHM.example",
            "https://rhythm.example/",
            "https://rhythm.example:443",
            "https://u@rhythm.example",
            "https://rhythm.example?",
            "https://rhythm.example#",
        ] {
            let mut candidate = options.clone();
            candidate.origin = origin.into();
            assert!(candidate.validate().is_err(), "{origin:?}");
        }
        for ca in [
            PathBuf::new(),
            PathBuf::from("bad\0path"),
            PathBuf::from("a".repeat(4097)),
        ] {
            let mut candidate = options.clone();
            candidate.ca = ca;
            assert!(candidate.validate().is_err());
        }
        let mut boundary = options;
        boundary.ca = PathBuf::from("a".repeat(4096));
        assert!(boundary.validate().is_ok());
    }
}

#[cfg(feature = "webtransport")]
#[test]
fn genuine_webtransport_trust_setup_accepts_explicit_pem_or_der_and_rejects_partial_material() {
    use crate::{multiplayer_quic::client_tls, multiplayer_webtransport_client::client_config};
    use quinn::rustls::pki_types::{pem::PemObject, CertificateDer};
    let tls = client_tls(PUBLIC_CERTIFICATE, wtransport::tls::WEBTRANSPORT_ALPN).unwrap();
    assert_eq!(tls.alpn_protocols, [b"h3".to_vec()]);
    assert!(!tls.enable_early_data);
    assert!(client_config(PUBLIC_CERTIFICATE).is_ok());
    let der = CertificateDer::from_pem_slice(PUBLIC_CERTIFICATE).unwrap();
    assert!(client_config(der.as_ref()).is_ok());
    let mut chain = PUBLIC_CERTIFICATE.to_vec();
    chain.extend_from_slice(PUBLIC_CERTIFICATE);
    assert!(client_config(&chain).is_ok());
    for malformed in [
        &b""[..],
        &b"not a certificate"[..],
        &b"\x30\x00"[..],
        &b"-----BEGIN CERTIFICATE-----\n?\n-----END CERTIFICATE-----\n"[..],
        &b"-----BEGIN PRIVATE KEY-----\nMAA=\n-----END PRIVATE KEY-----\n"[..],
    ] {
        assert!(client_config(malformed).is_err());
    }
    let mut trailing = PUBLIC_CERTIFICATE.to_vec();
    trailing.extend_from_slice(b"unconsumed data");
    assert!(client_config(&trailing).is_err());
    let mut incomplete_chain = PUBLIC_CERTIFICATE.to_vec();
    incomplete_chain.extend_from_slice(b"-----BEGIN CERTIFICATE-----\nMAA=\n");
    assert!(client_config(&incomplete_chain).is_err());
    let mut wrong_label = PUBLIC_CERTIFICATE.to_vec();
    wrong_label
        .extend_from_slice(b"-----BEGIN PRIVATE KEY-----\nMAA=\n-----END PRIVATE KEY-----\n");
    assert!(client_config(&wrong_label).is_err());
    assert!(client_config(&vec![b' '; 1_048_577]).is_err());
}
