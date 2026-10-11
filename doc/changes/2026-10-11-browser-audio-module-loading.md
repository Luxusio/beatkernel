# Browser audio module preparation ownership

Status: scoped implementation and independent code/security and full CLI checks
passed. Earlier browser/UX runs passed, but the latest strict browser run failed
on audio-WASM cancellation. AC-003 remains unfinished; the full player Goal
remains active and this task cannot close.

Audio package preparation now runs in the existing business Worker. Window
receives only a correlated compiled Module, while
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
Fresh independent deep formal code review passed at `ac1479e`. The full serial
Node rerun then completed **1,051 tests / 1,051 passed / zero failed**, exit zero,
with no cancellation, skipped tests or todo. This establishes the previously
unverified remainder of the gesture fixture file, as well as loading-state,
Worker setup, deferred room-fatal and Window ownership tests. Evidence:
`target/wf/audio-setup-qa-cli-12b49-2/node-full.log`. Command:

```bash
source target/toolchain/env.sh
node --experimental-vm-modules --test --test-concurrency=1 app/web/*.test.mjs
```

The product/probe sources are unchanged between implementation `a4e56ad` and
fixture correction `ac1479e`; independent review and CLI QA checked all recorded
browser source/generated hashes against current files. Existing generated WASM
was used without a Rust source change or unnecessary rebuild. The main package
SHA-256 is `ab1732c2308c34815c06e8c6bc6d83839835f1d347425962122010427b9fff47`;
the 62,096-byte audio package is
`dde5198fb9779d4f1f8c5927ca446f929ea1fcdab8c8ca1a15ebaf46f35453b8`.
Syntax checks and `git diff --check` passed. No new WBS item is promoted:
91/193 remains the broad software-completion inventory.

These logs and browser JSON are local ignored artifacts under `target/wf/`;
they are not shipped by these commits. Reproduction requires the pinned Node
environment and actual generated WASM. The production browser probe uses
`CHROMIUM=/usr/bin/chromium`, an installed Puppeteer module via
`PUPPETEER_MODULE`, the main package hash via `EXPECTED_WASM_SHA256`, and a
unique `SETTINGS_POLICY_OUT`, then `node app/web/settings-policy.browser.mjs`.
The fixture constructor/scheduler/received-clone rule is captured in the
[fixture guide](../verification/GUIDE__browser-worker-preparation-fixtures.md).
No narrative PASS substitutes for Harness completion receipts.

On 2026-10-12 a fresh third production browser QA run at the same product source
completed all twelve public journeys, then exited one: the strict assertion
`actual Worker audio request must complete in CDP` failed. Both Puppeteer and
the independently attached Worker CDP session recorded HTTP 200 followed by
`net::ERR_ABORTED`; no matching Worker `loadingFinished` existed. Application
observations still recorded full 62,096-byte consumption, reader release without
cancel, successful compilation and matching genuine Module publication, and
Window preparation remained zero. Those successes do not override the strict
network failure. Page and console errors were zero; both network error records
remain preserved. Original evidence and log:
`target/wf/audio-setup-qa-browser-12b49-3/evidence.json` and
`target/wf/audio-setup-qa-browser-12b49-3.log`. Cleanup recorded Chrome/Xvfb exit
zero, closed HTTPS, removed owned directories and the two already-removed CDP
session detach diagnostics. The earlier two PASS results remain historical
evidence; they do not establish current complete browser acceptance. AC-003 is
unfinished pending diagnosis and correction without weakening its assertions.

## Known ceiling

This removes confirmed Window loading/copy/compile invocation; physical input
latency and overall performance superiority have not been measured. Setup may
temporarily occupy the existing business Worker. Moving ownership does not
establish the cause or repair of the recurring Chromium audio-WASM abort;
all strict production browser diagnostics remain required.
