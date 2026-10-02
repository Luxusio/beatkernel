# Audio runtime contract

This is the implementation contract for Phase 7 of [the original plan](../../plan.md).
It describes required behavior; it does not attest that audio implementation or
native playback has passed verification. Native device control belongs to
[the platform contract](../platform/REQ__windows-audio.md).

## Loading and ownership

- Decode assets before starting output. RIFF WAVE PCM16/24/32 and IEEE float32
  become owned finite interleaved samples with explicit rate and channel count.
  Extensible WAVE accepts known PCM/float subformats only, with consistent
  container width, valid bits, block alignment and channel mask.
- Integer extensible PCM permits 1 through container-width valid bits, stored
  left-aligned; floating PCM requires 32 valid/container bits. A zero channel
  mask denotes direct output without speaker assignment. Nonzero speaker masks
  require one assignment per channel in this loader. RIFF pad bytes need only
  exist; their value is ignored. Plain PCM's unused cbSize is ignored.
- Validate declared RIFF size, all chunk bounds and odd padding, one fmt/data
  chunk each, frame alignment, nonzero rate/channels and allocation limits.
  Data may precede fmt because complete validation occurs before decoding.
  Reject unsupported compression, invalid samples and malformed input with
  typed errors. Empty valid data is safe and produces no active voice.
- Check both per-asset and aggregate bank limits. Samples remain owned for the
  output worker's lifetime; natural voice completion does not free an asset.
  Channel counts must match the mix configuration unless explicitly converted
  before loading. Source/output rate differences use linear interpolation.

## Scheduling and deterministic mixing

- Construct a mixer with an explicit scheduling domain, nanosecond origin,
  fixed output sample rate and bounded capacities. It owns an absolute output
  frame cursor. Render advances contiguous frames from caller-provided buffers;
  callback boundaries never create new rounding anchors.
- Convert command timestamps to absolute frames with checked signed ceiling
  division. Exact block-end commands execute in the next block. Negative
  preroll and late commands execute at the first available frame and count as
  late. Scheduled buffer position does not prove physical presentation time.
- Play identifies a caller-selected voice and sample, target time and finite
  signed gain. Stop addresses a voice. SetRate uses the existing rational Rate;
  Seek sets observable song position at its output execution frame. Equal-frame
  commands run in submission order; otherwise target frame determines order.
  Future commands must not block a newly admitted earlier command.
- Forward Play starts at the first sample; reverse Play at the last. Zero rate
  starts at the first sample, emits silence and preserves read heads. SetRate
  affects active and future voices; direction changes preserve fractional heads.
  Retain fractional progression across render partitions. Retire at either
  sample boundary; interpolation must never access outside the asset.
- Seek clears active voices, retains rate and future output-scheduled commands,
  and changes the song-position anchor without rewinding output chronology.
  Seek then Play and Play then Seek have distinct submission-ordered results.
- Validate commands before affecting existing voices. Duplicate Play voice IDs
  replace only that voice; unknown Stop is a counted no-op. Unknown samples,
  non-finite gain, capacity exhaustion and unrepresentable arithmetic reject
  that command with distinct counters. Other commands at that frame continue.
  Extreme Rate values must use checked arithmetic or explicit typed rejection,
  never undefined casts or partial changes to unrelated voices.
- Sum voices in a fixed deterministic order using sufficiently wide amplitude
  arithmetic, then clamp once to [-1, 1]. Validate output buffer alignment,
  configured frame limit and frame-cursor arithmetic before consuming commands
  or changing state. Partitioning the same output frames must preserve samples
  and command behavior for commands admitted to the mixer before their execution
  frames, including at 44.1 kHz and nonzero origins.

### Explicit scheduling pause

CommandProducer::request_pause(bool) sets desired state independently of the
command ring. A full ring or a paused consumer cannot prevent a resume request.
Requests may coalesce before the mixer renders; command admission counters and
ring slots do not change. Runtime::request_audio_pause and the solo/local
runtime adapters reach this same producer. These methods control audio only:
the application must separately coordinate Transport, input and judging.

