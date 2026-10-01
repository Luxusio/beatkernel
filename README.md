# BeatKernel

A low-latency, cross-platform runtime foundation for rhythm games, implemented
in Rust. The architecture and phased implementation specification are in
[plan.md](plan.md).

## Current implementation

The source implements **Phase 0: repository skeleton**, **Phase 1: integer time
and transport**, **Phase 2: canonical physical input**, **Phase 3: binding**,
and **Phase 4: Windows native input**. **Phase 5: chart compilation** passed
independent review and CLI QA. **Phase 6: Instant/Hold judging** is implemented.
**Phase 7 audio implementation is present:** PCM loading, bounded command
queuing, deterministic mixing and native WASAPI streams are present. Independent
audio review and actual shared/exclusive playback verification remain pending.
Phases 8–10 now provide the integrated runtime, indexed visual projection and
complete logical replay checkpoints. Section restart preparation selects frames
from original PCM and creates fresh output owners. Phase 11 generic repeated,
composite and tracking interactions and Phase 12 Linux evdev/hidraw/ALSA source
are present. Phase 13 IOHID/CoreAudio source is also implemented and cross-checked;
Phase 14 provides a bounded BMS text adapter and a separate offline composition sample.
These additions have build checks, while review, tests and native execution are
deferred by user instruction. The full runtime is not yet complete.

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
│   │   ├── src/runtime/         # integrated forward loop and section restart setup
│   │   ├── src/visual/          # indexed Lane/Point/Path logical projection
│   │   ├── src/replay/          # accepted operation recording and checkpoints
│   │   ├── src/telemetry/       # bounded software processing percentiles
│   │   ├── tests/time_transport.rs
│   │   └── examples/transport.rs
│   └── beatkernel-platform/     # native Windows/Linux/macOS acquisition and output
│       ├── src/keyboard.rs
│       ├── src/raw_input.rs
│       ├── src/{windows,linux,macos}/
│       └── examples/{input_inspector,windows_input_inspector}.rs
└── .github/workflows/ci.yml
```

The native library dependency direction is `beatkernel-platform → beatkernel`.
`adapters/beatkernel-bms` depends only on core; `samples/bms-runtime` composes all
three at the executable boundary. The kernel has
no OS dependency, no game-specific assumptions, no unsafe code, and no third-party
dependencies. The platform uses pinned `windows-sys` and generated `windows`
bindings only on Windows, and the optional `cc` build dependency for explicit
SDK bridge builds,
with unsafe confined to native FFI; portable input modules prohibit unsafe.
Windows acquisition has native guest evidence. Linux acquisition and ALSA output
have native source implementations; Linux/macOS native runtime evidence is pending.

## Unified BMS application

The existing `beatkernel-bms-runtime` crate now provides one primary executable
with `player`, `play`, `replay`, `play-replay`, `render`, `render-replay` and `compete` modes.
No extra app, UI or networking crate is added. `play --help` prints this host's
native device/buffer options. Native play accepts saved opponents and optional
two-player TCP progress exchange:

```sh
cargo run -p beatkernel-bms-runtime -- play --help
cargo run -p beatkernel-bms-runtime -- player --help
cargo run -p beatkernel-bms-runtime -- compete --chart song.bms --local-replay now.bkr --ghost-self past.bkr --ghost-other other.bkr --song-ns 10000000000
```

Add `--ghost-self FILE` or `--ghost-other FILE` to native `play` options, up to
eight opponents. Capture files using `--record-replay NEW_FILE`. Peers select
`--mp-host 127.0.0.1:9000` and `--mp-join 127.0.0.1:9000` respectively, with the
same chart and judging profile and their own explicit native device options.
Use an explicit reachable IP for another machine. Network loss leaves local
play running; remote progress is self-reported and song starts are independent.
The graphical `player` mode uses `winit` on the main thread and `wgpu` to draw
notes, holds, judgment feedback and counters from actual game snapshots.
Use `player --library DIR` or `player --chart PATH` with optional advanced native
device/buffer/binding options as `play`. Up/Down select, Enter starts, and
Escape/focus loss cancels. Native settings can also be edited before play.
Catalog rows can also be clicked; Start, Cancel, Return and Exit buttons use
the same session commands. Mouse hit testing follows the rendered logical
viewport and never contributes gameplay input timestamps.
`--gpu-backend auto|vulkan|dx12|metal|gl` selects graphics discovery;
`--present fifo|immediate|mailbox` selects presentation (default FIFO), with errors
for unsupported explicit choices. `--ui-fps` and `--ui-lookahead-ms` control drawing.
Native input/judging stays on the game thread, audio keeps its output
worker/callback, and socket I/O has its own worker. The graphical player is the product interface; native `play` commands remain
available as developer compositions. Ranked online services are not implemented.
Saved own/other replay prefixes and peer-reported progress feed the graphical
comparison view for each local player. Connection state and the last received
peer prefix survive cleanup; peer song time stays independent. Display updates
are coalesced and never change judging or audio scheduling.
Local panels keep their normal lane space until Comparisons/C is toggled;
solo comparisons use the sidebar.
See the [competition contract](doc/kernel/REQ__bms-competition.md) for limits.
Solo play resolves omitted devices automatically: system/default audio output
and a usable keyboard. Device assignment is reserved for multiple local players;
three/four and larger rosters use the same collection-based model. Native Linux
APIs accept repeated `--local-input PATH` or stable `--local-player ID:PATH` for
2..64 players;
The graphical player draws independent local panels with pages for larger
rosters. Settings → Players provides count and distinct-keyboard assignment for
Linux, Windows and macOS local groups, while solo keeps automatic input. Windows uses
stable `--local-player ID:INTERFACE_PATH` assignments with one Raw Input pump.
macOS uses stable --local-player ID:REGISTRY assignments over IOHID and CoreAudio. The shared execution primitive
owns independent core runtimes with one authoritative song transport and output
producer; native solo playback now uses the same composition.
Selection F3 or the search field filters title and artist with whitespace-separated
substring tokens. Up/Down navigates matches; Enter exits search editing before
playing, and Escape clears it before ordinary close. Empty results cannot play
or open Records for a hidden chart. Search survives Settings Back and Play return.
Selection supports PageUp/Down by fifteen rows and Home/End within search
results. Wheel over a chart row moves through those results, with fractional
trackpad movement accumulated and at most one page admitted per event.
Focused search Home/End moves its text cursor.
Native IME input supports search, settings values and profile paths. Composition
previews stay separate from saved drafts; switching fields or leaving the active
screen discards them. Existing bitmap glyph fallback still applies.
Selection rows show artist metadata below the title when available.
In Records, Remove Own and Remove Other remove one selected record occurrence
from the competition draft without deleting its file. Selected own/other counts
show duplicates; Apply remains separate. An overfull edited draft can still be
repaired by removing records, while Add is disabled at eight opponents.
Titles and artists clip to separate line bands within the row, including
cropped glyph textures at the edges. Long text stays inside the row padding;
overhanging glyphs may be cut rather than wrapped or shortened with an ellipsis.
Use `player --library DIR --title-font PATH` to draw catalog titles and artists with a
caller-provided TrueType/OpenType font. Both use a fixed 14-pixel prepared
atlas, uploaded once per renderer and rebuilt with its texture binding on
renderer recovery. Invalid or excessive font data fails before window startup.
Other controls retain the bitmap font; shaping and fallback remain pending.
F5 or Retry starts the same chart again after the previous native session has
finished cleanup. The accepted chart/device/timing/roster options stay pinned;
recorded retries use distinct .retry<N>.bkr stems and create-new saves.
Settings Practice (button or F6) edits exact start and optional end as seconds,
M:SS or H:MM:SS with up to nine fractional digits. Tab/click selects the field;
empty end means through song end, and a configured end must follow start.
Done updates both fields atomically in the draft; Settings Apply selects the
next original-song section. Back discards, Full Song clears both endpoints,
and Through End clears only the end. Raw PRACTICE START/END (NS) fields remain
available in Settings and profiles;
F5 retries the selected start. Earlier note heads/crossing holds are excluded, and
overlapping automatic BGM resumes from original PCM frames. Section recordings
store their start for logical replay, matching ghosts and recorded audio output.
During live Play, F7/Mark bookmarks the latest observed native song position;
F8/Restart Mark preflights that exact start and waits for native cleanup before
creating a fresh session. The mark survives retries of this session, including
Results. F5 keeps the original pinned start. Replay Watch cannot override its
recorded start. Snapshot delivery is coalesced, so marks use the latest observed
position, not the physical key event timestamp. Linux ALSA, Windows WASAPI
shared/exclusive and macOS CoreAudio support solo/local 2..64 F9/Pause without
network competition, using native-frontier acknowledgement, shared Transport
fencing and paused-key reconciliation. Replay Watch supports the same F9 control
when an actual output/host clock pair is available, freezing recorded progress
and sounds together. ASIO and network pause,
live scrubbing and gapless repetition remain work; native/GUI execution is
still unverified. In live nonnetwork play, F7 marks a loop start, F10 marks a later
end and F11 enables a fresh native finite session with exact start/end options.
Enabling first preflights, cancels, drains and joins the current owner. Audio
stops at its immutable exclusive PCM frame end and judging caps at the logical
end. Repetition waits for the actual native presented/drained endpoint, successful
cleanup and worker join. Coalesced UI positions do not stop audio or authorize
repetition. New sessions use one preroll and distinct replay retry filenames.
F5 and explicit cancel disable loops; toggling off leaves the current finite
section to finish. Reopening can leave a gap; this is not gapless playback.
Linux solo and local 2..64 owners can opt into `--end-ns N`, strictly after
`--start-ns`, to use the immutable audio/judging ends and wait for actual native
presentation plus drained input before finite-prefix completion. Manual pause
and short resume retain their physical gaps. Every assigned keyboard must drain
and every member must reach the same logical end. SDK-enabled Windows ASIO
solo/local owners also accept finite prefixes, using actual rendered block and
assessed latency intervals. Completion waits for the upper host frontier of a
block starting at or after the endpoint and drained input; prepared frames alone
do not finish a session. Physical accuracy remains unknown and ASIO pause is
still unsupported. Network endpoint support remains unfinished. Portable regression fixtures are prepared for
later execution, while native hardware/GUI/acoustic checks remain unverified.
Windows WASAPI shared/exclusive and macOS CoreAudio solo and local 2..64 owners also accept
`--end-ns`, using their actual PCM grid and native output/host clock relation.
The endpoint freezes audio, caps judging, and finishes an incomplete prefix only
after output presentation, input collection drain and pending resume reconciliation.
Finite ASIO and network sessions are rejected
before session resources are opened. Omitting the end retains full-song completion.
A first clock observation already beyond the endpoint fails explicitly because
no actual lower bracket exists. Native device execution remains unverified.
Settings Records browses an explicit directory and previews a selected recording
against the current chart, judging profile and practice start before attaching
it as My Record or Other Record. Changes stay in the settings draft until Apply.
F2 or Settings opens a bounded advanced draft editor for devices, buffers, timing,
bindings and competition options. Apply validates through the existing native
parser and updates the next session; Back discards. Missing device/binding
configuration can be overridden there before starting; device selection is optional. Audio Devices queries native
output metadata on the settings worker; select an entry and Use Device to copy
its exact ID into the draft. Apply remains separate. ASIO discovery requires an
explicit registry view. Keyboard metadata discovery is source-integrated for
automatic preparation and Linux per-player assignment. Clipboard and IME
composition remain pending. `--profile PATH` loads saved
native and display options; explicit arguments replace matching profile entries.
Settings Load/Save use an editable path and the same serialized settings worker. Save stores
the draft; Apply remains separate. Profiles retain the OS identity and exclude
chart selection. Version 2 includes GPU backend, presentation mode, FPS and note
lookahead; version 1 loads with display defaults. Settings → Display edits these
values. GPU backend changes require saving and restarting with that profile;
other display changes apply
between play sessions. Actual file-I/O and GPU acceptance remain pending.
See the [player contract](doc/kernel/REQ__bms-player.md) for lifecycle and scope.
Source compilation does not establish executed GUI, multiplayer or playback.

GPU geometry and async renderer initialization are exposed by the `graphics`
feature, independently of desktop window ownership. This supports later WASM
reuse; a browser player still needs canvas/startup/input/audio/files/network
adapters. Check the reusable library with:

```sh
cargo check -p beatkernel-bms-runtime --lib --no-default-features --features graphics --target wasm32-unknown-unknown --locked
```

The graphics foundation supports raw RGBA8 texture upload/removal and clipped
sprite quads. Contiguous texture batches preserve painter order; ASCII text uses
one atlas quad per glyph. Texture resources have finite count/byte admission,
and stale handles are rejected. Image decoding and multilingual font shaping
remain separate preparation work.

Headless commands build with `--no-default-features`. Project-authored code is
MIT; preserve the [graphics dependency notices](samples/bms-runtime/THIRD_PARTY_NOTICES.md),
including winit's Apache-2.0 license, when distributing binaries.

## Build and verify

Rust 1.98.1 or newer is required. `rust-toolchain.toml` pins Rust 1.98.1 with
`rustfmt` and `clippy` for reproducible development. Native
Linux builds also require a C linker and libc development files (for example,
the `gcc` and `libc6-dev` packages on Ubuntu).

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
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
The `custom_judge` example sends the same virtual bound key to default and custom
judges, illustrating later-target priority and directional grade labels:
`cargo run -p beatkernel --example custom_judge`. Custom policies retain timing
eligibility; replay checkpoints additionally require their snapshot hooks.
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
cross-compilation proves source compatibility, not playback. ASIO has a separate
[stream API](doc/platform/REQ__asio-stream.md) source implementation; SDK compilation
and native playback remain pending. WASAPI reports `BackendUnavailable(Asio)` for such requests.
Project-authored source and builds without ASIO are distributed under MIT,
with applicable third-party notices retained. Builds incorporating the ASIO SDK follow
[GPLv3 distribution and source-delivery conditions](doc/platform/REQ__asio-distribution.md).
Read-only [ASIO driver discovery](doc/platform/REQ__asio-driver-discovery.md)
lists registrations from an explicit Windows registry view without loading the
SDK or a driver. For example:

```sh
cargo run -p beatkernel-platform --example asio_inspector -- --view native
cargo run -p beatkernel-platform --example asio_inspector -- --view 64 --max-drivers 256 --max-value-units 4096
```

These discovery commands do not establish ASIO stream support or device output.
Optional [ASIO driver control](doc/platform/REQ__asio-driver-control.md) uses an
original C++ bridge against a supplied SDK. On Windows, set
`BEATKERNEL_ASIO_SDK_DIR` to the SDK root and use
`cargo check -p beatkernel-platform --features asio-sdk --locked` for an MSVC
target with MSVC or clang-cl. Enabled Windows GNU SDK builds are currently rejected
pending compatible ABI support. Default CI and verification commands check the SDK-free build;
`--all-features` on Windows requires these ASIO prerequisites. Driver controls
expose native capabilities and explicit rate changes. Clock-source controls
enumerate bounded actual native identities and explicitly select an enumerated
internal/external source before stream construction. Raw names, current flags
and associated input channel/group remain visible. Rate-zero external sync is
a separate request; source selection does not prove external signal presence.
`windows::asio::stream::AsioStream`
consumes a control and Mixer, maps distinct output channels, creates SDK double
buffers and primes B before an explicit one-time start. Callback state stays alive
until stop/dispose/Release and callback drain. Native reset/rate/buffer notifications
require explicit reconstruction. Software prepared frames and raw driver sample
position/system time are separate; no physical synchronization is inferred.
Portable `audio::asio` validates exact buffer
sizes and distinguishes positive Hertz from explicit external clock selection.
Its [planar PCM conversion](doc/platform/REQ__asio-pcm.md) extracts selected mixer
channels into eighteen native ASIO PCM layouts, covering both byte orders and
reduced valid-bit containers. Conversion allocates nothing, checks extents and
finite selected samples before writing, and explicitly rejects DSD/unknown types.
`AsioBlockRenderer` renders the actual Mixer into preallocated scratch and validates
all planes and all mixed samples before delivery. SDK-free fixtures compile on all
three targets; execution remains deferred. Metadata-only Windows Rust type-checking
of the optional wrapper does not compile/link the SDK C++ bridge or establish an
enabled GNU SDK build. Actual SDK-enabled builds still require MSVC and supplied SDK.
The separate BMS sample forwards this feature through its own `asio-sdk` feature;
its default remains SDK-free.
See the [core audio contract](doc/kernel/REQ__audio.md) and
[Windows audio contract](doc/platform/REQ__windows-audio.md).

The [runtime](doc/kernel/REQ__runtime.md) shares JudgeEngine with
[replay](doc/kernel/REQ__replay.md); ReplayRecorder captures accepted live
operations without another judge. The [bounded input codec](doc/kernel/REQ__input-codec.md)
preserves complete canonical events/native provenance and raw IEEE float bits.
The separate BMS runtime's three native executables support optional
full-song completion using actual judge, mixer and native presentation state.
Live play defaults to the whole song; optional `--seconds 1..3600` retains a
diagnostic cutoff. Final keysound and BGM tails drain before normal completion;
source compilation does not establish native or acoustic acceptance. See the
[player contract](doc/kernel/REQ__bms-player.md).
They also support optional
[live replay capture](doc/kernel/REQ__bms-replay-capture.md) with
`--record-replay NEW_PATH`, `--replay-max-records N` and `--replay-max-bytes N`.
Defaults are 1,000,000 accepted operations and 64 MiB of encoded data. Inputs
and explicit advances come from the actual runtime reports; accepted prefixes
are retained after failure. Files are created exclusively after native cleanup.
The application header fingerprints the pristine compiled judge/profile setup;
it does not authenticate BMS source or PCM assets. Capture performs bounded
control-thread allocation/encoding work and records no physical audio timing.

The separate [`replay_bms` inspector](doc/kernel/REQ__bms-replay-playback.md)
reconstructs captured BMS play with the stored profile and the same JudgeEngine:

```sh
cargo run -p beatkernel-bms-runtime --bin replay_bms -- --chart CHART.bms --replay SESSION.bkr
cargo run -p beatkernel-bms-runtime --bin replay_bms -- --chart CHART.bms --replay SESSION.bkr --song-ns 12400000000
```

It checks the exact runtime version and recompiled setup identity before replay,
loads no PCM assets, and prints logical results and the actual engine state hash.
`--cursor N` selects an exact operation boundary; `--song-ns N` performs core
time seek, including a boundary advance when needed. These options are mutually
exclusive. File/record caps default to the capture defaults and can be adjusted
with `--max-bytes N` and `--max-records N`. Failed-session prefix logs reconstruct
only their recorded extent. This inspector provides no native audio output;
the commands above and replay comparison fixtures remain unexecuted.

[`render_replay_bms`](doc/kernel/REQ__bms-replay-audio.md) renders the captured
judgments and chart BGM through the actual core Mixer, with shared bounded WAV
preparation:

```sh
cargo run -p beatkernel-bms-runtime --bin render_replay_bms -- --chart CHART.bms --replay SESSION.bkr --output NEW.f32le --seconds 60 --rate 48000 --channels 2
```

The output is newly created raw interleaved f32le. Preroll defaults to three
seconds; `--preroll-ns`, `--block-frames`, `--command-capacity`, `--voices`,
`--max-records` and `--max-bytes` expose finite controls. Audio uses recorded
unoffset song times, while logical counts/hash describe the full recording even
when output is cut short. Empty logs have no sounds, and prefix logs include no
later BGM. This reconstructs song-time sounds; original native scheduling points
and past audio queue failures were not recorded. This command's execution
remains pending.

The [`play_replay_bms` native player](doc/kernel/REQ__bms-native-replay.md) sends
the same plan to WASAPI on Windows, ALSA on Linux, CoreAudio on macOS or optional
Windows ASIO. Select
`--chart PATH --replay PATH --device ID --rate HZ --channels N`
with optional `--seconds N`;
Linux also requires `--buffer-frames N --period-frames N`, and macOS requires
`--buffer-frames N`. Windows accepts `--mode shared|exclusive` and
`--shared-policy engine-period|legacy`, buffer/period frames or native defaults.
Explicit `--backend wasapi|asio|alsa|coreaudio` must match the host; omission keeps
the existing host default. ASIO additionally requires sample feature `asio-sdk`,
an enumerated driver CLSID as `--device`, explicit `--asio-view native|32|64` and
distinct mixer-ordered `--output-channels 0,1`. It rejects mode/shared-policy/period
flags and uses driver-preferred or exact buffer frames. The actual driver rate
must match `--rate`; no rate/device fallback occurs. The host supplies a hidden
window until stream close/drain. For a Windows MSVC environment with the supplied
SDK, replace the example CLSID with the explicitly enumerated registration:

```sh
cargo run -p beatkernel-bms-runtime --features asio-sdk --bin play_replay_bms -- --chart song.bms --replay session.bkr --backend asio --device "{12345678-9ABC-DEF0-1234-56789ABCDEF0}" --asio-view native --output-channels 0,1 --rate 48000 --channels 2 --buffer-frames 256 --seconds 30
```

SDK-combined artifacts follow the documented GPLv3 distribution conditions.
This command remains unexecuted; actual SDK/MSVC build and native output remain
unverified. Live ASIO input play uses the separate `windows_bms` composition
described below, with explicitly assessed finite clock relations.
Preroll, lookahead, command capacity, voices and replay limits are configurable.
Commands retain planned output times and are supplied from actual completed
Mixer reports; explicit late/queue/native failures end through cleanup. The
optional wall cutoff includes preroll. Omitting it finishes the actual recorded
prefix and queued PCM tails through native presentation drain; this does not
prove acoustic completion. SDK-enabled ASIO requires explicit multimedia-clock
selection and timer/drift/output-latency assessments. Actual rendered blocks
wait in a bounded queue until fresh QPC reaches their upper presentation interval,
then advance recorded visuals and natural drain. GUI Watch preserves exact
frame buffers and routing, and queries omitted rate from the selected driver
before PCM loading. ASIO pause remains unsupported; physical accuracy and
SDK/MSVC/driver execution remain unverified.

The graphical app's Settings → Records → Preview → Watch uses the same playfield
and native recorded audio. W watches when the record list has focus; F5 retries
the pinned recording after cleanup. REPLAY and RECORD PREFIX RESULTS distinguish
recorded prefixes from full-song results. Only draft output settings are used;
watching never acquires keyboards, uses network input or saves a new capture.
Accepted live settings remain unchanged. Device defaults resolve on the game
owner, and absent native presentation remains unavailable. GUI/audio execution
is still deferred.

The [durable replay codec](doc/kernel/REQ__replay-codec.md) stores ordered operations,
runtime identity and optional calibration metadata with explicit byte/count limits.
Logical restoration does not rewind hardware.
[Section restart](doc/kernel/REQ__section-restart.md) prepares a fresh Mixer and
queue at an explicitly selected original source frame. The host must stop/reset
old output and establish the first sample's presentation-to-host clock mapping.
Integer frame selection prevents accumulated selection rounding; it does not
prove physical synchronization. The portable `section_restart` example renders
the same suffix into newly prepared software outputs and demonstrates explicit
synthetic output/host observations. The [affine clock mapper](doc/kernel/REQ__clock-calibration.md)
accepts supplied clock pairs, finite validity and explicit uncertainty. It never
collects observations or promotes hardware observations to an exact relation.

The Windows `windows_runtime` example accepts `--song FILE.wav --start-ns N`
and `--restarts 1..8`. Each repetition uses fresh output and an observed WASAPI
position/QPC relation to anchor the applied source frame. Ongoing observations
guard freshness and apply continuous bounded Transport rate corrections. See the
[presentation contract](doc/platform/REQ__wasapi-presentation.md). Native playback
and physical synchronization remain unverified.
Visual projection also exposes Polar and custom logical output, and
[reverse keysound playback](doc/kernel/REQ__reverse-playback.md) provides explicit
normal-sample, reversed-sample and mute policies on a dedicated Mixer/queue.

The `runtime_visual` example connects virtual physical input and the actual
Runtime to a reusable logical RenderFrame. Its finite four-lane/path SVG contact
sheet uses returned song times and emitted judge events rather than a fixed
projection timestamp. It creates a new output file and keeps rendering outside
the kernel. This is an authored software composition; execution and renderer
QA remain deferred.

```sh
cargo run -p beatkernel --example runtime
cargo run -p beatkernel --example visual
cargo run -p beatkernel --example runtime_visual -- new-runtime-frames.svg
cargo run -p beatkernel --example reverse_playback
cargo run -p beatkernel --example replay
cargo run -p beatkernel --example replay -- --save new-demo.bkr
cargo run -p beatkernel --example replay_file -- new-demo.bkr new-copy.bkr
cargo run -p beatkernel --example section_restart
cargo run -p beatkernel --example generalization
cargo run -p beatkernel --example contact_rebind
cargo run --release -p beatkernel --example runtime_bench -- --help
cargo run -p beatkernel-platform --example linux_native -- --help
cargo run --release -p beatkernel-platform --example linux_input_cadence -- --help
cargo run --release -p beatkernel-platform --example macos_input_cadence -- --help
cargo run -p beatkernel-platform --example device_adapter
```

The [contact policy example](doc/kernel/REQ__generalization.md) uses existing
custom evaluator hooks for fixed-contact sustain and same-surface reacquisition
after normal release within a configured grace. It preserves device/contact
identity in snapshots and uses the same judge during replay. Its fixtures are
authored and compiled; execution remains deferred.

The [Linux cadence tool](doc/platform/REQ__linux-input-cadence.md) selects one
native keyboard signal or hidraw report ID and an explicit nominal period. It
reports retained interval deviations, delivery age, provenance and native counters
without per-event printing; native execution remains deferred.
The [IOHID cadence tool](doc/platform/REQ__macos-input-cadence.md) provides the
same observers for an explicit registry identity, element cookie and scalar value,
preserving original mach timestamps.
The [Windows inspector cadence mode](doc/platform/REQ__windows-input-cadence.md)
uses `--cadence DEVICE_ID HID_KEY_USAGE down|up|repeat NOMINAL_NS` to summarize
selected keyboard QPC receipt intervals; these are acquisition receipt times.

See [Linux native requirements](doc/platform/REQ__linux-native.md) for explicit
nodes/endpoints and supported settings. The `linux_native audio` command also
prints a [joined render-worker cadence summary](doc/platform/REQ__alsa-render-cadence.md)
with an explicit bounded prefix and startup-fill scope.
WASAPI `windows_audio` and CoreAudio `macos_native` also print
[direct render-start cadence](doc/platform/REQ__native-render-cadence.md) after
cleanup, preserving their distinct software clock boundaries.
Optional ASIO live/recorded BMS output now opts into QPC render-start cadence,
excluding buffer priming and reporting only after callback drain. Existing
clock-free ASIO preparation remains available.
Exact ALSA sizing is the default;
optional rounding reports applied period/buffer sizes. Evdev event loss requires
an explicit queried-state acknowledgment before gameplay resumes.
Separate [ALSA timing snapshots](doc/platform/REQ__alsa-timing.md) associate native
status, signed delay and monotonic timestamp with worker submission counts. Sound
frame position is an estimate; acoustic latency and automatic input calibration
remain unmeasured.
ALSA also retains the [last mixer render report](doc/platform/REQ__alsa-render-telemetry.md)
with actual late/rejected command counters, independently of native frame submission.

The portable [device adapter registry](doc/platform/REQ__device-adapters.md)
connects caller-owned native descriptors and raw HID reports to bounded canonical
fanout. Adapters own per-device state and exact acquisition metadata is retained;
native APIs and game controls remain separate. Its example uses a synthetic vendor
report decoder and actual device-aware bindings.
[Interval jitter telemetry](doc/kernel/REQ__telemetry.md) compares supplied clock
pairs with an explicit nominal period, separately from processing percentiles.
The offline benchmark's cadence observations are generated synthetic data.
The same contract provides bounded input delivery-age telemetry from explicit
same-domain event/receipt points. Native BMS diagnostics distinguish Windows
QPC receipt-to-runtime age from Linux kernel and macOS IOHID event-to-runtime
age. These retained percentiles remain separate from CPU processing and physical
input-to-sound latency.

macOS has explicit IOHID device acquisition and CoreAudio packed-float32 output
with requested/applied rate and frame sizes. See the
[macOS contract](doc/platform/REQ__macos-native.md). Apple-target compile checks
cover its native source; successful linking, execution and hardware timing remain
unverified. [Phase 15 SDK status](doc/kernel/REQ__sdk-status.md) retains the
specified condition for introducing a C host ABI.
CoreAudio retains the [last successful mixer report](doc/platform/REQ__coreaudio-render-telemetry.md)
with late and rejected command counters after stop, separately from callback
presentation timestamps and native device delivery.

macOS also exposes an explicit timestamped raw-report input mode. It retains
native bytes/IDs and mach arrival timestamps for host-selected vendor adapters;
the default scalar value mode is separate. Raw mode requires an available native
timestamped callback API. Report framing conversion requires a declared ID layout,
preserving the original envelope instead of guessing from payload bytes.

The [BMS adapter](doc/kernel/REQ__bms-adapter.md) parses bounded UTF-8 text,
base/direct/extended BPM, STOP, measure lengths, layered BGM, paired LNTYPE1
and LNOBJ holds with exact rational subdivision. LNOBJ endpoints close the nearest
preceding visible head on their lane; endpoint tokens are retained as metadata
and remain silent even when their WAV is defined. Unsupported commands fail explicitly;
this is a documented subset, not universal BMS compatibility. It returns real
SourceChart/rules/sample mappings without opening assets or depending on platform.
The [offline sample](doc/kernel/REQ__bms-sample.md) uses shared WAV preparation,
sends synthetic input through Runtime and interleaves scheduled commands with
chunked float32 PCM rendering. Total notes are independent of concurrent voices
and outstanding commands; finite capacity failures are explicit. Output channels
default to two; an optional final argument selects another exact channel count.
It does not open a sound device. Both commands require actual files:

```sh
cargo run -p beatkernel-bms --example load_bms -- chart.bms
cargo run -p beatkernel-bms-runtime -- chart.bms new-output.f32le 30 48000
```

The separate [Windows BMS binary](doc/kernel/REQ__bms-native.md) uses actual Raw
Input, loaded note/BGM timing and WASAPI shared/exclusive output by default, or
explicit optional ASIO output. It requires an explicit device and HID-key-to-lane
bindings and exposes requested buffer sizes and profile offsets. WASAPI also
exposes periods. Its source has not established native playback.
Its [presentation observer](doc/platform/REQ__presentation-discipline.md) tracks
progressing device/host observations during playback and applies bounded,
continuous transport rate corrections. Stale observations or excessive clock
disagreement fail explicitly; this does not establish a physical timing bound.
[Shared preparation](doc/kernel/REQ__bms-preparation.md) provides bounded asset
loading with a WAV default and an injected off-thread decoder boundary; compressed
formats are not implemented by that default. Explicit mono-to-stereo conversion
is available without changing source frame positions.

[Rolling BGM admission](doc/kernel/REQ__bms-bgm-admission.md) keeps native command
storage independent of total BGM count. Its explicit lookahead and outstanding
credit budget use completed mixer frames; missed unadmitted cues fail with their
original mapped times. This schedules preloaded assets and does not stream PCM.

```sh
cargo run -p beatkernel-bms-runtime --bin windows_bms -- --help
```

Live ASIO requires Windows, sample feature `asio-sdk`, caller-supplied SDK/MSVC
toolchain, exact driver CLSID, registry view and ordered output channels. Use
`--backend asio --asio-view native --output-channels 0,1` with the chart/device/
seconds/binding options. It queries the driver's actual integral rate and rejects
WASAPI mode/period/shared-policy flags and nanosecond buffer sizes. Explicit
`--asio-system-clock multimedia` declares this driver's timer source; supply
`--asio-timer-error-ns`, `--asio-drift-error-ns` and `--asio-latency-error-ns`
assessments rather than relying on inferred precision. Optional
`--asio-anchor-age-ns` defaults to a one-second finite relation horizon.
Fresh shared-QPC timer brackets and coherent rendered-block observations feed
startup calibration and the existing continuous correction loop. Replay capture
and input/judge logic use the same path as WASAPI. SDK-combined artifacts follow
GPLv3 conditions; the default ASIO-free build remains MIT.

The [Linux BMS binary](doc/kernel/REQ__bms-linux-native.md) composes the same chart
preparation and Runtime with a selected evdev node and ALSA endpoint. Rate, channels,
period and buffer are explicit; acquired key events drive actual judging. Native
status-derived output/host pairs feed bounded continuous transport correction with
Unknown accuracy. Input-loss barriers stop the session instead of guessing missing
input. Source compilation does not establish native playback or acoustic sync.

```sh
cargo run -p beatkernel-bms-runtime --bin linux_bms -- --help
```

The [macOS BMS binary](doc/kernel/REQ__bms-macos-native.md) uses an explicit
CoreAudio device and IORegistry input entry with the same preparation and judge.
[CoreAudio presentation pairs](doc/platform/REQ__coreaudio-presentation.md) retain
the callback's native mach/output-frame association and explicit host mapping.
Native playback, permissions and physical synchronization remain unverified.

```sh
cargo run -p beatkernel-bms-runtime --bin macos_bms -- --help
```
