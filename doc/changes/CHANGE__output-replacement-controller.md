# Coordinate output retirement, reopening and first observation

A common static output replacement controller owns native/recovered/pending
states, consumes distinct stream attempt epochs and keeps candidate clocks
unpublished until genuine paused output evidence arrives. An exclusive core
PauseHold prevents resume during native priming and first-observation waiting;
releasing it still leaves pause requested. A Linux ALSA bridge connects the
policy to real stream opening, start, stop, timing and recovery APIs.

## Evidence

Implementation and thirteen independent tests are authored: three core hold
cases, two actual core/solo/cohort delegation cases and eight full controller
memory-backend cases. These cover held priming, first real evidence, failed
attempt epochs, retained/unavailable ownership, cleanup/recovery diagnostics,
wrong epoch/basis, exact deadlines, cancellation and atomic preflight refusal.
Tests use actual Mixer/queues/PCM and real presentation algorithms with explicit
memory IO effects. Native device opening is not executed. Assertions/runtime,
formal review and QA remain deferred. The policy performs no native clock reads,
uses static ports and adds no callback lock or allocation.
Scoped Rustfmt and whitespace checks completed. Four sequential compile-only
checks exited zero: workspace/all-targets WebTransport, headless runtime
all-targets WebTransport, WASM browser lib and WASM browser-audio lib.
The thirteen tests and concrete Linux ALSA adapter compiled in the host check;
no assertions or native device calls ran. A newly authored unused test import
was removed before the successful workspace retry. Existing unused-code
warnings remain. These checks do not establish runtime/device acceptance.

## Known ceiling

Observation wait timeout cannot interrupt a blocked foreign open/start call.
Native/UI pump wiring, Windows/macOS replacement adapters, blocking-call
isolation, fallback policy and device/acoustic acceptance remain pending.
Caller commits returned clock/output ownership before explicit resume.
Full BMS player Goal stays active and incomplete.
