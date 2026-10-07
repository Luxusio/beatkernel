//! Staged clipboard edits with no backend access or shared mutable editor.

use super::text_input::{LineEditor, MAX_LINE_BYTES};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardAction {
    Copy,
    Cut,
    Paste,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardRequest {
    Read,
    Write(String),
}

/// An edit tied to the complete editor baseline that prepared it.
/// Callers validate a cut candidate before submitting its write and check
/// `matches` again before committing any returned editor.
pub struct ClipboardEdit {
    base: LineEditor,
    request: ClipboardRequest,
    candidate: Option<LineEditor>,
}

impl ClipboardEdit {
    pub fn prepare(base: &LineEditor, action: ClipboardAction) -> Result<Option<Self>, String> {
        if base.composition().is_some() {
            return Err("clipboard actions are unavailable during text composition".into());
        }
        let (request, candidate) = match action {
            ClipboardAction::Copy | ClipboardAction::Cut => {
                let Some((begin, end)) = base.selection() else {
                    return Ok(None);
                };
                let request = ClipboardRequest::Write(base.value()[begin..end].to_owned());
                let candidate = if action == ClipboardAction::Cut {
                    let mut candidate = base.clone();
                    candidate.insert("")?;
                    Some(candidate)
                } else {
                    None
                };
                (request, candidate)
            }
            ClipboardAction::Paste => (ClipboardRequest::Read, None),
        };
        Ok(Some(Self {
            base: base.clone(),
            request,
            candidate,
        }))
    }

    pub fn request(&self) -> ClipboardRequest {
        self.request.clone()
    }

    pub fn candidate(&self) -> Option<&LineEditor> {
        self.candidate.as_ref()
    }

    pub fn matches(&self, current: &LineEditor) -> bool {
        &self.base == current
    }

    /// Completes a read or write without modifying the caller's editor.
    /// A successful write has no payload; a successful read has a text payload.
    pub fn complete(
        self,
        reply: Result<Option<String>, String>,
    ) -> Result<Option<LineEditor>, String> {
        match (self.request, reply?) {
            (ClipboardRequest::Write(_), None) => Ok(self.candidate),
            (ClipboardRequest::Read, Some(text)) => {
                if text.len() > MAX_LINE_BYTES {
                    return Err("clipboard text exceeds its byte limit".into());
                }
                let mut candidate = self.base;
                candidate.insert(&text)?;
                Ok(Some(candidate))
            }
            (ClipboardRequest::Write(_), Some(_)) => {
                Err("clipboard write returned an unexpected text payload".into())
            }
            (ClipboardRequest::Read, None) => Err("clipboard read returned no text payload".into()),
        }
    }
}

#[cfg(test)]
#[path = "clipboard_fixtures.rs"]
mod tests;
