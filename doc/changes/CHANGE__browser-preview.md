# Shared selected-file preparation and browser preview

The existing BMS application crate now has an optional `browser` feature with
WASM bindings and a static JavaScript host. A dedicated module Worker prepares
user-selected resources and renders the actual compiled chart and static BGA on
a transferred OffscreenCanvas. The DOM owns selection, controls and layout.
There is no additional application crate or React dependency.

This is a browser preview component. Browser audio output and presentation
evidence, gameplay input/clock mapping, actual judging, capture/replay and
network transport remain required follow-on work. Preview positions are explicit
original-song nanoseconds; animation callbacks do not stand in for an audio clock.

## Shared preparation

`AssetSource` separates resource resolution and bounded reads from the chart,
audio and image algorithms. Native preparation retains its filesystem adapter.
The memory adapter holds selected original bytes and resolves references relative
to the selected chart's parent. The common strict text decoder, seeded parser,
compiler, audio decoders, image crops/layers and unavailable-image policy remain
authoritative. Replay identity validation precedes asset access where requested.

Memory paths preserve Unicode spelling and case while normalizing separators and
dot components. Unsafe paths, duplicate normalized keys and file/directory
collisions reject admission. Audio lookup shares the finite literal-first
extension policy; images retain exact lookup. Resource names do not become
network URLs or native filesystem requests.

The host checks metadata before sequential reads. Default retained encoded limits
are 32,768 files, 64 MiB per file, 256 MiB total and 4,096 UTF-8 bytes per path.
Rust independently checks the same boundary. The supplied PCM preparation limits
are 64 MiB per asset and 256 MiB aggregate. Assets retain their original sample
rates; mixed rates remain supported by the existing SampleBank/Mixer policy.
Preparation adds no pre-resampling pass. Decoder scratch, binding copies,
decoded images, GPU resources and overlap during replacement remain additional
memory costs.

## Canvas and ownership

`BrowserCanvas` owns the OffscreenCanvas, wgpu instance, existing async renderer,
persistent Scene and BGA texture cache. It delegates extent validation before
canvas configuration, retains the last successful size, suspends drawing at zero
extent and retains the canvas for surface replacement. Shared visible-note
queries, cached GPU note instances, BGA selections and opacity handle preview
drawing. Neutral judge state avoids inventing Poor-background activation.

The Worker owns selected bytes and prepared PCM/images/chart indexes, so these
remain in one WASM instance. Bindings expose library admission, preparation,
canvas creation, explicit seek and redraw status. Import generations reject
late results. One import pump holds one active candidate/read and the latest
queued request; a superseded unabortable read settles before replacement begins.
Relative file paths travel in explicit message fields. Mutable WASM calls are
serialized, and replacement metadata follows
the prepared selection. DOM labels use text nodes. Teardown terminates the Worker
and cancels callbacks/observers; restoration starts a fresh owner.
Catalog results are proposals until the main thread acknowledges the current
generation. A discarded late proposal never replaces the accepted library, so
failure of a newer import preserves the same library on both sides.

The host requires a secure context and browser support for module Workers,
transferred OffscreenCanvas and Worker WebGPU. The pinned configuration supplies
WebGPU without an implicit WebGL fallback. Folder selection preserves relative
paths; flat-file selection can resolve only names actually supplied.

Build and usage commands are documented in the
[web host README](../../samples/bms-runtime/web/README.md). They use the optional
`browser` feature and `wasm-bindgen-cli` 0.2.129. Generated `web/pkg/` bindings are
local build artifacts. The durable scope and limits are in the
[browser requirements](../kernel/REQ__bms-browser.md).

## Verification status

Twelve Rust fixture groups are authored for the actual memory source and common
preparation paths, including BMS/audio/image inputs, path admission and replay
identity ordering. Twelve JavaScript fixture groups cover the actual host
metadata/time helpers and the Worker module with mocked WASM ownership, including
stale imports, acknowledged library replacement, failed replacement and bounded
redraw retries. Fixture assertions
have not been executed; Rust fixtures have been compiled, JS fixtures have not
been executed or syntax-checked.

Six locked Rust 1.98.1 source compile configurations completed with exit 0:

| Configuration | Scope | Status |
| --- | --- | --- |
| Host | Workspace, all targets | Compiled |
| Windows GNU | BMS runtime, all targets | Compiled |
| macOS | BMS runtime, all targets | Compiled |
| Headless | BMS runtime, all targets, no default features | Compiled |
| WASM graphics | BMS runtime library, graphics without default features | Compiled |
| WASM browser | BMS runtime library, browser without default features | Compiled |

The initial offline browser check admitted four already pinned direct dependencies
to the app's lockfile entry; no package version changed. Existing WASM unused
native-cadence warnings and the macOS `block` future-compatibility warning remain.
The SDK/MSVC ASIO branch is outside these ordinary GNU checks. A later fixture
literal correction uses the actual `#VOLWAV` directive; its focused host
test-target compile check also completed with exit 0. It changes no production
or target-specific code.

Source checks do not establish WASM linking, generated bindings, JavaScript
execution, Worker ordering, browser file access or GPU output. Build/bindgen/server
commands, tests, browser/native/audio/device execution, performance measurements
and formal review/QA remain deferred under the user's sequencing. The full BMS
player Goal and Harness task remain open.
