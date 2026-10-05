# Offline gauge failure and owned stop evidence

The bounded synthetic offline renderer observes the default BmsGauge from every
actual Runtime normal/hazard report, once per operation. Keep its chronological
BGM/input/mine-deadline schedule and actual overlapping-hold judgments. A mine
alone does not imply failure; avoided mines and recoverable empty gauge continue.
After consuming the failure operation's committed results and Play prefix, fence
the Runtime and call its shared fence_gameplay_sounds with report.audio_at.
Attempt every prepared voice once, retain original queue errors and suppress
later grading/gameplay sounds through the Runtime fence. BGM remains independent.
Reject an actual failure whose gameplay voice namespace collides with BGM; prepare
that ownership check before the loop. No global pause, queue flush or retry.

Continue rendering the caller's exact requested frame extent and background
audio after numeric failure; frames written mean offline output extent only,
never song clearance, native playback, silence or device drain. Preserve existing
public OfflineReport/OfflineError fields. Zero-frame rendering performs no gauge
observation or synthetic failure and retains current setup validation.

This renderer owns its fresh queue and admits BGM Play commands only. Its only
Stop source is the successful commands in actual RuntimeSoundStopReport. Keep a
private cumulative count of those actual accepted Stops. Only this closed owner
may allow cumulative unknown_stops up to that count, because inactive or expired
owned voices are legitimate idempotent Stop targets. Planned/requested/rejected
Stops provide no allowance. Preserve the raw RenderReport/counters without
normalization; all other render diagnostics remain strict errors. The generic
render_block retains zero unknown-stop allowance. The separate
[replay PCM owner](REQ__replay-render-owned-stops.md) reuses the same internal
evidence component, recording its own actual queue admissions.
This owner count is admission evidence, not per-command acoustic execution proof.

Judge/gauge errors and original Play admission failures remain technical errors;
consume any actual failure prefix and attempt its stops before returning them.
Retain all original Play and Stop admission errors together, latest render evidence
and the already written PCM prefix. An accepted numeric failure by itself does
not stop output writing. No successful output after technical rejection is claimed.

Independent deferred fixtures cover fatal held/equal-time normal prefixes, BGM
preservation and later gameplay suppression across block sizes; missing WAV00,
fatal-only and silent/avoided/recoverable mines; partial Stop admission with
original queue refusals and written-prefix evidence; and strict generic render
diagnostics against actual queues/Mixer. Assertions, device/browser output,
performance and formal review/QA remain deferred. Worklet acknowledgement,
native/replay completion, clear/fail policy and high-level mine admission remain
unfinished; do not remove the mine preparation guard.
