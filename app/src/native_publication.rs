//! Complete, exclusive publication in a caller-owned directory.
//!
//! The synced file is closed before linking its final name. Directory durability
//! and hostile directory changes are outside this boundary; staging cleanup is
//! best effort, including after a successful publication.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);
const STAGE_ID_LIMIT: u64 = 1 << 44;
const COLLISION_LIMIT: usize = 32;

pub(crate) trait PublicationWriter: Write {
    fn sync_all(&mut self) -> io::Result<()>;
}

impl PublicationWriter for File {
    fn sync_all(&mut self) -> io::Result<()> {
        File::sync_all(self)
    }
}

pub(crate) fn publish_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    publish_new_with(path, bytes, |file| file)
}

pub(crate) fn publish_new_with<W: PublicationWriter>(
    path: &Path,
    bytes: &[u8],
    wrap: impl FnOnce(File) -> W,
) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    publish_new_with_candidates(path, bytes, wrap, || loop {
        let id = NEXT_STAGE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1).filter(|next| *next <= STAGE_ID_LIMIT)
            })
            .map_err(|_| io::Error::other("publication staging identity exhausted"))?;
        #[cfg(target_arch = "wasm32")]
        let process_id = 0u32;
        #[cfg(not(target_arch = "wasm32"))]
        let process_id = std::process::id();
        let identity = id ^ ((process_id as u64) << 12);
        // Uppercase hex is already a valid 8.3 name, so Windows need not
        // synthesize another short-name alias for the staging file.
        let stage = parent.join(format!("{:08X}.{:03X}", identity >> 12, identity & 0xFFF));
        if !stage_aliases_final(&stage, path) {
            return Ok(stage);
        }
    })
}

// The candidate seam keeps collision tests local while retaining the actual
// create_new, write_all and publication operations used by the native adapter.
pub(crate) fn publish_new_with_candidates<W: PublicationWriter>(
    path: &Path,
    bytes: &[u8],
    wrap: impl FnOnce(File) -> W,
    candidate: impl FnMut() -> io::Result<PathBuf>,
) -> io::Result<()> {
    publish_new_with_candidates_and_cleanup(path, bytes, wrap, candidate, |stage| {
        fs::remove_file(stage)
    })
}

pub(crate) fn publish_new_with_candidates_and_cleanup<W: PublicationWriter>(
    path: &Path,
    bytes: &[u8],
    wrap: impl FnOnce(File) -> W,
    mut candidate: impl FnMut() -> io::Result<PathBuf>,
    cleanup: impl FnMut(&Path) -> io::Result<()>,
) -> io::Result<()> {
    if let Some(name) = path.file_name() {
        let name = name.to_string_lossy();
        let bytes = normalized_basename(&name).as_bytes();
        if bytes.len() == 12
            && bytes[8] == b'.'
            && bytes[..8]
                .iter()
                .chain(&bytes[9..])
                .all(u8::is_ascii_hexdigit)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "publication filename is reserved for staging",
            ));
        }
    }
    let (file, stage) = create_stage(
        &mut || {
            let stage = candidate()?;
            if stage_aliases_final(&stage, path) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "publication stage must differ from final filename",
                ));
            }
            Ok(stage)
        },
        cleanup,
    )?;
    let mut writer = wrap(file);
    let written = (|| {
        writer.write_all(bytes)?;
        writer.flush()?;
        writer.sync_all()
    })();
    // In particular, Windows cleanup must not rely on an open staging handle.
    drop(writer);
    written?;
    // hard_link creates the final name exclusively, including when it is a
    // dangling symlink. Unsupported links refuse without a write fallback.
    fs::hard_link(&stage.path, path)?;
    // Dropping the stage preserves either the primary error or committed
    // success, even when cleanup refuses. The final name is never removed.
    Ok(())
}

fn stage_aliases_final(stage: &Path, final_path: &Path) -> bool {
    let (Some(stage), Some(final_name)) = (stage.file_name(), final_path.file_name()) else {
        return false;
    };
    let stage = stage.to_string_lossy();
    let final_name = final_name.to_string_lossy();
    normalized_basename(&stage).eq_ignore_ascii_case(normalized_basename(&final_name))
}

fn normalized_basename(name: &str) -> &str {
    // Conservative on every host: case-insensitive filesystems also occur on
    // Unix, and Windows normalizes spaces/dots and permits data-stream aliases.
    name.split(':')
        .next()
        .unwrap_or_default()
        .trim_start_matches(' ')
        .trim_end_matches([' ', '.'])
}

struct OwnedStage<C: FnMut(&Path) -> io::Result<()>> {
    path: PathBuf,
    cleanup: C,
}

impl<C: FnMut(&Path) -> io::Result<()>> Drop for OwnedStage<C> {
    fn drop(&mut self) {
        let _ = (self.cleanup)(&self.path);
    }
}

fn create_stage<C: FnMut(&Path) -> io::Result<()>>(
    candidate: &mut impl FnMut() -> io::Result<PathBuf>,
    cleanup: C,
) -> io::Result<(File, OwnedStage<C>)> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        let path = candidate()?;
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, OwnedStage { path, cleanup })),
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists && attempts < COLLISION_LIMIT => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
#[path = "native_publication_fixtures.rs"]
mod fixtures;
