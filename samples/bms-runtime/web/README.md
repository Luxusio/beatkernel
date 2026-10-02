# Browser chart preview

The optional `browser` feature belongs to the existing `beatkernel-bms-runtime`
application crate. This host imports user-selected files, prepares chart/audio/
image data through the common Rust algorithms, and displays compiled notes and
static BGA at an explicit song position. It does not yet play audio, acquire
gameplay input, judge notes, capture/replay a session or connect multiplayer.

The DOM owns file selection, controls and layout. A dedicated module Worker owns
the imported bytes, preparation and wgpu rendering on a transferred
OffscreenCanvas. The host uses plain JavaScript without React or external
scripts. Existing retained note instances and BGA texture caching are reused.

## Build and serve

Run the following from the repository root when browser execution is scheduled.
These build, binding-generation and server commands have not been executed as
part of the current source-only development phase.

Use Rust 1.98.1 or newer with the WASM target installed:

```sh
rustup target add wasm32-unknown-unknown
cargo build -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser --release --locked
```

Generate the web bindings with `wasm-bindgen-cli` **0.2.129**, matching the pinned
Rust dependency:

```sh
cargo install wasm-bindgen-cli --version 0.2.129 --locked
wasm-bindgen --target web --out-dir samples/bms-runtime/web/pkg target/wasm32-unknown-unknown/release/beatkernel_bms_runtime.wasm
```

The generated `pkg/` directory is a local build artifact and is not committed.
The static host loads its generated JavaScript and WASM from that directory.
Serve the directory over localhost, for example:

```sh
python3 -m http.server 8080 --bind 127.0.0.1 --directory samples/bms-runtime/web
```

Open `http://127.0.0.1:8080/`. Use HTTPS when hosting elsewhere. The host requires
a secure context, module Workers, transferable OffscreenCanvas and WebGPU in a
dedicated Worker. Opening the page through `file://` is unsupported. The pinned
wgpu configuration provides WebGPU; this host has no WebGL fallback. Unsupported
capabilities and preparation failures are reported explicitly.

## Select and preview

1. Select a chart folder, including its audio and image resources. Folder
   selection preserves relative paths. The flat-files option is useful for
   charts whose resource names match the selected filenames; it cannot
   reconstruct missing subdirectories.
2. Choose a chart from the imported `.bms`, `.bme`, `.bml` or `.pms` list. Set
   the preparation sample rate and unsigned 64-bit random seed, then prepare
   the chart. The mix rate starts at 48,000 Hz. Individual assets retain their
   original sample rates, and mixed source rates use the existing kernel policy.
   Preparation adds no pre-resampling pass. This mix format is not an actual
   browser audio device selection. The supplied host
   prepares stereo output and permits the common mono-to-stereo channel policy.
3. Enter a nonnegative original-song time in decimal seconds, with up to nine
   fractional digits, and request a preview. For example, `12.345678901` selects
   exactly 12,345,678,901 nanoseconds. The boundary uses signed 64-bit nanoseconds
   through BigInt and reserves room for the fixed two-second note lookahead.

The displayed position changes only when requested. Animation callbacks schedule
redraws; their timestamps do not advance the song. No physical keyboard press or
miss feedback is fabricated for this preview. BGA selections, opacity and note
positions come from the actual prepared chart.

## Resource limits and lifecycle

The supplied host admits at most 32,768 selected files, 64 MiB per encoded file,
256 MiB total encoded bytes and 4,096 UTF-8 bytes per normalized relative path.
Metadata is checked before sequential file reads, and Rust checks admission
again. PCM preparation has a separate 64 MiB per-asset and 256 MiB aggregate
budget; decoded images have their own limits. These are not an overall browser
memory cap: binding copies, decoder scratch, prepared data, GPU resources and
overlapping old/new imports consume additional memory.

Resources resolve relative to the chart's selected parent directory. Unsafe
paths, normalized duplicates and file/directory collisions are rejected. Charts
cannot request unselected files or turn resource names into network URLs.

Imports and asynchronous initialization carry generation identities. A stale
result cannot replace a newer selection, and Worker mutations are serialized.
One import pump admits one active read/candidate and only the latest queued
selection. A superseded file read settles before that candidate is freed and
the queued import starts. Relative paths travel as explicit message fields.
The Worker stages a completed import until the DOM accepts its current catalog
generation. Ignoring an old result or failing the next import therefore keeps
the same accepted library on both sides.
Rendering is event driven with bounded surface retries. Page teardown terminates
the Worker and releases presentation callbacks and layout observers; restoring
the page creates a fresh canvas owner and requires selecting files again.

See the [browser contract](../../../doc/kernel/REQ__bms-browser.md) for the full
boundary. Portable fixtures are authored for the shared source/preparation path.
The deferred JavaScript regressions use Node's built-in test runner and VM module
mocks of WASM ownership. They cover the actual host helpers and Worker without
requiring a GPU or generated bindings. Run later from the repository root:

```sh
node --experimental-vm-modules --test samples/bms-runtime/web/host_model.test.mjs samples/bms-runtime/web/worker.test.mjs
```

This command has not been executed. It does not replace real DOM/Worker/WebGPU
acceptance. Host, Windows GNU, macOS, headless, WASM graphics and WASM browser
source configurations compiled with Rust 1.98.1. Tests, binding
generation, browser/Worker/GPU execution, audio/timing acceptance and formal
review/QA remain deferred; the full player task stays open.
