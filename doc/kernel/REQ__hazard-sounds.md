# Optional committed hazard sounds

The portable runtime supports an opt-in immutable HazardSoundTimeline whose
bindings map a full-width HazardId to caller-supplied SampleId, VoiceId and finite
signed gain. Validate caller capacity, duplicate IDs and gain before installation.
Sort bindings by identity and use binary lookup without per-event allocation.
The core does not interpret opaque hazard damage values, load assets, invent
voices, select BMS fatal-sound behavior or apply gauge/death policy.

Runtime installs a timeline at most once before committed input or advancement.
Rejected clock/sequence acquisition does not lock otherwise pristine setup;
committed unbound input does. Failed or repeated configuration preserves the
previous owner. Unconfigured runtimes keep their existing sound behavior.

After ordinary judged sounds and accepted invisible press sounds, publish
optional sound commands in actual RuntimeReport hazard-event order. A binding
matches only its exact identity and Triggered outcome; avoided and unbound
hazards remain silent. Preserve original marker time and input provenance in
the report while scheduling at its independently normalized output timestamp.
Successful input, explicit advance and finite-end advance all use the same
publication path. A later judge fanout failure retains the already committed
hazard prefix and its sound attempts. Rejected operations must not replay a
retained old hazard report; subsequent successful empty reports add no sound.

Reuse existing queue admission, command report and telemetry accounting. Queue
failure preserves the successful sound prefix and every actual failure without
retry, rolling back the judge or fabricating playback success. Shared voice
replacement is caller policy, consistent with normal SoundBinding. Canonical
judge bytes and normal score/judge-result accounting are unaffected.

Author independent deferred configuration, exact identity/provenance/output
mapping, normal/invisible/hazard ordering, held/contact/finite-end behavior and
queue/fanout-failure prefix fixtures using actual Runtime/JudgeEngine queues.
This is the common sound primitive; BMS WAV00 selection/loading, app/live/local/
replay/offline installation, voice separation and completion integration remain
required before mine file admission. Device/browser/audio/performance execution,
formal review and QA remain deferred.
