# Browser Worker preparation fixtures

Worker result/history/archive tests must reach successful preparation before
asserting behavior owned by gameplay or recording. The fake generated-WASM
library follows the current production preparation surface; the real Worker,
numeric helpers and renderer/room ownership boundaries remain under test.

## Required preparation seam

1. Declare file paths and exact byte lengths without eagerly reading them.
2. Hydrate a declared file only when the Worker acquires its bytes. Check
   declared length, library lifetime and single hydration.
3. Begin the appropriate incremental chart, section, policy or replay job.
   Preserve each fixture's constructor argument mapping and replay bytes.
4. Advance through at least one not-ready turn, then readiness. Deliver the
   continuation on a queued `MessageChannel` host task, independently of
   Promise microtasks and deliberately held renderer/room ACKs.
5. Take the prepared owner once. Preserve cancellation, job/library cleanup
   and existing no-resurrection/late-transfer assertions.

A real incremental job can report the same pending asset path across processing
stages after its bytes have been supplied. Reuse acquired bytes while advancing
the job; do not require pending paths to be unique. At Ready, assert each
required asset was hydrated exactly once. This keeps genuine binding tests
aligned with production without weakening the declaration or length checks.

`app/web/play-worker.test.mjs` supplies the existing queued-task pattern.
Its flush tracks both queued tasks and a set of reserved delivery promises.
Concurrent flushers must join deliveries already removed from the queue;
otherwise setup can appear finished while another flusher still owns work.
Delivery drains microtasks without recursively awaiting its own reservation.
Both success and rejection remove the exact reservation, and rejection stays
visible to waiting flushers.

## Meaningful assertions

Successful import, accepted library, selection and preparation are explicit
preconditions. Keep original capture bytes, finalization, refusal, archive
ordering, member identity, stale transfer and free-count assertions. Do not
silence failures by weakening these assertions or by making preparation
synchronous. Preparation progress alone does not prove result completion.

Sample transfer fixtures must expose the current generated
`BrowserSample.take_pcm_array()` surface, retaining one-shot consumption,
original PCM contents and cleanup counters. An old `take_pcm()` fake can hide
the actual transfer refusal behind a later missing sample result; assert the
sample RPC error and result before using its kind.

Binding tests with actual generated WASM complement these Worker fixtures;
actual finite recording/save/comparison still requires interactive browser
acceptance. Node fixture passes do not prove GPU, physical input or acoustic
timing. Commands and observed results belong in the task change ledger.

## Compiled Module handoff

When a fixture uses host `structuredClone` for Worker messages, inject the same
host `WebAssembly` into nested VM contexts. Otherwise the cloned Module and
the receiving fixture's Module constructor belong to different realms, and a
genuine Module can fail the production `instanceof` guard. Keep the real
guard; align the fixture's constructor and clone environment instead.
Inject the shared fixture's real host scheduling primitives, including
`queueMicrotask`, into nested contexts too. A missing scheduler can make the
fake Worker throw before it delivers the Module, leaving Play unavailable.
Check the feedback immediately after CPU ready, before later preview feedback
overwrites the diagnostic. Do not replace queued replies with synchronous ones.

Record the Module actually delivered in the cloned Worker reply. Assert that
AudioHost receives that exact object and that it is a genuine Module, rather
than comparing it with the sending Worker's original object. Structured clone
preserves executable content, not JavaScript reference identity. Express
reference checks as boolean assertions so a failure does not place a Module
inside Node test-runner error serialization and obscure the original error.
Preserve the synchronous trusted-click assertion around AudioHost opening.

## Immutable recording handles and native storage completion

Align real host File and Blob constructors with the VM endpoints. Node's
structured clone can return a Blob for a File; use an explicit immutable File
endpoint fixture for Window tests and retain actual Chromium verification for
browser clone and lifetime claims. The fixture must not reacquire accessors
while walking a cloned message: preserve enumerable data properties, bound the
walk and fence cycles. Keep single-acquisition assertions in place.

