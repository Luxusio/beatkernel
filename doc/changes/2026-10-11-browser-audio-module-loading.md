# Browser audio module preparation ownership

Status: implementation reviewed and actual browser checks pass; full Node
verification remains unfinished after test-environment remediation.

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
Independent deep code/security review returned PASS at source `a4e56ad`.
Both independent production QA-browser and UX runs completed all twelve
journeys with zero page, console or network errors. Window audio preparation
records were zero; Worker read/compiled the real 62,096-byte package once,
published the actual Module and independent CDP recorded request completion.
Actual live/captured replay and original Display/profile journeys passed.
Evidence: `target/wf/audio-setup-qa-browser-12b49-1/evidence.json` and
`target/wf/audio-setup-ux-browser-12b49-1/evidence.json`. Both runs recorded
Chrome/Xvfb exit zero, closed HTTPS and removed owned profile/cache/TLS;
two already-removed CDP session detach diagnostics per run are preserved.
The QA-browser bootstrap initially lacked CHROMIUM and failed before browser
creation; the actual run used `/usr/bin/chromium` and its bootstrap log remains.

The first independent full Node suite actually finished 924 passed / five
failed (929 reported tests), exit one. Four loading-state failures came from
the nested fixture's missing `queueMicrotask`: direct CPU-ready diagnostics
confirmed the fake Worker could not send its reply. Aligning host WebAssembly
alone still failed those four tests; both the constructor and real scheduler
are now injected. The other failure was Node error deserialization following
an obsolete gesture assertion comparing the delivered cloned Module with the
sender's original object; remaining tests in that file were not established
by this failed run. Original evidence is preserved in
`target/wf/audio-setup-qa-cli-12b49-1/node-full.log`.

The fixture now records the actual delivered Module, checks genuine type and
identity with the AudioHost argument using a boolean assertion, and retains
synchronous trusted-gesture checks. Product and browser-probe sources were not
changed. Sixteen focused loading/ownership/gesture tests pass after remediation;
the earlier four failures and successful correction are respectively recorded
in `audio-setup-fixture-remediation-development-12b49.log` and
`audio-setup-fixture-remediation-fixed-development-12b49.log` under `target/wf/`.
Fresh independent review and complete-suite rerun are still required. No new
WBS item is promoted or task closed.

## Known ceiling

This removes confirmed Window loading/copy/compile invocation; physical input
latency and overall performance superiority have not been measured. Setup may
temporarily occupy the existing business Worker. Moving ownership does not
establish the cause or repair of the recurring Chromium audio-WASM abort;
all strict production browser diagnostics remain required.
