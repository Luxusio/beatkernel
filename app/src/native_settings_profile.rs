//! Native file IO for the pure, versioned profile codecs.

use super::{
    decode_player_profile, decode_profile, encode_player_profile, encode_profile, PlayerProfile,
    SettingsHost, MAX_PROFILE_BYTES,
};
use crate::{
    native_publication::{self, CommitDisposition},
    settings::NativeSettings,
};
use std::{
    error::Error,
    fs::{self, File},
    io::{self, Read},
    path::Path,
};

/// Reads a regular, non-symlink version 1 native profile with a growth-safe cap.
pub fn load_profile(path: &Path, host: SettingsHost) -> Result<NativeSettings, Box<dyn Error>> {
    Ok(decode_profile(&read_profile_bytes(path)?, host)?)
}

/// Reads a combined player profile or a legacy native profile with display defaults.
pub fn load_player_profile(
    path: &Path,
    host: SettingsHost,
) -> Result<PlayerProfile, Box<dyn Error>> {
    Ok(decode_player_profile(&read_profile_bytes(path)?, host)?)
}

fn read_profile_bytes(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    regular_path(path)?;
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_PROFILE_BYTES as u64 {
        return Err("profile must be a regular file at most 72 KiB".into());
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(MAX_PROFILE_BYTES + 1)?;
    file.take((MAX_PROFILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err("profile exceeds 72 KiB".into());
    }
    Ok(bytes)
}

/// Encodes before IO, then syncs and closes a uniquely owned sibling before publication.
/// Existing targets must be structurally valid same-host profiles. A new target
/// is published with create-only hard linking; replacements use rename. Neither
/// path promises interprocess locking or directory crash durability.
/// Hexadecimal 8.3 staging names and their native aliases are reserved: such
/// final names return `InvalidInput` before target metadata or reads.
pub fn save_profile(
    path: &Path,
    values: &NativeSettings,
    host: SettingsHost,
) -> Result<(), Box<dyn Error>> {
    let encoded = encode_profile(values, host)?;
    save_encoded(path, &encoded, host, validate_native_profile)
}

/// Writes version 2 using the same complete-file publication protocol.
/// Existing valid same-host version 1 or version 2 may be replaced; malformed,
/// foreign and symlink targets remain refused. Native-only save refuses version 2.
/// Encoding errors precede the reserved staging-name refusal and all native IO.
pub fn save_player_profile(
    path: &Path,
    values: &PlayerProfile,
    host: SettingsHost,
) -> Result<(), Box<dyn Error>> {
    let encoded = encode_player_profile(values, host)?;
    save_encoded(path, &encoded, host, validate_player_profile)
}

fn validate_native_profile(bytes: &[u8], host: SettingsHost) -> Result<(), String> {
    decode_profile(bytes, host).map(|_| ())
}

fn validate_player_profile(bytes: &[u8], host: SettingsHost) -> Result<(), String> {
    decode_player_profile(bytes, host).map(|_| ())
}

fn save_encoded(
    path: &Path,
    encoded: &[u8],
    host: SettingsHost,
    validate: fn(&[u8], SettingsHost) -> Result<(), String>,
) -> Result<(), Box<dyn Error>> {
    let replacing = classify_target(path, host, validate)?;
    native_publication::publish_with(path, encoded, |stage, final_path| {
        commit_profile(stage, final_path, replacing, host, validate)
    })
}

#[cfg(test)]
fn save_encoded_with<W: native_publication::PublicationWriter>(
    path: &Path,
    encoded: &[u8],
    host: SettingsHost,
    validate: fn(&[u8], SettingsHost) -> Result<(), String>,
    wrap: impl FnOnce(File) -> W,
    cleanup: impl FnMut(&Path) -> io::Result<()>,
) -> Result<(), Box<dyn Error>> {
    let replacing = classify_target(path, host, validate)?;
    native_publication::publish_with_writer_and_cleanup(
        path,
        encoded,
        wrap,
        |stage, final_path| commit_profile(stage, final_path, replacing, host, validate),
        cleanup,
    )
}

fn classify_target(
    path: &Path,
    host: SettingsHost,
    validate: fn(&[u8], SettingsHost) -> Result<(), String>,
) -> Result<bool, Box<dyn Error>> {
    if path.file_name().is_none() {
        return Err("profile path requires a file name".into());
    }
    native_publication::validate_final_name(path)?;
    match fs::symlink_metadata(path) {
        Ok(_) => {
            validate(&read_profile_bytes(path)?, host)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn commit_profile(
    stage: &Path,
    final_path: &Path,
    replacing: bool,
    host: SettingsHost,
    validate: fn(&[u8], SettingsHost) -> Result<(), String>,
) -> Result<CommitDisposition, Box<dyn Error>> {
    if replacing {
        // Recheck a changed/foreign/symlink target immediately before rename.
        // Concurrent replacements still require external coordination.
        validate(&read_profile_bytes(final_path)?, host)?;
        fs::rename(stage, final_path)?;
        // Nothing fallible may follow rename before the shared owner disarms.
        Ok(CommitDisposition::Moved)
    } else {
        // Preserve a destination that appeared after classification. Unsupported
        // links fail explicitly without a clobbering fallback.
        fs::hard_link(stage, final_path)?;
        Ok(CommitDisposition::Linked)
    }
}

fn regular_path(path: &Path) -> Result<(), Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("profile path must be a regular file without symlinks".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "native_profile_publication_fixtures.rs"]
mod fixtures;
