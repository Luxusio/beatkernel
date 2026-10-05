# Gameplay sound stop after a committed fence

Keep sound stopping separate from the portable judge/input fence. Prepare one
sorted unique list of this Runtime's normal, press and hazard VoiceIds before
gameplay starts; new timelines update the list only after setup validation.
Retain that bounded list and no per-note lookup/sort on failure. Shared BGM voices
and other members' voices are outside this list; existing voice ownership
validation remains required. No mixer, queue codec or replay wire changes.

Expose Runtime::fence_gameplay_sounds(requested_at) as an explicit control-thread
operation returning Option<RuntimeSoundStopReport>. Require an actual committed
gameplay fence: before a fence return None without changing configuration or stop
state. Attempt at most once per gameplay state, even after partial queue failure;
repeated calls return None and never retry or duplicate accepted commands.
The report retains the effective output timestamp, every accepted Stop command
and every original admission error. Admit all configured unique voices in order
using the same producer and telemetry as normal gameplay. Do not stop at the
first rejected command or discard accepted prefixes. An empty voice list still
records the one attempt, with empty evidence.

Track the maximum output timestamp of successfully admitted normal, press and
hazard gameplay commands. The effective stop timestamp is max(requested_at,
that watermark); rejected commands do not raise it. At equal target frames,
the existing queue/mixer order places these Stop commands after previously
admitted gameplay Play commands. This is a scheduled stop, not a promise of
immediate physical silence: future admitted sound times can postpone it.
Do not replace the caller's output-domain timestamp with song, input or arrival
time. Explicit externally enqueued commands remain caller-owned and outside this
guarantee. No queue flush, fake pause, synthetic input or drain completion.

RuntimeGroup exposes fence_player_sounds(player, requested_at), rejecting unknown
members without mutation and borrowing the real shared producer through the
existing owner guard. It remains available after poison without clearing poison.
SoloRuntime delegates the exact same path. Restoration retains prepared voices
but clears the one-attempt latch and scheduling watermark; callers must stop/reset
the old output before changing timeline, as already required by restoration.

StepGameplay and StepLocalGameplay request the stop immediately after consuming
the actual gauge-failing report and fencing its committed frontier. Preserve
normal/hazard/score/capture observations and append stop admission evidence to
the original actual report. Use report.audio_at.timestamp. Reset drain evidence
for accepted stops. Queue failure is a technical error with the numeric fence
and committed prefix still readable; numeric failure with successful stops does
not abort healthy local members. Consume and stop all newly failed members even
when another report fails; keep BGM and surviving voices untouched.

Independent deferred fixtures cover real queue/Mixer output, future scheduling,
unique overlapping sound configurations, queue admission failures, no-fence and
repeat behavior, restoration, exact local voice ownership and actual stepped
gauge failure/error prefixes. Authoring or compile-only checks do not establish
assertion, hardware, Worklet, native driver or performance acceptance.

## Native owner connection

The shared native solo and cohort pumps use the same scheduled voice-stop API.
After independently consuming gauge, capture, score/competition and presentation
observations, fence the failed member and attempt its prepared voice stops using
the actual report.audio_at.timestamp. Preserve the original input, judge/hazard,
Play prefix and all independent errors; append accepted Stop commands and exact
admission errors to that same actual report. Local observation consumes the whole
committed prefix before stopping newly failed members, even after group poison.
The normal input and deadline paths and original GroupError completed-prefix
paths all use this connection. Do not reset poison or abort survivors solely for
a numeric gauge failure with successfully admitted stops.

Native typed observation/processing errors retain the augmented committed
reports alongside the original core failure identity and independent observer
errors. Any stop admission refusal is a technical owner error with readable
numeric failure/fence and accepted audio prefix. No automatic retries, global
Stop/Seek/pause, BGM interruption or OS-specific gameplay policy. UI attachment
must not determine whether stop commands are admitted. Do not invent physical
silence, drain/presentation evidence or successful terminal outcomes.

Independent native deferred fixtures cover actual common report/pump paths,
headless/publisher cases, real queue/Mixer future scheduling and survivor/BGM
preservation, simultaneous capture/presentation/audio errors, group poison and
prefix retention, and no duplicate post-fence stops.

Replay/offline audio policy connections, result clear/fail and output cleanup
remain unfinished after the common native owner connection.
Stopping a configured inactive or never-played voice preserves the Mixer's
existing unknown_stops diagnostic. The current strict normal-completion evidence
validator rejects that counter; failed-session output completion needs an
explicit evidence policy in its own integration. Do not weaken the generic
validator or hide real counters to claim completion after scheduled stops.
Keep the high-level mine file admission guard until all actual owners are wired.
