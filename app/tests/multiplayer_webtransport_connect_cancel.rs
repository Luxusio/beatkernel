//! Actual pending setup cancellation before TLS/HTTP/3 admission or a room lease.
//! Credentials come from the coordinator; each case owns its silent UDP sink.
#![cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]

use beatkernel::{
    input::CodecLimits,
    replay::{codec::ReplayCodecLimits, ReplayHeader, REPLAY_VERSION},
    time::ClockDomainId,
};
use beatkernel_bms_runtime::{
    local_players::PlayerId,
    multiplayer::competition_identity,
    multiplayer_start::{StartPolicy, StartRole},
    multiplayer_webtransport_client::{WebTransportEndpoint, WebTransportOptions},
};
use std::{
    env, io,
    net::{Ipv4Addr, UdpSocket},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug)]
enum Setup {
    Admission,
    Play,
}

fn identity() -> Vec<u8> {
    competition_identity(
        &ReplayHeader {
            version: REPLAY_VERSION,
            chart_identity: b"webtransport-pending-test-chart".to_vec(),
            rules_identity: b"webtransport-pending-test-rules".to_vec(),
            options: b"webtransport-pending-test-options".to_vec(),
            seed: 41,
            normalized_clock: ClockDomainId(17),
        },
        "webtransport-pending-test-runtime",
        ReplayCodecLimits::new(65_536, 1, 4096, CodecLimits::new(64, 0).unwrap()).unwrap(),
    )
    .unwrap()
}

fn cancel_pending_setup(setup: Setup, case: &str) {
    // Keep the destination open but never reply. The real client must emit its
    // QUIC Initial before cancellation; this sink cannot admit an application.
    let sink = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    sink.set_read_timeout(Some(WAIT)).unwrap();
    let config = WebTransportOptions {
        url: format!(
            "https://127.0.0.1:{}/rooms/pending_{}_{case}",
            sink.local_addr().unwrap().port(),
            std::process::id(),
        ),
        origin: env::var("BEATKERNEL_TEST_WEBTRANSPORT_ORIGIN")
            .expect("pending setup integration requires BEATKERNEL_TEST_WEBTRANSPORT_ORIGIN"),
        ca: PathBuf::from(
            env::var_os("BEATKERNEL_TEST_QUIC_CA")
                .expect("pending setup integration requires BEATKERNEL_TEST_QUIC_CA"),
        ),
        role: StartRole::Join,
    };
    let endpoint = WebTransportEndpoint::prepare(&config)
        .expect("pending setup requires normally prepared explicit CA trust and Origin");
    let identity = identity();
    let players = [PlayerId(11), PlayerId(12)];
    let stop = AtomicBool::new(false);
    let deadline = Instant::now() + WAIT;
    let mut packet = [0u8; 2048];

    let (received, before_cancel, completed, joined) = thread::scope(|scope| {
        let (completion_tx, completion_rx) = mpsc::channel();
        let stop_ref = &stop;
        let worker = scope.spawn(move || {
            let result = match setup {
                Setup::Admission => endpoint
                    .connect_room(&identity, &players, stop_ref, deadline)
                    .map(drop),
                Setup::Play => endpoint
                    .connect_room_play(
                        &identity,
                        &players,
                        StartPolicy::default(),
                        25_000_000,
                        stop_ref,
                        deadline,
                    )
                    .map(drop),
            };
            // Preserve the actual endpoint error as the joined result. A marker
            // also exposes completion that was published before cancellation.
            let _ = completion_tx.send(());
            result
        });

        let received = sink.recv_from(&mut packet);
        let before_cancel = completion_rx.try_recv();
        // No controller assertion or error propagation may precede this pair.
        // The original endpoint deadline and outer test process bound also apply
        // when the receive or bounded completion observation fails.
        stop.store(true, Ordering::Release);
        let completed =
            completion_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        let joined = worker.join();
        (received, before_cancel, completed, joined)
    });
    drop(sink);

    let result = joined.expect("pending setup worker panicked and was joined");
    let (received, _) = received.unwrap_or_else(|error| {
        panic!(
            "{setup:?} QUIC Initial receive failed ({:?}: {error}); endpoint result: {result:?}",
            error.kind(),
        )
    });
    assert!(
        received >= 1200,
        "{setup:?} real QUIC Initial was only {received} bytes; endpoint result: {result:?}"
    );
    assert_eq!(
        packet[0] & 0xc0,
        0xc0,
        "{setup:?} received packet lacks a QUIC long header; endpoint result: {result:?}"
    );
    assert!(
        matches!(before_cancel, Err(TryRecvError::Empty)),
        "{setup:?} completed before cancellation: {before_cancel:?}; endpoint result: {result:?}"
    );
    assert!(
        completed.is_ok(),
        "{setup:?} bounded completion observation failed: {completed:?}; endpoint result: {result:?}"
    );
    let error = result.expect_err("pending room setup returned an application owner");
    assert_eq!(
        error.kind(),
        io::ErrorKind::Interrupted,
        "{setup:?} must return actual cancellation, preserving original error: {error:?}: {error}"
    );
}

#[test]
#[ignore = "requires ephemeral valid CA/Origin and actual local UDP execution"]
fn real_pending_room_admission_setup_cancels_after_quic_initial_and_joins() {
    cancel_pending_setup(Setup::Admission, "admission");
}

#[test]
#[ignore = "requires ephemeral valid CA/Origin and actual local UDP execution"]
fn real_pending_room_play_setup_cancels_after_quic_initial_and_joins() {
    cancel_pending_setup(Setup::Play, "play");
}
