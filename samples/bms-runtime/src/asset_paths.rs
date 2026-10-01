//! Contained off-thread asset lookup; no file content or codec access.
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

/// Exact preserves literal lookup. Variants are considered only when that
/// literal is absent, never to conceal an existing damaged or unsafe reference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AssetPathPolicy {
    #[default]
    Exact,
    AudioVariants,
}

fn relative_name(name: &str) -> io::Result<PathBuf> {
    let portable = name.replace('\\', "/");
    if portable.is_empty()
        || portable.contains('\0')
        || portable.as_bytes().get(1) == Some(&b':')
        || Path::new(&portable)
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty, absolute, parent or drive-qualified asset path rejected",
        ));
    }
    Ok(PathBuf::from(portable))
}

fn variants(relative: &Path) -> Vec<PathBuf> {
    let families = match relative
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some(extension) if extension.eq_ignore_ascii_case("wav") => ["wav", "flac", "ogg"],
        Some(extension) if extension.eq_ignore_ascii_case("flac") => ["flac", "wav", "ogg"],
        Some(extension) if extension.eq_ignore_ascii_case("ogg") => ["ogg", "wav", "flac"],
        Some(extension) if extension.eq_ignore_ascii_case("mp3") => ["wav", "flac", "ogg"],
        None => ["wav", "flac", "ogg"],
        _ => return Vec::new(),
    };
    let mut candidates = Vec::with_capacity(32);
    for family in families {
        for mask in 0..(1 << family.len()) {
            let extension: String = family
                .bytes()
                .enumerate()
                .map(|(index, byte)| {
                    char::from(if mask & (1 << index) == 0 {
                        byte
                    } else {
                        byte.to_ascii_uppercase()
                    })
                })
                .collect();
            let candidate = relative.with_extension(extension);
            if candidate != relative {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn existing(root: &Path, candidate: &Path) -> io::Result<Option<PathBuf>> {
    match fs::symlink_metadata(candidate) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    // A dangling symlink is an existing reference: canonicalization errors are
    // returned, rather than being mistaken for permission to try another file.
    let resolved = fs::canonicalize(candidate)?;
    if !resolved.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "asset symlink escapes chart directory",
        ));
    }
    if !fs::metadata(&resolved)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "asset reference is not a regular file",
        ));
    }
    Ok(Some(resolved))
}

