# Retained native playback-end evidence

The mixer now reports the first physical frame where its immutable playback
endpoint was reached. That optional scalar persists across silent callbacks,
manual resume requests and empty reports; empty/invalid renders never create it.
This preserves the actual boundary when native telemetry coalesces a short
resume and endpoint crossing into a later silent report. Fixed native telemetry
encodes presence separately from its full u64 value, including zero and maximum.

NativeEnd validates configured playback end, render grids/counters, persistent
marker identity and actual associated output/host clock pairs. It retains proof
across unavailable reports and emits one boundary only after real presentation
crosses the physical endpoint. A prior actual lower clock bracket is required;
once the marker is known, the closest observed lower pair is retained. Malformed,
regressing, disappearing or overflowing evidence commits no state. Checked host
interpolation has Unknown physical accuracy and never claims acoustic precision.
The observer performs no native IO, wall-clock reads, Transport mutation or UI
publication. Core PCM/RT fixtures, scalar publication and actual Mixer/native
frontier composition fixtures are authored and compiled only.

Known ceiling: native BMS owners still need to opt into endpoint intent, combine
manual pause/resume and terminal end, drain the correct input frontier, cap
judging/capture and stop/join before restart. Current UI loops remain observed
position with possible overshoot and reopening gaps. Gapless looping, ASIO
presentation, network pause policy, browser/full controls and native/GUI/replay
acceptance remain required. Execution and formal close gates remain user-deferred.
