# Graphical screen navigation and lifecycle

The single app uses an MVP-like structure: the desktop coordinator accepts UI
intent, models hold product/session state, and existing scene components draw
snapshots. Navigation has one typed active route. Draft storage does not decide
which screen receives input or renders. A screen transition is admitted before
its new data is installed and before old screen data is released. Failed
admission leaves the active route and drafts unchanged.

Selection may open Settings or native Play. Settings may open Display, Records,
Players or an output device picker. The Players keyboard picker retains its
Players parent; other settings children retain Settings only. Back exits the
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
Closing clears navigation only after game and metadata owners drain.

Widget layout, text editing, focus and controls should use a suitable existing
Rust toolkit where integration is clearer than retaining custom widget plumbing.
A toolkit's widget/event lifecycle does not own BeatKernel audio/input clocks,
game-owner cancellation/joins or record compatibility. Keep navigation/back-stack
and domain models independent of toolkit and use wgpu for the timed playfield.
Framework selection is not decided yet: egui is a candidate for integration into
the existing winit/wgpu host; iced is a candidate if adopting its app/state/message
architecture. No framework dependency or host migration is authorized solely by
this candidate comparison. The current typed route model is a foundation;
desktop integration and back-stack acceptance remain unfinished.
