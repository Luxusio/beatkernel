//! Native application routing over existing bilateral and room owners.
//! Completion is marked only by the common native output/input completion gates.
use crate::{
    competition_live::NetworkRole,
    local_players::PlayerId,
    multiplayer::{GroupMultiplayer, MultiplayerError, MultiplayerNotice, MultiplayerOptions},
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_start::{StartPolicy, StartSchedule},
    multiplayer_webtransport_client::WebTransportOptions,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::{
    multiplayer::MultiplayerEvent,
    multiplayer_start::StartRole,
    native_room_competition::NativeRoomCompetition,
    native_room_network::{NativeRoomNetwork, NativeRoomOptions, NativeRoomOutcome},
    native_start::NativeStartAgreement,
};

enum Backend {
    Bilateral(GroupMultiplayer),
    #[cfg(not(target_arch = "wasm32"))]
    Room(NativeRoomCompetition),
}

pub struct NativeCompetitionNetwork {
    backend: Backend,
    policy: StartPolicy,
    native_completed: bool,
}
impl NativeCompetitionNetwork {
    pub fn new(
        role: &NetworkRole,
        identity: Vec<u8>,
        players: Vec<PlayerId>,
        options: MultiplayerOptions,
    ) -> Result<Self, MultiplayerError> {
        let policy = options.start_policy;
        let backend = match role {
            NetworkRole::Host(address) => Backend::Bilateral(GroupMultiplayer::host(
                *address, identity, players, options,
            )?),
            NetworkRole::Join(address) => Backend::Bilateral(GroupMultiplayer::join(
                *address, identity, players, options,
            )?),
            NetworkRole::WebTransport { url, role, origin } => {
                Backend::Bilateral(GroupMultiplayer::webtransport(
                    WebTransportOptions {
                        url: url.clone(),
                        origin: origin.clone(),
                        role: *role,
                        ca: options
                            .quic
                            .ca
                            .clone()
                            .ok_or(MultiplayerError::InvalidOptions)?,
                    },
                    identity,
                    players,
                    options,
                )?)
            }
            NetworkRole::RoomWebTransport { url, origin } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Interactive admission must exist before any credential or endpoint acquisition.
                    if !crate::player::attached() {
                        return Err(MultiplayerError::Protocol(
                            "native room mode requires the graphical player lobby".into(),
                        ));
                    }
                    if options.quic.cert.is_some()
                        || options.quic.key.is_some()
                        || options.quic.server_name.is_some()
                    {
                        return Err(MultiplayerError::InvalidOptions);
                    }
                    let transport = WebTransportOptions {
                        url: url.clone(),
                        origin: origin.clone(),
                        role: StartRole::Join,
                        ca: options
                            .quic
                            .ca
                            .clone()
                            .ok_or(MultiplayerError::InvalidOptions)?,
                    };
                    let settings = NativeRoomOptions {
                        setup_timeout: options.setup_timeout,
                        drain_timeout: options.io_stall_timeout,
                        finish_timeout: options.io_stall_timeout,
                        queue_capacity: options.queue_capacity,
                        start_policy: policy,
                        preroll_ns: options.preroll_ns,
                    };
                    let port =
                        NativeRoomNetwork::webtransport(transport, &identity, &players, settings)?;
                    let owner =
                        NativeRoomCompetition::new(port, players, options.io_stall_timeout)?;
                    return Ok(Self::from_room(owner, policy));
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = (url, origin, identity, players, options);
                    return Err(MultiplayerError::Io(
                        "native room mode is unavailable on WASM".into(),
                    ));
                }
            }
        };
        Ok(Self {
            backend,
            policy,
            native_completed: false,
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn from_room(owner: NativeRoomCompetition, policy: StartPolicy) -> Self {
        Self {
            backend: Backend::Room(owner),
            policy,
            native_completed: false,
        }
    }
    pub fn is_room(&self) -> bool {
        match &self.backend {
            Backend::Bilateral(_) => false,
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(_) => true,
        }
    }
    pub(crate) fn mark_native_completed(&mut self) {
        self.native_completed = true;
    }
    pub(crate) fn native_completed(&self) -> bool {
        self.native_completed
    }
    pub(crate) fn room_failed(&self) -> bool {
        match &self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(owner) => {
                owner.network_error().is_some()
                    || owner.snapshot().terminal.as_ref().is_some_and(|outcome| {
                        outcome.error.is_some() || outcome.cleanup_error.is_some()
                    })
            }
            Backend::Bilateral(_) => false,
        }
    }
    pub fn try_ready(&mut self) -> Result<(), MultiplayerError> {
        match &mut self.backend {
            Backend::Bilateral(owner) => owner.try_ready(),
            // Poll explicit UI requests; never infer readiness or Seal authority.
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(owner) => owner.poll().map_err(Into::into),
        }
    }
    pub fn is_ready(&self) -> bool {
        match &self.backend {
            Backend::Bilateral(owner) => owner.is_ready(),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(owner) => owner.committed_schedule().is_ok(),
        }
    }
    pub fn start_schedule(&self) -> Option<StartSchedule> {
        match &self.backend {
            Backend::Bilateral(owner) => owner.start_schedule(),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(owner) => owner.committed_schedule().ok(),
        }
    }
    pub fn start_policy(&self) -> StartPolicy {
        self.policy
    }
    pub fn clock_now_ns(&self) -> Result<i64, MultiplayerError> {
        match &self.backend {
            Backend::Bilateral(owner) => owner.clock_now_ns(),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(owner) => owner
                .clock_now_ns()
                .map_err(|error| MultiplayerError::Io(error.to_string())),
        }
    }
    pub fn poll(&mut self) -> Vec<MultiplayerNotice> {
        match &mut self.backend {
            Backend::Bilateral(owner) => owner.poll(),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(owner) => match owner.poll() {
                Ok(()) if owner.snapshot().terminal.is_some() => vec![MultiplayerNotice::Session(
                    MultiplayerEvent::Disconnected(MultiplayerError::Closed),
                )],
                Ok(()) => Vec::new(),
                Err(error) => vec![MultiplayerNotice::Session(MultiplayerEvent::Disconnected(
                    error.into(),
                ))],
            },
        }
    }
    pub fn try_publish(&mut self, members: Vec<MemberProgress>) -> Result<(), MultiplayerError> {
        match &mut self.backend {
            Backend::Bilateral(owner) => owner.try_publish(members),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(owner) => owner.observe(&members).map_err(Into::into),
        }
    }
    /// Actual room observation borrows the native prefix; its retained owner
    /// makes the single required copy before applying publication cadence.
    pub(crate) fn observe_room(
        &mut self,
        members: &[MemberProgress],
    ) -> Result<(), MultiplayerError> {
        match &mut self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(owner) => owner.observe(members).map_err(Into::into),
            Backend::Bilateral(_) => {
                let _ = members;
                Err(MultiplayerError::InvalidOptions)
            }
        }
    }
    pub fn finish_delivery(
        &mut self,
        members: Vec<MemberProgress>,
    ) -> Result<(), MultiplayerError> {
        match &mut self.backend {
            Backend::Bilateral(owner) => owner.finish_delivery(members),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(owner) => {
                let outcome = owner.finish(&members, self.native_completed);
                match outcome.error {
                    Some(error) => Err(MultiplayerError::Io(error.to_string())),
                    None => Ok(()), // Cleanup remains separate and is returned by stop().
                }
            }
        }
    }
    pub fn request_stop(&mut self) {
        match &mut self.backend {
            Backend::Bilateral(owner) => owner.request_stop(),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(owner) => owner.request_stop(),
        }
    }
    pub fn stop(&mut self) -> Result<(), MultiplayerError> {
        match &mut self.backend {
            Backend::Bilateral(owner) => owner.stop(),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(owner) => {
                let outcome = owner
                    .outcome()
                    .cloned()
                    .unwrap_or_else(|| owner.finish(&[], false));
                match outcome.cleanup_error {
                    Some(error) => Err(MultiplayerError::Io(error.to_string())),
                    None => Ok(()),
                }
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn room_outcome(&self) -> Option<&NativeRoomOutcome> {
        match &self.backend {
            Backend::Room(owner) => owner.outcome(),
            _ => None,
        }
    }
    pub fn remote_roster(&self) -> Option<&[PlayerId]> {
        match &self.backend {
            Backend::Bilateral(owner) => owner.remote_roster(),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(_) => None,
        }
    }
    pub fn remote_progress(&self) -> Option<&GroupPrefix> {
        match &self.backend {
            Backend::Bilateral(owner) => owner.remote_progress(),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(_) => None,
        }
    }
    pub fn remote_final_progress(&self) -> Option<&GroupPrefix> {
        match &self.backend {
            Backend::Bilateral(owner) => owner.remote_final_progress(),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Room(_) => None,
        }
    }
}

#[cfg(test)]
#[path = "native_room_app_fixtures.rs"]
mod fixtures;
