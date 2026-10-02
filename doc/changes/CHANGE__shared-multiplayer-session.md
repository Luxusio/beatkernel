# Shared multiplayer session orchestration

The application protocol module provides one transport-independent session
owner for exact setup matching, readiness, clock probes, committed software
start, progress and final application acknowledgements. Native QUIC delegates
these transitions to that owner. The session composes the existing shared
protocol, probe and start components, retaining BKMP version 6 wire messages.

Transport adapters supply explicit elapsed timestamps and retain immutable
admitted frame bytes through partial writes. A checked frame ID identifies the
single pending complete-write receipt. Stale, duplicate or wrong receipts
reject; local write completion is separate from the final peer application ACK.
The session bounds event storage and exposes an application slot only after
control-message priorities and the committed start permit dequeueing progress.
Native connection, framing, channels, deadlines and cancellation stay in the
adapter. Invalid time or fatal protocol errors make the session unusable.

This prepares the shared state owner required by a browser adapter. The subsequent
[browser transport change](CHANGE__browser-multiplayer-transport.md) provides
callable WASM bindings and WebTransport stream source. HTTP/3 endpoint service,
gameplay-derived setup and browser competition integration remain unfinished.
WebTransport connects to an HTTP/3
service and opens a bidirectional stream; the native raw-QUIC ALPN is not that
service. See [MDN WebTransport](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport)
and [bidirectional stream creation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/createBidirectionalStream).

Independent fixture assertions and browser/network/hardware execution remain
deferred, as do formal review and QA. Compilation alone does not establish
physical synchronization, peer receipt or a usable multiplayer service. The
full BMS player Goal remains active.
