# Browser audio host ownership

The browser audio component gains an actual AudioContext/AudioWorkletNode owner
in `samples/bms-runtime/web/audio-host.mjs`, using the existing Worklet protocol.
Opening from a user gesture requests resume before asynchronous initialization.
The host uses a precompiled audio module, the actual context sample rate, explicit
bounded configuration and a finite setup deadline with cancellation.
Read-only context-frame estimation supports choosing a future arm request;
actual Worklet chronology rejects late requests. A fulfilled resume must leave
the context running, and subsequent suspension/interruption fences the owner.

The host separates resource setup, allocation, one-shot context-frame arming,
command admission, actual render reports and shutdown. Only one ordinary control
operation can be pending; callers cannot build an unbounded host queue. Sample
transfer consumes a full standalone backing buffer. Exact generation, sequence,
operation and admitted-prefix acknowledgement protect command ownership. A
partial admission or processor/transport failure fences the owner, without
retrying committed gameplay input. Stop cancels pending operations and cleans
node, port, timers and context even when acknowledgement fails.
A bounded close attempt that times out cannot prove browser resource release.

This source step does not connect the preview page to gameplay. Prepared-resource
wiring, shared nonblocking SoloRuntime ownership, keyboard timestamp/watermark
handling, output-presentation mapping, capture/replay/results and WebTransport
remain required for the full browser player.

## Verification status

Production source and 14 independent regression groups are authored. Tests load
the actual host module into a Node VM with controlled WebAudio endpoints and
timers; they cover ownership, transfer, admitted-prefix failures, correlation,
cancellation/deadlines, context interruption and cleanup. Tracked diff checks
and explicit new-file whitespace checks reported no diagnostics. Source
inspection corrected the test helper to retain failed state after clean cleanup.

No JavaScript syntax/test execution, generated bindings, AudioContext/Worklet/
device execution, formal review or QA is performed under the standing user
deferral. No compile/runtime/acceptance PASS is claimed for this JavaScript
component. Cargo source was unchanged; prior Rust checks are not evidence of
this host's browser behavior. The full player Goal remains active.
