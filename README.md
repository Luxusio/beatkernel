# BeatKernel

A low-latency, cross-platform runtime foundation for rhythm games, implemented
in Rust. The architecture and phased implementation specification are in
[plan.md](plan.md).

## Current implementation

The source implements **Phase 0: repository skeleton**, **Phase 1: integer time
and transport**, **Phase 2: canonical physical input**, **Phase 3: binding**,
and **Phase 4: Windows native input**. **Phase 5: chart compilation** passed
independent review and CLI QA. **Phase 6: Instant/Hold judging** is implemented.
**Phase 7 audio implementation is in progress:** PCM loading, bounded command
queuing, deterministic mixing and native WASAPI streams are present. Independent
audio review and actual shared/exclusive playback verification remain pending.
Integrated gameplay, replay and the remaining phases are still required.

**Phase 4 passed independent review and CLI QA:** portable Raw Input processing and a Windows
QPC receipt sampler, native packet acquisition and explicit registration are
present. Their contract is [Windows input](doc/kernel/REQ__windows-input.md).
The inspector has captured device-attributed keyboard input in an isolated
Windows VM. **Phase 5 chart compilation:**
its [timing contract](doc/kernel/REQ__chart-compiler.md) covers BPM, STOP,
object endpoints and separate SV markers.

```text
beatkernel/
├── Cargo.toml
├── crates/
│   ├── beatkernel/              # OS-independent kernel
│   │   ├── src/time/            # nanoseconds and clock domains
│   │   ├── src/transport/       # rates and piecewise host/song mapping
│   │   ├── src/input/           # typed events, device identity, virtual FIFO, bindings
│   │   ├── src/chart/           # source charts and absolute compiled timelines
│   │   ├── src/interaction/     # typed evaluator and active-interaction seams
│   │   ├── src/judge/           # profiles, policies and forward Instant/Hold judging
│   │   ├── src/audio/           # PCM preload, bounded queue and deterministic mixer
│   │   ├── tests/time_transport.rs
│   │   └── examples/transport.rs
│   └── beatkernel-platform/     # pure input processing and Windows acquisition
│       ├── src/keyboard.rs
│       ├── src/raw_input.rs
│       ├── src/{windows,linux,macos}/
│       └── examples/{input_inspector,windows_input_inspector}.rs
└── .github/workflows/ci.yml
```

The workspace dependency direction is `beatkernel-platform → beatkernel`. The kernel has
no OS dependency, no game-specific assumptions, no unsafe code, and no third-party
dependencies. The platform uses pinned `windows-sys` and generated `windows`
bindings only on Windows,
with unsafe confined to native FFI; portable input modules prohibit unsafe.
Windows acquisition has native guest evidence; native Linux/macOS input and audio
backends remain subsequent phases.

## Build and verify