The mixer adopts the desired state at the start of a valid nonempty block,
after checking buffer alignment, limits and cursor arithmetic. Empty or rejected
renders do not acknowledge requests, consume commands or change output/state.
An explicitly paused block emits zero PCM and advances the physical output
cursor and rendered-frame counter. It does not drain the ring, execute pending
commands, advance PCM heads or alter rate, active voices and the Seek anchor.
Resume continues the exact rational heads and original ordered command targets.
SetRate ZERO remains distinct: it silences/freezes heads while scheduled commands
and the playback cursor continue, preserving its previous contract.

RenderReport.start_frame/frames and counters.rendered_frames always count
physical output, including inserted silence. playback_start_frame/playback_frames
count scheduling progress excluding that silence. Fully silent paused blocks
report zero playback_frames; a block straddling an immutable playback end reports
its actual active prefix. Report.paused is the applied state at block end, and shared native
telemetry preserves all three fields. A successful nonempty report is evidence
of rendering, not physical presentation or an application pause acknowledgement.

Before the first explicit pause, physical and playback grids coincide. After
pause, AudioCommand.at continues on the original scheduling grid relative to
MixerConfig.origin, excluding paused physical frames. Pending commands therefore
need no timestamp rewrite. New commands mapped from native physical time must
be converted to this playback grid by the session coordinator. Rolling native
BGM admission uses completed playback frames and waits during paused reports;
native physical clock/presentation observations retain their output grid.

Known ceiling: this is the audio scheduling primitive. Linux ALSA, Windows WASAPI
shared/exclusive and macOS CoreAudio solo/local 2..64 BMS owners coordinate observed
presentation boundaries, Transport/input fencing and song-time capture. ASIO,
and network owners still require that integration. Output-only replay Watch
also uses the same native/mixer pause boundaries and cumulative playback gap;
complete native/GUI timing acceptance remains unfinished. Output already buffered in
a native device may continue to present after a render pause begins. Paused
queue storage remains bounded; callers must stop generating gameplay commands
until coordinated resume. No callback allocation, lock, rate rewrite or worker
thread is introduced; native/GUI/timing and allocation fixtures remain unexecuted
under the user's deferred verification policy.

### Exact phase and boundary arithmetic

MixerConfig optionally fixes an exclusive playback_end_frame before construction.
The default has no end fence. The mixer executes frames strictly below that
endpoint, zeros the suffix of a straddling block, and freezes the playback cursor,
voices, rational sample heads, pending commands and song anchor at the endpoint.
Physical output frames/counters continue through silence. A block ending exactly
at the endpoint may report playback_frames==frames with paused=true; otherwise
paused prefix extent is 0..frames. Valid nonempty render adopts this state; empty
and invalid render do not consume commands or change acknowledgement. A zero end
consumes no commands. Manual queue pause may occur earlier, but resume cannot
lift the immutable end; fresh session construction owns repetition.

