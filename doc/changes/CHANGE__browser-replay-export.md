# Shared recording and browser replay export

StepGameplay optionally captures actual RuntimeReport operations through the
existing LiveReplayCapture. The prepared branch seed is carried into canonical
native replay metadata. Accepted input provenance and drift-corrected song time
remain unchanged; no alternate judge, operation model or byte format is added.

Report and operation capture failures retain committed score, actual partial report and
the prior accepted prefix. Capture can coexist with judge/audio/score failure
evidence. Export is stopped/fenced-only and consumed once, including an encoding
failure. LiveReplayCapture encodes with its original limits without file I/O.
Native exclusive saving reuses that encoder.

Browser recording is unchecked by default and capped at 64 MiB encoded data and
1,000,000 operations. These are recording limits, not a process-memory cap or
song-time range. The Worker attempts stop, optional export and free in order,
always attempting release and reporting serialization failures separately from
resource cleanup. Whole owned buffers are transferred once before WASM release.

The Window waits both game and audio cleanup before offering an explicit replay
download. Complete labeling requires actual natural completion and successful
cleanup. Manual stop, cancellation, recording failure and cleanup failure retain
prefix semantics. Layout/size/metadata validation does not certify canonical
file contents or acoustic behavior. Completeness is a UI/filename label; the
existing replay format and noncryptographic setup identity remain unchanged.

At most one last captured result is held. Blob URLs are created only on explicit
download, revoked on replacement/page hiding and after 60 seconds.
Local replay import/playback and explicit saved-record catalogs were subsequently
source-integrated; browser competition/transport remain unfinished.

Host workspace all-targets, headless application all-targets, WASM browser
library and WASM browser-audio library checks exited zero after the relevant
Rust writers stopped. The 22 StepGameplay fixture groups compile, including
actual canonical ReplaySession parity, seeded provenance, negative preroll and
bounded failure prefixes. Their assertions were not executed. Existing WASM
platform dead-code warnings remain. JavaScript fixtures remain authorship only.
Runtime file writes/downloads, generated bindings, browser/audio execution,
JavaScript syntax checks, formal review, QA and acceptance remain user-deferred.
The full player Goal stays active.