/// Resolves a contained regular file from a canonical directory root. Variant
/// lookup preserves stem/directory spelling and tries at most 32 WAV/FLAC/OGG ASCII
/// extension case combinations. Unknown extensions remain literal-only.
/// This assumes a trusted static filesystem; it is not a race-free sandbox.
pub fn resolve_asset(root: &Path, name: &str, policy: AssetPathPolicy) -> io::Result<PathBuf> {
    let relative = relative_name(name)?;
    let root = fs::canonicalize(root)?;
    if !fs::metadata(&root)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "asset root must be a directory",
        ));
    }
    if let Some(resolved) = existing(&root, &root.join(&relative))? {
        return Ok(resolved);
    }
    if policy == AssetPathPolicy::AudioVariants {
        for candidate in variants(&relative) {
            if let Some(resolved) = existing(&root, &root.join(candidate))? {
                return Ok(resolved);
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("asset not found: {name}"),
    ))
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    #[test]
    fn bounded_case_order_preserves_unicode_stems_directories_and_family_priority() {
        let candidates = variants(Path::new("日本/音.mp3"));
        assert_eq!(candidates.len(), 32);
        assert_eq!(
            candidates[..8],
            ["wav", "Wav", "wAv", "WAv", "waV", "WaV", "wAV", "WAV"]
                .map(|extension| PathBuf::from(format!("日本/音.{extension}")))
        );
        assert_eq!(candidates[8], Path::new("日本/音.flac"));
        assert_eq!(candidates[9], Path::new("日本/音.Flac"));
        assert_eq!(candidates[23], Path::new("日本/音.FLAC"));
        assert_eq!(candidates[24], Path::new("日本/音.ogg"));
        assert_eq!(candidates[31], Path::new("日本/音.OGG"));
        let supported = variants(Path::new("日本/音.FlAc"));
        assert_eq!(supported.len(), 31);
        assert_eq!(supported[0], Path::new("日本/音.flac"));
        assert_eq!(supported[15], Path::new("日本/音.wav"));
        assert!(
            !supported
                .iter()
                .any(|candidate| candidate == Path::new("日本/音.FlAc"))
        );
        let ogg = variants(Path::new("日本/音.oGg"));
        assert_eq!(ogg.len(), 31);
        assert_eq!(ogg[0], Path::new("日本/音.ogg"));
        assert_eq!(ogg[7], Path::new("日本/音.wav"));
        assert_eq!(ogg[15], Path::new("日本/音.flac"));
        assert_eq!(variants(Path::new("tone")).len(), 32);
        assert!(variants(Path::new("tone.xyz")).is_empty());
    }
    #[test]
    fn portable_lexical_rules_reject_escapes_and_preserve_supported_relative_names() {
        for name in [
            "",
            "/absolute.wav",
            "\\absolute.wav",
            "../escape.wav",
            "a/../b.wav",
            "a\\..\\b.wav",
            "C:relative.wav",
            "C:\\absolute.wav",
            "\\\\host\\file.wav",
            "bad\0.wav",
        ] {
            assert_eq!(
                relative_name(name).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "{name}"
            );
        }
        assert_eq!(
            relative_name("日本\\音.wav").unwrap(),
            Path::new("日本/音.wav")
        );
        assert_eq!(
            relative_name("./日本/音.wav").unwrap(),
            Path::new("./日本/音.wav")
        );
        assert_eq!(AssetPathPolicy::default(), AssetPathPolicy::Exact);
    }
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "beatkernel-assets-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("fixture directory: {error}"),
                }
            }
        }
        fn write(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, b"opaque fixture, no decoding").unwrap();
            fs::canonicalize(path).unwrap()
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn exact_literal_wins_and_supported_fallback_extensionless_and_unknown_are_explicit() {
        let temp = Temp::new();
        let literal = temp.write("tone.wav");
        let variant = temp.write("tone.FlAc");
        assert_eq!(
            resolve_asset(&temp.0, "tone.wav", AssetPathPolicy::AudioVariants).unwrap(),
            literal
        );
        assert_eq!(
            resolve_asset(&temp.0, "tone", AssetPathPolicy::AudioVariants).unwrap(),
            literal
        );
        fs::remove_file(&literal).unwrap();
        assert_eq!(
            resolve_asset(&temp.0, "tone.wav", AssetPathPolicy::Exact)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            resolve_asset(&temp.0, "tone.wav", AssetPathPolicy::AudioVariants).unwrap(),
            variant
        );
        assert_eq!(
            resolve_asset(&temp.0, "tone.ogg", AssetPathPolicy::AudioVariants).unwrap(),
            variant
        );
        assert_eq!(
            resolve_asset(&temp.0, "tone.mp3", AssetPathPolicy::AudioVariants).unwrap(),
            variant
        );
        assert_eq!(
            resolve_asset(&temp.0, "tone.xyz", AssetPathPolicy::AudioVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        let error =
            resolve_asset(&temp.0, "missing.wav", AssetPathPolicy::AudioVariants).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("missing.wav"));
        fs::create_dir(temp.0.join("日本")).unwrap();
        let nested = temp.write("日本/音.WAV");
        assert_eq!(
            resolve_asset(&temp.0, "日本\\音.wav", AssetPathPolicy::AudioVariants).unwrap(),
            nested
        );
        assert_eq!(
            resolve_asset(&literal, "tone.wav", AssetPathPolicy::Exact)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
    }
    #[test]
    fn existing_directory_or_non_directory_root_rejects_without_variant_fallthrough() {
        let temp = Temp::new();
        let file = temp.write("tone.flac");
        fs::create_dir(temp.0.join("tone.wav")).unwrap();
        assert_eq!(
            resolve_asset(&temp.0, "tone.wav", AssetPathPolicy::AudioVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            resolve_asset(&file, "tone.wav", AssetPathPolicy::AudioVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        // An earlier variant directory is also a failure, not a skipped choice.
        fs::create_dir(temp.0.join("other.wav")).unwrap();
        temp.write("other.flac");
        assert_eq!(
            resolve_asset(&temp.0, "other.mp3", AssetPathPolicy::AudioVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
    #[cfg(unix)]
    #[test]
    fn existing_symlinks_are_contained_or_fail_without_fallback() {
        use std::os::unix::fs::symlink;
        let temp = Temp::new();
        let outside = Temp::new();
        let inside = temp.write("inside.flac");
        let external = outside.write("external.flac");
        symlink(&inside, temp.0.join("contained.wav")).unwrap();
        assert_eq!(
            resolve_asset(&temp.0, "contained.wav", AssetPathPolicy::AudioVariants).unwrap(),
            inside
        );
        symlink(&external, temp.0.join("escaped.wav")).unwrap();
        temp.write("escaped.flac");
        assert_eq!(
            resolve_asset(&temp.0, "escaped.wav", AssetPathPolicy::AudioVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        symlink(temp.0.join("absent"), temp.0.join("dangling.wav")).unwrap();
        temp.write("dangling.flac");
        assert_eq!(
            resolve_asset(&temp.0, "dangling.wav", AssetPathPolicy::AudioVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        symlink(&external, temp.0.join("variant.wav")).unwrap();
        temp.write("variant.flac");
        assert_eq!(
            resolve_asset(&temp.0, "variant.mp3", AssetPathPolicy::AudioVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        symlink(temp.0.join("absent"), temp.0.join("broken.wav")).unwrap();
        temp.write("broken.flac");
        assert_eq!(
            resolve_asset(&temp.0, "broken.ogg", AssetPathPolicy::AudioVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        symlink(&temp.0, temp.0.join("folder.wav")).unwrap();
        temp.write("folder.flac");
        assert_eq!(
            resolve_asset(&temp.0, "folder.wav", AssetPathPolicy::AudioVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
