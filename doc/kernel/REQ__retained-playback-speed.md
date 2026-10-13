# Retained practice at fixed positive speed

This is the behavior contract for the active retained-speed implementation.
It is not evidence of implementation, QA PASS or completion of WBS05.13.
It extends [fixed live speed](REQ__live-playback-speed.md) and
[gapless practice](REQ__gapless-practice.md) on the original audio owner.

## Selected speed and output ownership

Use one immutable positive reduced NUM/DEN for the retained session. Pitch
changes with speed. Physical sample Hz, native buffers, HOST acquisition units
and the output epoch retain their existing meanings. Solo and local cohorts
use the same prepared core program; all members share the original output cut.

Restart, scrub and loop retain the same original PCM bank, Mixer, consumer,
converter history, unread converted PCM and native endpoint. They do not
reopen output, copy/resample sample suffixes, create another clock or duplicate
platform business logic. Existing NORMAL constructors and recording bytes
remain compatible; nonnormal support uses explicit validated opt-in APIs.

## Original and output coordinates

For original start S, physical output preroll P, positive rate N/D and active
output elapsed E, song time is S + trunc((E-P)*N/D), with checked wide arithmetic
over the whole signed phase. Adding independently rounded elapsed time to an
initial integer song snapshot is invalid. P is not scaled a second time.

Initial cues and region boundaries use the inverse of that same phase on the
actual output frame grid. A finite region ends at the first actual output
sample crossing its exclusive original-song end, using checked Ceil mapping.
Cues at or beyond the exclusive source end do not execute in that pass.
Source-frame selection always uses immutable original samples and reports the
existing requested/applied source time and Ceil correction.

The opt-in initial Started receipt requests S and applies
S-trunc(P*N/D), with the exact signed preroll correction. It describes the
initial snapshot; it must not replace the already prepared whole-phase
transport with a rounded anchor. Later fresh-attempt receipts have no new P.

Core finite durations use their checked frame representation. Timestamp
representability depends on the actual output origin and physical cut,
including pauses; a duration above i64::MAX nanoseconds is not alone proof
of overflow when the original output origin is negative. Validate the final
qualified timestamp before installation or callback mutation.

Each fresh attempt anchors its selected-rate transport at the original actual
qualified retained boundary, with zero new output preroll. Repeated loops and
backward scrubs select from original coordinates, never from a previously
rounded/sliced attempt. Physical/playback counters and converter phase remain
monotonic; original song position may restart only through the existing
validated receipt/generation transition. Pause inserts physical silence while
freezing playback and sample heads; resume retains the selected rate.

The phase port must carry the attempt/basis identity and relative active
source duration alongside the existing normalized ClockPoint. Compute one
elapsed-nanosecond floor from the original integer/fractional active source
position before applying the selected rate. Converted target observation can
overshoot the source cut by a proper source-frame fraction; retain that real
fraction rather than treating the first target sample as elapsed zero.
Chronology, raw audio scheduling and input provenance retain their existing
clock meanings. A pure phase value does not establish native presentation or
input admission. Transport and Runtime must share their original judging and
command paths, with explicit phase opt-in and legacy NORMAL compatibility.

Converted timing must preserve the exact physical source relation through
active output, held output and target-rate replacement. Ordinary contiguous
active blocks may share one relation; held output freezes converter phase.
Retaining only the latest render report must not erase an earlier relation
needed by an acquired input. Preparation allocates bounded history before
callbacks; lack of capacity must refuse before generating new PCM or pulling
source commands. Existing history-disabled conversion remains compatible.
Enabling history after generation must refuse rather than reconstruct missing
relations from rounded timestamps. Complete converter ownership retains the
history across moves and replacement.

This history describes generated correspondence only. Native qualification
must additionally preserve the actual positive submitted prefix and original
presentation coordinate on its creation epoch. Held output alone grants no
mixer pause acknowledgment. History retirement requires proof that retained
observations and pending input/control no longer need the removed interval.

Native observation ports must carry the original output counter/frequency or
stream-relative frame from the same observation as its existing clock pair.
WASAPI retains its driver counter; ASIO retains the original rendered block and
HOST bracket rather than inventing a hardware counter. ALSA retains the
submitted-minus-delay frame estimate and its Unknown accuracy; CoreAudio uses
the original integer frame and keeps floating-point sample metadata diagnostic.
Existing snapshot literals remain compatible. New coordinate wrappers and
decoded generated spans validate their mathematical structure; authority must
still qualify original epoch, basis, evidence and actual submitted range.
The existing finite audio authority keeps each original pair and optional
native coordinate in one bounded queue. An opted epoch has an immutable
source-frame, source-counter/frequency or target-frame basis. Coordinate
progress may be retained even when rounded nanoseconds are identical; an
unchanged coordinate never refreshes observation age. Scalar and coordinate
observations cannot mix. Descriptor-less scalar mapping and epoch replacement
must fail closed until their exact counterparts are implemented. Metadata
admission alone grants no generated, submitted or presentation qualification.

An acquired HOST timestamp must first map to exact native output duration and
then to the applicable active/held segment. Interpolating source endpoints
across held output would move frozen source phase. Wider interpolation
denominators must not trigger rounded timestamp inversion or scalar fallback.
Exact input preparation retains the original acquired HOST point, raw output
point and normalized chronology separately. It selects the existing retained
interval under acquisition, freshness and extrapolation policy and compares
prediction limits before flooring. Its frozen mapper has Unknown physical
accuracy and provides a mathematical HOST-to-logical relation only. A later
validation must recheck owner state and freshness before native source/phase
qualification; preparation alone cannot consume input or commit gameplay.
The interpolated physical presentation point for an input is separate from
the actual raw scheduling point used to enqueue keysounds. Phase integration
must retain the existing audio scheduling result rather than substituting
the input's historical presentation coordinate.
Covered past input may retain a future right latency association when the
existing policy has fresh already-due evidence; preparing it must not require
every retained association to be due.
Native frame admission retains its original frame and basis beside the
existing accepted record. Frame progress may share a rounded nanosecond;
unchanged frames preserve age, and scalar/frame modes cannot mix within an
epoch. Playback ports retain portable source facts with the backend's original
native packet and require matching timing in the same publication before
producing a new observation. Checked packet construction is mathematical
consistency validation, not hardware authentication.
The existing native presentation owner admits original coordinates through its
same validator and audio authority. Coordinate kind, basis and frequency must
match even for duplicates or deferred HOST progress. Legacy scalar admission
refuses coordinate mode. Native progress commits only after actual authority
retention; authority refusal preserves the native record and pinned basis.
Compare coordinate operation and presentation positions before nanosecond
rounding. Equal rounded raw points can produce different active elapsed values
after subtracting a fractional source cut. Exact query comparison preserves
signed whole and fractional order without allocation or denominator narrowing.
A real observed clock may trail a predicted input operation: advancing actual
presentation must preserve the later operation rather than move it backward.
Group and solo phase execution use the existing shared transport and audio
producer. Common phase validation precedes group lifecycle effects; actual
member failures retain their original partial reports and poison semantics.
Supplied raw scheduling audio_at remains unchanged.

Source pause adoption and its first projected target sample are distinct.
Playback mapping selects a caller-qualified half-open physical source interval
before rounding: Running subtracts its physical/playback integer gap, while
Frozen preserves the actual adopted playback frame. A mathematical interval
descriptor is not evidence of generation, submission or pause acknowledgment.
The source interval's exclusive end selects the next interval at equality.
Actual source mappings must be captured from the same pause/start-gate values
used by Mixer rendering and retained with the complete conversion owner.
Unconverted native outputs must retain the same actual source mappings through
their Mixer owner, whose ordinary render path automatically records opt-in
history. Cold enabling at an advanced cursor starts a new retained range and
does not reconstruct earlier output. Converted owners delegate to that same
history while rejecting late enabling after positive target generation.
Direct source-duration mapping selects exact physical frame boundaries and
removes playback gaps before the final relative floor. Native prediction
limits compare exact coordinates against the permitted duration, so a shared
floored nanosecond timestamp cannot hide a fractional limit violation.
WASAPI, CoreAudio and ASIO must not require a fabricated
converter to obtain playback timing. Their optional publication must pair
history with actual native delivery evidence under the publisher's existing
revision, preserving legacy snapshot interfaces.
Shared atomic source history stores bounded descriptors and source pull facts,
without its own revision or clock authority. Each adapter must enclose its
publication and lookup in the existing native publisher's revision alongside
associated delivery evidence. Reading the source payload alone grants no
coherence, submission, presentation or pause acknowledgment proof.
Optional common scalar telemetry must publish the source payload under the
same existing revision as its associated scalar facts. Publishing actual
history without a new report preserves the previous report and status;
advancing source pulls must not require a fabricated successful render report.
Missing history permanently closes the opted-in source query while legacy
scalar reads retain their existing behavior. Exact source lookup rechecks the
surrounding revision and never treats software prepared/submitted counts alone
as proof of native delivery.
Native opt-in publishers must attach actual source history at every existing
publication site, including final failure publication. WASAPI keeps its current
rule: only successful generation and ReleaseBuffer count as music submission;
a later clock error preserves that prefix, while a SILENT fallback does not
invent successful music delivery. ASIO pairs source facts with the copied
successful callback event under its existing outer revision, retaining prepared
counts as software facts; owner-side priming supplies no callback evidence.
CoreAudio pairs source generation, all-buffer copy completion and original
presentation fields under its existing checked outer revision. Successful
delivery extent is a range bound, not proof of a gap-free or audible prefix;
callback failure and configuration diagnostics still require native admission
qualification. Failed generation/delivery retains actual source facts without
fabricating a new report, delivered extent or presentation timestamp.
CoreAudio compound queries require a live callback context and become
unavailable after retirement. Complete source history remains with the recovered
Mixer for subsequent output preparation; pending input must be drained and
qualified before retirement.
Ordinary ALSA source, positive submission and optional native timing must be
read under the existing timing publication revision. Reopened pending output
uses the new stream's actual first unsent physical frame as its submission
basis; retained source history alone grants no new-stream admission. Exact
queries must stay within that stream's positive submitted prefix and reject
fractional future coordinates before rounding.
The source publication must finish before ALSA's bounded device wait, so
queries can read generated and submitted facts while timing is unavailable.
After a successful wait, native timing is published under a newly acquired
write region of the same revision. Missing timing cannot create a clock
observation. Render, encoding, write and timing errors preserve already-real
source generation and positive admission without inventing callback reports.
Public ALSA source queries retain those facts after stop or failure, while
their optional native timing is unavailable outside the running lifecycle.
History capacity must be checked before rendering mutates PCM, commands or
playback evidence. Consecutive identical mappings merge without consuming
additional slots. Empty source calls and target-only Held rendering create no
source adoption. Queries may include the actual pulled frontier endpoint but
must reject fractional physical positions beyond that frontier. Endpoint
calculation grants no authority for a future source interval.
Native queries must read generated conversion, positive submission and actual
source playback mapping under the same publication revision. Compound queries
require both histories; missing or malformed source history closes compound
lookup without changing legacy converter-only lookup. Source-only progress
after an error must be published even when converter history has not advanced,
while preserving earlier associated reports and submission evidence.
Fractional source overshoot after pause adoption must not advance frozen
playback. Resume control retains the acknowledged frozen phase; subsequent
presentation preserves the real fractional overshoot after the resume cut.
Map original physical coordinates through the actual source playback/pause
segments before the single relative elapsed floor. Generated source mapping,
positive native submission and native pause acknowledgment remain separate
facts, including inputs acquired before the projected pause boundary.

