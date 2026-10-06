# Align stepped score, fanout and BGM fixture setup

Archived-score fixtures render 50-frame blocks but allowed only 16. Their test
output helper now permits 64 frames, retaining actual command delivery, render
counters, full/finite completion and exact exported score checks. Assigned local
players receive exact device selectors; solo bindings retain Any-device policy.

The multiweek fixture now supplies base BPM 0.1 directly through the shared
preparation helper instead of appending it after BPM 60. Duplicate base-BPM
rejection remains strict, while the >7-day deadline and absent/repeated output
completion refusal assertions can execute against a valid source.

One physical input fans out to three bound controls although only two have
notes. Solo/local capture expectations retain all three canonical bound inputs
and still compare the two judgment outcomes; a note-free mapped control is not
dropped from original input history. Whole-report capture failure still produces
no saved partial fanout, and already-committed members remain independently
inspectable after a later error.

The local stop-ACK helper verifies construction-time BGM pre-admission: another
feed call admits zero additional cues and total admitted remains one. Actual
queued Play, correlated batch/ACK, failed-member stop and surviving/shared BGM
checks remain unchanged. Production render caps, binding/capture semantics and
BGM lifecycle are not relaxed.

Only test setup/expectations change. Portable actual Mixer/owner evidence does
not establish native latency or hardware execution. The broad task stays open
with formal QA and other functional work pending.

Verification (2026-10-06): initial full runtime library execution reports
1563 passed, 5 failed versus the preceding 1556/12 baseline. Exactly seven
archived-score, multiweek, fanout and local stop-ACK failures resolve. Subsequent
fixture repairs described separately bring the full library to 1568/0. Workspace
all-target webtransport check exits 0 with existing dead-code warnings.
