# Prepare pause and presentation clocks on the new stream anchor

Common timing preparation stages both pause and presentation owners at one
new output epoch using the recovered mixer's frame basis. It preserves the
original song mapping by translating its base from the original startup point
to the new presentation origin. Actual solo/cohort resume uses the same checked
translation, retaining existing behavior at the original origin.

## Evidence

Implementation and seven independent tests are authored: four actual-mixer/core
and native presentation-port staging cases and three genuine pause/origin cases.
Coverage includes epoch/config/refusal atomicity, old owners and queued PCM,
observed resume, gated startup, fractional rates, week/20-hour origins and final
wide-arithmetic cancellation. Old owners and mixer stay untouched on preparation failure. Candidates
retain actual discipline settings and require genuine new observations/warmup.
The desired pause request must remain active through native Ready priming.
Assertions/runtime/device/formal review and QA remain deferred.
Scoped Rustfmt and whitespace checks completed. Four sequential compile-only
checks exited zero: workspace/all-targets WebTransport, headless runtime
all-targets WebTransport, WASM browser lib and WASM browser-audio lib.
The seven new tests compiled in the host workspace check but did not run.
Existing unused-code warnings remain. These checks do not prove native device
opening, continuous pause holding, backend/UI handoff or physical acceptance.

## Known ceiling

Actual opening, pause pinning, epoch consumption and seeding remain caller responsibilities.
This prepares timing state; it does not open a device, publish a new runtime
owner, allocate stream attempt epochs or connect UI requests. Callers still
retire old callbacks, pin the producer pause request and seed genuine new
observations. Physical/acoustic acceptance and complete application handoff
remain pending. Full BMS player Goal stays active and incomplete.
