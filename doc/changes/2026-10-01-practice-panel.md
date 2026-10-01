# Graphical precise practice-start panel

Settings now opens Practice with its button or F6. The retained editor accepts
exact decimal seconds, M:SS or H:MM:SS with up to nine fractional digits and
shows the resulting original-song nanoseconds. It uses checked integer
arithmetic through nonnegative i64 maximum, including long-song positions.
Malformed, overflowing and overprecision input is rejected without rounding.
The editor is bounded to 64 bytes; empty means zero. Canonical formatting
preserves exact nanoseconds. Existing raw --start-ns settings stay supported.

Done/Enter prepares the updated parent Settings values/editor before popping
the child route. It changes only --start-ns, preserving other settings and
repeated bindings/opponents. Validation failure preserves both the child and
previous parent draft. Reset/Full Song changes only the child editor; Back/Escape
discards edits and resumes the original Settings instance. Settings Apply and
profile Save remain separate. Next native playback and immutable F5 retries
use the existing fresh-section transport/judge/audio/record paths; no live seek,
wall-clock progress or synthetic historical hold/keysound state was introduced.

Practice owns a typed PanelScope and Floem reactive scope with retained static,
editor/preview, button and error nodes. Editor/error/button dependencies update
independently. Child exit/Closing disposes subscriptions. Main-thread bindings
perform no native I/O or device/chart discovery. Selection and Practice share
the existing event-driven composition/upload cache and surface retry policy;
metadata/game ownership and other menu drawing remain separate. The explicit
Navigator gains one Settings child without increasing its four-entry maximum.

Known ceiling: this is fresh start-to-song-end practice. Bounded loops, live
scrubbing, pause/resume and historical active-hold reconstruction remain
unfinished. Input syntax is unsigned ASCII; colon seconds must be below 60,
and three-part minutes below 60. A syntactically valid position may still be
outside a particular chart and is subject to the existing native preparation
policy. Presentation still uses the fixed 960x720 logical viewport; changed
nodes cause full visible-packet concatenation and rectangle uploads.

Three pure parser/settings and two reactive-view fixture groups plus two desktop
draft/lifecycle groups were authored and compiled only. Existing Navigator
ancestor/admission fixtures include the new child. Product/tests/GUI/native/GPU/
shader/device/actualfile/network execution, benchmarks and independent formal
review/security/QA remain explicitly user-deferred. The full player Goal and
Harness task remain open; no acceptance PASS or completion is claimed.

Rust 1.98.1 app all-target source compilation succeeded on Linux, Windows GNU
and macOS; headless all-target and WASM graphics-library checks succeeded.
Scoped formatting and diff whitespace checks succeeded. Existing macOS block0.1.6
future-incompatibility and WASM cadence warnings remain.
