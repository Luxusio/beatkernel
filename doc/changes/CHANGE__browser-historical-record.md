# Browser loaded historical result presentation

Saved replay selection needs to retain its archive and original player association
through the Worker boundary. A separate historical Rust presentation decodes and
matches stored data and caches geometry without converting history into live
completion evidence. Window remains the storage/control adapter, and Worker
owns rendering and historical binding lifetime.

## Known ceiling

Actual browser/WASM/IndexedDB/GPU acceptance and performance remain unverified.
Native local-member sidecar discovery and rich score/timing/comparison archival
remain separate work. Header equality establishes consistency of editable stored
files, not authentication or live completion proof. Execution and formal review/QA
remain deferred.

## Implementation evidence

The graphics-gated pure presenter fully decodes bounded replay/archive bytes,
uses the shared exact original-player matcher, and builds a small-capacity frozen
geometry packet. Repeated composition appends that packet without formatting
historical strings or querying timed playfields. The separate WASM binding
exposes only availability and diagnostics; it never supplies live completion
proof. BrowserCanvas renders it on the Worker while idle.

Window's stored replay selection forwards its opaque archive with the original
member ID and retains the selected replay if historical display fails. Matching
owner/operation checks and a ten-second deadline bound the response. Worker
captures the originally admitted replay size before asynchronous acquisition;
epoch invalidation prevents late reads from adopting bindings after clear,
selection, play or disposal. Clear and invalidation release retained bindings.
Legacy absence clears history without inventing a result.

Both paired writers stopped before scoped Rust formatting and the exact four
compile-only checks. Workspace/all-targets with webtransport, runtime/all-targets
without defaults with webtransport, wasm32 library with browser, and wasm32
library with browser-audio all exited zero. Existing dead-code warnings remain.
Seven independent deferred fixture groups were authored (Rust 3, actual Worker
VM 4). No fixture assertions, JavaScript parsers, generated WASM, native/browser
apps, filesystem/GPU operations, benchmarks or formal review/QA ran.
