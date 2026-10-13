# Browser native context clock integration

## Scope and behavior

The browser Host and Worklet now use one fresh marker and three native a-rate
phase lanes to identify the original audio callback frame. Pure Rust combines
scalar residues with checked u64 arithmetic; the adapter validates every
original array sample before readiness, callback-time arm, PCM or refill.
Constructor ACK and scheduled metadata do not establish native readiness.
Two contiguous native callbacks certify one immutable origin. Whole future
refills advance all three own horizons without rewriting the original program.

Live play, local play and immutable replay retain the same strict Rust audio
runtime, natural output end and report29 coordinates. No chart, replay or user
profile schema migration was introduced. The requirement and procedure are
[REQ](../platform/REQ__browser-native-context-clock.md) and
[GUIDE](../platform/GUIDE__browser-native-frame-diagnostic.md).

## Author verification

The fresh audio and UI builds and matching wasm-bindgen generation exited0
using cached Rust1.98.1 and wasm-bindgen0.2.129. Cached CC/AR were necessary for
native dependency build scripts. The audio artifact was generated before the
UI variant overwrote the shared target cdylib. Existing platform/app warnings
were observed; this task does not claim the full build is warning-free.

| Artifact | Bytes | SHA256 |
|---|---:|---|
| audio-pkg WASM |77456|`c50fdb9b8cf6e5cc6f0b7c2a17a488692f00c7d1c600ea8a4067a105bb567ecd`|
| pkg UI WASM |5067734|`5484e86029689e407bab54ad05ef5bdbe44f88b5107d02a734129a34d9e23b13`|

Generated native_clock_start/extend/frame/ready and arm_words glue calls are
scalar; no callback array import or BigInt result is required.

| Author check | Actual result |
|---|---|
| Pure native clock Rust fixtures |28 PASS, including independent u128 CRT, safe-domain/long arithmetic and actual WorkletAudio composition|
| Native JS helper |39 PASS|
| Actual fresh audio WASM bridge |22 PASS|
| Host |Initial67 PASS; post-review71 PASS; paused-opening refinement82 PASS with11 more transition/cancellation regressions|
| Actual Worklet source with scalar spies |53 PASS:27 existing and26 native/cancellation/allocation cases|
| Chronology controller before fixture import alias fix |93 PASS|
| Affected controller/observer branches after alias fix |66 PASS, including five actual served complete-module compilation regressions; final full file now has98 tests|
| Existing numeric helper |51 PASS|
| Strict actual cold mode |cold-1/2/3, native-gap, late-arm and sample-control PASS; three supplemental normal results also PASS|
| Real live UI |16 required checks PASS, including nonzero-section and actual1.25s preview seek/restart|
| Genuine capture-to-replay UI |10 required checks PASS, including0.25–2s section and fresh-context restart|

Rust fixture tests first exposed admission-error precedence; the smallest pure
validation correction passed all28. Host tests exposed canceled-refill cleanup
retiring the fresh stop ACK; cancellation now preserves that cleanup path.

The first long browser attempt failed before module loading because observer
and production bytes declared nativePhase twice. Its unchanged failed evidence
is `target/wf/browser-native-clock-product/author-long-13686/evidence.json`.
Only the fixture import aliases changed; actual served-module regression tests
now cover all five modes. The second attempt passed:

| Long actual native result |44100Hz|48000Hz|
|---|---:|---:|
| Observed callbacks |32780|32776|
| Checked active lane samples |12583104|12583104|
| Maximum represented-ideal error |0.06250000069849193|0.06250000069849193|
| Whole accepted refills |2|2|
| Crossed OLD horizons |3148608 /3148605 /3148599;4197184 /4197180 /4197172|3148736 /3148733 /3148727;4197312 /4197308 /4197300|
| Final observed native end |4197248|4197376|
| Retained original callback receipts |7|7|

Crossing receipts require phase2, actual report availability, nonzero PCM and
the unchanged armed start. Both fresh-context and web-server-boundary checks
passed. Browser/server closure exited0; the live/replay owned PIDs were absent
and reusable binds for8165/8167/8168 succeeded. Raw counter agreement alone
cannot pass these checks.

Actual author evidence is under `target/wf/browser-native-clock-product/`:
`author-cold-13686`, `author-long-13686-02`, `author-live-13686`, and
`author-replay-13686`. UI screenshots and genuine .bkr records are retained
there. The ignored generated artifacts and evidence are local rather than
durable committed binary proof; each run records current source/WASM hashes.

## Independent gate status

The first independent DEEP review of95028a36 returned FAIL with three FIX_NOW
findings: repeated startup failure retired its own stop ACK; valid queued
refill progress was checked against canceled active geometry during stop; and
the replay runner could click disabled Play before successful cleanup ended.
Both discovery attempts completed, then the full formal reviewer independently
verified the three findings and the complete18-path scope.

