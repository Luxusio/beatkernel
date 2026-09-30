# Native BMS rolling background command admission

The separate BMS runtime crate maps prepared BGM once onto the native output
frame grid and admits commands as completed mixer rendering advances. Native
composition queues retain fixed finite command/pending capacities while total
BGM count follows the parser's independent limits. Lookahead, nominal live slack,
outstanding BGM credits and bounded per-poll work are explicit. Commands keep
their original mapped timestamps and identities; cursor regression, arithmetic
failure, admission failure and already-rendered unadmitted cues are reported
without silent drops or receipt-time replacement. Startup calibration feeds the
same producer before Runtime owns it; gameplay uses Runtime::enqueue_audio.
This uses preloaded WAV assets, not PCM streaming or added codec support. Native
playback, tests and formal review/QA remain deferred, with the full Goal active.

Locked Rust 1.98.1 compile-only checks passed after both implementation lanes
and the independent fixture author stopped writing: workspace/all targets on
the Linux host, plus platform/BMS runtime all targets for x86_64-apple-darwin and
x86_64-pc-windows-gnu. Nine independent BGM fixtures and native portable CLI
lookahead fixtures were authored and compiled, not executed. The fixtures cover
literal PCM and fractional ceil placement, 65,537 sparse cues with one credit,
equal-time ordering, render-end credit boundaries, budget/credit deferral, exact
Full/Disconnected prefixes and wide overflow/late-cue atomic errors. These checks
establish compilation only; native linking/playback, acoustic synchronization and
Apple ARM64 compilation remain unestablished.
