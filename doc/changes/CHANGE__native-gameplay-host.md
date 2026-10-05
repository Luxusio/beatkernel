# Explicit native gameplay host

Shared solo/cohort pumps now receive player commands, publication and typed
diagnostics through `NativeGameplayHost` and explicit `run_*_with_ports` entry
points. The outer composition bridge selects player publication and system
control for the unchanged public native/control-only APIs. Pause state belongs
to the shared contract, with `player::PauseState` preserved as a re-export.
Publication refusal keeps the committed judge/gauge/capture prefix and actual
Stop evidence. Diagnostics borrow reports after Stop augmentation; the bridge
owns formatting and console output. Local whole-batch publication still precedes
fencing, and local diagnostic delivery now follows Stop augmentation.

Five independent fixture groups cover explicit host independence from ambient
cancel/pause state, explicit cancellation, actual pause/resume acknowledgements,
repeated PCM/capture/hash results, and publication refusal with successful or
partial Stop admission. They disable processing-clock profiling, inject virtual
waits and omit competition/network owners. Assertions have not been executed.
Platform presentation types and competition-owner network/time/UI effects remain
coupled. Actual tests, formal review/QA and performance acceptance are deferred;
the complete player and full layer-separation goal remain unfinished.

Scoped formatting and diff checks completed. All four compile-only checks
succeeded: workspace/all-targets with WebTransport, no-default WebTransport
all-targets, WASM browser and WASM browser-audio. Existing dead-code warnings
remain; compiled fixtures do not constitute executed assertion evidence.
