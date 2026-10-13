# Browser joint native phase proof

The opt-in native fixture now measures three coprime AudioParam ramp lanes
together and reconstructs exact Number-safe modulo offsets. This is evidence
for the next product-clock repair, not a replacement master clock. Production
Host/Worklet/Rust/WASM and legacy bootstrap/runCase/verify remain unchanged;
the earlier strict chronology failure and WBS 02.07/11.08 remain unresolved.

Implementation: `cf66618b` adds the exact diagnostic helper and independent CRT
vectors; `1f9c5652` adds the joint fixture and actual-bootstrap tests. The
[guide](../platform/GUIDE__browser-native-frame-diagnostic.md#joint-native-phase-experiment)
contains the reproducible command, input prerequisites and fixed bounds.

## Author verification

Focused Node tests passed 146/146, including 51 helper cases and 28 actual
controller/bootstrap cases. Source syntax, scoped diff checks and byte identity
against `d06cb175` passed. Actual Chromium 153.0.8010.12 runs passed joint 11/11,
existing single-phase 11/11 and marker 19/19; all three Node processes exited 0.
Author artifacts are local ignored files under
`target/wf/browser-native-joint-phase-proof/author-{joint,phase,marker}-c1/`.

| Measured joint stream | 44.1 kHz | 48 kHz |
|---|---:|---:|
| Native callbacks |24726|24737|
| Joint active samples |3145719|3145719|
| Joint active callbacks |24577|24577|
| CRT endpoint checks |49154|49154|
| Own lane sample counts |3145728 /3145725 /3145719|3145728 /3145725 /3145719|
| Own lane maximum errors |0.06250000058207661 /0.06250000069849193 /0.06250000046566129|0.06250000046566129 /0.06250000046566129 /0.06250000046566129|
| Summary-time / final raw records |13 /14|13 /14|
| Private replies |27|27|
| Raw global discrepancies |16|17|
| Closing interval frames |3155456|3156224|

Only 128-frame native extents were measured. Each lane has two interior resets:
first indices 17/16/14, second 17/15/11. Own horizons fall at 17/14/8; nine/six/zero
tail samples remain after common eligibility ends. All six resets and three
horizons retain one all-lane record per actual callback cluster. The completion
summary follows closing-marker corroboration and precedes its final raw callback.

The nine joint cases observed 83,166 callbacks, 149 replies and 68 retained raw
records in total. They kept production phase 0, zero PCM and reports unavailable.
Nine contexts, ten pages and 26 private endpoints closed. PID 1752816 was absent,
kernel tables had no listener on 8163 and local bind/close passed. Single-phase
PID 1771432 and marker PID 1791065 were also absent after their completed runs.
All three retained the same production Worklet/WASM hashes:

- Worklet: `5263660c246739e6e1daf29c4aa2ea4b8d9e338cdc75d71e4a654dbad15266c8`.
- Reused WASM, 74,651 bytes: `34c0bda380ad673c29298e0cec0873009cdfe274772d55904537dff8acef0b18`.

An independent pre-review arithmetic inspection recomputed 3,501 retained active
lane samples from submitted double event times and 22 common-index endpoints
using a general exact CRT oracle. It found no discrepancies. Constant lane 2
remained unscheduled scalar 0 while lanes 0/1 checked 111 samples each; no joint
candidate was granted. Late lane 2 had eligibility [true,true,false], no admitted
session, and watched ordinals 170/171 before retirement 173.

## Independent review and QA

Formal code review and fresh independent CLI/browser QA are pending. Author
measurements and arithmetic inspection do not authorize task completion.

## Known ceiling

- Known ceiling: Full-modulus aliases cannot certify epoch or age.
- Known ceiling: finite phase0 diagnostic evidence only; native precision remains unproved until browser execution.
- Known ceiling: retained arrays independently substantiate boundary slices; full-stream error maxima and scans remain emitted native aggregates. Formal review and fresh QA remain mandatory. Product chronology, gameplay and the full Goal remain unproven.

The implementation worker's native-precision statement preceded the actual
author runs above. Those runs now measure both requested rates; cross-browser,
aged-context, rolling-horizon and product authority remain unproved. The fixed
modulus 1152917106563481600 exceeds the safe Number domain, but modulo uniqueness
alone cannot establish an epoch. Diagnostic BigInt, native parameter generation
and full scanning are sustained costs, not zero-allocation product evidence.

## Next product work

Connect an immutable certified native origin and fixed scalar Rust clock
reconstruction to actual Host/Worklet rendering, pending arm commitment and
bounded rolling horizons. Preserve strict core chronology and output report
coordinates. Product startup must not inherit the diagnostic 200ms scheduling
margin or a boot-time three-period measurement. Fresh Rust/WASM, strict cold
health and actual live/local/section/seek/replay completion still need verification.
The full cross-platform player, UI, competition and platform scope stays active.
