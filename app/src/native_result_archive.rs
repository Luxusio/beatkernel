//! Explicit-directory file adapter with complete, exclusive file publication.
//! Synced files do not imply directory durability; group saves are nontransactional.
use crate::result_archive_store::ResultArchiveStoragePort;
use std::{
    fs::File,
    io::{self, Read},
    path::{Component, Path, PathBuf},
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
    /// Publishes a complete synced file exclusively; staging cleanup is best effort.
    /// Hexadecimal 8.3 staging names and aliases fail with `InvalidInput` before I/O.
    fn create_new(&mut self, key: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.path(key)?;
        crate::native_publication::publish_new(&path, bytes)
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
/// Outer adapter publishing a complete file exclusively; staging cleanup is best effort.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_sidecar(
    archive: &crate::result_archive::ResultArchive,
    base: &std::path::Path,
) -> crate::native_gameplay::NativeGameplayResult<()> {
    let bytes = crate::result_archive::encode_archive(archive)?;
    let path = sidecar_path(base)?;
    write_sidecar_bytes(&path, &bytes)
}
fn write_sidecar_bytes(
    path: &Path,
    bytes: &[u8],
) -> crate::native_gameplay::NativeGameplayResult<()> {
    crate::native_publication::publish_new(path, bytes)?;
    Ok(())
}

struct NativeArchivePublication<'a, F> {
    base: &'a Path,
    write: F,
}
impl<F: FnMut(&Path, &[u8]) -> crate::native_gameplay::NativeGameplayResult<()>>
    crate::result_archive_publication::ResultArchivePublicationPort
    for NativeArchivePublication<'_, F>
{
    type Destination = PathBuf;
    type Error = Box<dyn std::error::Error>;
    fn destination(
        &self,
        player: Option<crate::local_players::PlayerId>,
    ) -> Result<PathBuf, Self::Error> {
        match player {
            None => sidecar_path(self.base),
            Some(player) => sidecar_path(&crate::native_cohort::replay_path(self.base, player)?),
        }
    }
    fn create_new(&mut self, destination: &PathBuf, bytes: &[u8]) -> Result<(), Self::Error> {
        (self.write)(destination, bytes)
    }
}
/// Native composition of portable staging policy and caller-supplied exclusive publication.
pub fn publish_cohort_sidecars(
    archive: &crate::result_archive::ResultArchive,
    base: &Path,
    write: impl FnMut(&Path, &[u8]) -> crate::native_gameplay::NativeGameplayResult<()>,
) -> crate::native_gameplay::NativeGameplayResult<()> {
    use crate::result_archive_publication::PublicationError;
    crate::result_archive_publication::publish_archive_set(
        archive,
        &mut NativeArchivePublication { base, write },
    )
    .map_err(|error| match error {
        PublicationError::Destination(error) | PublicationError::Storage(error) => error,
        PublicationError::Archive(error) => Box::new(error),
        PublicationError::Allocation(error) => Box::new(error),
        PublicationError::DuplicateDestination => "duplicate completed sidecar destination".into(),
    })
}
/// Outer filesystem adapter; files are exclusive but the group is not transactional.
pub fn save_cohort_sidecars(
    archive: &crate::result_archive::ResultArchive,
    base: &Path,
) -> crate::native_gameplay::NativeGameplayResult<()> {
    publish_cohort_sidecars(archive, base, write_sidecar_bytes)
}

