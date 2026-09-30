# Native IOHID scalar cadence

`macos_input_cadence` observes an explicitly selected IOHID scalar signal using
the native input backend and shared MachClock. This is source tooling for plan
P11 and §17.4; compilation is not a native measurement.

```sh
cargo run --release -p beatkernel-platform --example macos_input_cadence -- --help
cargo run --release -p beatkernel-platform --example macos_input_cadence -- --list
cargo run --release -p beatkernel-platform --example macos_input_cadence -- REGISTRY_ENTRY ELEMENT_COOKIE 7 4 1 1000000 10
```

Help parses without opening IOHID. `--list` explicitly opens the scalar manager,
copies enumerated device identities and closes before printing. Measurement
requires exactly one current device matching the caller's nonzero IORegistry
entry, exact element cookie, HID usage page/usage and exact signed integer
value. `macos_native` prints scalar samples with cookies for signal inspection.
All identity/period fields are unsigned decimal, the scalar integer is signed
decimal; nominal interval is 1..i64::MAX nanoseconds and duration 1..60 seconds.
There is no implicit device or generated source. The caller must supply the
chosen signal at the stated cadence; ordinary key transitions do not represent
an inferred keyboard polling period.

IOHIDValueGetTimeStamp mach ticks remain in each selected sample, alongside
native metadata and normalized host time. IntervalJitter receives actual event
points; InputDeliveryTelemetry receives the same event and a fresh normalized
mach sample immediately after dequeue. The first selected point establishes
the baseline without an interval. Both observers retain at most 65536 values;
IOHID's acquisition queue is separately bounded to 65536 scalar samples.
Nearest-rank summaries describe retained history; accepted totals are printed
separately. Before any observed interval, its summary is `None`.

The measurement loop polls the owner-thread runloop with a 100 microsecond
timeout, drains without per-sample output, and checks its wall-time deadline
between samples. Actual runloop timing, scheduling, acquisition and queueing
affect receipt age. Native element filtering/conversion and timestamp accuracy
retain the existing IOHID backend's scope. Skipped unsupported elements are
reported in the manager's counters, not treated as known selected-signal losses.

Native poll error, callback queue overflow, selected timestamp regression or
non-increasing sequence terminates the segment. Removal of any acquired device
also terminates conservatively. No pair crosses removal/reconnection or silently
switches to a new session identity. Sequence gaps include nonselected elements
and do not infer missing events. Equal event timestamps remain valid zero
intervals. Selection compares native integer values, avoiding f32 axis narrowing.

Callbacks are unscheduled and the manager closes before final summaries and
first/last accepted sample metadata print, including partial failure segments.
Cleanup failures are reported separately; no zero observations replace absent
data. This is owner-thread tooling, not an allocation-free callback claim.
Physical input-to-audio latency, exact missing-event counts, hardware timestamp
accuracy and actual 1 kHz behavior are not established. Native execution, tests,
independent review and QA remain deferred by the user's sequencing instruction.
