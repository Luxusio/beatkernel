# BMS desktop player

Records displays the selected record's own/other opponent occurrence counts.
Remove Own and Remove Other clear exactly the first matching kind and literal
path in the parent settings draft; duplicates remain individually removable.
No file is deleted or reopened. Removal needs selected membership, not a preview,
and pending/hidden/closing Records cannot mutate the draft. Empty/missing/invalid
targets preserve unrelated settings and row identities; invalid paths reject
atomically. Cleared rows remain reusable by Add. Parent selected editor refreshes
after removal so the path cannot be restored accidentally; Apply stays separate
from draft changes and accepted native session options remain pinned.
Retained membership/gates update without repainting catalog rows. Frame membership
cannot exceed the actual bounded draft total and requires a selected path.
An overfull manually edited draft remains visible and individually removable;
Add is disabled at eight opponents. Native Apply still enforces accepted limits.
Prepare exact-kind/duplicate/capacity/editor/gate/route/pending regression fixtures
for later execution; source compilation is not native GUI acceptance.

Windows Settings → Players uses exact keyboard catalog assignments for 2..64
stable local members. Solo remains automatic. Each member publishes its actual
chart/time/score/competition prefix through the existing graphical bridge, while
sharing one native audio output and song transport. Backend and buffer controls
remain the existing WASAPI shared/exclusive or optional ASIO settings. Keyboard
disconnect cancels the group without substitution. Graphical/native acceptance
is still deferred. macOS uses the same roster with exact IORegistry keyboard
identities and shared CoreAudio output.

## Presentation settings and combined profiles

Settings → Display edits GPU backend, presentation mode, UI FPS (30..240)
and note lookahead (100..10000ms). Done validates into the settings draft;
Back discards display edits. Apply validates native and display configuration
before committing between sessions. FPS/lookahead and supported presentation
mode update on the UI owner; changing GPU backend requires saving the profile
and restarting with that profile (or an explicit backend CLI argument).
The display never changes acquisition timestamps, judging or song transport.

Player profile version 2 stores native options and all four display fields in
the existing bounded same-host file format. Load replaces the complete draft;
Save stores the draft, and Apply stays separate. Version 1 loads with default
display values. Explicit display CLI arguments override stored display fields;
omitted arguments retain the profile. Limits are 128 native + four display
records and 72KiB encoded bytes. Unknown/duplicate/missing display records,
wrong host, invalid ranges and malformed records reject the whole replacement.
Native-only version 1 APIs remain strict and refuse combined-profile replacement.
Both formats use the existing bounded regular-file, synced temporary and guarded
publication protocol; file/GPU execution acceptance remains deferred.

Competition views retain per-player actual saved-record prefixes (up to eight)
and one peer-reported prefix with its independent song time. Network lifecycle
is visible during play and results; loss of connection preserves the last prefix
while local play continues. The latest-state bridge retains these through native
cleanup. Compact comparison rows reuse existing drawing components; their labels
use bounded file basenames, never full replay paths. Rendering acceptance is
still deferred.

Local play keeps normal lane space by default. A view-only Comparisons button
or C key shows/hides all retained comparison rows for the visible local panels;
the toggle never changes gameplay state or provides a gameplay timestamp. Solo
comparisons use the sidebar. Showing many rows explicitly trades local lane
space for comparison detail; the default local view preserves its lane geometry.

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
commands. MVVM presentation state does not own judging, audio or input timing.
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
newlines. Native IME search/settings input is owned by the active screen and
selected field, including profile paths. Bounded preedit previews never modify
the draft, search projection or persisted arguments; commits use the existing
transactional editor. Switching fields/screens or losing active UI admission
clears composition and native enable acknowledgement. Active composition
suppresses ordinary keyboard text and shortcuts. The native event API carries
no composition generation ID; ordered enable/disable and current-target guards
are used without an absolute stale-event guarantee. Current glyph fallback,
caret-only previews and OS candidate positioning remain; multilingual shaping,
selection underlines and clipboard are future work. Prepared regression
fixtures await later execution and do not certify native IME behavior.
Native audio output metadata selection is described below. Native profile persistence is described below. Editing never acquires devices or
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
in version 1. Player profile version 2 also stores display settings;
chart/library selection stays separate.
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
integration, including Linux, Windows and macOS local-player assignment.
Expanded transport/practice controls remain player work. Native and display
profiles have source
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

- Comparison views retain eight records and one peer; local panels show this
  detail only when toggled. A four-player panel with eight records has a 72px
  lane region while comparisons are open (44px if a future native group+peer
  mode is enabled). Default local lanes retain normal geometry; richer detailed
  comparisons need a larger view. Rendering acceptance remains deferred.

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
- At most 2048 visible notes per displayed playfield and a finite UI rectangle
  batch are rendered per frame (65,536 UI rectangles). Note overlap and geometry
  capacity overflow report an error — revise admission for denser layouts.
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

For Linux multiple local players, input assignment uses the same bounded catalog
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
per-player judging/replay using the shared output and graphical assignment UI.
Windows uses simultaneous Raw Input acquisition with the same group; macOS
uses IOHID acquisition and one CoreAudio output.
Linux local-input mode now publishes independent members to the graphical player;
network competition with a local group still fails before device acquisition.

## Collection-based local display

