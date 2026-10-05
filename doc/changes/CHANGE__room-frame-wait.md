# Common incomplete-frame deadline policy

Browser and native room reception now share RoomFrameWaitState. A partial
prefix starts a fixed deadline; additional fragments do not extend it. The
deadline is checked before a completing fragment can dispatch protocol changes.
Timely completion releases the bound for the next frame. Negative or regressing
observations and expiration seal the state.

The actual split-operation driver and native RoomPlayIo configure this policy
once while the decoder is empty. Native frame_timeout defaults to ten seconds
and accepts one millisecond through 120 seconds; app composition forwards its
existing IO stall setting. Native checks reuse existing clock observations.
The WASM facade preserves typed timeout/frame diagnostics. Worker timers wake
Rust policy and schedule its remaining duration, with cleanup on lifecycle exit.
Window rendering and gameplay hot paths gain no new work.

## Evidence and remaining work

Seven pure policy groups, two additional actual driver groups (seventeen total),
two actual native IO groups and four additional Worker adapter groups are
authored for later execution. Existing option fixtures cover the new field,
default and bounds. Driver fixtures use real protocol messages; native IO
fixtures retain original clock counts and actual late read history. Worker
fixtures script Rust outputs and do not independently prove protocol behavior.
Both writers returned terminal stop reports before scoped Rust formatting.
Four sequential compile-only checks exited zero: workspace/all-targets with
WebTransport, no-default-features WebTransport/all-targets, WASM browser/library
and WASM browser-audio/library. Existing unused-code warnings remain. Host checks
compile the Rust fixture children; WASM checks compile actual facade methods,
not generated JavaScript or browser behavior. JavaScript was read as text only.
Tests, JavaScript parsing, generated bindings, browser/native runtime, actual
transport timing, formal review, QA, verification and close remain deferred.

Browser policy uses the processing observation supplied before decoding. This
does not prove CPU decoding or callbacks complete before expiry. Native policy
also uses its existing fresh processing observation before protocol dispatch.
The full BMS player, browser controller integration and measured performance
remain unfinished; this change does not establish overall completion.
