# Stepped gauge failure fence

StepGameplay now fences its actual solo runtime after consuming the numeric
failure-causing report for gauge, mine damage, score and capture. StepLocalGameplay
consumes the entire committed report prefix before fencing only newly failed
members through bounded stack storage. Numeric failure does not become a
technical owner error; simultaneous technical errors retain their original report
and independent observations. Read-only fence getters expose the actual frontier.

Subsequent acquisition retains clock/sequence validation but contributes no new
judgments or gameplay sounds. The failed member's capture excludes later empty
advances/acquisition; successful capture retains the exact failure-causing prefix,
while capture errors preserve their shorter accepted prefix and error evidence.
Surviving members continue independently. Frozen member reports do not overwrite
the shared control song frontier with an earlier value. Existing queue commands
remain intact. No replay wire version or retrospective legacy-log interpretation
was changed.

Four independently authored deferred groups cover actual multi-binding committed
reports/queue preservation, failure-prefix reconstruction with matching retained
hash and ReplayVisual/StepReplay observations, distinct local survivors/captures
and shared progress, and queue/capture technical errors alongside numeric failure.
The existing five gauge groups remain, with only post-fatal continued-play
expectations updated to the automatic fence. Both writers delivered terminal
Writes STOPPED before scoped formatting. Assertions were not run.

Scoped rustfmt and git diff --check succeeded. Authorized compile-only checks for
workspace/all-target WebTransport, headless/all-target WebTransport, WASM browser
and WASM browser-audio each completed with exit 0. Existing unused playfield-wrapper
and WASM cadence warnings remain. Compilation does not execute the authored
reconstruction/failure fixtures or establish browser/device/output acceptance.

Known ceiling: native owners, pressed feedback cleanup, per-player audio stops,
output drain and clear/fail completion remain unfinished. Legacy recordings keep
their recorded operation semantics; configurable policy/capture identity is not
introduced. The high-level mine admission guard remains. Tests, applications,
browsers, devices, measured performance, formal review and QA remain deferred.
