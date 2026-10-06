# Rebind paused playback evidence after output replacement

NativePause retains its acknowledged frozen playback frame and cumulative gap
while rebinding to a strictly newer output epoch on the same recovered mixer
grid. It clears old interpolation/source-kind state, permits a new point or
interval source, and keeps lower physical/actual-host frontiers. Tagged requests
and observations reject old stream epochs before interpreting their metadata.

## Evidence

Implementation and seven independent real-mixer replacement/resume tests are
authored. Cases cover queued PCM, three fractional gaps combined on a 3Hz grid,
point/interval transitions with honest arrival floors, nonzero/max/stale epochs,
physical/host floors, actual startup gates, finite endpoints and atomic refusal.
Read-only Mixer startup getters share existing applied-start publication.
The fixed owner gains scalar stream guards, without native imports,
heap allocation or per-note work. Existing untagged APIs remain compatible and
assume the current owner. Assertions/runtime/formal review/QA remain deferred.
Scoped Rustfmt and whitespace checks completed. Four sequential compile-only
checks exited zero: workspace/all-targets WebTransport, headless runtime
all-targets WebTransport, WASM browser lib and WASM browser-audio lib.
The seven new tests compiled in the host workspace check but did not run.
Existing unused-code warnings remain; native/backend handoff and acoustic timing
are unverified. No task or full Goal completion is claimed.

## Known ceiling

Native retirement and runtime handoff are still caller responsibilities.
Callers still must retire old callbacks and assign epoch at stream creation;
legacy untagged APIs cannot identify stale observations themselves. This does
not execute backend reopening, adapt a different sample grid, bind runtime/UI
switch requests or prove native/acoustic timing. Full BMS player Goal remains
active and incomplete.
