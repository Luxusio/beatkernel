# ASIO finite live prefixes

SDK-enabled Windows ASIO live solo and local 2..64-player owners now accept an
original-song `--end-ns`, strictly after `--start-ns`. They reuse the common
exclusive Mixer playback fence, original-song judging cap and keyboard/member
drain gates. Windows endpoint observation is shared by both owners.

ASIO completion consumes an actual rendered-block presentation observation,
including its assessed output latency and multimedia-clock host interval.
NativeEnd validates the immutable frame grid, origin, rate, domains and retained
physical marker. Prepared frames alone cannot complete a prefix. A real
observation at or below the endpoint is required before crossing, and the
completion frontier uses the first crossing block's upper host interval rather
than a midpoint or an invented native sample-position epoch. Fresh input/message
and member frontiers must reach that host point before completion. Cleanup and
join still precede automatic UI repetition.
Progressing blocks can share one coarse host upper timestamp. ASIO uses that
actual upper frontier directly, avoiding unnecessary endpoint interpolation;
regressing observations still fail atomically.

Known ceiling: completion waits for an observed block starting at or after the
endpoint and its supplied upper interval — finer endpoint presentation would
require native evidence with tighter timing bounds. Missing observations can
delay completion further. The supplied error assessments do not prove physical
accuracy. ASIO pause and network finite protocols remain unfinished. SDK-free
launch rejection, authored MIT licensing and conditional SDK-build licensing
remain unchanged; no driver or SDK is bundled.

Portable fixtures compose actual finite Mixer reports and ASIO presentation
observations with NativeEnd, including delayed upper-frontier completion and
invalid-observation rejection. Windows fixtures cover actual upper-interval,
logical-end and message-backlog completion gates, exact endpoint parsing,
local-player IDs and network rejection. Tests and native/driver execution are
deferred. SDK-free source compilation does not validate the enabled MSVC/SDK
branch or establish real ASIO acceptance.

Locked source compilation succeeded for Linux workspace all-targets, Windows GNU
and macOS app all-targets, headless app all-targets and the WASM graphics library.
Scoped rustfmt and diff checks also succeeded. No tests or native sessions ran.
