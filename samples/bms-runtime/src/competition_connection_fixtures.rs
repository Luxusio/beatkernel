use super::*;
use crate::{
    local_players::PlayerId, multiplayer_configuration::MultiplayerOptions,
    multiplayer_credentials::QuicCredentials, multiplayer_start::StartRole,
};
use std::{path::PathBuf, sync::Arc, time::Duration};

struct Opaque(Arc<u8>); // No Clone, Debug, Display or Error requirements.
struct Connection {
    role: usize,
    identity: Vec<u8>,
    players: Vec<PlayerId>,
    options: MultiplayerOptions,
    room_available: bool,
}
#[derive(Default)]
struct Factory {
    calls: usize,
    refusal: Option<Opaque>,
}
impl CompetitionConnectionFactory for Factory {
    type Connection = Connection;
    type Error = Opaque;
    fn create(&mut self, request: CompetitionConnectionRequest<'_>) -> Result<Connection, Opaque> {
        self.calls += 1;
        if let Some(error) = self.refusal.take() {
            return Err(error);
        }
        Ok(Connection {
            role: request.role as *const NetworkRole as usize,
            identity: request.identity,
            players: request.players,
            options: request.options,
            room_available: request.room_available,
        })
    }
}
fn host() -> NetworkRole {
    NetworkRole::Host("127.0.0.1:1234".parse().unwrap())
}
fn options(hosting: bool) -> MultiplayerOptions {
    MultiplayerOptions {
        quic: if hosting {
            QuicCredentials {
                cert: Some(PathBuf::from("cert.pem")),
                key: Some(PathBuf::from("key.pem")),
                ca: None,
                server_name: None,
            }
        } else {
            QuicCredentials {
                cert: None,
                key: None,
                ca: Some(PathBuf::from("ca.pem")),
                server_name: Some("peer.example".into()),
            }
        },
        ..MultiplayerOptions::default()
    }
}
fn request(role: &NetworkRole) -> CompetitionConnectionRequest<'_> {
    CompetitionConnectionRequest {
        role,
        identity: vec![11, 23, 47],
        players: vec![PlayerId(u32::MAX), PlayerId(7)],
        options: options(matches!(role, NetworkRole::Host(_))),
        room_available: true,
    }
}
fn refused(request: CompetitionConnectionRequest<'_>) {
    let mut factory = Factory::default();
    assert!(matches!(
        acquire_connection(&mut factory, request),
        Err(ConnectionAcquireError::Policy(_))
    ));
    assert_eq!(factory.calls, 0);
}
fn admitted(request: CompetitionConnectionRequest<'_>) -> Connection {
    let mut factory = Factory::default();
    let connection = match acquire_connection(&mut factory, request) {
        Ok(connection) => connection,
        _ => panic!("valid pure request must be handed to the factory once"),
    };
    assert_eq!(factory.calls, 1);
    connection
}

#[test]
fn identity_and_whole_roster_preflight_refuse_all_bad_members_before_factory_effects() {
    let role = host();
    let mut smallest = request(&role);
    smallest.identity = vec![1];
    assert_eq!(admitted(smallest).identity, [1]);
    for identity in [vec![], vec![1; 65_537]] {
        let mut request = request(&role);
        request.identity = identity;
        refused(request);
    }
    for players in [
        vec![],
        vec![PlayerId(0)],
        vec![PlayerId(7), PlayerId(7)],
        vec![PlayerId(u32::MAX), PlayerId(7), PlayerId(0)],
        vec![PlayerId(u32::MAX), PlayerId(7), PlayerId(7)],
        (1..=65).map(PlayerId).collect(),
    ] {
        let mut request = request(&role);
        request.players = players;
        refused(request);
    }
    for count in 1..=64 {
        let mut request = request(&role);
        request.players = (0..count).map(|i| PlayerId(u32::MAX - i * 17)).collect();
        let connection = admitted(request);
        assert_eq!(connection.players.len(), count as usize);
        assert_eq!(connection.players[0], PlayerId(u32::MAX));
    }
}

#[test]
fn bilateral_wire_boundary_includes_twelve_header_bytes_and_every_original_player_id() {
    let role = host();
    for count in 1..=64 {
        let players: Vec<_> = (0..count).map(|i| PlayerId(u32::MAX - i * 17)).collect();
        let identity_limit = 65_536 - 12 - 4 * players.len();
        let mut exact = request(&role);
        exact.players = players.clone();
        exact.identity = vec![29; identity_limit];
        assert_eq!(admitted(exact).identity.len(), identity_limit);
        let mut oversized = request(&role);
        oversized.players = players;
        oversized.identity = vec![29; identity_limit + 1];
        refused(oversized);
    }
}