Native boundary models derive a straddling pause from the reported playback end,
not its block start, and still wait for actual presentation crossing. Repeated
coalesced partial reports cannot advance a frozen prefix. This adds no callback
allocation, deallocation, lock, queue command or telemetry wire field.
Runtime's separate immutable original-song end caps input admission and judging
without changing the physical or playback grids. Its song_end_reached report is
logical evidence only; actual native presentation must still cross the audio
boundary before an owner acknowledges completion or disposes the session.
RenderReport also retains playback_end_physical_frame, the actual physical frame
where a valid nonempty render first reaches the configured immutable endpoint.
Unlimited/manual-only pauses report None. Zero end records frame zero on the
first nonempty render; empty/invalid buffers do not adopt a marker. Once reached,
the marker survives silent callbacks and resume requests, so latest-only native
telemetry cannot lose the boundary or substitute a later silent block. Native
scalar publication preserves presence separately from its full u64 value.
Finite NativePause sessions opt into an expected playback end during setup.
Only matching retained endpoint evidence may coexist with manual pause/resume;
a short resume ending in the same rendered block derives its physical pause gap
from the immutable marker, excluding the later terminal silent suffix. Resume
acknowledgement still waits for actual native presentation. Unlimited sessions
keep rejecting unsolicited paused reports. Endpoint completion remains distinct
from manual pause and requires the finite native owner's input/capture cleanup.
After observing an immutable endpoint, further manual pause/resume requests are
no-ops; a fresh owner is required to play again.
ReplayPause forwards the same setup-only finite endpoint option for composed
finite output, preserving its recorded-song projection and strict unlimited
default. Native Watch owners remain unlimited unless explicitly configured.
Known ceiling: the fence is an audio component. Native BMS owners still need
explicit endpoint intent, Transport/input/judging/capture admission and cleanup
integration before graphical loops can claim an exact native endpoint. Playback
frame mapping rounds upward once and cannot provide subframe acoustic precision;
gapless repetition and native timing/allocation execution remain unverified.

Sample heads retain an integer frame and an exact rational fractional remainder.
The increment is the reduced ratio of source sample rate times signed playback
rate to output sample rate. Changing rate preserves the fractional head. Before
changing any active voice, check that every required common denominator and
scaled increment fits signed 128-bit arithmetic. If a sequence of incompatible
denominators exceeds that representation, count an invalid rate and retain the
previous rate and all heads. There is no rate magnitude preset; the extreme
numerators and denominators accepted by `Rate` remain supported when this
arithmetic is representable.

A head is active in the half-open interval from zero through the asset's frame
count. Interpolation holds the final sample when its upper neighbor would be
outside the asset; crossing either boundary retires the voice. Rendering checks
the unsigned 64-bit output frame cursor and block extent, independently from the
signed nanosecond command range. It does not impose an additional nanosecond
limit on output duration. Negative command frames remain ordered by their
original target before late execution. A valid empty output buffer reports
current state without consuming commands or advancing the cursor.

Unknown Stop is an applied no-op: it increments both the applied-command and
unknown-stop counters. Rejected commands increment their rejection counters
without counting as applied. Preroll counts as late even when upward rounding
places its target at frame zero.

### Known ceiling

A rate change for a live fractional sample head is rejected if its exact common
denominator exceeds signed 128-bit representation. The previous rate and every
voice head remain unchanged. Supporting wider exact phases would require a
different arithmetic representation and corresponding real-time verification.

## Queue, backpressure and real-time boundary

- One non-clonable producer and consumer share a fixed-capacity scalar SPSC
  queue. Producer Acquire-observes reusable slots, writes atomic payload fields,
  then Release-publishes. Consumer Acquire-observes publication, loads all
  fields, then Release-reclaims. Bounded modulo cursors never imply ownership
  from an overflowing sequence number. Core remains free of unsafe code.
- Full/disconnected producer admission returns the original command and an
  explicit error. No overwrite or hidden retry. Queue capacity and mixer
  pending capacity are separately configurable.
- Consume a bounded snapshot/budget per render. If pending storage is full,
  reject newly consumed commands and count them; preserve prior pending work.
  Preallocate voice and pending storage. Expose maximum frames, voices, samples,
  PCM bytes, queue/pending commands and drain work as checked setup limits.
- Queue admission does not guarantee mixer admission in that same callback.
  Commands beyond the per-render drain budget remain queued for a later render;
  they count as late if their target frame has passed when admitted. Changing
  callback partitions can therefore change queue admission timing when the
  budget is insufficient, while admitted sample phase and target-frame mapping
  remain partition invariant. For budget 1 and prequeued Play/Stop at frame 0,
  one four-frame render admits only Play; two two-frame renders admit Stop on
  the second callback and silence its last two frames. Size the drain budget
  for expected command bursts and observe lateness/capacity counters.
