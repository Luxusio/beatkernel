//! One-effect storage policy; archive validation precedes external writes.
use crate::result_archive::{
    ResultArchive, ArchiveError, MAX_ARCHIVE_BYTES, encode_archive, decode_archive,
};

pub trait ResultArchiveStoragePort {
    type Error;
    /// Exclusively creates a new key; an existing object must remain unchanged.
    fn create_new(&mut self, key: &str, bytes: &[u8]) -> Result<(), Self::Error>;
    /// Returns complete bytes, refusing a value larger than the supplied bound.
    fn read_bounded(&mut self, key: &str, max_bytes: usize) -> Result<Vec<u8>, Self::Error>;
}
#[derive(Debug)]
pub enum ArchiveStoreError<E> {
    Archive(ArchiveError),
    Storage(E),
}
impl<E: std::fmt::Display> std::fmt::Display for ArchiveStoreError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Archive(e) => e.fmt(f),
            Self::Storage(e) => write!(f, "result archive storage: {e}"),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for ArchiveStoreError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Archive(e) => Some(e),
            Self::Storage(e) => Some(e),
        }
    }
}
pub fn save_archive<S: ResultArchiveStoragePort>(
    store: &mut S,
    key: &str,
    archive: &ResultArchive,
) -> Result<usize, ArchiveStoreError<S::Error>> {
    let bytes = encode_archive(archive).map_err(ArchiveStoreError::Archive)?;
    store
        .create_new(key, &bytes)
        .map_err(ArchiveStoreError::Storage)?;
    Ok(bytes.len())
}
pub fn load_archive<S: ResultArchiveStoragePort>(
    store: &mut S,
    key: &str,
) -> Result<ResultArchive, ArchiveStoreError<S::Error>> {
    let bytes = store
        .read_bounded(key, MAX_ARCHIVE_BYTES)
        .map_err(ArchiveStoreError::Storage)?;
    decode_archive(&bytes).map_err(ArchiveStoreError::Archive)
}
