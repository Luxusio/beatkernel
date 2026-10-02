# Browser multiplayer binding and transport source

The existing application crate exposes WASM bindings for its shared multiplayer
Session and FrameDecoder. Native QUIC and browser bindings use the same setup,
readiness, probe, software start, progress and final ACK transitions. Exact
signed song times and unsigned counters cross into JavaScript as BigInt. The
binding accepts caller-supplied canonical identity and elapsed time; it does
not acquire a clock or derive gameplay setup itself.

The plain JavaScript transport module opens one WebTransport bidirectional
stream, retains one reader and writer, and admits bounded read prefixes and
immutable outbound snapshots. Setup and I/O waits have finite deadlines.
Cancellation, remote close and late results cannot resurrect a closed owner.
Cleanup is idempotent and does not wait indefinitely on platform promises.
One read and one write may proceed together; extra concurrent operations reject
instead of growing an application queue.

Read prefixes are limited to 65,547 bytes and retained incoming chunks to 1 MiB.
Callers slice prefixes to the Rust decoder's `needed_bytes` before crossing the
WASM glue boundary, avoiding a copy of an entire coalesced transport chunk.
The adapter resolves writes only after the stream writer accepts the complete
snapshot. The caller then reports its matching frame ID to the shared session;
that local observation does not establish the separate peer application ACK.

A subsequent source inspection found that detached views were treated as empty
chunks. The adapter now validates attachment before skipping a zero-length
chunk, rejecting a detached buffer immediately. The existing independent
negative fixture covers that branch but remains unexecuted. This uses the
[ECMAScript typed-array construction rule](https://tc39.es/ecma262/multipage/indexed-collections.html#sec-initializetypedarrayfromarraybuffer)
without copying the backing buffer.

WebTransport requires an HTTPS HTTP/3 service and a bidirectional stream, as
documented by [MDN WebTransport](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport)
and [stream creation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/createBidirectionalStream).
A [stream writer promise](https://developer.mozilla.org/en-US/docs/Web/API/WritableStreamDefaultWriter/write)
describes local sink completion, not peer application consumption.

Known ceiling: these callable source components are not yet connected to the
gameplay-derived setup, browser multiplayer controls or a compatible HTTP/3
service. Generated bindings, actual browser/network sessions and independent
fixture assertions have not run. Formal review, QA and hardware acceptance
remain deferred; the full player Goal stays active.

After both writers stopped, workspace/all-targets, headless/all-targets,
browser-WASM and AudioWorklet-WASM `cargo check` commands each exited zero.
The browser check includes the actual binding module. Existing WASM platform
dead-code warnings remain. Eight independent transport fixture groups are
source-authored; JavaScript parsing and assertions were not executed.