/// Publish sidecars beside the actual current member captures, retaining the
/// whole-cohort archive at its current base. Publication remains nontransactional.
pub fn save_cohort_sidecars_with_paths(
    archive: &crate::result_archive::ResultArchive,
    base: &Path,
    paths: &[(crate::local_players::PlayerId, Option<PathBuf>)],
) -> crate::native_gameplay::NativeGameplayResult<()> {
    struct ActualPaths<'a> {
        base: &'a Path,
        paths: &'a [(crate::local_players::PlayerId, Option<PathBuf>)],
    }
    impl crate::result_archive_publication::ResultArchivePublicationPort for ActualPaths<'_> {
        type Destination = PathBuf;
        type Error = Box<dyn std::error::Error>;
        fn destination(
            &self,
            player: Option<crate::local_players::PlayerId>,
        ) -> Result<PathBuf, Self::Error> {
            match player {
                None => sidecar_path(self.base),
                Some(player) => sidecar_path(
                    self.paths
                        .iter()
                        .find(|(id, _)| *id == player)
                        .and_then(|(_, path)| path.as_deref())
                        .ok_or("completed member is missing its actual capture path")?,
                ),
            }
        }
        fn create_new(&mut self, destination: &PathBuf, bytes: &[u8]) -> Result<(), Self::Error> {
            write_sidecar_bytes(destination, bytes)
        }
    }
    crate::multiplayer_group::validate_roster(
        &paths.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
    )?;
    use crate::result_archive_publication::PublicationError;
    crate::result_archive_publication::publish_archive_set(
        archive,
        &mut ActualPaths { base, paths },
    )
    .map_err(|error| match error {
        PublicationError::Destination(error) | PublicationError::Storage(error) => error,
        PublicationError::Archive(error) => Box::new(error),
        PublicationError::Allocation(error) => Box::new(error),
        PublicationError::DuplicateDestination => "duplicate completed sidecar destination".into(),
    })
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

#[cfg(test)]
#[path = "native_member_sidecar_fixtures.rs"]
mod member_fixtures;

#[cfg(test)]
#[path = "native_result_archive_publication_fixtures.rs"]
mod publication_fixtures;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod practice_paths_fixtures {
    use super::*;
    use crate::{local_players::PlayerId, result_archive::member_fixtures::whole};
    use std::sync::atomic::{AtomicU64, Ordering};
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "beatkernel-practice-sidecars-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn practice_sidecars_follow_actual_member_retry_paths_and_preserve_exclusive_storage() {
        let directory = Directory::new();
        let archive = whole(2);
        let base = directory.0.join("song.retry3.bkr");
        let paths: Vec<_> = archive
            .entries()
            .iter()
            .map(|entry| {
                (
                    entry.player,
                    Some(
                        directory
                            .0
                            .join(format!("song.p{}.retry3.bkr", entry.player.0)),
                    ),
                )
            })
            .collect();
        save_cohort_sidecars_with_paths(&archive, &base, &paths).unwrap();
        assert_eq!(
            read_sidecar(&base).unwrap().unwrap(),
            crate::result_archive::encode_archive(&archive).unwrap()
        );
        for (entry, (_, path)) in archive.entries().iter().zip(&paths) {
            let bytes = read_sidecar(path.as_ref().unwrap()).unwrap().unwrap();
            assert_eq!(
                crate::result_archive::decode_archive(&bytes)
                    .unwrap()
                    .entries(),
                std::slice::from_ref(entry)
            );
            assert!(!directory
                .0
                .join(format!("song.retry3.p{}.bkr.bkresult", entry.player.0))
                .exists());
        }
        let before = read_sidecar(&base).unwrap();
        assert!(save_cohort_sidecars_with_paths(&archive, &base, &paths).is_err());
        assert_eq!(read_sidecar(&base).unwrap(), before);
    }
    #[test]
    fn practice_missing_or_aliased_member_paths_refuse_before_any_sidecar_write() {
        let directory = Directory::new();
        let archive = whole(2);
        let ids: Vec<PlayerId> = archive.entries().iter().map(|entry| entry.player).collect();
        let base = directory.0.join("song.retry3.bkr");
        let shared = directory.0.join("shared.retry3.bkr");
        for paths in [
            vec![(ids[0], Some(shared.clone())), (ids[1], None)],
            vec![
                (ids[0], Some(shared.clone())),
                (ids[1], Some(shared.clone())),
            ],
            vec![
                (ids[0], Some(shared.clone())),
                (ids[0], Some(shared.clone())),
            ],
        ] {
            assert!(save_cohort_sidecars_with_paths(&archive, &base, &paths).is_err());
            assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 0);
        }
    }
}
