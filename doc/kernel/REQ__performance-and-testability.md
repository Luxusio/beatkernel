# Performance and deterministic testability

## Product objective

BeatKernel's lasting product objective is to build the world's lightest and
fastest high-performance rhythm-game player core and player. This is a target,
not an achieved ranking. Comparative claims require published workloads,
hardware, configurations, versions, measurements and reproducible evidence.
Correct timing, input provenance and deterministic replay remain requirements
when optimizing latency, CPU, memory, startup and distribution size.

## Layer boundary

The user explicitly requires complete separation of UI Layer, Business Layer
and Native IO Layer with natural dependency injection. These are dependency and
ownership boundaries, not a requirement for three additional crates.

UI owns retained presentation and translates user actions into typed commands.
It consumes read-only business state/results and must not perform judgment,
device policy or direct driver/network/storage operations. Business owns
validation, judgment, state transitions, completion, replay and protocol policy.
Native IO owns device/OS access and physical effects; browser IO adapters have
the equivalent acquisition/effect role. Business defines narrow interfaces and
evidence values it needs; UI and IO adapt to those contracts. Business cannot
import concrete UI or IO implementations to choose or instantiate them.

The application composition root supplies concrete ports at construction or
explicit operation boundaries. Tests inject deterministic implementations
through those same entry points. Avoid hidden globals, thread-local service
lookup or platform branches as substitutes for injection. Legacy player
thread-local control/publication access belongs only in the outer compatibility
bridge; fully injected gameplay entry points receive
explicit command/publication ports. Remaining concrete adapters still require
audit against this rule. Backend conversion stays in IO while backend-independent
presentation/time algorithms belong in the domain.
UI lifecycles and back-stack decisions remain testable independently of graphics.
IO lifecycle/cleanup failures cannot fabricate business completion evidence.

The kernel and shared game policy must be testable without an operating-system
device, real clock, sleep, filesystem, network, renderer or audio driver.
Platform adapters acquire original evidence and perform effects. Business code
validates evidence and decides transitions through explicit ports. A port must
permit deterministic values, failures and admission limits in tests; moving an
OS call behind a module name alone does not satisfy this rule.

Archive-set publication follows this boundary through opaque destination values
and associated errors supplied by a generic port. Whole/member validation,
staging, duplicate refusal and error precedence are portable policy; original
native path construction and file writes stay in the adapter. See
[the publication contract](../runtime/REQ__archive-publication-port.md).

Competition progress polling and publication use a generic port with owned
notice iterators, borrowed original member data and associated errors. Shared
policy validates state/gates; native adapters translate transport notices and
perform publication. Group publication cadence now receives an explicit generic
clock, with exact portable nanosecond state and post-effect interval commit;
native clock acquisition lives in a per-owner adapter. Low-level endpoint ownership
and underlying room/ACK waits still require further separation. See
[the progress port contract](REQ__competition-progress-port.md) and
[its cadence contract](REQ__competition-progress-cadence.md).

Solo/group terminal orchestration separates borrowed final delivery, cleanup and
post-cleanup notice draining through a generic port. The outer native adapter
performs the ownership copy/join and preserves room controller completion
authority. Shared policy retains every outcome and the first original opaque
error; it cannot create a successful delivery or gameplay proof from cleanup.
Saved-opponent preparation now consumes opaque resource keys and loader errors
through a generic port. Whole-count capacity preflight precedes acquisition;
each supplied decoded recording still passes actual competition reconstruction.
Native file reads and path formatting live in the adapter. Loader failure keeps
the explicit previously accepted prefix, not an atomic batch claim. See
[the opponent loader](REQ__competition-opponent-loader.md).

QUIC credential metadata validation and ordered byte acquisition use portable
policy with an injected reader. Original Path keys and owned byte buffers cross
the boundary without policy formatting or copies, and opaque read errors retain
their identity. The native reader owns regular-file checks and bounded filesystem
reads; TLS decoding and sockets remain native. WebTransport destination/Origin
metadata validation and single-CA preparation use the same generic reader, with
original option borrowing and unchanged build availability. Full endpoint
construction now receives a preflighted request through a generic factory.
Native room availability is supplied at the outer boundary; policy preserves
original owned identity/roster vectors and opaque factory errors. Concrete
connection ownership and waits still need further boundary work. See
[credential preparation](REQ__multiplayer-credential-loading.md)
and [WebTransport preparation](REQ__webtransport-preparation.md).
The [connection factory](REQ__competition-connection-factory.md) defines the
pre-acquisition validation and ownership boundary.

