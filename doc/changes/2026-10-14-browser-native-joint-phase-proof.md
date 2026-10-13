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

The nine author joint cases' emitted summaries total 83,166 observed callbacks
and 66 raw snapshots. Final transport contains 149 replies and 68 retained raw
records. They kept production phase 0, zero PCM and reports unavailable.
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

Formal code review returned PASS at `ff94e215` after both fresh defect hunters
returned empty arrays and the reviewer independently swept all six planned
paths. Fresh helper execution passed 51/51; the reviewer independently checked
3,501 retained active lane samples and 22 common-index CRT endpoints.

Subsequent independent CLI execution passed the focused 146/146 tests, syntax
and diff checks. An independent general CRT oracle also matched 20,028 helper
vectors and checked 42 invalid inputs. The copied GUIDE Bash command produced exactly one invocation
with all five intended environment values. All three joint-mode conflicts
exited 1 before any HTTPS server, child spawn or Puppeteer acquisition.

Fresh browser execution passed joint 11/11, single-phase 11/11 and marker
19/19, with all three Node sessions exiting 0. These are separate actual runs
under `target/wf/browser-native-joint-phase-proof/qa-{joint,phase,marker}-c1/`,
using owned port 8164 and the unchanged production source/WASM bytes.

| Fresh joint stream | 44.1 kHz | 48 kHz |
|---|---:|---:|
| Native callbacks |24726|24737|
| Joint active samples / callbacks / CRT checks |3145719 /24577 /49154|3145719 /24577 /49154|
| Own lane sample counts |3145728 /3145725 /3145719|3145728 /3145725 /3145719|
| Own lane maximum errors |0.06250000058207661 /0.06250000069849193 /0.06250000046566129|0.06250000046566129 /0.06250000046566129 /0.06250000046566129|
| Summary-time / final raw records |13 /14|13 /14|
| Private replies |27|27|
| Raw global discrepancies |10|13|
| Closing interval frames |3155456|3156224|

Fresh joint emitted summaries total 83,164 callbacks and 66 raw snapshots;
final transport contains 149 replies and 68 retained raw records.
Independent browser and CLI oracles checked 3,501 retained active lane values
and 22 endpoints using submitted double event times and general exact CRT.
The CLI additionally checked 512 plausible first-two-lane samples and 256
third-lane samples in the refused late-admission watch. All watched third-lane
samples failed the nominal oracle after native late clamping; no joint session
or candidate was granted. Watched ordinals 170/171 precede retirement 172 in
this fresh run, independently of the author's retirement ordinal 173.

Fresh single-phase emitted summaries total 82,772 callbacks and 142 raw
snapshots; final transport contains 219 replies and 144 retained raw records.
Its positive streams record 24,725/24,741 callbacks, 41/45 replies,
maximum error 0.06250000058207661 at each rate, and summary/final raw counts
27/28 and 31/32. The extra warmup records remain within the unchanged cap32;
summary and final-stage counts need not equal the author run. Independent
checks covered 1,357 retained active samples. The marker regression records
2,444 callbacks, 2,507 replies and 2,444 raw records; eight positive marker
intervals independently conserve native extents.

Browser QA visited and captured 38 actual case/boundary pages at 800x600 and
visually inspected every screenshot: blank diagnostic pages are expected.
Verification depth is render-only-browser; native programmatic AudioParam,
Host, context and private-port interactions ran, while UI click/fill controls
do not exist. There were 12 expected console 404 reports, nine boundary body
fetch aborts associated with actual 404 responses, and zero page errors.

All 35 contexts, 38 pages and 94 private endpoints closed. Owned PIDs 1860661,
1878474 and 1894670 were absent at their respective run completions; kernel
tables showed no listener on 8164 and local bind/close passed between runs
and after the final run. No native retry, dependency installation, Rust build
or WASM regeneration occurred. Full-stream scans/maxima are emitted native
aggregates; retained-array oracle checks independently substantiate slices.
After the formal review, both independent qa-cli and qa-browser agents returned
actual final PASS verdicts and checked the published fresh measurements.
Those substantive finals remain separate from receipt-backed task status;
only ordered runtime-owned completion evidence can authorize task close.

## Known ceiling

- Known ceiling: Full-modulus aliases cannot certify epoch or age.
- Known ceiling: finite phase0 diagnostic evidence only; native precision remains unproved until browser execution.
- Known ceiling: retained arrays independently substantiate boundary slices; full-stream error maxima and scans remain emitted native aggregates. Formal review and fresh QA remain mandatory. Product chronology, gameplay and the full Goal remain unproven.
- Known ceiling: Retained arrays independently substantiate boundary slices; full-stream scans and maxima remain emitted native aggregates corroborated by actual-source tests and conservation. Unretained samples were not independently scanned.
- Known ceiling: Joint residues identify only Number-safe modulo candidates; native samples alone cannot certify epoch, age or future chronology beyond the finite measured interval.
- Known ceiling: A single repeating phase cannot distinguish nP-frame gaps; checked samples and conditioned endpoint corroboration establish only this measured interval.
- Known ceiling: AudioParam fill/scan and private diagnostic messaging are sustained test overhead, not a startup-only or zero-cost production claim.
- Known ceiling: Reused generated WASM is artifact-specific phase0 evidence. No fresh Rust build, strict playback repair, gameplay completion or full player Goal completion is claimed. The earlier auxiliary health failure reflected PATH configuration; existing Rust availability was subsequently confirmed by root.

The implementation worker's native-precision statement preceded the actual
author runs above; the pre-review statement requiring formal review and fresh
QA also preceded their actual final PASS results. Author and fresh QA runs now
measure both requested rates; cross-browser,
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
