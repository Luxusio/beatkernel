# Browser input dispatch isolation

Status: selected implementation contract. This work does not complete the
parked audio-authority migration or its actual-browser acceptance.

Keyboard, HID, touch and pointer callbacks capture their original samples and
send already acquired input. Their dispatch path must not synchronously query
Gamepads. Sample eligible live Gamepads on the existing 8 ms cadence immediately
before pumping input, with no additional timer. Explicit source discovery and
preflight verification retain their existing polls.

Polling requires the current play owner and generation, live Playing mode, no
outstanding input tick and no input pump already active. Preserve bounded
snapshots, native timestamps, actual source identities, shared acquisition
sequences, projection and whole-batch validation. Polling borrows the existing
reentrancy guard: nested input capture may queue originals, but cannot send a
partial batch during the poll. Restore the guard on every exit. Cancellation or
failure must prevent stale reading/publication and use existing cleanup rules.
Replay and retired owners never acquire live Gamepads. Pending ticks suppress
polling so unchanged samples cannot grow the Window queue behind a blocked ACK.

Input admission remains separate from the acknowledged partial prefix. Keep the
existing timing policy and original metadata; do not retimestamp, widen bounds,
silently drop events or claim an OS delivery guarantee. Pending input continues
to block page remapping and completion. The parked startup and late-touch
failures remain unresolved.

The first retained rendering primitive is a read-only borrowed view of changed
`NoteProgress` pages relative to a retained acknowledged snapshot. Require the
same prepared chart allocation. An unchanged shared directory returns an empty
iterator without allocation or page scanning; changed directories scan bounded
page pointers and yield current changed pages in order. Expose page index, valid
note extent, derived completed count and immutable packed states. Last-page
padding stays zero. `last_miss` is a separate scalar; an empty page delta does
not imply identical complete presentation state.

Compare against the last fully acknowledged snapshot so coalescing cannot omit
changes from skipped frames. Do not copy the whole chart or Scene every frame,
add global chart identities to generic progress, or introduce Virtual DOM.
Transport registration and receiver validation belong to the separately queued
renderer integration; this primitive does not itself create another Worker.

Verify callback Gamepad read counts separately from eligible cadence reads;
test pending ticks, reentrant acquisition/cancellation, polling failures, replay,
retired owners and original mixed-source metadata. Rust fixtures use real
compiled unique object identities and accepted judge events across multiple
pages, including no-op, holding/completion, cumulative deltas, partial padding,
empty charts, exact identity refusal and immutable baselines. Mock and source
results are not browser/device latency measurements.

## Separate renderer integration requirements

The next integration must run gameplay and graphics in different Workers,
connected directly by a MessageChannel. Window must not relay per-frame chart,
Scene or progress snapshots. Audio observations and genuine completion remain
gameplay-owned; renderer acknowledgements have no completion authority.

Register immutable visual chart/image content once, including mines, BGA and
opacity timelines, aliases and unavailable resources. Subsequent updates contain
bounded committed display scalars and changed COW note pages. Validate the whole
receiver update before publishing any player or scalar state, including ordered
page ranges, counts, reserved states, padding and the receiver's local chart.
Retain one immutable pending snapshot; adopt precisely it after its complete
state acknowledgement and coalesce later changes against the last full baseline.

State application and successful geometry submission are separate evidence.
Retain original touch acquisitions through page and resize transitions, with
their original time, coordinates, dimensions and contact mapping. Rendering
stalls and transient retries must not gate keyboard/HID/gamepad acquisition,
gameplay input/audio acknowledgements or capture. Zero extent cannot establish
that a new page is visible.

Local fresh-contact routing must use the acquisition page stored on the same
ordered queue entry as its projected position. Setting the current page before
queue admission is insufficient: the entry may be dispatched after another page
change. Apply its retained page at actual dispatch, preserving held/unbound
contact ownership. Reusing the same page must not allocate or remap unrelated
members. Feed these inputs through the ordinary prompt queue; do not hold them
in a separate renderer-wait buffer behind an already closed input prefix.

Local rendering converts recent feedback only for the at most four visible
players. Reuse four bounded prepared scratch buffers across redraws; do not
copy hidden members' recent events or allocate full-roster view/reservation
vectors each frame. Keep complete stable roster identities, original page
numbering and comparison reservations through borrowed fixed-size views.

Both Rust/WASM admission and the JavaScript channel enforce the same envelope
sequence rule: registration, history, completed Results and room snapshots use
zero; live frames and preview updates use a positive u64 sequence. Reject an
invalid sequence before payload copying or publishing presentation identity.
Refusal must preserve the prior presentation and generation/sequence floors.

Explicit preview/live/local/replay registration mode is admitted in the same
transaction as its roster and visual content. Preview requires no player,
live/replay exactly one, and local one to 64; a one-player local cohort remains
local. Incompatible mode refusal must leave the prior presentation and floors
unchanged. Never validate mode through a fallible setter after publication.

Results page/comparison choices accepted during previous-owner retirement must
survive replacement registration. Apply the latest validated choice through
the existing renderer control before its submitted geometry is reported; a
successful UI reply must not leave the renderer at the default detail page.
Cold Results waits for its first valid page/comparison control before drawing,
including when the desired selection is the default. A combined room packet
or resize does not unlock a pending Results selection.

A confirmed terminal runtime graphics error retains the existing failed-stop
behavior: gameplay exports genuine recorded prefixes and authoritative results,
then Window joins input/audio/room cleanup before gameplay ownership terminates.
Do not terminate the game Worker before delayed capture delivery, label an error
natural completion, or discard genuine capture. Historical/completed/room display
errors remain presentation-only. Headless continuation has not been selected.
The shared render byte codec must enforce the trusted caller's aggregate UTF-8
diagnostic allowance for image errors, nested frame room errors and every frozen
room page error before allocation/publication. Preserve original strings;
decorative headings, labels and counters do not consume diagnostic allowance.
The public self-reported progress validator checks counts and optional prefix
transitions without admitting membership or proving completion.

Verify these requirements with actual separate Workers as well as pure
registration/progress tests. Integration is still under development.