Bilateral final acknowledgement waiting uses generic notice/admission and
clock/park ports. The fixed exact deadline is never renewed, and cancellation,
disconnect and ACK evidence retain their precedence. Scalar/group native methods
adapt their real notices and queues to this policy; native clock acquisition and
thread parking live in a private bridge. Room final/drain waiting now similarly
uses generic observation/command and wait-control ports, with separate original
room/control deadlines and actual admission, acceptance and receipt gates.
Room startup now uses a generic initial/poll observation port, service callback
and narrow 1 ms wait port. Cancellation, Leave and terminal history refuse before
Commit; native UI effects now route through a generic owned RoomUiHost. The
fully injected controller constructor receives both network and UI ports; its
legacy constructor supplies actual player UI effects through an outer bridge.
Native network/owner internals, result building and cleanup diagnostics remain
unfinished separation work. Actual joins/cleanup errors
and underlying worker waits remain native.
See [final ACK waiting](REQ__final-ack-wait.md) and
[room final/drain waiting](REQ__room-final-wait.md).
See also [room startup waiting](REQ__room-start-wait.md).
The [room UI host](REQ__room-ui-host.md) defines request/publication injection.

Solo terminal delivery selection and its one-shot guard are pure; observation
and start requests cannot revive a finalized owner. Underlying endpoint ownership
and whole live-owner construction remain follow-up work beyond the injected
acquisition factory. See [the terminal contract](REQ__competition-terminal-port.md)
and [solo lifecycle](REQ__solo-competition-terminal.md).

Clock domains and physical input/output provenance cannot be substituted with
test/control time. Native and browser adapters use common business rules.
Use static dispatch or direct value inputs where sufficient; do not add heap
allocation, per-note virtual dispatch or new crates solely to enforce layering.
The user's explicit implementation preference is zero-cost abstraction wherever
practical. Prefer generic ports, monomorphized adapters and direct typed values
on judgment, input, audio and rendering hot paths. Introducing a trait boundary
must not itself require allocation, locking or per-note dynamic dispatch.
Dynamic dispatch or ownership allocation on setup, UI construction and storage
paths needs a concrete purpose and should remain outside real-time callbacks.
This is an implementation rule, not proof of zero overhead everywhere: evaluate
code size, compile time and runtime costs with evidence before claiming gains.
Real-time audio callbacks must not gain allocation, blocking I/O or locks.
Bound queues and work, and make overflow, refusal and cancellation explicit.

## Reliability target

