# Integrated runtime contract (Phase 8)

The Windows composition example owns a Raw Input window and WASAPI stream with
an explicit endpoint and shared/exclusive mode. It uses one QPC host origin,
binds four canonical keys and publishes keysounds from judge hits. Scheduling
uses the reported submitted-frame cursor converted to the mixer's zero-origin
output timeline; callback races can make a command late and remain observable.
This is an explicit output scheduling choice, not a calibrated host/device
latency estimate. The finite pump cleans foreground Raw Input messages exactly
once, closes registration before destroying its window, and joins audio before
returning. Software processing percentiles and inferred native counters retain
their distinct labels. Native execution remains deferred.

Optional bounded WAV input and an explicit original-source cue use RestartPlan's
applied frame position as song origin. Each requested repetition starts a fresh
WASAPI client/Mixer/queue after the preceding stream stops and joins. Two accurate
increasing native position/QPC observations anchor output frame zero in the host
domain and preserve the observed inverse rate. Finite mapping validity is checked
for each input/advance; unavailable calibration fails rather than assuming an
origin. Closing the window cancels all remaining repetitions. This four-key
synthetic chart example is composition source, not proof of native timing or a
general file-format gameplay player. Details: ../platform/REQ__wasapi-presentation.md.

`SoundBinding::command_for` selects a Play only for a matching object/stage Hit,
using the caller's independent output timestamp. Live publication and recorded
audio planning share this selection without changing JudgeEngine results.

The core runtime has one owner and runs outside the audio callback. It composes
canonical physical events, explicit clock normalization, BindingMap, Transport,
JudgeEngine and the bounded scalar audio command producer. The final app owns
native acquisition and output; core has no platform dependency.

Input is mapped to the configured host domain only through a supplied ClockMapper
when domains differ. The earliest original_clock_point and native metadata are
preserved, along with device, physical identity and acquisition sequence. Input
host times and per-device sequences cannot regress; equality permits report
fanout. Transport supplies unoffset song time, and JudgeEngine applies its profile
offset exactly once. Missing mappings and arithmetic/transport errors are explicit.

Every operation supplies an independent audio scheduling ClockPoint. It is mapped
to the configured audio domain before judging; JudgeEvent.at is song time and is
never used as output time. SoundBinding explicitly maps object and result stage to
sample, voice and finite gain. Only hits produce Play. Queue-full/disconnection
returns the exact failed command, increments counters and does not undo judging.
A judge failure in binding fanout stops remaining destinations and is retained in
the report together with all preceding results: fanout is not transactional.

`Runtime::enqueue_audio` lets the owning control thread submit an explicit
AudioCommand to that same bounded producer between gameplay operations. This
supports application background audio and scalar rate/stop/seek controls without
creating another producer or introducing game-specific logic. Command timestamps
must already be in the configured audio output domain; this method performs no
host/song mapping, judge advance, transport edit or acquisition-sequence change.
It returns the exact queue failure without retry and shares the hit-publication
admission/full/disconnection counters. Admission does not prove mixer execution
or native output; scalar Seek does not reconstruct gameplay state.

The Phase 8 forward loop does not restore judge state for seek/reverse. Negative
transport segments and song-time regression are rejected. Mutable engine and
transport access are intended for a caller coordinating Phase 10 restoration;
editing the transport alone is not a seek implementation. A forward seek can skip
objects and cause deadline misses; it does not reconstruct historical gameplay.

Timing telemetry retains a bounded ring of processing durations, with nearest-rank
p50/p95/p99/max calculated on demand outside callbacks. These durations measure
software processing only, never physical input-to-audio latency. Native drop and
underrun counts are supplied explicitly by the host. Counters saturate rather than
wrap; unavailable measurements remain absent. Reporting may allocate, and there
is no allocation-free judge/runtime claim. Hardware benchmarks and formal
verification are deferred by the user's instruction on 2026-09-30.

## Caller-selected processing clock

`RuntimeProcessingClock` selects only software operation profiling: `Native`
uses the existing `std::time::Instant`, `External(fn() -> Option<u64>)` reads
caller-supplied monotonic nanoseconds, and `Disabled` acquires no processing
clock. Native remains the default. Environments without a usable standard
monotonic clock, including browser WASM, must explicitly select an external
clock or disable duration measurement before input or deadline operations.
The core remains dependency-free and contains no browser or OS clock adapter.

