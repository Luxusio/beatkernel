# Stepped replay acknowledged stop evidence

StepReplay now stages the small shared owned Stop evidence and commits it only
after the existing ACK validator accepts a full or valid partially rejected batch.
The acknowledged_stop_commands getter retains that exact admitted Stop prefix
after failure. Invalid/repeated/unsolicited ACK earns no credit; original typed
batch errors and technical fences remain. No command-vector clone or new wire
representation was introduced.

The shared section-output and render-cursor algorithms now have explicit internal
owned-evidence variants. StepReplay supplies only its actual ACK count; raw
unknown_stops must fit both that count and commands_applied. Generic public
validators keep zero allowance, and all other clock/grid/capacity/counter/
chronology/endpoint checks remain. Frontiers are adopted only after validation;
finite exact command counts and unlimited later-idle/presentation barriers remain.

Four independent deferred groups use actual captured mine-aware replays, retained
batches and queues/Mixer to cover full/partial/invalid ACK, unacknowledged versus
accepted inactive Stop output, ten malformed-report cases without visual/feeder
frontier adoption, and finite/unlimited completion plus no-mine behavior. Full
logical replay hash is compared through reconstruction/planning; StepReplay event
and score prefixes are compared through its existing interfaces, with no test-only
production seam. Both writers delivered actual terminal Writes STOPPED before root
scoped rustfmt and the exact four authorized locked compile-only checks.
Workspace/all-targets webtransport, runtime no-default/all-targets webtransport,
WASM browser lib and WASM browser-audio lib exited zero. git diff --check emitted
no diagnostics. Existing unused strict-wrapper/platform cadence/playfield warnings
remain.

Assertions, actual browser/Worklet/device, performance, formal review and QA were
not executed. Task remains open/PENDING and the Goal active; no PASS or completion
receipt is claimed. Native/live/local output ownership, final clear/fail and
high-level mine file admission remain unfinished.
