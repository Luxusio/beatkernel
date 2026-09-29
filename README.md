# BeatKernel

A low-latency, cross-platform runtime foundation for rhythm games, implemented
in Rust. The architecture and phased implementation specification are in
[plan.md](plan.md).

## Current implementation

The source implements **Phase 0: repository skeleton**, **Phase 1: integer time
and transport**, **Phase 2: canonical physical input**, and **Phase 3: binding**. Native input,
chart compilation, judgment, audio scheduling, and replay remain subsequent phases.

```text
beatkernel/
├── Cargo.toml
├── crates/
│   ├── beatkernel/              # OS-independent kernel
│   │   ├── src/time/            # nanoseconds and clock domains
│   │   ├── src/transport/       # rates and piecewise host/song mapping
│   │   ├── src/input/           # typed events, device identity, virtual FIFO, bindings
│   │   ├── tests/time_transport.rs
│   │   └── examples/transport.rs
│   └── beatkernel-platform/     # pure key normalization, native I/O still pending
│       ├── src/keyboard.rs
│       ├── src/{windows,linux,macos}/
│       └── examples/input_inspector.rs
└── .github/workflows/ci.yml
```

The only crate dependency is `beatkernel-platform → beatkernel`. The kernel has
no OS dependency, no game-specific assumptions, no unsafe code, and no third-party
dependencies. Platform dependency sections are reserved for the corresponding
target; native input/audio backends are not implemented or advertised as available.

## Build and verify

Rust 1.83 or newer is required, with `rustfmt` and `clippy` installed. Native
Linux builds also require a C linker and libc development files (for example,
the `gcc` and `libc6-dev` packages on Ubuntu).

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --release
cargo run -p beatkernel --example transport
cargo run -p beatkernel --example binding
cargo run -p beatkernel-platform --example input_inspector
cargo doc --workspace --no-deps
```

The CI workflow runs on Linux, Windows and macOS. Local test execution establishes
correctness on the available host; CI must execute before claiming results on the
other operating systems.

## Time and playback

`Timestamp` and signed `Duration` are i64 nanoseconds. Checked arithmetic returns
`Option`, with `None` on overflow. Clock points identify their domain explicitly;
`ClockMapper` is the interface for a caller-provided normalization/calibration
mapping. Native clock calibration is not part of this slice.

`Rate` is a normalized signed rational number. It handles positive playback,
zero-rate pause and negative-rate reverse without floating-point timestamps.
Scaling uses i128 intermediates and truncates fractions toward zero.

```rust
use beatkernel::time::Timestamp;
use beatkernel::transport::{Rate, Transport};

let ns = Timestamp::from_nanos;
let mut transport = Transport::new(ns(0), ns(0), Rate::NORMAL);
assert_eq!(transport.position_at(ns(1_000_000_000))?, ns(1_000_000_000));

// The new segment begins at the old segment's current song position.
transport.set_rate(ns(1_000_000_000), Rate::new(1, 2)?)?;
assert_eq!(transport.position_at(ns(2_000_000_000))?, ns(1_500_000_000));

transport.pause(ns(2_000_000_000))?;
assert_eq!(transport.position_at(ns(3_000_000_000))?, ns(1_500_000_000));
transport.resume(ns(3_000_000_000))?; // restores 1/2x

transport.seek(ns(4_000_000_000), ns(-1_000_000_000))?;
transport.set_rate(ns(4_000_000_000), Rate::REVERSE)?;
assert_eq!(transport.position_at(ns(5_000_000_000))?, ns(-2_000_000_000));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Host timestamps must use the same caller-selected monotonic clock domain.
Commands accept equal timestamps and reject times before the last successful
command. A seek changes song position without changing the rate. An initial
zero-rate transport resumes at 1x; later pauses restore the last nonzero rate,
including reverse. Errors leave transport state unchanged, and seek can recover
from a trajectory whose song position has overflowed.

The transport retains previous anchors so delayed timestamped input can query
the correct historical segment. At equal host timestamps, the last anchor wins.
Actual rate changes quantize to integer nanoseconds at the new anchor. Setting
the same rate, pausing while paused, and resuming while already playing preserve
the anchor, avoiding repeated fractional rounding from no-op commands.

Historical lookup is O(log n) and does not allocate. Mutations append to a retained
history vector, with O(n) memory and possible allocation. They belong on a control
thread, outside the future real-time audio callback.

The exact behavior and verification contract are documented in
[the time/transport requirements](doc/kernel/REQ__time-transport.md).

## Canonical physical input

`beatkernel::input` keeps runtime `DeviceId` separate from hardware descriptions
and physical control identity. Controls use HID page/usage, a native backend/code,
or a vendor namespace/code. Button, Axis, Touch, Pointer, Pose, Raw HID report and
Custom events retain typed payloads and common source/timing metadata. Touch keeps
contact identity and lifecycle; axis, coordinates and quaternion values pass
through unchanged in adapter-defined units.

`VirtualInputBackend` registers caller-assigned IDs and queues events in FIFO
acquisition order. Per-device sequences must not decrease; equal values permit
one raw report to emit several events through `DeviceAdapter` and
`PhysicalInputSink`. Clock conversion requires an explicit `ClockMapper` when
domains differ, preserving both incoming and native clock provenance. Rejections
leave sequence and queue state intact. Retirement preserves accepted events and
reserves the ID against reuse. Owned payloads, registration and enqueue may
allocate; this queue belongs outside an audio callback.

`beatkernel_platform::keyboard` provides pure Windows scan-code, Linux evdev and
macOS HID normalizers on every host. Standard supported keyboard positions
converge to page 0x07 usages. Unknown native codes retain their full value and
namespace; Windows fallback includes E0/E1. The Windows helper consumes complete
scan codes: native acquisition must assemble the multi-packet Pause sequence.
Scan 0x2B uses conventional HID 0x31 because the native code cannot distinguish
HID 0x31 from 0x32; HID-origin input retains the supplied usage.

The inspector example prints three virtual sources with the same canonical A
key, distinct native provenance, and explicitly mapped timestamps. It opens no
devices and reports no hardware latency. Golden fixtures and namespace sweeps
verify the tables alongside two-device and typed-event contract tests.

See [the canonical-input requirements](doc/kernel/REQ__canonical-input.md).

## Game controls and channels

`BindingMap` maps a complete physical control identity and `DeviceSelector` to a
caller-defined `GameControlId`. Two devices with the same HID key can have
different exact bindings. If an exact source/control rule matches, it overrides
common `Any` rules; unrelated exact rules leave fallback intact. Multiple
destinations fan out in insertion order, and removal preserves remaining order.
Identical selector/control/destination triples are rejected without editing the map.

Each `GameInputEvent` owns an unchanged physical sample and can outlive both the
input and map. Touch contacts, pointer/axis modes, pose data, and all timing/native
provenance remain intact. Raw HID and Custom payloads need adapter interpretation
before they have semantic controls for binding.

Mapping uses immutable current configuration and at most two linear passes,
without heap allocation for the supported semantic variants. Configuration edits
may allocate; constructing through validated additions is quadratic. Edits during
a held input stream can change later destinations, so consumers coordinate them
with gameplay state. This API does not track held state or synthesize releases.

Run the virtual `binding` example to see source 101 and 102 map the same HID A
to logical controls 10 and 20, with source 103 retaining touch data on channel 30.
See [the binding requirements](doc/kernel/REQ__binding.md).
The next specified phase is Windows native input. The full runtime remains in development.