The Host correction adds no state: reentering the same owned failure joins its
existing stop, and stopping ignores known clock progress only after validating
generation/version/id. Exact canceled ACK, unknown kind and malformed identity
checks remain. Four actual Host regressions require opening/stop to remain
pending with context still owned before the real stop ACK; abort, remote
rejection, accepted late commit plus next refill, and malformed teardown
identity/ACK are covered. Root reran all71 Host tests successfully. The replay
runner now waits for Play, speed and file controls to be enabled after Worker
terminal, preserving its existing20-second bound and original assertions.

The original author evidence above precedes that Host correction; fresh
post-remediation author evidence and final independent verification are
recorded separately. At this checkpoint final DEEP review, fresh CLI/browser
QA, documentation review and task verification remain pending. Author results
are not an authoritative review/QA PASS.

Post-remediation `author-cold-13686-02` passed the three cold contexts and all
labeled negatives. `author-replay-13686-02` passed nine gameplay checks but
failed clean-console-network on initialization audioWASM ERR_ABORTED. The one
bounded retry, `author-replay-13686-03`, retained the same aborted request and
also failed actual seventh-context Host startup: the UI reported Native clock
scheduling missed its original deadline before Worker play-start. The actual
click was received and a context created/closed; this is not a disabled-control
race. Both failures remain GATE/OWN pending diagnosis, with no network exclusion,
fake clock or assumed pre-existing classification. Final acceptance is pending.

The passive `author-replay-13686-04` diagnostic passed nine gameplay checks and
retained the initialization ERR_ABORTED. Its bounded observer recorded original
native API/clock messages without replacing calls: reserve wall time around
0.4–0.55ms coincided with native currentTime advancing8.7–11.6ms, leaving a
future-origin margin as small as1.45ms. This explains why wall-time delivery
does not bound native startup progress; it does not identify the exact expired
API in the earlier bare failure. Control-observer overhead is not product
performance evidence.

The native setup-pause feasibility probe used the original Worklet, matching
WASM and native AudioParams at44.1/48kHz. Clean
`author-paused-feasibility-13686-02` exited0: native currentTime stayed frozen
during paused reserve/program/whole commit ACK; actual callback certification
followed resume; a separate suspended cancellation received its real stop ACK.
The first probe's unrelated favicon404 remains recorded. The clean probe owned
and closed its Chromium/server, with PID absent and reusable8171 bind passing.
This is setup/control feasibility evidence, not gameplay or acoustic proof.

The refined Host opening contract preserves its original lead/origin formula
but executes initial reserve/program/commit while natively suspended after the
initial gesture resume and measured quantum. It resumes before actual callback
certification. All new awaits retain the original deadline and cancellation
owner. Rejected/stalled transitions or wrong native state fail closed. The
REQ and canonical PLAN were refined before source edits. Independent author
and root executions of the actual Host suite passed82 tests. The initial
gesture resume's late rejection first reproduced premature cleanup (81 PASS,
1 FAIL); the minimal stopped-owner guard made all82 pass, including abort and
timeout across first resume/suspend/second resume. Full production browser
acceptance remains pending at this checkpoint.

The separate original-native Worker EOF probe
`native-fetch-eof-ac008-13686-01` exited0 collecting all four diagnostic
outcomes. getReader with absent or exact Content-Length received all77456
matching bytes, compiled the actual WASM, and retained signal.aborted=false,
yet Chromium reported ERR_ABORTED. Native arrayBuffer with either framing
reported requestfinished for the same bytes. Owned browser/server closure and
reusable8173 bind passed. This disproves Content-Length alone as the remedy;
it identifies a native consumer/report distinction without weakening clean
network acceptance or replacing the production64MiB streaming limit.
The Harness task is TASK__browser-native-clock-product, run
01a13686-2986-79f1-a971-6cc00056d10a. Update this section with actual ordered
finals and runtime attestation outcome before closure.

## Limits and full player scope

These runs measure Chromium153.0.8010.12,44.1/48kHz and native128-frame
extents. Pure one-frame/variable-extent/20-hour/week vectors are arithmetic or
adapter tests rather than aged native-context measurements. Initial index0 or
one-frame onset refuses because it cannot certify the fresh interior anchor.
The startup lead is a measured delivery policy and supplies no delivery
guarantee. Late scheduling, malformed/held lanes, precision failure, an actual
armed chronology gap or expired refill terminates the owner without rebasing.

Native fills, three full scans, output copying and browser message cost remain.
No measured throughput/acoustic timing, universal zero cost, other-browser,
device/backend/buffer-change, long-aged-context or full game completion claim
is made. The full native/web BMS player Goal and its broader WBS remain open;
this child does not promote WBS acceptance. Deferred scope remains in its task
document and the full project backlog. Source, Host, Worklet and regenerated
bindings deploy or revert together.