## Finite and full-song intent

The native full-song i64::MAX sentinel with completion intent None is a
conceptual open end, not an explicit finite deadline to inverse-map. Slow
ordinary full-song play must not fail because this sentinel would overflow a
finite output timestamp. Explicit finite regions and controls still require
representable actual timing before admission. No saturation, shortened
duration or fabricated end is permitted.

Retained Mixer playback has no immutable finite endpoint. Completion of a
finite practice attempt uses its actual program receipt and existing native
presentation/input drain. Dynamic full song also requires real final BGM
starts/tails, original judging deadlines, command settlement and native drain;
an empty legacy feeder or rate alone cannot prove completion.

## PCM, capacity and command truth

Program BGM and gameplay keysound heads use the same immutable selected rate.
Keysound scheduling retains the actual raw `audio_at` supplied by the existing
rendering/write-frontier scheduler; original input HOST metadata stays intact.
An interpolated historical input `raw_output` is not a command scheduling point,
and source speed does not multiply raw `audio_at` again. A cold rate installation
must be coherent before output starts, without fake command consumption or
ACK credit. Arbitrary installed SetRate/Seek commands remain unsupported until
the separate runtime rate/reverse contract is implemented.

Cold preparation validates rational head representation and rate-aware
discrete overlap envelopes, including mixed sample Hz and rounded cue starts.
Configured voice, control, receipt and event limits remain explicit. Actual
render, control application and loop callbacks allocate zero and remain
bounded. Preflight and execution share the same transition rules; an error
cannot partially modify PCM, cursors, queue state or counters.

## Fresh attempt, capture and comparison

Every committed loop/scrub attempt prepares coherent original chart, judge,
gauge, score, timing policy, selected-rate transport, capture, completion and
launch identity. Canonical source speed survives record envelopes and opponent
comparison; same-speed conditions may match and different-speed conditions
must refuse. NORMAL retains its legacy comparison/header compatibility.

Qualify an original projected target receipt and close the real acquired input
prefix before replacing gameplay owners. A failed later cohort member must
not partially replace/archive other members. Publication failure after actual
commit preserves the applied generation, fresh capture path and primary
failure. No synthesized input timestamps, reports, presentation or ACKs.

## Existing controls and visible behavior

Reuse Settings --speed, F2/F5, bookmarks and existing practice controls. Draft
validation keeps keyboard focus and a specific error. Start freezes selected
speed; committed fresh attempts and retries preserve it. Existing correlated
pending/refused/applied replies remain visible and stale replies cannot relabel
a later attempt. Capability/help text must describe actual supported retained
speed and the still-unavailable runtime rate/reverse behavior accurately.

## Verification and known ceiling

The complete converted native state retains an exact positive-admission
frontier, updated only after a successful positive prefix. Generation, zero
submission and refused submission cannot advance it. Moving that complete
state preserves the frontier; wrapping a generated converter starts without
submission evidence. The frontier proves software submission only. A new
native stream must independently qualify its own submission range and epoch;
neither historical generation nor a previous stream's submission proves
current presentation.

An opted converted ALSA stream publishes generated correspondence and its own
positive submitted range coherently with the existing output facts. Readers
use bounded retries and reject a query without current-stream submission
proof. New native streams seed generated history but reset submitted proof.
Successful positive writes publish their frontier before later timing/clock
queries; failures of those queries cannot erase submission. Ordinary output
callbacks append only new transition descriptors and update the upper bound,
without copying the whole history or allocating. Post-generation projection
failure still preserves and publishes the generation that actually committed.

Exact native-counter interpolation preserves fractions wider than the target
duration codec can store. It supports signed HOST offsets for the existing
explicit extrapolation policy, including negative mathematical coordinates,
without granting that policy's permission itself. Compare exact coordinates
with generated/submitted boundaries before selecting a converter span. Apply
physical-to-playback pause mapping before the final elapsed-time floor. Large
intermediate denominators cannot reject a representable final result. Query,
comparison and elapsed calculation allocate no memory; ordinary fixed-rate
phase APIs keep their existing path.

Coordinate-mode input and presentation commits retain exact mathematical
watermarks in the existing authority. Preparation snapshots bind both values,
including across correlation restarts; equal rounded ClockPoints and revision
cannot hide different exact state. Mathematical equality permits different
encodings of the same duration. Observed frontier advancement selects actual
retained native coordinates covered by the drained prefix, never HOST-now
extrapolation. A presentation behind a predicted input may close the prefix
without regressing the operation watermark. These metadata APIs alone do not
prove actual Runtime/native pump integration.

The production solo and cohort loops opt into coordinate preparation through
the explicit source history binding. Keep original acquired input queued while
the exact playback mapping is unavailable; do not fall back to scalar mapping.
Use the current shared Transport phase basis for each input or observed advance,
check finite endpoints with phase-aware position, and revalidate the prepared
token before consuming input. Only an actual completed Runtime operation commits
its operation watermark. For cohort failures, retain and publish completed
member reports and commit once when that partial result is nonempty; an empty
failed operation does not commit. A frontier with no gameplay advance may close
the proven native presentation/input prefix without emitting a gameplay report.

The actual output owner validates compound source packets against its published
epoch/basis and requested source history identity before native admission. An
exact query must return the same mathematical coordinate. Recheck actual output
identity after IO before returning an available packet, including readonly
queries whose adapter may use interior mutability. Observation report caches
use the report from that same compound publication; no separately acquired
report may be substituted. Static output wrappers preserve the original native
evidence type without mandatory Clone/Copy bounds.

The common device query returns portable source or converted playback facts;
native platform evidence remains with the typed adapter. Compute active elapsed
from the original source cut, actual playback adoption and conversion
correspondence, independently of normalized chronology. Reject the old converted
segment at an exact next-segment boundary, and reject fractional progress beyond
the actual positive submitted prefix before flooring. Converter Held alone is
not a source pause acknowledgment. A default unsupported hook and checked
mathematical mapping do not prove actual device or pump support.

Native ASIO block order uses original nonoverlapping frames, including progress
that occupies one rounded nanosecond. Legacy ASIO scalar consumers preserve
their previous states and freshness when the scalar authority or estimator
remains Unchanged. Preserve the existing WASAPI contract: original counter
progress may update native evidence while quantized scalar authority history
and freshness remain unchanged. Coordinate-mode owners instead require exact
Retained admission before native commit.

Coordinate startup retains two original admitted observations and the actual
readiness HOST sample, together with their immutable epoch and source-frame,
source-counter or target-frame descriptor. Reuse the existing bounded startup
loop and its input, BGM, timeout, cancellation and freshness behavior. Refuse a
changed epoch or descriptor during startup, even when each replacement owner is
valid independently and its rounded origin matches the earlier owner.

Startup inverse mapping uses original units and selects the earliest integer
HOST point whose exact interpolated coordinate is at least the requested
coordinate. Require explicit finite backward HOST permission, reject negative
permission and arithmetic overflow, and never extrapolate past the latest
original observation. This software acquisition origin does not acknowledge
native playback or close an acquired-input prefix.

Original target-frame admission couples the existing native validator and audio
authority transactionally. Check their original histories, epoch and immutable
target basis before duplicate or deferral handling; commit native progress only
after exact authority retention. Target observation through the actual output
owner checks current output identity before and after IO. A separate BGM render
report does not qualify a coherent converted playback mapping. Unsupported
target counter evidence remains unsupported until its own native qualification
is implemented.

Coordinate gameplay binds the actual cold source playback history identity
explicitly, including its source rate and original physical/playback origins.
Do not infer it from a rounded playback origin, a current attempt cut or a
receipt generation. Keep this identity through warm attempts and read the
current attempt phase basis from the actual shared Transport for each operation.
Existing scalar entrypoints retain their current contracts; coordinate entry
paths opt in explicitly and use the same production gameplay loops.

Coordinate NORMAL playback without a selected speed must still validate the
actual Runtime endpoint and NativePause epoch, HOST domain and Running state
before device observation, input acquisition, playback lookup or judging.
Matching Transport configuration alone does not prove these independent owners
agree. A mismatched owner is refused without consuming queued input or changing
the source cursor, authority, Transport, judge, capture or reports.

Coordinate control cutoffs use the enclosing retained original native
observations and the earliest integer HOST at which their exact interpolation
reaches the original cut. No rounded output inverse or extrapolation qualifies
a control. The right observation must already exist at validation time and
remain fresh. A caller-supplied HOST cutoff must match this qualified minimum.
Pause requires input at or below the cutoff to be drained; resume and a fresh
attempt require only earlier input to be drained, retaining equality.