- The drain budget bounds new queue admissions, not execution of already due
  pending commands. Due execution is bounded by pending capacity. Sorted
  insertion and front removal can require quadratic work for dense batches;
  mixing scans configured voice slots per frame/channel. Finite capacity bounds
  do not guarantee that every maximum configuration meets a device deadline.
  Measure representative settings during integrated timing verification.
- Rendering performs no heap allocation, reallocation, deallocation, blocking
  lock, decode, disk/network I/O or ordinary logging/formatting. Disconnect is
  observable state, not permission to drop queue/sample ownership during render.
  Output returns a fixed report; readable telemetry is collected outside RT.
- Stop and join output workers before releasing queue backing storage or assets.
  Integer timestamps remain separate from PCM/interpolation floating values.

## Verification

Tests require literal WAV/sample conversion, hostile chunk validation, capacities
1/2 and repeated queue wrap, full recovery and both disconnect orders, concurrent
exact payload delivery, command reordering and pending-full rejection, fractional
frame boundaries, rate/seek transitions and invalid-render atomicity. A dedicated
allocator test counts allocation, reallocation and deallocation on silent, active,
completion, Stop, Seek, invalid, full-capacity and disconnected render paths.
Native output and physical latency require their own evidence; offline PCM
assertions prove neither device operation nor input-to-sound latency.

## Physical-frame startup foundation

A dedicated initially held command queue permits silent native calibration while
physical output frames advance and playback/commands remain frozen. The producer
can arm one immutable physical start frame; a straddling render emits the silent
prefix then playback frame0 exactly at that frame. Missed starts reject rather
than silently clamp. Applied first playback frame is independently observable.
Existing default queue and ordinary pause behavior remain intact. Pure checked
session/host bracketing and nominal ClockPair output-frame projection preserve
intervals, check domains/overflow and enforce future frame margins.
These prerequisites now support Linux solo network startup as specified below.
CoreAudio and WASAPI now use held-device calibration/frame arming below; ASIO
still requires its bounded-interval startup port. Authored fixtures compile only;
runtime/formal acceptance remains deferred.

## Linux calibrated initial frame startup

Linux solo network playback starts ALSA behind the initial silence gate, measures
actual advancing native pairs, obtains an early committed target and projects it
with observed slope to a future physical frame beyond the render/buffer margin.
Arm once, require matching applied-frame evidence and native presentation crossing,
then anchor transport/discipline to that physical point. Checked session-to-HOST
bracketing and retained uncertainty are independent of native physical accuracy.
Explicit startup geometry lets pause/end observers validate silent prefix and
finite suffix without weakening default guards. Preserve up to4096 actual evdev
events after arm until origin known, then use original timestamps in existing
runtime input filtering. Cancellation/loss/resync/overflow/native errors remain
explicit inside joined native cleanup. Offline/ghost/local cohorts retain previous
startup; CoreAudio/WASAPI ports are specified below. Actual physical accuracy,
drift/device/socket execution and formal acceptance remain unverified/deferred.

Calibration requires at least100ms of advancing native host/source observations;
observed slope must stay within the discipline default1000ppm. Initial calibration
is bounded by2s, then readiness/commit and actual crossing use the configured
multiplayer setup timeout. Session bracket freshness uses start-policy max age.
A zero-length finite section uses its exact physical end marker plus native
crossing, because it cannot publish a positive-playback start acknowledgement.

## CoreAudio and WASAPI calibrated initial frame startup

MacOS CoreAudio and Windows WASAPI shared/exclusive solo network paths use the
held-device frame startup contract, actual native clock observations, conservative
future projection, bounded calibration/commit/crossing and original post-arm input
retention. IOHID registry/health/loss and Raw Input foreground cleanup/removal/close
remain explicit. Windows physical stream-clock zero and playback-start coordinate
are separate: converting a WASAPI position never adds the selected playback frame.
PresentationDiscipline::new_with_playback_origin validates same output domain and
playback origin at/after stream origin before observations; default new uses the
same coordinate for both. Desired song phase subtracts playback origin, while
native conversion and source identity retain original physical stream origin.
Network keysound commands stay on logical playback grid even without manual pause.
Synthetic raw-snapshot regression fixtures must preserve physical positions and
zero phase error across a delayed start. ASIO still uses the existing software
start-call commitment; its bounded presentation intervals require separate startup
projection and SDK/device evidence. Offline/ghost/local groups and wireversion6
remain unchanged. Physical accuracy, drift, actual devices/sockets and formal
acceptance stay unverified; tests are authored/compiled without execution.

