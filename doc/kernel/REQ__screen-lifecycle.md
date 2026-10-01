# Graphical screen navigation and lifecycle

The single app uses MVVM-like presentation state: ViewModels expose UI state
and commands, Views bind to changed state, and models own gameplay/session data.
The navigator owns screen lifecycle and back-stack admission. Gameplay notes
have no individual ViewModels or reactive bindings. Navigation has one typed active route. Draft storage does not decide
which screen receives input or renders. A screen transition is admitted before
its new data is installed and before old screen data is released. Failed
admission leaves the active route and drafts unchanged.

Selection may open Settings or native Play. Settings may open Display, Records,
Players or an output device picker. The Players keyboard picker retains its
Players parent; other settings children retain the Settings/Selection ancestry
without a Players draft. Back exits the
current child and resumes its immediate parent. Sibling children cannot replace
one another directly. Apply commits a validated draft before returning to
Selection; Back discards it. Records Watch enters replay Play after validation
and native-owner creation. Child exit releases data outside the new route's
ancestry and clears gestures/hit targets; stale controls cannot cross screens.

Only a joined native owner may enter Results or leave a play route. Results
retains the final snapshot and immutable invocation; retry starts a new owner of
the same live/replay mode after cleanup. Failed spawn retains Results. A pending
metadata operation fences navigation except application close. Close enters a
terminal Exiting phase, cancels native play and drains game and metadata owners
before process exit. Late metadata completion cannot open another screen.

Platform suspend enters Suspended while retaining route/drafts and cancelling
native play through its existing owner contract. Resume returns that route to
Active after window/graphics recreation. Focus loss disables input and cancels
play; it does not discard screen state. Resize changes the surface, not the
route. Rendering and UI dispatch use the navigator's active route rather than
precedence between optional data fields.

This is a bounded explicit navigation model, not a generic callback/plugin UI
framework. Native/audio resources remain owned by their existing threads and
cleanup contracts; screen exit cannot bypass those joins. Pure navigation
fixtures and source compilation do not establish GUI lifecycle acceptance.
Actual GUI/OS event and formal review/QA execution remain user-deferred.


## Back stack and toolkit boundary

Back navigation belongs to the application navigation model. Opening a child
retains its parent's draft and selection; Back releases the child and restores
that exact parent. Replacing a route and retrying a play do not append duplicate
history entries. A transition that fails validation/resource preparation must
not mutate the back stack. Returning from Results to Selection releases the
joined play owner rather than resurrect an already-finished Play route.
Closing invalidates navigation and UI scopes immediately. Application-owned
game and metadata resources remain until their workers drain.

Widget layout, text editing, focus and controls should use a suitable existing
Rust toolkit where integration is clearer than retaining custom widget plumbing.
A toolkit's widget/event lifecycle does not own BeatKernel audio/input clocks,
game-owner cancellation/joins or record compatibility. Keep navigation/back-stack
and domain models independent of toolkit and use wgpu for the timed playfield.
The user confirms Svelte-like, fine-grained updates as the primary UI policy.
Keep the view/widget tree alive across normal frames. A state change invalidates
its dependent bindings and the necessary layout/paint work; it must not rebuild
the complete menu tree merely because the playfield renders another frame.
For example, a changed selected chart updates its dependent title/metadata and
selection appearance; an unrelated audio counter does not rebuild the settings
form. Geometry changes may require ancestor layout and repaint, so this rule
is not a claim that only one widget or pixel can ever be processed.

Runtime signals/effects satisfy this policy when dependency updates are scoped;
the implementation need not copy Svelte's compiler. Audio/input callbacks do not
write UI signals directly. The main thread adapts immutable game-owner snapshots,
updates changed presentation values and keeps native gameplay timestamps intact.
Navigation/back-stack state and lifetime fences remain independent of toolkit.