Rust 1.98.1 or newer is required. `rust-toolchain.toml` pins Rust 1.98.1 with
`rustfmt` and `clippy` for reproducible development. Native
Linux builds also require a C linker and libc development files (for example,
the `gcc` and `libc6-dev` packages on Ubuntu).

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --release
cargo run -p beatkernel --example transport
cargo run -p beatkernel --example binding
cargo run -p beatkernel --example chart
cargo run -p beatkernel --example audio -- --fixture
cargo run -p beatkernel-platform --example windows_audio -- --help
cargo run -p beatkernel-platform --example windows_audio -- --fixture
cargo run -p beatkernel --example judge -- --help
cargo run -p beatkernel --example judge -- --fixture
cargo run -p beatkernel-platform --example input_inspector
cargo run -p beatkernel-platform --example windows_input_inspector -- --fixture
cargo doc --workspace --no-deps
```

The CI workflow declares Linux, Windows and macOS checks using Rust 1.98.1.
Local and isolated-guest verification establish only their observed
results; the declared CI matrix has not been executed in this workspace.

Phase 4 packet/state tests pass in debug and release on Linux, and the new public
API examples pass as doctests. Rust 1.83 Windows GNU cross-check and strict Clippy
compile the Windows backend. Linked Rust 1.83 test binaries also execute in an
isolated Windows Server VM: six native API integration tests and five platform
unit tests pass. This verifies QPC, enumeration, ownership/cleanup and query
bounds. In the interactive guest desktop, the production inspector also captured
A Down/Up from nonzero native handle `0x10041`, runtime source 1, canonical HID
usage `0x07:0x04`, sequence 3/4, and QPC/posted-message provenance. Alt+F4 exited
with code 0 after registration cleanup. This is native Windows input from a
Hyper-V virtual keyboard; physical hardware latency remains unmeasured. The
latest inspector revision passed finite native execution and cleanup in a locked
guest with no acquisitions; its live-input capture was from an earlier build.

On Windows, run `cargo run -p beatkernel-platform --example windows_input_inspector
-- --seconds 10` in the interactive desktop and press keys in its window. Add
`--hid 0x01:0x04` for an explicitly selected HID collection. The application owns
its window, registration and foreground cleanup; the library never registers on
construction. Classes must stay exclusively owned until the guard closes.
The inspector prints receipt time and raw fields, limits HID previews to 32 bytes,
and omits device interface paths/serials. `--fixture` is explicitly synthetic on
all hosts; native mode reports unsupported on other OSes.

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

## Chart compilation

`beatkernel::chart::SourceChart` stores nonnegative integer beat ticks, rational
BPM, nanosecond STOPs, opaque object bindings and separate rational SV changes.
Compilation produces owned objects with absolute start/end song timestamps,
sorted by start time and ID. `objects_in_window(start, end)` borrows starts in
the half-open window without allocation. Compilation belongs on a control
thread.

This API accepts Rust data structures. BMS/osu! file parsers are not implemented
yet; future format adapters will translate their source semantics into this
model. The chart tests currently use synthetic source data.

At a shared beat, object endpoints and markers receive the pre-STOP time; the
new BPM and STOP affect following beats. SV never changes judge targets.
Duplicate IDs/markers, invalid ranges and overflow are explicit errors. At
120 BPM a beat-1 object lands at 500 ms. A 250 ms STOP and change to 60 BPM at
beat 1 place beat 2 at 1,750 ms. Run the chart example to see the compiled point
and range. [The chart contract](doc/kernel/REQ__chart-compiler.md) defines
rounding, capacity and adapter boundaries.

## Judging and console playback

`JudgeEngine` owns a compiled chart, a validated `JudgeProfile` and caller `Rule`
registrations mapping opaque interaction IDs to logical controls and evaluators.
`InstantEvaluator` accepts point objects; `HoldEvaluator` requires an end strictly
after the start. `with_policies` replaces candidate selection and grading.
The default resolver picks the closest eligible target, then earlier target and
ObjectId; `EarliestCandidate` provides another deterministic choice. Builtin
starts use `StartEligibility::ProfileButtonPress`: fresh Down inside the widest
profile window. Repeat and duplicate Down do not become new builtin presses.
Custom evaluators default to `EvaluatorDefined` and select accepted typed input
through a separate pending index, including Up or samples outside the builtin
window. Candidate resolution chooses at most one pending start per logical
destination. Builtins ignore nonbutton samples, which remain typed for custom
evaluators.
Pending objects with a declared deadline strictly before effective input time
cannot consume candidate selection; equality and absent deadlines stay eligible.

Pass an unchanged `GameInputEvent` and explicitly mapped song timestamp to
`push_input`. Both `push_input` and `advance_to` apply the signed profile offset
once; positive offset moves effective time later. Times may be equal or negative
but cannot regress. Windows have nonnegative asymmetric early/late bounds,
nested from narrow to wide with unique grade IDs; the first inclusive match
wins. Deadlines expire strictly after their late boundary, so advancing to a
deadline still permits input at that same time.

A Hold reports a graded head, acquires `(DeviceId, PhysicalControlId,
GameControlId)` ownership, then reports a separate tail. Only the owner Up
releases it: release before the widest early tail boundary breaks it, release
inside the tail windows grades it, and expiry yields a miss. There is no regrab
or automatic perfect tail. Callers combine stage results into their own score.
Ordered `JudgeEvent` values include object, stage, outcome, effective song time
and original input metadata; timeouts have no input provenance. Library-owned
validation errors leave state unchanged. Extension callbacks are trusted and
infallible; their panics and external side effects are outside that guarantee.
Pending interactions advance on declared deadline expiry; active interactions
also receive each engine advance. A pending interaction with no deadline does
not receive time callbacks.
Setup, dispatch and result ownership can allocate. The engine is a single-owner,
forward gameplay-thread API with no measured latency guarantee.

The example's `--fixture` prints a labeled synthetic transcript. With no
arguments or `--help`, it prints help. `--stdin` accepts nonblank lines of
`host_ns lane down|up|repeat`, where lanes are 1..4 and signed i64 timestamps
must be nondecreasing and at least 1,000,000,000 ns. Virtual canonical input
passes through four-control bindings and `Transport`; that host origin maps to
song zero. The fixed chart has lane 1 Instant at 500 ms, lane 2 Hold from
500 to 1,500 ms, lane 3 Instant at 1,000 ms and lane 4 Instant at 1,500 ms.
Grade 1 is inclusive +/-20 ms, grade 2 +/-100 ms, and offset is zero.

```sh
cargo run -p beatkernel --example judge -- --stdin <<'EOF'
1500000000 1 down
1500000000 2 down
2000000000 3 down
2500000000 2 up
2500000000 4 down
EOF
```

This produces five grade-1 stage hits with zero deltas. Equal timestamps are
valid and blank lines are ignored. Malformed fields, unknown lanes/states,
timestamps before origin and decreasing timestamps exit nonzero with a line
number. EOF advances to at least song 1,600,000,001 ns to report remaining
misses; timeout output says `input=none`. This console path uses virtual input.
Native gameplay/audio integration, file parsers, replay/snapshot restoration
for seek/reverse and the remaining Phases 7–15 stay required. The
[judge contract](doc/kernel/REQ__judge.md) specifies the API and extension limits.

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
Windows native input passed independent review and CLI QA. The full runtime
remains in development.

## Audio scheduling and Windows output

`beatkernel::audio` preloads PCM into a bounded `SampleBank`, schedules scalar
commands through a fixed-capacity SPSC queue and mixes into caller-provided
buffers without callback allocation or asset release. Play/Stop offsets use one
absolute frame grid. Rates preserve exact sample phase across buffer partitions;
Seek clears active voices while retaining future output-scheduled commands.
Core scheduling time and host/device clock domains remain explicit.

The `audio` example checks literal PCM mixing, Play/Stop offsets and partition
invariance. The platform `windows_audio --fixture` checks scheduled PCM and
PCM16 byte packing on every host. These fixtures are synthetic.

On Windows, use `windows_audio --list` to obtain an exact endpoint ID. Probe and
play require `--device ID --mode shared|exclusive`. With no format flags they
use the queried mix format; explicit format flags override that reported base.
`--help` lists arbitrary rate/channel/encoding/valid-bit/layout requests,
buffer/period frame or nanosecond sizes, optional supported rounding, engine or
legacy shared operation, event or shared timer wake, and MMCSS priority/off.
Unsupported formats and size suggestions never silently replace the request.

```sh
cargo run -p beatkernel-platform --example windows_audio -- --list
cargo run -p beatkernel-platform --example windows_audio -- --probe --device "ID" --mode shared
cargo run -p beatkernel-platform --example windows_audio -- --play --device "ID" --mode shared --seconds 2
cargo run -p beatkernel-platform --example windows_audio -- --play --device "ID" --mode exclusive --buffer frames:480 --period frames:480 --allow-rounding --seconds 2
```

Use a format the selected endpoint actually supports; the queried shared mix
format is not a promise of exclusive support. Shared engine/event buffering is
native-managed. To request independent legacy shared buffer sizing, explicitly
choose `--shared-period default --wake timer --poll-ms N --buffer frames:N`.
Exact sizing is the default. Probe buffer bounds are labeled as event queries;
timer playback queries its selected wake policy internally. Playback is finite
and bounded to 60 seconds, with a 4 MiB example tone preload limit. The CLI
reports requested/applied settings, submitted frames, raw device clock and
inferred metrics outside buffer fill, and requires activity beyond prefill,
clock progression and joined stop before reporting success.

Actual native shared/exclusive playback and independent audio review/QA remain
pending. The available Windows VM previously had no render endpoints. Windows
cross-compilation proves source compatibility, not playback. ASIO currently
returns an explicit unresolved-license error; native Linux/macOS audio remains
later work. See the [core audio contract](doc/kernel/REQ__audio.md) and
[Windows audio contract](doc/platform/REQ__windows-audio.md).
