# Browser native frame diagnostic

The historical opt-in experiments below compare native a-rate parameter edges with actual callback extents in setup phase0. Those experiments did not establish a product clock or resolve the earlier strict chronology failure. The separate production integration and its current verification procedure are described under **Production native context clock** below.

## Run

Use the existing generated audio WASM and installed Chromium/Puppeteer. No dependency installation or Rust rebuild is required for this fixture.

```bash
PUPPETEER_MODULE="$PWD/target/wf/record-score-ui-qa-browser/node_modules/puppeteer-core" \
CHROMIUM=/usr/bin/chromium WORKLET_NATIVE_FRAME_PROBE=1 \
WORKLET_CHRONOLOGY_OUT="$PWD/target/wf/browser-native-frame-diagnostic/local" \
WORKLET_CHRONOLOGY_PORT=8161 \
node app/web/worklet-chronology.browser.mjs

node --experimental-vm-modules --test app/web/worklet-native-frame-probe.test.mjs \
  app/web/audio-host.test.mjs app/web/audio-worklet.test.mjs
```

Choose an unused owned port and a unique output directory. The runner owns its HTTPS server, browser, pages, contexts and private probe channels. Evidence records source/WASM SHA, Chromium version, applied sample rate, actual callback extents, marker targets/edges, refusals and cleanup. Default and `WORKLET_CHRONOLOGY_REQUIRE_COLD=1` chronology acceptance remain separate.

## What constitutes an initial anchor

Start a fresh context/parameter with native default0 and exactly one first `setValueAtTime(1,time)` event. No previous reset, parameter-value assignment, generation or cursor is admissible. For intended onset T and native rate F, `markerTime` uses `(T-0.25)/F` only when the represented double, multiplied by F, remains strictly inside (T-1,T) and rounds upward to T.

A full native Float32Array must contain an exact zero prefix and one suffix with one interior edge k. For a known actual onset T the callback base is T-k. Constant arrays, index0, nonbinary values, mismatched lengths and unsafe arithmetic refuse. This spatial calculation cannot authenticate an arbitrary scheduled onset: the fresh first-event prerequisite belongs to the caller.

The second independent marker must agree with the first base plus every actual observed callback extent. A contradiction fails and preserves both raw candidates; the runner never rebases. Raw global `currentFrame` is a separate observation, never the oracle. Two successful observations establish only their measured interval.

## Why the scheduling prerequisite matters

Native late automation is clamped. With a preceding reset, floating multiplication can turn the clamped quantum boundary into the following sample:

- `(1664/44100)*44100 = 1664.0000000000002`, whose ceiling is1665.
- `(896/48000)*48000 = 896.0000000000001`, whose ceiling is897.

An interior edge at index1 can therefore appear even when the original scheduled onset was missed. The pure oracle deliberately cannot detect that prerequisite violation. The initial native anchor requires a fresh single first event; previous-reset cases are refused separately.

## Native evidence and remaining limits

The pinned Chromium sources are commit971a7443b0c9b0a9b2860529b33331b76077ec62, corresponding to the tested Chrome153.0.8010.12 artifact:

