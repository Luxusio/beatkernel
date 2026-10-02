//! QUIC credential and shared-option fixtures; no endpoint or socket is created.
use crate::{
    competition::OpponentKind,
    competition_live::{CompetitionOptions, NetworkRole},
    multiplayer_quic::QuicCredentials,
};
use std::{path::PathBuf, time::Duration};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
fn host() -> QuicCredentials {
    QuicCredentials {
        cert: Some(PathBuf::from("証明/server cert.pem")),
        key: Some(PathBuf::from("秘密/server key.pem")),
        ..Default::default()
    }
}
fn join() -> QuicCredentials {
    QuicCredentials {
        ca: Some(PathBuf::from("信頼/root CA.pem")),
        server_name: Some("rhythm.example".into()),
        ..Default::default()
    }
}

#[test]
fn role_validation_accepts_only_complete_role_credentials_without_reading_paths() {
    let host = host();
    let join = join();
    assert!(host.validate_for_role(true).is_ok());
    assert!(join.validate_for_role(false).is_ok());
    assert!(host.validate_for_role(false).is_err());
    assert!(join.validate_for_role(true).is_err());
    assert_eq!(
        host.cert.as_deref(),
        Some(std::path::Path::new("証明/server cert.pem"))
    );
    assert_eq!(
        host.key.as_deref(),
        Some(std::path::Path::new("秘密/server key.pem"))
    );
    assert_eq!(
        join.ca.as_deref(),
        Some(std::path::Path::new("信頼/root CA.pem"))
    );
    assert_eq!(join.server_name.as_deref(), Some("rhythm.example"));
    for (credentials, hosting) in [
        (QuicCredentials::default(), true),
        (QuicCredentials::default(), false),
        (
            QuicCredentials {
                key: None,
                ..host.clone()
            },
            true,
        ),
        (
            QuicCredentials {
                cert: None,
                ..host.clone()
            },
            true,
        ),
        (
            QuicCredentials {
                ca: None,
                ..join.clone()
            },
            false,
        ),
        (
            QuicCredentials {
                server_name: None,
                ..join.clone()
            },
            false,
        ),
        (
            QuicCredentials {
                ca: join.ca.clone(),
                ..host.clone()
            },
            true,
        ),
        (
            QuicCredentials {
                server_name: join.server_name.clone(),
                ..host.clone()
            },
            true,
        ),
        (
            QuicCredentials {
                cert: host.cert.clone(),
                ..join.clone()
            },
            false,
        ),
        (
            QuicCredentials {
                key: host.key.clone(),
                ..join.clone()
            },
            false,
        ),
    ] {
        assert!(credentials.validate_for_role(hosting).is_err());
    }
}

#[test]
fn credential_path_and_server_identity_bounds_reject_empty_nul_and_malformed_names() {
    let exact_path = format!("{}a", "音".repeat(1365));
    assert_eq!(exact_path.len(), 4096);
    assert!(
        QuicCredentials {
            cert: Some(exact_path.clone().into()),
            ..host()
        }
        .validate_for_role(true)
        .is_ok()
    );
    for invalid in [
        String::new(),
        "bad\0cert.pem".into(),
        format!("{exact_path}b"),
    ] {
        assert!(
            QuicCredentials {
                cert: Some(invalid.clone().into()),
                ..host()
            }
            .validate_for_role(true)
            .is_err()
        );
        assert!(
            QuicCredentials {
                key: Some(invalid.clone().into()),
                ..host()
            }
            .validate_for_role(true)
            .is_err()
        );
        assert!(
            QuicCredentials {
                ca: Some(invalid.into()),
                ..join()
            }
            .validate_for_role(false)
            .is_err()
        );
    }
    let longest = format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61)
    );
    assert_eq!(longest.len(), 253);
    for name in [
        "localhost",
        "rhythm.example",
        "127.0.0.1",
        "::1",
        longest.as_str(),
    ] {
        assert!(
            QuicCredentials {
                server_name: Some(name.into()),
                ..join()
            }
            .validate_for_role(false)
            .is_ok(),
            "rejected {name}"
        );
    }
    for name in [
        "",
        " ",
        "rhythm .example",
        "rhythm\n.example",
        "a\0b",
        "리듬.example",
        "-rhythm.example",
        "rhythm-.example",
        "rhythm..example",
        ".example",
        "https://rhythm.example",
        "127.0.0.1:443",
    ] {
        assert!(
            QuicCredentials {
                server_name: Some(name.into()),
                ..join()
            }
            .validate_for_role(false)
            .is_err(),
            "accepted {name:?}"
        );
    }
    for name in [format!("{longest}x"), format!("{}.example", "x".repeat(64))] {
        assert!(
            QuicCredentials {
                server_name: Some(name),
                ..join()
            }
            .validate_for_role(false)
            .is_err()
        );
    }
}