Check whether the Worker harness initializes automatically before sending init.
Normal recording tests must assert one ready receipt and no fatal receipt;
pre-ready refusal tests explicitly opt out of automatic initialization. Genuine
File body reads can require a native event-loop turn. Wait for a bounded named
milestone in the affected recording tests, keeping the existing preparation
task queue and deliberately held reads unchanged.

Storage cancellation tests distinguish rejected caller promises from actual
opening and transaction terminal events. Hold and release those native events
separately, assert that physical capacity remains occupied, and preserve the
unknown-commit diagnostic when terminal evidence is missing. Record catalogs
use the production newest-first order; historical tests select the original
record explicitly rather than relying on an ascending fake catalog.

Inspect exact replay and archive bytes in the Worker or test driver. Production
Window assertions cover immutable handle type, extent, owner, roster and joined
completion, with traps for affected binary reads/construction and IndexedDB.
Handle identity alone does not prove capture-buffer uniqueness: retain the
Worker's raw alias-refusal and original member-byte assertions.

Actual two-player browser fixtures must enable the public touch-input control
before discovering keyboard plus touch sources. Desktop Chromium can expose
only the keyboard by default; waiting for a second source without enabling it
does not exercise local recording or storage. Preserve real source discovery,
explicit member selection and exact per-member capture/archive assertions.

## Current production audio acceptance

Use the cached toolchain; build `browser-audio` and immediately run the existing
wasm-bindgen into `app/web/audio-pkg`, then build `browser` and immediately run
bindgen into `app/web/pkg`. Both builds use the same intermediate cdylib path,
so generating both packages after both builds would mix their identities.
The cached offline commands are recorded in
[the native-frame diagnostic guide](../platform/GUIDE__browser-native-frame-diagnostic.md).
Do not install or update tooling as part of this acceptance run.

After all writers stop and both fresh packages exist, compute each actual WASM
SHA256 independently and provide both values:

```sh
EXPECTED_WASM_SHA256=<fresh-ui-wasm-sha256> \
EXPECTED_AUDIO_WASM_SHA256=<fresh-audio-wasm-sha256> \
CHROMIUM=/usr/bin/chromium \
PUPPETEER_MODULE=<existing-absolute-puppeteer-core-path> \
SETTINGS_POLICY_OUT=target/wf/<unique-existing-parent>/<new-run> \
node app/web/settings-policy.browser.mjs
```

The driver must reject missing/malformed hashes or either actual digest mismatch
before creating the evidence directory or starting processes. Freeze both
generated packages and the named host/worklet/clock/probe/encoding/failure
sources in its existing source snapshot. Serve those buffers, check their disk
hashes after the run, and assert the actual single server audio body matches
the supplied audio digest and frozen byte length. This establishes identity
for the explicitly snapshotted files, not the entire UI transitive import tree.

Preserve all 13 current journey steps and every strict page/console/network
assertion. Independently correlate Worker EOF, no completed-reader cancellation,
one release, compile extent and genuine Module publication with the actual
Worker CDP loading completion; full body reading and compilation cannot excuse
an abort. Inspect the rendered screenshots and cleanup diagnostics as well as
the status. Verify only this run's browser/Xvfb ended and its key, certificate,
profile and cache were removed, without touching other sessions' resources.

CDP sessions received from a page session's `Target.attachedToTarget` belong to
that attaching page session. Send `Target.detachFromTarget` with the original
child session ID through that owner. In the cached Puppeteer implementation,
calling the child's `detach()` instead sends at the root connection and fails
with `No session with given id`. An actual Chromium probe confirms the owner
command succeeds for the same child after the root command fails. Keep genuine
cleanup errors visible; browser exit alone does not substitute for detachment.

When the existing authorized-account wrapper resets PATH, invoke the already
installed Node by its absolute path. In this checkout the verified executable
is `/home/ccc/.local/share/mise/installs/node/22/bin/node`; this uses the cached
runtime and does not run mise or install anything. A wrapper's prelaunch exit127
is separate from an actual browser run and must not consume or overwrite its
evidence directory.
