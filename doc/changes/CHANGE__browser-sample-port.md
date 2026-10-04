# Dedicated browser setup sample transport component

## Ownership and protocol

AudioHost.openSamplePort hands a dedicated endpoint to AudioWorklet during empty
setup and returns a frozen descriptor for a Worker-side AudioSampleClient.
It permanently transfers sample producer authority, even when adoption becomes
ambiguous after an error; host sample uploads cannot silently resume. Window
retains AudioContext activation, output clock observations, finish/arm and actual
joined shutdown. The existing command endpoint remains separate.

AudioSampleClient validates bounded sample metadata and exclusive transferable
backing, then sends each original PCM buffer without copying it. One operation
is pending at a time, with independent generation/sequence and finite timeout.
Identity and byte counters advance only on a matching successful ACK. An explicit
end-samples carries those exact totals; the Worklet verifies them against its
actual admitted sample IDs/bytes, acknowledges and closes the setup endpoint.
Host finish before that genuine end fails. Client end/close fences the local producer; it does not prove endpoint resource
release and cannot substitute for actual processor stop/free.

Worklet host and endpoint paths share the same pre-insertion metadata/finite
validation. Duplicate, malformed, wrong-owner/sequence, over-limit and non-finite
uploads refuse explicitly. Terminal failure reaches the sample endpoint too;
actual fresh host stop closes all transferred endpoints and owns deallocation.
Late saved callbacks after closure cannot act on replacement owners. Preserve
legacy host-only setup and the dedicated command path. No new render callback
allocation or alternative audio engine is introduced.

## Source evidence and remaining migration

Both producer and independent author returned terminal STOPPED finals. Source
implements AudioHost.openSamplePort, AudioSampleClient.sample/end/close and the
Worklet attach-samples/sample/end-samples endpoint. Root source inspection
confirmed the single shared sample helper, independent sequence/ACK boundary,
actual count/byte end gate and close/stop lifecycle, with process() unchanged.

Ten independent deferred fixture groups are added: host+2 (26 total), Worklet+3
(19 total), new sample-client5. All40 prior host/Worklet groups remain. Fixtures
cover exact transferred buffers, ACK-only accounting and frozen end totals,
metadata/finite/duplicate/budget refusal, exclusive ownership, independent
sequences, stale callbacks, getter reentrancy, deadlines and joined release.
Whitespace inspection found no diagnostics; these are authored source fixtures,
not executed evidence.
At this component commit the live/local/replay caller still used Window PCM
relay. The dependent caller migration is recorded in
[Direct browser sample upload](CHANGE__browser-direct-sample-upload.md). Component source does not establish
actual direct application upload, real browser/audio behavior or reduced input
latency. No JavaScript parser, assertion/test, browser/app/audio/device/generated
binding or formal review/QA execution is claimed; unchanged Rust checks do not
provide evidence for this JS-only component. The full player task/Goal remains
open, with ordered code/security and browser/CLI/desktop QA still required.