Control preparation and validation change no authority or input state. A
genuine accepted control commits only the existing exact/logical operation
watermark. It does not manufacture an input occurrence, native presentation
or acquired-prefix closure. While held, separate validated closure uses the
actual due retained observation and fully drained acquired prefix, preserving
the operation watermark even when presentation advances. These mathematical
authority tokens do not themselves prove native source adoption or emit a
pause/end acknowledgment; actual native owners must qualify and stage those.
Before acknowledgment, a native owner must compare the token's exact query to
its staged actual adoption cut. Distinct legal cuts can share the same minimum
integer HOST and rounded output; those shared values do not establish equality.

Finite coordinate completion binds the native end owner's output epoch and
original coordinate descriptor at cold setup. It stages the actual source
endpoint marker and, for converted output, its actual target projection before
setting emitted/deferred state. The source cut is the original physical endpoint
divided by source rate; the converted cut is the original projected target time.
Native crossing and drained-input qualification must precede final Runtime
advance and a single end acknowledgment. Final elapsed phase comes from the
absolute playback endpoint minus the current attempt's active frame origin,
floored once at source rate. Target projection overshoot and a rounded song-time
seek must not replace this phase or authorize completion. Coordinate startup
must not prime end state through the scalar emission/defer path.

NativePause also binds the original output epoch, coordinate descriptor and
source playback identity before any request or observation. Preparing pause,
resume or end adoption is read-only and binds the complete native owner's
prior semantic state. Missing actual source adoption or target projection
cannot emit an ACK. A coordinate-bound owner cannot emit through a legacy
scalar observation path. Stale preparations and duplicate ACKs are refused.

Coordinate pause requests retain the actual original native observation from
the matching authority. The adoption cut must not precede that request's exact
coordinate, even when both cuts share a rounded output and earliest integer
HOST. A rounded ClockPair request cannot supply this evidence. Compare the
authority stream origin to the descriptor's stream creation point; retain the
absolute frame-zero origin separately for projecting the physical duration.

The coordinate descriptor bound at native setup is immutable for that epoch.
A converter's current pending/admitted frontier can move after rendering and
must not replace this original descriptor when constructing an authority or
validating a pause/end preparation. Tests must preserve the original owner
basis while exercising subsequent projected boundaries.

Endpoint observations may be staged and retained on the same NativeEnd owner
before the crossing is due. Retaining checked source reports and target
projections does not acknowledge completion. The later acknowledgment must
still qualify the exact retained cut against the current authority, input
drain and complete owner snapshot.

Production coordinate control facts must come from the same compound native
publication as the admitted original observation and source report/history.
The common device port preserves its original epoch, coordinate descriptor,
source identity and report; it must not assemble these from independent reads.
An unavailable publication yields no preparation. A stale or mismatched
publication cannot acknowledge pause or completion. Converted control remains
unsupported until a coherent publication also carries the original target
projection and submitted target span.

On converted ALSA output, the existing worker publication must cover native
timing, positive target admission and converted/source history facts together.
A bounded reader accepts one unchanged outer revision and the original cold
source-history identity. A conversion phase's initial source position must not
replace that history's original physical/playback creation frames after an
output replacement. Busy, exhausted, unavailable, stopped or mismatched
publications yield no native playback permission. Legacy output behavior stays
unchanged when the converted compound path is not opted in.

The app's converted playback Port carries a platform-neutral Target packet:
immutable target basis and epoch, original source-history identity, exact target
query, converted/source history mapping, original reports and boundary facts,
and adapter-owned native evidence. Outer native publication and inner converted
publication revisions remain distinct. Packet validation reuses the existing
exact mapping checks and never invents a source submission count from target
fractions. A native observation requires evidence and an exact match between
the original coordinate and packet query; rounded timestamp equality is
insufficient. Construction alone grants no native authenticity, authority
admission, input drain or pause/end ACK. Source Port contracts stay compatible.
Mapped boundary times cannot exceed actual generated output. A boundary may
still lie beyond the native query or submitted prefix, and a retained boundary
may precede a replacement stream's start; those cases remain valid facts until
the existing control owner qualifies a native crossing.

Source coordinate startup receives the actual opted-in source-history identity
explicitly. It services original input and uses the same compound report for
BGM feeding while awaiting two original, due and fresh native associations.
It must not read a separate render report, prime a scalar endpoint ACK, guess
initial history frames or replace the original coordinate descriptor. Mixer
endpoint markers remain available in original reports for the gameplay owner's
later qualified staging. Unsupported Target startup and mismatched cold
identity are refused before IO; existing scalar startup behavior is preserved.

Native ACK commit revalidates the current authority control token before that
token is committed to the authority; committing the authority first changes
its preparation state. Production integration must preflight the actual phase
operation and preserve its genuine accepted reports in the same ownership
section. A native ACK alone does not publish game completion. Pause and resume
control elapsed phase uses the same adopted frozen absolute playback frame
minus the current attempt's active origin. Subsequent converted playback keeps
its actual fractional resume overshoot; the control does not add it or create
a new selected-rate anchor.

The pause cutoff drains input at or below its qualified integer HOST. An input
exactly at that cutoff is retained by the existing paused keyboard state,
without a gameplay judgment or input watermark commit. Resume retains cutoff
equality until keyboard reconciliation has completed. A derived release uses
the actual resume output phase and preserves the original paused input's
device, sequence and clock point; its frozen song time does not turn it into
an earlier gameplay input. Solo and cohorts use the same cutoff semantics.

Before coordinate play mutates a session, check the matching native/authority
owners, original descriptor, HOST/raw/logical domains, source history rate,
actual phase Transport and exact pristine section/preroll/rate/end setup. A
target output rate can differ from the immutable source rate. Missing bindings,
mixed scalar/phase modes and a changed cold setup are refused. These cold checks
do not themselves qualify a native observation or a playback mapping packet.

Converted native publication accepts exact interpolated coordinates directly.
An interpolated coordinate just beyond a positive submitted endpoint is
rejected even if its rounded timestamp equals that endpoint. Selection at
Active/Held transitions uses the exact coordinate. Both existing target-time
and interpolated queries share the same bounded coherent reader and checks.

Verify literal 1/2, 1, 2 and 3/2 PCM with actual Mixer callbacks, odd preroll,
non-grid finite ends and repeated loops, original source selection, pressure,
atomic refusal and zero allocation. Test real native solo and 2/4-member cuts,
converted target projection, original input/capture provenance and canonical
fresh record identity. Use shipped CLI and actual existing GUI controls at the
highest available local tier, followed by independent review and required QA.

Known ceiling: dynamic rate switching, reverse policies, pitch-preserving DSP,
network retained practice, retained browser playback, physical audio/input
timing and foreign OS execution remain full-Goal requirements. Pure fixtures,
software output and foreign Rust typing do not prove those requirements.
The prior browser ERR_ABORTED and registered x11-mcp QA blocker remain
unresolved; successful functional checks do not erase that evidence.
## Target coordinate control provenance

History retirement preserves immutable cold Source/converted identities and
separately exposes the actual retained lower bound. An owning-thread operation
may discard only complete spans before the span covering its conservative minimum
live pin; it always keeps the last span and does not rebase fractions, allocate or
increase capacity. The caller must qualify all outstanding references first.
Pins outside retained/generated bounds refuse without mutation. Positive removal
advances a checked mathematical retirement revision; no-op leaves it unchanged.
Queries before the retained lower bound return a range error, never an underflow
or a fabricated cold mapping. Tests compare remaining exact mappings before and
after removal and drive subsequent real pause/resume rendering at fixed capacity.
Native callbacks must not apply retirement until coherent publication and all
authority/control/input/practice/unsubmitted-PCM pin handling are connected.

Target startup obtains two original associations through the same compound
Source/Target seed port and the existing bounded startup loop. Each Target packet
must match the current original presentation record and explicit Source history
identity before its actual source report feeds BGM. The output sample rate can
differ from the Source rate. Missing reports stay absent; no independent render
read, scalar observation or endpoint acknowledgement substitutes for the packet.
The first two associations stay immutable while waiting for a future second HOST
or BGM readiness. Timeout, age, input cancellation and clock regression checks
remain unchanged. Linux solo/cohort seed adapters forward this exact static port.

Solo and local cohort playback use the same existing Runtime control loops for
Source and Target observations. Target pause/resume consumes the packet's actual
source adoption report and converted projection; Target end staging consumes its
original endpoint projection. There is one shared native cut for all members.
Pending or stale observations cannot acknowledge output commands or consume
input beyond an unknown cut. Existing native acknowledgement, exact HOST input
ordering, partial Runtime commit and completion gates remain in force. Entry
tests must drive real conversion and the actual Runtime command queue through
pause/resume and finite end, in addition to preserving Source regression cases.

Target output enters explicit held rendering after genuine paused native
adoption and Runtime prefix commit. It releases hold when a resume request is
accepted so the original Mixer can adopt the queued resume. A pause request alone
cannot enable hold. Held callbacks freeze converter source position and consume
no Mixer commands. A later publication failure must preserve an already genuine
pause acknowledgement and its held effect; an empty failed Runtime operation
cannot enable hold. Compound provenance is checked before any endpoint seal or
report side effect, so stale finite packets cannot seal gameplay audio.

The common statically dispatched device port accepts Source or Target control
facts. Existing Source device implementations retain a single-call default
adapter. Explicit Target implementations must survive the native-device blanket
adapter and the bound audio pump without falling back to Source or independently
reading reports. Linux solo/cohort device adapters obtain Target control facts
from the existing output owner's one admitted packet and use exact packet queries
for playback mapping. Tests cover defaults, explicit overrides, absent facts and
errors; actual Linux binary compilation checks the production wiring.

Target control preparation consumes the same immutable playback packet that the
existing native presentation admitted. It preserves the original native record,
exact Target query, distinct inner/outer revisions, actual source report and
converted boundaries. A later admitted observation invalidates the earlier facts
for a new preparation. Construction supplies provenance only; freshness, input
draining and native acknowledgement remain separate existing checks.

