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