Extend the existing latest-state bridge and view organisms with player-tagged
charts/results rather than another runtime/UI framework. Solo retains its
layout. Local groups use independent panels, at most four visible per page;
3/4 use the same grid and larger groups retain all state while changing page.
Page controls affect display only and work through results; cancellation and
worker drain remain session-wide. Linux native local sessions publish actual
member reports including committed failure prefixes before cleanup. Linux local inputs can be assigned from Settings Players or imported via
CLI/profile.

## Player setup from the graphical settings screen

Players opens a draft roster; increase/decrease count and select a member before
Assign keyboard. The existing worker queries a typed catalog and Use assigns
its exact native identity to that member. Solo hides the device chooser. Done
validates every assignment before updating the settings draft; Apply separately
accepts next-session options, while Back discards roster changes. Modal/pending
operations fence underlying text/menu editing. Stable player IDs survive group
resize/profile import and reach the real native runtime/replay filenames.
Current multi-input source preparation supports Linux, Windows and macOS. Developer native/terminal commands do not replace graphical play.

## Graphical fresh-session retry

F5 or Retry restarts the accepted session from its configured original-song
position (default beginning), retaining its invocation. Prepare and validate the next invocation before
cancelling; invalid requests leave ongoing play intact. Wait for the old game
owner to finish native cleanup and join before creating a fresh publisher and
native game owner. Old cleanup failure prevents automatic retry. Focus loss,
suspend, close or explicit cancellation clears queued retries. The UI supplies
no gameplay timestamps. Solo and local cohorts follow the same lifecycle.

Recorded retry filenames derive from the original configured stem using
.retry<N>.bkr, with a checked increasing ordinal. Existing create-new semantics
still reject collisions. Retry keeps the configured practice start. Live scrubbing or restoration of
historical play state remains separate work. Acoustic restart accuracy still
requires native acceptance under the section-restart contract.

## Graphical saved-record selection

Settings exposes Records for the selected chart. Its directory editor starts at
the chart parent and allows explicit locations. Scan lists direct regular .bkr
files (ASCII case-insensitive extension), bounded to 4096 inspected entries and
256 results, with visible truncation. Relative chart names default to directory
".". Records can also be opened with F4 while in Settings; Tab switches between
directory and list, Enter scans/previews, and PageUp/PageDown changes pages.
The serialized metadata worker performs directory enumeration and selected-file
reconstruction, using the shared 64 MiB/one-million-operation replay limits.
The UI shows actual recorded prefix time, operations and hits/misses/combo.
It never labels a prefix as full-song completion. Chart/rules/runtime, profile
and practice start must match the current draft before Own/Other attachment.
Attachment adds one of at most eight ghost paths to the draft; Apply remains
separate. Clear removes draft ghost paths. Back closes the chooser and retains
attachments in the settings draft; settings Back discards the entire draft.
Pending work blocks navigation/editing/apply/start; directory or row changes
invalidate the preview, and failures preserve settings and show the error.
All keyboard/mouse actions use the existing graphical interaction components.
No gameplay timestamps, native devices or audio are accessed by this chooser.

Known ceiling: directory enumeration inspects at most 4096 direct entries and
returns at most 256 records, without recursive search — add incremental paging
when a single record directory routinely exceeds these limits.

## Graphical recorded playback

Records Watch starts the validated selected record with native music/keysound
output and the existing graphical lane/score view, marked REPLAY. It uses the
draft's output settings and the recording's actual profile/start. Output defaults
resolve on the replay owner without keyboard discovery or live input capture.
Recorded operations run incrementally through the same JudgeEngine, retaining
equal-time order and original timestamps; no timeout is invented after the
recorded prefix. Display progression uses reported native output presentation,
not UI time or a fabricated render cursor. Missing presentation stays unknown.
Omitted diagnostic duration waits for the recorded prefix, admitted audio,
idle mixer block and reported native presentation to drain. Escape/focus loss
or close cancels and joins native output before results. F5 repeats the pinned
record through the same cleanup lifecycle. Live settings remain unchanged by
Watch; no replay is captured and no live/network input affects its judge.
The window title identifies the recording, the header shows REPLAY, and joined
results are labeled RECORD PREFIX RESULTS. Watch is available after a valid
preview; W starts it only when the record list has focus. Pending metadata
operations disable it. Failed parser admission retains Records and its draft.
Reported presentation remains distinct from acoustic timing proof. Execution
acceptance is deferred.

SDK-enabled ASIO Watch uses actual rendered-block presentation observations
with an explicit multimedia-clock relation and caller-assessed timer, drift and
output-latency errors. A bounded queue retains future blocks until fresh QPC
reaches each observation's upper host interval, then advances the shared replay
visual/completion cursor using its actual output frame. No raw native sample
counter or prepared-frame count substitutes for audible presentation. Natural
recorded-prefix completion is admitted without diagnostic seconds; ASIO pause
remains unavailable. GUI Watch forwards the pinned clock assessments. Graphical
finite retries likewise defer ASIO capability to the actual native parser rather
than retaining an obsolete backend veto. Driver/MSVC and physical acceptance
remain unverified; portable composition fixtures are prepared for later use.
GUI Watch preserves ASIO exact frame buffers and derives omitted channels from
the explicit output routing. When rate is omitted, query the selected trusted
driver's actual integral rate before PCM/files, without creating a stream,
starting callbacks or changing its rate; standalone replay rate stays explicit.