Common Source/Target control accessors preserve absent and zero source reports.
Target facts never fabricate a Source submitted prefix or assume equal source
and output sample rates. Tests must exercise real converted histories and actual
presentation admission, including stale records and mismatched identities.


Source history publication must preserve original creation identity when the
owner retires a closed prefix. Its actual retained first span becomes the query
lower bound; queries before that span are refused, while the pulled endpoint
remains inclusive. A changed mathematical retirement revision forces bounded
republishing of remaining preallocated slots under the existing adapter revision,
even when retirement and rendering restore the previous count. An unchanged
retirement revision retains the existing append-only count limits and rejects
count/frontier corruption. The retirement revision grants neither native
presentation authority nor permission to discard live consumer coordinates.

The complete converted output owner protects the original start of any block
with pending PCM during history retirement, including held/cached blocks and
partial native admission. Phase pins may not advance beyond that block's exact
Target start; Source pins may not advance beyond its integer source-start floor.
Full admission releases the pending-block protection; the converter's next
sampling Source integer floor remains protected even after admission, because
lookahead may have pulled later pause/resume spans ahead of cached consumption.
A Source pin beyond either internal floor refuses. Downsample sampling positions
may lead pulled frames; no sampling-position ≤ pulled-frontier constraint is
imposed. Refusal preserves PCM, reports,
admitted prefix, boundaries and history. External live consumer pins still need
separate qualification; neither operation grants native acknowledgement.

Converted phase publication must republish retained original spans under the
existing generation when the mathematical retirement revision advances. An
unchanged generation endpoint/count must not hide retirement. Cold identities,
stream origin and positive submitted prefix are not rebased. Retired lower bounds
reject earlier exact queries; unchanged retirement revisions retain original
append/count/frontier refusals. Source and Target payloads remain qualified by
the same publication generation. A newly seeded stream still requires its own
positive admission even when seeded from a previously admitted retired owner.

Future-input retention uses a read-only exact query from the actual original
coordinate bracket covering the closed HOST prefix. Inputs at or before that
prefix remain refused; old observations retained for diagnostics do not pin
future input forever. Before the first bracket, bounded-before extrapolation
requires retaining the immutable descriptor creation bound. The getter leaves
all authority history/revisions and prepared operation tokens unchanged.
Before-origin prepared input may precede that creation bound; its existing
discard-only path performs no native playback lookup and remains unchanged. Actual
outstanding control/practice queries must still be pinned separately.

Pure history retirement prevalidation must use the same exact range, covering
span and checked positive revision rules as mutation, without changing state or
allocating. The complete output owner can retire Source and phase histories as
one mathematical operation only after both validations and its internal PCM/cache
protection succeed. A bad request for either history must leave both histories,
PCM, admission and reports unchanged. This supplies neither a native callback
request protocol nor permission to discard external consumer references.

Native retirement requests use a bounded stream-bound mailbox containing exact
Source/Target pins, the original Source history identity and immutable Target
basis. Only nonregressing qualified pins are accepted; current mathematical
retained/generated ranges are checked together. Busy publication returns retry
without changing the request. Accepted request revisions prove queued mathematics
only, never native presentation, completion or pause acknowledgement.

The converted output loop consumes requests inside its existing timing update
before generation capacity checks or pending PCM encoding. It clamps pins to the
original pending block and next Source sampling floor, retires both histories on
the same owner, and republishes only actual removals under existing revisions.
A clamped request remains pending until its original pins can be reached. Partial
admission, EAGAIN wait ordering, original PCM/reports and legacy output cadence
remain unchanged. External live consumer pins must be qualified by the business
port before requests are submitted; selected launchers are not enabled by this
low-level connection alone.

The current static output owner can submit an exact minimum query for history
retention only against its matching presentation epoch, immutable Target basis
and original Source history identity. It obtains one validated exact playback
packet and requests the original covering phase Target start and Source playback
physical start from that packet. It does not reconstruct coordinates from
nanosecond floors, acquire another native observation or change presentation,
authority and cached reports. Missing output or packet queues nothing. The native
adapter rechecks expected epoch and basis before touching the stream mailbox;
busy and native errors propagate without becoming acknowledgement. The caller
must still combine all live input/control/practice references before supplying
this minimum; availability of the port alone does not enable selected launchers.

The complete audio owner also protects every original practice receipt still in
its fixed queue: raw receipts preserve their actual Source physical frame;
projected receipts additionally preserve their exact Target boundary. Retirement
reads these references without consuming receipts or manufacturing a projection,
and clamps requests until the controller releases them. The app's first held
projected receipt separately preserves its original Source/Target pair until
actual application. Merely retained prior attempt metadata does not pin history.
The common loop must poll the first receipt after original observation and before
queuing combined minima, preserving the transfer from queue to app-held ownership.
The controller must also expose the original queued Source/Target minima without
consuming them: a raw receipt is still a Source reference while no projected
receipt can be popped. The loop combines those queued minima with app-held pins
before submitting any request, so transferring an older receipt into app-held
ownership cannot reveal a lower Source pin after a newer request was accepted.
The app exposes queued references separately from its held receipt, preserving
Source-only raw state and converting only actual projected Target boundaries to
exact queries. Both getters are read-only and neither pops the queue.

A live coordinate pause/resume request preserves its exact original request
coordinate while transition adoption remains pending. Running/Paused or scalar
owners supply no such pin; old saved request metadata does not permanently pin
history. A staged adoption/end still preserves its own exact cut until commit.
Read-only retention getters change no owner state, authority tokens or ACKs.

The actual common solo/cohort loops submit one statically dispatched converted
history-retirement request after original control acquisition and staging,
before further practice service/input acquisition. Exact optional cuts include
the live pause request, staged pause/end, queued projected practice and app-held
practice, together with the authority's future input minimum. The original queued
and held Source minima travel separately; the output owner caps its covering
Source span start by that minimum and rejects values before original creation.
Raw Source-only receipts never acquire an invented Target coordinate.

Practice queue pins are read before the first peek transfers ownership to the
app; the resulting held pin joins the same aggregate. Repeated existing peeks
remain idempotent. Earlier queued requests must remain conservative during this
transfer; actual ordering/interleaving evidence is required before activation.
Legacy/scalar/Source timing opts out without new IO. Current Linux devices merely
forward the same common request through the existing output owner. Busy returns
retry, failure propagates before subsequent input/Runtime effects, and a queued
revision cannot acknowledge pause/end or establish native presentation.

When the newest compound control publication is absent, an already admitted
presentation may still be used for a read-only retirement query. The current
output owner must revalidate its epoch, basis, original Source identity and
exact packet before queueing; an absent mathematical packet queues nothing.
Stale observed control facts fail before dispatch. Retained presentation reuse
never creates a new native observation or acknowledgment.

In the actual shared cohort loop, an unresolved pause/resume transition blocks
normal closed-input frontier advancement. Native output may progress while the
shared prefix remains fixed; its original pending request remains protected.
ACK removes the pending reference before held-frontier advancement. Tests must
exercise these reachable states instead of inventing a closed frontier beyond
an unresolved cut. Independent exact-minimum tests cover earlier optional
references against later legally constructed future-input bounds.

Development evidence for the common dispatch: 27 new cases pass, along with
2,969 app library and 35 Linux binary cases in
`target/wf/retained-speed-development-12e0c/loop-retirement-dispatch-app-c2.log`
(`--features desktop,webtransport --lib --bin linux_bms`, failure 0,
existing environment ignores 7/3). This proves the tested loop/port behavior;
actual native mailbox consumption during queue-to-app transfer, fresh retained
Target practice and ordered full-task review/QA remain separate requirements.

An original projected practice receipt in TargetFrame mode must qualify its
exact TargetTime against retained original native coordinate observations.
Nanosecond rounding supplies only the display/Runtime projection; it cannot
authorize the boundary. Missing crossing evidence waits. Readiness requires
the actual acquired prefix and strictly earlier input drain; input exactly at
the cut remains for the fresh attempt. Commit uses the same qualified exact
coordinate control cutoff and preserves epoch, creation descriptors, retained
observations and closed input chronology. A prepared boundary is bound to its
original qualification mode and projection. Scalar behavior remains intact;
Source-frame/counter practice support requires its own exact boundary path.

A fresh same-output practice attempt preserves the selected positive speed and
an explicit output phase. Its owner/domain/Source sample grid remain unchanged;
the generation is the actual receipt generation and active origin is the
receipt's playback-frame cut. Original conversion overshoot at the first Target
sample remains in exact playback mapping. The old attempt advances to that
qualified cut through its old phase before archival; equal-cut input remains
for the fresh phase. Missing playback mapping waits before installation.
Solo and every member of a cohort must install coherent fresh phase, scope,
judge, gauge, capture and competition state through the common preflighted
replacement transaction. Scalar replacement keeps its existing normal-rate
contract. Same-output practice never restarts original audio correlation or
clears acquired/closed input history to create a new attempt.


Fresh phase replacement has an explicit API on `RuntimeGroup` and
`SoloRuntime`; the inherited scalar replacement keeps its normal-rate guard.
The explicit path requires one fresh positive-rate anchor with the same selected
speed, owner, domain and Source grid, an increasing receipt generation/audio
scope and a nonregressing original playback origin and logical anchor. Every
member judge, roster, policy, endpoint and required storage is preflighted before
the first Runtime mutation. Native code additionally checks the actual fresh
mapping and coherent attempt metadata before retiring old captures.

External archive, UI and competition ports retain their visible terminal error
semantics. Runtime replacement preflight does not promise rollback of external
archive effects or atomicity across all ports. An observer refusal after fresh
identity installation must leave that installed identity coherent.

Development checkpoint (2026-10-13): `fresh-practice-phase-app-c3.log` has
2,989 passing app tests, one failing new solo fractional-cut fixture and seven
existing ignores. All six new Runtime replacement and four real cohort cases
pass; solo loop/missing-mapping/observer-refusal pass. Actual solo loop and
observer-refusal canonical reconstructed replay hashes equal the current judge;
cohort cases compare each member's canonical replay with its current judge.
The remaining scrub oracle must establish the actual applied fractional cut,
not infer it from queue minimum retention pins. Linux binary tests were not
executed by this command because its library test target failed. These are
bounded development results, not full-task review or QA acceptance. WBS remains
91/193; selected launcher activation and original backend/browser obligations
remain open.