An external start or end reading of `None`, or an end before its start, leaves
that duration unrecorded. Unknown measurements are not recorded as zero. A valid
zero duration is retained. Existing rejected-operation counters still update
when duration measurement is unavailable or disabled. Changing the clock affects
future observations and retains prior telemetry. RuntimeGroup and SoloRuntime
forward the same selection to their actual member Runtime owners.

All clock selections invoke identical binding, chronology, Transport, Judge,
audio queue and replay behavior. The processing clock is never an input, song,
presentation or audio-scheduling clock; its precision proves no acoustic timing.
Caller clock functions are trusted synchronous callbacks, outside the audio
callback. Regression and unavailable-duration fixtures are authored for later
execution under the standing verification deferral.

The portable runtime example composes an actual canonical keyboard fixture,
binding, chart judge, scalar queue and Mixer and renders literal PCM. This is
software composition evidence, not native Windows playback evidence.

`Runtime::replace_state` resets chronology only while replacing both JudgeEngine
and Transport by ownership transfer after replay reconstruction. It returns the
previous owners, clears source sequences, and preserves bindings and telemetry.
It does not flush queued audio: the final app must separately synchronize or
replace its audio queue/output when restoring a timeline.

## Explicit control-owner exchange

Runtime exposes explicit exchange of Transport and CommandProducer for a
single-owner composition using shared song/output owners. Exchanges preserve
judge/sequence/chronology state and do not reset the session or remap timestamps.
The caller must install the correct mapping/output domain and restore owners
before another operation, including on failure/unwind. CommandProducer remains
a unique endpoint; exchanging it does not clone it or create another publisher.
This is control-thread setup/composition, never an audio callback operation.
The BMS group provides the RAII transaction and failure fence.

## Immutable original-song end boundary

Runtime optionally configures a nonnegative song end before its first committed
operation. Default sessions remain unlimited. Invalid negative ends, a second
configuration or configuration after committed chronology fail without changing
the limit, judge, transport, sequences or audio queue. Replacing/restoring the
session clears the end for explicit fresh configuration; ownership exchanges
preserve it. The boundary is exclusive for physical-input admission.

Clock normalization, host/song monotonicity, reverse-segment rejection and source
sequence checks still apply before fencing. Strictly earlier input follows the
ordinary binding/judge/keysound path with original native provenance. An input
whose mapped original-song position is at or after the end cannot bind, hit or
generate a new input keysound. It validates acquired chronology and advances the
same JudgeEngine to the capped end, with report.input absent so actual capture
records an Advance rather than a fabricated input. Timers use the same cap.
Validated acquisition is counted, but deliberate end fencing is not unbound input.
Repeated late operations remain at the end and produce no duplicate judge results.

RuntimeReport.song_end_reached labels a capped logical boundary; judge_error and
partial-result/audio-failure evidence still need inspection. It never proves
native output completion. Capture/reconstruction uses original judging semantics
and existing wire records; hold state is not fabricated and unfinished notes are
not forced into completed scores. Transport history, pause and clock discipline
remain authoritative and are not rewritten by this limit.

Known ceiling: callers still own native audio end-frame mapping, presentation
crossing, input backlog/frontier draining and cleanup. This logical component
alone does not implement complete native/gapless looping. Pure fixture source
and compilation are not execution proof; runtime/native acceptance remains deferred.

## Optional spatial touch routing

Runtime may install an immutable bounded touch router before processing.
Physical acquisition timing and finite-end validation precede routing mutation.
A configured touch produces one logical destination without altering its variant,
coordinates or provenance. Unconfigured input keeps existing binding fanout.
Projected hit coordinates are separate from original acquisition coordinates.

Fresh state/session replacement clears contact ownership; explicit paired
replacement accepts a routing checkpoint alongside the restored judge and
transport. The caller synchronizes output and supplies a coherent checkpoint.
Partial judging/audio failures keep actual admitted input/report evidence.

## Optional shared input-sound timeline

Add portable runtime::input_sound::InputSoundMarker with fields control
(GameControlId), at(Timestamp), sample(SampleId), voice(VoiceId), gain(f32).
InputSoundTimeline::new(markers:Vec<InputSoundMarker>, max_markers:usize) validates
positive caller capacity at most100000, totalcount, finite signed gains and
unique control/time positions before constructing an owned immutable sorted
index. Signed song timestamps remain exact. The same voice may be deliberately
reused; collision policy belongs to the app. Empty timelines are valid.

