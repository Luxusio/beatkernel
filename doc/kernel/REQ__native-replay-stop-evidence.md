# Native replay output Stop evidence

The native recorded player feeds planned Play/Stop commands through BgmFeeder
callbacks backed by the actual CommandProducer. Successful Stop callback counts
are valid evidence of this native queue's admission, including accepted prefixes
before a later refusal. Planning, voices, visual failure or unsuccessful callbacks
cannot earn Stop evidence. Browser/remote owners continue requiring their actual
ACK ledger; the native callback contract cannot substitute for remote acceptance.

Add an explicit public replay-audio cursor helper taking the actual feeder and a
raw RenderReport. Reuse existing strict cursor validation, permitting unknown_stops
only within the feeder's successfully admitted Stops and commands_applied. Check
usize-to-u64 conversion, preserve every other existing execution diagnostic and
frame overflow check, and retain the original report in rejection errors. The
ordinary public completed_render_cursor stays strict with zero allowed Stops.
Do not expose an arbitrary credit setter or relax a global Mixer validator.

Use this explicit helper in the native recorded player's per-poll playback cursor
and final-core diagnostics, with the same retained feeder used for actual initial
and rolling producer admission. Preserve physical/playback grid checks, pause
evidence, native presentation, full recorded visual/hash/capture semantics,
completion BGM/idle/presentation barriers, cancellation, and stop/final-check error
ordering. A Stop admission is neither successful execution nor physical silence.
No platform-specific replay policy, new crate or new replay format is needed.

Independent deferred library and binary fixtures cover real fatal replay sound
planning, actual feeder/producer/Mixer admission, inactive Stops, rolling and
partial/refused admission, exact equal-frame Play/Stop order, raw strict versus
owned rejection, excess/other bad diagnostics, physical/playback pause grids and
natural replay completion requiring later idle render plus actual presentation.
Keep existing fixture signatures and no-mine regressions.

Source/compile evidence is not device/browser/performance acceptance. Actual
execution and formal review/QA remain deferred. Final clear/fail classification,
profile identity, high-level mine file admission and full player acceptance remain
unfinished; the task and overall Goal remain active.
