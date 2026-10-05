//! Portable destination metadata and CA preparation through an injected reader.
use crate::{
    multiplayer_start::StartRole,
    multiplayer_credentials::{CredentialReadPort, CredentialLoadError, acquire},
};
use std::{io, path::PathBuf};

/// Explicit relay destination and trust. Both start roles are network clients.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebTransportOptions {
    pub url: String,
    pub origin: String,
    pub ca: PathBuf,
    pub role: StartRole,
}

impl WebTransportOptions {
    /// Validate without opening credentials, acquiring a socket or resolving DNS.
    pub fn validate(&self) -> io::Result<()> {
        validate_metadata(&self.url, &self.origin, &self.ca)
    }
}

/// Validate borrowed metadata without constructing or copying client options.
pub(crate) fn validate_metadata(url: &str, origin: &str, ca: &std::path::Path) -> io::Result<()> {
    #[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
    {
        validate_url(url)?;
        if !valid_origin(origin) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid canonical WebTransport Origin",
            ));
        }
        let path = ca.as_os_str().as_encoded_bytes();
        if path.is_empty() || path.len() > 4096 || path.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid WebTransport CA path",
            ));
        }
        Ok(())
    }
    #[cfg(not(all(not(target_arch = "wasm32"), feature = "webtransport")))]
    {
        let _ = (url, origin, ca);
        Err(unavailable())
    }
}

#[cfg(not(all(not(target_arch = "wasm32"), feature = "webtransport")))]
pub(crate) fn unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "native WebTransport requires a native build with the webtransport feature",
    )
}

#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
pub(crate) fn validate_url(value: &str) -> io::Result<url::Url> {
    if value.is_empty() || value.len() > 4096 {
        return Err(invalid("invalid bounded WebTransport URL"));
    }
    let url = url::Url::parse(value).map_err(|_| invalid("invalid WebTransport URL"))?;
    let key = url.path().strip_prefix("/rooms/").unwrap_or_default();
    if url.scheme() != "https"
        || url.host().is_none()
        || url.port_or_known_default() == Some(0)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.as_str() != value
        || key.is_empty()
        || key.len() > 1024
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(invalid(
            "WebTransport requires a canonical HTTPS /rooms/ASCII_KEY URL",
        ));
    }
    Ok(url)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "webtransport"))]
pub(crate) fn valid_origin(origin: &str) -> bool {
    if origin.is_empty() || origin.len() > 4096 {
        return false;
    }
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    let loopback = match url.host() {
        Some(url::Host::Domain(name)) => name == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    (url.scheme() == "https" || (url.scheme() == "http" && loopback))
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.origin().ascii_serialization() == origin
}

pub struct PreparedWebTransport<'a> {
    pub options: &'a WebTransportOptions,
    pub ca: Vec<u8>,
}

pub fn prepare_webtransport<'a, P: CredentialReadPort>(
    port: &mut P,
    options: &'a WebTransportOptions,
) -> Result<PreparedWebTransport<'a>, CredentialLoadError<P::Error>> {
    options
        .validate()
        .map_err(CredentialLoadError::Validation)?;
    let ca = acquire(port, &options.ca)?;
    Ok(PreparedWebTransport { options, ca })
}

#[cfg(test)]
#[path = "webtransport_preparation_fixtures.rs"]
mod fixtures;
