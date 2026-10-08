# Exact native target time and converted boundaries

Pure target ownership and pause/rebind tests are portable across supported Rust
targets. Shared converter rigs and controlled original association values may
be constructed without platform-specific drivers, with that evidence tier
explicit. Linux-specific ALSA timestamp/snapshot tests retain their actual
association checks. A missing native fixture on another platform must not be
hidden by skipping the otherwise portable business tests.

Replay conversion must preserve the existing saved timeline, source commands,
section and preroll. `play_replay_bms --rate` remains the PCM preparation rate;
Linux-only `--output-rate` selects the native target rate and defaults to source.
Unsupported platforms refuse that override before effects. One existing replay
loop consumes typed target identity, coherent source/converted boundary facts
and original native associations. Pause holds native output only after mapped
ACK and releases hold before producer resume. Finite completion requires the
original source endpoint, exhausted feeder and completed visual plus mapped end
and native crossing. Natural drain maps the real idle source frontier through
actual converted generation, retains that target barrier across silence/held
output and waits original native crossing. Source pull/submitted frames cannot
prove replay presentation completion. Refusals preserve complete ownership and
admitted pause/completion state. Null output tests remain functional evidence;
clock-capable and other backend proof remain mandatory.

Replay target library development verification passes six focused fixtures
(`replay-target-library-focused-development.log`). They drive actual converted
state and source commands through pause/held/resume, finite mapped completion,
idle lookahead/held silence and skipped telemetry recovery with atomic identity
refusal. An unmapped idle barrier missed between telemetry reads may be replaced
by a later real idle source frontier; completion can be delayed conservatively,
but song and cue time do not shift. Once mapped, the target barrier is retained.
These library fixtures do not complete the replay binary consumer or physical
native presentation proof.

The Linux replay binary consumer now uses the complete converted output owner
with direct coherent telemetry and original target associations. It has no
unused AudioAuthority/input-merger history or alternate playback clock. Combined
development checks pass library 2105 / fail 0 / ignored 4 and replay binary
26 / fail 0 / ignored 1 (`replay-target-app-consumer-development.log`). The actual
null factory test separately passes (`replay-target-null-development.log`),
covering held source freeze, complete stopped converter recovery and original
future cue execution. This remains submission/recovery evidence, not native
played-frame timing. Other OS/backend conversion and clock-capable replay play
remain outstanding; fresh independent review/QA is still required.

Status: exact target-time and converted-state development checks passed; native
startup/pause/resume/end helpers and their integration fixtures are being
verified. Actual launcher/replacement integration and hardware acceptance remain
open. Same-rate continuity and portable phase/history prerequisites are verified.

Represent exact physical output duration across target rate epochs and held
segments without flooring each segment. The next native stream starts at the
exact duration of its first unsubmitted target frame. Original native presented
frames use the actual target rate; source pull frontier and consumed rational
source position cannot substitute for this physical basis.

Retain one Mixer/converter/pending target owner from stream construction.
Block extent/rate/interpretation and positive admission are target facts;
source RenderReport remains command/playback evidence. Cached nonzero output
with empty/paused source reports cannot prove target silence, pause or end.
Incompatible old PCM cannot be dropped or relabeled during retarget.

Source pause adoption, target pause boundary and actual native presentation
precede acknowledgement/held release. Held silence freezes source phase/history
while physical duration advances. Startup explicitly suppresses target output
before the mapped source gate. Preserve current strict exclusive finite-end
clipping; completion waits actual target crossing rather than source lookahead.

Checked rational/LCM/time/cursor/capacity/allocation failures refuse cold without
changing the original owner. Rendering uses prepared bounded storage/static
dispatch without callback heap or locks. Legacy fixed-grid APIs remain
compatible, and actual ALSA worker/gameplay/output-master input/pause/end/
replacement paths must use the new facts. Portable arithmetic alone is not
native enablement or hardware/acoustic proof.

Resume requires equivalent genuine source adoption and mapped target crossing:
cached paused PCM can remain after a nonempty source resume report. Retain that
mapped resume boundary independently of latest/coalesced reports and held spans;
neither source resume nor target-held release alone authorizes native resume ACK.

Scheduled startup chooses the latest admitted deadline endpoint. Project exact
physical duration and retained fractional source phase to a source gate with
one final source-frame ceiling, then map the adopted gate to the actual target
sample boundary. An intermediate target-frame ceiling must not add an extra
source sample. A 44.1 kHz source / 48 kHz target deadline of 589 microseconds
selects source gate 26 and target frame 29; the actual generated PCM and retained
startup fact must agree before that origin can become playback authority.

