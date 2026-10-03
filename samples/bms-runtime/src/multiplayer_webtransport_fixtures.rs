//! Deferred adapter fixtures: configuration and real in-memory streams, never sockets.
use crate::{
    multiplayer_protocol::{encode_frame, FrameDecoder},
    multiplayer_webtransport::{
        relay_pair, tls_from_pem, AdmissionError, ConfigError, ServerOptions,
    },
};
use std::{future::Future, path::Path, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn options() -> Vec<String> {
    [
        "--bind",
        "127.0.0.1:4433",
        "--cert",
        "証明/server cert.pem",
        "--key",
        "秘密/server key.pem",
        "--origin",
        "https://rhythm.example",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn replace(flag: &str, value: &str) -> Vec<String> {
    let mut args = options();
    let index = args.iter().position(|arg| arg == flag).unwrap();
    args[index + 1] = value.into();
    args
}

fn with_options(extra: &[&str]) -> Vec<String> {
    let mut args = options();
    args.extend(extra.iter().map(|value| (*value).to_owned()));
    args
}

fn deferred_io(future: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(5), future)
                .await
                .expect("in-memory fixture exceeded its outer cleanup deadline");
        });
}

#[test]
fn configuration_preserves_explicit_endpoints_and_bounded_policy_without_opening_paths() {
    let defaults = ServerOptions::parse(&options()).unwrap();
    assert_eq!(
        defaults.bind,
        "127.0.0.1:4433".parse::<std::net::SocketAddr>().unwrap()
    );
    assert_eq!(defaults.cert, Path::new("証明/server cert.pem"));
    assert_eq!(defaults.key, Path::new("秘密/server key.pem"));
    assert_eq!(defaults.origins, ["https://rhythm.example"]);
    assert!(!defaults.allow_missing_origin);
    assert_eq!((defaults.max_rooms, defaults.max_key_bytes), (64, 128));
    assert_eq!((defaults.max_sessions, defaults.max_setups), (128, 16));
    assert_eq!(defaults.waiting_ttl, Duration::from_secs(30));
    assert_eq!(defaults.setup_timeout, Duration::from_secs(10));
    assert_eq!(defaults.io_timeout, Duration::from_secs(10));

    let upper = ServerOptions::parse(&with_options(&[
        "--max-rooms",
        "4096",
        "--max-key-bytes",
        "1024",
        "--max-sessions",
        "8192",
        "--max-setups",
        "256",
        "--waiting-ms",
        "86400000",
        "--setup-ms",
        "60000",
        "--io-ms",
        "120000",
        "--allow-missing-origin",
        "--origin",
        "http://localhost:8080",
        "--origin",
        "http://127.3.4.5:8000",
        "--origin",
        "http://[::1]:8080",
    ]))
    .unwrap();
    assert_eq!((upper.max_rooms, upper.max_key_bytes), (4096, 1024));
    assert_eq!((upper.max_sessions, upper.max_setups), (8192, 256));
    assert_eq!(upper.waiting_ttl, Duration::from_secs(86_400));
    assert_eq!(upper.setup_timeout, Duration::from_secs(60));
    assert_eq!(upper.io_timeout, Duration::from_secs(120));
    assert!(upper.allow_missing_origin);
    assert_eq!(upper.origins.len(), 4);

    let lower = ServerOptions::parse(&with_options(&[
        "--max-rooms",
        "1",
        "--max-key-bytes",
        "1",
        "--max-sessions",
        "2",
        "--max-setups",
        "1",
        "--waiting-ms",
        "1",
        "--setup-ms",
        "1",
        "--io-ms",
        "1",
    ]))
    .unwrap();
    assert_eq!(lower.max_rooms, 1);
    assert_eq!(lower.io_timeout, Duration::from_millis(1));
}

