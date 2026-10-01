# BMS desktop player

The native Goal is to develop a BMS player. Keep all application features in
the existing `beatkernel-bms-runtime` crate, with internal modules, and retain
the core/platform/BMS adapter boundaries. Graphical presentation must use
actual native gameplay, not autoplay or a separate simulation passed off as play.
BeatKernel is intended as a Rust cross-platform rhythm-game engine, built from
the low-latency runtime kernel. This app is its first game composition. Keep generic geometry/rendering components distinct from BMS display
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

Native play defaults to the full song on Windows, Linux and macOS. An optional
`--seconds 1..3600` imposes the existing wall-clock diagnostic cutoff after
startup; a cutoff or cancellation still describes the actual captured prefix.
Normal completion requires all builtin BMS interactions to be terminal after
their inclusive late deadlines (with input offset applied once), all BGM cues
admitted and retired, and a later nonempty mixer block with no active voices or
pending commands. The completion component establishes a frame barrier after
the last admission, then waits for native output-domain presentation to reach
the first subsequent idle block's end. Newer buffered silence does not move
that finish line. The native compositions drain the entire bounded queue
capacity per render; that property is a prerequisite of this barrier. Native
presentation telemetry remains an estimate, not proof of acoustic delivery.
Neither wall time nor the last note timestamp substitutes for output progress.
Cancellation and timing/native errors still use the existing owner cleanup.

Preparation uses checked wide arithmetic for chart deadlines and source PCM
frame/rate durations. Windows initial calibration derives a finite extent from
the chart and PCM tails when no cutoff is supplied, preserving preroll and its
existing extrapolation margin. Long charts are not limited to one hour; an
unrepresentable timestamp/calibration extent fails explicitly. PCM preparation
budgets remain independent limits. `completion.rs` is a portable game-owner
component; it neither reads devices nor executes on audio callbacks.

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
Native settings are composed from a bounded UTF-8 line editor, text-field
presentation and a draft of the existing platform options. Settings open only
when no game owner exists. Apply uses the same pure native option parser as
playback, then changes the next session's arguments; Back/Escape discards the
draft. Missing required values or incompatible backend options remain explicit
errors. This parser validation does not certify device availability. Empty
fields omit the option, allowing app defaults before strict native preparation.
Device IDs, rates, channel layouts, buffer/period requests, binding rows and
timing options remain advanced overrides; omitted device IDs use the solo
automatic preparation policy below. No in-session device substitution occurs.
Bound drafts to 128 fields, 4096 UTF-8 bytes per value and 64 KiB total value
bytes. Keep values intact as flag/value pairs rather than parsing shell text.
Cursor movement and deletion honor UTF-8 scalar boundaries; reject controls and
newlines. Current glyph fallback still applies to non-ASCII text. Clipboard,
IME composition, multilingual shaping and enumerated keyboard device selectors
remain future work. Native audio output metadata selection is described below. Native profile persistence is described below. Editing never acquires devices or
changes native input/audio owners.
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

Audio Devices in Settings queries the configured backend on the settings
operation worker, then shows exact identities and native metadata. Selecting
a row updates only the device field in the draft; Apply remains separate.
Disabled/absent WASAPI endpoints cannot be chosen, and ASIO discovery requires
the explicit registry view. Refresh repeats discovery; Back leaves values
unchanged. Neither default-role metadata nor the first row becomes an automatic
device choice. Availability and format/buffer support remain native preparation
checks. ALSA entries are output/duplex PCM hints, including configured plugins,
not a proof of playable formats. CoreAudio metadata describes current settings.
The portable catalog admits at most 1024 rows, 4096 UTF-8 bytes per ID/name/detail
and 4 MiB in total; capacity errors reject the catalog instead of silently
showing a partial list. IDs remain exact; only display controls are flattened.
Existing Windows/macOS discovery may allocate the native list before app-level
admission. Device queries do not open playback streams or instantiate ASIO
drivers. They are never executed on input/audio callbacks or during a game.

Native settings profiles are explicit user-selected files. `--profile PATH`
loads a host-tagged versioned UTF-8 profile before opening the window; explicit
native CLI values replace the matching profile flag group, including all
repeated bindings/opponents for that flag. Profiles contain native options
only; chart/library selection and GPU/UI presentation settings stay separate.
Load/Save in Settings use an editable path. Load replaces the draft, Save
persists the syntax-validated draft, and Apply separately changes the next
session. No profile is written automatically. Errors preserve the previous
draft or saved file. GUI profile I/O runs on an operation worker while the main
thread keeps drawing; pending operations fence settings/game mutations and
must drain before exit. File bytes are bounded, not filesystem wall time.

The profile codec preserves UTF-8 values and repeat ordering as tab-separated
records, bounded to 72 KiB encoded data plus existing draft limits. Unknown
version/host/options, malformed records and nonregular/symlink leaf files fail
explicitly. Save refuses existing malformed, foreign or wrong-host profiles,
writes and syncs a uniquely owned sibling file, then renames it into place for
replacement. New files are published with a create-only hard link, preventing
replacement of a concurrently created target. Filesystems without hard-link
support fail new-file publication explicitly. Pre-publication failures retain
the old target; temporary cleanup is limited to the owned file and is best
effort on filesystem errors. This uses the filesystem's rename/link semantics; it does
not promise interprocess locking, directory crash durability or cancellation
of an in-flight OS file operation. Native resource availability remains checked
by actual game preparation.

