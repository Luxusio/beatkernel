# Separate browser acquisition, gameplay and rendering ownership

Status: integration in progress in `TASK__browser-render-worker`. The gameplay
Worker now exports visual-only packets; a separate renderer implementation owns
`BrowserView`. Window startup/lifecycle wiring is an implemented candidate;
whole-application browser acceptance remains unfinished. Component checks do
not establish deployed isolation.

Window owns browser-required input acquisition, permission gestures, DOM
configuration and acquisition geometry. The gameplay Worker owns Runtime,
judging, gauge, recording/replay operations, competition and audio authority.
The rendering Worker owns OffscreenCanvas, Scene construction, GPU
submission, immutable chart presentation data and BGA texture caches. It never
advances gameplay or samples a song clock independently.

At registration, transfer bounded immutable chart/BGA metadata and selected
image assets once. Bind a registration identity to that content and the current
session/generation. Process-local Arc identity only validates a producer's
snapshot baseline; it is not an identity shared across Workers. Receiver
validation checks registration, sizes, page order/ranges, reserved states,
padding and shape before applying an update.

Send committed scalar visual state and changed packed note pages, rather than
rebuilding or diffing a virtual tree or serializing a whole chart each frame.
Keep one bounded immutable snapshot in flight. The receiver acknowledges only
after the complete snapshot is applied; the sender then adopts precisely that
snapshot as its baseline. While blocked, later changes coalesce against the
last complete ACK, including all changed pages across skipped visual frames.
Never coalesce or discard acquired input. Immutable COW pages reuse the current
`NoteProgress` ownership, with explicit chart/page/asset budgets.

Stale generations, malformed or partial application never produce a success
ACK. Retain the last acknowledged baseline through timeout or render failure;
report an explicit renderer error or register a new generation. Renderer failure
does not fabricate gameplay success, discard capture or replace audio evidence.
Preserve the existing runtime policy: rendering stalls and transient surface
retries leave gameplay/input/audio service running; a confirmed terminal live,
local or replay graphics error initiates a failed, capture-preserving stop.
Gameplay must deliver its correlated capture and cleanup outcome before Window
terminates its ownership, including asynchronous room cleanup. Static history,
completed and room presentation errors retain their presentation-only semantics.
Headless continuation is an optional unselected behavior; no answer to the
earlier question is inferred. Test stalled rendering separately from terminal
failure and from an explicit user Stop.

Resize and local-page changes carry a geometry/version barrier. Preserve each
input's original acquisition geometry while waiting for the required page or
surface acknowledgement; do not reinterpret a retained touch using a later
layout or discard newly acquired contacts. A state acknowledgement proves atomic
application; a geometry acknowledgement proves submission for that page/surface
version, not physical scanout. Zero-size surfaces suspend drawing without
inventing time or successful visible-page evidence. Loading,
ready, playing, stopping, error and disposal each retain one clear owner. Stop
retires outstanding generations and joins only owned resources before reuse.

Geometry evidence contains the exact admitted generation, content and positive
geometry version, plus the applied page and nonzero backing width/height. Read
the page from the validated renderer presentation after successful submission;
never copy a requested page into this evidence. Window retains the frozen tuple
for the matching play/selection owner. Each local touch carries that acquisition
page with its original CSS/backing dimensions. The ordered input entry retains
page and projection together and applies the relevant touch router at dispatch,
preserving held/unbound contact ownership. Input service continues while visual
state or geometry acknowledgements are pending.

A new presentation generation establishes a fresh correlated geometry version
even when backing dimensions are unchanged. Reusing the prior renderer extent
does not let a new client adopt the preceding owner's submission acknowledgement.
Keep versions monotonic over the renderer lifetime and preserve the ordering of
already reserved resize/page changes.

The planned Window defaults admit at most 577,598,288 packet bytes: the
309,162,792-byte conservative cold registration bound, a separate 256 MiB
aggregate diagnostic allowance, and the 40-byte wire header. The allowance is a
validation ceiling, not a preallocated buffer. Kind-specific smaller limits and
the existing decoded-image limits still apply. Reject oversized diagnostics
explicitly rather than truncating stored presentation data. Initialization and
individual channel operations have a 10-second deadline. Final acceptance must
report retained producer/receiver/copy/index/GPU peak memory separately; a wire
ceiling alone is not a memory or latency guarantee.

The conservative peak estimator is `peak_visual_transport_bytes(cold_bytes,
note_count, roster_count)` in `app/src/browser_render_state.rs`. It accounts for
six encoded cold representations, positive decoded-record expansion across
three chart owners, three copies of note indexes/timeline staging and image
metadata, and two decoded-image/GPU banks. Progress accounting retains
`3 * roster_count + 8` snapshots: current/ACK/receiver state for all members,
four pending sender members and four staged receiver members. Each snapshot
counts packed pages plus directory/Arc metadata and bounded scalars; two encoded
frame buffers are additional. At most four fields, 2,048 visible notes and 2,048
visible mines per field reach the existing renderer admission boundary. Surface
and depth attachments, static presentation caches, PCM, browser implementation
storage and non-visual gameplay state are separate. This estimator does not
promise that the maximum admitted combination fits a particular browser/device;
actual allocation/device failure follows the explicit failure/cleanup contract.

At the existing one-million-item/64-player maximum, the conservative committed
frame bound is 1,073,008 bytes, plus its 40-byte wire header. The progress/scalar
snapshot allowance including two encoded frame representations is 57,102,816
bytes. These are upper bounds for the selected representation, not routine
allocations or measured live usage; unchanged pages and skipped frames retain
the cumulative acknowledged baseline rather than allocating full progress anew.

The integration covers preview, solo/live, local cohorts, replay, historical
records, completed results and room results. A solo-only transport is not the
completed design. Actual two-Worker tests must prove input/audio acknowledgements
continue while rendering is blocked, cumulative deltas remain bounded, stale
messages and partial failures cannot alter gameplay, and all navigation modes
preserve their current observable data.

This staged approach reuses immutable pages and retained geometry. Full Scene
transfer per frame would duplicate work and allocation; a Virtual DOM would add
an unnecessary reconciliation layer. The producer exposes read-only page data;
the renderer consumer adds atomic validated imports and the bounded wire codec.
Chart identity remains local to each owner, with explicit transport
generation/content identities.