Final bounded fresh-phase development verification (2026-10-13):
`target/wf/retained-speed-development-12e0c/fresh-practice-phase-app-c6.log`
exits 0 with 2,991 app library and 35 Linux binary tests passing, zero failures,
and seven/three existing environment ignores. All 15 new Runtime/solo/cohort
cases pass. Actual solo odd-Source scrub and two/four-member odd-Source loop
prove fractional original correspondence, positive first-fresh-input timing,
equal-cut archive exclusion/new capture attribution and canonical replay/current
judge equality. Missing mapping waits without archival/replacement/ACK;
post-install observer refusal preserves coherent fresh identity. Original
Source history creation and continuous playback spans remain unchanged.

This supersedes the pending c3/c4/c5 development results above. It is bounded
software evidence from genuine Mixer/practice queue/Converter with native facts
supplied through DI, not physical/native-device acceptance or formal full-task
review/QA. Enabled competition is still a fixture coverage gap. Selected Linux
launcher history/startup/initial output phase/coordinate entry activation,
Source-frame/counter practice, other native backends and browser acceptance
remain required. WBS stays 91/193 and the full player Goal stays active.


Local Linux retained playback must explicitly open a coordinate-capable Target
output: enable original Mixer Source history before converter ownership moves,
and enable conversion-phase history before the first generated sample. The
original Source identity comes from the histories' creation descriptors, not
assumed zero coordinates. Network committed startup retains its current scalar
route until its own coordinate policy is implemented.

Selected positive-speed practice uses the retained speed-aware audio program
and matching PracticePlayback setup. Coordinate startup uses original Target
observations and an exact query to qualify HOST; an unavailable correspondence
cannot become a scalar fallback. The initial shared phase preserves actual
output/session ownership, original Source grid/playback cut and whole-preroll
intent. The same collected-input buffer survives startup and gameplay. Solo
and all cohort members use selected-policy coordinate practice entries; fresh
scores, exact original-ID roster, gauge/capture/competition identity and original
sound plans remain admission requirements. Activation must preserve scalar
network and Legacy behavior and does not establish physical timing acceptance.


`AudioAuthorityEpoch::logical_output` is a public checked mathematical
projection for native binary callers. It verifies raw-domain and timestamp
arithmetic only, grants no evidence or native admission/ACK and changes no
authority state. A coordinate launcher must first obtain a valid exact native
HOST correspondence; the scalar NativeAudioPresentation projection API still
refuses coordinate mode. The cold Target startup query excludes whole preroll,
which belongs once in the initial phase transport. Coordinate cohort activation
requires a fresh single positive phase anchor on the logical domain and
installs its scope; the original scalar activation keeps its NORMAL guard.


Final bounded Linux coordinate activation development verification:
`linux-coordinate-launch-app-c3.log` proves 2,999 app PASS/zero failures/seven
existing ignores; its binary target initially failed one new completion oracle.
Finite diagnostic sections intentionally lack a full-song completion owner.
`linux-coordinate-launch-bin-c4.log` verifies the corrected finite contract and
an added genuine full-song selected 3/2 completion case for two/four members:
46 binary PASS/zero failures/three existing ignores. App sources were unchanged
between c3 and the binary-only c4 execution. All 19 new cases pass (eight
selected-policy bridge, six cold solo factory/phase and five cohort composition).
Scoped formatting and source/doc diff checks pass. These results supersede
pending c1/c2/c3 checkpoints; first failure logs remain available.

Actual cold factory tests preserve nonzero original Source physical8/playback5,
1000-to-1500 grids, original commands/PCM, partial submission truth, immutable
cold query and whole-preroll NORMAL/3/2 phase setup. Cohort composition preserves
sparse original two/four-player rosters, gauge/capture metadata and one shared
phase/scope; malformed IDs, section, axes, missing phase/generation and old
scalar nonnormal activation refuse. Actual selected cohort scrub/loop entries
produce canonical replay/current judge equality, reject malformed initial
scores/rosters/policy/capture/source/end/scope before effects and prepare selected
UI policy once. New native binary launch source routes local nonnetwork playback
through those coordinate factories/startup/phase/selected practice wrappers.

Known ceiling: fixture proof excludes actual ALSA/HID clock startup qualification,
ambient UI and physical timing. New bridge fixtures do not directly execute the
solo private score helper, enabled competition or custom gauge. Bounded history
can refuse when old protected pins outlive its fixed budget. Scalar network
committed startup, Source-frame/counter practice, foreign backend/browser
acceptance and full-task ordered independent review then QA remain open.
WBS stays91/193; the full player Goal remains active.


Original SourceFrame and SourceCounter practice boundaries must qualify the
exact physical Source sample cut using their actual sample grid and retained
original native coordinate observations. Native counter frequency may differ
from Source sample rate; crossing and HOST rounding preserve exact fractions.
A direct Mixer projection keeps its original Source frame/time identity; an
unprojected direct receipt must not acquire an invented converted Target grid.
Missing original crossing evidence waits. Coordinate readiness and commit use
the same qualified cutoff, leave equal-cut input for the fresh attempt and
preserve original native history, epoch and acquired/closed chronology. Fresh
solo and shared cohort Source attempts reuse the same phase-bearing replacement
and canonical replay logic as Target practice. Display nanoseconds cannot
authorize SourceCounter crossing, and native correlation is not reset.

Direct-output projected receipts must preserve the exact normalized Source
duration `physical_frame / sample_rate`, as well as the original frame, rate
and origin. Matching floored nanoseconds is insufficient: two distinct
fractional durations may project to the same display timestamp. Such a
projection is rejected before qualification or owner mutation; an original
receipt still derives its Source query from the physical frame and Source Hz.


Source-coordinate retained practice development verification (AC090–092):
`target/wf/retained-speed-development-12e0c/source-coordinate-practice-app-c8.log`
records app 3,015 PASS/zero failures/seven existing ignores and Linux 46 PASS/
zero failures/three existing ignores. All 16 new cases pass. Exact Source
receipt frame/Hz qualification supports SourceFrame and SourceCounter without
using native counter frequency as Source sample rate. Genuine direct projected
and plain receipts retain their original creation grid. Same-nanosecond but
unequal rational projection times now refuse before mutation; the earlier
reported exact-projection identity ceiling is resolved by its malformed test.
Solo NORMAL/3:2 loops/scrub and sparse 2/4-player SourceFrame loops/scrub use the
shared phase replacement and preserve archive/fresh capture identity. Solo uses
canonical section reconstruction; cohort compares per-member ReplaySession
judge hashes. The missing solo packet case explicitly reaches the actual
Source30 loop query and then waits without archive, scope or success ACK.
Counter startup uses an actual gate at physical1 with original history0/0 and
positive native observations; physical/playback differences remain explicit.
First failure logs c1–c5/c7 are retained; final c8 supersedes those checkpoints.
Scoped source formatting and source/doc whitespace checks pass.

Known ceiling: these are software DI observations and genuine rendered queue
receipts, with empty sample banks; they do not prove audible PCM, physical
latency, real devices, ambient GUI or browser execution. Cohort SourceCounter,
nonzero original creation identities, preroll, enabled competition/custom gauge
and an explicit numeric fractional-counter song-time oracle remain outside
these fixtures. The tests preserve pending equal-cut input; they do not prove
retroactive reassignment of inputs already committed before a later request.
Source history retirement remains opt-out, so no unlimited-history guarantee
is made. Windows/macOS actual launch adapters and startup still require Source
packet forwarding; WASAPI needs actual native frequency discovery and a start
cut covered by retained positive counter observations, while ASIO needs an
immutable coherent playback query seam. Full-task independent code/security
review followed by browser/CLI/desktop QA remains open. WBS91/193 and the full
BMS player Goal remain unchanged and active.

Foreign native Source adapters must forward the existing Source playback port
using original creation identity and actual coherent publication facts.
WASAPI keeps native counter frequency independent of Source sample rate;
exact queries retain that frequency as u64. Its submitted bound is checked
against the original physical start and actual endpoint submission count.
CoreAudio uses the successfully delivered buffer end, independently of pulled
Source frames. Matching source and native observation publication is required
for an observation packet; a bounded read race waits without splicing views.
Invalid identity, revision, range or native failure refuses before admission.
These packets grant mathematical mapping, not native presentation or pause ACK.
ASIO immutable playback reads must reuse coherent callback publication without
calling SDK diagnostics, changing anchors or substituting a receipt timestamp.
Cached lifecycle diagnostics in such a read are not current native evidence;
mutable acquisition retains the actual SDK status/fault/clock checks.

Windows native composition must borrow its existing output stream when using
the library Source adapter. Library-owned output and the Windows backend enum
must call the same packet qualification implementation. Borrowing must not
reopen the endpoint, transfer Mixer ownership, reset history or create another
clock anchor. ASIO's existing HWND/message owner retains native diagnostics
and timestamp acquisition; a public mathematical qualifier cannot replace
those checks or grant native presentation permission.

### Foreign Source adapter development checkpoint — 2026-10-13

`CoordinateNativeAudioSnapshot::source_query` preserves the exact original
Source grid and native u64 counter frequency. WASAPI Source packets now use
the actual submitted prefix and require the complete original native snapshot
for observation. CoreAudio packets use successful callback delivery, allowing
pulled Source frames to exceed delivered frames. ASIO's immutable
`source_playback_view_at` reads coherent publication without SDK calls; mutable
observation still performs existing diagnostics and native anchor validation.
Changed publication waits; malformed identity, revision, range or callback
metadata refuses. None of these mathematical reads grants presentation ACK.

