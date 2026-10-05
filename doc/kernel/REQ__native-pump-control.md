# Native pump control boundary

## Contract

Shared solo and local-cohort pumps must not directly read `Instant::now()` or
sleep. Preserve existing public pump entry points as native wrappers and add
explicit generic control entry points for deterministic integration testing.
The control port supplies a monotonic, ordered instant, checked deadline
addition and fallible waiting. The system adapter alone owns real `Instant`
reads and thread sleeps. It preserves the native one-millisecond wait cadence
and existing checked diagnostic deadline behavior.

Control time only bounds an optional diagnostic `seconds` session. It never
replaces input timestamps, audio presentation observations, transport time or
replay operation timestamps. No diagnostic limit means no control-clock reads
in the pump. Expiration uses an exclusive deadline; clock faults, regressing
readings, deadline overflow and wait failures propagate as technical errors.
Cancellation, device closure and expiration keep their existing return behavior;
none becomes proof of song completion. Existing output drain, pause, ordering,
stop admission, gauge and capture rules remain intact.

Use static dispatch without allocating a scheduler, adding a thread, changing
device trait implementations or adding a crate. Tests inject virtual time and
wait failures through the same generic loop used by native wrappers.

## Deferred acceptance cases

Pure control cases: zero and exact deadlines, before/after boundary, checked
overflow, clock regression, clock errors, wait errors and no-clock unlimited
operation. Solo and cohort integration cases: deterministic device/Mixer
evidence, repeated runs, cancellation/closure/timeout distinct from completion,
and failures without synthesized evidence or judge/capture mutation.

This increment isolates two effects. It does not establish complete business
independence from platform presentation or player publication/control state.
