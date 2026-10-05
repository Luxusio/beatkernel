//! Explicit-directory file adapter. Flush is not crash-atomic durability.
use crate::result_archive_store::ResultArchiveStoragePort;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf, Component},
};

pub struct NativeResultArchiveStore {
    root: PathBuf,
}
impl NativeResultArchiveStore {
    /// Does not create directories or perform filesystem access.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    fn path(&self, key: &str) -> io::Result<PathBuf> {
        // Use the same portable filename subset on every native platform.
        if key.is_empty()
            || key.len() > 255
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "archive key must be one file component",
            ));
        }
        let stem = key.split('.').next().unwrap_or("");
        let reserved = matches!(
            stem.to_ascii_uppercase().as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        );
        if key.ends_with('.') || key == "." || key == ".." || reserved {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "archive key must be one file component",
            ));
        }
        let mut components = Path::new(key).components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "archive key must be one file component",
            ));
        }
        Ok(self.root.join(key))
    }
}
impl ResultArchiveStoragePort for NativeResultArchiveStore {
    type Error = io::Error;
    /// On write/flush refusal a partial newly created file may remain.
    fn create_new(&mut self, key: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.path(key)?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(bytes)?;
        file.flush()
    }
    fn read_bounded(&mut self, key: &str, max_bytes: usize) -> io::Result<Vec<u8>> {
        let path = self.path(key)?;
        read_regular_file(&path, max_bytes)
    }
}

/// Append to the entire native filename without UTF-8 conversion.
#[cfg(not(target_arch = "wasm32"))]
pub fn sidecar_path(
    base: &std::path::Path,
) -> crate::native_gameplay::NativeGameplayResult<std::path::PathBuf> {
    let name = base.file_name().ok_or("replay base path has no filename")?;
    let mut name = name.to_os_string();
    name.push(".bkresult");
    Ok(base.with_file_name(name))
}
/// Outer exclusive-create adapter. A refused write/flush may leave a partial new file.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_sidecar(
    archive: &crate::result_archive::ResultArchive,
    base: &std::path::Path,
) -> crate::native_gameplay::NativeGameplayResult<()> {
    use std::io::Write;
    let bytes = crate::result_archive::encode_archive(archive)?;
    let path = sidecar_path(base)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&bytes)?;
    file.flush()?;
    Ok(())
}

fn read_regular_file(path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
    if max_bytes
        .checked_add(1)
        .is_none_or(|n| n > isize::MAX as usize)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "archive read bound",
        ));
    }
    if !std::fs::symlink_metadata(&path)?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "archive is not a regular file",
        ));
    }
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "archive is not a regular file",
        ));
    }
    if metadata.len() > max_bytes as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "archive exceeds byte bound",
        ));
    }
    let mut bytes = Vec::new();
    let mut block = [0u8; 8192];
    loop {
        let limit = (max_bytes + 1 - bytes.len()).min(block.len());
        let count = match file.read(&mut block[..limit]) {
            Ok(0) => break,
            Ok(count) => count,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if bytes.len() + count > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "archive exceeds byte bound",
            ));
        }
        bytes
            .try_reserve(count)
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "archive allocation"))?;
        bytes.extend_from_slice(&block[..count]);
    }
    Ok(bytes)
}
/// Adjacent sidecar admission supports the original native filename without conversion.
pub fn read_sidecar(base: &Path) -> io::Result<Option<Vec<u8>>> {
    let path = sidecar_path(base)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    match read_regular_file(&path, crate::result_archive::MAX_ARCHIVE_BYTES) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
