//! Native publication of a fully encoded replay capture.

use super::{CaptureError, LiveReplayCapture};
use std::path::Path;

impl LiveReplayCapture {
    /// Encodes before filesystem effects; invoke after native cleanup.
    ///
    /// Publishes the complete, synced file exclusively, preserving existing
    /// destinations. Failure before publication leaves no partial final file.
    /// Owned staging cleanup is best effort, including after success. The caller
    /// owns the directory; success does not promise directory power-loss safety.
    /// Filesystems without hard-link support refuse without a write fallback.
    /// The hexadecimal 8.3 staging namespace and native aliases are reserved;
    /// such final basenames fail with `InvalidInput` before filesystem effects.
    pub fn save_new(self, path: &Path) -> Result<usize, CaptureError> {
        let bytes = self.into_bytes()?;
        crate::native_publication::publish_new(path, &bytes)?;
        Ok(bytes.len())
    }
}