#[test]
fn configuration_refuses_ambiguous_missing_and_out_of_range_inputs() {
    for flag in ["--bind", "--cert", "--key", "--origin"] {
        let mut args = options();
        let index = args.iter().position(|arg| arg == flag).unwrap();
        args.drain(index..index + 2);
        assert_eq!(
            ServerOptions::parse(&args).unwrap_err(),
            ConfigError::Missing(flag)
        );
    }
    assert_eq!(
        ServerOptions::parse(&with_options(&["--io-ms"])).unwrap_err(),
        ConfigError::Missing("--io-ms")
    );
    assert_eq!(
        ServerOptions::parse(&with_options(&["--unknown"])).unwrap_err(),
        ConfigError::UnknownOption
    );
    for extra in [
        vec!["--bind", "127.0.0.1:4434"],
        vec!["--max-rooms", "1", "--max-rooms", "2"],
        vec!["--allow-missing-origin", "--allow-missing-origin"],
    ] {
        assert!(matches!(
            ServerOptions::parse(&with_options(&extra)),
            Err(ConfigError::Duplicate(_))
        ));
    }
    for (flag, values) in [
        ("--max-rooms", &["0", "4097"][..]),
        ("--max-key-bytes", &["0", "1025"][..]),
        ("--max-sessions", &["1", "8193"][..]),
        ("--max-setups", &["0", "257"][..]),
        ("--waiting-ms", &["0", "86400001"][..]),
        ("--setup-ms", &["0", "60001"][..]),
        (
            "--io-ms",
            &["0", "120001", "-1", "18446744073709551616"][..],
        ),
    ] {
        for value in values {
            assert!(
                ServerOptions::parse(&with_options(&[flag, value])).is_err(),
                "{flag} {value}"
            );
        }
    }
    assert!(
        ServerOptions::parse(&with_options(&["--max-sessions", "2", "--max-setups", "3"])).is_err()
    );
    for (flag, value) in [
        ("--bind", "host.example:4433"),
        ("--cert", ""),
        ("--key", "a\0b"),
    ] {
        assert!(ServerOptions::parse(&replace(flag, value)).is_err());
    }
    for origin in [
        "http://rhythm.example",
        "http://localhost.example:8080",
        "http://127.0.0.1.example",
        "https://rhythm.example/",
        "https://rhythm.example/path",
        "https://rhythm.example?x",
        "https://rhythm.example#",
        "https://user@rhythm.example",
        "https://RHYTHM.example",
        "https://rhythm.example:443",
        "https://",
        "https://rhythm.example\n",
    ] {
        assert!(
            ServerOptions::parse(&replace("--origin", origin)).is_err(),
            "{origin:?}"
        );
    }
    let mut args = options();
    for index in 1..16 {
        args.extend(["--origin".into(), format!("https://room{index}.example")]);
    }
    assert_eq!(ServerOptions::parse(&args).unwrap().origins.len(), 16);
    args.extend(["--origin".into(), "https://room16.example".into()]);
    assert!(ServerOptions::parse(&args).is_err());
    assert!(ServerOptions::parse(&with_options(&["--origin", "https://rhythm.example"])).is_err());
}

#[test]
fn request_admission_requires_exact_room_and_origin_with_explicit_native_opt_in() {
    let config = ServerOptions::parse(&with_options(&[
        "--max-key-bytes",
        "4",
        "--origin",
        "http://localhost:8080",
    ]))
    .unwrap();
    let path = String::from("/rooms/A_-9");
    let key = config
        .request_room(&path, Some("https://rhythm.example"))
        .unwrap();
    assert_eq!(key, "A_-9");
    assert_eq!(key.as_ptr(), path[7..].as_ptr());
    assert_eq!(
        config.request_room("/rooms/a", Some("http://localhost:8080")),
        Ok("a")
    );
    for path in [
        "/rooms/",
        "/rooms/abcde",
        "/rooms/a/",
        "/rooms/a/b",
        "/rooms/%41",
        "/rooms/a?x",
        "/rooms/a#x",
        "/rooms/한",
        "/rooms/a.b",
        "/Rooms/a",
    ] {
        assert_eq!(
            config.request_room(path, Some("https://rhythm.example")),
            Err(AdmissionError::InvalidPath),
            "{path}"
        );
    }
    for origin in [
        None,
        Some("null"),
        Some("https://other.example"),
        Some("https://rhythm.example/"),
        Some("https://RHYTHM.example"),
        Some("https://rhythm.example:443"),
    ] {
        assert_eq!(
            config.request_room("/rooms/a", origin),
            Err(AdmissionError::ForbiddenOrigin)
        );
    }
    let native = ServerOptions::parse(&with_options(&["--allow-missing-origin"])).unwrap();
    assert_eq!(native.request_room("/rooms/A", None), Ok("A"));
    assert_eq!(
        native.request_room("/rooms/A", Some("https://other.example")),
        Err(AdmissionError::ForbiddenOrigin)
    );
    let mut missing = options();
    missing.truncate(6);
    missing.push("--allow-missing-origin".into());
    assert!(ServerOptions::parse(&missing).is_err());
}

