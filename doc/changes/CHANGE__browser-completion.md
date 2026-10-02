# Browser song completion from shared output evidence

The browser Play source path now uses the same SongCompletion owner as native
play. It waits for all original chart judgments and BGM credit, clears local and
awaiting-ACK command work, then requires a later idle Mixer block and a reported
output position covering it. New producer work resets the drain barrier. The
Window joins already captured input and outstanding control operations before
the existing audio/game stop handshake, preserves actual score and restores the
accepted preview.

AudioContext output timestamps are converted directly to the armed Mixer grid
with integer arithmetic and conservative rational-origin rounding. UI elapsed
time does not advance the reported cursor. Missing, zero, stale, future,
regressing or prestart evidence cannot finish a song; manual Stop remains
available. Malformed owner/report data fails explicitly. Failed game disposal
during natural or manual stop terminates Worker ownership, retains the cleanup
error and requires reload. Natural-finish status cannot hide failed cleanup.

The supplied freshness policy is one second, not an accuracy guarantee. Browser
output timestamps are reported estimates; physical latency is unmeasured. The
input timeline still uses its nominal software anchor, so output clock drift
discipline, capture/replay, durable result storage and browser networking remain
unfinished.

Validation is source compilation only: host workspace all-targets, headless
application all-targets, WASM browser library and WASM browser-audio library
cargo checks exited successfully. Deferred fixtures cover real portable
Runtime/Mixer completion, direct Rust Worklet-word decoding, and actual helper,
Worker and Window modules with mocked browser/generated owners. No assertions,
JavaScript syntax checks, generated bindings, browser, audio/device output,
formal review or QA were executed. The full player Goal stays active.

The single output-word decoder lives in the existing portable application audio
boundary. BrowserGame and the ordinary host test build use that same decoder;
there is no duplicate test algorithm. All four source checks were repeated after
the mechanical move and exited zero. A WASM browser library test compilation
also exited zero before the move. Existing WASM platform dead-code warnings
remain; compiled fixtures are not runtime verification.
