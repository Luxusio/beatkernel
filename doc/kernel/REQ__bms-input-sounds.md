# BMS input-sound preparation

BMS invisible selections must use the portable core InputSoundTimeline policy,
including original song time, fresh presses and actual ordinary-hit precedence.
They remain unjudged and do not create chart objects or background Play commands.

The application module input_sounds exposes InputSoundPlan::prepare(source:
&BmsChart, sounds:&[SoundBinding], bgm:&[AudioCommand], max_markers:usize)
-> Result<InputSoundPlan,String>. It compiles actual invisible timing, applies
source.wav_gain(), and assigns one dedicated reusable voice per logical lane.
Successive presses on that lane replace its own prior fallback voice. These
voices start strictly after every supplied gameplay and BGM voice. Their order
is ascending logical control, independent of source row order. Never wrap u64
identities; failure does not return a partial plan. An empty invisible timeline
needs no additional voice, even if a supplied occupied identity is u64::MAX.
Invalid capacity, nonfinite occupied sound gains, non-Play BGM commands and
invalid invisible source/timing reject explicitly.

InputSoundPlan::markers() returns an immutable slice of core InputSoundMarker;
samples() returns an immutable sorted, unique slice of original SampleId values;
timeline() returns an owned clone of the validated core timeline. Preparation
does no asset IO, decoding, runtime input, output scheduling or device access.
Sample identities, original signed timestamps and gain remain unchanged.

VoiceAllocator::remap_input_sounds(&mut [InputSoundMarker]) uses its existing
monotonic namespace to remap distinct old voices, preserving intentional reuse.
It is atomic on exhaustion for both all markers and allocator state, including
the final usable u64::MAX voice. Empty input consumes no identities. Gameplay
and fallback remaps performed consecutively are disjoint; separate local
members receive separate ranges. Callers continue to reserve actual BGM voices.

Independent deferred fixtures must cover real parsed BPM/STOP/radix timelines,
WAV gain, lane reuse, deterministic/disjoint voices, original resource coverage,
capacity/invalid configuration/exhaustion and atomic allocator behavior. Include
actual core fresh-press command selection through the resulting timeline.

This prerequisite does not enable BMS playback yet. Actual preparation must
load these samples and install the plan in all solo/local runtimes; replay
identity and audio reconstruction must incorporate the same selections before
the existing unsupported-invisible admission guard can be removed. Browser,
device/audio execution, tests and formal QA remain deferred.

The remaining integration must treat invisible lanes as required input-binding
coverage alongside visible lanes for local members. Practice filtering removes
earlier judged objects but retains the original invisible timeline, including
the selection active before the start. Existing pristine-judge-only replay
identity does not include invisible selections; a canonical source-aware replay
identity must compare them before playback. Explicit contact-mode ownership must
also work on an invisible-only chart with no visible press evaluator. None of
these requirements can be satisfied by adding synthetic judged notes.

Explicit contact tracking is provided by the constructors documented in
REQ__judge.md and selected by stepped solo/local and replay construction for
invisible contact-mode sources. It tracks contact ownership independently of
remaining visible objects. This prerequisite alone does not install sounds or
make the existing replay identity source-aware.

Practice preparation accepts an original invisible selection even when the
selected section retains no judged object or BGM command. Empty ordinary
sections with no invisible selection keep their existing rejection. Reserve all
original invisible SampleId values when allocating BGM suffix sample identities
so that a keysound-only resource cannot be overwritten by a practice music tail.
This typed preparation prerequisite does not remove source asset admission's
invisible playback guard.

## Runtime installation and replay selection

InputSoundTimeline::markers() exposes a read-only slice of its validated sorted
markers for preparation-time voice checks. RuntimeGroup::configure_input_sounds
(Vec<(PlayerId,InputSoundTimeline)>)->Result<(),String> installs exactly one
timeline for every member, in source-plan order, at most once before processing
and while unpoisoned. Validate all rows and all voices before touching any member.
Fallback voices cannot overlap existing gameplay or reserved BGM voices or other
members' fallback voices; intentional reuse within one member is retained. A
failed validation leaves every member and the setup lock unchanged. Actual
private Runtime owners install through configure_input_sounds after complete
preflight. SoloRuntime::configure_input_sounds(InputSoundTimeline) delegates to
the same one-member operation. Existing constructor/callback behavior remains.