Development evidence: `foreign-source-adapters-app-c2.log` under
`target/wf/retained-speed-development-12e0c/` records 3,024 app tests and 46
Linux launcher tests passing, zero failures, with seven and three existing
ignores respectively. Nine new portable query/WASAPI cases passed. Windows GNU
and macOS all-targets checks with desktop,webtransport also passed; their
`foreign-source-adapters-{windows,macos}-c1.log` files record Rust typing with
stub C/archive tools. Eleven changed source/test files passed scoped rustfmt.
Host platform regression also passed: `foreign-source-adapters-platform-c1.log`
records 341 passing tests, zero failures and one existing ignore. The combined
executed app/Linux/platform suites therefore contain 3,411 passing tests;
foreign prepared fixtures are excluded from that count.

Known ceiling: CoreAudio's ten new macOS-gated fixtures were typechecked but
not executed here. ASIO's eleven new SDK-gated fixtures and actual SDK code
remain uncompiled and unexecuted without authentic SDK/MSVC. No actual native
device, audible PCM, latency or browser result follows from these checks.
The earlier missing ASIO immutable query seam is implemented; Windows enum
port forwarding, foreign startup and four solo/local launcher integrations
remain open. In particular, WASAPI must discover actual native frequency and
qualify an exact start cut using positive counter observations. Source history
retirement, full-task independent reviews and browser/CLI/desktop QA remain
open. WBS remains 91/193; the full player Goal stays active.

### Windows Source owner composition — 2026-10-13

`WasapiReplacementBackend` exposes borrowed native, coordinate, Source query
and compound playback routes; its existing port implementations delegate to
the same routes. `AsioReplacementBackend::source_playback_for_stream` provides
the immutable Source read, and the original callback qualifier is reusable
after genuine SDK/clock acquisition.

The Windows `Output` composition and `WindowsReplacementBackend` now forward
the coordinate and Source ports. `WindowsSourceEvidence` preserves the full
WASAPI snapshot or ASIO Source view. Wrapping retains the original epoch,
basis, history identity, exact query, facts and coordinate, with the existing
checked packet constructors. New mutable report-cache writes use only the
returned original coordinate or coherent compound packet. They do not change
the accepted pause/end cache; immutable lookup does not mutate either cache.
ASIO keeps its existing HWND/message, fresh SDK fault/status, configuration
and clock-anchor checks, followed by matching callback qualification.

Direct Source launch preparation is platform-neutral. It enables bounded
history before native open, retains the actual Source Hz and independent
physical/playback creation cursors, and queries the exact original creation
frame. Initial Transport uses that original playback identity and generation
one on the logical output domain. This preparation grants no native permission.
Local CoreAudio solo/cohort must use the same coordinate seed, Transport and
practice APIs for every positive fixed speed; network admission retains its
existing constraints. Original roster, input acquisition, bank, song region,
preroll, policy, gauge, capture and completion intent remain authoritative.

Development typing: `windows-source-enum-xcheck-c1.log` in
`target/wf/retained-speed-development-12e0c/` records a successful Windows GNU
all-targets desktop,webtransport check. Scoped formatting and whitespace
checks pass. A new dead-field warning for the opaque native evidence carrier
is present; the launcher does not yet consume that payload directly.
`windows-source-enum-app-c1.log` records app3,024/Linux46 passing tests, zero
failures and seven/three existing ignores. These are host regressions;
Windows-native execution is not included in their 3,070 passing cases.

Known ceiling: this check uses stub C/archive tools and excludes the ASIO SDK
feature. It proves no Windows execution, actual SDK compatibility, native
permission or device timing. Foreign cold Source history/startup and all four
solo/local launcher paths remain to be connected; their existing selected
speed exclusion has not been removed by this composition change. ASIO runtime,
Source retirement, independent full-task review and ordered browser/CLI/desktop
QA remain open. WBS remains 91/193 and the full player Goal stays active.

### macOS retained Source launch development checkpoint — 2026-10-13

Both macOS local solo and cohort launchers now use retained practice for every
positive fixed speed. They prepare the original song program with whole
preroll, enable Source history before CoreAudio open and preserve its original
creation basis. Startup uses the existing SourceFrame authority, coupled
Source seed and exact original creation query, followed by the common initial
Source phase Transport. The same input collector, roster and gameplay owner
continue into the common coordinate practice pumps. Network startup remains
on its existing admission path. Cohort setup restores original sound maps and
prepares full-song completion for each selected judge; finite-end intent stays
with Runtime/practice completion. The helper uses the caller's logical domain;
native admission and epoch validation remain with the existing authority.

Development verification under `target/wf/retained-speed-development-12e0c/`:
`macos-source-launch-app-c2.log` records app3,024/Linux46/newhelper7 PASS, zero
failures and seven/three existing ignores: 3,077 executed host cases. The seven
new cases exercise actual Mixer PCM/history, nonzero physical/playback cursors,
refusal preservation, exact subnanosecond queries, cold identity, capacity and
selected phase/preroll/end/domain checks. The first c1 test run exposed an
incorrect independently rounded preroll oracle; c2 checks explicit values
before and after preroll against the existing whole signed-phase contract.
No production rounding or clock guard was changed to fix that test.
`macos-source-launch-xcheck-c1.log` records successful macOS all-targets
desktop,webtransport Rust typing with C/archive stubs.

Known ceiling: these host tests do not execute macOS launchers or a CoreAudio
device. Real IOHID input, native startup, audible sync, output replacement and
physical latency remain unverified. The earlier macOS selected-speed launcher
exclusion is removed in source; Windows still needs positive-counter start
qualification and its two launcher integrations. Source history retirement
and complete ordered independent review/security/browser/CLI/desktop QA remain
open. WBS stays 91/193; the full BMS player Goal stays active.

Local Source counter startup must keep its real command-queue gate closed
until actual positive native observations are available. It schedules one
future Source frame using the real rendered frontier and applied native buffer
size, then requires the original observation pair to cover that exact frame
and the original producer to report the same applied gate frame. A late gate,
changed epoch/descriptor/frequency, invalid publication or timeout refuses;
there is no counter-zero or PCM-frequency substitute. Pause/end start frames
are staged before queue admission. Cancellation leaves cleanup to the original
output owner. This startup evidence establishes software/native correspondence,
not audible timing or a pause/end ACK.
This helper starts a fresh unarmed gate: playback has not advanced, so its
initial playback cursor must be zero. Prefill may advance the physical cursor
before history creation. Existing Source runtime/preparation paths continue
to preserve nonzero playback identities; this fresh-start helper does not
rebase or reset them. Coordinate observers are bound once by the existing
caller after startup, rather than by this helper.

The local Source start helper also bounds cooperative waiting with a checked
resource watchdog. It does not derive song time, native coordinates or input
timestamps from that timer. Existing public priming keeps its original HOST
contract; the watchdog is passed privately only by local gated startup. Port
calls must return for the watchdog to run; it does not preempt a blocked driver.

### Local Source gate development verification — 2026-10-13

`start_local_source_coordinate_native_audio` now stages pause/end start frames,
arms the original producer once and requires its actual applied frame plus
strict original `left < cut <= right` coverage. SourceFrame and SourceCounter
retain their creation descriptor, including u64 native frequency. Coherent
render facts provide the future Source cut; a separate scalar render cache is
not read. The producer's new read-only gate getter exposes existing queue state
without allocating, resetting the queue or granting native permission.

`target/wf/retained-speed-development-12e0c/local-source-gate-app-c3.log`
records app3,034/Linux46 PASS, zero failures, seven/three existing ignores.
All ten new gate cases pass, including actual Mixer PCM silence before the
gate and first positive sample at the applied frame, nonzero physical creation,
12GHz and nondivisible 12,000,000,001Hz counters, exact Source cut/HOST ceil,
stale facts, invalid initial state, cancellation/timeout before and after arm,
and stalled HOST with missing publications before arming. Core library12 and
queue/start-gate integration16 also passed in `local-source-gate-core-c1.log`
and `local-source-gate-core-queue-c1.log`. First c1/c2 failures were fixture
PCM byte capacity and instantaneous interpolation argument mistakes; c3 fixes
them without weakening production admission. Source code passed scoped format
and whitespace checks. Windows GNU all-targets Rust typing passed in
`local-source-gate-windows-c1.log` (C/archive stubs, no ASIO SDK feature).
macOS all-targets desktop,webtransport Rust typing also passed in
`local-source-gate-macos-c1.log`, with the same native-execution limitation.

Known ceiling: timestamps, submitted prefixes and WASAPI snapshots in these
tests are supplied DI, alongside genuine rendered PCM/history/queue state.
They prove no native device, audible latency or SDK execution. Frozen HOST
after arming, pause/end staging failure and real gate-admission races remain
outside the new fixtures. Zero-length playback that never publishes a positive
applied frame cannot claim success. Windows frequency discovery and both actual
launcher connections still remain to be implemented; the helper alone does
not enable those launchers. Full ordered independent reviews and browser/CLI/
desktop QA, Source history retirement and full player scope remain open.

Windows local launch must use the real published WASAPI counter frequency as
u64 and the original creation basis. Reading frequency creates the mathematical
descriptor; it does not admit counter zero. Descriptor discovery services the
original input collector and has a finite resource bound. Local startup then
uses the original closed queue and common Source gate helper with the actual
applied native buffer size. ASIO uses its original SourceFrame descriptor and
keeps existing same-thread SDK/window checks. Both local solo/cohort use the
same fixed-speed retained program, whole preroll, phase Transport and practice
pumps. The existing network admission path keeps its own constraints.
Before native open, Windows conservatively reserves Source history against the
configured Mixer render ceiling plus queued input/practice/anchor budget;
the actual native buffer is discovered afterward. This is a cold reservation,
not an allocation in rendering or a claim about actual buffer size.

### Windows Source launch development checkpoint — 2026-10-13

Both Windows launchers now install Source history before native open and use a
closed start gate for local retained play, including positive fixed speeds.
`Output::source_coordinate_basis` reads the actual published WASAPI u64
frequency; absent publication waits under the existing startup bound, while
a published zero frequency is rejected. ASIO uses its immutable SourceFrame
basis. `prime_coordinate_output` services the original input collector and
delegates gate admission to `start_local_source_coordinate_native_audio` using
the actual applied buffer size. Both launchers create Transport from the
qualified gate query on the original creation basis and call the existing
coordinate practice pumps; network startup retains its original path.

