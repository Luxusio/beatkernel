# Scheduled gameplay voice stop

Runtime prepares a sorted unique normal/press/hazard voice union during setup,
tracks only successfully admitted gameplay audio timestamps and exposes
fence_gameplay_sounds returning RuntimeSoundStopReport {at, commands, failures}.
It requires an actual fence and attempts every prepared voice once, including
after individual queue refusal. The output time is no earlier than the last
accepted gameplay command; repeated calls do not duplicate or retry. Paired
restoration clears the latch/watermark while retaining prepared configuration.

RuntimeGroup::fence_player_sounds and SoloRuntime use the real shared producer
through the existing owner guard, including after poison without clearing it.
StepGameplay and StepLocalGameplay consume gauge/score/mine/capture observations
first, fence newly failed members, append accepted Stop commands and original
admission errors to the actual failure reports and reset drain evidence on
accepted stops. Numeric failure alone does not stop healthy cohort play. BGM
and unrelated member voices are outside the failed member's stop list.

Six independently authored groups cover actual core queue/Mixer scheduling,
deduplicated normal/press/hazard voices, rejected future command watermark,
one-attempt/full-partial rejection, pristine/empty/restored state, shared local
ownership and poisoned control, and actual stepped solo/three-member gauge
failure/capture prefixes. Two existing gauge fixture files are aligned with
appended Stop evidence; stale fatal-replay pressed masks are aligned with the
already implemented display cleanup while preserving judge/capture hash checks.

Both writers delivered terminal Writes STOPPED before scoped formatting and the
four authorized compile-only checks. Assertions and real browser/device execution
remain deferred. Native pump/replay audio wiring, physical output cleanup and
clear/fail results remain unfinished; mine admission stays guarded.
Scoped formatting and whitespace checks succeeded. Workspace/all-targets
WebTransport, headless/all-targets WebTransport, WASM browser and WASM browser-audio
compile-only checks all exited zero. Existing unused-code warnings in cadence and
the playfield progress helper remain. Test source compilation is not assertion
execution or native/Worklet hardware acceptance.
Configured inactive-voice Stop attempts retain unknown_stops diagnostics; the
existing strict normal-completion validator still rejects them. Failed-session
completion needs explicit diagnostic/evidence handling, rather than resetting
or suppressing the actual mixer counters.
