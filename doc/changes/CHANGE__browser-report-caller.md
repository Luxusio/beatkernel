# Worker-owned direct output report caller

Move actual Worklet report polling from Window to the attached gameplay Worker.
Window sends only its original output-presentation observation; report words
travel on the direct audio endpoint and pass through the existing Rust evidence
path. One serialized operation owner preserves command admission and report
ordering without hidden queues. Waiting output observations are serviced after
the current batch ACK and before the next active command batch. Actual input
chronology, pending state and current core completion remain authoritative.

Production source is written in main.js and worker.js. The page uses a
presentation-only packet; the Worker directly reads actual report words and
reuses the existing output observer for direct and explicitly unattached paths.
Actual queue probing accounts for newly produced work before completion is
published. Pending observation and audio operation block natural stop.
Six independently authored deferred fixture groups are written: Worker +3
(53 total), Window +3 (62 total), with all 14 preview groups unchanged. The
actual client/Worker source runs against controlled endpoints in deferred
fixtures; Window fixtures refuse AudioHost.poll() and require scalar-only
report requests. Coverage includes fair command/report ordering, original
presentation/current input evidence, refused overlap/payloads, report failures,
cancellation and late receipts. Scoped whitespace checks found no diagnostics.
No parsing, tests, assertions, generated bindings, runtime, formal review or
QA has been executed; unchanged successful Rust checks were not repeated for
this JavaScript-only change.
Browser, physical audio, generated bindings and measured input/render/main-thread
performance remain unverified. The full player Goal and task remain active.

Known ceiling: Window retains periodic AudioContext output-presentation and
control-frame/input observations, required user activation and lifecycle work.
This removes report arrays and Worklet poll/ACK relay from Window, but does not
prove input-only host execution or measured main-thread/input/render performance.