Windows GNU all-targets desktop,webtransport typing passed in
`target/wf/retained-speed-development-12e0c/windows-source-launch-xcheck-c3.log`.
Root host regression passed in `windows-source-launch-app-c2.log`: app3034,
Linux46, Windows portable59, actual CLI9 and cold Source integration7, totaling
3155 executed tests; seven app and three Linux existing ignores remain.
The Windows finite-end oracle now checks successful 20h/week and safely
representable near-limit durations, and refusal when i64::MAX plus preroll
exceeds the shared chronology. No production overflow guard was relaxed.
Scoped formatting and source whitespace checks passed.

Known ceiling: GNU typing uses C/archive stubs and excludes the authentic ASIO
SDK path. Host Windows tests exercise portable parsing/planning, not native
Windows playback. Actual Windows/macOS input, audible output, SDK execution,
latency and physical synchronization remain unverified. The large Windows
history reservation is conservative; Source history retirement remains opt-out
on these Source-coordinate paths, so this checkpoint does not establish
unbounded playback lifetime. Retained-specific CLI/lineage files named in the
original AC009 are still absent; the existing live-speed CLI suite is useful
regression evidence, not fulfillment of that whole AC. Ordered independent
DEEP code/security reviews and browser/CLI/desktop QA have not run for the
whole task and remain required before close. WBS stays91/193.

### Retained invocation and recording lineage verification

The shipped native help must describe local retained practice at positive fixed
speed and identify remaining runtime-speed/network restrictions. Actual command
tests must distinguish successful parser/cold chart preparation from missing
input or foreign-platform refusal; the latter is not successful native play.
Normalized selected speed must survive the original finite region and preroll
for both solo and local-member invocations.

An F5 retry, backward bookmark and finite loop selected from the same pinned
SessionLaunch must retain original chart, speed, roster and policy options.
Preparing their attempts must create fresh score/gauge owners and canonical
capture metadata for the selected region/speed while deriving distinct retry
and member recording paths from the original base. Capture decode, section
validation and replay reconstruction must consume the resulting headers.
Rejected actions must preserve the pinned invocation. These composed public
tests supplement fresh-owner fixtures; they do not prove GUI delivery of F5,
audible loop boundaries or native hardware synchronization.

### Source history lifetime and cold allocation gap

Current Source paths still need callback-owned retirement requests. Both solo
`native_gameplay` dispatch and `AudioTiming::retire_coordinate_history` currently
restrict retirement to TargetFrame. Removing one guard alone cannot enable it.
The eventual Source request must include the minimum exact future-input query,
pending pause/end cuts, queued and held practice references; the callback must
also preserve its own outstanding practice receipts. A queued request revision
is mathematical intent, not completed retirement or a native acknowledgement.
Existing Source publication already tracks retirement revisions and covering
spans, but no Source-only request mailbox/backend route is connected yet.

Known ceiling: Windows reserves 1,052,930 Source history slots from the configured
render ceiling and consumer allowance. Although Mixer history only reserves its
Vec, `SourcePlaybackPublication::new` initializes three atomic u64 words per
slot when native telemetry is created, roughly 24 MiB of slot payload before
allocator overhead. WASAPI creates this telemetry before its worker discovers
GetBufferSize. Therefore the current cold reservation has a real initialization
cost; it is not evidence of immediate loading. Tighter reservation must use real
native buffer metadata before publication construction without guessing buffer
size, reopening the endpoint or allocating on the callback.

### Retained invocation development evidence — 2026-10-13

The originally missing AC009 test files now exist. Host execution in
`target/wf/retained-speed-development-12e0c/retained-invocation-c2.log` passed all
six new tests: three shipped CLI cases and three recording-lineage cases.
After bounded independent inspection, the final lineage oracle was strengthened
to require the intended object's Instant Hit, independently calculated time
delta, event time and original input metadata. Both test targets passed again
in `retained-invocation-c3.log` (six executed passes, no failure or ignore).
CLI coverage includes standalone Linux/Windows/macOS and primary help, twelve
real Linux cold launches (three selected speeds, solo/cohort, standalone/primary)
against the same four-note chart/WAV, and foreign valid-region platform refusal
versus invalid-region parser refusal. Linux runs use ALSA null and absent evdev:
they verify section preparation and input-initialization refusal, not playback.
The original chart/WAV remain unchanged, and owned temporary files/processes
are cleaned. Initial CLI c1 matched only Display OS-error text; the final test
also accepts the actual structured NotFound/code2 initialization error.

Lineage tests compose F5 retry, backward bookmark, finite loop and preparation
ordinals, then real Runtime accepted Down/Up and finite completion through
canonical capture decode/section validation/replay reconstruction. Original
6/4 arguments normalize to 3/2 in capture metadata. Solo and local members7
and u32::MAX retain roster, policy and distinct original-base recording paths.
Rejected actions preserve the active owner; a previously selected loop rebases
against intervening retained retries. Cold invocation preroll100ms remains in
the pinned arguments; warm attempts correctly use zero-preroll phase transport.

Both new test targets also passed Windows GNU and macOS Rust typing with
desktop,webtransport in `retained-invocation-{windows,macos}-c1.log`. These are
C/archive-stub checks, not foreign test execution or authentic ASIO SDK builds.
After the final oracle change, both targets passed fresh typing again in
`retained-invocation-{windows,macos}-c2.log`. Scoped formatting and whitespace
checks on the final new tests and durable docs also passed.

Known ceiling: these tests do not deliver GUI F5/bookmark/loop actions or prove
native presentation/physical timing. Phase points in the public Runtime tests
are supplied mathematical input. The full AC009 user-facing acceptance and
ordered independent reviews/QA remain open; six test passes do not promote a
whole WBS item. Source lifetime/cold allocation gaps described above remain.
Cold CLI feedback exposes section start/excluded objects and normalized speed,
but does not expose applied end/preroll. Accepting those arguments is not
independent evidence that native playback applied them.

### Source retirement request contract

A Source-only stream-bound mailbox must carry the original output frame basis,
Source history identity and one physical minimum pin with an inline checked
request revision. Requests coalesce monotonically; duplicates return the same
revision, contention returns no request after bounded attempts, and identity,
regression or overflow refusal leaves the payload unchanged. No heap allocation,
lock or fabricated converted phase belongs in this request path. Generated
range qualification remains the stream publication's responsibility.

The statically typed output owner must obtain the existing exact Source packet
for the caller's minimum query, require native evidence, and preserve original
epoch/basis/history/query identity. It selects the packet's covering span start,
minimizes it with any queued or held Source pin, rechecks current output identity,
then queues that pin through the native backend. Missing facts wait; invalid
identity/evidence fails. This does not mutate presentation, cached render reports,
pause/end acknowledgement or playback state. A returned revision is request
intent; the callback owns actual compaction and must retain its own live receipts.
Ports and mailbox alone do not enable production retirement: native publication
qualification, callback application and all launch/device/pump routes must follow.

### Source request development checkpoint — 2026-10-13

Source-only inline mailbox, static backend port/output owner and shared telemetry
range qualification are implemented. Platform host regression passed361 tests
with one existing ignore in `source-retirement-platform-c1.log`: the new cases
include mailbox12 and telemetry8. App regression in `source-retirement-app-c1.log`
passed3043 with seven existing ignores, including nine new owner cases; Linux46,
portable Windows59, retained CLI3, lineage3 and cold Source7 also passed. This is
3522 executed host passes across both commands, with11 ignores not counted.
All logs live in `target/wf/retained-speed-development-12e0c/`.

The new tests exercise actual Mixer/history generation and original nonzero
physical/playback identity, exact subnanosecond queries, range/identity/refusal,
duplicate/overflow/busy/concurrent request coherence, zero request heap calls,
and unchanged cached/scalar/Source facts when only intent is queued. Owner
native evidence is supplied DI, not an actual hardware observation. Bounded
independent read-only inspection found no additional concrete defect; it is
not full formal review or QA.

Known ceiling: no native callback consumes these Source requests yet, and native
factories/devices/pump dispatch are not opted in. New private request APIs are
therefore unused in production and add dead-code warnings until connected.
This checkpoint does not repair production Source lifetime or justify WBS
promotion. Actual callback pin clamping/retry, native/SDK execution, tighter cold
allocation, full ordered reviews/QA and the whole player Goal remain open.

### Production callback retirement integration

WASAPI, CoreAudio and ASIO must enable the request mailbox from the same
Mixer/history and immutable stream creation basis before prefill. Their public
request routes qualify original stream basis/history/current publication;
app adapters and the live Windows wrapper recheck the active epoch before
queuing. All SourceFrame/SourceCounter solo/cohort pumps must forward the same
future-input, pending-control and queued/held practice minima as TargetFrame.

Only the owning callback/worker applies retirement, after destination admission
and before Source history render preflight. It clamps the requested minimum
to its actual outstanding Mixer practice references and retains the request
until those pins release. Completion of a request revision means only the
mathematical compaction was applied at its full requested minimum; it grants
no native presentation, submission, delivery or pause/end acknowledgement.
Render/failure publication must expose the compacted Source history without
inventing a successful render or rebasing original identity/cursors. The shared
application function and request paths must not allocate or block the callback.

### Retained desktop F5 contract

While the retained native owner is active, F5 must queue the existing Scrub
transaction at the immutable original invocation's start (`--start-ns`, or
zero when absent). A later bookmark, current attempt or active loop must not
change that target. The original section end and selected speed remain pinned;
the qualified boundary creates fresh solo/cohort attempts and recording lineage
on the same output owner. F5 must not cancel or respawn that worker.

Pending, paused, output-changing or refused requests preserve the current
attempt and loop state. Only a successful applied response disables repetition.
After the native worker has finished and joined, retry continues through the
existing fresh-launch path. This does not claim gapless output after cleanup.

### Production Source retirement checkpoint — 2026-10-13

