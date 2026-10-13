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
| Host |67 PASS:40 existing and27 new clock/lifecycle cases|
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

At this implementation checkpoint independent DEEP code review, fresh CLI and
browser QA, final documentation review, and task verification have not yet
completed. Author results above are not an authoritative review/QA PASS.
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
