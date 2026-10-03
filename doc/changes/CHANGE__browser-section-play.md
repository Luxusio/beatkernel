# Browser original-song section Play

AC-203/204 connects the stepped section owner to real BrowserLibrary preparation
and Window/Worker launch. Browser preparation decodes fresh original assets,
uses the existing section_start::prepare_at once, retains original-song chart
indexes and moves the selected bank into the live owner. The prepared resource
carries the actual start; the live constructor uses StepGameplay::new_at.
Recorded replay preparation carries its decoded start and retains its dedicated
replay owner.

The Window retains a Start time draft in seconds and snapshots exact integer
nanoseconds before asynchronous setup, preserving the audio resume gesture.
Busy owners block edits; failed preparation and stopped playback preserve the
draft for retry. The Worker independently validates the requested start, uses
section preparation only for nonzero live starts, and compares actual prepared
metadata with the request before activation. The Window checks the same returned
start. Replay ignores the live draft and uses its original recorded section.

Original asset decoding remains bounded to 1296 samples. Up to 4096 crossing
BGM suffixes can be added by the existing selection helper, so the output host
admits at most 5392 samples while preserving 64 MiB per-asset and 256 MiB total
PCM budgets. Song time, visuals, recorded operations and comparison identity
retain original absolute coordinates. Audio target mapping subtracts the
section start exactly once; no new clock or judging path is introduced.

This is source integration. Generated bindings, browser/application/audio/device
execution and acoustic or gapless restart acceptance remain deferred, together
with tests, formal review and QA. Goal remains active and task open/PENDING.


Direct source tracing also found BrowserPrepared.lanes exported as a method
while the actual Worker reads a property. The owned binding now explicitly
exports a getter to match that caller. Existing property-shaped mocks did not
prove the old generated API worked; this is a source-level correction, with
actual binding generation and browser acceptance still deferred.


Six independent JavaScript fixture groups are authored (one numeric boundary,
three actual Worker source groups and two Window source groups). Existing
gameplay Worker fixtures receive only compatible start metadata and a corrected
exact prepared response expectation including opponentCount. These fixtures
remain unparsed and unexecuted; instrumented bindings prove no real judge or
generated API behavior. Genuine Rust section/Mixer/reconstruction fixtures
compiled in AC-202 remain the separate prepared test evidence.


Scoped Rust formatting and whitespace checks completed. Cargo check reached
exit0 for workspace/all-targets, headless app/all-targets, WASM browser library
and WASM browser-audio library after this integration. The three existing
platform render-cadence dead-code warnings remain on WASM. No JavaScript
parsing, test/assertion execution, generated binding build or product runtime
verification is implied by these compiler results.
