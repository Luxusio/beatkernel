//! Portable role metadata and bounded credential acquisition through an opaque port.
use std::{
    io,
    path::{Path, PathBuf},
};

/// Explicit trust material. Private key contents never enter this metadata.
#[derive(Clone, Debug, Default)]
pub struct QuicCredentials {
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
    pub ca: Option<PathBuf>,
    pub server_name: Option<String>,
}

impl QuicCredentials {
    /// Validate a complete role before reading credentials or binding a socket.
    /// Paths retain native spelling and are bounded to 4096 encoded bytes.
    pub fn validate_for_role(&self, host: bool) -> io::Result<()> {
        for path in [&self.cert, &self.key, &self.ca].into_iter().flatten() {
            let bytes = path.as_os_str().as_encoded_bytes();
            if bytes.is_empty() || bytes.len() > 4096 || bytes.contains(&0) {
                return Err(invalid("invalid QUIC credential path"));
            }
        }
        if host {
            if self.cert.is_none()
                || self.key.is_none()
                || self.ca.is_some()
                || self.server_name.is_some()
            {
                return Err(invalid("QUIC host requires only certificate and key paths"));
            }
        } else {
            if self.cert.is_some() || self.key.is_some() || self.ca.is_none() {
                return Err(invalid("QUIC join requires only CA path and server name"));
            }
            validate_name(self.server_name.as_deref().unwrap_or_default())?;
        }
        Ok(())
    }
}

pub(crate) fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub(crate) fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name.len() > 253 || !name.is_ascii() {
        return Err(invalid(
            "QUIC server name must be a bounded ASCII DNS name or IP",
        ));
    }
    if name.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    let dns = name.strip_suffix('.').unwrap_or(name);
    if dns.is_empty()
        || dns.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        || dns
            .rsplit('.')
            .next()
            .is_some_and(|label| label.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(invalid("invalid QUIC server name"));
    }
    Ok(())
}

pub const CREDENTIAL_BYTE_LIMIT: usize = 1024 * 1024;

pub trait CredentialReadPort {
    type Error;
    fn read(&mut self, path: &Path) -> Result<Vec<u8>, Self::Error>;
}

pub enum CredentialLoadError<E> {
    Validation(io::Error),
    Read(E),
    InvalidBytes,
}

pub enum LoadedCredentials<'a> {
    Host { cert: Vec<u8>, key: Vec<u8> },
    Join { ca: Vec<u8>, server_name: &'a str },
}

pub(crate) fn acquire<P: CredentialReadPort>(
    port: &mut P,
    path: &Path,
) -> Result<Vec<u8>, CredentialLoadError<P::Error>> {
    let bytes = port.read(path).map_err(CredentialLoadError::Read)?;
    if bytes.is_empty() || bytes.len() > CREDENTIAL_BYTE_LIMIT {
        return Err(CredentialLoadError::InvalidBytes);
    }
    Ok(bytes)
}

pub fn load_credentials<'a, P: CredentialReadPort>(
    port: &mut P,
    credentials: &'a QuicCredentials,
    host: bool,
) -> Result<LoadedCredentials<'a>, CredentialLoadError<P::Error>> {
    credentials
        .validate_for_role(host)
        .map_err(CredentialLoadError::Validation)?;
    if host {
        let cert = acquire(port, credentials.cert.as_deref().unwrap())?;
        let key = acquire(port, credentials.key.as_deref().unwrap())?;
        Ok(LoadedCredentials::Host { cert, key })
    } else {
        let ca = acquire(port, credentials.ca.as_deref().unwrap())?;
        Ok(LoadedCredentials::Join {
            ca,
            server_name: credentials.server_name.as_deref().unwrap(),
        })
    }
}

#[cfg(test)]
#[path = "multiplayer_credential_loading_fixtures.rs"]
mod fixtures;