#[test]
fn every_common_option_and_start_bound_is_preflighted_before_factory_creation() {
    let role = host();
    for case in 0..15 {
        let mut request = request(&role);
        match case {
            0 => request.options.setup_timeout = Duration::ZERO,
            1 => request.options.setup_timeout = Duration::from_millis(120_001),
            2 => request.options.io_stall_timeout = Duration::ZERO,
            3 => request.options.io_stall_timeout = Duration::from_millis(60_001),
            4 => request.options.queue_capacity = 0,
            5 => request.options.queue_capacity = 1025,
            6 => request.options.preroll_ns = -1,
            7 => request.options.start_policy.min_remaining_ns = 0,
            8 => {
                request.options.start_policy.lead_ns = request.options.start_policy.min_remaining_ns
            }
            9 => {
                request.options.start_policy.lead_ns =
                    request.options.start_policy.min_remaining_ns - 1
            }
            10 => request.options.start_policy.lead_ns = u64::MAX,
            11 => request.options.start_policy.min_remaining_ns = u64::MAX,
            12 => request.options.start_policy.max_age_ns = u64::MAX,
            13 => request.options.start_policy.max_uncertainty_ns = u64::MAX,
            _ => request.options.start_policy.max_release_lateness_ns = u64::MAX,
        }
        refused(request);
    }
    for maximum in [false, true] {
        let mut request = request(&role);
        request.options.setup_timeout = Duration::from_millis(if maximum { 120_000 } else { 1 });
        request.options.io_stall_timeout = Duration::from_millis(if maximum { 60_000 } else { 1 });
        request.options.queue_capacity = if maximum { 1024 } else { 1 };
        request.options.preroll_ns = if maximum { i64::MAX } else { 0 };
        request.options.start_policy.max_release_lateness_ns = 0;
        admitted(request);
    }
}

#[test]
fn native_addresses_and_complete_role_credential_metadata_refuse_without_a_factory_call() {
    for role in [
        NetworkRole::Host("127.0.0.1:0".parse().unwrap()),
        NetworkRole::Join("127.0.0.1:0".parse().unwrap()),
        NetworkRole::Join("0.0.0.0:1234".parse().unwrap()),
        NetworkRole::Join("[::]:1234".parse().unwrap()),
    ] {
        refused(request(&role));
    }
    for hosting in [false, true] {
        let role = if hosting {
            host()
        } else {
            NetworkRole::Join("[::1]:1234".parse().unwrap())
        };
        for case in 0..7 {
            let mut request = request(&role);
            match (hosting, case) {
                (true, 0) => request.options.quic.cert = None,
                (true, 1) => request.options.quic.key = None,
                (true, 2) => request.options.quic.ca = Some("ca.pem".into()),
                (true, 3) => request.options.quic.server_name = Some("peer.example".into()),
                (true, 4) => request.options.quic.cert = Some(PathBuf::new()),
                (true, 5) => request.options.quic.key = Some(PathBuf::from("k\0ey")),
                (true, _) => request.options.quic.key = Some(PathBuf::from("k".repeat(4097))),
                (false, 0) => request.options.quic.ca = None,
                (false, 1) => request.options.quic.server_name = None,
                (false, 2) => request.options.quic.cert = Some("cert.pem".into()),
                (false, 3) => request.options.quic.key = Some("key.pem".into()),
                (false, 4) => request.options.quic.ca = Some(PathBuf::new()),
                (false, 5) => request.options.quic.server_name = Some("invalid_name".into()),
                (false, _) => request.options.quic.ca = Some(PathBuf::from("c".repeat(4097))),
            }
            refused(request);
        }
        admitted(request(&role));
    }
    let wildcard = NetworkRole::Host("0.0.0.0:1234".parse().unwrap());
    admitted(request(&wildcard));
}

#[test]
fn room_availability_is_the_first_refusal_even_with_invalid_identity_and_metadata() {
    let role = NetworkRole::RoomWebTransport {
        url: "https://relay.example/rooms/a".into(),
        origin: "https://ui.example".into(),
    };
    for malformed in [false, true] {
        let mut request = request(&role);
        request.options.quic.server_name = None;
        request.room_available = false;
        if malformed {
            request.identity.clear();
            request.players.clear();
            request.options.setup_timeout = Duration::ZERO;
            request.options.quic.ca = None;
        }
        let mut factory = Factory::default();
        assert!(matches!(
            acquire_connection(&mut factory, request),
            Err(ConnectionAcquireError::Policy(
                crate::multiplayer_protocol::MultiplayerError::Protocol(_)
            ))
        ));
        assert_eq!(factory.calls, 0);
    }
}