#[test]
fn parser_retains_native_values_and_ghost_paths_while_extracting_complete_host_trust() {
    let (options, native) = CompetitionOptions::extract(&args(&[
        "--chart",
        "曲/chart.bms",
        "--mp-key",
        "秘密/server key.pem",
        "--backend",
        "wasapi-exclusive",
        "--mp-host",
        "[::1]:50000",
        "--mp-cert",
        "証明/server cert.pem",
        "--profile",
        "--mp-ca",
        "--mp-timeout-ms",
        "321",
        "--ghost-other",
        "--mp-server-name",
        "--help",
    ]))
    .unwrap();
    assert_eq!(
        native,
        args(&[
            "--chart",
            "曲/chart.bms",
            "--backend",
            "wasapi-exclusive",
            "--profile",
            "--mp-ca",
            "--help"
        ])
    );
    assert_eq!(
        options.network,
        Some(NetworkRole::Host("[::1]:50000".parse().unwrap()))
    );
    assert_eq!(options.setup_timeout, Duration::from_millis(321));
    assert_eq!(options.quic.cert, host().cert);
    assert_eq!(options.quic.key, host().key);
    assert_eq!(options.quic.ca, None);
    assert_eq!(options.quic.server_name, None);
    assert!(options.quic.validate_for_role(true).is_ok());
    assert_eq!(
        options.ghosts,
        vec![(OpponentKind::Other, PathBuf::from("--mp-server-name"))]
    );
}

#[test]
fn parser_supports_join_and_incomplete_drafts_but_role_validation_never_grants_missing_trust() {
    let (options, rest) = CompetitionOptions::extract(&args(&[
        "--mp-server-name",
        "rhythm.example",
        "--mp-join",
        "127.0.0.1:50000",
        "--buffer-frames",
        "256",
        "--mp-ca",
        "信頼/root CA.pem",
    ]))
    .unwrap();
    assert_eq!(rest, args(&["--buffer-frames", "256"]));
    assert_eq!(
        options.network,
        Some(NetworkRole::Join("127.0.0.1:50000".parse().unwrap()))
    );
    assert_eq!(options.quic.ca, join().ca);
    assert_eq!(options.quic.server_name, join().server_name);
    assert!(options.quic.validate_for_role(false).is_ok());
    for (values, hosting) in [
        (vec!["--mp-host", "127.0.0.1:50000"], true),
        (
            vec!["--mp-host", "127.0.0.1:50000", "--mp-cert", "server.pem"],
            true,
        ),
        (
            vec!["--mp-key", "key.pem", "--mp-host", "127.0.0.1:50000"],
            true,
        ),
        (vec!["--mp-join", "127.0.0.1:50000"], false),
        (
            vec!["--mp-ca", "ca.pem", "--mp-join", "127.0.0.1:50000"],
            false,
        ),
        (
            vec![
                "--mp-join",
                "127.0.0.1:50000",
                "--mp-server-name",
                "rhythm.example",
            ],
            false,
        ),
    ] {
        let (draft, rest) = CompetitionOptions::extract(&args(&values)).unwrap();
        assert!(rest.is_empty());
        assert!(draft.quic.validate_for_role(hosting).is_err());
    }
    let (offline, rest) =
        CompetitionOptions::extract(&args(&["--backend", "asio", "--chart", "--mp-cert"])).unwrap();
    assert_eq!(rest, args(&["--backend", "asio", "--chart", "--mp-cert"]));
    assert!(offline.network.is_none());
    assert!(
        offline.quic.cert.is_none()
            && offline.quic.key.is_none()
            && offline.quic.ca.is_none()
            && offline.quic.server_name.is_none()
    );
}

