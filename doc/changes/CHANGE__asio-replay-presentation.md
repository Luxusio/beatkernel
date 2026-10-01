# ASIO recorded presentation and graphical finite retries

SDK-enabled Windows ASIO Watch now advances the existing replay visual and
completion logic from actual rendered-block presentation observations. Explicit
multimedia-clock selection and timer/drift/output-latency assessments are
required. Natural recorded-prefix completion no longer requires diagnostic
seconds. ASIO pause remains unsupported.

The portable presentation component retains actual blocks until fresh QPC
reaches their assessed upper host interval. Newer prepared blocks cannot replace
and starve older pending maturity. Repeated blocks keep their originally admitted
upper interval without occupying another slot; missing readings release already
observed mature blocks, then freeze the last presented cursor. Origin, rate,
domains, grids, counters, host chronology and queue capacity reject atomically.
No raw sample-counter epoch or software/time extrapolation supplies presentation.

GUI Watch preserves ASIO clock assessments, exact frame buffer and selected
channel order. Omitted channel count follows routing; omitted rate is queried
from the selected trusted driver before PCM/files without creating a stream,
starting callbacks or changing its rate. The driver control releases before
its owning window, including query failures. Standalone replay format remains
explicit. The obsolete SessionLaunch ASIO finite-retry veto is removed; actual
native preflight still owns admission before cancel/join/replacement.

Known ceiling: at most 4096 distinct observed blocks can await their upper
frontiers. Overflow fails explicitly without dropping evidence; missing
observations may delay completion. Caller-supplied error bounds do not prove
physical timing. SDK-free compilation does not check the enabled MSVC/SDK path
or establish actual driver/visual/acoustic behavior.

Prepared fixtures compose real Mixer reports, ASIO observations and replay drain
completion, including continuous future-block admission, exact upper boundaries,
missing/repeated/coarse readings and atomic rejection. Parser/conversion/retry
fixtures cover explicit clock bounds, natural playback, output configuration,
stable local identities and pinned recording ordinals. Tests, native playback
and formal acceptance remain deferred; authored MIT and conditional SDK-build
licensing are unchanged.

Source checks completed with Rust 1.98.1: workspace/all-targets on the host;
application/all-targets for Windows GNU, macOS and no-default-features;
and the graphics library for wasm32-unknown-unknown. All five cargo check
commands exited successfully. Scoped formatting and git diff --check also
passed. Fixtures were compiled where applicable, not executed; the enabled
ASIO SDK/MSVC branch and physical devices remain unverified.
