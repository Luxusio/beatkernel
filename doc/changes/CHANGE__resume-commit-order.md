# Publish resumed clocks after accepted device evidence

Common solo/cohort resume previously published the resumed Transport before
presentation reconstruction and device seeding could fail. Stage both owners,
require an accepted latest pair after seeding, and publish clocks and local
resume bookkeeping only after every fallible preparation step succeeds.
Original device errors propagate unchanged. A seed that returns success but
supplies no observation is rejected before publication.

## Evidence

Implementation and four independent fault-injected actual memory-pump tests are
authored, using two failure modes with nonzero and max-u64 epochs in both solo
and cohort paths. The shared adapter forwards ordinary device operations to the
original memory device. Tests preserve typed error Box identity, compare the
old observer after its last genuine observation, derive paused transport history
from the observed boundary, and reconstruct retained replay captures.
Assertions, runtime, formal review and QA remain deferred.
Scoped Rustfmt and whitespace checks completed. All four sequential compile-only
commands exited zero: workspace/all-targets with WebTransport, headless runtime
all-targets with WebTransport, WASM browser lib and WASM browser-audio lib.
Existing unused-code warnings remain. The four tests were compiled, not run;
these configurations do not establish native device or physical timing acceptance.

## Known ceiling

Native audio effects and pause acknowledgement are not rolled back.
NativePause and device playback may already acknowledge resume before software
clock preparation. This does not establish whole-gameplay rollback.
Physical resume effects, device rollback, and backend handoff remain outside
this coverage. Automatic backend/buffer handoff,
physical output fences and platform/device acceptance remain pending. Full BMS
player Goal stays active and incomplete.