Known ceiling: ASIO Watch retains at most 4096 distinct actual blocks awaiting
presentation. Capacity exhaustion fails explicitly without dropping evidence.
Missing observations freeze the last matured cursor and may delay natural
completion; cancellation remains available. Repeated actual blocks do not grow
the queue even when a refreshed anchor changes their assessed interval. No
interpolation or raw native position establishes a new presented cursor.
WASAPI, ALSA and CoreAudio keep their existing reported presentation paths.

## Configured fresh practice start

Nonzero practice sessions display PRACTICE in the graphical header.
The existing Settings editor exposes PRACTICE START (NS), stored in native
profiles as --start-ns. Empty or zero starts the full song. A nonnegative i64
original song position starts fresh practice through the song end on Windows,
Linux and macOS, for solo or local cohorts. F5 retries that pinned start.
The whole song transport retains original song times; the output is newly
calibrated with start minus preroll at output zero. Earlier note heads, including
crossing holds, are excluded rather than fabricate prior input. Automatic BGM
that still overlaps the position resumes from explicit original PCM ceiling
frames; old gameplay keysound voices are not reconstructed.

Recordings identify the filtered pristine section judge and store positive
original-song starts in v2 options. Standalone logical replay and recorded audio
output reconstruct that section automatically from the original chart/assets.
Ghosts require the same start; full-song and different-section identities fail.
Zero-start v1 recordings remain supported. Native finite practice loops and acknowledged
native pause are described below. Sample-exact loops, live scrubbing and historical
hold-state restoration remain player work. Acoustic restart acceptance is deferred.

## Screen lifecycle

The MVVM presentation layer and desktop navigator follow the typed navigation and enter/exit,
suspend/resume and owner-cleanup requirements in [screen lifecycle](REQ__screen-lifecycle.md).
Draft presence must not decide active-screen input or drawing.

The primary menu UI must retain its view tree and update dependent bindings on
state changes (Svelte-like semantics), independently of the playfield render
cadence. Full-menu reconstruction each gameplay frame is not the chosen model.
Toolkit selection and migration remain subject to the screen-lifecycle contract.


## Cached GPU playfield projection

The approved performance trial separates reactive menu presentation from note
rendering. Visible note instances are retained until membership, lane geometry
or a local time epoch changes. Every panel receives its own drift from actual
reported song time; UI/wall clocks never advance it. The GPU projects and clips
heads, tails and hold bodies, preserving the existing painter order. No note has
a reactive binding or per-note ViewModel.

Subtract integer timestamps with wide arithmetic before conversion. Rebase a
local epoch before drift exceeds one quarter of the visible time span; seek,
retry or reverse time invalidates it. Far hold endpoints can be saturated only
outside the clipping region plus the epoch drift margin. Twenty-hour and
week-long charts must therefore not lose precision through absolute f32 time.

The first path retains at most four displayed playfields and 2048 visible notes
per field. A denser actual overlap is an explicit presentation error, never a
silently shortened note list. All gameplay members keep independent judging.

Known ceiling: CPU selection and membership comparison still cost work per
frame; visible geometry and GPU fill remain proportional to displayed content.
The desktop Navigator is integrated by the scoped-panel implementation below;
Selection now uses retained reactive nodes as described below; other menus and
full widget toolkit migration remain unfinished.
Cargo compilation is source evidence only; shader execution, native rendering
and frame-time/upload benchmarks remain deferred by the user. This trial does
not establish that this architecture is fastest on any device.


## Reusable panel lifetime integration

Records, Display, Players, Devices and Settings draft data use typed PanelScope
ownership. The desktop dispatches from one ScreenNavigator route; retained
parent Options never independently select a visible screen or input recipient.
Actual back-stack instance IDs survive parent return; child exit cancels its
metadata permits and clears stale gestures/hits. Metadata results require the
active initiating instance and an uncancelled permit. Closing releases all UI
scopes while game/metadata owners drain separately before exit. Platform suspend
retains drafts and defers metadata presentation; resume restores focus from the
native window and admits completed session Results after join.

Known ceiling: cancellation is cooperative at metadata task boundaries and cannot
interrupt an in-progress filesystem/native metadata operation. Pending operations
fence ordinary navigation; application close cancels scopes and drains workers.
The current fixed route graph has maximum stack depth four; expanding navigation
requires updating its admission graph and bounded depth. Selection's retained
binding is integrated below; other menus and actual GUI/OS lifecycle acceptance
remain separate work.


## Retained Selection presentation

Selection creates its fixed nodes once per retained Navigator instance, using
floem_reactive 0.2.0 signals/memos/effects on the UI thread. Immutable catalog
labels and diagnostics are prepared once at startup. Selected-row, hovered
control, armed control, error and backend-pending states have independent
signals. Row/button memos suppress unchanged geometry; changing an error cannot
repaint chart rows. Page changes can affect all fifteen visible row slots.
Starting Play or Closing releases Selection's reactive scope; opening Settings
retains it, and Back restores it with the same screen identity.