Shared output lifecycle ownership must not require fixed-grid software timing.
`OutputReplacementBackend<O, Basis = OutputFrameBasis>` and the existing
replacement controller retain legacy defaults while permitting a complete
converted owner with `TargetFrameBasis`. Attach/retire/recover/cancel/retry
preserve that owner and its pending suffix. Legacy timing methods remain
constrained to their original capability; a converted state must not implement
that capability or manufacture a source-grid basis to satisfy a lifecycle bound.
This type foundation alone does not complete target publication or launchers.

Target rebind stages the planned new physical basis and source/end facts before
the complete software owner enters the worker. Retained zero-valued active PCM
is insufficient held proof: genuine held generation provenance survives partial
admission and cold refusal. Earlier retained-tail observations withhold native
anchors; fresh held generation and original native progression must reach the
target frontier before publication. Source callbacks and frozen playback remain
separate validation facts.

Publication validates the input merger's host-domain and committed frontier
even when its queue is empty. A different host domain or regressing publication
frontier refuses before authority commit and returns the candidate output and
original producer hold; the old gameplay owners remain unchanged.

Development evidence (2026-10-08): the current app library run with desktop and
webtransport features passes 2,050 tests, with four environment-dependent tests
ignored. All six target replacement transaction fixtures pass, including empty
wrong-domain merger refusal and source BGM/keysound FIFO timing across a rate
change and held gap. This covers the controller transaction and deterministic
native effect boundary; actual launcher integration and physical alignment
remain separate required evidence.
The actual ALSA null endpoint test also passes separately with the production
`prepare_replacement_start` hook selecting Held before native start, followed
by native progress and stop/join/full converter recovery. Null output does not
prove physical-device or acoustic behavior.

The existing generic GameplayOutputOwner must own converted target outputs
directly. It caches source command reports separately from converted reports
and target boundary facts, admits only original observations matching its
immutable creation epoch/basis, and publishes replacement facts only after a
successful authority commit. A refused ready target output remains owned with
its producer hold until explicit cancellation retires it. Target finite-end
checks use persistent boundary facts and original native crossing. The shared
pause/resume loop and launchers must consume these target methods explicitly;
the owner API alone does not establish complete launcher integration.

Shared live pause consumes a typed Target observation carrying the actual epoch,
creation basis, boundary facts, source report and original native pair. Requests
and mapped pause/resume observations are staged together; source-rate, epoch,
basis or domain refusal leaves the admitted pause unchanged. Logical song time
uses source playback frames and source sample rate, while the physical cutoff
comes from the mapped target transition. Legacy source-grid replacement paths
refuse Target evidence instead of reinterpreting it as a source Point.

Failed retirement must keep the producer hold alongside the pending native
output until recovery actually finishes. While retirement remains pending, the
owner reports both output-clock suspension and replacement pending, preventing
resume or progress through stale output observations. A successful retry can
release the hold only after the native worker is stopped and ownership recovered.

Follow-up development evidence (2026-10-08): app library tests pass 2,065 / fail 0
with four environment cases ignored. Six actual typed GameplayOutputOwner cases
and four shared Target live-pause cases pass, including pending retirement hold
retention and fractional 44.1-to-48 kHz native crossing. Current browser WASM
type checking also passes. These deterministic owner/pump boundary checks do not
replace actual launcher, native device, browser QA or acoustic acceptance.

Solo and cohort pumps share an explicit native held-output effect port. For
Target evidence, an acknowledged mapped native pause enables held output;
an accepted resume disables held output before releasing the source producer's
pause request. Replacement-pending prevents resume. An effect refusal must not
release the producer or publish resume success; stage pause state until required
native effects succeed. Legacy source/interval adapters keep their prior behavior.
The effect changes no source scheduling origin, source rate or input timestamp.
Target evidence requires the original audio-authoritative pump; legacy
host-clock pumps refuse it before invoking held effects or changing pause state.

Converted output settings use the same correlated request/reply owner as legacy
outputs. Advertised target rate, buffer/period and channel matrix reflect the
actual applied native configuration, independently of source PCM rate. Legacy
capabilities without target-rate support do not expose a rate editor; unsupported
requests refuse before native effects. Rejected incompatible pending PCM retains
its original interpretation and complete owner. This bridge requires actual
launcher consumption before live output UI is considered fully integrated.