local_preparation::prepare_local_input_sounds(prepared:&PreparedBms,
members:&[MemberConfig],reserved:&[VoiceId])
->Result<Vec<(PlayerId,InputSoundTimeline)>,String> returns empty for no invisible
data. Otherwise compile the real InputSoundPlan once, require its samples in the
shared PCM bank, and allocate member fallback voices strictly after all actual
gameplay and reserved voices. Preserve lane replacement aliases and exact times,
controls, samples and gain. No PCM is copied. Local binding coverage includes
invisible lanes alongside visible lanes. No MemberConfig/PreparedLocalMembers
field or existing native helper interface is changed.

Actual stepped solo builds and validates its plan against PCM before moving
prepared data and installs it in its SoloRuntime. The local path builds member
plans against prepared configs before moving them and installs them atomically
in RuntimeGroup. Empty sources preserve the unconfigured legacy path. Both use
the original full invisible timeline even in practice; ordinary hits still take
priority and endpoint/advance behavior belongs to the existing core runtime.

Replay audio prepares the same timeline and verifies its samples. For nonempty
invisible data, reconstruct a pristine compatible actual judge and process each
original bound record/advance in order. Query real freshness before mutation and
pass its actual per-operation results to command_for_press. Map a selected
command's original unoffset song time to the caller's output origin/start/preroll
once, using existing checked wide arithmetic and exclusive rounded endpoint.
Duplicate/repeated/down-held inputs and advances cannot invent sounds. Existing
ordinary-hit/BGM planning, chronological ordering and full-log results/hash stay
exact; replay selection must not infer freshness from input labels alone.
Original physical output times/queue failures are not in logical recordings and
cannot be reconstructed by this audio plan. Sources without invisible data keep
their existing planning path.

This connects typed prepared data to actual stepped sound admission/replay plans.
Source asset preparation remains guarded until original sample loading, native
solo/cohort Runtime sound installation and all remaining callers are integrated.
Native recording identity follows the source-aware contract below. Tests and actual
audio/device verification remain deferred.

Unlimited song completion must not finish before the last invisible selection.
Use its checked original song timestamp plus one nanosecond as a lower bound
for the existing terminal-input frontier, without adding a judged object or
result. Calibration extent also includes referenced invisible PCM duration at
each selection. Actual output drain still decides audible completion after
accepted inputs; no automatic Play is emitted at an invisible marker. Finite
sections retain their explicit endpoint and rounded frame policy.

## Presentation lanes

PlayerChart::from_compiled includes the union of original visible-object lanes
and validated invisible selection lanes, in the existing left-to-right
scratch/key order. Invisible-only charts therefore retain actual playable lane
geometry and touch regions. Repeated selections and shared visible/invisible
lanes produce one lane. An invisible selection never creates a PlayerNote,
ObjectId, judged result or note render instance. Existing visible objects retain
their compiled identity/time and map through the resulting ordered lane list.
Presentation duration includes the last original invisible selection timestamp,
but does not imply PCM completion or replace SongCompletion/output drain.
Validate nonempty invisible timing through compile_invisible before using lane
metadata; malformed typed sources return PlayerChartError. Empty invisible
sources preserve the existing presentation data and query behavior. Practice
retains original lane availability without synthesizing section notes.

Prepare deferred fixtures against actual chart projection, touch-region routing
and playfield geometry, including invisible-only, mixed scratch/double-side,
practice, exact timing and malformed source cases. These fixtures do not prove
native/browser rendering or input-device execution. Source preparation remains
guarded until asset and native caller integration is complete.

## Native recording and competition identity

Native source-aware recording prepares InputSoundIdentity from the actual
selected BmsChart and supplies it to LiveReplayCapture::new_with_input_sounds
with the native ButtonOnly mode. Retain existing start, branch seed and capture
bounds/budgets. The old source-free prepare_capture helper remains compatible;
actual native solo and cohort callers use prepare_capture_for_source instead.
Disabled capture retains None without validating unused source metadata.
An enabled capture refuses invalid invisible metadata before creating a log.
No-invisible sources retain byte-identical legacy replay headers.

LiveCompetition prepares enabled ghost/network compatibility from the same
source-aware capture header before reading opponents or acquiring network
resources. NativeGroupCompetition computes the selected source identity once
and uses it for every member's actual header and competition identity, retaining
roster/member validation and mismatch refusal. Equivalent per-player voice,
binding and device assignments do not change chart compatibility. Changed
invisible sample, lane, original time or gain changes compatibility even when
the judged chart is unchanged. This fingerprint is neither authentication nor
an asset-content digest. No platform-specific competition protocol is added.

Prepare deferred fixtures against actual native capture, cohort preparation,
ghost loading and pure native-group canonical identity. Check legacy bytes,
changed-source refusal, disabled paths, setup limits and untouched judge state.
Source preparation remains guarded until asset loading and native Runtime sound
installation are integrated. Compilation does not prove recording/network or
device execution.
