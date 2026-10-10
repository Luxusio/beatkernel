# Browser replay audio presentation admission

Recorded playback progresses from genuine output audio presentation. Raw
browser estimates may repeat or regress without proving a physical clock reset.
The browser adapter holds those observations before the strict replay port;
it still admits the genuine Worklet report for audio credits and validation.
No HOST elapsed time, render frames, nominal FPS or substituted previous point
advances the recorded timeline. A held observation is supplied as null.

The application sends its actual Window time origin for replay, as for live
play. Raw replay observations require that origin so asynchronous polls can
recheck freshness in the original Window host coordinate. Missing or invalid
origin is an explicit protocol failure; do not guess a cross-Worker offset.
Legacy projected output-only replay points remain compatible without a host
association. They preserve exact signed-64-bit nonnegative BigInt coordinates;
no host coordinate is invented for them.

Raw, paired and output-only entry paths share one accepted output frontier.
Duplicate/regressed output cannot progress it. Genuine pair coherence remains
checked against actual paired history, without combining an old host value
with a newer output-only observation. Commit adapter history only after native
report validation and completion/command checks succeed. Stop/replacement
fences late polls from a newer owner.

Strict replay/native report, domain, endpoint and command-count checks remain
authoritative. A held presentation does not erase a malformed report or claim
completion. Verify null-observation credits, chronological recovery, legacy
compatibility, asynchronous freshness, failure atomicity and full replay parity.

Browser replay exposes its actual maximum combo as an exact u64/BigInt getter
from the same Replay score used for hits, misses and current combo. Maximum and
current combo may differ. Reading these fields does not create live completion
proof or substitute an archived score.

Nonzero mixer rejection counters remain terminal. The browser diagnostic names
each offending fixed counter (`pending_full`, `voice_full`, `unknown_samples`,
`unknown_stops`, `invalid_gains`, `invalid_rates`, `invalid_times`) and its exact
integer value. Preserve armed-start, flags, extent and overflow validation;
diagnostic detail does not authorize swallowing a counter or changing clock
admission. Exercise single/multiple counters and high-word values, then retain
actual retry evidence for any further mixer correction.

The new named-counter path passed all 38 audio/play-model development tests.
The current release WASM built and wasm-bindgen regenerated the actual
BrowserReplay maximum-combo getter; package SHA-256 is
`5d947a3f11cf8cf1589f9210d3f9d9dda0073bac5ac2c0f73221dfa9b4626358`.
Generated API presence is not actual browser score or retry evidence; those
were subsequently exercised by independent browser QA on 2026-10-10.
The actual replay reached terminal `completed=true` at song 6000000000 ns,
with hits/misses/combo/maximum combo exactly matching capture: 1/4/0/1.
Conflicting live selectors did not supply a replacement replay timing policy.
Two actual local keyboard/touch recordings retained all sixteen selected
timing windows. No mixer rejection or console/Worker error recurred. Evidence:
`target/wf/qa-browser-record-continuity-vulkan/focused/evidence.json`.
These passes do not resolve the blank historical screen reported in the
[historical record contract](REQ__browser-historical-record.md); the connected
task's browser verdict remains FAIL.

Actual QA preserved a complete six-second recording that failed against
baseline3aa4ccb at song269326157ns with `completion presentation precedes its
output frontier`. The replay must finish after correction, including repeated
zero-extent/recovery; a shorter passing prefix or successful GPU frame is
insufficient. Baseline recording and traces are ignored artifacts under
`target/wf/browser-render-feedback/qa-browser/baseline`.

## Development checkpoint — 2026-10-09

The adapter correction passed 336 relevant Node tests, including eleven new
clock/frontier/setup cases. A current-source development Chromium run captured
the full six-second section, saved its recording and replayed it to an actual
`play-render-done` with completed=true at song6000000000ns, followed by
`play-stopped`, with no replay error. It exercised real zero-extent/recovery
in live/history/replay and kept exact status feedback. The owned server/browser
exited; evidence is under `target/wf/browser-replay-frontier/development`.
This is development proof, not independent QA or whole-player completion.
The unchanged current native test artifact also passed nineteen StepReplay
boundary/finite/stop-ACK cases, preserving strict evidence and retained-credit
behavior. No Rust or WASM source was rebuilt or relaxed for this correction.

Source/test and independent discovery/review allocations returned the host's
agent thread limit. Coordinator implementation and tests do not substitute for
the required independent review and QA. Resume those roles when allocation is
available, retain the full six-second completion acceptance, and do not claim
task completion from this checkpoint.

## Current package verification — 2026-10-10

At source `fc60219`, the current browser WASM SHA-256 is
`3b8d33803fb2979ce51f04fdbf8cd5908b4eee2e2e8fc0b292755283116ab1b5`;
the AudioWorklet package hash is
`7235be08ccf465e4b67678afa34273abe1d30ed9cdc05aafc8556fa4f42f06dc`.
The release builds and wasm-bindgen generation exited zero. Independent CLI
inspection confirmed wasm-bindgen 0.2.129, the browser-native-free dependency
graph and the unsigned-u64 `BrowserReplay.max_combo` BigInt binding. These
build/static facts do not prove replay completion.

The current headed connected run proved six-second capture, saved-record
acceptance and visible historical score `1/4/0/1`. Its replay observation
incorrectly selected the first progress `play-render-done`; this notification
is terminal only for the current play ID with `completed === true`. A second
run using that corrected selector failed actual gameplay initialization before
play began. Consequently current-package six-second replay completion and
exact score parity remain unverified. Preserve earlier successful scoped
replay observations as dated evidence, without promoting them to this run.
See [current historical acceptance](REQ__browser-historical-record.md#current-headed-acceptance-evidence--2026-10-10)
for the failure, cleanup and artifact locations.
