# Separate browser acquisition, gameplay and rendering ownership

Status: selected staged design. The current gameplay Worker still owns
`BrowserView` and borrows live/local/replay owners to draw. This document and the
borrowed note-page primitive do not claim that actual renderer isolation exists.
The full integration remains queued as `TASK__browser-render-worker`.

Window owns browser-required input acquisition, permission gestures, DOM
configuration and acquisition geometry. The gameplay Worker owns Runtime,
judging, gauge, recording/replay operations, competition and audio authority.
The future rendering Worker owns OffscreenCanvas, Scene construction, GPU
submission, immutable chart presentation data and BGA texture caches. It never
advances gameplay or samples a song clock independently.

At registration, transfer bounded immutable chart/BGA metadata and selected
image assets once. Bind a registration identity to that content and the current
session/generation. Process-local Arc identity only validates a producer's
snapshot baseline; it is not an identity shared across Workers. Future receiver
validation must check registration, sizes, page order/ranges, reserved states,
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
The eventual failure policy must distinguish continued headless gameplay from
an explicit user-requested stop and be tested in that integration.

Resize and local-page changes carry a geometry/version barrier. Preserve each
input's original acquisition geometry while waiting for the required page or
surface acknowledgement; do not reinterpret a retained touch using a later
layout. Zero-size surfaces suspend drawing without inventing time. Loading,
ready, playing, stopping, error and disposal each retain one clear owner. Stop
retires outstanding generations and joins only owned resources before reuse.

The integration covers preview, solo/live, local cohorts, replay, historical
records, completed results and room results. A solo-only transport is not the
completed design. Actual two-Worker tests must prove input/audio acknowledgements
continue while rendering is blocked, cumulative deltas remain bounded, stale
messages and partial failures cannot alter gameplay, and all navigation modes
preserve their current observable data.

This staged approach reuses immutable pages and retained geometry. Full Scene
transfer per frame would duplicate work and allocation; a Virtual DOM would add
an unnecessary reconciliation layer. The current producer primitive therefore
exposes read-only page data without an unused importer, serialization format or
new global chart-ID field. Actual transport and consumer validation are added
with their first real renderer Worker consumer.