Node geometry is retained in immutable packets and concatenated in painter
order only after dependency changes or scene restoration. An unchanged redraw
uses the existing composed scene and skips rectangle-instance uploads. Upload
identity includes both scene allocation identity and a checked epoch, avoiding
cross-scene or counter-wrap aliasing. Idle Selection waits for native window or
input events; gameplay still uses its configured cadence and actual-song-time
GPU playfield uniforms.

This introduces only the MIT standalone Floem reactive engine, not its released
wgpu22/forked-winit host. The existing wgpu27/winit0.30 host and native resource
owners remain. Settings, Display and Records use the retained bindings below.
Players and Devices use the retained setup panels below. A changed node
currently causes full composed rectangle
buffer upload, not a partial GPU update; concatenation and fill cost remain
proportional to visible geometry. GUI execution, dependency disposal behavior
and performance measurements remain user-deferred; source compilation alone
cannot establish those outcomes. See the
[screen-lifecycle contract](REQ__screen-lifecycle.md#retained-reactive-selection-screen).

Event-driven Selection retries drawing at the configured cadence after transient
surface acquisition timeout, outdated configuration or lost-surface recreation.
It enters idle Wait only after a presented frame (or a zero-size suspended
surface), so a recovered surface cannot wait indefinitely for unrelated input.


## Graphical practice-section draft

Settings exposes a dedicated Practice child (button or F6). Independent retained
start/end inputs accept nonnegative decimal seconds, M:SS or H:MM:SS with 1..9
fractional digits. Empty start means zero; empty end means through song end.
A configured end must be strictly later than start. Colon seconds stay below60,
minutes below60 in three-part form; hours/two-part minutes can span long songs.
Each input is bounded to64bytes and nonnegativei64 nanoseconds without rounding.
Tab/click chooses the active field; text/cursor edits target only that editor.
Full Song resets startzero/emptyend, Through End clears onlyend. Back/Escape
discards child edits. Done/Enter validates and atomically changes both parent
settingsfields, synchronizing the selected start/end raweditor. Invalid range,
precision/overflow or finalbytecapacity preserves parentdraft and childscope.
Old endpoint bytes are released within a candidate transaction before checking
new values, so replacing at the byte ceiling is judged by final configuration.
Apply/profileSave stayseparate; no native acquisition/clock/chartread in bindings.
Native parsers still own supported modes and applied configuration validation.

The view owns one retained tree per Navigator instance. Each endpoint/cursor
update repaints only its own node; focus updates its two affected fieldnodes,
error/button dependencies remainindependent, unchangedframes remainclean.
Scope disposal drops all child effects; transient surface restoration retains
geometry/hitorder. Native loops/pause use the existing owner contracts. Actual
GUI/native/acoustic execution and live scrubbing remain deferred or unfinished.

## Retained Settings presentation

Settings uses one stable reactive node tree per retained Navigator instance.
Ten visible row slots bind to individual field signals; only the selected slot
observes the active editor and cursor. Focus, profile path/editor, hint, page
count, message/error and button state are separate dependencies. Editing one
field or changing a message does not recreate unrelated rows or the tree.
Child panels retain their parent's Settings bindings and Back restores the same
instance; loading a profile or adding/reordering fields updates existing signals.

View updates borrow the bounded NativeSettings fields and compare old values
before cloning changed strings/editors. They do not clone the whole field vector
on pointer/window redraw. There are 128 bounded field signals and ten visible
slots; comparisons still cost up to the existing 64 KiB values per update.
Changed packets still require composition and full rectangle upload. No custom
signal engine, batch scheduler, per-note ViewModel or native I/O is introduced.

Pending metadata suppresses controls and hit regions while the owner polls.
Metadata completion queues redraw before returning to event-driven idle Wait,
including errors and Save completion. Clearing hit regions invalidates retained
composition even if every signal compares equal. Button hover uses the shared
layout so it remains valid after hit-cache invalidation. The obsolete immediate
Settings renderer is removed; Display and Records use retained children.
Players and Devices use retained setup panels. Actual GUI, performance and
dependency-disposal execution
remain user-deferred; source compilation is not acceptance evidence.


## Retained Display presentation

Display creates stable editor, control and error nodes per Navigator instance,
using a reusable retained-node primitive for immutable geometry packets and
ordered composition. Each of its four editor values has an independent signal;
selection focus updates the old/new field, and errors never repaint fields.
Updates borrow editors and clone only changed state. Pending disables all hits.
Scope disposal remains the containing view's responsibility, independent of
native session ownership.

Done validates the existing PresentationSettings model before updating the
parent Settings draft. Back discards edits; failure retains both drafts. Apply
still changes the next accepted presentation settings, and GPU backend changes
still require profile save/restart. Idle Display uses the same event-driven
redraw, hit invalidation and surface retry policy as the other retained menus.
Source compilation cannot establish actual GPU capability, GUI behavior or
performance; execution remains user-deferred.


## Shared retained UI component base

Selection rows show a nonempty artist below its title while retaining the same
catalog identity, row hit bounds and projection-driven repaint. Empty artists
retain one-line layout. Bitmap mode uses a smaller artist line; supplied-font
mode uses two fixed-scale line origins within the existing row spacing.
Each selection title and artist clips to its own 15-pixel line band with
10-pixel horizontal row padding; a title without artist uses the full 30-pixel
row. Cropping adjusts texture UVs rather than stretching the glyph. Long text
and font overhang cannot paint into the next line, row or outside row padding.
The row's original hit bounds and retained repaint dependencies stay unchanged.
Known ceiling: overhanging glyphs can be visibly cut at a line boundary; ellipsis,
wrapping, shaping and fallback fonts remain future widget work.

Portable sprite clipping uses an immutable checked rectangle with positive
extents and representable endpoints, intersected with the scene viewport.
Clip scope is explicit per draw, with no mutable renderer/scissor stack that can
leak between sibling components. Original sprite/rect/glyph viewport behavior
remains available. Prepared-font text checks cached metrics/positions before
geometry writes and stops at the clip's right edge. Bitmap text uses the same
cropped-UV path, including partial final glyphs. Rejected clips/UVs preserve
geometry; the existing sticky scene capacity limit remains authoritative.

Desktop `--title-font PATH` selects a caller-provided font for catalog titles and artists.
Preparation reads at most 32 MiB, caches the first 1024 scalars of each title and artist
in a fixed 14-pixel, 1024-square atlas with 4096 cached-character capacity, and
fails explicitly before window/native startup on invalid or excessive data.
The immutable font cache is uploaded once per renderer; retained title rows
use actual advances, baseline offsets and normalized sprite UVs without raster
work on redraw. Full renderer recovery creates a new texture identity and
rebuilds the Selection view; surface-only recovery retains the resource.
The option stays outside native invocation/replay/profile arguments. Controls,
search fields and other screens keep their existing bitmap font for now.
Font fallback/shaping, broader widget integration and actual native visual
acceptance remain unfinished; source fixtures are prepared for later execution.

Portable font preparation accepts caller-provided TrueType/OpenType bytes and
a fixed pixel scale, preparing glyphs into a bounded RGBA atlas for the existing
GPU texture path. Font bytes, dimensions, scale and cached glyph count have
explicit limits. Cache hits reuse metrics/UVs; rejected glyphs preserve prior
pixels, allocations and cache entries. Baseline bounds and advances are retained,
whitespace needs no pixels, and missing glyphs are explicitly reported. Non-whitespace
characters resolving to the same font glyph identity reuse its metrics and atlas
placement, including missing glyph zero. The character cache limit remains unchanged;
whitespace never acquires geometry from an alias, and failed preparation preserves
both identity and character caches. Raster
and packing run outside input/audio callbacks. No font is bundled; native font
discovery, broader widget integration, fallback/shaping and actual multilingual
GUI acceptance remain unfinished. Self-authored font fixtures are prepared for
later execution; compilation alone does not establish visual behavior.

All seven migrated views (Selection, Practice, Settings, Display, Records,
Players and Devices) use the same RetainedNodes geometry storage/binding/composition implementation. Individual
views retain their signal dependencies, data model and explicit scope disposal;
sharing packets does not introduce shared navigation state or native owners.
Initial geometry errors are checked before a view is returned. Composition
errors preserve dirty state and stop partial presentation. Existing selectivity,
retained instance, pending-hit and Back/disposal behavior remain required.

This removes the earlier per-view packet helper duplication. All implemented
menu routes now use retained UI components. There is no full widget host,
custom signal engine or batching scheduler, and no measured performance claim.

## Retained Records UI

The Records screen retains geometry nodes per screen instance. Directory edits,
selection and paging update the affected visible rows and controls; preview,
opponent count, errors and messages update their own nodes. Equal frames do not
repaint nodes. Scan/Preview/Watch/Add still use the existing current-selection
validation and metadata worker boundaries. Pending metadata disables directory,
row, paging and action hits. Replay Watch preserves native replay ownership.
An editable settings draft may contain more than eight saved opponent paths;
Records still displays the count and permits Clear All/Back. Existing Add and
native launch validation own the eight-opponent limit; view construction must
not prevent the user from correcting that draft.

Known ceiling: only ten catalog rows are drawn; catalog comparison and visible
row projection still run on UI events. Changed packets are concatenated and
the complete rectangle buffer is uploaded. Live practice controls, browser
adapters and full acceptance remain unfinished. Compilation alone does not prove GUI, native or timing behavior.

## Retained player and device setup panels

Players and Devices retain their UI node trees per Navigator instance. Players
keeps its identity and state while its keyboard device child is open. Back drops
the child before restoring its parent; Closing drops both. Row labels, selection,
paging, button interaction and metadata status update their own retained nodes.
Pending metadata disables every hit region. Effects own geometry only; device
discovery, assignment validation and native input attachment stay external.

Solo retains automatic input without assignment/clear controls. Multiple players
retain distinct keyboard assignment with stable positive player IDs and the
existing 64-player bound. Device entries marked unselectable remain visible but
admit no row click. The existing external Use admission still validates drafts.

Known ceiling: ten visible rows per panel and the fixed logical viewport remain.
Changed packets are concatenated and the full rectangle buffer uploads. Native
execution, live practice controls, browser adapters and complete acceptance are
still unfinished. No measured performance or GUI execution is claimed.

## Live practice bookmark restart

During live Play, F7/Mark records the latest accepted native song timestamp as
a session bookmark. Loading, missing time, negative time, replay Watch and
cancellation do not create a bookmark. This is an observed snapshot position;
UI wall time and floating display text never substitute for native time.

F8/Restart Mark prepares a fresh invocation with that exact original-song
--start-ns. Native parser preflight completes before cancellation. The previous
owner cancels, drains audio/input and joins before the fresh owner starts. A
cleanup failure prevents automatic replacement; explicit cancel discards the
prepared replacement. The bookmark follows fresh retries of this chart and
can be reused at Results. F5 preserves the originally pinned start instead.
Recording retry names continue to derive from the original base and ordinal.

The bookmark is owned by the session, not the settings draft. New chart launches
start with no bookmark; replay Watch cannot use this live practice override.
Known ceiling: snapshot delivery is coalesced, so the mark is the latest observed
position rather than the exact physical key event time. This creates a fresh
transport rather than seeking/pause-resuming a live driver. Native finite loops and
presentation-derived pause are described below; sample-exact native loops,
browser host and full native timing acceptance remain unfinished.

## Native finite practice loop

SDK-enabled Windows ASIO live solo and local 2..64-player sessions also admit
an exact original-song `--end-ns`. The common Mixer fence and runtime/group end
cap remain the audio and judging authority. ASIO completion observes actual
rendered blocks through the established multimedia-clock and assessed output
latency interval. It requires a real lower observation and a block starting
at or after the retained physical endpoint, then waits for that block's upper
host interval and drained keyboard/member frontiers. Prepared frame counts and
raw native sample position never establish audible completion. The upper host
point is a conservative admission frontier under the supplied estimates, not
an exact physical endpoint timestamp; physical accuracy remains unknown.
Invalid or regressing ASIO endpoint observations fail without changing retained
evidence. ASIO pause remains unsupported and network finite sessions still
reject before acquisition. SDK-free launch rejection and conditional ASIO build
licensing remain unchanged. Driver/MSVC execution and physical acceptance are
deferred; portable composition fixtures are prepared for later execution.
Progressing ASIO blocks with equal conservative upper host points remain valid
on a coarse timer; completion uses the observed upper point directly and does
not require strict host progress for endpoint interpolation.

F7 marks an exact observed original-song start; F10 marks a strictly later end.
F11 enable preflights a fresh pinned SessionLaunch with both --start-ns and
--end-ns, then cancels/drains/joins the current owner before replacement. Native
parsers admit supported solo/local modes, including SDK-enabled ASIO, and reject
network finite playback.
The region remains scalar; notes acquire no per-frame reactive state.
Each native owner configures immutable audio/logical ends before gameplay and
records completed_end only after native presentation and drained input/resume
completion. Finished publishes only after native cleanup returns. Automatic
repeat requires the worker joined, successful noncancelled Finished, exact
completed_end matching the region and an enabled loop. UI positions, diagnostic
seconds, cancelled sessions, failed cleanup and mismatched endpoints cannot
repeat. Repetition preflights the same pinned original with checked recording
ordinals, one preroll and preserved independent member identities.
F5 returns to the pinned original and disables loops; cancel/focus loss also
disables them. Toggling off stops automatic repetition while the current immutable
section can finish. Failed preflight never cancels the current owner and disables
repetition. Hidden/suspended/closing UI cannot initiate restart; joined terminal
evidence is retained for an active later frame. Pause and loop controls keep
separate footer hit rectangles and retained screen scopes/backstack lifecycle.

Known ceiling: native owner teardown/reopening/loading can introduce a gap —
upgrade when a continuous native owner can reset all playback/input/capture state
without reopening while preserving verified presentation and judging boundaries.
Native hardware, GUI and acoustic acceptance remain unverified.

The audio foundation supports an immutable exclusive playback frame end supplied
before Mixer construction. PracticeLoop maps its matching session start, explicit
preroll and nonzero sample rate to that endpoint using checked integer ceiling
arithmetic. The first frame at or after the marked end is silent; mapping applies
original start/preroll once and excludes physical pause gaps. Native pause models
accept the actual active-prefix/silent-tail report and acknowledge its playback
end only after presentation crossing. This primitive is wired into live solo/local loop owner and judging admission.

The shared runtime also supports a setup-only immutable original-song end.
Solo and local 1..64 members install the same scalar boundary before processing;
group configuration validates all setup conditions before changing any member.
Earlier input keeps original acquisition metadata and judging behavior. Input
mapped at/after the end cannot hit or bind; the existing judge advances only to
the capped end and capture records its actual Advance prefix. Shared Transport,
pause/discipline, source validation and independent player identities remain.
Logical end reports do not acknowledge native presentation or complete remaining
hold/notes. Native solo/local owners and UI loop intent install both endpoints and wait for
the presentation/input frontier and cleanup before repeating.
The native-end observer consumes a retained physical render endpoint and actual
associated output/host clock pairs. It validates the expected playback endpoint,
monotonic grids and clocks, retains evidence across unavailable telemetry and
emits a boundary once only after native presentation crosses that physical frame.
Malformed/regressing/overflowing observations are error-atomic. Host boundary
interpolation has Unknown physical accuracy and never proves acoustic completion.
An actual clock observation at/below the boundary is required as a lower bracket;
starting observation after an already-presented endpoint cannot invent that
missing relation. Once the physical marker is known, the observer retains the
latest actual pair strictly below it for checked interpolation.
This observer does not itself mutate Transport, drain inputs or restart owners;
native lifecycle integration remains required.
Linux solo and local 2..64 playback may configure --end-ns strictly after --start-ns.
It installs matching immutable logical/audio ends before gameplay. Rendered end
stops additional BGM supply; actual native crossing, drained input and a safe lag
watermark reaching the logical end are all required for finite prefix completion.
Original pre-boundary input remains valid; events at/after the terminal native
host boundary cannot enter gameplay. Pending manual resume is reconciled first.
Endpoint completion does not fabricate remaining holds, misses or full scores.
Initial clock observation after an already-presented endpoint fails explicitly.
For local groups every assigned device must drain, and the actual committed
global input frontier must pass the terminal host boundary. A single backlogged
device blocks advancement and completion. All members share one immutable end,
output owner and Transport; independent player IDs/captures/scores remain intact.
Remaining queued input strictly after the terminal frontier cannot enter gameplay.
Windows WASAPI shared/exclusive and macOS CoreAudio solo and local 2..64 owners use the same finite audio/judging/presentation contract. Their local groups additionally require the actually committed InputMerger frontier and every member at the logical end. Network endpoint connections remain required; SDK-enabled ASIO uses the upper-interval admission frontier described above. Windows uses the actually negotiated PCM rate and QPC receipt ordering; all pending WM input must drain before completion. macOS uses the configured native PCM rate and HID collection frontier; its pending resume parking must reconcile before completion. These owners reject unsupported finite modes before session resources open. Omitting --end-ns retains full-song behavior.

Directly unverifiable implementation paths need portable regression fixture
source for later execution: exercise actual mixer/runtime/capture components,
controlled clock observations, boundary ordering and failure paths. Keep native
device, GUI and acoustic acceptance separately listed; mocked clocks and source
compilation cannot prove physical output behavior. Test execution remains deferred
until authorized, while fixture authorship continues alongside implementation.

## Catalog search and filtered selection

Catalog navigation supports Up/Down, PageUp/Down by fifteen displayed rows, and
Home/End within the current search projection. Focused search Home/End moves
the text cursor. Vertical wheel input over a visible chart row navigates that
projection; native pixel deltas follow the renderer's physical/logical stretch.
Fractional deltas accumulate, each event admits at most fifteen steps, and
positive vertical deltas move earlier. Nonfinite input and lifecycle/focus/query
changes clear the remainder. Inactive/hidden views and other controls cannot
consume catalog scrolling. Scrolling cancels an armed click; no gameplay input
or timestamp is produced. Rapid events use retained painter-order hit regions
while central click hits await redraw, preserving search-field overlap rules.
Navigation retains original chart identities and
does not rebuild the cached search projection. Regression fixtures are prepared
for later execution; source compilation alone is not GUI acceptance.

Selection exposes a 256-byte single-line search field (F3 or click). Whitespace
separated tokens must each occur in the combined lowercased title and artist.
The original catalog order and entry identity remain; a changed query retains
the selected entry when matched, otherwise selects the first match. Zero matches
admit neither Play nor Records for a previously selected hidden chart.

Up/Down navigates matches. Enter exits search editing before a subsequent Enter
starts play; Escape clears search and exits editing before ordinary close.
Settings Back and Play return preserve the query, while navigation clears focus
so hidden views cannot consume text. Matching runs only when the query changes.
Known ceiling: normalized title/artist text is cached once, adding catalog-sized
memory. Matching is linear per query edit; Unicode lowercase substring matching
is not locale collation, full case folding, accent removal or fuzzy ranking.

## Presentation-derived live pause

F9 or the Pause/Resume button requests live pause when the native owner announces
support. Linux ALSA, Windows WASAPI shared/exclusive and macOS CoreAudio
solo/local 2..64 without network competition implement the same acknowledged
boundary and key reconciliation flow; saved-record ghosts remain supported.
Replay Watch uses the same native/mixer boundary coordinator to stop recorded
operation progress and sound together, preserving the playback cursor and
recorded start. It acquires no keyboard or network and does not mutate recordings.
Capability is announced only from an actual native output/host relation. ASIO
announces no pause capability and requires a validated
presentation relation; backend selection cannot invent that relation.
Cross-platform native acceptance remains open.
Local cohorts use one native pause coordinator, one shared Transport and
one mixer. Each source still maps to its original player, including sparse IDs
and u32::MAX. Partial group failures retain the already committed member reports
before terminating the whole cohort. Paused idle periods add no member advance
or capture records, and group completion remains disabled until resume input
reconciliation finishes.
In local multi-player play, all members use the same
acknowledged boundary, shared Transport and output; independent per-player
pause clocks are prohibited.
Local input retains its fair acquisition and global timestamp ordering.
Pause commits one shared boundary only after every source drains. Paused key
levels remain per device, with independent judges, scores and captures. Resume
must wait for the configured input-lag frontier to reach the acknowledged resume
boundary before reconciling releases and admitting subsequent original events;
pause must not bypass the merger's chronology or late-input policy.
Windows WASAPI and macOS CoreAudio local owners adopt this same shared
boundary, lag and per-member policy. Windows continues QPC receipt acquisition
and native owner messages during pending acknowledgements; macOS preserves
normalized native HID timestamps and attachment/loss checks. ASIO remains
unsupported for pause until presentation evidence establishes a usable relation.
While paused, ordered input is removed through the lag frontier into key levels,
without advancing the committed judge frontier past the shared pause boundary.

UI requests change desired state only. The owner publishes Running, Pausing,
Paused and Resuming; controls are disabled during pending transitions, cancellation,
cleanup/retry and unsupported output modes. The mixer freezes its original scheduling grid. The
owner retries acknowledgement publication after transient UI-slot contention,
so an idle paused session cannot strand the UI in Pausing. Successful delivery
clears that retry without continuously cloning unchanged paused snapshots. The
owner recovers the first pause/resume output frame from coalesced render reports
and waits for the actual native output frontier to cross it. Host boundary time
is interpolated from request and crossing output/host observations; its physical
mapping error remains Unknown. UI wall time never substitutes for this relation.

While waiting for a boundary all owners fence judging and correction updates.
Linux and macOS hold gameplay input collection. Windows continues its native
message pump and Raw Input acquisition, parking at most 65536 events in receipt
order before judge admission; timestamps remain actual QPC receipts rather than
historical hardware key times. Close and device-change messages still run.
After pause acknowledgement the owner appends a shared Transport
pause, drains pre-boundary input, and commits one judge advance at the boundary
before announcing Paused. During pause it drains native input into bounded key
levels without generating scoring input or repeated frozen capture operations.
After resume acknowledgement it resumes Transport, reconstructs presentation
discipline using the cumulative paused-frame gap, and drains the paused prefix
before admitting post-boundary original input. macOS solo parks at most 4096
post-resume HID events until the collector is actually empty, preserving their
original timestamps and admitting reconciliation releases first. Local cohorts
on all three platforms use the existing bounded 65536-entry shared InputMerger
for original events during pause and resume; release reconciliation runs outside
that native-event merger through the actual member Runtime. Hitting a
collection iteration budget does not count as an empty collector. Windows
suppresses automatic judge advances while the message pump remains backlogged.
Native keysounds use the mixer
playback grid; buffered physical output and clock observations retain their grid.
A transient unavailable render read reuses the last validated report; initial
input waits until real render evidence exists. It never invents a frame cursor.

Keys logically held before pause remain held if their final physical level is
down. If released, synthesize only an Up at the resume boundary through the same
actual Runtime and capture path, preserving native provenance and the original
physical clock point. New keys held during pause stay suppressed until Up;
paused taps cannot score. Ordinary unpaired Up/Repeat outside pause retain
previous runtime handling and do not establish tracked presses. Reconciliation
is bounded to 65536 tracked controls and ordered by device/acquisition sequence.
Capture records accepted judge operations in monotonic song time; paused idle
wall time adds no replay operation or new wire-format command. Resume release
operations are recorded and reconstructed by the existing judge logic.

Cancellation still stops and joins the native owner while pending or paused.
Explicit diagnostic --seconds remains a wall-time cutoff including pause.
Known ceiling: boundary interpolation has no acoustic accuracy guarantee.
Missing/regressing/unrepresentable clock relations, native device failure and
raw queue SYN_DROPPED/resync terminate and drain the session with its valid
recorded prefix. Waiting for output presentation can grow a raw-input backlog;
input loss is never silently repaired. ASIO/network pause policy,
gapless loops/browser/full widget host and full native/GUI/replay acceptance remain
unfinished. Source compilation and authored fixtures are not execution proof.

### Native finite solo expansion
Windows WASAPI shared/exclusive and macOS CoreAudio solo or local 2..64 commands accept a strictly later unsigned `--end-ns`. Derive the endpoint on the actual PCM grid, freeze audio at that exclusive frame, cap judging at the logical end, and finish only after native presentation and drained input/resume reconciliation. Unlimited sessions retain full-song completion. Network finite modes remain explicitly rejected pending their owners; SDK-enabled Windows ASIO uses the actual block upper-interval completion frontier described above. Author portable argument/frontier and model fixtures for later execution; compilation is not native acceptance.

### Native finite UI practice intent
Enabling a marked practice loop preflights a fresh pinned invocation with exact --start-ns/--end-ns, then cancels and joins the previous owner. Each native session ends only at its immutable PCM/logical endpoint and native/drained frontier. Automatic repeat requires the old worker joined, successful noncancelled Finished publication and its exact completed endpoint. Observed UI song time never authorizes an audio stop/repeat; diagnostic seconds and failures cannot repeat. Disabling repetition leaves the current immutable section fence until it finishes; F5 returns to the pinned original. Fresh repeated owners preserve replay filename ordinals and one preroll. Restart may leave a device reopening gap; no gapless claim. Prepare fixtures for later execution; native GUI/acoustic acceptance remains deferred.

### Precise practice section editor
F6 Practice edits retained start and optional end fields independently, with Tab/click focus and exact seconds/M:SS/H:MM:SS up to nine decimal places. Empty end plays through song end; a configured end must be strictly later than start. Done atomically updates both original-song nanosecond fields in the parent settings draft, preserving unrelated options. Invalid range/capacity preserves that draft and child error state. Apply remains separate; Back discards. Full Song sets start zero and clears end; Through End clears only end. Views repaint only changed dependencies and dispose with screen scope. Native mode validation remains authoritative on Apply/start; Network finite configurations are unsupported; SDK-enabled Windows ASIO admits finite prefixes. Prepare focused regression fixtures for later execution; GUI/device execution remains deferred.
