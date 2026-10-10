//! Bounded selected-file and contained filesystem acquisition for shared preparation.
use crate::asset_paths::{self, AssetPathPolicy};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

pub trait AssetSource {
    fn resolve(&self, name: &str, policy: AssetPathPolicy) -> io::Result<PathBuf>;
    fn read<'a>(&'a self, key: &Path, max_bytes: usize) -> io::Result<Cow<'a, [u8]>>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryAssetLimits {
    pub max_files: usize,
    pub max_file_bytes: usize,
    pub max_total_bytes: usize,
    pub max_path_bytes: usize,
}
impl Default for MemoryAssetLimits {
    fn default() -> Self {
        Self {
            max_files: 32_768,
            max_file_bytes: 64 * 1024 * 1024,
            max_total_bytes: 256 * 1024 * 1024,
            max_path_bytes: 4096,
        }
    }
}
enum MemoryFile {
    Declared(usize),
    Loaded(Vec<u8>),
}
impl MemoryFile {
    fn len(&self) -> usize {
        match self {
            Self::Declared(length) => *length,
            Self::Loaded(bytes) => bytes.len(),
        }
    }
}
pub struct MemoryFiles {
    files: BTreeMap<String, MemoryFile>,
    limits: MemoryAssetLimits,
    total_bytes: usize,
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn key(name: &str, max_path_bytes: usize) -> io::Result<String> {
    if name.len() > max_path_bytes {
        return Err(invalid("selected file path exceeds its byte limit"));
    }
    let relative = asset_paths::relative_name(name)?;
    let portable = relative
        .to_str()
        .ok_or_else(|| invalid("selected file path is not UTF-8"))?;
    let value = portable
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/");
    if value.len() > max_path_bytes {
        return Err(invalid("selected file path exceeds its byte limit"));
    }
    Ok(value)
}
impl MemoryFiles {
    pub fn new(limits: MemoryAssetLimits) -> io::Result<Self> {
        if limits.max_files == 0
            || limits.max_file_bytes == 0
            || limits.max_total_bytes == 0
            || limits.max_file_bytes > limits.max_total_bytes
            || limits.max_path_bytes == 0
            || limits.max_file_bytes > isize::MAX as usize
            || limits.max_path_bytes > isize::MAX as usize
        {
            return Err(invalid(
                "selected file limits must be positive and representable",
            ));
        }
        Ok(Self {
            files: BTreeMap::new(),
            limits,
            total_bytes: 0,
        })
    }
    /// Reserve the canonical file's bounds without acquiring its content.
    pub fn declare_file(&mut self, name: &str, length: usize) -> io::Result<()> {
        let name = key(name, self.limits.max_path_bytes)?;
        let total = self.admit_new(&name, length)?;
        self.files.insert(name, MemoryFile::Declared(length));
        self.total_bytes = total;
        Ok(())
    }
    fn admit_new(&self, name: &str, length: usize) -> io::Result<usize> {
        if self.files.contains_key(name) {
            return Err(invalid("duplicate normalized selected file path"));
        }
        if self.is_directory(name)
            || name
                .match_indices('/')
                .any(|(at, _)| self.files.contains_key(&name[..at]))
        {
            return Err(invalid("selected file and directory paths collide"));
        }
        let total = self
            .total_bytes
            .checked_add(length)
            .ok_or_else(|| invalid("selected file byte count overflow"))?;
        if self.files.len() >= self.limits.max_files
            || length > self.limits.max_file_bytes
            || total > self.limits.max_total_bytes
        {
            return Err(invalid("selected file storage exceeds its limits"));
        }
        Ok(total)
    }
    pub fn insert(&mut self, name: &str, bytes: Vec<u8>) -> io::Result<()> {
        let name = key(name, self.limits.max_path_bytes)?;
        if let Some(file) = self.files.get_mut(&name) {
            match file {
                MemoryFile::Declared(length) if *length == bytes.len() => {
                    *file = MemoryFile::Loaded(bytes);
                    return Ok(());
                }
                MemoryFile::Declared(_) => {
                    return Err(invalid("selected file length differs from declaration"))
                }
                MemoryFile::Loaded(_) => {
                    return Err(invalid("duplicate normalized selected file path"))
                }
            }
        }
        let total = self.admit_new(&name, bytes.len())?;
        self.files.insert(name, MemoryFile::Loaded(bytes));
        self.total_bytes = total;
        Ok(())
    }
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }
    pub fn len(&self) -> usize {
        self.files.len()
    }
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }
    fn is_directory(&self, name: &str) -> bool {
        let prefix = format!("{name}/");
        self.files
            .range(prefix.clone()..)
            .next()
            .is_some_and(|(name, _)| name.starts_with(&prefix))
    }
    pub fn read_file(&self, name: &str, max_bytes: usize) -> io::Result<&[u8]> {
        let name = key(name, self.limits.max_path_bytes)?;
        if let Some(file) = self.files.get(&name) {
            if file.len() > max_bytes {
                return Err(invalid("encoded file exceeds preparation limit"));
            }
            return match file {
                MemoryFile::Loaded(bytes) => Ok(bytes),
                MemoryFile::Declared(_) => Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "selected file content has not been acquired",
                )),
            };
        }
        if self.is_directory(&name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "asset reference is not a regular file",
            ));
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("selected file not found: {name}"),
        ))
    }
    pub fn scope(&self, chart_path: &str) -> io::Result<MemoryAssetSource<'_>> {
        let chart_path = key(chart_path, self.limits.max_path_bytes)?;
        self.read_file(&chart_path, self.limits.max_file_bytes)?;
        let parent = chart_path
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent)
            .to_owned();
        Ok(MemoryAssetSource {
            files: self,
            parent,
        })
    }
}
pub struct MemoryAssetSource<'a> {
    files: &'a MemoryFiles,
    parent: String,
}
impl MemoryAssetSource<'_> {
    fn scoped_key(&self, relative: &Path) -> io::Result<String> {
        let relative = relative
            .to_str()
            .ok_or_else(|| invalid("selected asset path is not UTF-8"))?;
        key(
            &if self.parent.is_empty() {
                relative.to_owned()
            } else {
                format!("{}/{relative}", self.parent)
            },
            self.files.limits.max_path_bytes,
        )
    }
    fn existing(&self, relative: &Path) -> io::Result<Option<PathBuf>> {
        let name = self.scoped_key(relative)?;
        if self.files.files.contains_key(&name) {
            return Ok(Some(PathBuf::from(name)));
        }
        if self.files.is_directory(&name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "asset reference is not a regular file",
            ));
        }
        Ok(None)
    }
}
impl AssetSource for MemoryAssetSource<'_> {
    fn resolve(&self, name: &str, policy: AssetPathPolicy) -> io::Result<PathBuf> {
        let relative = PathBuf::from(key(name, self.files.limits.max_path_bytes)?);
        if let Some(path) = self.existing(&relative)? {
            return Ok(path);
        }
        for candidate in asset_paths::variants_for(&relative, policy) {
            if let Some(path) = self.existing(&candidate)? {
                return Ok(path);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("asset not found: {name}"),
        ))
    }
    fn read<'a>(&'a self, path: &Path, max_bytes: usize) -> io::Result<Cow<'a, [u8]>> {
        let name = key(
            path.to_str()
                .ok_or_else(|| invalid("selected asset key is not UTF-8"))?,
            self.files.limits.max_path_bytes,
        )?;
        if !self.parent.is_empty() && !name.starts_with(&format!("{}/", self.parent)) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "asset key escapes chart directory",
            ));
        }
        Ok(Cow::Borrowed(self.files.read_file(&name, max_bytes)?))
    }
}
/// Native containment and canonical alias identity stay in this adapter.
pub struct FileAssetSource {
    root: PathBuf,
}
impl FileAssetSource {
    pub fn new(root: &Path) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        if !fs::metadata(&root)?.is_dir() {
            return Err(invalid("asset root must be a directory"));
        }
        Ok(Self { root })
    }
}
impl AssetSource for FileAssetSource {
    fn resolve(&self, name: &str, policy: AssetPathPolicy) -> io::Result<PathBuf> {
        asset_paths::resolve_asset(&self.root, name, policy)
    }
    fn read<'a>(&'a self, path: &Path, max_bytes: usize) -> io::Result<Cow<'a, [u8]>> {
        let path = fs::canonicalize(path)?;
        if !path.starts_with(&self.root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "asset key escapes chart directory",
            ));
        }
        if !fs::metadata(&path)?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "asset reference is not a regular file",
            ));
        }
        Ok(Cow::Owned(read_bounded(&path, max_bytes)?))
    }
}
pub(crate) fn read_bounded(path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    if file.metadata()?.len() > u64::try_from(max_bytes).unwrap_or(u64::MAX) {
        return Err(invalid("encoded file exceeds preparation limit"));
    }
    let mut bytes = Vec::new();
    let mut block = [0; 8192];
    loop {
        let count = match file.read(&mut block) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            return Ok(bytes);
        }
        if bytes
            .len()
            .checked_add(count)
            .is_none_or(|len| len > max_bytes)
        {
            return Err(invalid("encoded file exceeds preparation limit"));
        }
        bytes
            .try_reserve(count)
            .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
        bytes.extend_from_slice(&block[..count]);
    }
}
