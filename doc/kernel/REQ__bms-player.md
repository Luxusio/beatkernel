# BMS desktop player

The native Goal is to develop a BMS player. Keep all application features in
the existing `beatkernel-bms-runtime` crate, with internal modules, and retain
the core/platform/BMS adapter boundaries. Graphical presentation must use
actual native gameplay, not autoplay or a separate simulation passed off as play.
This app is the first game composition of the Rust cross-platform rhythm-game
engine. Keep generic geometry/rendering components distinct from BMS display
layouts and rules; developing them inside this app is not proof of a completed
general engine UI API. Promote proven reusable abstractions after concrete use.

## Presentation and ownership

The main thread owns the desktop window, navigation and lane rendering. A game
thread owns native acquisition and Runtime/JudgeEngine. Audio remains on its
native output worker/callback and selected multiplayer on its socket worker.
The game publishes latest snapshots without waiting for a UI-held lock. Snapshot
publication never runs on audio callbacks. Native input keeps its clock and
provenance; UI key events navigate menus and never timestamp gameplay.

The game publishes its actual prepared chart and RuntimeReport song time/results
for lane/hold drawing and hit/miss/combo feedback. The UI does not invent an
independent song origin or judge. Windows GUI mode hides the acquisition window
and keeps background Raw Input registration. Focus loss or Escape cancels the
game instead of allowing background gameplay keystrokes; native cleanup must
finish before completion/results or another session starts.

## Selection and startup

The graphical mode accepts an explicit chart or an explicit library directory.
Library scans have finite directory/file/depth/byte limits, avoid symlink
traversal and retain diagnostics for invalid or capped entries. Selection does
not load all sound banks. Enter starts the selected chart with caller-supplied
native audio/device/buffer/binding options. Missing/incompatible configuration
must be shown as an error; do not silently substitute a device, mode or keys.
The app must keep selection, loading, gameplay, cancellation, result and failure
states visible and support another attempt after owner cleanup.

The user approved `winit` window/event ownership and `wgpu` GPU rendering,
prioritizing native performance with future WASM reuse. Keep bounded reusable
geometry/instance buffers;
draw notes, holds and text on the GPU rather than uploading a CPU framebuffer.
The `desktop` feature is enabled by default; headless modes remain buildable
with `--no-default-features`. A separate `graphics` feature exposes the reusable
renderer without native window/audio ownership. Kernel/platform RT code does
not depend on either UI library.

The UI is a passive View of game-owned snapshots with explicit start/cancel
commands. MVP terminology does not move judging or audio into the Presenter.
The user requires incremental construction in the style of Atomic Design:
build geometry/color/text primitives first, compose note/hold/counter elements,
then playfield/scoreboard components and screens. Each layer consumes explicit
state and emits geometry; it does not acquire devices or run judging. Introduce
abstractions only after their constituent components exist and there is concrete
reuse. Keep these as modules in the one app crate; avoid artificial framework
layers and a monolithic screen containing rendering, judging and I/O.
The current reusable component tree is `scene.rs` for bounded geometry,
`ui/atoms.rs` for rectangle/text, `ui/molecules.rs` for note/hold/counter,
and `ui/organisms.rs` for playfield/scoreboard. `graphics.rs` submits the scene;
`desktop.rs` composes screens and owns native window/session commands. The
graphics feature exports these components for native and WASM library users.
Resize, zero-size surfaces, focus loss and close must preserve owner cleanup.
Menu interaction is built bottom-up from logical hit rectangles and a
press/release/cancel state, then a button molecule and screen composition.
A click activates only when press and release hit the same enabled control;
focus loss, resize, suspension and close cancel an armed gesture. Physical
pointer positions map through the same logical-to-physical stretch as rendering;
non-finite/outside positions and zero-sized windows cannot activate controls.
Clicking a catalog row selects it. Start, Cancel, Return and Exit buttons use
the existing session commands, with cleanup still required before another run.
Pointer events are menu commands only and never produce gameplay timestamps.
The next primitives are validated RGBA8 texture resources and clipped sprite
quads. Solid, glyph and custom texture quads preserve painter order through
contiguous draw batches; they must not be reordered globally by texture.
Custom texture resources have typed unique IDs, explicit removal and finite
count/byte budgets; invalid or stale IDs fail before drawing. Built-in white
and ASCII glyph atlas resources are immutable and cannot be removed. Text
uses atlas glyph quads with the existing metrics, replacing one rectangle per
lit glyph pixel. This is a bitmap font foundation; multilingual fonts and
shaping are still pending. Texture preparation/upload never runs on native
input or audio callbacks.
The renderer uses nearest sampling, straight-alpha blending and RGBA8 UNORM
resources. Upload accepts validated raw pixels; image-file decoding is a
separate preparation step. Normalized sprite UV rectangles crop with viewport
clipping. The 64-resource/64-MiB admission budget includes built-ins and counts
RGBA source bytes; custom removal releases the resource's budget.
Backend and presentation options must reject unavailable explicit selections;
queue-depth settings are hints, not measured latency guarantees.