#[test]
fn tls_preparation_rejects_incomplete_wrong_label_and_oversized_material_without_io() {
    let certificate = b"-----BEGIN CERTIFICATE-----\nMAA=\n-----END CERTIFICATE-----\n";
    let private_key = b"-----BEGIN PRIVATE KEY-----\nMAA=\n-----END PRIVATE KEY-----\n";
    for (cert, key) in [
        (&b""[..], &private_key[..]),
        (&certificate[..], &b""[..]),
        (&b"not PEM"[..], &private_key[..]),
        (&b"-----BEGIN CERTIFICATE-----\nMAA="[..], &private_key[..]),
        (&private_key[..], &private_key[..]),
        (&certificate[..], &certificate[..]),
        (
            &certificate[..],
            &b"-----BEGIN PRIVATE KEY-----\n?\n-----END PRIVATE KEY-----"[..],
        ),
    ] {
        assert!(tls_from_pem(cert, key).is_err());
    }
    let too_large = vec![b' '; 1_048_577];
    assert!(tls_from_pem(&too_large, private_key).is_err());
    assert!(tls_from_pem(certificate, &too_large).is_err());
    let duplicate_keys = [private_key.as_slice(), private_key.as_slice()].concat();
    assert!(tls_from_pem(certificate, &duplicate_keys).is_err());
    let mut trailing = certificate.to_vec();
    trailing.extend_from_slice(b"unparsed trailing material");
    assert!(tls_from_pem(&trailing, private_key).is_err());
}

#[test]
fn real_relay_preserves_fragmented_and_coalesced_frames_under_duplex_backpressure() {
    deferred_io(async {
        let payload = vec![0xA5; 65_536];
        let first = encode_frame(0xFE, &payload).unwrap();
        let second = encode_frame(5, &[]).unwrap();
        let forward = [first.as_slice(), second.as_slice()].concat();
        let reverse = encode_frame(4, &[0, 255, 0, 128]).unwrap();
        let (left, relay_left) = tokio::io::duplex(31);
        let (right, relay_right) = tokio::io::duplex(17);
        let (mut left_read, mut left_write) = tokio::io::split(left);
        let (mut right_read, mut right_write) = tokio::io::split(right);
        let (a_read, a_write) = tokio::io::split(relay_left);
        let (b_read, b_write) = tokio::io::split(relay_right);
        let (_stop, cancel) = tokio::sync::watch::channel(false);
        let relay = relay_pair(
            a_read,
            a_write,
            b_read,
            b_write,
            Duration::from_secs(2),
            cancel,
        );
        let send_left = async {
            for fragment in forward.chunks(997) {
                left_write.write_all(fragment).await.unwrap();
            }
            left_write.shutdown().await.unwrap();
        };
        let send_right = async {
            right_write.write_all(&reverse).await.unwrap();
            right_write.shutdown().await.unwrap();
        };
        let receive_left = async {
            let mut actual = Vec::new();
            left_read.read_to_end(&mut actual).await.unwrap();
            assert_eq!(actual, reverse);
        };
        let receive_right = async {
            let mut actual = Vec::new();
            right_read.read_to_end(&mut actual).await.unwrap();
            assert_eq!(actual, forward);
            let mut decoder = FrameDecoder::new();
            let mut consumed = 0;
            while decoder.needed().unwrap() != 0 {
                consumed += decoder.push(&actual[consumed..]).unwrap();
            }
            assert_eq!(decoder.take().unwrap(), Some((0xFE, payload)));
            assert_eq!(&actual[consumed..], second);
        };
        let (result, (), (), (), ()) =
            tokio::join!(relay, send_left, send_right, receive_left, receive_right);
        let writers = result.unwrap();
        assert!(writers.deadline <= tokio::time::Instant::now() + Duration::from_secs(2));
        drop((writers.first, writers.second));
    });
}

