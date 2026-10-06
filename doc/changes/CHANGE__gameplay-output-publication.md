# Connect paused output publication to shared gameplay

The common gameplay device contract gains an optional paused-output publication
operation. Solo and cohort pumps invoke the same contract without selecting a
platform backend or recreating the session. Existing native devices keep the
default no-replacement behavior until their output ownership and UI command
composition are connected.

Publication must validate the ready output and stage finite-completion evidence
before changing output ownership, pause/presentation clocks or resume origins.
The hold remains owned on refusal and leaves audio paused after publication.
Original logical song position and retained input/replay/competition state stay
with the existing session. Finite completion cannot interpolate across output
epochs using a previous backend bracket.

`GameplayOutputContext` carries the existing timing owners and an encapsulated
`GameplayPauseControl` that obtains a hold from the actual solo/group producer.
`publish_ready_output` requires an empty output slot and stages all fallible
validation before assigning the output, observer, pause, origins and end owner.
`NativePause::validate_replacement` checks original startup/finite/frozen identity;
`NativeEnd::restart_for_output` preserves that original grid and discards previous
observation brackets. The compatibility device trait forwards the hook through
the existing static adapter.

Six independent fixture groups are authored: four genuine memory-rendered
publication/refusal/finite-end groups and two actual solo/cohort pump traces.
The pump cases use unlimited playback prefixes; finite endpoint behavior is
covered separately through the real publication helper, Mixer and NativeEnd.
After both writers returned terminal `Writes STOPPED`, scoped Rustfmt completed
and four sequential compile-only commands exited zero: workspace all-targets
with WebTransport, headless runtime all-targets with WebTransport, WASM browser
library and WASM browser-audio library. All six fixtures compiled in the host
all-targets configurations. Existing unused-code warnings remain; no assertions
were executed and no runtime/performance or cross-platform hardware evidence
was produced. Git whitespace checks completed.
Assertions, actual devices, browser, formal reviews and QA remain deferred.
The full player Goal stays active; platform/UI command wiring, blocking foreign
initialization isolation and acoustic accuracy are still outstanding.
