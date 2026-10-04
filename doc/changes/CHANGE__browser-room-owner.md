# Browser room admission ownership

BrowserRoomClient exposes the actual common RoomClientSession and bounded BKMR
decoder through WASM. Accepted-message revisions, participant IDs and deadlines
retain their full widths. Requests and complete-write credits keep their common
Rust validation. JavaScript moves bounded prefixes and complete snapshot DTOs.

The Worker-oriented BrowserRoomOwner component acquires one WebTransport channel,
keeps one read and one write outstanding, and wakes queued writes directly.
Setup remains bounded until the first accepted snapshot. Idle acquisition waits
for data or cancellation; a begun frame has one deadline across its fragments.
Leave confirms a complete local write only. Closing fences WASM immediately and
joins owned loops plus pending channel API promises. The channel separately
owns best-effort platform cleanup; this does not certify platform resource release.

Six independently authored owner fixture groups and one additional transport
fixture group are prepared (ten transport groups total). They cover actual write
credit, deadlines, recoverable requests, bounded full-width snapshots, Leave,
and cancellation with independently delayed read/write continuations. Their
source exposed an early-close join defect; the owner now registers each channel
operation before invoking it and joins those operations during close.
No JavaScript parsing, test or runtime execution has been performed.

Scoped Rust formatting and four locked compile-only checks completed with exit
0: workspace all targets with webtransport (85775), headless app all targets
with webtransport (18093), WASM browser (38778), and WASM browser-audio (21702).
Existing WASM audio cadence dead-code warnings remain. Linux/WASM compilation
does not establish active Windows/macOS or browser runtime acceptance.
Generated WASM glue, actual Worker/page lobby integration, multi-host clocks,
shared gameplay starts/progress/final ACKs, TLS/browser interoperability and
physical platform/device validation remain unfinished. Ordered review/security
and required browser/CLI/desktop QA remain deferred and mandatory before close.
The full player Goal and Harness task remain active/open.
