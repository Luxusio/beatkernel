//! Portable multiplayer setup bounds and role-neutral configuration.
use crate::{
    multiplayer_credentials::QuicCredentials,
    multiplayer_start::StartPolicy,
    multiplayer_protocol::{MultiplayerError, MAX_IDENTITY},
};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct MultiplayerOptions {
    /// Explicit QUIC server credentials or joining trust anchor/server name.
    pub quic: QuicCredentials,
    /// Includes connect, identity, readiness and clock probes; 1 ms through 120 seconds.
    pub setup_timeout: Duration,
    /// Each application queue has this capacity, 1 through 1,024 messages.
    pub queue_capacity: usize,
    /// Maximum lack of I/O progress while a frame is pending; quiet peers stay connected.
    pub io_stall_timeout: Duration,
    /// Checked software-start lead, clock age and uncertainty bounds.
    pub start_policy: StartPolicy,
    /// Local output-zero to section-start interval; independent of peer preroll.
    pub preroll_ns: i64,
}
impl Default for MultiplayerOptions {
    fn default() -> Self {
        Self {
            quic: QuicCredentials::default(),
            setup_timeout: Duration::from_secs(10),
            queue_capacity: 32,
            io_stall_timeout: Duration::from_secs(5),
            start_policy: StartPolicy::default(),
            preroll_ns: 0,
        }
    }
}

pub(crate) fn validate_identity(identity: &[u8]) -> Result<(), MultiplayerError> {
    if identity.is_empty() || identity.len() > MAX_IDENTITY {
        return Err(MultiplayerError::InvalidOptions);
    }
    Ok(())
}
pub(crate) fn validate_options(
    identity: &[u8],
    options: &MultiplayerOptions,
) -> Result<(), MultiplayerError> {
    validate_identity(identity)?;
    if options.preroll_ns < 0 {
        return Err(MultiplayerError::InvalidOptions);
    }
    options
        .start_policy
        .validate()
        .map_err(|_| MultiplayerError::InvalidOptions)?;
    if options.setup_timeout < Duration::from_millis(1)
        || options.setup_timeout > Duration::from_secs(120)
        || options.io_stall_timeout < Duration::from_millis(1)
        || options.io_stall_timeout > Duration::from_secs(60)
        || !(1..=1024).contains(&options.queue_capacity)
    {
        return Err(MultiplayerError::InvalidOptions);
    }
    Ok(())
}
