# Selected native input cadence

`linux_input_cadence` connects real Linux acquisition to the existing bounded
`IntervalJitter` and `InputDeliveryTelemetry` observers. It supplies source
tooling for plan P11 and §17.4; it is not an executed hardware result.

```sh
cargo run --release -p beatkernel-platform --example linux_input_cadence -- --help
cargo run --release -p beatkernel-platform --example linux_input_cadence -- evdev /dev/input/eventN 4 down 1000000 10
cargo run --release -p beatkernel-platform --example linux_input_cadence -- hidraw /dev/hidrawN none 1000000 10
```

Paths are explicit caller selections, not discovered defaults. Evdev selection
uses decimal keyboard HID usage and exactly one Down, Up or Repeat state; usage
4 is the canonical A key. Hidraw selects an explicit numbered input report ID
1..255 or `none` for unnumbered reports, using descriptor-derived native layout.
The caller must provide a source producing the selected signal at the specified
nominal period. The example does not generate input or infer a device's poll
rate from keyboard transitions. Duration is 1..60 seconds, nominal nanoseconds
1..i64::MAX. Help and numeric parsing precede opening any device.

Evdev's event point preserves the kernel CLOCK_MONOTONIC timestamp. Hidraw's
event point is the backend's userspace CLOCK_MONOTONIC read receipt; the kernel
does not provide a hardware timestamp in this path. A separate fresh same-domain
clock sample immediately after the selected read measures delivery age at the
example boundary. The two timestamp meanings must not be compared as equivalent
hardware measurements. First actual and last accepted metadata preserve source,
sequence and native provenance in the output.

The first selected event establishes the cadence baseline; it contributes no
fabricated interval. Subsequent equal timestamps are valid observations with a
negative nominal deviation. Source sequence non-increase or event timestamp
regression terminates the segment and increments a distinct counter. Evdev
SYN_DROPPED terminates the segment immediately without acknowledgment or
cross-barrier pairing; the handle closes. Exact missing native events remain
unknown. Sequence gaps can include intentionally unselected records and are
not counted as loss. Other signals are counted separately.

Two preallocated rings retain at most 65536 intervals/ages. Native backend
acquisition may allocate; this is owner-thread measurement code, not a real-time
audio callback or an allocation-free acquisition claim. Empty reads sleep for
100 microseconds; actual scheduler wakeup and queueing affect userspace receipt
measurements. No per-record printing occurs in the acquisition loop. Final
summaries are sorted/printed after the handle closes, including a partial segment
on read, clock, loss or ordering failure. Setup errors have no fabricated segment.
Percentiles describe the retained tail; accepted observation totals and native
records/ignored/discarded/loss-barrier counters describe their explicit scopes.
No observations produces `None`, not zero jitter.

The tool measures chosen timestamp interval deviation and delivery age. It
provides no acoustic latency, audio underrun measurement, exact event-loss count,
timestamp accuracy proof or Windows/macOS cadence result. Native acquisition,
1 kHz source measurements and fixture execution remain deferred by the user's
verification instruction. Locked workspace compilation covers source typing only.
