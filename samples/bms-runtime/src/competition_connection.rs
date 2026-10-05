//! Connection metadata admission before opaque factory acquisition.
use crate::{
    local_players::PlayerId,
    multiplayer_configuration::{MultiplayerOptions, validate_options},
    multiplayer_group::validate_roster,
    multiplayer_protocol::{MultiplayerError, group_setup_size},
    webtransport_preparation::validate_metadata,
};
use std::net::SocketAddr;

/// Explicit bilateral role or multi-host room selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkRole {
    /// Listen on this address without silently selecting public interfaces.
    Host(SocketAddr),
    /// Join the supplied address without DNS or service discovery.
    Join(SocketAddr),
    /// Both relay participants connect as clients; this role controls the shared start.
    WebTransport {
        url: String,
        role: crate::multiplayer_start::StartRole,
        origin: String,
    },
    /// Room authority comes from actual admission order, not a chosen start role.
    RoomWebTransport { url: String, origin: String },
}

pub struct CompetitionConnectionRequest<'a> {
    pub role: &'a NetworkRole,
    pub identity: Vec<u8>,
    pub players: Vec<PlayerId>,
    pub options: MultiplayerOptions,
    pub room_available: bool,
}

pub trait CompetitionConnectionFactory {
    type Connection;
    type Error;
    fn create(
        &mut self,
        request: CompetitionConnectionRequest<'_>,
    ) -> Result<Self::Connection, Self::Error>;
}

pub enum ConnectionAcquireError<E> {
    Policy(MultiplayerError),
    Factory(E),
}

fn validate_request(request: &CompetitionConnectionRequest<'_>) -> Result<(), MultiplayerError> {
    let room = matches!(request.role, NetworkRole::RoomWebTransport { .. });
    if room && !request.room_available {
        return Err(MultiplayerError::Protocol(
            "native room mode requires the graphical player lobby".into(),
        ));
    }
    validate_options(&request.identity, &request.options)?;
    validate_roster(&request.players)?;
    if !room {
        group_setup_size(&request.identity, &request.players)?;
    }
    let credentials = &request.options.quic;
    match request.role {
        NetworkRole::Host(address) => {
            credentials
                .validate_for_role(true)
                .map_err(|error| MultiplayerError::Io(error.to_string()))?;
            if address.port() == 0 {
                return Err(MultiplayerError::Io(
                    "QUIC host requires a nonzero port".into(),
                ));
            }
        }
        NetworkRole::Join(address) => {
            credentials
                .validate_for_role(false)
                .map_err(|error| MultiplayerError::Io(error.to_string()))?;
            if address.port() == 0 || address.ip().is_unspecified() {
                return Err(MultiplayerError::Io("invalid QUIC join address".into()));
            }
        }
        NetworkRole::WebTransport { url, origin, .. }
        | NetworkRole::RoomWebTransport { url, origin } => {
            if credentials.cert.is_some()
                || credentials.key.is_some()
                || credentials.server_name.is_some()
            {
                return Err(MultiplayerError::InvalidOptions);
            }
            let ca = credentials
                .ca
                .as_deref()
                .ok_or(MultiplayerError::InvalidOptions)?;
            validate_metadata(url, origin, ca)
                .map_err(|error| MultiplayerError::Io(error.to_string()))?;
        }
    }
    Ok(())
}

pub fn acquire_connection<'a, F: CompetitionConnectionFactory>(
    factory: &mut F,
    request: CompetitionConnectionRequest<'a>,
) -> Result<F::Connection, ConnectionAcquireError<F::Error>> {
    validate_request(&request).map_err(ConnectionAcquireError::Policy)?;
    factory
        .create(request)
        .map_err(ConnectionAcquireError::Factory)
}

#[cfg(test)]
#[path = "competition_connection_fixtures.rs"]
mod fixtures;