`player (--library DIR | --chart PATH) NATIVE_OPTIONS` selects the graphical
mode; no arguments open the current directory catalog. Native configuration
can be supplied as flag/value pairs or edited through F2/the Settings button.
The settings screen shows ten rows per page; Up/Down or Tab select a field,
Left/Right/Home/End move its caret, Backspace/Delete edit, Enter/Apply validates,
and Escape/Back discards. Add Binding creates another explicit lane/key row.
Existing repeatable opponent rows are preserved. `play --help` describes the
host's device/rate/buffer/binding controls. Up/Down select, Enter starts or returns to
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
Graphical native settings and audio output metadata selectors now have source
integration; multi-player assignment UI, persistence of GPU/UI presentation settings and expanded
transport/practice controls remain player work. Native profiles have source
integration with file-I/O and interruption acceptance still pending. Existing
casual multiplayer has independent local starts and unauthenticated progress;
this screen does not establish ranked online play.

Source checks on 2026-10-01 passed for the default graphical application on
Linux host, Windows GNU and macOS x86_64, the headless application, and the
`graphics` library on wasm32-unknown-unknown. Scoped formatting also passed.
These compile fixture bodies without running them and do not link or execute
native or browser graphics/audio/input. WASM has existing unused native cadence
warnings; macOS's transitive block 0.1.6 has a Rust future-incompatibility warning.

## Known ceiling

- Settings validation checks syntax and cross-option constraints, not resource
  availability — actual device/file checks remain in native preparation.
- HID usage bindings still require typed values; clipboard
  and IME composition remain absent — extend these while retaining
  bounded drafts and next-session-only application.
- Profile operations drain on close rather than being forcibly interrupted —
  assess filesystem stall behavior during deferred native acceptance.
- New-profile publication requires hard-link support; concurrent replacement
  writers need external coordination and directory crash durability is not
  promised — verify target filesystem behavior during deferred file acceptance.

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
- Full-song completion now has source integration; native presentation and
  full-queue admission behavior remain unexecuted — verify these boundaries,
  final keysound/BGM tails and long-chart cancellation during deferred native
  acceptance before claiming complete playback.
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

## Native keyboard metadata selection

For multiple local players, input assignment will use the same bounded catalog
and serialized settings worker as audio metadata selection. Solo play does not
prompt for input device selection. Optional advanced configuration changes only --keyboard-path on
Windows, --evdev on Linux or --keyboard-registry on macOS in the draft. No
query registers gameplay input, reads key events, grabs devices or changes
input clocks. Metadata does not certify acquisition access or future attachment
availability. Refresh clears selection; disabled rows remain visible.

Windows optionally selects one exact Raw Input interface path; omitting that
option retains the existing Any-keyboard behavior. Native preparation resolves
one attached keyboard to its session DeviceId. A selected-device removal fails
the session; another keyboard or reconnect never silently replaces it.
Linux candidates require read-only metadata access and keyboard key capability;
unavailable candidates are disabled. macOS uses IOKit service metadata without
an opened IOHID acquisition manager. All actual acquisition stays on its game
owner. Native discovery and gameplay acceptance remain deferred.

## Automatic solo preparation

The user corrected manual-only device setup: a single local player uses
automatic input and the system/default audio endpoint. Device assignment is
required only when multiple local players need disjoint input routes. Manual
audio selection remains an advanced override, never a mandatory first step.
The primary app resolves omitted native arguments on the game worker before
calling strict platform compositions. GUI syntax checks do not query devices.
Windows uses the active multimedia default WASAPI endpoint and shared mode,
with Any-keyboard input unless an advanced override is supplied. Linux uses
ALSA default and the first readable standard keyboard in numeric event order.
macOS uses the actual system default output and the first ordered keyboard
registry identity. Supplied identities/settings remain exact; acquisition
failure never silently substitutes a different device during play.

Conservative Linux app defaults are 48000 Hz, stereo, 256-frame period and
1024-frame buffer. macOS preserves current output rate/buffer and uses up to
two channels. Omitted channel treatment permits mono-to-stereo. ASIO has no
OS-default driver and remains an explicitly selected advanced backend. These
policies do not certify native support; actual preparation can fail explicitly.
Shared Runtime composition has source integration and native solo adoption.
Linux terminal composition now provides simultaneous evdev acquisition and
per-player judging/replay using the shared output. Player assignment UI,
Windows/macOS multi-input composition remain required implementation work.
Linux local-input mode now publishes independent members to the graphical player;
network competition with a local group still fails before device acquisition.

## Collection-based local display

Extend the existing latest-state bridge and view organisms with player-tagged
charts/results rather than another runtime/UI framework. Solo retains its
layout. Local groups use independent panels, at most four visible per page;
3/4 use the same grid and larger groups retain all state while changing page.
Page controls affect display only and work through results; cancellation and
worker drain remain session-wide. Linux native local sessions publish actual
member reports including committed failure prefixes before cleanup. Current
explicit local inputs are provided via CLI/profile until roster UI is added.