#[test]
fn clean_eof_half_closes_forward_direction_but_delivers_reverse_ack_before_release() {
    deferred_io(async {
        let final_frame = encode_frame(3, b"final-prefix").unwrap();
        let ack = encode_frame(4, b"exact-final-ack").unwrap();
        let (left, relay_left) = tokio::io::duplex(8);
        let (right, relay_right) = tokio::io::duplex(8);
        let (mut left_read, mut left_write) = tokio::io::split(left);
        let (mut right_read, mut right_write) = tokio::io::split(right);
        let (a_read, a_write) = tokio::io::split(relay_left);
        let (b_read, b_write) = tokio::io::split(relay_right);
        let (_stop, cancel) = tokio::sync::watch::channel(false);
        let relay = relay_pair(
            a_read,
            a_write,
            b_read,
            b_write,
            Duration::from_secs(1),
            cancel,
        );
        let first_peer = async {
            left_write.write_all(&final_frame).await.unwrap();
            left_write.shutdown().await.unwrap();
            let mut actual = Vec::new();
            left_read.read_to_end(&mut actual).await.unwrap();
            assert_eq!(actual, ack);
        };
        let second_peer = async {
            let mut actual = Vec::new();
            right_read.read_to_end(&mut actual).await.unwrap();
            assert_eq!(actual, final_frame);
            // A peer ACK is sent only after observing the genuine forward EOF.
            right_write.write_all(&ack).await.unwrap();
            right_write.shutdown().await.unwrap();
        };
        let (result, (), ()) = tokio::join!(relay, first_peer, second_peer);
        assert!(result.is_ok());
    });
}

#[test]
fn rejected_frame_tail_cannot_erase_an_already_forwarded_prefix_or_wait_for_oversize_body() {
    deferred_io(async {
        let valid = encode_frame(5, &[]).unwrap();
        let mut bad_magic = encode_frame(5, &[]).unwrap();
        bad_magic[4] = b'X';
        let mut bad_version = encode_frame(5, &[]).unwrap();
        bad_version[8] = 7;
        let oversized_header = 65_544_u32.to_le_bytes().to_vec();
        let truncated_body = vec![7, 0, 0, 0, b'B'];
        for (tail, eof) in [
            (bad_magic, false),
            (bad_version, false),
            (oversized_header, false),
            (truncated_body, true),
        ] {
            let (mut left, relay_left) = tokio::io::duplex(128);
            let (mut right, relay_right) = tokio::io::duplex(128);
            let (a_read, a_write) = tokio::io::split(relay_left);
            let (b_read, b_write) = tokio::io::split(relay_right);
            let (_stop, cancel) = tokio::sync::watch::channel(false);
            left.write_all(&valid).await.unwrap();
            left.write_all(&tail).await.unwrap();
            if eof {
                left.shutdown().await.unwrap();
            }
            let relay = relay_pair(
                a_read,
                a_write,
                b_read,
                b_write,
                Duration::from_secs(4),
                cancel,
            );
            let receive = async {
                let mut actual = Vec::new();
                right.read_to_end(&mut actual).await.unwrap();
                actual
            };
            let (result, actual) = tokio::time::timeout(Duration::from_secs(1), async {
                tokio::join!(relay, receive)
            })
            .await
            .expect("invalid header must not await a body or the I/O timeout");
            assert!(result.is_err());
            assert_eq!(actual, valid);
        }
    });
}

