# Native owner gauge failure fence

NativeGameplaySession now retains a borrowed default BmsGauge on the game thread,
independently of UI attachment. Linux, macOS and Windows solo launchers supply
persistent gauge state; native local PlayerState owns a gauge initialized by
common preparation and roster constructors. Both common pumps reject nondefault
profiles before processing, preserving the fixed recorded policy.

Solo report processing consumes gauge, capture, competition and presentation
independently before fencing numeric failure. Its typed observation error retains
the complete actual report and independent errors, including simultaneous judge/
audio failures. Local processing consumes the whole actual prefix before fencing
newly failed members via bounded stack storage; observation errors retain original
reports, and group processing errors preserve both GroupError and independent
observation failure. Existing poison remains poisoned. Numeric failure alone does
not abort surviving local members. Captures skip only previously fenced operations
and keep successful failure prefixes. No OS-specific gameplay policy was added.

Two independently authored solo groups and two local groups cover actual common
pump/report paths with and without a publisher, committed fanout/PCM queue
preservation, capture reconstruction with matching retained judge state, sparse
high-u64 source ownership and surviving members, simultaneous capture/UI/
competition and group failures, and nondefault profile admission rejection.
Both writers delivered terminal Writes STOPPED before scoped formatting.
Assertions were not run.

Scoped Rust formatting and whitespace checks completed successfully. All four
authorized compile-only checks exited zero: workspace/all-targets WebTransport,
headless/all-targets WebTransport, WASM browser and WASM browser-audio. Existing
unused-code warnings remain in audio cadence and playfield progress helpers.
These checks do not establish test assertions or native/browser execution.

Known ceiling: per-player voice stops, pressed feedback cleanup, actual native
output drain and completion clear/fail outcomes remain unfinished. Legacy replay
wire semantics are preserved. The high-level mine admission guard remains;
Windows/macOS target compilation, real native input/audio/GPU, browser execution,
assertions, performance measurements and formal review/QA remain deferred.
