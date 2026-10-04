# Direct gameplay audio command caller

Connect live and replay gameplay to the transferred AudioWorklet command port.
The Worker owns initial drain, active command submission and real prefix ACKs,
while Window retains browser-required output timestamp observation, setup and
joined cleanup. Core batch identities remain distinct from transport sequences.
No failed batch is retried or routed back through the Window command producer.
Command-pending and current input chronology remain completion barriers; late
ACKs cannot reach freed or newer owners.

Production source is written in main.js and worker.js. The page transfers the
matching endpoint with play-audio, and initial drain returns audio-ready only
after genuine core acknowledgement. Step replies carry commandsPending; render
replies also carry observedTick. Six independently authored deferred fixture groups are written: Worker +3
(50 total), Window +3 (59 total), with all 14 preview groups retained. The Worker
fixtures link the actual AudioCommandClient against controlled endpoints; Window
fixtures reject attempts to use the old host command API. They cover initial
drain, core/transport identity separation, prefixes, cancelled late responses,
endpoint handoff and pending/current-input completion. No test assertions,
parsing or runtime has been executed. Scoped whitespace checks are the only
verification for this JavaScript-only slice; unchanged successful Rust checks
are not repeated. Real audio,
generated bindings, browser execution and input/render/main-thread performance
are not yet verified. The full player Goal and task stay active.

Known ceiling: Window still polls real Worklet output reports and acquires
AudioContext presentation timestamps. This command migration removes command
arrays and ACK relay, not all continuous host orchestration. Real browser input,
rendering, audio and main-thread performance remain unmeasured.