#[test]
fn cancellation_and_whole_frame_read_or_write_deadlines_bound_owned_streams() {
    deferred_io(async {
        // An explicit cancellation interrupts idle streams without waiting for I/O expiry.
        let (_left, relay_left) = tokio::io::duplex(16);
        let (_right, relay_right) = tokio::io::duplex(16);
        let (a_read, a_write) = tokio::io::split(relay_left);
        let (b_read, b_write) = tokio::io::split(relay_right);
        let (stop, cancel) = tokio::sync::watch::channel(false);
        let relay = relay_pair(
            a_read,
            a_write,
            b_read,
            b_write,
            Duration::from_secs(4),
            cancel,
        );
        let cancel_owner = async {
            tokio::task::yield_now().await;
            stop.send(true).unwrap();
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(relay, cancel_owner)
        })
        .await
        .unwrap();
        assert!(result.is_err());

        // A quiet peer cannot retain the relay forever, even before the first header byte.
        for partial in [&b""[..], &b"\x07\x00"[..]] {
            let (mut left, relay_left) = tokio::io::duplex(16);
            let (_right, relay_right) = tokio::io::duplex(16);
            left.write_all(partial).await.unwrap();
            let (a_read, a_write) = tokio::io::split(relay_left);
            let (b_read, b_write) = tokio::io::split(relay_right);
            let (_stop, cancel) = tokio::sync::watch::channel(false);
            assert!(relay_pair(
                a_read,
                a_write,
                b_read,
                b_write,
                Duration::from_millis(20),
                cancel
            )
            .await
            .is_err());
        }

        // Full valid input with an unread one-byte destination exercises write backpressure.
        let (left, relay_left) = tokio::io::duplex(128);
        let (right, relay_right) = tokio::io::duplex(1);
        let (mut left_read, mut left_write) = tokio::io::split(left);
        let (mut right_read, mut right_write) = tokio::io::split(right);
        left_write
            .write_all(&encode_frame(5, &[]).unwrap())
            .await
            .unwrap();
        let (a_read, a_write) = tokio::io::split(relay_left);
        let (b_read, b_write) = tokio::io::split(relay_right);
        let (_stop, cancel) = tokio::sync::watch::channel(false);
        let relay = relay_pair(
            a_read,
            a_write,
            b_read,
            b_write,
            Duration::from_millis(20),
            cancel,
        );
        let heartbeat = async {
            for _ in 0..20 {
                if right_write
                    .write_all(&encode_frame(5, &[]).unwrap())
                    .await
                    .is_err()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        let drain_reverse = async {
            let mut bytes = Vec::new();
            left_read.read_to_end(&mut bytes).await.unwrap();
        };
        let (result, (), ()) = tokio::join!(relay, heartbeat, drain_reverse);
        assert!(result.is_err());
        let mut accepted_prefix = Vec::new();
        right_read.read_to_end(&mut accepted_prefix).await.unwrap();
        assert_eq!(accepted_prefix, encode_frame(5, &[]).unwrap()[..1]);

        // Continuing byte arrivals must not restart the whole-frame read deadline.
        let (left, relay_left) = tokio::io::duplex(16);
        let (right, relay_right) = tokio::io::duplex(16);
        let (mut left_read, mut left_write) = tokio::io::split(left);
        let (mut right_read, mut right_write) = tokio::io::split(right);
        let (a_read, a_write) = tokio::io::split(relay_left);
        let (b_read, b_write) = tokio::io::split(relay_right);
        let (_stop, cancel) = tokio::sync::watch::channel(false);
        let relay = relay_pair(
            a_read,
            a_write,
            b_read,
            b_write,
            Duration::from_millis(40),
            cancel,
        );
        let trickle = async {
            for byte in encode_frame(5, &[]).unwrap() {
                if left_write.write_all(&[byte]).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        // Keep reverse frame activity independent so an idle reverse reader cannot
        // accidentally supply the timeout being asserted for the trickling frame.
        let heartbeat = async {
            for _ in 0..20 {
                if right_write
                    .write_all(&encode_frame(5, &[]).unwrap())
                    .await
                    .is_err()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        let drain_reverse = async {
            let mut bytes = Vec::new();
            left_read.read_to_end(&mut bytes).await.unwrap();
        };
        let receive = async {
            let mut bytes = Vec::new();
            right_read.read_to_end(&mut bytes).await.unwrap();
            bytes
        };
        let (result, (), (), (), bytes) =
            tokio::join!(relay, trickle, heartbeat, drain_reverse, receive);
        assert!(result.is_err());
        assert!(bytes.is_empty());
    });
}
