# Negotiated group network session

The common Session gains an explicit group constructor with independent
bounded local/remote rosters and the same canonical gameplay identity. Group
setup uses tag 14 with a BKGC envelope; scalar setup retains tag 1 and arbitrary
identity bytes. Progress/final group frames use tags 12/13 and the existing
whole-cohort codec. The legacy scalar constructor and lifecycle event enum
retain their existing behavior.

The group uses one actual readiness, probe/start agreement, outbound write
owner and final acknowledgement state. Group final flags create no write
credit. Exact full-frame completion precedes a valid peer ACK, and data waits
for the common committed start. Group roster/progress events share the bounded
eight-event budget with normal lifecycle events. Wrong mode, invalid member
transitions, changed rosters, unexpected sequences and stale credits fence
mutations while committed events remain drainable.

Browser bindings add explicit group construction, exact member-word submission
and typed roster/progress polling. Portable roster and word decoders validate
inputs using the same common rules. Host inputs must also be bounded before
generated WASM glue copies them. No new platform protocol, gameplay evaluator,
clock sampler or per-member start coordinator is introduced.

Six deferred fixture cases were added: five exercise paired actual Sessions,
and one covers roster/member-word boundaries. The cases cover independent
rosters, scalar identity collision avoidance, common clock/start negotiation,
write/ACK barriers, rejected prefix retention and the combined event bound.
All-target workspace and headless checks, plus browser and browser-audio WASM
library checks, completed successfully. These are compilation results; no
fixture assertions or generated bindings were executed.

The actual Worker/Page/native transport callers, remote HUD targeting and
multi-host room integration remain unfinished; local network UI still refuses
explicitly. Authored fixtures and compilation do not establish socket/device
execution, generated binding acceptance, measured performance, formal review,
QA or full player completion.