The user requests SQLite-grade test stability. Adopt the relevant methods from
the [SQLite testing documentation](https://www.sqlite.org/testing.html):
independent harnesses, fault injection, boundary and generated cases, coverage,
regression cases and recovery testing. This is a testing strategy and long-term
quality target, not a claim of equivalent existing coverage or reliability.

Each layer needs an independent reference or invariant rather than assertions
that repeat its implementation. Pure domain tests cover time arithmetic,
ordering, judgment, gauge, replay, scope and lifecycle transitions. Port tests
inject clock faults, queue admission/refusal, partial progress, cancellation,
storage failures and reconnects. Adapter contract tests check original evidence
and error propagation; real-platform tests separately cover drivers, graphics,
browser workers and network interoperability. Generated failures retain their
seed and inputs as reproducible regressions. Crash/recovery tests must verify
record integrity once persistence paths are connected.

Compilation is not executed test evidence. Test count alone is not a quality
metric. Track covered transitions, branches, failure sites, invariant coverage,
repeatability and platform/configuration gaps. Do not mark an unexecuted suite
or an unmeasured performance target as passed.

## Performance evidence

Measure input-to-judgment and presentation latency distributions (including tail
and maximum), audio underruns, frame deadlines, CPU, memory and allocations.
Use sparse and dense charts, long sessions, restart/pause, solo/local/online and
native/browser configurations. Record backend, buffer, display and workload
parameters. Set numerical budgets from recorded baselines and product latency
requirements; arbitrary numbers or average FPS do not establish superiority.

## Loading responsiveness requirement

The user explicitly requires loading to feel immediate; slow startup, screen
transitions and repeated chart loading are product defects to measure and
reduce. This is a required outcome, not an achieved performance claim.

Measure native and browser cold startup separately from warm startup, first
usable submitted frame, legal screen transitions, chart parsing, media
acquisition/decoding, preparation and repeat loads. Record workload size,
asset format, platform, backend and cache state, including tail and maximum
latencies. Derive numerical budgets from recorded baselines and product needs;
finite filesystem, network and decoding work cannot be represented as zero time.

Keep input acquisition and visible UI responsive during preparation. Reuse
accepted immutable assets and existing retained geometry where identity and
lifetime permit; avoid redundant reads, decoding and geometry reconstruction.
Use existing workers and owners rather than introducing a general loading
framework. Preserve readiness, cancellation, failures and original ownership:
a loading indication or cached preview cannot authorize incomplete gameplay.
Expensive work must have accurate progress and cancellation rather than an
unresponsive UI. Any resource cache must have explicit memory limits and
invalidation; responsiveness does not justify unbounded preloading.

Loading optimization and representative native/browser latency measurements
remain pending under TASK__player-loading-latency. Existing motion correctness
checks and software-browser screenshots are not loading-speed benchmarks.

The 2026-10-10 read-only source audit identified pending loading improvements:
browser library import currently awaits every supplied File's bytes before
reporting its chart list (`app/web/worker.js`); preparation reuses encoded
MemoryFiles but its decoded-audio cache is local to one preparation call
(`app/src/browser.rs`, `app/src/audio_assets.rs`); canonical WAV aliases avoid
another decode but still clone PCM for distinct SampleIds. Prioritize early
metadata/chart-list publication and bounded warm reuse before changing PCM
ownership. These are source observations, not measured elapsed times. Existing
gapless practice restart uses a separate retained path and must not be described
as reloading every asset.

## Current implementation boundary

Separation is unfinished. Core runtime processing telemetry currently defaults
to a native `Instant` clock; deterministic tests must select its existing
`RuntimeProcessingClock::Disabled` or scripted `External` mode. Telemetry timing
is distinct from judgment timing. Native pump wall-clock deadlines and waits use
an explicit control port. The fully injected solo/cohort entry points also
receive a business-owned host for commands, publication and typed diagnostics;
the outer compatibility bridge selects legacy player/system adapters. Shared
pumps now receive a business-owned device contract with an associated injected
presentation implementation. The actual
bounded observation ring, freshness, drift/phase calculation and continuous
transport correction now belong to the pure core presentation estimator; the
platform wrapper validates native source metadata and delegates calculation.
Gameplay sessions and resume reconstruction use the presentation port; native
trait specialization and platform port implementation live in the compatibility
bridge. See [the presentation port](REQ__gameplay-presentation-port.md) and
[the estimator boundary](REQ__pure-presentation-estimator.md).
Their generic sessions now accept business-owned solo/group competition observers; concrete
native specializations live in the compatibility bridge. The native competition
implementations still own network, preparation/storage and terminal
diagnostics. Their outer solo/cohort setup waiting now delegates to a shared
business start gate with explicit readiness/polling and opaque control-clock
ports; network release time stays separate. Group network publication cadence
uses an injected clock through portable policy; underlying room-adapter waiting
still uses native time. See
[the start gate](REQ__competition-start-gate.md). Competition display payloads,
projection and 50ms cadence
now live in a pure policy using an injected presentation host; native UI and
display-clock effects live in its bridge. Actual native publication delegates
to that policy, with explicit host-injected observation/publication entry points.
This does not make the remaining native network owners pure. See
[the display boundary](REQ__competition-presentation-port.md).
Those adapters, file/network boundaries and adapter coverage require continued
audit and separation. See [the competition ports](REQ__gameplay-competition-ports.md)
and [the host boundary](REQ__native-gameplay-host.md) for
its exact scope and remaining effects. Native completion now returns typed
solo/per-member results through injected hosts and retains an atomic registered
roster table in player snapshots. The native UI now validates the whole completed
table and caches formatted labels and page geometry. It displays that retained
view only after cleanup acknowledgement, bypassing timed playfield and BGA
selection on the completed screen. Final score, timing and comparison prefixes
are frozen with that first accepted table. Detail and comparison page changes
reuse prepared packets; original IDs, recording extents and self-reported peer
status remain explicit. This is source-level work, not measured
rendering or performance evidence. See [the results screen](../ui/REQ__completed-results-screen.md).
Browser source now captures actual stepped completion into a separate retained
presentation before freeing gameplay. The Worker owns rendering and requests;
the Window acknowledges presentation only after its existing cleanup path settles.
See [the browser results requirement](../ui/REQ__browser-completed-results.md).
The common completed-result archive now separates historical storage values from
live completion evidence. Its codec and one-effect save/load policy accept an
injected storage port; the native file adapter remains outside business policy.
Native solo/local application save paths now retain the typed pump result and
use shared finalization with generic callbacks after device cleanup attempts.
Pure association receives borrowed player/capture/profile data, while sidecar
filesystem publication remains in the native adapter. Finite capture headers
retain their original endpoint. See
[the native save contract](../runtime/REQ__native-completed-result-save.md).
Browser source now performs cold Rust archive export on the Worker before
consuming captures and transfers one bounded whole-roster artifact. The Window
uses a pure finite admission model and retains opaque bytes; IndexedDB stores
replay/archive associations in the existing transaction, charging both byte
lengths. See [the browser storage contract](../runtime/REQ__browser-completed-result-storage.md).
Native catalog preview now uses pure exact header/member association and displays
stored gauge/outcome independently of reconstructed prefix scores. New local
recordings receive adjacent original-member sidecars. Browser loaded historical
results use a separate Worker-owned cached presentation. See
[native association](../runtime/REQ__historical-record-association.md) and
[browser history](../runtime/REQ__browser-historical-record.md).
Explicit selection/migration for old native local files, rich archived
score/timing/comparison values and runtime acceptance remain unfinished. See
[the archive boundary](../runtime/REQ__completed-result-archive.md).
Browser execution remains unfinished. The earlier instruction to defer
verification has been lifted: executable tests and feasible native/browser QA
are active. Outstanding hardware QA and representative performance benchmarks
remain unverified; authoring fixtures or passing compile-only checks does not
establish a quality or performance target.

Room network values and the command/poll/clock/stop port belong to the portable
[room network contract](REQ__room-network-model.md). Native aliases retain API
compatibility and Arc sharing; the worker owns its actual thread and stream.
The [portable room controller](REQ__portable-room-controller.md) uses these
values and all three injected hosts. Native defaults and physical start
compatibility live outside its unconditional module. Cold retained result
construction remains controller policy; browser adapter integration and worker
IO still require continued development.

The [room runtime host](REQ__room-runtime-host.md) separates control-clock and
sleep construction plus implicit-drop reporting from controller policy. Its
associated controls are statically selected; native fixed deadline construction
and diagnostic output belong to the adapter. This does not establish physical
timing or peer completion. Native start compatibility and defaults belong to
the separate compatibility module rather than the portable controller.

The [shared room network actor](REQ__room-network-actor.md) owns caller-driven
protocol state and retained snapshots over an injected stream port. Its native
worker delegates commands and observations to that same actor; acquisition,
queue/reply credits, locking, actual time and thread join stay in the adapter.
This is a code boundary, not evidence that a real adapter never blocks or that
browser integration and physical timing are complete.

The [split-operation room client driver](REQ__room-client-driver.md) separates
the existing WASM room client's protocol lifetime and decoder/revision state
from JS materialization. The actual binding delegates to it, retaining real
async write-completion authority in the Worker transport. This is a separate
integration step from a browser stream adapter or the shared room controller.