Converted UI development evidence (2026-10-08): the combined app library run
passes 2084 tests / fails 0 / ignores 4 (`converted-ui-app-development.log`).
Three actual target-owner request/reply fixtures cover retained-tail publication,
reply backpressure/retry, atomic mapping refusal and complete recovery on native
effect failure. Four ALSA settings fixtures validate target rate/sizes/matrix,
applied metadata and absence of a rate editor in legacy capabilities. The shared
cold service is statically generic over the original owner and frame basis.
Linux solo/local startup and settings consumers are now connected, including
network committed target start. Linux replay conversion is connected (E26);
other OS/backend conversion and clock-capable replay/endpoint acceptance remain
required. Bridge fixtures do not prove physical playback.

Linux non-network solo must consume the real converted stream, target owner,
target startup prime, source reports and source PCM configuration through the
existing solo runtime. Pause/end observe mapped target facts and original native
crossing; held selection and target settings use the shared tested ports.
Network and local consumers now use target output through the shared typed
committed-start path; their functional/physical verification remains separate.
This staging does not complete those paths or reduce
the original requirement for all modes and backend/rate combinations.

Offline Linux local cohorts must use the same converted output owner and target
startup/held/pause/end/settings contracts as solo, with one asset bank and one
audio output shared by all players. Each player's input collector, judging,
capture and selected policy remain independent. Source scheduling stays on the
immutable source PCM rate; output changes must not rebuild the cohort or shift
its song origin. Network committed-start consumers now use the actual target
gate and original native evidence ports; clock-capable socket-to-driver playback
verification and target interval support remain separately incomplete.

Committed target start plans retain the immutable source rate as well as the
creation basis and selected source gate. Startup boundary validation must reject
facts from another source rate even if their origin, gate frame and mapped target
time match. A successful start must prove the original source identity before
publishing its output origin.

The focused target startup suite passes 2 tests / fails 0
(`target-start-source-identity-development.log`). The actual 44.1→48 kHz owner
oracle retains source gate 47 and mapped target frame 52; otherwise identical
startup facts carrying source rates 0, 24 kHz or 48 kHz are rejected. Plan creation
basis and owner continuation basis are checked independently for refusal
immutability. This verifies the projection/identity boundary, not the still
pending functional verification of the network launcher integration.

Committed source and converted-target startup share one bounded orchestration
loop for calibration, input acquisition, agreement, cancellation and host arrival.
Static policies select distinct typed plans. A target adapter exposes its immutable
epoch/basis and a coherent optional converted telemetry tuple; no running software
owner is extracted. Calibration uses target buffer/rate, while commands, BGM and
pause/end source frames retain the original source rate. Missing telemetry waits
under a deadline. Arming publishes staged observers only after exact source-gate
admission; confirmation requires actual mapped startup facts and genuine native
crossing. Existing point/interval source adapters retain their behavior.

Known ceiling: committed converted-target startup currently accepts original
point clock associations and explicitly refuses interval evidence. Legacy source
startup retains interval support; target interval backend integration remains
required before claiming all native backends supported.

Shared committed-start development verification passes application library
2099 tests / fails 0 / ignores 4 (`target-committed-app-development.log`). Six
new controlled-IO tests drive a real 44.1→48 kHz converted owner through the
public startup loop, covering exact source gate/mapped onset, original crossing
and delayed host arrival, telemetry recovery/timeout, identity refusal, input
retention on cancellation, missed gate refusal before observer publication and
genuine zero-length endpoint facts. The Linux network consumers also compile
(`target-committed-linux-check-development.log`). These fixtures do not prove
socket-to-driver or physical network playback synchronization; real endpoint
and independent review/QA evidence remain required.

The offline local consumer now selects `CohortOutput::Target` and reuses the
solo cold output factory, target timing configuration and delayed-end priming
helper. Its actual cohort device forwards target pause/end observations, held
effects and typed output settings through the existing group pump. The network
branch now reuses the same native target startup adapter as solo, while the
source/target policies share one committed-start orchestration loop. The Linux binary type check
passes (`linux-target-local-check-development.log`). The Linux binary suite passes
34 tests / fails 0 / ignores 3 (`linux-target-local-bin-development.log`). The
explicit null local wrapper test separately passes 1 test
(`linux-target-local-null-development.log`), covering unequal-rate held submission,
typed observation/refusal, complete source-owner recovery and original BGM cue.
Independent review/QA remains pending.

Known ceiling: native clock and physical playback evidence remain outstanding
for the offline local converted consumer. Its type check proves compilation,
not input acquisition, presentation accuracy or a complete multi-player play.

