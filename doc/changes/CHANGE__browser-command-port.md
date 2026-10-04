# Dedicated browser audio command transport

Introduce a bounded transferred command port between a future gameplay Worker
caller and the actual AudioWorklet. The host retains setup, activation, output
clock observation and joined cleanup. Command admission shares the existing Rust
enqueue path; the new endpoint has independent ordering and actual admitted-prefix
ACKs. Host command ownership cannot coexist with or silently recover from a
transferred command lane. A bounded single-pending client correlates responses
and fences errors without retrying partial commands.

Production source and seven independently authored deferred fixture groups are
written: command client 3, host +2 (22 total), Worklet +2 (13 total). The source
fixtures exercise actual modules against controlled endpoints. Scoped whitespace
checks found no diagnostics. No JavaScript parsing, assertions, tests, browser,
audio runtime, generated bindings, formal review or QA was executed. This is
a JavaScript-only change, so successful unchanged Rust checks were not repeated. Live and replay gameplay callers still
use the existing Window bridge until the next dependent integration. No removal
of output evidence, completion barriers or cleanup joins is authorized. Source
fixtures are deferred, and no browser, real audio or performance acceptance is
claimed. The full player Goal and task remain active.

Known ceiling: client close proves only local port closure; AudioHost remains
the sole actual deallocation owner. Gameplay caller migration, browser execution
and measured main-thread/input/render performance remain pending.
