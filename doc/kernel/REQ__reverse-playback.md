# Reverse replay keysound output

`runtime::playback::ReversePlayback` reconstructs the existing `ReplaySession`
through its forward JudgeEngine transitions. Audio policy never modifies judge
results, snapshots, recording operations or logical state hashes. A bounded
catalog contains only results emitted by the durable recording, obtained by
reconstructing its complete operation prefix. Synthetic seek-boundary misses
remain logical inspection results and do not become historical keysounds.

`seek` is silent inspection. `step_back(target, output_start)` reconstructs at
the target and traverses historical results with `target < at <= previous`,
in reverse original emission order, including reversed order for tied times.
It reports misses but plays only hit stages with explicit `SoundBinding`s.
No backward inputs are sent to a judge. A zero-distance step crosses nothing.
The recording is immutable through this owner; inspection exposes only shared
references. The catalog must fit a caller-supplied result limit in
1..=1,048,576; bindings are capped at 65,536 and gains must be finite. Each step
is capped at 1,048,576 command attempts, checked before reconstruction.

The explicit output clock domain stays fixed and output timestamps increase
while song timestamps decrease. Historical distance is divided by a nonzero
rational rate magnitude with checked integer arithmetic, rounding upward to
nanoseconds. Output start must not precede the last traversal end or the
host-supplied output floor. No host clock or hardware presentation is inferred.
Negative
rates are accepted as magnitude input through checked absolute conversion;
`i64::MIN` is an explicit overflow, and zero is invalid.

Policies are `ReverseTimelineOnly` (normal sample heads at positive magnitude),
`ReverseSamples` (last sample frame and negative rate), and `Mute` (no Play).
A dedicated owned Mixer, bank and queue isolate these globally applied rates
from unrelated music or voices. Construction exposes its admitted SetRate.
A policy transition requires a fresh caller-provided keysound bank/config,
creates a new queue/Mixer before replacing the producer and returns the fresh
Mixer for offline or native ownership. The host must stop/reset/drop the old
Mixer before installing the replacement; disconnect alone leaves old pending
commands intact. No old commands reach the replacement queue. The new origin
cannot regress the output timeline. This is off-thread setup: it allocates and drops assets. Silent seek
by itself does not flush output; call transition even with the same policy to
flush before changing the audible traversal. Native stop/reset/start and
already buffered hardware audio remain host responsibilities.

`observe_output_floor` accepts explicit submitted/rendered output telemetry;
it never invents physical presentation. Call it before scheduling against a
running Mixer to avoid placing commands behind its frame cursor.

Step reports exact admitted commands and exact queue failures; failures never
retry automatically or roll back reconstruction. Setup admission failures are
explicit errors. Admission is not execution: the separately returned Mixer
reports counters for late commands, capacity or rate/sample execution rejection.
The control owner cannot accept external commands; bindings must name assets
present in every new bank. Mixer rate arithmetic and voice/pending limits still apply.
A host must render often enough to drain the bounded queue; no promise of
physical playback or first-presentation synchronization is made.

Meaningful fixtures cover literal normal/reversed/muted PCM, stale pending
command discard, output chronology, queue failure isolation and reusable
logical restoration. Fixtures are authored but execution and formal QA are
deferred by the user; compile checks do not establish playback correctness.
