# Deterministic Runtime fault schedules

Exercise failures through the real portable Runtime, ClockMapper and audio
command queue. Keep immutable chart times and original input metadata. Drive
bounded explicit scripts with controlled consumption; avoid wall-clock sleeps,
native devices and random dependencies. Test repetition from fresh owners,
then compare with literal expected events, commands and rejection counts.
Identical repeated hashes alone cannot establish correct behavior.

## Connected verification contract

The integration target `runtime_fault_schedule` must establish these seams:

- Mapping refusal preserves the accepted judge/input prefix; restoring the
  mapping allows retrying the original event and sequence.
- Queue saturation reports the exact rejected sound command while accepted
  judging remains committed. Controlled consumption frees capacity for a later
  sound without overwriting prior queued commands.
- Distinct bounded consumption/mapping scripts repeat from fresh owners and
  have independently pinned expected hit/rejection/command facts. Include
  capacity one and a larger queue.

Run after loading the repository compiler environment:

```sh
cargo test -p beatkernel --test runtime_fault_schedule --locked
```

Keep setup, assertions and diagnostics outside any measured callback boundary.
These schedules verify portable logical state and command admission. They do
not prove native output timing, input-device behavior, network/UI fault
handling, real-time allocation properties or hardware performance. WBS13.04
retains those remaining layers; adding this target alone does not complete it.

## Current checkpoint

On 2026-10-10 focused author verification passed three tests, zero failures:

- `mapping_outage_long_stall_and_recovery_repeat_with_literal_prefixes`: input
  mapping refusal before the first accepted event, five hits without consumer
  progress, then drain and a sixth hit. Capacity one rejects voices 2–5;
  capacity three rejects voices 4–5. Both admit voice 6 after the drain.
- `audio_mapping_outage_and_split_stalls_repeat_with_distinct_expected_admission`:
  drain after hit 1, audio-domain mapping refusal for event 2, retry that same
  event, then two further consumption groups. Capacity one rejects voices
  3/5/6; capacity three admits all six.

Each script/capacity case runs four times from fresh owners, for sixteen
fixtures. Every accepted judge event pins its original object, song time,
stage, grade, delta, device, acquisition sequence and source clock. Every
refusal pins its clock domains or failed audio command. The final FIFO command
list and semantic counters have literal expectations independent of repeated
hash equality. Mapping refusal leaves judge hash/time and transport anchors
unchanged while incrementing only the diagnostic rejection counter.
Runtime processing profiling is disabled to remove native timing observations.

The third regression,
`mapping_refusal_preserves_sequence_and_time_watermarks_for_intermediate_input`,
adds two fresh fixtures for input-domain and audio-domain mapping refusal.
After accepting event 1 at song time 10 with sequence 101, it refuses event 2
at time 20/sequence 102, then accepts an unbound intermediate event at
time 15/sequence 101 before retrying original event 2. This catches premature
private sequence/chronology commits that an equal-time/equal-sequence retry
cannot detect. There are eighteen fresh fixtures in the complete target.

Author log: `target/wf/runtime-fault-schedule-author.log` (ignored, machine-local).
Final corrected author log: `target/wf/runtime-fault-schedule-author-prefix.log`.
Independent full source review and subsequent CLI QA passed on source
`cf3f110`. QA reran the three tests (zero failures), checked scoped Rust 2021
formatting and passed target-specific Clippy with warnings denied:

```sh
rustfmt --edition 2021 --check crates/beatkernel/tests/runtime_fault_schedule.rs
cargo clippy -p beatkernel --test runtime_fault_schedule --locked -- -D warnings
python3 tools/wbs_status.py
```

QA logs: `target/wf/runtime-fault-schedule/qa-cli/{test,clippy,rustfmt,wbs}.log`.
WBS remained 85/193 Done with 13.04 unchecked W. No complete WBS13.04 or
native/hardware acceptance is claimed. These are substantive verification
results; task closure separately requires the Harness ordered receipt gate.

