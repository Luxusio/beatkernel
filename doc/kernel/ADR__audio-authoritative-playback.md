# Audio-authoritative BMS playback

Status: target architecture selected on 2026-10-07; migration and verification
are not complete. This decision supersedes mandatory host-transport rate
correction in the BMS player's browser playback contract. Existing generic
clock-calibration and presentation-estimator APIs remain reusable capabilities.

Current source at `394c2c0` connects this authority through shared Step/browser
owners and Linux, Windows and macOS solo/local startup and gameplay. Raw output
remains the scheduling grid; a distinct logical output timeline owns Runtime
normalization and normal-rate Transport. Original HOST input is correlated with
explicitly unknown accuracy. Held output publication and native pause/control
consume original physical evidence. Existing recordings retain their recorded
domain and wire interpretation. Development regressions and Rust/WASM type
checks pass; formal review, current CLI/desktop/browser QA and physical
SDK/hardware acceptance remain pending. This is implementation progress, not
migration or full Goal completion.

## Decision

The BMS player's play position shall be determined by the active audio output
timeline. Do not advance an independent host-based play clock and continuously
adjust its rate to follow audio. Use the same business timing authority for solo
and local-cohort play, native adapters and browser adapters.

Host timestamps remain necessary provenance for keyboard, touch and HID input.
Convert the original event occurrence time to the audio timeline through an
explicit clock-correlation boundary. Never substitute input processing time,
overwrite original acquisition provenance or equate unrelated clock domains.
Runtime normalization may carry the mapped point while retaining the acquired
point in `original_clock_point`, as the existing core already does. Host time
may also serve control deadlines, watchdogs and telemetry; it has no independent
authority to advance judged song position when audio is paused or unavailable.

Distinguish produced/mixed frames from output presentation. A rendered-frame
counter alone does not prove what the player has heard. Adapters supply actual
available output observations, their clock domains, origin/epoch and quality.
Browser `getOutputTimestamp()` reports an estimated output association; the
[Web Audio specification](https://www.w3.org/TR/webaudio/#dom-audiocontext-getoutputtimestamp)
does not supply a numerical error bound. Do not label it Exact or invent one.
Input correlation and display prediction can require estimation even when the
host-rate correction loop has been removed.

## Preserved invariants

- Audio scheduling uses the actual output grid and existing checked section,
  preroll, source/output origin and frame conversions.
- Accepted input, judgment history and already committed output commands are
  immutable. Live play and replay use the same judgment transitions; recording
  must preserve the evidence needed to reproduce accepted logical times.
- Pause, resume, seek and output replacement preserve explicit ownership and
  epochs. Observations from a retired backend cannot advance the new timeline.
- Missing, stale, repeated or regressing observations do not fabricate output
  progress. Output uncertainty, genuine discontinuity and technical failure
  require explicit policy and observable outcomes, rather than hidden resets.
- No wider drift threshold or swallowed estimator error stands in for this
  migration. Retaining the same host-rate loop with optional bad-sample skipping
  does not fulfill the selected architecture.

## Evidence and migration criteria

Browser QA at `979f668` reproduced `BaseRateOutOfBounds`: supplied output elapsed
1.296468 seconds while supplied host elapsed 1.308 seconds, about -8816ppm,
exceeding the existing 1000ppm limit. These observations establish a failure of
the former HOST/correction path; they do not distinguish physical drift from timestamp
estimation error. No performance or hardware superiority is inferred.

Before implementation, trace actual shared/native/browser progression, input
mapping, scheduling, capture/replay and lifecycle owners and freeze the concrete
port and test plan. Verify with deterministic clock traces, delayed input,
paused/absent output, estimated timestamp jumps, output epoch replacement and
record/replay comparisons. Then verify genuine native and current browser live
play. Existing passing HUD and regression tests do not close the failed browser
QA or complete the full player Goal.
