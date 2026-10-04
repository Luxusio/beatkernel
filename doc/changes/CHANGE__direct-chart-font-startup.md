# Owned direct-chart title-font startup

## Problem and behavior

Previously --chart with --title-font read the font file and built its atlas
before EventLoop/window startup, while library preparation already used an
owned worker. Extend the same owner with CPU-only preparation so direct title
font I/O and rasterization run outside the UI thread, without scanning the
parent directory. Library scan budgets and coalesced progress remain intact.

Direct preparation preserves the literal chart path, filename title and empty
artist. It does not parse or invent chart metadata. The bounded font/search
helpers are shared with library preparation. Publish the complete direct
selection only after actual join, active Selection and GPU texture preflight;
pending startup exposes Settings/Exit and no Start hit. Hidden or suspended
screens retain the owned result for later install; close cancels and joins it.
Font failure or cancelled work cannot publish partial selection or silently
continue without the requested font. No-font direct startup stays inexpensive.

## Evidence and limits

Both producer and independent author returned actual terminal STOPPED finals.
Source inspection confirms actual run delegates direct font preparation through
spawn_catalog to NativeCatalog::spawn_prepared and shared catalog parts. Font
I/O/rasterization is removed from direct startup before EventLoop. Original
filename selection, bounded library scan and active/hidden/suspended/closing
joined installation use the existing owner and no new crate or dependency.

Four independent deferred groups were added: native owner+2 (4 total), Desktop+2
(4 total). All four existing groups in those modules remain. Real owned thread
and channel gates cover nonblocking progress/poll, joined publication, errors,
cancel/drop and one result. Actual direct prepare/spawn and Desktop fixtures
use a nonexistent literal chart/parent and a supplied font file to cover no
scan/parse, filename search/glyph preparation, pending Start/hits, hidden and
suspended return, cancellation/close and missing/invalid/oversized font refusal.
The independent author also found an invalid common test initialization using
empty Options arguments; it now supplies --chart fixture.bms without weakening
the runtime option rule. These fixtures have not been executed.

Scoped rustfmt on the four changed Rust files exited0. Exactly four compile-only
checks after both terminal STOPPED finals exited0: workspace all-targets with
WebTransport (session20962), headless all-targets with WebTransport (59656),
WASM browser library (43798) and WASM browser-audio library (82399). Existing
three WASM cadence dead-code warnings remain. Source whitespace inspection
reported no diagnostics. Compilation does not establish assertion outcomes
or actual native startup, font pixels, cancellation timing or GPU behavior.

At this increment native profile startup remained synchronous. The dependent
[Owned startup profile](CHANGE__native-startup-profile.md) records that migration.
GPU and OS I/O calls cannot be force
interrupted by this cooperative cancellation. Actual native startup, focus,
rendering, close/restart, input/audio and performance remain unverified.
Only scoped Rust formatting and four compile-only checks are authorized after
both actual terminal STOPPED finals; no assertion/test or runtime execution.
Required ordered reviews and browser/CLI/desktop QA remain deferred and must
complete before eventual close of the full player task/Goal.