#[test]
fn parser_rejects_duplicate_empty_missing_role_and_cross_role_credentials() {
    for (flag, value, role) in [
        ("--mp-cert", "server.pem", "--mp-host"),
        ("--mp-key", "key.pem", "--mp-host"),
        ("--mp-ca", "ca.pem", "--mp-join"),
        ("--mp-server-name", "rhythm.example", "--mp-join"),
    ] {
        for values in [
            vec![role, "127.0.0.1:50000", flag, value, flag, value],
            vec![role, "127.0.0.1:50000", flag, ""],
            vec![role, "127.0.0.1:50000", flag],
            vec![flag, value],
        ] {
            assert!(
                CompetitionOptions::extract(&args(&values)).is_err(),
                "accepted {values:?}"
            );
        }
        let opposite = if role == "--mp-host" {
            "--mp-join"
        } else {
            "--mp-host"
        };
        assert!(
            CompetitionOptions::extract(&args(&[opposite, "127.0.0.1:50000", flag, value]))
                .is_err()
        );
    }
    for values in [
        vec![
            "--mp-host",
            "127.0.0.1:50000",
            "--mp-join",
            "127.0.0.1:50001",
        ],
        vec![
            "--mp-host",
            "127.0.0.1:50000",
            "--mp-cert",
            "cert.pem",
            "--mp-key",
            "key.pem",
            "--mp-ca",
            "ca.pem",
        ],
        vec![
            "--mp-join",
            "127.0.0.1:50000",
            "--mp-ca",
            "ca.pem",
            "--mp-server-name",
            "rhythm.example",
            "--mp-key",
            "key.pem",
        ],
    ] {
        assert!(CompetitionOptions::extract(&args(&values)).is_err());
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn actual_tls_configuration_rejects_malformed_der_pem_and_missing_key_material_without_binding() {
    use crate::multiplayer_quic::{client_config, server_config};
    let invalid_certificates: &[&[u8]] = &[
        b"",
        b"not a certificate",
        &[0x30, 0x82, 0xff, 0xff],
        b"-----BEGIN CERTIFICATE-----\n%%%\n-----END CERTIFICATE-----\n",
        b"-----BEGIN CERTIFICATE-----\nMAA=\n-----END CERTIFICATE-----\n",
        b"-----BEGIN CERTIFICATE-----\nMAA=\n",
        b"-----BEGIN PRIVATE KEY-----\nMAA=\n-----END PRIVATE KEY-----\n",
    ];
    for certificate in invalid_certificates {
        assert!(client_config(certificate, "rhythm.example").is_err());
        assert!(server_config(certificate, b"not a private key").is_err());
    }
    for key in [
        b"".as_slice(),
        b"-----BEGIN PRIVATE KEY-----\n%%%\n-----END PRIVATE KEY-----\n".as_slice(),
        b"-----BEGIN PRIVATE KEY-----\nMAA=\n-----END PRIVATE KEY-----\n".as_slice(),
        b"-----BEGIN CERTIFICATE-----\nMAA=\n-----END CERTIFICATE-----\n".as_slice(),
    ] {
        assert!(
            server_config(
                b"-----BEGIN CERTIFICATE-----\nMAA=\n-----END CERTIFICATE-----\n",
                key
            )
            .is_err()
        );
    }
    // Helpers used by actual endpoints have no bind/connect stage, including on failure.
    for name in ["", "a b", "https://rhythm.example", "bad\0name"] {
        assert!(client_config(b"", name).is_err());
    }
}