## Shared native runtime ownership

Native capabilities must be maximally abstracted while preserving their actual
semantics. Multiplayer/gameplay/lifecycle/scheduling policy is shared. A single
native_start::start_committed owns the held-device startup sequence for normalized
pair/counter backends; OS code supplies NativeStartDevice operations/evidence,
not separate calibration/commit/arming/crossing workflows. NativeStartAgreement
adapts shared LiveCompetition; fake native adapters with an actual gated mixer
exercise the same owner without sockets or native hardware. Original evidence
and clock domains remain intact. ASIO interval support stays explicit. Broader
shared gameplay-pump migration remains required; this startup migration does not
claim every native entry point is already thin. See ADR__native-runtime-boundaries.md.

Native presentation metadata may legitimately name a future host coordinate.
The shared owner retains the first qualifying crossing and its original evidence,
continues acquisition/native status/BGM service under the same deadline, and
publishes ready only once actual normalized host_now reaches the interpolated
playback origin. Reject host-domain changes/regression; cancellation preserves
cleanup ownership. Do not replace that crossing with later observations.

## Shared native solo gameplay owner

A single native_gameplay::run_gameplay owns solo native input admission, actual
pause/resume acknowledgements, transport/discipline correction, judge/capture/UI/
competition reports, bounded backlog and finite/full completion policy. Native
adapters supply observation/render/host/acquisition/end/reseed operations only.
Retain original input timestamps and native provenance; loss, removal, capacity,
clock/source changes and cleanup errors remain explicit. Pause resume reseeds
the same native observation source rather than mixing a supplied pair with raw
counters. Apply configured advance_lag 0..1s through one monotonic watermark on
all platforms; Windows must use its parsed lag too. Pending input capacity 65536,
bounded native batches 256, acquisition continues during acknowledgement waits,
judgment waits until native backlog is drained at pause/resume boundaries.
Finite completion requires native boundary plus logical prefix/input drain; full
completion requires actual judge/BGM/mixer/native drain. Caller owns native stop/
join and saving captured prefix after cleanup on all exits. Local cohorts use the shared owner below;
ASIO interval startup remains pending, with no runtime acceptance claimed.

## Shared native local-cohort gameplay

All native local cohorts use one common gameplay owner over RuntimeGroup and
InputMerger. Native adapters acquire bounded original events and observations,
while the owner handles clock validation, pause acknowledgement, ordering, lag,
judgment, each player’s captured reports/score/competition and shared completion.
Acquisition continues during pause/resume acknowledgement waits. A native backlog
prevents judgment/deadline release; actual pause/resume boundaries retain the
ordered input prefix and reconcile releases before post-resume input. Resume
reseeds the same native observation source. Finite completion requires every
member’s logical prefix and an actually committed input frontier crossing the
native endpoint. Full completion requires each member’s actual completion and
drained native/merged input; absent completion metadata is not completion.
Native setup/resource cleanup and independent create-new replay saves remain
with their owners. Linux uses bounded fair sweeps across sources; native Raw
Input/HID queues retain bounded collection and exact device-loss semantics.
Physical timing, ASIO startup parity and native runtime acceptance remain unproven.

## Shared native preparation policy

Live play and replay watching resolve omitted native defaults through one shared
preparation owner. Validate original options before querying native metadata.
Explicit settings retain their exact values and validation errors. Solo live
input may choose an available keyboard automatically; assigned local players
never trigger automatic keyboard selection, and replay watching never queries
keyboards. Native adapters expose default output identity, actual output format,
keyboard candidates and explicit ASIO replay format operations only.

