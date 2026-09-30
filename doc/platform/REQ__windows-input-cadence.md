# Selected Windows Raw Input cadence

`windows_input_inspector` retains its ordinary native and synthetic-fixture modes
and adds an explicit native `--cadence` mode for plan P11 and §17.4 tooling.

```sh
cargo run --release -p beatkernel-platform --example windows_input_inspector -- --help
cargo run --release -p beatkernel-platform --example windows_input_inspector -- --seconds 10 --cadence DEVICE_ID 4 down 1000000
```

Selection requires a nonzero currently enumerated keyboard session DeviceId,
keyboard HID usage (decimal or 0x hexadecimal), one exact Down/Up/Repeat state
and positive nominal nanoseconds fitting i64. Cadence duration is 1..60 seconds,
default 10. The inspector prints enumerated device IDs, names and metadata
before starting. Session IDs are not persistent native paths; verify the printed
selected device for each invocation. An absent or non-keyboard selection fails.
No device is selected implicitly, no source is generated and no reconnect
retargets the chosen ID. The caller must provide the selected transition at the
stated period; keyboard poll rate is not inferred from ordinary transitions.
`--fixture` rejects cadence/native options and remains synthetic only.

Actual normalized QPC receipt timestamps from selected canonical button events
feed IntervalJitter. Raw Input has no hardware timestamp in this path. MSG.time
remains posted-message metadata and is not used as the cadence clock. A fresh
shared-QPC sample after native read and required foreground message cleanup
feeds InputDeliveryTelemetry, measuring receipt-to-inspector age separately.
The first actual matching event establishes the baseline without a fabricated
pair. Equal timestamps are valid; non-increasing selected sequence, timestamp
regression or domain change terminates the segment.

Both observers preallocate rings retaining at most 4096 measurements. Selected
metadata and totals are retained, with separate native acquisitions, native
read-rejection and ordering-regression counts and selected-removal status.
Sequence gaps include filtered records and cannot establish missing-event counts.
Raw Input's unavailable exact event-loss count remains unknown. Native read
rejection terminates conservatively, even for a nonselected packet; selected
device removal also terminates. No intervals bridge either discontinuity.

Cadence suppresses per-packet/event printing. The existing owner-thread message
pump still performs foreground WM_INPUT cleanup once and keeps registration
alive until explicit close. Its 1 ms idle sleep, OS scheduling and message queue
affect acquisition receipt cadence and delivery age. This is not an
allocation-free native input or real-time callback claim. Summary printing occurs
after registration close is attempted on normal exit and pump failure; a close
failure is printed separately. No observations produces absent summaries rather
than zero jitter. Retained-tail percentiles and total accepted pair counts have
different scopes.

The source was type-checked for Windows GNU without linking or native execution.
Actual device measurements, high-rate loss/order inspection, physical
input-to-sound latency, independent review and QA remain deferred. No acceptance
or native timing accuracy follows from compilation.
