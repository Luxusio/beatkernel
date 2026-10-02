# Browser shared-runtime gameplay source integration

The browser host now has a Play/Stop source path in the existing BMS application
crate. A platform-independent StepGameplay owner runs the actual SoloRuntime,
ScoreSummary and BgmFeeder without a native polling loop. Runtime input and
advancement continue while one bounded outgoing audio batch awaits admission.
Exact command-prefix failures retain committed evidence and fence the session.

BrowserGame consumes the same preparation result, moves original PCM ownership
out of its bank one asset at a time and exposes original sample IDs, rates and
channels to the separate AudioWorklet memory. The core PCM move accessor adds
no dependencies or operating-system APIs. Live drawing uses actual judgments,
score and note progress through the existing retained renderer.

The Window owns original keyboard timestamps, bounded input batching and the
AudioHost. The graphics Worker owns chart/game state. The Worklet owns Mixer
and audio resources. Setup transfers assets and admits initial BGM before
choosing a future start frame and reanchoring a pristine runtime. This nominal
software clock projection does not establish acoustic alignment or compensate
drift. Output-timestamp discipline remains unfinished.

Stop, focus loss, hiding and teardown cancel pending work. Ordinary stop waits
for audio cleanup and a correlated Worker release, reports available actual
score and restores the accepted preview. Missing Worker stop receipts cause
bounded termination and require a page reload. No-runtime cancellation reports
null counters. Automatic completion, full capture/replay and browser multiplayer
remain unfinished.

Validation evidence is limited to cargo check: host workspace all-targets,
headless application all-targets, WASM browser library and WASM browser-audio
library exited successfully. Initial compile errors in fixture key types,
binding Result extraction and poor-background progress arguments were corrected
before the successful checks. Portable Rust fixtures and JavaScript boundary/
Worker/audio lifecycle regressions are authored for a later execution phase.
No tests, JS syntax runner, generated bindings, browser, graphics or audio
runtime, formal review or QA were executed. The full player Goal stays open.
