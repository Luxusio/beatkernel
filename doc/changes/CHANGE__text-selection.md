# Shared text selection in desktop drafts

Search, settings values/profile, display options, practice start/end and record
directory fields now share scalar text selection. Shift+Left/Right/Home/End
extends from the initial caret. Ctrl+A on Windows/Linux and Command+A on macOS
select all using the logical character, and the shortcut does not insert A.
Extra modifiers do not trigger these selection commands.

Typing replaces the selected text after validating the replacement's final byte
count and control characters. Rejected edits preserve the whole editor.
Backspace/Delete remove a selection once; ordinary Left/Right collapse it.
IME previews replace selected base text in a clone, cancellation preserves the
base selection and commit replaces it once. Native composition keeps keyboard
ownership and its existing cursor visibility rules.

The existing borrowed projection and focused text-field painter render selection
without another rendering layer. Selection-only changes invalidate the relevant
retained input node. Each editor retains its own selection across focus changes;
unfocused fields hide it. Held modifier state clears when UI becomes unavailable,
including focus loss, occlusion, suspension, pending work and closing.

Prepared fixtures cover UTF-8 boundaries, anchor direction changes, full-capacity
replacement and rejection, selected deletion, clipped scene geometry, retained
field-only invalidation, all seven desktop draft targets, platform shortcut
mapping, text-event consumption, lifecycle and IME cancel/commit behavior.
Tests and native keyboard/IME/GUI acceptance remain deferred.

Source checks with Rust 1.98.1 passed for the workspace/all targets, Windows GNU
and macOS application/all targets, headless application/all targets and WASM
graphics library. The first native checks found a borrow conflict through the
PanelScope wrapper when indexing the selected display editor. Saving the index
before borrowing the editor resolved it; the three affected native checks then
passed. Existing macOS block future-compatibility and WASM cadence warnings
remain. These checks compile fixtures without executing them and do not verify
SDK-enabled ASIO, native keyboard/IME behavior or GPU output. Review and QA were
also deferred by the user's instruction.

Clipboard, mouse/word/grapheme selection, multilingual input glyph shaping and
candidate-window positioning remain separate work. This does not extend IME
to the display, practice or records dialogs.
