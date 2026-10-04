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
