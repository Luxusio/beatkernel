# Pure presentation time estimator

This is a reusable legacy HOST/transport-correction capability. The current BMS
player selects [audio authority](REQ__audio-authority.md) instead: raw output
supplies scheduling, logical output owns normal-rate Transport and Runtime,
and original HOST input uses bounded unknown-quality correlation. The correction
requirements below apply to explicitly selected legacy consumers, not current
native/browser play-time authority. Preserve their APIs and regression behavior.

Move backend-independent presentation observation retention, freshness, drift
and phase calculation and continuous transport correction into the BeatKernel
core time layer. Its public estimator, config, admission/update values and error
types cannot reference native snapshots, WASAPI/ASIO types, UI, clocks or IO.
Caller-supplied clock pairs and host query values are the only timing inputs.
Quality remains Unknown: neither midpoint relations nor integer arithmetic prove
acoustic accuracy. Core imports no platform crate or additional dependency.
Explicit output rebinding now follows
[output clock epochs](REQ__output-clock-epochs.md). It drops the old fit and
rejects mismatched tagged observations while retaining reserved storage; it does
not perform device handoff or alter live transport/judgment history.

The platform PresentationDiscipline remains a compatibility adapter. Preserve
its public constructors, getters, observe/observe_clock_pair/observe_asio,
validate_host/update and legacy error variants. Re-export shared config/update/
admission values while translating pure errors to the original platform errors.
Validate native source identity, unchanged/regressing counters, frequency changes,
ASIO frame/rate/midpoint evidence and malformed snapshots in the adapter before
forwarding actual original pairs. Do not lose native metadata or relax source
mixing rules. Commit adapter metadata only after successful pure admission.

The pure estimator owns one preallocated bounded ring of ClockPair values and
transport update chronology. The adapter retains only latest source metadata;
do not duplicate the ring or add observation/update allocations, thread effects
or per-note dynamic dispatch. Preserve retention decimation, ring replacement,
freshness and historical transport mapping exactly. All rejected admissions or
updates preserve prior observer/transport state.

Separate supplied-pair duplicate semantics from validated native progress.
Supplied unchanged output cannot refresh freshness; advancing supplied output
requires advancing host time. WASAPI counter progress can map to the same integer
output nanosecond when the native frequency exceeds the nanosecond grid. Provide
a checked progress admission that permits unchanged quantized output with strictly
advancing host, so moving the calculation does not reject previously valid native
progress. ASIO equal coarse host midpoints still await host progress without
refreshing state. Core progress admission validates domains/chronology; the
adapter remains responsible for original native counter evidence.

Retain checked signed arithmetic, drift/phase limits, rate clamping, warmup/update
interval behavior and transport errors. Playback origin is distinct from stream
origin, including negative logical song origins. No seek or fixed-rate reset may
replace continuous correction; historical mappings and original evidence survive.

Independent deferred core fixtures use known rational traces and boundary/fault
cases without a platform dependency. Adapter fixtures use memory-only native
snapshots and ASIO evidence, comparing actual forwarded pairs and update results,
source rejection, duplicate/stale behavior and failed-operation preservation.
Existing fixtures/assertions stay unchanged. This increment extracts the actual
algorithm. The subsequent [gameplay presentation port](REQ__gameplay-presentation-port.md)
migration injects either this estimator or the native wrapper into common
sessions without a concrete platform dependency in production gameplay policy. Actual assertions,
hardware timing, formal review/QA and performance measurements remain deferred.
