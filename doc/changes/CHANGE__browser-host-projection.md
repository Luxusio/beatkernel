# Worker projection of original browser host observations

Move relative audio scheduling and output-presentation projection from Window
to Worker. Window acquires original context frames and actual context/performance
timestamps; Worker reuses the exact shared conversion and prior monotonic guards
without substituting arrival time. A bounded shared helper preserves the existing
lookahead, floor conversion, prestart clamp and checked numeric range. Raw and
explicit lower-level projected forms are mutually exclusive. Input provenance,
output evidence, direct audio ordering and completion/cleanup barriers remain.

Production source and six independently authored deferred fixture groups are
written: model +2 (28 total), Worker +2 (55 total), Window +2 (64 total), with
all 14 preview groups unchanged. Source fixtures cover exact full-width/week/
lookahead/signed bounds, original Window timestamps across awaits and input
advancement, mixed-form refusal, monotonic/equal-output and unavailable evidence,
and scalar page acquisition without projection fallback. Scoped whitespace
checks found no diagnostics. No parsing, assertions, tests, generated bindings,
runtime, formal review or QA was executed. Unchanged successful Rust checks
were not repeated for this JavaScript-only change. Browser/device/audio
execution and measured input/render/main-thread performance remain unverified.
The original full player Goal and task remain active.

Known ceiling: Window retains browser-required frame/timestamp and physical
input acquisition, initial activation/start projection, periodic observations
and lifecycle work. Moving application calculations to Worker does not prove
input-only Window execution or measured input/render/main-thread performance.
