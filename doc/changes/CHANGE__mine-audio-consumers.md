# Replay and offline mine audio

Replay audio uses the common WAV00 plan and actual per-operation
hazard reports, preserving recorded logical time and normal/press/hazard order.
Offline preparation installs that plan and schedules real judge advances at mine
boundaries before output rendering, including during a normal held note. Mine
markers do not generate synthetic presses. Silent/fatal source policy and finite
output mapping remain common.

Practice section preparation already retains the original PCM bank and never
allocates a zero suffix ID, so WAV00 preservation needs coverage rather than a
separate source rewrite. Five independently authored deferred fixture groups
cover actual capture/replay command order, PCM and final judge hash, recorded
prefixes and silent variants, section/offset/preroll/rounded endpoint mapping,
offline held notes and block partitions without synthetic mine presses, and
missing PCM/zero extent/queue failures with the actually written prefix.

Both writers delivered terminal Writes STOPPED before scoped rustfmt and the
four authorized compile-only checks. Workspace all targets with WebTransport,
headless all targets with WebTransport, WASM browser and WASM browser-audio each
completed with exit 0; git diff --check completed successfully. Existing unused
playfield-wrapper and WASM cadence warnings remain. Assertions were not run.

Guarded file mine admission, WAV00 file loading and complete
gauge/fatal-stop policy remain unfinished. Assertions, applications, devices,
browsers, performance acceptance and formal review/QA remain deferred. This
slice does not establish overall Goal completion.
