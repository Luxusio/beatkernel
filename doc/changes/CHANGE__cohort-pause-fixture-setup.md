# Restore cohort pause setup and original resume observation expectations

The shared cohort fixture allocated command_queue(capacity) but configured its
mixer for a fixed queue capacity of eight. Use the actual requested capacity,
so one-slot partial-admission and sixteen-slot interval cases reach their intended
assertions. Kernel capacity matching remains strict.

Fixtures attached to the real Player channel now register their compiled chart
and original state-owned roster before running the cohort. This follows the
production publication prerequisite and retains original IDs rather than adding
unregistered-player exceptions. Terminal/unattached cases remain unchanged.
Registration lives solely in the test Fixture helper and uses the actual Player
publication path; shared run/controller code is unchanged.

Three solo/local presentation-port tests confused the resume boundary at 40 ms
with the latest original accepted observation at 50 ms. The memory device renders
and observes before resume admission, so reseeding uses the actual step-five pair.
Expect that pair, keeping the independent 40 ms release/boundary and original
input/capture provenance assertions, nondefault discipline settings and epoch
checks. Do not replace original observations with inferred boundary pairs.

Only fixture setup and expectations are changed. Portable PCM/clock/input tests
do not establish physical native-device latency or network multiplayer behavior.
The broad task remains open with required independent review and QA pending.

Verification (2026-10-06): full runtime library with webtransport reports
1510 passed, 58 failed against the preceding 1503/65 baseline. Exactly seven
existing failures resolve: four cohort acquisition/partial-audio/interval cases
and three original-observation solo/local resume cases. There are no new failing
names. All changed test paths compiled and executed in this run. The suite still
exits 101 for remaining failures; the task is not marked PASS or closed.