Linux consumer development evidence (2026-10-09): library 2086 PASS / fail 0 /
ignored 4 and Linux binary 34 PASS / fail 0 / ignored 2. The actual null probe
failed its native-clock premise: the native state stayed PREPARED (2), delay was
zero and estimated_played_frames was unavailable despite positive PCM writes.
No original clock pair was admitted. Submitted/rendered frames must not replace
that unavailable native played-frame evidence. Null remains usable for output,
held/cue/recovery functional tests; finite native crossing requires a genuinely
clock-capable endpoint and remains an outstanding acceptance obligation.
Separately, the target owner previously returned before admitting an original
clock snapshot when auxiliary boundary telemetry was unavailable. Independent
clock admission and coherent auxiliary caching now address that code defect;
it is not asserted to have caused the null clock unavailability.

The corrected null functional probe passes separately: actual held submissions,
source-phase freeze, stop/join, complete converted-owner recovery and first BGM
PCM at the original source cue are verified (`linux-target-solo-null-functional-fixed-development.log`).
It explicitly verifies no original clock admission when played frames are
unavailable. The positive finite-crossing fixture requires
`BEATKERNEL_TEST_ALSA_CLOCK_DEVICE` and remains unrun without that endpoint.
All current application binaries also pass type checking after the Linux solo
consumer changes (`linux-target-solo-bins-fixed-development.log`).

Original native clock admission is independent of auxiliary converted telemetry
availability. Source report, converted report and mapped boundary facts must be
read as one coherent optional snapshot; absent telemetry retains the previous
tuple rather than installing default facts or mixing generations. Available
identity mismatches refuse before authority/cache mutation. Missing auxiliary
facts alone do not discard a valid original epoch/basis-bound native observation.
Target replacement publication still requires genuine fresh held facts and
source adoption, so a clock-only observation cannot authorize replacement.

Finite startup retains the first original native pair even when auxiliary facts
arrive later. End priming completes only after actual mapped source/end facts
are available; a clock-only observation cannot mark that work done or emit an
endpoint. Reads remain bounded and nonblocking; no synthetic frame, target-rate
source inference or unbounded retry loop is introduced.

Coherent telemetry development verification (2026-10-09):
`coherent-target-app-development.log` reports application library 2093 PASS /
fail 0 / ignored 4 and Linux binary 34 PASS / fail 0 / ignored 2.
`coherent-target-platform-development.log` reports platform library 131 PASS /
fail 0 / ignored 1. Tests exercise missing auxiliary data, malformed identity
rollback, complete tuple generations, replacement deferral/timeout, seeded facts,
bounded unavailable reads, concurrent publication and delayed finite-end priming.
These controlled-port and atomic-publication tests do not prove physical playback
clock availability or complete all launcher modes; independent review/QA remains
required.

Verification interruption (2026-10-08): the latest focused audio-authoritative
shared-pump run, `target-held-pump-audio-focused-development.log`, ends at
`Compiling` with no test result. Its process is absent after the environment
restart, and the retained test executable predates both the rewritten fixtures
and the legacy-target refusal guards. Do not execute that binary as evidence for
these changes or count the four rewritten pump tests as passed. The earlier
2065-pass run remains bounded evidence for owner/retirement/clip changes before
this pump integration; it does not verify the current shared-pump end state.
Resume with a fresh focused build/run after checking available resources, then
run the combined suite and the required independent review/QA sequence.

Bounded recovery evidence (2026-10-08): a fresh focused run with one Cargo job,
an inherited 8 GiB address-space limit and a 180-second timeout passes all four
audio-authoritative shared-pump tests. After excluding the solo legacy HOST
Transport resume calculation from audio mode, the combined app library run
passes 2072 tests / fails 0 / ignores 4 (`target-held-pump-bounded-app-development.log`).
This supersedes the interrupted run for these changes. Both solo/cohort tests
use actual public audio pumps, a real converted state and original target
associations through controlled IO ports. They cover native crossing/held/resume
ordering, replacement deferral and effect-refusal rollback; physical driver
execution, launcher integration and independent QA remain separate obligations.

The existing runtime consumes two distinct time values: AudioAuthority/Transport
use original raw-target observations for judging, while source scheduling uses
the actual nonempty source RenderReport playback cursor and source PCM rate.
Converted launchers retain `logical_schedule=true`, `render_report()` as the
last real source callback, and `config.sample_rate` as source rate. BGM/cue
origins and command timestamps must not receive target held gaps or device-rate
offsets; raw-native `fallback_schedule()` is unavailable in converted mode.
At 1 kHz source playback cursor 1000, a one-second target-held span may move the
native clock to two seconds, but the next source command still belongs at one
source second. Using the two-second native point as command time would add an
unintended one-second delay. Rate/held/rebind tests must compare actual command
timestamps/FIFO and PCM source cue placement, separately from native resume ACK.