InputSoundTimeline::command_for(control,song_at,audio_at) selects the most recent
marker at or before original unoffset song_at and returns its Play at independent
audio_at. No prior marker means no sound. Control lookup and per-control binary
search must not scan all markers on each input. No state cursor, polling, file
IO or audio callback work. command_for_press(input:&GameInputEvent,
fresh:bool,song_at,audio_at,results:&[JudgeEvent]) is the common pure policy for
live/replay composition: require actual Button Down or Touch Down and fresh;
suppress fallback when this accepted bound operation emits an Instant/HoldHead
Hit. Up/Repeat/Move/Cancel/axis/pointer/custom input never trigger fallback.

Expose JudgeEngine::is_fresh_press(&GameInputEvent) using the engine's actual
button/contact ownership and enabled contact mode. push_input must use the same
freshness calculation; the query is read-only and cannot commit held state.
Duplicate Down is not fresh; distinct source/physical/control/contact ownership
remains independent. Reconstructed judge snapshots retain this same truth.

Runtime::configure_input_sounds(InputSoundTimeline) installs at most once before
committed operations, returning RuntimeError::InputSoundConfigurationLocked on
late/repeated configuration. The first committed-operation configuration lock
survives state restoration even when no timeline was installed. Default remains
unconfigured. At each successful
bound push, use freshness acquired before actual judge mutation and its actual
returned results to select fallback. Failed fanout does not erase already
accepted bound-operation sound attempts. Emit existing judged sounds first,
then selected fallback sounds in accepted binding order through the same
CommandProducer and telemetry. Preserve exact audio_commands/audio_failures
admission evidence, no retries or invented results. At/after configured song
end, bindings and fallback both remain suppressed; advances never synthesize
press sounds. Queue failure cannot undo accepted judging or held ownership.

Timeline lookup is stateless and retains its immutable mapping during existing
same-chart replace_state/session or paired contact checkpoint restoration.
Freshness derives from the installed reconstructed judge. This does not reset
or bypass song-time/output chronology. Callers changing charts still construct
fresh runtime owners and explicit voice plans.

Independent deferred fixtures cover validation/cap/identity/index boundaries,
original song versus output time, fresh presses, ordinary hit precedence,
button/contact ownership, duplicates/repeats/releases, unbound inputs, endpoint,
partial queue admission and restoration. Compare actual Runtime output and
actual ReplaySession bound-operation reconstruction using the same timeline
selection policy. No platform-specific or separate replay sound rule is added.

This establishes the real optional core Runtime path. BMS invisible preparation
continues to refuse playback until app voice allocation, sample loading, replay
identity and live/local/replay wiring all consume the same timeline. Core source
and compile-only checks do not prove browser/device/audio execution or complete
invisible-note playback. Runtime test execution and formal QA remain deferred.

## Hazard outcome delivery

RuntimeReport exposes a separate `hazard_events: Vec<HazardEvent>` from the
actual configured JudgeEngine. Configure hazards on the pristine engine before
handing it to Runtime. Every successful judge call copies its current operation
report immediately: mapped input fanout appends each successful call's hazards
in binding order; explicit advances and finite-song endpoint advances append
their outcomes as well. Copy only after success, so a rejected judge call cannot
re-publish the engine's retained previous report. Preserve earlier committed
fanout results when a later destination fails. Unbound/ignored input makes no
judge call and reports no hazards; it must not invent advancement or publish a
stale report. A later genuine advance remains responsible for pending markers.

Preserve each marker's full identity, original time/value/outcome and original
input metadata. Mapping normalization retains original clock provenance through
the existing input path. Hazards neither become JudgeEvent scores nor trigger
ordinary note/input-sound commands automatically. Queue rejection does not erase
hazard results. Existing judge-result telemetry counts ordinary JudgeEvents only.
RuntimeGroup/SoloRuntime wrappers carry these actual reports unchanged. Default
unconfigured engines report an empty hazard vector. Synthetic fixture reports
must explicitly initialize it. This new public struct field requires external
RuntimeReport literal constructors to add an empty vector where appropriate.

Author independent deferred fixtures over actual Runtime fanout, prefix failure,
advancement/end cap, mapping/touch ownership, unbound input, audio failure and
snapshot/replay consistency. Compile-only checks are not execution evidence.

Known ceiling: This delivery layer does not install BMS hazard plans, apply
gauge/death, play WAV00, render mines or enable source preparation. Keep mine
admission refused until actual live/local/replay/practice/offline integration.
