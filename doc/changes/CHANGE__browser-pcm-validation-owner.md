# Browser PCM value validation ownership

## Change and lifecycle

AudioHost.sample on Window retains metadata/layout/count/byte checks and detached
buffer detection, then transfers the original exclusive buffer. It no longer
runs Number.isFinite over every PCM element on the input acquisition thread.
The existing AudioWorklet sample handler checks values before invoking
BrowserAudio.insert_sample, whose Rust validation remains authoritative too.
This removes one application O(PCM length) Window loop per uploaded asset;
actual scheduling, latency and responsiveness remain unmeasured.

Bad NaN or infinite PCM now yields an asynchronous correlated remote sample ACK
error after transfer. That response fences the host; Worklet rejection fences
its processor before the WASM insertion call. The consumed buffer is not retryable.
Host sample identity and byte accounting update only after a successful ACK.
Metadata and memory-layout errors still reject locally before transfer. Existing
single-pending deadlines, generation/sequence filtering, stop races and cleanup
joining remain unchanged. This is a deliberate validation-owner/timing change
for standalone AudioHost.sample as well as the player launch caller.

Setup PCM still passes through Window as transferable messages and is not
claimed direct Worker-to-Worklet upload. AudioContext activation/output clock
observations/cleanup remain with the browser-required host; continuous commands,
input interpretation and gameplay rendering remain with Worker/Worklet.

## Source evidence and deferred acceptance

Producer changed audio-host.mjs only. Independent author added two host groups
(24 total) and one Worklet group (16 total). Both returned terminal STOPPED finals. The new
fixtures cover all three non-finite values, original-buffer transfer, stale versus
matching ACKs, asynchronous refusal, terminal fencing, joined context-close and
explicit Worklet stop/free, plus valid empty-buffer ownership. Metadata refusal
fixtures remain intact; the fake host boundary sends an explicit rejected ACK
rather than simulating a second host-side PCM scan. Root source inspection finds
the existing Worklet finite-value loop immediately before insert_sample and the
Rust PcmSample finite validation. Whitespace inspection reported no errors. Tests and JS
parsers are not executed under the standing deferral. This is JS-only source
work, so unchanged Rust checks are not repeated or presented as browser evidence.
Ordered code/security/browser/CLI/desktop review and QA plus actual browser/audio
and measured performance remain pending; the full player Goal stays active.
