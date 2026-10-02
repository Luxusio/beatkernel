# Shared multiplayer protocol components

The BMS application extracts native BKMP version 6 framing, progress/error/event
data, readiness/final ACK state and explicit-time clock probes into one transport
independent module. Native QUIC imports the actual shared components and keeps
its original public data paths through reexports. Clock/start, final-prefix
ordering and complete-frame write barriers retain their existing valid-input
behavior and wire bytes. Socket/thread/runtime ownership stays in the native
adapter; the shared components acquire no platform clock or transport I/O.

The subsequent [shared session change](CHANGE__shared-multiplayer-session.md)
also moves setup and control-message orchestration into the common module;
native QUIC keeps only its transport ownership and supplies explicit evidence.

A checked encoder bounds payloads before allocation. The incremental decoder
admits only the exact needed header/body prefix and returns consumed bytes,
allowing fragmented or coalesced transport chunks without dropping leftovers or
copying entire chunks. A held complete frame accepts zero more bytes until
taken; invalid lengths and internal excess extent reject explicitly. Native
reads use the same bounded admission surface. The contract is documented in
[the competition requirement](../kernel/REQ__bms-competition.md).

Known ceiling: shared source and compile-only evidence do not establish actual
QUIC socket behavior, WebTransport sessions, HTTP/3 endpoint compatibility or
acoustic synchronization. The browser transport/server and full competition
integration remain unfinished. Authored fixture assertions, browser/network/
hardware execution and formal review/QA remain deferred; the Goal stays active.