## Extend the schedule through real PCM and output ownership

The connected target `runtime_output_fault_schedule` exercises Runtime-produced
commands through the real Mixer and literal sample assets. Inject queue
saturation, invalid callback capacity, pending-output retirement refusal and
actual consumer disconnection. Check accepted judge state independently from
command admission, caller-buffer preservation on refusal, exact subsequent PCM
and retained original Mixer state. Compare callback partitions and fresh runs;
an uninterrupted reference alone is insufficient without literal expected PCM.

```sh
cargo test -p beatkernel --test runtime_output_fault_schedule --locked
```

A test-local owner can refuse retirement and later transfer the same Mixer
through `OutputOpenFailure`. This verifies the portable ownership contract;
its flag does not prove native callback cessation or physical device safety.
Destroying the actual Mixer/consumer must expose a disconnected queue to the
still-owned Runtime producer. The resulting command error must retain the
accepted judge transition and must not invent successful sound or completion.

The target uses mono 1000 Hz PCM `[0.25, 0.5, 0.75, 1.0]` with gain 0.5.
Keeping overlap below the Mixer's clipping limit makes missing voices visible
in the literal oracle rather than hiding them behind saturated output.

- Queue/render refusal schedule: first sound accepted, second sound refused
  by the one-slot ring while judging still hits, two PCM frames consumed, then
  third sound admitted. The eight-frame oracle is
  `[0.125, 0.25, 0.5, 0.75, 0.375, 0.5, 0.0, 0.0]`.
- Recovery/disconnect schedule: one active voice and one pending future command
  remain at cursor 3. Two retirement refusals preserve the same owner and
  original error. The Runtime queues another future sound while the consumer
  remains retained. One successful transfer resumes the original Mixer;
  the suffix is `[0.5, 0.125, 0.25, 0.5, 0.75, 0.375, 0.5, 0.0]`.
  Destroying that actual Mixer then makes command 5 fail as disconnected while
  its real judge hit remains accepted.

Each schedule uses two callback partitions and three fresh repetitions:
twelve scenario executions with eighteen Runtime/Mixer pairs including six
uninterrupted reference pairs. The literal arrays are checked separately from
reference equality. Render refusal uses a buffer prefilled with 91 and asserts
no mutation of buffer, physical/playback cursors, basis, rate, counters or pause.
Software owner drop instrumentation records premature destruction rather than
panicking during cleanup, so an assertion failure cannot cause a second Drop
panic and abort unrelated test diagnostics.

The additional stereo regression uses the same Runtime/queue prefix and an
interleaved asset with opposite channel signs. After two valid frames, it
queues another sound and submits three samples for a two-channel buffer.
`AudioError::InvalidBuffer` must leave its 91-filled buffer and all exposed
Mixer state untouched. Subsequent valid output must equal the mono oracle
interleaved with its negative:
`[0.125,-0.125,0.25,-0.25,0.5,-0.5,0.75,-0.75,0.375,-0.375,0.5,-0.5,0,0,0,0]`.
This adds one scenario/pair, giving thirteen scenario executions and nineteen
fresh Runtime/Mixer pairs in the complete three-test target.

Final author verification passed three tests, zero failures, including stereo
and cleanup corrections. Scoped formatting and whitespace checks passed;
the initial target Clippy also passed. Final author log:
`target/wf/runtime-output-fault-schedule-author-stereo.log`.
Remaining native output, network and UI fault campaigns stay part of WBS13.04.

Independent full source review and subsequent CLI QA passed the final target
on source `63963ce`: three tests, zero failures, scoped Rust 2021 formatting,
final target Clippy with warnings denied and WBS ID/status/dependency checks.
QA logs: `target/wf/runtime-output-fault-schedule/qa-cli/{test,clippy,rustfmt,wbs}.log`.
Native output, network/UI campaigns and full WBS13.04
acceptance remain unfinished.
