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
  and command behavior, including at 44.1 kHz and nonzero origins.

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
