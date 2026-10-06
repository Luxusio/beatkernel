# BMS desktop player

## Static image filename compatibility

Shared native/browser image preparation admits supported missing-filename
variants through the exact priority, containment, literal-error and budget
contract in [shared preparation](REQ__bms-preparation.md#missing-static-image-filename-variants).
A missing `background.bmp` can select `background.png`; an existing damaged
`background.bmp` retains its unavailable status. Original chart references,
gameplay/replay identity, crop definitions, Layer transparency, atomic native
publication and Worker-owned browser decoding keep their existing semantics.
Actual image rendering and platform acceptance require later execution.

## Portable finite live ownership

An explicit finite StepGameplay setup must use the existing SoloRuntime logical
song endpoint. Original-song start and strictly later end are immutable; actual
input acquisition remains validated at/after the end while binding is suppressed
and the judge advances only to the end. Capture records these real reports with
section metadata, preserving provenance and applying input offset once. No
terminal report is synthesized from audio silence or a UI clock.

The output frame endpoint uses the same PracticeLoop mapping as finite replay.
BGM and remote keysound targets rounding onto or past the exclusive frame fence
are excluded without erasing committed judgments. Finite completion requires
the logical end report, actual configured Mixer fence, actual presentation
crossing, all commands acknowledged and queues/feeder credits resolved, with
consumed/applied counts equal to acknowledged commands. Frozen voices need not
drain. Missing evidence keeps playback incomplete; rejected execution fails.
Original BGM targets at/after the logical end are excluded before output-time
conversion, so unreachable future cues cannot overflow a finite setup.
Invalid BGM command kinds and nonfinite gains remain refused even outside the
section. Filtering a queue prefix must not hide an eligible command behind it;
snapshot extraction stays bounded by the queue's actual capacity.

Unlimited setup retains existing full-song completion and recording bytes.
Finite competition identity includes the end through the existing section
identity envelope. Browser finite live bindings/controls must use this same
owner and actual output metadata. Native finite capture integration and actual
cross-host verification remain separate work.

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
pointer positions map through the same centered contained viewport as rendering;
bars, non-finite/outside positions and zero-sized windows cannot activate controls.
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
Committed cursor movement and deletion honor extended grapheme boundaries;
public positions remain UTF-8 byte offsets. Reject controls and
newlines. Native IME input in all existing editable fields is owned by the active
screen and selected field: search, settings/profile, display options, practice
start/end and record directory. Reuse the ordinary editing target and its
screen instance instead of a second, narrower focus model. Record-directory
publication through IME or clipboard clears stale catalog/selection/preview only
when its committed path changes, matching ordinary text editing. Bounded preedit previews never modify
the draft, search projection or persisted arguments; commits use the existing
transactional editor. Switching fields/screens or losing active UI admission
clears composition and native enable acknowledgement. Active composition
suppresses ordinary keyboard text and shortcuts. The native event API carries
no composition generation ID; ordered enable/disable and current-target guards
are used without an absolute stale-event guarantee. All admitted focused field
previews retain the complete composition and optional native selection
as UTF-8 byte ranges. Underline the composition and highlight a nonempty selected
range inside the visible scalar window; clip both to the text field. Keep the
caret position in view and show the whole composition when it fits. A missing
native preedit cursor range hides the caret while retaining the composition
underline; an explicitly collapsed range still displays a caret. An endpoint-only
range change repaints its retained field without rebuilding unrelated rows or
controls. Commit, cancellation and field/lifecycle changes clear decorations;
preview metadata never changes committed text or search results. Rejected edits
preserve text, cursor and metadata. Supplied-font field rendering is specified
below; bitmap rendering remains when no font is supplied or admission fails.
After composing the current screen, provide the focused field's actual retained
hit rectangle to the native IME cursor-area API. Project its logical rectangle
through the renderer's common contained viewport into window client physical
pixels, with outward rounding and clipping. Reuse actual field geometry rather
than duplicating layout or text metrics. Cache unchanged submissions by window,
screen/field and physical area; invalidate on loss of admission or replacement.
Zero-sized surfaces, absent/stale field geometry and unrepresentable native
coordinates yield no submission. Redraw after resize/scale or field changes
supplies fresh geometry. The OS chooses candidate placement; X11 supports the
position only, and unsupported backends cannot promise a visible popup.
Multilingual shaping and real native candidate-placement acceptance remain
unfinished. Prepared regression
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
may further limit actual frame rate. Surface resize contains the fixed logical
960x720 scene with whole-pixel aspect fitting; zero-size frames are skipped. Lost surfaces are recreated;
outdated surfaces reconfigure, timeout/occlusion skip a frame, fatal GPU errors
request cancellation and wait for game cleanup before exit.

## Shared text selection

All existing desktop LineEditor fields support Shift+Left/Right/Home/End and
select-all: Ctrl+A on Windows/Linux and Command+A on macOS. Select-all matches
the logical A character, consumes its text event, and excludes additional
Shift/Alt/command modifiers. Selection navigation uses Shift alone. Commands
apply only to the focused editable field while UI is ready; active IME
composition retains keyboard ownership. Search, native settings values/profile,
display options, practice start/end and record-directory drafts share this model.

Selections use ordered UTF-8 byte boundaries and extended grapheme movement. Reversing
direction retains the starting anchor until collapse. Ordinary Left/Right
collapse selection to its start/end without another step. Insertion replaces
selection once; Backspace/Delete remove it once. Validate control characters and
the replacement's final byte count before any mutation, preserving the entire
editor on failure. IME preview replaces the base selection in a clone; cancel
preserves the base and commit replaces it once. Empty preedit clears composition
without deleting the selected base text. Native cursor visibility stays intact.

Use full-string extended grapheme segmentation for committed edits, including
combining marks, decomposed Hangul, emoji modifiers, ZWJ sequences and regional
indicator pairs. After insertion or deletion joins neighboring clusters, move
the committed caret forward to the next whole-cluster boundary. Selection
endpoints must remain whole-cluster boundaries. Native preedit cursor/selection
keeps the platform's original ordered scalar byte endpoints, even inside a
cluster; ordinary edits of such a preview normalize endpoints before mutation.
Keep the 4096-byte field bound and atomic validation. Editing segmentation runs
on editor commands; window segmentation runs when input geometry is repainted.
Retained fields reuse geometry between actual updates, without a new per-frame
cache or work in gameplay/audio callbacks.
Visible borrowed windows begin and end on full-string extended grapheme
boundaries while decorations retain scalar/advance metrics and native internal
IME endpoints. Bitmap windows retain the requested scalar budget: omit a whole
cluster that cannot fit; if the caret's cluster cannot fit, show an empty window
at that caret. At an ordinary byte boundary, the caret's cluster is the preceding
one, or the following one at the start of the field. Supplied-font windows retain
the 1024-scalar draw budget, with an explicit error when the caret's cluster
exceeds it for positive width. Zero-width windows remain empty. Pixel clipping may
crop a whole admitted cluster wider than the field; do not split its text slice.
Prefer complete composition when its enclosing clusters fit both budgets;
otherwise keep the caret's cluster and native cursor visibility. Complex-script
shaping and automatic system font discovery remain separate work; supplied
fallback fonts use the shared atlas chain below. Projection borrows cached text and
glyph metrics without allocation, atlas mutation or per-note reactive work.

The borrowed visible projection clips selected ranges to scalar columns and the
existing focused text-field renderer highlights them. Equality changes repaint
the relevant retained field; identical updates stay idle. Each retained editor
keeps its selection across focus changes until that draft is reset/replaced;
unfocused fields hide it. Held modifiers reset when UI becomes unavailable,
including focus loss, occlusion, suspend, pending work and closing. Ordinary
field changes retain held modifiers. Mouse/word selection and
IME in future dialogs requires the same common target. Regression fixtures cover model,
scene geometry, retained invalidation and actual desktop event routing; execution
and native keyboard/IME acceptance remain deferred.

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

### Actual catalog read budget

Catalog scanning accounts raw bytes actually returned by chart readers,
including prefixes whose read, text decode or parse later fails. File metadata
can reject an individually oversized candidate, but cannot establish aggregate
read usage. Limit each candidate read to the lesser of its existing 8 MiB raw
cap and the remaining 64 MiB catalog raw budget. One extra byte may be read to
detect an oversized/growing stream; it is counted explicitly, never parsed or
published, and ends aggregate scanning when it exceeds the remaining budget.
Thus accepted raw usage is at most 64 MiB, and attempted usage is at most
64 MiB plus one detection byte across the scan, rather than one full extra file.

Preserve the separate per-chart decoded UTF-8 limit, synchronous read/scan APIs,
deterministic catalog order, existing directory/chart/depth limits, diagnostics,
cooperative cancellation and complete-catalog publication. Do not decode audio
assets during catalog scanning. Scalar progress bytes report actual returned
raw bytes, not advertised sizes. A cancelled scan still returns no partial
catalog. Deferred fixtures must exercise real reader budgets and controlled
scan-time file changes; actual filesystem/window/performance acceptance remains
unverified.

### Aspect-preserving viewport and input

Render the complete logical scene inside one centered, aspect-preserving pixel
viewport on native and browser surfaces. The surrounding bars retain the normal
surface clear color and never become menu hit regions. Rectangles, fonts, BGA
and GPU note instances use the same viewport. Keep this transform independent
of gameplay clocks, chart identity, judgment and native backend selection.

Use one portable integer-bounded viewport primitive for GPU presentation, native
menu pointer conversion and pixel-wheel scaling. Reject zero extents and
nonfinite coordinates; menu edges are half-open. Round the contained dimension
down to whole pixels, with a one-pixel minimum on nonzero surfaces. Center any
odd remainder deterministically; tiny extents necessarily quantize the ratio.
Identity-scale axes preserve finite fractional captured positions directly.
Avoid avoidable division/multiplication round trips; finite downscaled positions
must not be rejected solely because an intermediate product overflows.
Pending startup/profile/renderer preparation cannot acquire search focus or begin
text input. Lifecycle cleanup may still clear existing focus while UI is unready.

Browser Window forwards original touch coordinates, CSS extents and the cached
requested backing extent from the same acquisition observation. It does no lane
hit testing or rendering. Worker/WASM converts the original point through the
same viewport before existing contact routing. Preserve raw encoded payload,
timestamp, source, sequence, pressure and contact ownership. Bar or captured
off-surface points project outside logical bounds rather than clamping to a
lane, so held contacts still receive their genuine Up/Cancel. Resize/page changes
never rebind an existing held contact. Refuse invalid geometry explicitly.
Worker validates all touch projections using the common scalar WASM preflight
before admitting any event in that batch. The validation path must not mutate
game state or allocate a projected-point array; actual admission reuses the same
portable calculation. An additional touch-only crossing is explicit and awaits
performance measurement.

Retain the existing explicit projected-input API for callers that already own
logical coordinates. Generated bindings and actual native/browser/DPI/resize,
GPU and touch-device acceptance remain deferred; compile-only fixture evidence
does not prove pixel or performance correctness.

### Asynchronous native catalog startup

For `player --library`, construct the native window and event loop without
waiting for complete library enumeration, chart metadata parsing or supplied
title-font preparation. One owned background operation performs bounded
metadata scanning, search preparation and CPU font work, reusing existing
catalog limits and deterministic order. No audio files are opened by catalog
scanning. Publish only a complete prepared catalog; UI frames share bounded
progress counters and do not copy partial entry lists.

While loading, selection shows an explicit loading state and cannot launch a
chart from partial or stale data. Close requests cooperative cancellation and
waits for actual owner completion without blocking ordinary event dispatch.
Results arriving during closing must not install entries or fonts. Hidden or
suspended selection can retain completed data but cannot resurrect a disposed
screen or mutate another panel's draft. Installing the result updates selection
items, normalized search, diagnostics and font ownership atomically; failure
remains visible and cannot fabricate a usable selection.

Keep synchronous scan callers compatible. Cancellation is checked between
traversal/read/parse/preparation boundaries, and is not a promise to interrupt an
arbitrary filesystem syscall or decoder. Supplied font GPU upload remains on
the renderer owner. Profile loading and direct `--chart` setup are separate
existing startup paths. Deferred fixtures cover scan ordering/bounds/cancellation,
actual background ownership and native loading/close/hidden-publication behavior.
Native filesystem/window/device/performance acceptance remains unverified.

- Comparison views retain eight records and one peer; local panels show this
  detail only when toggled. A four-player panel with eight records has a 72px
  lane region while comparisons are open (44px if a future native group+peer
  mode is enabled). Default local lanes retain normal geometry; richer detailed
  comparisons need a larger view. Rendering acceptance remains deferred.

- Settings validation checks syntax and cross-option constraints, not resource
  availability — actual device/file checks remain in native preparation.
- HID usage bindings still require typed values. Search, native settings and
  profile fields support IME composition ranges and committed text selection;
  clipboard uses the shared asynchronous adapter. Retain bounded drafts and
  next-session-only application when extending editing.
- Profile operations drain on close rather than being forcibly interrupted —
  assess filesystem stall behavior during deferred native acceptance.
- New-profile publication requires hard-link support; concurrent replacement
  writers need external coordination and directory crash durability is not
  promised — verify target filesystem behavior during deferred file acceptance.

- Catalog reads retain separate raw/decoded per-file limits and charge actual
  returned raw prefixes against 64 MiB. A single counted detection byte can
  exceed the aggregate budget and stops scanning before parsing that candidate;
  physical disk traffic and arbitrary syscall interruption are not measured.
- Library catalog scan/search/title-font CPU preparation use one background
  owner after asynchronous startup; cooperative cancellation cannot interrupt
  arbitrary filesystem calls or decoder work. Direct title-font and startup
  profile preparation now use the owned stages specified below; game loading/
  calibration remain separate paths. GPU font upload can still
  consume renderer time; native responsiveness acceptance remains deferred.
- At most 2048 visible notes per displayed playfield and a finite UI rectangle
  batch are rendered per frame (65,536 UI rectangles). Note overlap and geometry
  capacity overflow report an error — revise admission for denser layouts.
- Supplied fonts render catalog titles/artists and editable fields; remaining
  labels and noneditable metadata use bitmap glyphs. Shaping
  and automatic system font discovery remain pending; caller-provided fallback
  chains are supported as described below. Native visual acceptance is pending.
- Full-song completion now has source integration; native presentation and
  full-queue admission behavior remain unexecuted — verify these boundaries,
  final keysound/BGM tails and long-chart cancellation during deferred native
  acceptance before claiming complete playback.
- WASM renderer compilation is source evidence only; browser startup, adapters
  and actual browser rendering/audio/input remain future work.
- The common centered viewport contains logical geometry with whole-pixel
  fitting; tiny surfaces necessarily quantize its aspect ratio. Captured browser
  touch geometry records the requested backing extent, which can precede a
  queued resize's actual presentation. Native/browser/DPI acceptance is deferred.
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
network competition with a local group uses the same common group owner and
committed native output-start path, with execution acceptance deferred.

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
recorded-prefix completion is admitted without diagnostic seconds; ASIO replay
pause uses the interval policy below. GUI Watch forwards the pinned clock assessments. Graphical
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
all seven migrated menu views use retained nodes as described below. A full
widget host remains unfinished.
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
wrapping and shaping remain future widget work; caller-supplied font fallback
uses the shared atlas chain described below.

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
fails explicitly before catalog publication or native play on invalid or
excessive data. The desktop window/renderer may start earlier in loading state.
The font also renders Search, Settings value/profile and visible value rows,
Display fields, Practice start/end and the Records directory. The option stays
outside native invocation/replay/profile arguments; labels, buttons and other
noneditable metadata retain bitmap text.

Repeatable `--fallback-font PATH` (up to seven, only with `--title-font`) forms
an ordered chain after the title font. Each read is bounded like the title font
and every file is parsed before catalog publication or native play; any invalid file fails
preparation explicitly. A character resolves to the first font in chain order
whose map contains it, else to the title font's glyph zero as the visible
replacement, flagged `missing`. Placement is keyed by chain index and font
glyph, so equal glyph ids in different fonts never alias. All chain glyphs
share the one atlas image, cache limits and renderer texture identity, and
transactional field extension keeps the same chain. Ascent and baseline remain
the title font's. Shaping, kerning and automatic system font discovery are not
provided; fallback paths stay outside native, replay and profile arguments.

At UI input/navigation/profile boundaries, extend the cache transactionally for
current field text and uncommitted IME previews. Each string is bounded to 4096
UTF-8 bytes, a batch to 64 KiB, and the atlas to 4096 cached characters. Cache hits
reuse the same immutable Arc. A miss prepares a private candidate with stable
existing UVs and publishes only after the entire batch succeeds. Update the
same renderer texture at the same extent, preserving resource count, byte budget
and retained title packets. Admission failure keeps draft text and the previous
atlas intact, displays an error and uses bitmap fields until successful admission.
No rasterization, font parsing or pixel copying occurs in retained paint effects
or audio/gameplay callbacks. A cache miss can allocate and upload on the UI owner.

Font fields use actual glyph advances for visible UTF-8 windows, caret, committed
selection and IME decorations; clip pixels to the field. Preserve native missing
cursor visibility, fit the composition when possible and keep the caret within
the 1024-scalar draw window even for long or zero-advance input. Font-generation
changes invalidate input nodes; cached identical updates leave retained nodes
idle. Full renderer recovery uploads the latest CPU atlas, creates a new texture
identity and rebinds fields/rebuilds Selection; surface-only recovery retains it.
Automatic font discovery, shaping and native visual acceptance remain
unfinished. Source fixtures cover these boundaries for execution.

Portable font preparation accepts caller-provided TrueType/OpenType bytes and
a fixed pixel scale, preparing glyphs into a bounded RGBA atlas for the existing
GPU texture path. Font bytes, dimensions, scale and cached glyph count have
explicit limits. Cache hits reuse metrics/UVs; rejected glyphs preserve prior
pixels, allocations and cache entries. Baseline bounds and advances are retained,
whitespace needs no pixels, and missing glyphs are explicitly reported. Non-whitespace
characters resolving to the same font-and-glyph identity reuse its metrics and atlas
placement, including missing glyph zero. The character cache limit remains unchanged;
whitespace never acquires geometry from an alias, and failed preparation preserves
both identity and character caches. Raster
and packing run outside input/audio callbacks. No font is bundled; native font
discovery, broader widget integration, shaping and actual multilingual
GUI acceptance remain unfinished. Self-authored font fixtures exercise these
boundaries; compilation alone does not establish visual behavior. Executed
regressions and bounded Linux multilingual GUI evidence are recorded in
[font fallback integration](../changes/CHANGE__font-fallback-integration.md).

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
evidence. Offline ASIO pause uses the exact live interval policy below; finite solo network sessions use
the section identity policy below. SDK-free launch rejection and conditional ASIO build
licensing remain unchanged. Driver/MSVC execution and physical acceptance are
deferred; portable composition fixtures are prepared for later execution.
Progressing ASIO blocks with equal conservative upper host points remain valid
on a coarse timer; completion uses the observed upper point directly and does
not require strict host progress for endpoint interpolation.

F7 marks an exact observed original-song start; F10 marks a strictly later end.
F11 enable preflights a fresh pinned SessionLaunch with both --start-ns and
--end-ns, then cancels/drains/joins the current owner before replacement. Native
parsers admit supported solo/local modes, including SDK-enabled ASIO, and solo
network finite playback with exact section identity agreement.
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
Windows WASAPI shared/exclusive and macOS CoreAudio solo and local 2..64 owners use the same finite audio/judging/presentation contract. Their local groups additionally require the actually committed InputMerger frontier and every member at the logical end. Solo network owners exchange the finite endpoint identity described below; SDK-enabled ASIO uses the upper-interval admission frontier described above. Windows uses the actually negotiated PCM rate and QPC receipt ordering; all pending WM input must drain before completion. macOS uses the configured native PCM rate and HID collection frontier; its pending resume parking must reconcile before completion. These owners reject unsupported finite modes before session resources open. Omitting --end-ns retains full-song behavior.

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
projection; native pixel deltas follow the renderer's contained viewport scale.
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
support. Linux ALSA, Windows WASAPI shared/exclusive, SDK-enabled ASIO and macOS CoreAudio
solo/local 2..64 without network competition implement the same acknowledged
boundary and key reconciliation flow; saved-record ghosts remain supported.
Replay Watch uses the same native/mixer boundary coordinator to stop recorded
operation progress and sound together, preserving the playback cursor and
recorded start. It acquires no keyboard or network and does not mutate recordings.
Capability is announced only from an actual native output/host relation.
ASIO recorded and live playback use the interval policies below. Backend
selection cannot invent a presentation relation.
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
normalized native HID timestamps and attachment/loss checks. ASIO uses its
original interval evidence with the exact logical freeze and input policy below.
While paused, ordered input is removed through the lag frontier into key levels,
without advancing the committed judge frontier past the shared pause boundary.

UI requests change desired state only. The owner publishes Running, Pausing,
Paused and Resuming; controls are disabled during pending transitions, cancellation,
cleanup/retry and unsupported output modes. The mixer freezes its original scheduling grid. The
owner retries acknowledgement publication after transient UI-slot contention,
so an idle paused session cannot strand the UI in Pausing. Successful delivery
clears that retry without continuously cloning unchanged paused snapshots. The
owner recovers the first pause/resume output frame from coalesced render reports
and waits for the actual native output frontier to cross it. Point-based host
boundary time is interpolated from request and crossing observations; interval
backends retain their complete assessed windows and explicit software cutoffs.
Physical mapping accuracy remains unmeasured. UI wall time never substitutes
for a native relation.

While waiting for a boundary all owners fence judging and correction updates.
Native adapters continue acquisition into bounded pending input while judge
admission waits. Windows continues its message pump and Raw Input acquisition;
its timestamps remain actual QPC receipts rather than historical hardware key
times. Close and device-change messages still run. After pause acknowledgement
the owner stages a shared Transport pause and exact playback-frame song anchor,
drains pre-boundary input, and commits one judge advance at the boundary
before announcing Paused. During pause it drains native input into bounded key
levels without generating scoring input or repeated frozen capture operations.
After resume acknowledgement it resumes Transport, reconstructs presentation
discipline using the cumulative paused-frame gap, and drains the paused prefix
before admitting post-boundary original input. Solo owners retain bounded
post-resume originals until acquisition reports no backlog, preserving their
timestamps and admitting reconciliation releases first. Local cohorts
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
input loss is never silently repaired. Network pause policy,
gapless loops/browser/full widget host and full native/GUI/replay acceptance remain
unfinished. Source compilation and authored fixtures are not execution proof.

### Native finite solo expansion
Windows WASAPI shared/exclusive and macOS CoreAudio solo or local 2..64 commands accept a strictly later unsigned `--end-ns`. Derive the endpoint on the actual PCM grid, freeze audio at that exclusive frame, cap judging at the logical end, and finish only after native presentation and drained input/resume reconciliation. Unlimited sessions retain full-song completion. Solo network finite modes use the exact section identity policy below; SDK-enabled Windows ASIO uses the actual block upper-interval completion frontier described above. Author portable argument/frontier and model fixtures for later execution; compilation is not native acceptance.

### Native finite UI practice intent
Enabling a marked practice loop preflights a fresh pinned invocation with exact --start-ns/--end-ns, then cancels and joins the previous owner. Each native session ends only at its immutable PCM/logical endpoint and native/drained frontier. Automatic repeat requires the old worker joined, successful noncancelled Finished publication and its exact completed endpoint. Observed UI song time never authorizes an audio stop/repeat; diagnostic seconds and failures cannot repeat. Disabling repetition leaves the current immutable section fence until it finishes; F5 returns to the pinned original. Fresh repeated owners preserve replay filename ordinals and one preroll. Restart may leave a device reopening gap; no gapless claim. Prepare fixtures for later execution; native GUI/acoustic acceptance remains deferred.

### Precise practice section editor
F6 Practice edits retained start and optional end fields independently, with Tab/click focus and exact seconds/M:SS/H:MM:SS up to nine decimal places. Empty end plays through song end; a configured end must be strictly later than start. Done atomically updates both original-song nanosecond fields in the parent settings draft, preserving unrelated options. Invalid range/capacity preserves that draft and child error state. Apply remains separate; Back discards. Full Song sets start zero and clears end; Through End clears only end. Views repaint only changed dependencies and dispose with screen scope. Native mode validation remains authoritative on Apply/start; solo network finite configurations require exact section identity agreement; SDK-enabled Windows ASIO admits finite prefixes. Prepare focused regression fixtures for later execution; GUI/device execution remains deferred.

## LNOBJ gameplay composition

LNOBJ chart endpoints project through the same compiled Hold objects, PlayerChart lane/body display, JudgeEngine and captured-operation replay reconstruction as LNTYPE1. No separate replay judge or UI clock is introduced. Endpoint markers are retained metadata and are unsounded, without requiring an endpoint WAV asset. Exact tempo/STOP timing and existing strict held-lane overlap/resource rules apply; parser and actual player/replay composition fixtures are prepared for later execution.


## Shared chart character decoding

Application file loaders accept valid UTF-8 first, stripping a single initial UTF-8 BOM; that BOM requires valid UTF-8. Without a BOM, invalid UTF-8 is decoded strictly as WHATWG Shift_JIS (Windows legacy extensions) before the unchanged text parser. UTF-16/32 BOMs, malformed/truncated bytes and replacement decoding reject. Explicit Utf8/ShiftJis modes are available at the shared decoding API; native/GUI/offline/replay loading uses the same Auto policy. This is a deterministic two-encoding preference, not general charset detection; ambiguous bytes that are valid UTF-8 retain that interpretation.

Encoded input and decoded UTF-8 independently obey the parser's 8 MiB cap; output expansion and reader growth fail before parsing. Conversion runs during preparation, with a bounded scratch buffer and capped output allocation, outside audio/input callbacks. Metadata and asset paths preserve decoded Unicode and separator bytes; sound containment and strict parser/resource policies remain in force. Pure literal-byte and real parser/replay composition fixtures are prepared for later execution; build checks do not establish native path/font behavior.


## Shared FLAC sample preparation

The same default app asset decoder prepares native FLAC, complete single-stream Ogg/Vorbis, declared-timing MP3 and strict WAV for offline, native live and recorded playback. Codec work stays outside audio/input callbacks and finishes before stream startup; source format and PCM limits remain explicit. Content signatures determine decoding; original chart references remain unchanged while compatible file lookup may select a supported missing-path variant, and malformed/over-limit assets reject. Other Ogg codecs, chained/multiplexed Ogg and remaining codec conformance remain required future work; this addition does not establish native playback or performance acceptance.


## Converted keysound file compatibility

Native/live/replay/offline default preparation resolves an existing literal asset first. When it is genuinely absent, bounded same-stem WAV/FLAC/OGG/MP3 extension variants may be selected, preserving original Unicode directories and chart identity. Existing corrupt, inaccessible, dangling, escaping or nonregular references fail rather than hiding errors through substitution. Explicit exact lookup remains available, and custom decoder entry points preserve their old exact behavior. OGG lookup is backed by the bounded single-stream Vorbis decoder; Other Ogg codecs, filesystem race protection and native acceptance remain unfinished.


## Declared MP3 timing

Shared preparation uses tagged MP3 timing before native/offline/live/replay stream startup. Valid Xing/Info frame counts and LAME/Lavc delay/padding trim decoded PCM before it enters the immutable bank. Missing metadata leaves raw decoded frames; no guessed encoder delay is introduced. Explicit raw timing is available at the codec API. Original chart/compiled/replay identity and playback-start mapping remain unchanged. Actual source format stays fixed and malformed, incomplete, resynchronized or over-limit input fails setup. Fixtures are prepared for later execution; source checks are not acoustic synchronization or native acceptance evidence.

## Admitted pressed-lane state

Pressed lane indicators consume only successfully admitted bound button inputs from the local RuntimeReport, or actual recorded input operations during replay. Ownership distinguishes device, physical control and BMS game control: repeated Down is idempotent, Repeat cannot manufacture a press, and releasing one owner does not release another. Only channels 11..19 and 21..29 contribute to the canonical 18-bit mask; non-button and other game controls are ignored. Per-player ownership is bounded at 4096 and exhausted batches reject before score/history presentation changes. Empty deadline reports neither clone nor scan ownership. Local players retain independent masks, and the renderer maps channel bits through the prepared chart's lane order.

Pause transitions suppress pressed indicators while retaining admitted ownership for the native resume reconciliation path. Cancellation and final cleanup clear ownership and masks; fresh sessions begin empty. Parked raw keyboard activity is not itself admitted input. Replay ownership is validated before native acquisition and reconstructed incrementally from the same input records; it never infers presses from judge results. Judgment strips and pressed bands remain separate, bounded presentation primitives without per-note signals. Pure ownership, publication, replay and Scene fixtures are prepared for later execution; hardware/native/performance acceptance is pending.

## Actual judge lane feedback

Actual local judge events also drive per-lane presentation feedback. A prepared object-ID index maps each event to its compiled note and lane without scanning the chart on redraw. A fixed 18-lane projection consumes at most the member's 128 retained results. Known Instant/HoldHead/HoldTail stages must match the mapped object shape; unknown objects, custom/mismatched stages, future events and events aged at least 150,000,000 song nanoseconds do not flash. Newest event time wins within each lane, with the later retained event winning equal-time ties. All age arithmetic uses i128 differences before bounded conversion. Invalid result/lane capacities reject before feedback geometry is drawn.

The actual solo desktop and each local panel pass their own recent judge events into the same feedback-aware playfield path. A cyan hit or red miss strip fades into that lane's background immediately above the judgement line; note layers, GPU epochs and white judgement line retain their established painter order. No wall clock or remote/ghost score drives the effect. Pause keeps feedback unchanged with unchanged reported song time; replay seek uses the reconstructed prefix and suppresses future events. Older playfield APIs retain an empty-feedback wrapper. This visual effect does not infer full object completion or hide notes from the bounded history, and adds no per-note signals. Pure projection and actual Scene/Runtime/replay composition tests must be prepared for later execution; native visual/performance acceptance remains pending.

## Reusable playfield visibility storage

The playfield's visibility query also exposes ordered chart-local indices into caller-owned scratch storage. After that storage reaches the bounded query capacity, successful queries reuse its allocation across forward frames, backward seeks, empty windows and chart replacement. An over-budget result clears scratch and rejects presentation rather than admitting a truncated note set. Exact inclusive compiled head/body overlaps, i128 time bounds and endpoint-tree pruning remain shared with the existing reference-returning APIs.

Each Scene owns one initially empty scratch vector, filled lazily and shared sequentially by its at-most-four playfields. Clear retains its capacity. It stores indices rather than references that could outlive a chart. Lane checks and visibility admission precede frame/batch insertion. GPU caches consume indexed borrowed notes without building a temporary reference vector, retain immutable instances on steady membership and invalidate on backward seek, geometry or local epoch changes. This removes the successful visibility-list allocation after warmup; membership/epoch rebuilds and other scene geometry can still allocate. No whole-renderer zero-allocation or measured performance claim is made. Regression fixtures must cover the actual scene path, exact boundaries, long holds, chart changes and rejection; native/GPU and benchmark execution remain pending.

## Finite solo network sections

Solo native network competition admits an optional original-song `--end-ns`, strictly after the recorded start. Both peers exchange an exact versioned finite-section identity containing that endpoint and the existing normalized chart/rules/profile/source-seed/start/runtime identity. Different endpoints, finite versus full-song setups, or incompatible seeds/starts/rules/profiles/runtime versions cannot connect as compatible opponents. Full-song identity bytes remain unchanged. Invalid endpoints reject before ghost asset/file or socket acquisition. This section supersedes earlier finite-network rejection notes; local groups of 2..64 use one shared group session and the same finite endpoint
identity and native completion contract.

Linux ALSA, Windows WASAPI/SDK-enabled ASIO and macOS CoreAudio solo owners pass the endpoint to shared competition preparation and retain their existing immutable PCM fence, logical judging prefix, actual native presentation and drained input frontier. Networking stays on its owned worker; remote summaries never enter local judging or determine completion. Peers share a software start commitment while native output intervals and completion remain independently observed. Cleanup joins the socket worker after native cleanup; the committed frame does not establish physical synchronization, authenticated score, authoritative ranking or guaranteed final delivery. Ghost comparison and captured replay headers/codecs remain unchanged, so a matching full recording may be compared over the local section prefix. Network pause and automatic network practice-loop repetition remain unsupported; a single finite network session is admitted. Pure identity and native argument fixtures must be prepared for later execution; native/socket/full acceptance remains pending.

## Authoritative object presentation

The chart-first native solo/local/replay publishers accumulate every actual judge event into exact prepared-chart-local progress before trimming the 128-event HUD history. Valid Instant hit/miss completes an instant; HoldHead hit consumes its head while preserving body/tail, HoldHead miss completes the hold, and HoldTail hit/miss completes it. Unknown objects, custom stages and shape-mismatched stages do not alter progress. Accepted results update monotonically and idempotently at the admitted prefix, independently of calibrated event timestamps. Per-member two-bit state is bounded by the core chart object limit, shared as immutable 4096-note pages and copied only on changed snapshots/pages; empty reports and redraws do not scan or clone whole chart state.

The actual renderer skips completed objects before admitting the 2048-visible-note budget, and suppresses only the head primitive for a holding note. Body/tail and scratch/double lane mapping retain exact compiled geometry. Progress must match the exact prepared chart incarnation before any geometry; another chart cannot reuse its indices. GPU reuse also compares head-consumption flags. Each member has independent progress, pause preserves it, final snapshots retain legitimate results, and a fresh practice/replay session begins with fresh progress. A caller using the legacy report-before-chart bridge after judge results lacks an authoritative full prefix and retains progress None rather than reconstructing it from truncated HUD history. All actual native owners register the chart first. Regression fixtures are authored for later execution; native/GPU/performance acceptance remains pending.

Completion-aware queries also prune fully completed subranges before descending the endpoint tree. Each immutable progress page retains a duplicate-safe completed count; full pages use that summary and partial-page boundaries inspect only bounded packed words. Long compiled endpoints therefore do not force visits to every completed hold on each redraw. Exact range/page-tail predicates and dense all-completed visitor pruning require prepared structural fixtures; no benchmark claim is made.

## Accepted timing summaries

Live, local, replay and recorded-prefix ScoreSummary accumulation also retains exact scalar timing statistics from every accepted known Instant/HoldHead/HoldTail result. Misses and custom stages do not become synthetic samples. Negative calibrated judge deltas mean early, positive late and zero exact; each accepted hold stage counts separately and grade IDs remain unweighted. Checked u64 counts, i128 signed sums and u128 absolute sums preserve i64-minimum deltas and atomic overflow rejection. Mean truncates toward zero. Full-prefix statistics are independent of the 128-result HUD history and require no per-note signals, floating arithmetic or unbounded samples.

Actual solo/local hit labels show EARLY/LATE/EXACT from the judge's delta, using integer milliseconds with three decimal digits (microsecond truncation, sign category preserved). Solo normal/competition sidebars and the retained records preview show signed bias and mean absolute error from the same summary. Records preview memoization includes timing-only changes. These are accepted-stage timing metrics, not all attempted input accuracy, measured device latency or automatic calibration. Existing score/ghost/replay identities, codecs and peer progress wire fields remain unchanged. Author extreme, stage, full-prefix, transactional, real Runtime/replay and retained Scene fixtures for later execution; native visual/performance acceptance remains pending.

## BMS image and visual timeline foundation

Retain BMP definitions (base36, including BMP00) as exact opaque paths without
opening assets. Channels04/06/07 select base/poor/layer images on the original
song timeline; zero tokens are rests, undefined nonzero references remain
selections for later blank resource resolution. Poor selection is initially
BMP00/BGA00 when defined; selecting poor does not automatically display it.
Use checked core BPM/STOP timing and separate bounded visual resolution so
adding visual subdivisions does not alter gameplay resolution, IDs, note/BGM
times or the compiled resolution in replay identity. Existing line-sensitive
object metadata still changes identity if source edits move gameplay lines.
Seeded conditional admission and duplicate policies
also govern visuals; include visual events in existing total source limits.

Prepare channel indexes before play and query each last selection at or before
the exact original-song timestamp. Pause, backwards seek and fresh practice
restart use the same state projection as live/replay, without accumulated
relative elapsed time or per-frame file IO. Store typed references, not GPU
resources in chart state. Static raster decoding/loading and native Base/Layer upload/rendering are
defined below, including channel opacity, miss activation and static cropping.
Video and ARGB RGB/color-key remain pending; timeline projection alone does not display backgrounds. Meaningful
fixtures must be authored for later execution; source checks are compile only.

Original BMP00/04/06 semantics: [author's BMS format](https://bm98.yaneu.com/bm98/bmsformat.html). Channel07 extension context:
[hitkey command memo](https://hitkey.nekokan.dyndns.info/cmds.htm).

## Static image preparation

Support BMP/PNG/JPEG raster signatures with bounded configurable decoding into
straight RGBA8 before playback; preserve alpha and literal image orientation,
without resizing. Use the maintained image codec with only these format
features enabled. Videos and other formats remain pending. Exact-black Layer
transparency follows the preparation policy below. Native Base/Layer
upload/display uses bounded ownership below;
prefetch/streaming improvements remain pending.

An explicit portable ImageAssets preparation API opens only visual references
and defined BMP00, outside audio/input/frame callbacks. Undefined references,
missing files and damaged/unsupported encoded images have explicit unavailable
reasons and later render blank; visual-only failures need not cancel audio.
Unsafe paths, escapes, nonfiles, access errors and configured capacity/aggregate
limits reject preparation atomically. Reuse contained path resolution and the
missing-only image variant policy above. Deduplicate canonical files and share
immutable decoded RGBA data across image IDs. Do not equate CPU asset count
with the renderer's concurrent texture slots.

Bound encoded inputs and decoded extents/output before admission and enforce
a cumulative decoded bank budget. Defaults: encoded/output64MiB, dimensions
4096, referenced IDs3844 (the selectable base62 namespace, including00) and bank64MiB. Bank budget configurable up to256MiB;
extent up to16384 remains subject to per-image64MiB output. Limits constrain
encoded/output/retained storage, not a hard total-process peak: decoder buffers
and RGBA conversion can coexist. Author codec and real filesystem/prepared
chart fixtures for later execution; source compilation alone does not establish
codec correctness or native rendering acceptance.

## Native BGA ownership and composition

Attached native live/local/replay owners prepare referenced static images
before report loops and atomically publish one immutable CPU bank with the
exact chart/roster. Terminal audio-only invocations keep no-image-IO behavior.
Snapshots share the bank across all members and retain it through results;
fresh sessions start without prior assets. Validate registration before IO.

The UI owner creates/removes GPU textures for the current visible Base/Layer
and activated Poor selections (up to four member views/sixteen unique images). Share canonical
pixel aliases, release unselected textures before admitting replacements,
reuse unchanged selections and bound failed-upload retry to wanted-set changes.
Changing bank identity, navigation/session replacement and renderer destruction
release or reset matching ownership; suspend/resume never reuses stale GPU IDs.
Undefined/missing/failed base selections are black; unavailable layers draw
nothing. Retain per-visible-view unavailable counts for a user-facing caption.

Compose centered aspect-fit base then layer in a black playfield background,
darkened below notes for readability; keep alpha, clipped field bounds, local
member time, note/pressed/feedback/judgement painter order and original timing.
Existing playfield wrappers retain plain lane geometry. Poor selection uses
the temporary miss activation policy below; do not display it continuously.
Videos, additional raster formats and extended color-key policies remain
pending. GPU upload on selection changes may stall the UI; there is no measured
frame latency or native correctness claim until deferred execution.

Author fake resource-owner/cache and real Scene/native publisher fixtures for
alias sharing, bounds, admission failure, selection/bank/page change, cleanup,
renderer reset, original-time pause/seek and sparse local IDs. Source compilation
is not native GPU/device acceptance.


### Static BGA exact-black Layer preparation
BeatKernel uses an explicit RGBA8 policy: chart-declared channel 07 Layer and channel 0A Layer2 images make exactly RGB (0,0,0) transparent before GPU filtering. Base and Poor retain source pixels. Preparation happens once on the game owner; unchanged images share the original Arc and canonical aliases share any changed variant. Changed unique variants count toward the existing aggregate decoded-byte admission limit; retaining a raw image and its changed variant costs both buffers. No threshold, EXBMP, per-frame pixel scan, or video behavior is implied. Pure pixel, alias, capacity, cache-role and native publication fixtures are authored for later execution; source compilation does not establish GPU visual correctness.


### Poor background activation
PoorBackgroundPolicy defaults to a 500ms original-song interval after the latest known, matching, newly accepted miss transition retained in exact-chart full-prefix NoteProgress. Its public lifetime accepts 0..10seconds, zero disables; the Desktop uses the default. In the default Replace mode during that interval, replace Base/Layer with the current BMP00/channel06 Poor selection using original raw pixels; no Poor selection preserves normal backgrounds. Declared undefined/missing selections remain explicit blank resources. Hits do not cancel the interval. Use i128 subtraction and per-member clocks/progress; pause freezes reported time. Unknown/custom/mismatched/duplicate completed events do not retrigger. Future miss timestamps activate only when reached; seek/restart must rebuild the admitted prefix and fresh progress. Retention is independent of the recent128 HUD limit, constant-size and allocation-free in projection. Equivalent but independently allocated charts reject progress mismatch. POORBGA replacement/overlay/off modes are defined below. Author real runtime/replay/publisher and pure boundary/dense-prefix/local fixtures for later execution; no runtime or GPU acceptance is claimed.


### POORBGA static display modes
Recognize selected #POORBGA headers 0 (default Replace), 1 (Overlay), 2 (Off). Cache the checked mode in prepared PlayerChart. The full BgaPresentation selection API carries normal timeline state plus a separately activated poor_overlay. Replace retains the existing raw-Poor-as-Base behavior; Overlay keeps normal Base/Layer and paints raw Poor last beneath gameplay; Off preserves normal background despite misses. Existing project remains selection-only compatibility; opacity and overlay consumers use select. Raw Poor alpha is preserved, without Layer black key. The native UI consumes full selections for each member. Extend bounded active GPU union to16 CPU Arc identities across4views, reuse rawBase/Poor aliases and existing aggregate GPU slot/byte limits; no new shaders/per-frame allocations/decoding. Legacy cache sync has no implicit Poor activation. Author modes/parser/conditional/duplicates/16resource/cache/painter-order/actual runtime and replay publication fixtures for deferred execution.


### Second BGA layer
Support channel0A as an independent Layer2 selection on the original-song visual grid. Prepare its exact-black transparency with the same canonical shared variant as Layer1; raw Base/Poor remain untouched. Compose Base, Layer, Layer2, activated raw Poor beneath gameplay. Replace mode suppresses both layers, Overlay keeps both, Off keeps normal display. Cache at most16 distinct current CPU Arcs across4visible member views; existing GPU total limits still govern uploads. Query and cache steady state use no file/decoder/pixel work or new shader. Author independent timing/zero/undefined/duplicate/conditional/STOP/identity, variant-sharing/byte-cap, 16-resource/admission/failure/release, Scene order and actual Runtime/capture/replay/native publication/fresh practice fixtures for later execution. Channel opacity is defined below; Static cropping is defined below; ARGB RGB/color-key and video remain pending.


### BGA channel opacity
Prepare independent original-song opacity indexes for channels0B (Base),0C (Layer),0D (Layer2),0E (Poor). Default alpha255; 00 rests retain the previous value, nonzero hex bytes use their direct alpha/255 value (01 is1/255, not coerced to0); no interpolation. Complete BgaPresentation carries these values without pixel mutation. Replace mode uses Poor opacity on its substituted Base; Overlay retains each normal channel opacity plus Poor opacity; Off normal. Cache identity depends on image Arcs, so alpha-only changes never reupload or change resource admission. Add Scene clipped sprite alpha multiplication through existing RGBA/shader layout, keep opaque callers compatible. Four-role order/clipping/raw image alpha and Layer black key remain. Source compilation and authored parser/timeline/Scene/cache/actualRuntime/STOP/live-replay/native publication fixtures support later execution; actual blend pixels and platform performance are unverified. Static cropping is defined below; ARGB RGB/color-key and video remain pending.


### Static BGA crop preparation
Filesystem and selected-file preparation share one pure image plan preflight.
Run it before root canonicalization, source resolution, reads or canvas
allocation, then pass the validated plan into preparation without rebuilding its
reference sets. Selected-file budgets require a positive per-file byte maximum
no larger than the aggregate maximum; reject incoherent settings at construction.
Prepare #BGA/#@BGA fragments into default256x256 transparent canvases before playback; explicit CANVASSIZE is defined below. Validate typed crop rectangles/IDs and combined visual counts before filesystem access; gather displayed IDs plus initial BMP00/BGA00 and their original BMP dependencies, capped together by max_images. Crop wins over raw same-ID BMP, and refers to original files even with self/swap IDs. Load dependencies via existing contained literal/canonical dedup; preserve source-unavailable reasons for selected cropped IDs with no fallback. Retain original raw source Arcs, charge all raw source buffers plus unique normalized-crop variants and changed Layer variants to aggregate decoded bytes before allocation. Share identical crops of canonical raw aliases. Apply existing exact-black key after cropping only to Layer/Layer2; Base/Poor retain produced RGBA. Publish immutable bank atomically through actual native preparation. Existing Scene, texture ownership and opacity paths consume it with no perframe cropping, IO or new shader. Half-open corners, negative-origin clamp before placement and clipped destination are explicit policy; full historical renderer conformity/overspill remain unverified/pending, while explicit CANVASSIZE is defined below. Author parser/pixel/clipping/cap/alias/unavailability/cache/Scene/actualRuntime/live-replay/native publication/freshpractice fixtures for deferred execution.


### Explicit BGA canvas dimensions
Honor selected CANVASSIZE for both ordinary static resources and crop canvases. Validate header, positive extent<=configured decode dimensions/hard16384 and checkedRGBA byte size<=per-image/64MiB limit before filesystem or canvas allocation. Header syntax permits1..9999 but preparation budgets still govern. Without header preserve original raw extents and legacy256crop. With header, top-left pad transparent black or crop to plane; never stretch source pixels. Exact-size raw images share originalArc; aliases/normalizeddefinitions share changedcanvas variants. Aggregate budget counts retainedsource plus changedcanvases and keyedLayers. Existing native atomic publication/cache/aspect-fit rendering uses producedcanvas aspect ratio; no frame pixel work or shader change. Parser/pixel/extreme/bounds/alias/nativepublication/cacheScene/actualRuntime STOP-replay fixtures authored for later execution; native pixels/performance and videos/ExtChr remain pending.
## Chart WAV gain preparation

All shared audio preparation paths apply checked VOLWAV percentage to both
keysound bindings (including LN heads) and scheduled BGM commands before
playback. Default unity; zero retains scheduling while muting. Keep decoded
PCM, frame counts, voice IDs, schedules, judgment and replay setup unchanged.
Resolve gain before WAV asset IO and reuse the same preparation in live,
offline, custom-decoder and replay-aware loaders. Device clipping and historical
player conformance require later execution; static header gain does not imply
dynamic97/98 support. Author fixtures for actual shared Runtime/Mixer PCM and
replay-aware preparation, compiling them without execution under the user deferral.
## Acknowledged network terminal prefix

After native audio/input cleanup, a connected solo network owner admits its
exact latest cumulative prefix and waits at most the configured I/O stall
timeout for a matching terminal acknowledgement. Periodic50ms throttling must
not omit that prefix. Wire version6 retains separate progress, terminal progress and
acknowledgement with ordered sequences and checked cumulative counts. Reject
duplicate/unsolicited/wrong acknowledgements, repeated terminal frames and
progress after terminal. Preserve complete-frame writes when prioritizing the
single pending acknowledgement. Simultaneous finish must write the peer's pending
acknowledgement before treating local delivery as acknowledged and stopping.

Remote terminal data stays unauthenticated display data; cancellation/error can
also produce a terminal prefix. It does not establish successful chart completion,
a winner or synchronized start. Queue pressure, disconnect, cancellation and
timeout report failure rather than silently declaring delivery. Stop/Drop retain
immediate cancellation and owned joining; no waiting or socket IO on gameplay or
audio threads. This section supersedes the earlier no-guaranteed-final note only
for a connected peer that explicitly acknowledges the prefix; disconnected peers
remain undeliverable. Prepare protocol/lifecycle fixtures for later execution;
native and socket acceptance remain deferred.
## Network preparation before native playback

Linux ALSA, Windows WASAPI/optionalASIO and macOS CoreAudio solo owners await
bilateral compatible native readiness immediately before audio start, inside
the existing cleanup-owned outcome. Use the configured multiplayer setup
deadline, explicit cancellation and bounded per-pass256 native input/message
draining. No judgment or replay input is admitted during waiting. Preserve
native input loss/removal/decode failures and foreground Raw Input cleanup.
Errors/timeouts cancel the network owner and run existing native cleanup;
offline and ghost-only modes keep their existing startup. Wireversion6 updates
the earlier terminal-prefix version2 contract while preserving final/ack data
semantics. The committed software start below extends this preparation barrier;
native output-zero targeting and measured synchronization remain future work.
## Clock estimate before network audio start

The shared native readiness gate also waits for eight validated session clock
exchanges and a retained conservative offset estimate. All three existing
native wait callbacks continue input draining/cancellation inside cleanup-owned
startup, before audio starts; no extra main UI/audio callback wait is added.
The existing multiplayer setup deadline bounds this phase. Wireversion6 extends
version3 readiness and terminal frames with finite clock probes. Local score,
replay identity and song timestamps remain independent of probe data. Estimates
are software observations. The following committed software start extends
this gate; measured physical synchronization remains unproven. Native/socket/formal acceptance remains deferred.

## Committed software start

Wireversion6 extends software clock sampling with bilateral clock-ready, host
proposal, exact join acceptance and host commit. Configurable checked nanosecond
policy bounds proposal lead, minimum remaining lead, estimate age and uncertainty.
Reject duplicates, wrong roles/order/echo, stale estimates and close deadlines.
Host publishes a local schedule after the entire commit is written; join after
receiving the commit. Native startup waiting services existing bounded input and
cancellation until the committed local target, rejecting materially late release.
This schedules a software audio-start call; hardware output-zero, differing
device latency, measured output-zero and disconnected-peer atomicity remain unproven.
Fixtures are authored and compiled only; execution/formal acceptance deferred.

Software start defaults:2000ms proposal lead,100ms minimum remaining lead,
5000ms maximum estimate age,100ms maximum interval width and25ms maximum gate
release lateness. Shared native application extraction supports
`--mp-start-lead-ms`, `--mp-start-min-lead-ms`, `--mp-clock-max-age-ms`,
`--mp-clock-max-uncertainty-ms` and `--mp-start-max-lateness-ms`. Require host or
join; reject duplicate flags, checked conversion overflow and invalid policies.
An insufficient overall setup timeout fails rather than bypassing agreement.

## Common song target across native prerolls

Wireversion6 carries actual nonnegative preroll in clock-ready. After compatible
preparation/clock sampling the host proposes a common song target with lead plus
the larger preroll. Each peer subtracts its own preroll for the software-start
target, preserving clock uncertainty and minimum earliest lead. Different sample
rates/buffers/prerolls need not match. Native solo preparation supplies its actual
preroll for Linux ALSA, Windows shared WASAPI/ASIO path and macOS CoreAudio.
Replay/judgment identity and offline/ghost playback remain independent.
Checked arithmetic rejects overflow, negative geometry and insufficient lead.
This aligns nominal software start plus preroll, not measured hardware output-zero
or device latency/drift. Execution/formal acceptance remain deferred.

## Physical-frame startup foundation

A dedicated initially held command queue permits silent native calibration while
physical output frames advance and playback/commands remain frozen. The producer
can arm one immutable physical start frame; a straddling render emits the silent
prefix then playback frame0 exactly at that frame. Missed starts reject rather
than silently clamp. Applied first playback frame is independently observable.
Existing default queue and ordinary pause behavior remain intact. Pure checked
session/host bracketing and nominal ClockPair output-frame projection preserve
intervals, check domains/overflow and enforce future frame margins.
These prerequisites now support Linux solo network startup as specified below.
CoreAudio and WASAPI now use held-device calibration/frame arming below; ASIO
still requires its bounded-interval startup port. Authored fixtures compile only;
runtime/formal acceptance remains deferred.

## Linux calibrated initial frame startup

Linux solo network playback starts ALSA behind the initial silence gate, measures
actual advancing native pairs, obtains an early committed target and projects it
with observed slope to a future physical frame beyond the render/buffer margin.
Arm once, require matching applied-frame evidence and native presentation crossing,
then anchor transport/discipline to that physical point. Checked session-to-HOST
bracketing and retained uncertainty are independent of native physical accuracy.
Explicit startup geometry lets pause/end observers validate silent prefix and
finite suffix without weakening default guards. Preserve up to4096 actual evdev
events after arm until origin known, then use original timestamps in existing
runtime input filtering. Cancellation/loss/resync/overflow/native errors remain
explicit inside joined native cleanup. Offline/ghost/local cohorts retain previous
startup; CoreAudio/WASAPI ports are specified below. Actual physical accuracy,
drift/device/socket execution and formal acceptance remain unverified/deferred.

Calibration requires at least100ms of advancing native host/source observations;
observed slope must stay within the discipline default1000ppm. Initial calibration
is bounded by2s, then readiness/commit and actual crossing use the configured
multiplayer setup timeout. Session bracket freshness uses start-policy max age.
A zero-length finite section uses its exact physical end marker plus native
crossing, because it cannot publish a positive-playback start acknowledgement.

## CoreAudio and WASAPI calibrated initial frame startup

MacOS CoreAudio and Windows WASAPI shared/exclusive solo network paths use the
held-device frame startup contract, actual native clock observations, conservative
future projection, bounded calibration/commit/crossing and original post-arm input
retention. IOHID registry/health/loss and Raw Input foreground cleanup/removal/close
remain explicit. Windows physical stream-clock zero and playback-start coordinate
are separate: converting a WASAPI position never adds the selected playback frame.
PresentationDiscipline::new_with_playback_origin validates same output domain and
playback origin at/after stream origin before observations; default new uses the
same coordinate for both. Desired song phase subtracts playback origin, while
native conversion and source identity retain original physical stream origin.
Network keysound commands stay on logical playback grid even without manual pause.
Synthetic raw-snapshot regression fixtures must preserve physical positions and
zero phase error across a delayed start. ASIO still uses the existing software
start-call commitment; its bounded presentation intervals require separate startup
projection and SDK/device evidence. Offline/ghost/local groups and wireversion6
remain unchanged. Physical accuracy, drift, actual devices/sockets and formal
acceptance stay unverified; tests are authored/compiled without execution.

## Shared native runtime ownership

Native capabilities must be maximally abstracted while preserving their actual
semantics. Multiplayer/gameplay/lifecycle/scheduling policy is shared. A single
native_start::start_committed owns the held-device startup sequence for normalized
pair/counter backends; OS code supplies NativeStartDevice operations/evidence,
not separate calibration/commit/arming/crossing workflows. NativeStartAgreement
adapts shared LiveCompetition; fake native adapters with an actual gated mixer
exercise the same owner without sockets or native hardware. Original evidence
and clock domains remain intact. ASIO interval support stays explicit. Broader
shared gameplay-pump migration remains required; this startup migration does not
claim every native entry point is already thin. See ADR__native-runtime-boundaries.md.

Native presentation metadata may legitimately name a future host coordinate.
The shared owner retains the first qualifying crossing and its original evidence,
continues acquisition/native status/BGM service under the same deadline, and
publishes ready only once actual normalized host_now reaches the interpolated
playback origin. Reject host-domain changes/regression; cancellation preserves
cleanup ownership. Do not replace that crossing with later observations.

## Shared native solo gameplay owner

A single native_gameplay::run_gameplay owns solo native input admission, actual
pause/resume acknowledgements, transport/discipline correction, judge/capture/UI/
competition reports, bounded backlog and finite/full completion policy. Native
adapters supply observation/render/host/acquisition/end/reseed operations only.
Retain original input timestamps and native provenance; loss, removal, capacity,
clock/source changes and cleanup errors remain explicit. Pause resume reseeds
the same native observation source rather than mixing a supplied pair with raw
counters. Apply configured advance_lag 0..1s through one monotonic watermark on
all platforms; Windows must use its parsed lag too. Pending input capacity 65536,
bounded native batches 256, acquisition continues during acknowledgement waits,
judgment waits until native backlog is drained at pause/resume boundaries.
Finite completion requires native boundary plus logical prefix/input drain; full
completion requires actual judge/BGM/mixer/native drain. Caller owns native stop/
join and saving captured prefix after cleanup on all exits. Local cohorts use the shared owner below;
ASIO interval startup remains pending, with no runtime acceptance claimed.

## Shared native local-cohort gameplay

All native local cohorts use one common gameplay owner over RuntimeGroup and
InputMerger. Native adapters acquire bounded original events and observations,
while the owner handles clock validation, pause acknowledgement, ordering, lag,
judgment, each player’s captured reports/score/competition and shared completion.
Acquisition continues during pause/resume acknowledgement waits. A native backlog
prevents judgment/deadline release; actual pause/resume boundaries retain the
ordered input prefix and reconcile releases before post-resume input. Resume
reseeds the same native observation source. Finite completion requires every
member’s logical prefix and an actually committed input frontier crossing the
native endpoint. Full completion requires each member’s actual completion and
drained native/merged input; absent completion metadata is not completion.
Native setup/resource cleanup and independent create-new replay saves remain
with their owners. Linux uses bounded fair sweeps across sources; native Raw
Input/HID queues retain bounded collection and exact device-loss semantics.
Physical timing, ASIO startup parity and native runtime acceptance remain unproven.

## Shared native preparation policy

Live play and replay watching resolve omitted native defaults through one shared
preparation owner. Validate original options before querying native metadata.
Explicit settings retain their exact values and validation errors. Solo live
input may choose an available keyboard automatically; assigned local players
never trigger automatic keyboard selection, and replay watching never queries
keyboards. Native adapters expose default output identity, actual output format,
keyboard candidates and explicit ASIO replay format operations only.

Query only metadata required by omitted fields. Reject unavailable defaults,
bounded-catalog violations and invalid native formats without substituting
parser-only placeholder identities. ASIO has no implicit OS default driver; an
explicit driver remains required. Output routing, clock assessments and exact
buffer requests survive projection. Metadata work belongs to the game owner,
with no device discovery or waiting on UI/audio callbacks. Compilation does not
establish native device availability or physical timing.

## Shared local member preparation and finalization

Local 2..64-player sessions share member preparation, runtime activation and
recording finalization across operating systems. Preparation preserves admitted
native device IDs and stable player IDs, validates bindings/rosters before ghost
loading, and assigns disjoint keysound voices outside the shared BGM namespace.
The same prepared chart, seed, section and rules initialize judges, captures,
record opponents and completion metadata. Group and input merger use the same
actual calibrated transport origin; no fabricated timestamp is substituted.

Native owners stop/join output and close input before common finalization.
Finalization finishes every record opponent and attempts every independent
capture save, retaining session, cleanup and save failures. Destinations match
player identity rather than a truncating positional zip. Recording publication
remains create-new. Shared composition does not erase backend capabilities or
claim native timing, file, socket or desktop acceptance. Fixtures are authored
and compiled only while execution remains deferred.

## Shared native audio construction and BGM supply

Solo and local native sessions share queue, section-relative BGM admission and
mixer construction. The prepared sample bank supplies the exact format/rate;
platform owners supply their real render capacity and supported initial start
gate. Preserve the 1024-command live reserve, explicit output origin, preroll,
lookahead and immutable finite playback endpoint, including zero. Backend buffer
negotiation and clock/calibration evidence remain native capabilities.

Startup and gameplay use one checked BGM replenishment policy: only actual
completed logical playback frames advance its cursor, absent/paused reports do
not supply commands, and each replenishment retains the 256-command budget.
Physical initial silence and pause displacement never shift this logical grid.
Late/capacity/overflow errors and admitted prefixes remain explicit. Setup runs
on the game owner; the abstraction adds no native work inside audio callbacks.
Source fixtures are authored and compiled only; physical playback, files,
network sessions, graphics and formal acceptance remain deferred.

## Shared native chart preparation

All native solo and local sessions use one chart preparation owner for asset
loading, practice-section slicing and required lane-binding coverage. Forward
the exact chart path, actual output format, channel policy and chart seed to the
existing loader. The same supplied PCM limits govern loading and section tails.
Reject negative starts before loading; check bindings against retained notes
after slicing, so excluded heads and crossing holds need no bindings. Preserve
absolute song coordinates, original warnings and actual overlapping BGM suffixes.
Missing required bindings reject during preparation before completion reporting.

Preparation does not publish a chart or eagerly load BGA images. Keep the existing
player publication/cancellation stage at each native resource boundary: Windows
solo selects its input and bindings first, Linux solo binds first, and macOS
solo/local publishes before opening input. Existing headless publication avoids
image loading. Native device ownership, exact roster identities and cleanup remain
unchanged. Author real-component regression fixtures for request forwarding,
section/binding boundaries, PCM tails and error propagation; execution and physical
acceptance remain deferred.

## Shared native judging and capture configuration

Native solo and local sessions share application policy for the grade-one judge
profile, optional full-song completion, recording limits and pristine replay
capture. Keep exact early/late windows and signed input offset. Full-song
completion uses the actual chart and PCM; finite sessions skip that preparation
and retain their existing endpoint validation. Disabled recording does not
validate ignored capture values. Enabled recording retains caller byte/record
budgets, a 4096-byte header budget and input codec bounds of 65536/32768.

The same core JudgeEngine continues to own judging. Preserve capture clock domain,
original-song section start, chart seed and existing replay bytes. Solos prepare
completion before chart publication, profile/judge after publication/cancellation,
and capture inside the cleanup-protected outcome after native devices open.
Local cohorts prepare their profile once and clone it for independent judges;
member capture/completion still precede opponent loading. Device selectors,
sound voices, recording paths and native cleanup remain with their existing
owners. Author actual judge/runtime/capture/replay and completion-boundary fixtures;
compilation does not establish runtime or physical acceptance.

## Interval-preserving native startup

Committed ASIO network starts must use the common held-device startup owner with
actual output-grid observations and their complete host before/after bounds.
Validate calibration consistency against the explicitly assessed rate band, then
project the committed target uncertainty through that entire band. A past average
does not tighten future instantaneous-rate bounds. Retain the whole frame range
and select its latest ceiling frame beyond the rendered frontier plus actual
buffer. The assessed rate bound is an assumption consistent with observations,
not measured drift or an acoustic accuracy guarantee.

Preserve original native evidence, sample rate, frame origin and render identity.
At the first actual gate crossing, retain a bounded host window derived from both
bracketing observations and wait for native host time to reach its upper endpoint.
The transport's nominal anchor is distinct from this retained uncertainty. Coarse
host plateaus are permitted when bounds remain consistent. Point-based backends
keep their existing projection. Configure pause/end frame grids before arming;
seed ASIO end and discipline using the original interval observation. Initial
network gating does not enable manual network pause. All ASIO gameplay uses
the common logical render-grid scheduler, so held startup silence never shifts
keysound commands by the physical start-frame offset, and manual pause silence
does not shift logical keysound scheduling.

Use actual applied ASIO buffer metadata from prepared stream evidence, never a
second requested-size estimate. Reject stale, incompatible, inconsistent or
unrepresentable evidence instead of inventing point samples. Author portable
interval and real gated-mixer startup fixtures for later execution. Ordinary
source/GNU checks do not cover the SDK/MSVC driver branch; SDK compilation, device,
network and physical timing acceptance remain deferred.

## Shared solo session finalization

All native solo sessions use one common finalization policy after their output
stop/join and input close/drop attempts. Finish competition before saving the
actual captured prefix. Attempt recording publication even when gameplay or
native cleanup failed; preserve the original capture, destination and exclusive
create behavior. The save status reflects session/cleanup errors. Return the
original first error in gameplay, output cleanup, input cleanup, then save order.
Preserve native diagnostics and original error identity; common finalization
must not replace native resource-retention or cleanup semantics. Local cohorts
retain their existing all-members policy and share the same capture publisher.
Author failure-priority and actual-capture forwarding fixtures using in-memory
publication callbacks; compilation does not establish file/device acceptance.

## Shared asynchronous text clipboard

All seven existing editable targets use the same clipboard transaction: search,
native setting value, profile path, display value, practice start/end and record
directory. Exact Ctrl+C/X/V on Windows/Linux or Command+C/X/V on macOS matches
logical characters; additional modifiers are excluded. Consume shortcut text
before ordinary insertion. Repeats do not submit again; IME composition owns its
keyboard. Copy/cut with no selection do nothing and do not initialize a backend.
Copy/cut preserve the selected UTF-8 bytes exactly. Paste replaces selection
without trimming or normalizing and uses existing byte/control validation.

Native access belongs to one lazy, persistent worker using desktop-only arboard
3.6.1 text support (no image feature), with one outstanding operation. The UI,
audio and game owners never call the native clipboard. Validate a cut candidate
against its model before writing; delete only after successful write and current
model validation. Settings editor and cloned NativeSettings publish together.
Backend, payload or validation errors preserve the draft and appear in the
existing error region.

A pending edit belongs to the exact screen instance, field and editor snapshot.
Screen/field changes, loss of readiness/focus, IME composition and any editor
change cancel its publication. Recheck on completion; a cancelled operation still
drains before another starts. An already submitted copy/cut write cannot be
recalled. Completion polling occurs only while work is pending; navigation and
suspend retain the clipboard owner. Closing drains and joins it before exit.

Reads are rejected above 4096 UTF-8 bytes after the native backend allocates its
String, then enforce the target's smaller limit. This is not an allocation bound
on arbitrary external clipboard data. Native operations lack a hard timeout and
can delay shutdown; no bounded native-latency guarantee is made. Linux requires
persistent clipboard ownership; survival after app exit depends on the desktop's
clipboard manager. Wayland uses arboard's supported data-control protocols with
its X11/XWayland path where available; unsupported desktops report errors.
Browser clipboard adapters remain separate work. Fake-backend/editor/desktop
fixtures are authored and compiled only; real clipboard/IME/close acceptance,
tests, review and QA remain deferred.

## Interval acknowledgement for recorded playback

Recorded ASIO playback uses the same NativePause and ReplayPause state machine
as point-based outputs, while retaining its original output-grid/render evidence
and complete assessed host interval. Never turn the discipline midpoint or
prepared-frame counter into pause presentation evidence. First valid use binds
one evidence kind for the session; mixed point/interval calls reject.

Recover the first physical pause/resume frame from validated mixer reports.
The request and first actual crossing observation bracket that frame. Exact
anchor equality uses its original interval; otherwise retain the conservative
[request.before, crossing.after] window without inventing a rate or precise
interpolation. Keep the first window through delayed polls, duplicate anchor
refresh and later blocks, and acknowledge only after fresh host time reaches
its latest endpoint. Metadata, chronology, report identity, arithmetic and
inconsistent grids reject atomically. Missing observations preserve evidence.

Replay freezes the exact song position derived from the boundary playback frame,
recorded start and preroll, independently of host uncertainty. Resume preserves
the cumulative once-rounded pause gap. Queued physical presentation older than
the actual resume frame cannot update visuals after the gap changes. ASIO's
bounded presentation queue accepts coherent paused/resumed grids while retaining
original maturity deadlines; it still reports physical output for final drain.

The common native replay loop consumes typed point or interval evidence without
platform branches. Audio commands, recorded operations and completion remain
fenced while pausing/paused/resuming; native polling and cancellation continue.
Control requests and observations are staged together; evidence validation and
checked song projection precede publication of state, capability and the mixer
request. Rejected evidence cannot issue a pause request or partly change phase.
Interval diagnostics retain both endpoints rather than claiming the upper
acknowledgement deadline is an exact acoustic timestamp. No live input, new
recorded operations, capture format or networking is introduced into Watch.

Live/local ASIO uses the conservative input policy and exact logical freeze in
the next section. A point-based Transport cutoff alone is insufficient evidence.
Network pause remains unsupported. SDK integration is Windows+asio-sdk+MSVC;
ordinary GNU checks do not compile that branch. Pure actual-Mixer, interval,
replay and native-loop fixtures are authored and compiled only. Tests, driver,
GUI, device, physical timing, formal review and QA execution remain deferred.

## Exact live pause with interval evidence

Solo and local cohorts consume the same typed native pause observation and
shared pause coordinator. Point backends retain their existing host cutoff.
ASIO supplies the original coherent render report, output origin, sample rate
and complete assessed host interval; correction midpoints cannot replace it.
An unavailable observation retains pending evidence, and fresh host time may
acknowledge an already established window. Capability publication follows actual
admitted evidence. Requests and checked boundary-song projection are staged
before issuing the sole producer's mixer command.

For interval evidence the software input policy is conservative: pause uses
the window's earliest endpoint as its input cutoff, while resume uses its latest
endpoint. Inputs strictly before the pause cutoff follow their original runtime
path. Later inputs update paused key levels without judging. On resume, earlier
inputs remain paused levels; reconciliation releases precede equal-time and
later original inputs. These are explicit software cutoffs, not claims about
the exact acoustic boundary. Full windows and original input provenance remain
available. New paused presses remain suppressed until release.

The frozen song position is the original song origin plus the once-rounded
logical playback-frame duration. A staged Transport pause and seek establishes
that exact position while preserving historical mapping before the cutoff.
Resume retains it and rebuilds presentation discipline using the cumulative
manual pause gap. Every local member shares this transport and pause boundary;
input ownership, merger order, lag and per-member captures remain unchanged.

Already committed judge history cannot rewind. If any member has progressed
beyond the exact frozen song, a queued pre-cutoff input maps beyond it, or the
selected cutoff conflicts with committed Transport chronology, terminate through
the existing cleanup path while retaining committed reports and captures.
Never silently move judge history backward to hide a timing conflict. Interval
observations alone cannot repair such a conflict; native acceptance must assess
these rejection cases as well as successful pause/resume.

The shared Windows adapter retains original ASIO evidence for resume seeding,
including when the old correction discipline awaited host progress. It seeds
the new discipline from that evidence without requiring equality with an older
midpoint pair. WASAPI retains its actual snapshot and equality checks. Offline
solo/local ASIO uses logical playback scheduling so silent pause frames do not
move keysound scheduling. Startup displacement and finite-marker precedence
remain intact. Network pause remains separate work.

Implementation and compiled fixtures do not establish runtime acceptance.
SDK/MSVC compilation, driver output, physical timing, tests and formal review/QA
remain deferred under the standing user instruction.

## Browser selected-file preview

The optional `browser` feature now adds selected-file preparation and a dedicated
Worker that presents the actual chart and static BGA on an OffscreenCanvas. It
reuses the common chart/audio/image preparation, visible-note query, GPU cache
and BGA composition inside this application crate. Explicit preview time does
not establish browser audio playback or a gameplay clock.

The [browser host contract](REQ__bms-browser.md) defines relative resource lookup,
input budgets, worker ownership, replacement/lifecycle rules and remaining scope.
Browser audio/presentation evidence, input clock mapping, actual judging,
capture/replay and network adapters remain required work. Source compilation,
generated bindings and browser execution have separate acceptance requirements;
this phase does not close the player Goal.

## Admitted contact pressed feedback

Visible pressed lanes derive from actual admitted bound input, independently
of hit or miss judgement. Preserve separate button and contact ownership using
full source, physical surface/control, game control and contact ID. Touch Down
adds one owner; duplicate Down is idempotent, Move does not acquire or relocate
it, and matching Up/Cancel removes only that owner. Unknown releases do nothing.
A button and contact, or two contacts, may share a lane; it stays pressed until
the final matching owner releases. Never synthesize keyboard events or re-hit-test
raw touch coordinates for presentation.

The existing common bounded ownership component supplies browser live feedback,
native member publication and recorded replay presentation. Whole input batches
retain atomic capacity refusal, and clear releases all visual ownership while
retaining reusable storage. Playback-prefix presentation follows only recorded
operations and must match live ownership transitions. Compilation and authored
fixtures do not establish actual browser/device acceptance.

## Selectable-radix audio preparation capacity

The shared application's default original PCM sample capacity is3844, covering
the two-character base62 namespace (zero stays an empty chart row token). All
native solo/local/offline/replay caller presets use the same exported constant;
64MiB per asset and256MiB total decoded bank bounds remain unchanged. Explicit
caller-supplied tighter PcmLimits remain authoritative. Section preparation keeps
its existing bounded addition of at most4096 crossing BGM suffixes; original
numeric sample identities and original-song times remain intact.

Browser Worker original preparation uses a shared JS3844 constant and Worklet
handoff admits at most7940 original-plus-suffix samples. Match the Rust preset;
this is a bounded cross-language compatibility contract, not a new runtime
allocation or gameplay loop. Update actual Worker launch fixtures, and author
portable real preparation/section tests with >1295 and up to3843 referenced
samples plus4096 tails, explicit low-count rejection before decode, byte-bound
rejection and unchanged keysound identities. Test execution, generated bindings,
browser/device/audio/performance and ordered QA remain deferred.

## Owned direct-chart title-font startup

For --chart with --title-font, create the native window/event loop without
reading the font file or preparing its glyph atlas on the UI thread. Extend
NativeCatalog with a CPU-only preparation constructor using its same cancellation,
nonblocking progress/poll and owned join/drop rules, without scanning the chart's
parent directory. The existing library constructor must delegate through the
same owner and preserve actual scan budgets/progress.

Direct preparation preserves the original literal chart path, filename title
and empty artist; it does not claim to parse chart metadata. Prepare its search
and bounded font using the same helper/checkpoints as library preparation.
While the job is pending, selection has no published entries/font or Start hit;
Settings/Exit and native lifecycle stay usable. Install complete entries/search/
font together only after real join into active Selection, respecting suspension,
hidden screens, close and GPU texture preflight. Font error/cancellation never
publishes a partial direct selection or silently proceeds without requested font.
Without a supplied font, preserve the cheap direct filename selection path.

Thread spawn failure is an explicit preparation error; no synchronous I/O
fallback. Final close cancels and joins the same owner. Startup profile
preparation uses the owned stage below; GPU/OS syscalls are not force-interruptible.
Author independent actual owned-job and Desktop fixtures for direct operation
without directory scan, original path/title/search/font, worker-thread ownership,
pending gates, failure, suspension/hidden return, cancellation and joined cleanup.
Scoped Rust formatting and exactly four compile-only checks after both terminal
STOPPED finals are authorized; no assertion/test, app/device/runtime or formal
review/QA execution under the standing deferral.

## Owned startup profile before renderer initialization

Read and decode --profile through the existing NativeCatalog::spawn_prepared
CPU owner, including its bounded 72 KiB regular-file loader, cancellation
checkpoints, nonblocking poll and actual join/drop. Preserve native profile
values overlaid by explicit CLI native flags (excluding chart) and presentation
values overlaid by explicit CLI display flags. Chart/library/font selection
remains external and literal. No synchronous profile I/O fallback.

Create the native window/event loop without waiting for profile I/O. While
startup is pending, use the window title for loading status, create no renderer,
GPU instance, selection/font job or native audio/input owner, publish no
selection and admit no settings/navigation/play. OS close and nonrepeat Escape
cancel startup; focus, resize, occlusion and suspend still route normally.
No temporary renderer using CLI defaults may precede the profile's actual GPU
backend/presentation. A plain startup without --profile retains existing paths.

After real join, apply the complete prepared Options only to eligible Selection.
Suspended/hidden/occluded UI retains the joined-ready owner until eligible;
closing consumes/discards without install. Set active backend from the fully
prepared profile/CLI result, then create its renderer and begin existing
selection/font preparation. A startup error is fatal and closes/joins, with
no default-settings success fallback. Poll only while ownership is pending;
completed idle UI waits for events. Close waits for the startup owner too;
unexpected exit cancels and joins it. Cooperative cancellation cannot forcibly
interrupt an OS filesystem call or later GPU initialization.

Author independent real profile-file and Desktop owned-worker fixtures for
version compatibility/CLI precedence/literal selection, missing/malformed/
wrong-host/oversized profiles, pending input/start/GPU gates, hidden/suspended
return, cancellation/close and exactly one join/install. Add source-linked
fixtures without opening windows/devices. Scoped formatting plus exactly four
compile-only checks after both terminal STOPPED finals are authorized; actual
assertions, app/device/runtime and formal review/QA remain deferred.

## Owned native renderer preparation

After profile/CLI preparation, create the native wgpu instance and window
surface on the window owner, then transfer their owned handles to a cancellable
preparation thread for actual adapter/device/pipeline initialization. The UI
must not `block_on` renderer preparation. Reuse the existing owned preparation
worker with nonblocking completion polling and exactly one actual join; no new
crate or executor framework. Return the instance and renderer together.

While preparation is pending, retain the native window with a loading title,
fence gameplay Start, navigation, stale hits and drawing, and keep Escape,
close, focus, resize, suspend and occlusion handling reachable. Catalog/font
preparation may run independently but must not publish before renderer adoption
on an eligible active surface. Resize and atlas upload use current UI state at
adoption, not stale size or font snapshots. Publish renderer/font ownership
only after that adoption preflight succeeds; initialization/adoption errors
close explicitly without a silent backend or default-presentation fallback.

Hide/occlusion retains a ready owned result until eligibility returns. Suspend
retires and cancels the preparation generation; its delayed success/error must
be discarded after join, even after resume. Resume starts fresh preparation
only after the retired owner has joined. Close cancels and waits for renderer
preparation alongside all existing owners; unexpected exit also joins. Late
results may never resurrect a closed/replaced surface. Poll only pending
ownership, retaining event-driven idle behavior after completion.

Known ceiling: native instance/surface creation and adoption resize/texture
upload remain window-owner calls. Cooperative cancellation cannot interrupt a
driver call already in progress. Actual cross-platform surface/device/thread
behavior and responsiveness require deferred device/GPU acceptance.

Independent source fixtures must use real owned-thread/channel gates for
pending input/navigation, error/retirement/close/hidden retention and replacement
isolation, without inventing a fake successful GPU renderer. Actual successful
GPU creation/adoption remains a later integration run. Both writers must return
terminal STOPPED before scoped format and exactly four compile-only checks;
assertions, native/browser execution and formal review/QA remain deferred.
# Live channel matrix settings

ASIO common-output composition must renew finite multimedia-clock anchors from
fresh, same-QPC native timer brackets before their validity horizon expires.
Renewal retains caller-supplied age/measurement/drift bounds and never resets
host chronology to conceal a regressed reading or a different clock domain.
The SDK-free checked renewal policy is shared with the Windows live path;
its portable week/wrap fixtures are not native driver or acoustic evidence.

The Linux paused-live output panel exposes OUTPUT CHANNEL MATRIX. Each output
channel is a semicolon-delimited row; comma-delimited coefficients address the
original source channels in order (`1;0.5` maps mono to two channels). Accept at
most 32 equal-width rows/columns, finite f32 values and 4096 bytes with no
controls. No speaker-position inference or clipping-policy choice is hidden.
Empty preserves the current matrix; explicit `exact` clears it and returns
native channel count to the original Mixer source count. Rate remains fixed.

Validate grammar/dimensions before the output owner retires the current stream.
The native adapter retains the accepted matrix as metadata; successful replies
advertise canonical round-trippable coefficients and actual native dimensions.
Editing only device/buffer/period preserves that matrix. Existing original
pause/input/clock/lifecycle ownership remains. Rejection leaves the old live
output and transport intact under the normal output-request policy.

This field is live-output-only: it is not added to initial native arguments or
saved profile schema. Windows solo/local nonnetwork WASAPI play uses the common
owner and paused-boundary policy for device/buffer/period/matrix fields. Its
mapper preserves the current shared/exclusive mode and negotiation policy.
ASIO solo/local observation also uses that owner while retaining the native
HWND/message pump and original interval evidence. ASIO nonnetwork live controls
expose buffer and same-width matrix changes for the currently trusted driver
and selected channel order. Reopening preserves the original clock error bounds
and registry view; pending native cleanup retains its stream and attempt HWND.
Driver/channel-selection and cross-backend controls remain pending.
Output capability arguments declare the exact editable fields: the UI must not
invent a WASAPI period or device selector for an ASIO buffer-only capability.
Network/Watch output controls remain unavailable.
macOS solo nonnetwork play uses the common output owner
and paused-boundary policy for device/buffer/matrix fields; cohort/network/Watch
capabilities remain unavailable until their owner integration is complete.
Matrix selection/preservation/reset rules belong to common domain functions.
CoreAudio rates and original frame basis remain fixed across reopen. The actual
stream reports startup acknowledgement even when bootstrap used its raw handle.
Source/target rates cannot be changed here.
Pure parser/mapper/retained-panel tests and an actual ALSA null request diagnostic
are separate from physical-device/audio-sync acceptance and full player completion.
