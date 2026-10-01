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
draw/input/transition routing. Actual GUI lifecycle acceptance and reactive
menu/widget toolkit migration remain unfinished.


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

This lifecycle integration does not select or introduce a reactive UI toolkit.
Retained reactive menu/widget binding remains a separate implementation step.
