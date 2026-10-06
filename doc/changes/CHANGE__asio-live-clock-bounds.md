# ASIO live clock bounds

ASIO live settings now include timer measurement error, drift error, output
latency error and anchor age alongside driver/buffer/channels/matrix: eight
advertised fields. Empty values preserve the current explicit estimates.
Nonnegative ASCII decimal values must fit signed timestamps; age is positive
and below the finite timer half-wrap horizon. Combined age/error/bracket bounds
must exclude wrap ambiguity before queueing output replacement.

SDK-free `MultimediaClockAnchor::validate_bounds` shares the existing constructor
rule. Width zero checks minimum feasibility without creating a fabricated
anchor. Native admission also samples an actual QPC-bracketed timer receipt
before retiring output. Accepted values reach the new stream's configuration;
actual native startup creates and renews fresh anchors using those values.
The QPC origin, source Mixer and rate, request correlation, epoch/frame basis,
driver trust and pending/HWND cleanup remain unchanged.

Portable tests cover preservation, canonical eight-field replies, malformed and
ambiguous bounds and actual-width boundary validation. Retained UI tests reach
the last page and submit all eight values together. WASAPI refuses ASIO-only
clock fields before queueing. Independent code/security reviews and scoped QA
passed: platform 207 passed / 1 ignored, library 1,588 passed / 2 ignored, main
242 passed, Windows binary 39 passed, and actual ALSA settings diagnostic 1
passed, zero failures. All four new bound/parser/8-field UI fixtures executed.
Workspace, WASM, Windows normal and isolated SDK Rust checks exited 0; help
exited 0 and invalid CLI input returned expected exit 1.

Caller estimates are
not measured driver precision; real Windows sampling/audio/SDK bridge and
physical acceptance remain unverified. Full player completion remains pending.