WASM reuse covers renderer/shaders and portable game logic. It does not imply a
working browser player: browser canvas/startup, input clocks, Web Audio /
AudioWorklet, file selection and networking need browser adapters. Native
WASAPI/ASIO/ALSA/CoreAudio device control is not promised in a browser.

Project-authored code stays MIT. wgpu/bytemuck/pollster use their MIT options;
winit is Apache-2.0 and its license must be retained. See the application's
[third-party notices](../../samples/bms-runtime/THIRD_PARTY_NOTICES.md).
The renderer pins wgpu 27.0.1: source compilation found a Windows DX12 binding
type mismatch in the published wgpu 30.0.1 dependency graph. The compatible
published version preserves all selected native backends without a local
registry patch or disabling DX12. Upgrade after native/WASM source checks show
the newer dependency graph is compatible.

## Controls and rendering behavior

`player (--library DIR | --chart PATH) NATIVE_OPTIONS` selects the graphical
mode; no arguments open the current directory catalog. Native configuration
is still supplied as flag/value pairs, using `play --help` for the host's
device/rate/buffer/binding controls. Up/Down select, Enter starts or returns to
selection after cleanup, Escape cancels, and focus loss cancels active play.

`--gpu-backend auto|vulkan|dx12|metal|gl` selects GPU discovery. Explicit
unavailable backends return errors. `--present fifo|immediate|mailbox` defaults
to FIFO and rejects unsupported explicit modes, rather than silently changing
the request. `--ui-fps 30..240` limits redraw scheduling (default 120);
`--ui-lookahead-ms 100..10000` controls note display (default 2000). Presentation
may further limit actual frame rate. Surface resize scales the fixed logical
960x720 scene; zero-size frames are skipped. Lost surfaces are recreated;
outdated surfaces reconfigure, timeout/occlusion skip a frame, fatal GPU errors
request cancellation and wait for game cleanup before exit.

## Acceptance and scope

The user's verification deferral remains in force: fixture authoring and source
compilation may proceed; actual GUI rendering/focus/close/restart/input/audio,
replay/network execution and independent reviews/QA remain required later.
This presentation increment does not by itself prove the full player complete.
Graphical native-device settings, reusable persistent configuration and expanded
transport/practice controls must be assessed as remaining player work. Existing
casual multiplayer has independent local starts and unauthenticated progress;
this screen does not establish ranked online play.

Source checks on 2026-10-01 passed for the default graphical application on
Linux host, Windows GNU and macOS x86_64, the headless application, and the
`graphics` library on wasm32-unknown-unknown. Scoped formatting also passed.
These compile fixture bodies without running them and do not link or execute
native or browser graphics/audio/input. WASM has existing unused native cadence
warnings; macOS's transitive block 0.1.6 has a Rust future-incompatibility warning.

## Known ceiling

- Catalog reads are capped per file at 8 MiB; aggregate accounting uses
  advertised file sizes — tighten aggregate accounting if concurrent file
  replacement must count against that budget.
- Catalog scan happens before window creation; loading/calibration cancellation
  waits for the current preparation step — add asynchronous catalog/progressive
  preparation if startup responsiveness requires interrupting these steps.
- At most 2048 visible notes and a finite rectangle batch are rendered per
  frame (65,536 rectangles); exceeding geometry capacity reports an error — revise admission when
  denser layouts require more geometry.
- In-scene text currently uses ASCII glyphs; Unicode title/artist survive in
  native window titles — add a font atlas when multilingual in-scene text is
  implemented.
- Native playback remains bounded by the existing `--seconds` session control;
  results describe the actual played prefix, not guaranteed full-song completion
  — implement chart/audio-tail completion before calling this a complete player.
- WASM renderer compilation is source evidence only; browser startup, adapters
  and actual browser rendering/audio/input remain future work.
- Logical geometry stretches to the physical surface — introduce letterboxing
  when aspect-preserving presentation is required.
- Shader validation and native/browser rendering remain unexecuted — verify
  when the user's execution deferral is lifted.
- Alternating textures create separate contiguous draw batches — use texture
  arrays only when observed draw-call cost justifies that design.
- Texture admission counts raw RGBA bytes, excluding driver allocation overhead
  — add backend memory accounting if a hard VRAM budget is required.
