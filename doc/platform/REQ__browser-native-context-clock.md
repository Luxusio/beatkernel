# Browser native context clock

The browser player must use validated native AudioParam samples to identify
the original output callback frame. Published Worklet `currentFrame`, a host
timer, or a locally advanced callback counter cannot repair audio chronology.
Live play and immutable replay keep the same strict Rust output/runtime logic.
This requirement covers actual Host, Worklet and numeric WASM integration;
diagnostic or pure arithmetic acceptance alone does not complete the player.

## Native program and admission

Use one fresh a-rate marker and three fixed a-rate phase lanes with periods
1,048,576, 1,048,575 and 1,048,573. All default to zero. Marker maximum is1;
each phase maximum is its period+1. The marker's first and only event sets1;
no preceding value write, reset, event or cancellation is allowed. Each phase
starts at1 at the same origin and ramps to period+1 before resetting to1 at
its own integral period boundaries.

The exact source/artifact evidence for fresh first-event admission is pinned
[Chromium AudioParamHandler](https://raw.githubusercontent.com/chromium/chromium/971a7443b0c9b0a9b2860529b33331b76077ec62/third_party/blink/renderer/modules/webaudio/audio_param_handler.cc).
Its late-clamped fresh first event yields index0 rather than a certifying
interior edge. An interior edge after earlier resets does not have this
provenance. Phase residues and control metadata alone do not establish epoch.
Other engines require their own actual source/native verification.

Until onset, every lane may be scalar zero or a full original callback array
of zeros. Waiting outputs silence and establishes no readiness, PCM evidence,
arm or clock progress. Zero-length callbacks also make no progress. The first
certificate requires a full marker with a strict interior zero-to-one edge,
full original phase arrays with the same indices, exact zero prefixes, and
first active phase0 in all three lanes. The origin minus edge gives the
candidate callback base. A second contiguous nonempty callback must validate
before native readiness is published. Constructor readiness acknowledges
allocation only; `AudioHost.open()` also waits for this native readiness.

After certification the marker may be scalar1 or fullN all1. The phase lanes
must always have the full original callback length; scalar expansion, slicing
and reindexing cannot supply evidence. Every active value must match both the
decoded expected phase and represented native ramp ideal with strict error
less than0.25. Validate every original sample before ready messages, arm ACK,
PCM, render or refill request. A scalar candidate may update internal pure
chronology first, but a failed full scan terminates the whole owner and later
callbacks/commits cannot revive it. Inputs remain unchanged.

## Exact pure frame domain and binding

Rust uses fixed checked u64 arithmetic to combine the three residues and
stores an immutable positive origin, own associated horizons and observed
end. Accept only Number-safe absolute coordinates through9,007,199,254,740,991;
reject unsafe narrowing, alias-domain overflow and invalid endpoints. Each
horizon exceeds origin by positive integral own periods; descending horizons
are valid. All active samples must precede the minimum committed horizon.
The first partial endpoint and subsequent full endpoint must exactly match
actual extents. Readiness requires the second contiguous block. Later
backtracks/overlaps refuse; forward gaps remain visible to unchanged strict
armed WorkletAudio and therefore become terminal during playback.

Control-only `clockGeometry(origin,maxFrames,sampleRate)` returns null for
invalid geometry and otherwise returns origin, own horizons, lowWater W and
integer periods K (the reserved cycle count). Fixed lane periods are separately
exported as CLOCK_PERIODS. Capacity is1..1,048,576 and every scheduled boundary
must have a representable event time. The pure Boolean lane validator permits
periods2..2^20 for independent small-cycle tests; the actual protocol always
associates the three fixed CLOCK_PERIODS with their own lanes.

Use scalar numeric WASM exports for native start, whole horizon extension,
first/last phase decoding, readiness and low/high-word arm. No callback array
import, allocated BigInt result, object result or scratch3N copy is required.
Return a Number-safe exact f64 frame only after integer reconstruction; return
-1 plus existing status on refusal. Existing arm/render and29-field reports
remain intact, including original context/start association. Fresh bindings
missing the required numeric exports fail open without an old-clock fallback.

## Startup scheduling and control ownership

On the existing owned MessagePort add a bounded clock namespace with exact
generation, clockVersion1 and clockId1. Valid different generations are
ignored first; malformed current identities, revisions or geometry fence the
owner. General sequence and transferred sample/command authority remain
independent. One clock pending request/timer is separate from the public
operation pending slot. Stop/abort cancels both and any pending arm; late ACK
cannot activate a stopped or replacement generation.

Measure deliveryMs from node acquisition until receipt of constructor ready
and the first nonempty actual quantum probe Q. With actual sample rate F,
choose lead=max(4Q,ceil(2*deliveryMs*F/1000)), then
origin=ceil((ceil(context.currentTime*F)+lead)/Q)*Q+floor(Q/2).
Require safe coordinates and representable quarter-early event times
(targetFrame-0.25)/F. Initial Q=1 cannot produce this certificate and refuses.
Native currentTime can advance several quanta during a short wall-time
delivery interval. After initial gesture resume and receipt of actual Q,
suspend the owned opening context and await its exact suspended state before
selecting origin. Use the same lead/origin formula against frozen native
currentTime. Reserve, every native scheduling call and whole initial commit
ACK run while the context is suspended. Then resume and await exact running
state before native callback certification. No event is rewritten or origin
rebased, and no fixed delay, retry or clock correction is introduced.

The existing single opening deadline covers both native transitions and the
complete handshake. After every await check current ownership, failure and
stop before further work; late suspend/resume/ACK completion cannot resume or
activate a canceled generation. Rejected/stalled transitions, wrong native
state or missing paused ACK fail closed through actual stop/context cleanup.
Opening suspension is internal setup only; active setup/allocated/armed
interruption handling remains unchanged. Native fills and first certificates
still require actual callbacks after resume; suspended control ACKs alone
never establish readiness. The policy gives no universal startup/quantum
guarantee and imposes no diagnostic200ms delay. Device suspend/resume cost and
other engine behavior require measured verification.
Reserve exact initial geometry before native API writes. After reserve ACK
and before/after all scheduling calls require origin still future. Whole
commit follows successful scheduling of marker and all three lanes; partial
throw or lateness terminates the generation without rebasing/reusing origin.

The messages are clock-probe(frames), clock-reserve(revision0,origin,h0,h1,h2),
clock-reserved(revision,status,error), clock-commit(revision,origin,h0,h1,h2,
futureEligible:true), clock-committed(revision,status,error), clock-ready
(revision0,origin), and clock-refill(revision,from0..2,to0..2,frames,endLow,
endHigh). Exact current revision/geometry and whole commit are required;
metadata ACK is never native readiness.

## Pending arm and rolling horizons

Arm control validates the target and stages numeric words plus a prebuilt ACK.
It succeeds only on the next nonempty fully validated native callback: call
unchanged core arm with that actual base, publish successful ACK, then render
the same callback. Zero, default wait and initial partial blocks cannot commit
arm. Success ACK has no diagnostics; failure retains exact existing error and
admitted prefix. Duplicate, stale target, stop, timeout and terminal failure
must preserve public cancellation and cleanup ordering.

For maximum callback capacity M, W=max(P_min,4M) and
K=ceil((W+2M)/P_min)+1. Initial own horizons are origin+K*P_i; K3 at4096
is future program length, not a startup wait. At actual validated native end
within W of the common horizon, reserve one next revision and request all
own horizons advance by one own period. No repeated request is allowed while
pending. Host verifies exact old/new geometry and schedules only each new
endpoint ramp followed by its reset. Do not duplicate a reset at OLD H_i,
cancel automation, rewrite played prefix or commit one lane independently.
Every native call and the completed transaction must remain before all OLD
H_i event times. Rust extension is whole and atomic; expiry or failed owner
cannot be revived by a late commit. Native try-lock fallback, held/malformed
values, precision failure and expired horizon must silence/fence before PCM.

## Cost, lifecycle and verification

The callback uses setup-owned output memory/views, fixed state, original arrays,
scalar bindings and preallocated control replies. It creates no arrays/views,
BigInts, formatted strings or callback objects and never frees Rust in the
callback. Preserve pre/post-render memory identity checks. Native fills,
three complete scans, planar PCM copying and browser message costs remain;
measure them rather than claiming universal zero cost.

Required evidence includes independent pure integer vectors and core
composition, actual fresh WASM scalar tests/glue inspection, actual Host and
Worklet negative/lifecycle tests, three strict fresh cold contexts, real
44.1/48k product playback through two committed refills and past both sets of
old own horizons, and actual live/local/nonzero-section/seek/restart plus
genuine capture-to-replay natural completion. Preserve existing invalid0/1
cold input refusal. Interactive probes inspect original inputs, PCM, reports,
records and rendering, with current Host/helper/Worklet/WASM fingerprints.

Long native mode WORKLET_NATIVE_CLOCK_PRODUCT=1 is exclusive with diagnostics
and REQUIRE_COLD. It alone uses180s total per-case budget,210s CDP deadline,
65,536 callback cap and32 retained receipts. Existing single/joint phase
diagnostics retain their120s/32,768/150s bounds and3s owned page-close. Marker
mode retains1,024 callbacks,30s CDP deadline and ordinary page.close(). All probes own their
resources and prove PID/listener/bind cleanup; scheduling two refills without
traversing their old native horizons is insufficient.

Known ceiling: Initial one-frame/index-zero onset is not authenticated; bounded
startup refuses it. Authenticated one-frame callbacks remain required.
Known ceiling: Pure20-hour/week coordinates do not establish an aged native
context. Physical acoustic/device timing, other browser engines and full
player performance acceptance remain full Goal verification work.

## Deployment

Private clock protocol, numeric WASM ABI, Host, Worklet and matching generated
bindings must be replaced together. Ignored generated outputs must be rebuilt
from the restored source after coherent revert. No user chart/replay/profile
schema migration is introduced. A temporary raw-counter fallback cannot mask
an invalid native clock. Implementation and measured acceptance are recorded
separately; this requirement is not a claim that they have already passed.
