# First-failure AudioWorklet grid evidence

Status: independently reviewed; final CLI and browser QA passed on `f22b55e`.

Final QA initially passed all 646 Node tests, 14 native WorkletAudio tests and
the four required browser diagnostic scenarios. The fresh `browser-audio`-only
WASM build exposed a preexisting graphics-gated UI dependency in touch routing.
The existing four-player page capacity now belongs to shared `playfield_layout`,
with a compatible UI reexport. Input routing uses the shared constant without
requiring graphics. Page behavior and native chronology are unchanged; fresh
review and QA of this correction passed.

Final verification: 646 Node tests, 14 native WorkletAudio tests, seven browser
input and ten local-input tests passed. The separate browser-audio WASM built
successfully; scratch generated bindings instantiated the actual BrowserAudio
and verified absent/high-frame words plus strict armed gap/repeat/backward
failure retention. Actual Chromium diagnostic checks passed 4/4. Evidence is
under `target/wf/worklet-chronology-qa-{cli,browser}-2/`. The successful cold
observation in this run does not establish reliability or repair earlier gaps.

Audio failures now preserve bounded numeric first-cause facts instead of only
a status code. The preallocated Worklet payload records operation origin,
processor phase, actual callback frame and extent when known, retained native
expected/start words, and the actual successful arm frame. Presence flags
distinguish unknown values from valid zero. Successful callbacks add no
diagnostic polling, object/view allocation or BigInt conversion.

Host, command and sample owners validate the snapshot and retain it on their
original errors. Rejected ACKs preserve their existing order, sequence and
admitted prefix while carrying the same first snapshot. Healthy cleanup keeps
its existing success behavior; distinct cleanup failures remain distinct from
the original processor error. Legacy status-only messages stay compatible.

Focused actual-source Node fixtures pass all 114 tests, including high words,
absence, hostile getters, reentrancy, first-cause and cleanup behavior. These
checks do not establish a timing repair. The implemented
[owned-browser probe](../../app/web/worklet-chronology.browser.mjs)
has exercised the unchanged native frame chronology validator through a
declared test-only skipped callback and verified actual diagnostic delivery.
Independent final browser diagnostic QA passed.

The original sporadic status6, pre-play domain choice and delayed-input frontier
issues remain unresolved and unwaived. Diagnostic fields never authorize
presentation, judging, output advancement or replay completion. No timestamp,
frontier, buffer/backend, native chronology or broader player policy changes.

## Observed natural first cause

The actual cold Chromium context reproduced status6 before arming, without the
test-only omitted callback. A successful phase1 callback covered frame0..128;
the next actual callback began at1664 with128 frames. Diagnostics preserved
expected128, current1664, phase1, and absent start/successful-arm facts on both
Host and command errors. This identifies an unarmed context-grid gap; it does
not establish its browser scheduling cause or authorize chronology forgiveness.
The normal lane also passed in another actual context. Deliberate gap/late-arm
fixtures may warm phase0 before finish to reach their targeted failure paths,
but that test precondition is not a production startup workaround.

Actual warmed diagnostic fixtures reached all intended native branches: an
explicitly omitted callback at10240 produced actual current10368, expected10240
and128-frame extent, with start57728 and arm9728 retained identically by Host
and command errors. Late-arm and sample rejection preserved ACK sequence,
admitted prefix and absence facts. The normal cold path again failed without
injection. Diagnostic acceptance records that failure accurately; healthy
cold-start reliability remains unresolved for the broader player Goal.

## Known ceiling

Known ceiling: actual cold AudioWorklet setup can skip from frame0 to1664 before
arming, violating the current expected128 chronology. This task does not repair
that policy and does not establish gameplay-worker/replay-prefix propagation.