The native factories, actual WASAPI/CoreAudio/ASIO callback owners, static
backend adapters, Windows remix wrapper, native launch devices and all three
retirement dispatch sites now connect the same Source request chain. This
supersedes the earlier request-only checkpoint's missing-connection statement.
Callbacks retain outstanding practice references and retry clamped requests.
CoreAudio now returns an enqueued revision even if a subsequent callback
changes status; a deterministic callback interleaving fixture covers that race.

`source-retirement-callback-platform-c1.log` records 367 host passes and one
existing ignore, including six new shared Mixer/application and SDK-independent
ASIO planar-renderer tests. Final all-target Windows GNU and macOS typing passed
in `source-retirement-callback-windows-c2.log` and
`source-retirement-callback-macos-c1.log`. Windows c1 found missing Source-port
forwarding on the remix wrapper; the final implementation forwards the same
owner, epoch, basis, history identity and pin. Native memory callback fixtures
are prepared under foreign cfg; they are not host-executed passes.

Known ceiling: foreign checks use C/archive stubs and exclude authentic ASIO
SDK execution. Physical devices, audible synchronization and native presentation
remain unverified. Windows cold publication still initializes roughly24MiB
from the maximum render ceiling before actual native buffer sizing. GUI F5
implementation, final app regression and ordered full review/QA are still open
at this checkpoint. No whole WBS item or player completion follows from it.

The subsequent frozen-source app command completed with 3045 app passes,
46 Linux and59 portable Windows passes, retained CLI3/lineage3 and cold Source7:
3163 executed passes with10 existing ignores. Together with the platform
command this is3530 host passes and11 ignores, not a foreign device run.
`source-retirement-callback-app-c1.log` contains the actual results, including
the two new SourceFrame/SourceCounter retirement dispatch tests. Final app
regression for callback connection is therefore proven at the host tier;
desktop F5 tests and whole-task review/QA still follow separately.

### Windows cold history sizing contract

Retained Windows startup must size new Source history from the actual native
buffer plus the existing4354-slot consumer headroom, before initial rendering.
WASAPI uses its successful validated GetBufferSize; ASIO uses the frames actually
resolved inside native stream preparation. An earlier constraints query or
maximum render ceiling is insufficient. Allocate publication on that same
existing cold owner; do not reopen an endpoint or allocate in the callback.

Existing history must retain its capacity, original identity, spans and retirement
revision across recoverable opens. Cold allocation/preparation failure must
preserve recoverable Mixer ownership. A pure pre-open identity capture uses
actual Mixer cursors, with no render between capture and history creation;
post-prefill cursors must not be substituted as creation identity.

### Retained desktop F5 development evidence — 2026-10-13

Desktop F5 now queues the existing Scrub transaction on an active retained
owner. Seven focused control tests passed in `retained-f5-desktop-c1.log`.
Final full primary-binary regression passed372 with13 existing ignores in
`retained-f5-desktop-full-c2.log`. Full c1 exposed a stale fixture expecting
F5 cancellation; it was replaced with genuine application of F5 through the
existing worker, Mixer, converter and gameplay pump before explicit cleanup.
The test confirms a single output open, unchanged worker thread, actual fresh
recording ordinal and capture generation, and original PCM/input/replay checks.

Additional key/channel cases cover an immutable start123 against current
start789/bookmark456, speed6/4 and original recording base, duplicate pending
requests, refused edits, paused/output-pending refusal, loop clearing only
after an applied reply, and fresh joined retry lineage. These tests invoke
Desktop key dispatch directly; they do not deliver physical OS keyboard input.

Known ceiling: the genuine pump fixture supplies native-shaped DI observations;
it does not prove hardware presentation or acoustic timing. Interactive desktop
QA and ordered full review/browser/CLI/desktop QA remain required before close.
The cold geometry changes are implemented separately and still await tests
at this checkpoint. WBS91/193 and the full player Goal are unchanged.

### Selected-speed BGM voice reservation

Every retained solo/cohort launcher must reserve BGM voices with the same
discrete lifetime envelope as the core practice program at the selected rate.
Compute the Ceil output-frame lifetime, then its Ceil original-song extent
at that rate; the shared core calculation is the authority for both program
admission and launcher budgeting. A NORMAL-only estimate must not reject valid
selected-speed preparation despite ample total voice capacity. Preserve the
existing NORMAL helper and checked total/gameplay voice limits.

The regression is a48000Hz one-frame sample with cues0ns/25000ns at3/2 speed:
the NORMAL estimate is1; the exact conservative program envelope requires2.
Tests must compose the real budget helper and retained preparation, rather
than supply a manually enlarged overlap that bypasses launcher estimation.

### Windows native-buffer sizing development evidence — 2026-10-13

WASAPI now initializes new opted history after its validated GetBufferSize,
then constructs publication before prefill on the same worker. The existing
opened channel transfers that worker's shared Control to the stream. ASIO's
opted preparation uses its actual staged frame count before renderer/priming.
Both Windows launchers capture cold identity without allocating history and
delegate sizing to those native paths. Existing history and legacy prelaunch
allocation refusal remain preserved.

`native-history-geometry-platform-c1.log` passed371 with one existing ignore;
`native-history-geometry-app-c1.log` passed3045 app, Linux46, portable Windows59,
retained CLI3/lineage3 and cold Source9:3165 with10 existing ignores. New tests
cover geometry/refusals, existing retired-history conservation without heap
calls and pure creation identity. At supplied geometry64 frames, the actual
publication uses4418 slots/106032 bytes versus25270320 bytes at the former
maximum-ceiling reservation. This is storage evidence, not measured loading
latency. Final Windows all-target stub typing passed in
`native-history-geometry-windows-c1.log`.

Known ceiling: authentic ASIO SDK, foreign device execution, physical latency,
interactive QA and full task acceptance remain unverified. Independent full
code review found a separate NORMAL-only launcher overlap reservation defect;
its shared selected-speed remediation is underway. The independent security
review reported no concrete finding on this reviewed source. Review results
do not replace ordered QA or whole WBS completion.

### Duplicate native coordinate control admission

A repeated native frame or counter with later host metadata is an unchanged
observation, not a fatal playback error. Control acquisition must wait when
that candidate's provenance was not retained. It must preserve the previous
accepted host age and exact control-facts evidence checks. Ordinary playback
packet/report updates still operate independently of native coordinate progress.
An exact repeat of the already admitted evidence remains usable for control:
new render or pause reports must not wait indefinitely for native clock progress.
Source and converted Target owner regressions must prove duplicate waiting,
subsequent progress, and continued refusal of invalid observations. The current
full review identified this defect. The shared owner now retains admission
disposition from its one acquisition and defers only unchanged candidates whose
original evidence differs from the retained record. Linux solo/cohort converted
Target control uses the same owner API. Four regressions exercise actual
SourceFrame/Counter and Target owners, newer real Mixer/converter reports,
accepted-age expiry, continued progress and invalid regression refusal.
The final app suite passed 3,568 with 23 existing ignores in
`duplicate-control-app-full-c2.log`. The first focused attempt passed 45 and
failed one new Counter oracle: unchanged HOST with changed QPC violates native
monotonicity, so that negative case now correctly refuses without state changes.
The third full independent DEEP code review and second security review passed
with zero findings. Subsequent interactive QA results are recorded below;
this task has not passed its final acceptance gate.

### Selected BGM reservation development evidence — 2026-10-13

The six native launchers now reserve BGM overlap with the selected speed using
the core's exact discrete voice extent. The one-frame 48 kHz case at starts
0 and 25,000 ns needs two reserved voices at speed 3/2; an ample total voice
budget alone does not correct an undersized BGM partition. NORMAL retains its
existing duration semantics. Core practice tests passed 20; app/lib and main,
Linux and portable Windows suites passed 3,522 with 23 existing ignores.
The subsequent external suite passed 42, including real BMS/PCM loading and
12 Linux standalone/primary solo/cohort initialization routes. Earlier failed
attempts remain recorded: the new CLI import and NORMAL feedback expectation,
and the mixed-Hz legacy overlap expectation were corrected in tests.
Windows all-target stub typing passed; this does not execute native Windows.

Known ceiling: these earlier results preceded the duplicate-coordinate control
correction above. They do not complete interactive QA or establish native device,
acoustic latency, full task acceptance, or whole WBS completion.

### Independent QA and commit checkpoint — 2026-10-13

CLI QA passed at the available software tier: core 731, platform 371 and
application/binaries/selected external suites 3,661 tests passed, with zero
failures and 25 ignored tests. Another 24 direct shipped CLI cases passed.
Evidence is under `target/wf/retained-speed-qa-cli-12e0c-c2/`. Missing-input
startup refusals do not prove successful physical gameplay.

Browser QA failed: the strict production runner passed 10 of 14 flows, then
failed local touch acquisition. The actual UI reported that touch arrived
behind the accepted gameplay watermark. Audio WASM initialization also recorded
`net::ERR_ABORTED`. Evidence is under
`target/wf/retained-speed-qa-browser-12e0c-c2/`; neither failure was excluded.
Unchanged diagnostic runs subsequently passed 14/14 and 13/14 respectively;
the latter reproduced the network failure. These do not supersede formal FAIL.
Two controlled diagnostics executing actual `main.js` reproduced a 12.25 ms
delayed touch terminating playback and an in-flight prefix ACK followed by
submission of an older original touch. Their ignored artifacts are under
`target/wf/retained-speed-delayed-touch-unit-c2/`. No late-input recovery fix
has been implemented or verified.

Desktop QA returned BLOCKED_ENV after actual same-window GUI checks passed:
speed validation/persistence, Practice/backstack, exact start/end fields,
resize, startup refusal and joined-error F5 retry. Missing `/dev/input` and
`/dev/snd` prevent active native playback. Evidence is under
`target/wf/retained-speed-qa-desktop-12e0c-c2/`.

Known ceiling: physical input/audio, active retained F5/pause/loop/drain,
authentic foreign SDK/device execution and acoustic synchronization remain
unproved. Agent finals do not replace missing task-bound attestation. Local
checkpoint commits preserve implementation and tests without claiming task,
WBS or whole-player completion; WBS remains 91/193. The native Goal was paused
by the user before these commit checkpoints.