- `BaseAudioContext::UpdateWorkletGlobalScopeOnRenderingThread` publishes the Worklet frame only after a successful graph try-lock. A failed lock leaves the published global frame unchanged. [Primary source](https://raw.githubusercontent.com/chromium/chromium/971a7443b0c9b0a9b2860529b33331b76077ec62/third_party/blink/renderer/modules/webaudio/base_audio_context.cc).
- `RealtimeAudioDestinationHandler` can skip graph pulling while still advancing the destination frame. Thus a local callback counter does not prove uninterrupted native chronology across all lifecycle states. [Primary source](https://raw.githubusercontent.com/chromium/chromium/971a7443b0c9b0a9b2860529b33331b76077ec62/third_party/blink/renderer/modules/webaudio/realtime_audio_destination_handler.cc).
- `AudioParamHandler::CalculateTimelineValues` uses the destination sample frame; first-event handling, late-event clamping and sample rounding explain both the independent marker and its admission constraint. Native parameter generation remains ongoing work when attached; it is not a zero-cost product fix. [Primary source](https://raw.githubusercontent.com/chromium/chromium/971a7443b0c9b0a9b2860529b33331b76077ec62/third_party/blink/renderer/modules/webaudio/audio_param_handler.cc).
- The Web Audio rendering-loop contract is distinct from implementation evidence and browser-specific measurements. [Web Audio specification](https://www.w3.org/TR/webaudio/#rendering-loop).

The earlier actual failure recorded PROCESS status6, owner phase2, current161408, expected161536 and block128 without injected omissions. The native try-lock branch is a plausible mechanism, not an instrumented proof of the exact lock collision in that run. Neither a delayed DOM suspend event nor a first raw frame proves an exact callback origin.

A source-possible lifecycle counterexample remains: when embedding policy restricts media-playback-while-not-visible, native interruption and automatic restart can complete before queued statechange notifications run. Those notifications carry no prior-state snapshot; the current Host handler reads only the current state and can see running on both. This is conditional source evidence, not a reproduced failure or a claim about every top-level page hide. [Pinned AudioContext source](https://raw.githubusercontent.com/chromium/chromium/971a7443b0c9b0a9b2860529b33331b76077ec62/third_party/blink/renderer/modules/webaudio/audio_context.cc), [current Host handler](../../app/web/audio-host.mjs).

Real observations apply only to measured rates, extents, contexts and generated WASM bytes. They do not prove cross-browser chronology, future callback continuity, presentation/acoustic timing, successful gameplay, replay/input integrity or high-load performance. Production chronology checks, report coordinates and ownership remain unchanged.

See [recorded change evidence](../changes/2026-10-13-browser-native-frame-diagnostic.md) for actual results and independent review/QA status.

## Native modulo-phase experiment

The separate `WORKLET_NATIVE_PHASE_PROBE=1` mode measures finite native ramp
samples using the same owned runner and production phase0. It rejects conflicts
with the marker probe or strict cold mode. Run it with a new owned output directory:

```bash
PUPPETEER_MODULE="$PWD/target/wf/record-score-ui-qa-browser/node_modules/puppeteer-core" \
CHROMIUM=/usr/bin/chromium WORKLET_NATIVE_PHASE_PROBE=1 \
WORKLET_CHRONOLOGY_OUT="$PWD/target/wf/browser-native-phase-decoding/local" \
WORKLET_CHRONOLOGY_PORT=8161 \
node app/web/worklet-chronology.browser.mjs
```

A fresh first binary marker anchors the Worklet-side cursor. Page message
delivery time never supplies or rebases that origin. The phase parameter starts
at default 0, then has a first set to 1 and linear ramps to P+1, with
P=2^20. At each interior reset the prior ramp endpoint is registered before
the set back to 1 and the next ramp.

Quarter-early event times do not produce exact integer ramp samples. For a
cycle beginning at A and ending at A+P, let t0=(A-0.25)/F and
t1=(A+P-0.25)/F. The double ideal is
`1 + P * (frame/F - t0) / (t1-t0)`; its first sample is approximately 1.25.
The pinned native implementation uses Float32 intermediates, so every measured
sample must decode to the expected modulo phase and differ from its ideal by
strictly less than 0.25. Exact halfway values, invalid shapes, unsafe event
boundaries and contradictory samples refuse.
[Native ramp source](https://raw.githubusercontent.com/chromium/chromium/971a7443b0c9b0a9b2860529b33331b76077ec62/third_party/blink/renderer/modules/webaudio/audio_param_handler.cc).

Three periods contain 3,145,728 samples, requiring about 71.33 seconds at
44.1 kHz or 65.54 seconds at 48 kHz. Only the phase session uses a fixed
32,768-callback cap, a 120-second case watchdog and a 150-second CDP timeout.
The controller starts the case deadline before page acquisition and shares it
across acquisition, navigation and evaluation. Owned page closure has a separate
3-second budget, including pages arriving after acquisition timeout.
The marker probe retains its original bounds. Validation streams inside the
Worklet and saves bounded aggregates and at most 32 raw opening/reset/horizon/failure
snapshots; it does not send or retain every callback.

Active boundary slices are views into the original arrays. Their lengths
determine checked sample counts, while the original output extent alone
advances the cursor. A closing binary reset/set sequence is endpoint
corroboration rather than fresh-origin authentication: both events must remain
future at submission, native zero reset must be observed, and the endpoint must
agree with all observed extents without rebasing. The final same-time reset
makes the finite horizon hold 1; those samples are refused as chronology evidence.

Native held-value arrays after the horizon may have scalar length 1 even when
the original output extent is 128. Preserve that raw shape and refuse it as
active chronology; do not require full ramp-array shape for a held-value check.
The completion summary follows closing-marker corroboration but precedes that
callback's final raw record. Final retained snapshots can therefore exceed the
summary-time count; check each against its own observation stage and the fixed
cap rather than assuming the counts match.

One phase cannot distinguish a missing interval of nP frames, even with a full
sample scan. A finite closing endpoint can expose a contradiction in the
measured interval, but cannot establish every future callback. Arithmetic
vectors at twenty hours or one week are not aged native-context measurements.
These historical diagnostic results did not adopt a product clock, fix the strict playback
failure, or prove performance, physical output, gameplay or a fresh Rust build.

See [finite phase evidence](../changes/2026-10-14-browser-native-phase-decoding.md)
for measured results and independent verification status.

## Joint native phase experiment

The separate `WORKLET_NATIVE_JOINT_PHASE_PROBE=1` mode measures three native
a-rate ramp lanes together, with fixed periods 1,048,576, 1,048,575 and
1,048,573. It rejects marker, single-phase and required-cold mode conflicts
before acquiring resources. Use a new owned directory and unused port:

```bash
PUPPETEER_MODULE="$PWD/target/wf/record-score-ui-qa-browser/node_modules/puppeteer-core" \
CHROMIUM=/usr/bin/chromium WORKLET_NATIVE_JOINT_PHASE_PROBE=1 \
WORKLET_CHRONOLOGY_OUT="$PWD/target/wf/browser-native-joint-phase-proof/local" \
WORKLET_CHRONOLOGY_PORT=8163 \
node app/web/worklet-chronology.browser.mjs

node --experimental-vm-modules --test app/web/worklet-native-frame-probe.test.mjs \
  app/web/worklet-chronology.test.mjs app/web/audio-host.test.mjs \
  app/web/audio-worklet.test.mjs
```

The fixed periods are pairwise coprime. Their product is
`1152917106563481600`, greater than `Number.MAX_SAFE_INTEGER`.
`nativeJointOffset(v0,v1,v2)` decodes three scalars and performs exact BigInt
reconstruction before narrowing to a safe Number; invalid scalars and unsafe
reconstructions return null. A displacement of P0 can
alias the first lane while the other lanes distinguish it; a displacement of
the complete modulus still aliases all three. Thus the result is a modulo
candidate in a separately certified domain, without epoch or age authority.
This diagnostic BigInt calculation, three native parameter fills and sample
scanning are ongoing test overhead, not a production allocation or speed claim.

One fresh binary marker establishes the immutable origin inside the Worklet.
The three lanes share a start but each retains its own integral three-period
horizon. Their own checked sample counts are 3,145,728 / 3,145,725 / 3,145,719;
the common interval contains 3,145,719 samples. The first two lanes have nine
and six remaining samples after common eligibility ends. Every active native
sample must meet the existing strict quarter-sample error margin. CRT reads
the same original first and last eligible array indices in all lanes, and the
cursor advances once by the original callback extent.

At the measured 128-frame extent and start offset 17, the first reset indices
are 17/16/14, the second 17/15/11, and own horizon indices 17/14/8. All lanes in
one reset or horizon callback share one retained record. The closing marker
follows the latest horizon; its future-submission, observed-zero reset and
endpoint agreement prerequisites remain required. Held value 1 and expired
horizons cannot supply chronology.

The unscheduled-third-lane case admits fixed geometry then rejects actual
active evidence. The late-third-lane case retains a fresh origin, future first
two lanes and an existing watch near late submission; it refuses joint-start
and grants no joint session. Both preserve native arrays before retirement.
Identity/range refusal, active timeout/stop/suspend and the actual callback cap
remain bounded. Joint mode reuses the shared 120s controller, 150s CDP,
32,768-callback and 128-reply bounds, independent 3s owned-page close and
32-record cap. Positive author and fresh independent QA joint runs summarized
13 records before the final raw callback, retaining 14. Each counter belongs
to its recorded stage. Single-phase regressions can retain additional warmup
records: fresh QA observed summary/final counts 27/28 and 31/32, both within
the unchanged cap. Use the actual mode, run and observation stage for counts.

See [joint phase evidence](../changes/2026-10-14-browser-native-joint-phase-proof.md)
for the historical author and independent review/QA results. Product chronology,
rolling horizons and gameplay were pending at that experiment's completion.
The full player Goal remains open; current integration evidence follows below.

## Production native context clock

The [product requirement](REQ__browser-native-context-clock.md) specifies the
fresh marker, three original native phase arrays, scalar Rust reconstruction,
second-callback readiness, callback-time arm and atomic rolling horizons.
The Host schedules native automation; the Worklet validates every original
sample before publishing readiness, arm ACK or PCM. Published `currentFrame`
and callback counters remain observations rather than clock authority. Live
and replay use the existing strict Rust renderer and unchanged report29.

Opening first resumes through the user gesture and observes the actual native
quantum. It then awaits native suspension, selects the original origin against
frozen currentTime, and reserves/programs/commits the complete clock while
suspended. Only after whole commit ACK does it resume and await two actual
callback certificates. All transitions share the existing opening deadline
and check current ownership after every await. This avoids treating elapsed
wall-time delivery as a bound on native startup progress; it adds no fixed
delay, retry, event rewrite or origin correction. Suspended ACKs do not certify
readiness. Other engines and device suspend/resume costs still require actual
measurement, and active-play interruptions remain terminal.

Generate matching audio and UI artifacts from the current source before running
the product probes. With this checkout's cached toolchain, native dependencies
also require the cached CC/AR wrappers; sourcing env.sh alone is insufficient.
Do not run mise or manually edit generated glue.

```bash
source target/toolchain/env.sh
export CC="$PWD/target/toolchain/host-cc.sh"
export AR="$PWD/target/toolchain/host/usr/bin/x86_64-linux-gnu-ar"
cargo test -p beatkernel-bms-runtime --no-default-features --lib browser_native_clock --locked
cargo build -p beatkernel-bms-runtime --lib --no-default-features --features browser-audio --target wasm32-unknown-unknown --release --locked
env LD_LIBRARY_PATH="$PWD/target/toolchain/host/usr/lib/x86_64-linux-gnu" target/toolchain/wasm-bindgen-cli/bin/wasm-bindgen --target web --out-dir app/web/audio-pkg target/wasm32-unknown-unknown/release/beatkernel_bms_runtime.wasm
cargo build -p beatkernel-bms-runtime --lib --no-default-features --features browser --target wasm32-unknown-unknown --release --locked
env LD_LIBRARY_PATH="$PWD/target/toolchain/host/usr/lib/x86_64-linux-gnu" target/toolchain/wasm-bindgen-cli/bin/wasm-bindgen --target web --out-dir app/web/pkg target/wasm32-unknown-unknown/release/beatkernel_bms_runtime.wasm
```

Generate audio-pkg before building the UI variant: both variants use the same
target cdylib path. Existing outputs can belong to another checkout owner; in
that case use the existing authorized account for the generator, without
changing ownership or permissions across the repository.

Run the strict cold and long product modes separately with new evidence
directories and available owned ports. These example suffixes are placeholders
for a new run; do not overwrite an earlier proof.

```bash
export PUPPETEER_MODULE="$PWD/target/wf/record-score-ui-qa-browser/node_modules/puppeteer-core"
export CHROMIUM=/usr/bin/chromium
WORKLET_CHRONOLOGY_REQUIRE_COLD=1 WORKLET_CHRONOLOGY_PORT=8165 WORKLET_CHRONOLOGY_OUT="$PWD/target/wf/browser-native-clock-product/cold-new-run" node app/web/worklet-chronology.browser.mjs
WORKLET_NATIVE_CLOCK_PRODUCT=1 WORKLET_CHRONOLOGY_PORT=8165 WORKLET_CHRONOLOGY_OUT="$PWD/target/wf/browser-native-clock-product/long-new-run" node app/web/worklet-chronology.browser.mjs
LIVE_SPEED_PORT=8167 LIVE_SPEED_OUT="$PWD/target/wf/browser-native-clock-product/live-new-run" node app/web/live-speed.browser.mjs
REPLAY_SPEED_PORT=8168 REPLAY_SPEED_OUT="$PWD/target/wf/browser-native-clock-product/replay-new-run" node app/web/replay-speed.browser.mjs
node --experimental-vm-modules --test app/web/native-audio-clock.test.mjs app/web/browser-native-audio.test.mjs app/web/audio-host.test.mjs app/web/audio-worklet.test.mjs app/web/worklet-chronology.test.mjs app/web/worklet-native-frame-probe.test.mjs
```

The long mode requires actual armed PCM and native callbacks crossing every
OLD own horizon for two accepted whole refill transactions. It alone uses a
180-second total case deadline, 210-second CDP timeout, 65,536 callbacks and
32 retained receipts; older diagnostic bounds remain unchanged. Bootstrap
observers use distinct import names because their bytes and the complete
production module are served as one module. Controller tests compile that
actual served combination for cold/product/marker/single/joint modes.

The live probe preserves its 14 existing checks and adds nonzero-section and
seek-restart. The replay probe preserves eight checks and adds nonzero-section
and section-restart, uploading the genuine captured record unchanged. Each
checks real PCM/reports, natural end, rendering, context replacement and input
or replay provenance. Await final process exit, inspect screenshots and verify
owned PID/listener cleanup; a closed listener can still leave TCP TIME_WAIT,
so a reusable-bind check must use SO_REUSEADDR like Node's server.

Native parameter fills and three full scans remain sustained callback work.
No universal zero-cost, acoustic timing, other-engine, aged-context or full
player completion claim follows from these probes. See [current product
evidence](../changes/2026-10-14-browser-native-clock-product.md) for author and
independent verification status, exact generated fingerprints and remaining
limits.
