# Dedicated native acquisition and completed input evidence

Status: source integration and independent local review/QA passed; native
hardware acceptance remains unproved.

One native input collector must acquire independently of UI, output processing
and gameplay across existing Linux evdev, Windows Raw Input and macOS IOHID
solo/local compositions. The admitted player/device selection remains unchanged:
solo automatic selection and exact nonoverlapping multi-player assignments.
Native affine objects are constructed, polled, closed and dropped on their
collector owner thread. The winit main event loop stays on its existing owner.

The shared statically dispatched source port transports original owned
PhysicalInputEvent values without changing EventMeta, native provenance,
source IDs, sequence or timestamps. Existing startup retain/discard phase
boundaries and gameplay pre-origin count/drop remain unchanged. This structural
change does not decide the pending domain policy for future preplay handling
and does not add raw-packet archival.

Transport and native servicing have explicit finite entry, payload/byte and
work budgets. Publication never blocks waiting for gameplay. Capacity loss,
native loss/detach, worker panic/disconnect and cleanup failures must be visible;
events cannot be silently overwritten or discarded as successful admission.
Cancellation and fatal status remain observable even when data transport is
full. Startup failure, gameplay failure and ordinary completion must close
on the source owner and join the worker before another session can start.
Preserve the acquisition failure as primary with cleanup context. An earlier
close error or close panic takes precedence over a later destructor panic;
destructor failure becomes the cleanup cause only when close succeeded.

Collector completed-drain observations are separate from gameplay receipt
time. A completion marker follows every event it covers in the same bounded
FIFO. The consumer can expose that cut only after consuming preceding covered
events. InputBatch carries an optional completed-through ClockPoint: absence
cannot advance deadlines or the audio authority's acquired prefix. Receipt
host_now remains for validation, telemetry and output correlation. Transfer
queue emptiness and later consumer clock samples never prove native draining.
The raw drain observation is not a timestamp fence on future native delivery:
a delayed event can predate a previous observation. Preserve that timestamp;
InputMerger applies the existing lag and committed-frontier refusal. The
collector validates drain chronology without adding a stricter late-input rule.

Transport byte accounting covers queued entry footprint and owned payload
capacity, including reserved unused Vec capacity. The separately finite entry
limit also bounds channel slot reservation; this is not allocator instrumentation
or a claim that all acquisition operations allocate nothing.

Every selected source must justify a conservative common cut with original
domain and chronology. Linux uses actual nonblocking drain observations;
Windows uses the acquisition-owner message drain; macOS requires every selected
checked native queue to report empty. Runloop timeout and an empty callback
buffer alone are insufficient. An unavailable proof
withholds advancement. Existing lag, deterministic InputMerger admission,
pause/resume/finite completion and output authority remain strict.

macOS runloop timeout alone is not proof that HID ports are empty. A checked
queue path must distinguish actual native underrun from dequeue/conversion
errors and publish a cut only after every selected queue reports empty.
Existing callback APIs remain compatible. Detectable application/transport
overflow remains fatal; underlying kernel/driver loss is not completely
observable and no universal physical loss-detection guarantee is claimed.

Verification must exercise actual spawned-thread construction/destruction,
acquisition during a gated consumer stall, exact event equality, partial FIFO
delivery and withheld cuts, invalid/stale domains, all-source completion,
bounded overflow, cancellation, panic/error/cleanup and startup handoff. Native
adapter fixtures and supported-host source checks supplement shared tests.
Mock or foreign stub execution never establishes native hardware correctness,
physical latency, scheduler guarantees or best-in-world performance.

## Successful termination and final delivery

Successful worker termination does not prove that the transport FIFO is empty.
Every final event and completed-drain marker must be delivered before the
consumer reports exhausted closure. A drain returning any FIFO record, or
retaining transport backlog, reports `closed=false`; only a successful terminal
drain with no records and no backlog reports `closed=true`. A zero-item budget
cannot declare closure while transport backlog remains.

The startup handoff follows the same rule for retained events and completion
evidence: transferring either keeps that acquisition open even when the
collector itself is already exhausted. The next empty acquisition can close.
Original metadata and cuts are delivered exactly once and never retimestamped.

This does not change immediate abort semantics of a native adapter's explicit
`InputBatch.closed`, or turn EOF into successful song completion. Fatal source,
capacity, panic and cleanup errors remain immediately visible. Missing or stale
audio observations at EOF cannot manufacture a mapping, acquired frontier,
judgment or completion; the owner performs only bounded final delivery and
exits incomplete when the next acquisition is exhausted.
