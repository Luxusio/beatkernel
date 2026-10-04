# Browser local gameplay boundary

Expose the common StepLocalGameplay owner to browser Workers. One live prepared
chart and original PCM bank feed the shared output transport, BGM producer and
command/ACK lane. The portable numeric setup retains ordered player IDs and
full-width acquired sources; per-player physical rows enforce assigned exact
ownership and chart coverage within a 256-row aggregate budget. Canonical inputs
retain their actual variant, timestamps and provenance. Member capture and
feedback remain separate; actual output evidence is validated before BGM work.

BrowserView local drawing reuses the shared four-field page composer and bounded
image cache. Its borrowed presentation bridge reads actual score, recent results
and note progress without cloning score maps or chart-sized progress per frame.
The existing native snapshot composer uses that same borrowed core. Roster/page
validation precedes canvas resource changes; visible backgrounds follow each
member's actual frontier. Presentation pages do not limit the gameplay roster.

Six independently authored deferred fixture groups cover full-width source and
physical identity preservation, automatic solo routing and actual fanout,
malformed/source-leaking setup refusal, aggregate/member/codec bounds, real
three/four-member input and shared PCM/capture/reconstruction, and borrowed versus
owned Scene packets on sparse member pages. They include retained-reference and
invalid-page/roster geometry checks. Their assertions have not been executed.

Scoped Rust formatting and whitespace inspection completed cleanly. Four
compile-only configurations finished with exit zero: workspace all targets,
headless app all targets, wasm32 browser library and wasm32 audio library.
Existing three platform cadence dead-code warnings remain on WASM. No tests,
JavaScript parser, binding generation, build/link, app/browser/device/audio/network
execution, benchmarks, formal review, QA, task verification or close ran.

Known ceiling: The current browser page and Worker gameplay caller still use
the solo binding. Actual local device assignment, Worker protocol integration,
page controls and contact projection into member fields remain required work.
The numeric API does not certify that supplied sources were genuinely acquired;
the actual host must check admission against its retained acquisition owners.
The 256-row aggregate budget can constrain wide-lane charts before the common
64-member roster bound; incomplete coverage refuses explicitly.
Browser execution, generated JS bindings, hardware/audio completion and measured
latency/performance remain unverified. Required ordered review and browser,
CLI and desktop QA remain pending; the full player Goal stays active.
