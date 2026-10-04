# Whole-grapheme text editing

The shared desktop LineEditor moves, selects and deletes committed text using
extended Unicode grapheme boundaries. Combining marks, decomposed Hangul,
emoji modifiers and joined emoji remain together. Regional indicator grouping
uses full-string context. Existing public positions remain UTF-8 byte offsets;
native preedit keeps its exact scalar byte cursor and selection, including a
missing cursor. Successful edits snap the committed caret forward when changed
neighbors form a new cluster. Invalid controls and capacity violations preserve
the complete editor.

Segmentation runs on editor commands over at most 4096 bytes, rather than on
rendering frames or gameplay/audio callbacks. The application directly reuses
locked unicode-segmentation 1.13.3 with its MIT license text retained. This adds
no workspace crate and leaves the ASIO distribution split intact.

Six deferred fixture groups use literal byte expectations for navigation and
deletion, full-string regional indicator parity, shifted selection and
replacement, neighbor joins, atomic control/capacity refusal, and native
preedit/cancellation versus subsequent ordinary edits. The fixture source is
compiled with the workspace all-targets check; no test body was executed.

After both writers returned terminal STOPPED, scoped rustfmt and whitespace
checks succeeded. Rust 1.98.1 compile-only checks completed successfully for the
workspace/all targets with WebTransport, headless/all targets with WebTransport,
WASM browser library and WASM browser-audio library. Existing WASM native-cadence
dead-code warnings remain. These checks did not build generated JS bindings,
link/run the player or exercise physical inputs.

## Known ceiling

Visible text windows and font decorations still use scalar metrics; whole-cluster
window boundaries are subsequently implemented in
[whole-grapheme input windows](CHANGE__grapheme-windows.md). Shaping,
mouse/word selection and font fallback remain pending.
Native keyboard/IME behavior and performance require later execution. Source
fixtures and compile-only checks do not establish runtime acceptance. Tests,
formal reviews and QA remain deferred by the user; the full player task remains
open.
