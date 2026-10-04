# Direct browser sample upload caller migration

## Owner boundary

The common live/local/replay setup hands one AudioHost sample endpoint to
Worker. PCM buffers travel directly from the actual game sample wrappers to
AudioWorklet through AudioSampleClient, without per-sample Window RPCs, copies
or value scans. Window retains browser input acquisition (keyboard, touch,
pointer, HID and Gamepad), permissions, activation, lifecycle, resize and
output observations; Worker retains input interpretation, judgment and
OffscreenCanvas/HUD rendering. Worklet owns audio rendering.

## Preparation and lifetime

Each insertion has a correlated ACK and finite timeout. Worker must enumerate
exactly the declared sample count, release every acquired WASM wrapper once,
verify the next actual sample is null and await end-samples ACK. Only the exact
admitted count/bytes receipt permits Window finish and command handoff. A
whole-bank 10-second timeout would incorrectly reject larger valid banks;
the aggregate upload instead relies on bounded count/bytes and each actual
client operation timeout.

Direct adoption and explicit legacy sample reads are mutually exclusive.
Stopping invalidates the owner before game release, cancels the pending setup
RPC and closes the upload client. Stale continuations must never access a freed
game. Failed or stale transferred endpoints close; there is no relay fallback.
Actual joined AudioHost stop remains required for processor deallocation.

## Evidence

Both producer and independent author returned actual terminal STOPPED finals.
Source inspection confirms one transferred sample endpoint, no Window PCM
relay, ACK-only sequential Worker upload, actual count/null/EOS gates and
identity checks before stale continuations can access the game. Shared sample
extraction preserves even null/false exceptions and releases wrappers once.

Seven independent deferred groups were added: host+3 (101 total), Worker+4
(101 total). All195 prior host/Worker groups are preserved. The14 preview
Worker groups are retained with actual AudioSampleClient module linking.
Fixtures cover solo/local/replay/empty uploads, original buffers and rates,
full u64 sample IDs, count/byte/configuration rejection, ACK/EOS and timeout
failures, command/activation bypass refusal, exact wrapper cleanup, null/false
exceptions, stop while awaiting ACK and late callbacks after replacement.
Source whitespace inspection reported no diagnostics. These are authored
fixtures and source evidence, not executed results.

No parser,
JavaScript assertion/test, app/browser/audio/device/network or generated
bindings execution is authorized for this slice. Rust is unchanged; Cargo
checks do not provide evidence for these JavaScript changes. Formal code and
security review and browser/CLI/desktop QA remain required before task close.
The complete BMS player Goal remains open.