An immediate-mode egui menu refreshed at playfield cadence is not the primary UI
architecture. Floem is the leading candidate because its official documentation
specifies a retained, constructed-once view tree with fine-grained signals and
effects, and Windows/macOS/Linux rendering over wgpu. Floem is MIT-licensed.
[Official Floem documentation](https://docs.rs/floem/latest/floem/) and
[upstream repository](https://github.com/lapce/floem) describe these capabilities.
Its runtime reactivity is not identical to Svelte's compiled updates and gives
no performance guarantee for this app. Released-package custom playfield/host,
WASM and build compatibility still need inspection before dependency migration.
Slint's tracked property bindings also fit the update semantics, but its
licensing needs a separate distribution decision; no default GPL UI dependency
is introduced into the ASIO-free MIT distribution by this requirement.

The confirmed decision is update semantics; framework adoption is not complete.
The desktop now uses the typed Navigator and retained instance back stack for
draw/input/transition routing. Actual GUI lifecycle acceptance remains unfinished. Selection has the retained
binding described below; other menu/widget toolkit migration remains unfinished.


## Scoped reusable panels

Independent panels such as Records, Players and Devices own their typed draft
state and task-cancellation scope. Small controls and the GPU playfield inherit
the containing screen lifetime. There is no lifecycle state machine per button
or note. Panels request navigation from the single Navigator; they do not own
separate global navigation stacks.

The Navigator retains actual screen entries with stable, non-reused instance
IDs. Opening a child hides its parent without recreating the parent's draft.
Back resumes that same parent entry. Closing Settings releases its children
before the Settings scope. Entering Play from Records releases that entire
menu branch; Results retains completed play data, and retry creates a fresh
play entry rather than adding duplicate history.

Every metadata operation captures the initiating screen ID and cancellation
permit. Work checks cancellation before beginning and before returning its
result. Dropping a scope cancels its permits; completed results are admitted
only for the still-active initiating instance. Closing invalidates all UI
instances, cancels permits and drains already-running operations. A filesystem
call already in progress remains non-interruptible and must be joined before
process exit. Suspended UI retains drafts and defers metadata presentation until
resume; Closing still drains workers.

Ordinary transitions are rejected while suspended or metadata is pending.
Data preparation and worker spawn complete before route commit; failure retains
the old route and drafts. Cleanup must never drop a running native play owner.
Native owners remain application session resources and publish their final
snapshot after join before Results, retry or return to Selection.

This lifecycle boundary is independent of the UI toolkit. Selection now uses
the standalone reactive engine below; other menu/widget binding remains a
separate implementation step.


## Retained reactive selection screen

The first migrated screen is Selection. Its fixed header, instructions, row
slots, diagnostics, buttons and status nodes are created once per retained
ScreenInstanceId. The standalone floem_reactive 0.2.0 engine tracks signal
dependencies; memo equality prevents unchanged rows/buttons from repainting.
Changing a selected-row highlight updates dependent rows, while page/visible
range changes may update all row slots. Unrelated status changes cannot rebuild
the catalog-row nodes. No application batching scheduler or custom signal engine
is introduced. Scope disposal releases signals/effects when its screen leaves
the Navigator, and retained parent return reuses that scope.

Scene composition concatenates existing node geometry when dependencies change
or another screen overwrote the surface scene. An unchanged selection redraw
reuses the composed scene; idle Selection waits for input/window events rather
than refreshing at gameplay FPS. GPU rectangle uploads require a changed scene
identity/geometry epoch; equal counters from different scenes cannot alias.
Timed playfield uniforms retain their independent actual-song-time updates.
A changed node currently causes a full composed rectangle buffer upload; partial
GPU subrange updates are not implemented.

The published Floem 0.2.0 host uses wgpu22/floem-winit0.29.5, unlike this app's
wgpu27/winit0.30.13. Its separately released MIT reactive engine depends only on
smallvec and is used without introducing that host. See the
[published Floem manifest](https://docs.rs/crate/floem/0.2.0/source/Cargo.toml) and
[reactive engine documentation](https://docs.rs/floem_reactive/latest/floem_reactive/).
This is actual dependency tracking and retained node rendering for Selection;
other menu screens and full existing-widget toolkit migration remain pending.
Source checks do not prove GUI behavior or performance; execution remains deferred.

Event-driven Selection retries drawing at the configured cadence after transient
surface acquisition timeout, outdated configuration or lost-surface recreation.
It enters idle Wait only after a presented frame (or a zero-size suspended
surface), so a recovered surface cannot wait indefinitely for unrelated input.


## Practice panel lifetime

Practice is an independent Settings child with typed draft/editor/view ownership
and one retained reactive scope. It uses the existing three-entry ancestry
Selection/Settings/Practice and the same bounded Navigator graph. Back drops
child edits and resumes the retained Settings instance. Done validates the exact
position and prepares the updated parent values/editor before committing the
route. Failure retains the child and original parent draft. Buttons/input nodes
inherit the panel scope; they do not own separate navigation lifecycles. Native
play/replay owners and transport remain outside the panel lifetime.


## Retained reactive Settings screen

Settings retains one reactive scope and stable node tree per Navigator instance
while children are active. Each visible row subscribes to its own field and only
the selected row subscribes to editor/cursor; message/error changes cannot paint
rows. Profile focus/editor, selected hint, page count and button state are separate
dependencies. Model changes from child Done, device use, Add Binding or profile
load update existing field signals, preserving repeated option ordering.

View updates borrow the bounded draft, compare before cloning changed values,
and never clone the full field collection on pointer redraw. Fixed field signals
cover the existing 128-field bound; visible geometry remains ten row slots.
Pending metadata disables controls and hit regions, while the owner continues
polling. Completion must queue a redraw before the event loop returns to idle
Wait, including load/save failures with no user input. Clearing cached hit regions
forces restoration even when no signal changed. Child Back restores the retained
Settings scope and scene; branch exit disposes subscriptions.

Display, Records, Players and Devices use the retained panels below. Full
widget-host migration and actual GUI/performance acceptance remain unfinished
and user-deferred.


## Retained Display child

Display owns one retained view scope per Navigator instance. Its four editors
(GPU backend, presentation mode, FPS and lookahead) have independent signals;
focus updates affect the old/new field, and errors cannot repaint fields.
The view borrows draft editors and compares before cloning changes. Done
validates and updates the parent Settings presentation draft; Apply remains
separate. Back discards Display edits. Failed Done preserves the child/parent.

Display uses a reusable retained node/immutable packet primitive on the UI
thread. The primitive handles ordered geometry and dirty composition; the view
still owns Floem scope disposal. It has no native I/O, gameplay clock, navigation
stack or custom signal/batch scheduler. Pending state disables all hits and idle
Display follows the existing event-driven/surface-retry rules. Other menu
migrations and GUI/performance acceptance remain unfinished and user-deferred.


## Shared retained node composition

Selection, Practice, Settings, Display, Records, Players and Devices use one RetainedNodes
implementation for packet construction, dependency binding, dirty state and ordered composition.
Views still own their Floem scope, signal graph and Navigator instance identity;
only the containing view disposes its scope. The shared primitive never owns
navigation, native sessions, clocks or a batching scheduler. It checks initial
packet errors before a view is published; later composition errors remain
explicit and cannot clear dirty status. Scope disposal and retained Back behavior
must preserve the existing dependency and geometry/hit order contracts.

## Retained Records panel

The Records view retains a main-thread Floem scope per Navigator instance and
uses the shared retained node primitive. Directory editor, ten visible rows,
preview, controls and status have separate dependencies. Returning Back or
starting Watch releases the Records scope; metadata workers still belong to
the coordinator and carry its existing cancellation permits. Pending operations
disable every hit region. Accepted completion requests redraw before idle Wait.
No filesystem scan, preview reconstruction or native playback runs in view effects.

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

## Practice restart lifetime

A live session bookmark belongs to Game and survives fresh retry replacement
of that session. F8 shares F5's prepared invocation, cancellation, final-snapshot
drain and joined-owner replacement boundary. Preflight failure retains the
current session and bookmark. Closing and explicit cancellation discard prepared
replacement, and failed cleanup cannot auto-start another owner. UI nodes own no
audio seek state or clocks. F5 continues to use the original pinned start.

## Selection search focus

Selection search retains its app-owned query and original catalog identities
across Settings Back and fresh sessions. Text editing is admitted only while
Selection is active and its search field is focused. Every route commit clears
search focus. Filtered hit IDs still identify original catalog entries, and
hidden/nonmatching entries cannot be selected through stale click IDs.