Query only metadata required by omitted fields. Reject unavailable defaults,
bounded-catalog violations and invalid native formats without substituting
parser-only placeholder identities. ASIO has no implicit OS default driver; an
explicit driver remains required. Output routing, clock assessments and exact
buffer requests survive projection. Metadata work belongs to the game owner,
with no device discovery or waiting on UI/audio callbacks. Compilation does not
establish native device availability or physical timing.

## Shared local member preparation and finalization

Local 2..64-player sessions share member preparation, runtime activation and
recording finalization across operating systems. Preparation preserves admitted
native device IDs and stable player IDs, validates bindings/rosters before ghost
loading, and assigns disjoint keysound voices outside the shared BGM namespace.
The same prepared chart, seed, section and rules initialize judges, captures,
record opponents and completion metadata. Group and input merger use the same
actual calibrated transport origin; no fabricated timestamp is substituted.

Native owners stop/join output and close input before common finalization.
Finalization finishes every record opponent and attempts every independent
capture save, retaining session, cleanup and save failures. Destinations match
player identity rather than a truncating positional zip. Recording publication
remains create-new. Shared composition does not erase backend capabilities or
claim native timing, file, socket or desktop acceptance. Fixtures are authored
and compiled only while execution remains deferred.

## Shared native audio construction and BGM supply

Solo and local native sessions share queue, section-relative BGM admission and
mixer construction. The prepared sample bank supplies the exact format/rate;
platform owners supply their real render capacity and supported initial start
gate. Preserve the 1024-command live reserve, explicit output origin, preroll,
lookahead and immutable finite playback endpoint, including zero. Backend buffer
negotiation and clock/calibration evidence remain native capabilities.

Startup and gameplay use one checked BGM replenishment policy: only actual
completed logical playback frames advance its cursor, absent/paused reports do
not supply commands, and each replenishment retains the 256-command budget.
Physical initial silence and pause displacement never shift this logical grid.
Late/capacity/overflow errors and admitted prefixes remain explicit. Setup runs
on the game owner; the abstraction adds no native work inside audio callbacks.
Source fixtures are authored and compiled only; physical playback, files,
network sessions, graphics and formal acceptance remain deferred.

## Interval-preserving native startup

Committed ASIO network starts must use the common held-device startup owner with
actual output-grid observations and their complete host before/after bounds.
Validate calibration consistency against the explicitly assessed rate band, then
project the committed target uncertainty through that entire band. A past average
does not tighten future instantaneous-rate bounds. Retain the whole frame range
and select its latest ceiling frame beyond the rendered frontier plus actual
buffer. The assessed rate bound is an assumption consistent with observations,
not measured drift or an acoustic accuracy guarantee.

Preserve original native evidence, sample rate, frame origin and render identity.
At the first actual gate crossing, retain a bounded host window derived from both
bracketing observations and wait for native host time to reach its upper endpoint.
The transport's nominal anchor is distinct from this retained uncertainty. Coarse
host plateaus are permitted when bounds remain consistent. Point-based backends
keep their existing projection. Configure pause/end frame grids before arming;
seed ASIO end and discipline using the original interval observation. Initial
network gating does not enable manual ASIO pause. Committed ASIO gameplay uses
the common logical render-grid scheduler, so held startup silence never shifts
keysound commands by the physical start-frame offset. Ungated ASIO retains its
existing software-frontier scheduling.

Use actual applied ASIO buffer metadata from prepared stream evidence, never a
second requested-size estimate. Reject stale, incompatible, inconsistent or
unrepresentable evidence instead of inventing point samples. Author portable
interval and real gated-mixer startup fixtures for later execution. Ordinary
source/GNU checks do not cover the SDK/MSVC driver branch; SDK compilation, device,
network and physical timing acceptance remain deferred.
