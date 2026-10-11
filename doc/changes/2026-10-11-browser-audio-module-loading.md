# Browser audio module preparation ownership

Status: implementation in progress; no independent verification claim.

Audio package preparation is being moved from Window into the existing
business Worker. Window will receive only a correlated compiled Module, while
file import and chart preview remain available independently of optional audio
setup. The [preparation requirement](../runtime/REQ__browser-audio-module-loading.md)
defines the 64 MiB binary ceiling, ten-second deadline, physical single-flight
capacity, current-owner admission and nonblocking retirement.

Development checks currently pass twelve actual-WASM Worker setup tests and
ten Window ownership tests. Worker cases include the exact 64 MiB boundary,
fresh-id retry, uncancelable compile capacity after timeout and actual gameplay
capture-before-fatal retirement. Window fixtures use a genuine Module and fail
if Window invokes audio fetch or compilation. These are focused development
checks, not independent QA or actual browser acceptance.

Original scoped failures remain in
`target/wf/audio-setup-worker-development-12b49.log` and
`target/wf/audio-setup-worker-development-fixed-12b49.log`. The first revealed
cross-realm transfer-array comparison and premature assertions before native
compilation settled. After those fixture corrections, retries exposed a reused
exhausted response reader; each fetch now provides a fresh reader. Assertions
were preserved. The successful rerun is
`target/wf/audio-setup-worker-development-retry-12b49.log`; Window evidence is
`target/wf/audio-setup-window-development-12b49.log`.

The retained room-cleanup fatal branch also passed its focused production
Worker/controller fixture, with pending fetch and pending compile variants.
The actual-WASM fixtures establish real gameplay/capture; this separate room
fixture controls transport and model owners and does not claim real QUIC or
physical playback. Evidence:
`target/wf/audio-setup-room-retirement-development-12b49.log`.
The production-browser ownership probe remains pending. No new WBS item is
promoted or task closed.

## Known ceiling

This removes confirmed Window loading/copy/compile invocation; physical input
latency and overall performance superiority have not been measured. Setup may
temporarily occupy the existing business Worker. Moving ownership does not
establish the cause or repair of the recurring Chromium audio-WASM abort;
all strict production browser diagnostics remain required.
