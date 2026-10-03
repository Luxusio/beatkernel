# Physical touch-region binding

Touch input selects a logical control from a configured region when the contact
begins. Preserve the complete original TouchEvent, including source, surface,
contact ID, acquisition time/sequence, coordinates, pressure and provenance.
Never rewrite it as a keyboard/button event or replace event time with arrival.

Regions use the chosen hit-coordinate units (adapter coordinates by default)
and half-open rectangles: minimum inclusive, maximum exclusive. Bounds are
finite with positive extent. Reject overlapping
regions for the same device selector and physical surface. Exact device/surface
rules override Any rules over that whole surface; gaps remain unbound.

A contact is identified by source, physical surface and contact ID. Its original
destination remains until matching Up or Cancel, even after movement outside
the region. Duplicate Down keeps that destination. A Down outside every region
remains unbound for its lifetime; later movement cannot acquire a lane. Unknown
Move/Up/Cancel is ignored. Other devices/surfaces/contacts cannot release it.

Configured touch surfaces never fall back to ordinary BindingMap fanout.
Non-touch events and unconfigured surfaces remain available to other bindings.
An explicit projected hit position may be supplied separately when presentation
coordinates differ from acquisition coordinates. It selects the initial region
without overwriting the physical sample. Both original and projected positions
must be finite; subsequent samples retain the original contact destination.
Invalid samples and capacity failures must preserve prior routing state.
Finite coordinates and optional pressure are required without imposing new
normalization units or pressure ranges.

Setup accepts at most 256 regions and a contact capacity from 1 to 4096.
Reserve all contact storage during setup. Routing is bounded by configured
regions and active contacts, independent of chart size. The router produces
owned GameInputEvent values without allocating routing storage per event.
This is a source contract, not measured latency or audio-callback suitability.

Clearing routing state does not synthesize cancellations or finish judge holds.
The gameplay owner must coordinate cancellation/fresh-session restoration.
Actual Runtime/StepGameplay/browser integration must validate acquisition clocks,
chronology and finite endpoints before adopting routes, retain partial failure
reports, and explicitly restore routing ownership for any resumed live contact.
This component alone does not establish playable browser touch support.

## Runtime admission and restoration

Opt-in Runtime routing runs after input/output clock mapping, acquisition
sequence and host/song chronology checks, before committing acquisition time.
A router refusal preserves judge, routing, sequence and input-counter state.
Configured touches select one destination or stay unbound; other inputs retain
ordinary BindingMap behavior. At a finite endpoint, skip routing and preserve
the same capped judge advancement and output/report semantics.

Judging or audio publication failures after admission retain the actual bound
input and routing state. Report the committed prefix; never roll it back or
retry automatically. Cohort member selection and solo execution share this path.

A fallible routing clone includes outside contacts and original destinations,
with full configured capacity. Restoring a live contact requires a coherent
judge, transport and routing checkpoint supplied explicitly by the caller.
Fresh replace_state/replace_session clears held contacts and retains region
configuration. Explicit replacements may install a paired routing checkpoint;
normal replacement must never carry unrelated old contacts into a new timeline.
Routing checkpoints do not synchronize native audio or authenticate a recording.