#[test]
fn admitted_requests_move_original_allocations_and_preserve_options_role_and_opaque_failure() {
    let role = host();
    let mut original = request(&role);
    original.room_available = false;
    original.options.queue_capacity = 17;
    original.options.preroll_ns = 604_800_000_000_001;
    original.options.setup_timeout = Duration::from_millis(71);
    original.options.io_stall_timeout = Duration::from_millis(53);
    let identity_pointer = original.identity.as_ptr();
    let roster_pointer = original.players.as_ptr();
    let expected_options = original.options.clone();
    let connection = admitted(original);
    assert_eq!(connection.identity.as_ptr(), identity_pointer);
    assert_eq!(connection.players.as_ptr(), roster_pointer);
    assert_eq!(connection.identity, [11, 23, 47]);
    assert_eq!(connection.players, [PlayerId(u32::MAX), PlayerId(7)]);
    assert_eq!(connection.role, &role as *const NetworkRole as usize);
    assert!(!connection.room_available);
    assert_eq!(
        connection.options.setup_timeout,
        expected_options.setup_timeout
    );
    assert_eq!(
        connection.options.io_stall_timeout,
        expected_options.io_stall_timeout
    );
    assert_eq!(
        connection.options.queue_capacity,
        expected_options.queue_capacity
    );
    assert_eq!(
        connection.options.start_policy,
        expected_options.start_policy
    );
    assert_eq!(connection.options.preroll_ns, expected_options.preroll_ns);
    assert_eq!(connection.options.quic.cert, expected_options.quic.cert);
    assert_eq!(connection.options.quic.key, expected_options.quic.key);
    assert_eq!(connection.options.quic.ca, expected_options.quic.ca);
    assert_eq!(
        connection.options.quic.server_name,
        expected_options.quic.server_name
    );
    let token = Arc::new(73);
    let mut factory = Factory {
        calls: 0,
        refusal: Some(Opaque(token.clone())),
    };
    match acquire_connection(&mut factory, request(&role)) {
        Err(ConnectionAcquireError::Factory(Opaque(actual))) => {
            assert!(Arc::ptr_eq(&actual, &token))
        }
        _ => panic!("factory refusal must retain its original opaque associated error"),
    }
    assert_eq!(factory.calls, 1);
}

#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
#[test]
fn supported_webtransport_roles_preflight_metadata_and_room_uses_full_identity_budget() {
    for role in [
        NetworkRole::WebTransport {
            url: "https://127.0.0.1:4433/rooms/a".into(),
            origin: "http://localhost:8080".into(),
            role: StartRole::Host,
        },
        NetworkRole::WebTransport {
            url: "https://[::1]:4433/rooms/a".into(),
            origin: "https://ui.example:0".into(),
            role: StartRole::Join,
        },
        NetworkRole::RoomWebTransport {
            url: "https://relay.example/rooms/a".into(),
            origin: "https://ui.example".into(),
        },
    ] {
        let mut valid = request(&role);
        valid.options.quic.server_name = None;
        admitted(valid);
        for case in 0..5 {
            let mut invalid = request(&role);
            invalid.options.quic.server_name = None;
            match case {
                0 => invalid.options.quic.cert = Some("cert".into()),
                1 => invalid.options.quic.key = Some("key".into()),
                2 => invalid.options.quic.server_name = Some("name.example".into()),
                3 => invalid.options.quic.ca = None,
                _ => invalid.options.quic.ca = Some(PathBuf::new()),
            }
            refused(invalid);
        }
    }
    for role in [
        NetworkRole::WebTransport {
            url: "http://relay.example/rooms/a".into(),
            origin: "https://ui.example".into(),
            role: StartRole::Host,
        },
        NetworkRole::RoomWebTransport {
            url: "https://relay.example/rooms/a".into(),
            origin: "http://ui.example".into(),
        },
    ] {
        let mut invalid = request(&role);
        invalid.options.quic.server_name = None;
        refused(invalid);
    }
    let room = NetworkRole::RoomWebTransport {
        url: "https://relay.example/rooms/a".into(),
        origin: "https://ui.example".into(),
    };
    let mut full = request(&room);
    full.options.quic.server_name = None;
    full.identity = vec![3; 65_536];
    full.players = (0..64).map(|i| PlayerId(u32::MAX - i * 17)).collect();
    assert_eq!(admitted(full).identity.len(), 65_536);
    let mut too_large = request(&room);
    too_large.options.quic.server_name = None;
    too_large.identity = vec![3; 65_537];
    refused(too_large);
}

#[cfg(not(all(not(target_arch = "wasm32"), feature = "webtransport")))]
#[test]
fn unsupported_webtransport_roles_refuse_before_factory_acquisition() {
    for role in [
        NetworkRole::WebTransport {
            url: "https://relay.example/rooms/a".into(),
            origin: "https://ui.example".into(),
            role: StartRole::Host,
        },
        NetworkRole::WebTransport {
            url: "https://relay.example/rooms/a".into(),
            origin: "https://ui.example".into(),
            role: StartRole::Join,
        },
        NetworkRole::RoomWebTransport {
            url: "https://relay.example/rooms/a".into(),
            origin: "https://ui.example".into(),
        },
    ] {
        let mut request = request(&role);
        request.options.quic.server_name = None;
        let mut factory = Factory::default();
        assert!(matches!(
            acquire_connection(&mut factory, request),
            Err(ConnectionAcquireError::Policy(
                crate::multiplayer_protocol::MultiplayerError::Io(_)
            ))
        ));
        assert_eq!(factory.calls, 0);
    }
}
