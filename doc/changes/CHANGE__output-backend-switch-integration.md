# Integrate the static output backend switch into the current app architecture

Integrate the useful wf/output-switch work (6f6faa8) into the current checkout,
placing its canonical implementation/fixtures under gameplay/output/adapters.
The root output_backend_switch module only re-exports the public static types.
No new crate or duplicated controller/policy is added; the old branch is retained.

OutputBackendSwitch<A,B> keeps two statically injected adapters with the same
presentation port type. The request variant selects the opening side; operations
on an owned output follow that output's variant. Native pause/end overrides are
forwarded, retaining original backend observation evidence. Switched forwards
stopped Mixer recovery and preserves the original side error as its error source.
Open failure tagging retains recovered Mixer or partial owner and separate cleanup
error; there is no silent switch/fallback to the other side.

Existing memory-Mixer/controller fixtures cover a round trip and monotonic epochs,
resumed queued PCM, requested-side refusal without fallback, pending owner cleanup
and explicit retirement recovery, and current-side retirement refusal. The common
owner/controller implementations are unchanged. Static dispatch adds no dyn
backend boxing or per-note allocation; this is not a measured zero-CPU/performance
claim. Both sides currently share the same PCM format and grid.

This is the reusable kernel/app composition layer for future WASAPI/ASIO wiring.
Actual Windows selection/UI composition, format conversion and SDK/hardware
acceptance remain unfinished; current device support is not overstated. The broad
Harness task and full Goal remain open.

Verification (2026-10-06):

- Focused switch tests: 4 passed, 0 failed.
- Independent review-code PASS (DEEP bounded formal-only) and review-security
  PASS, no findings in the new adapter/fixture/port export and REQ scope.
- Independent qa-cli PASS for this integration: complete runtime library
  1572 passed, 0 failed, exit 0; workspace all-target webtransport check exit 0;
  WASM browser library check exit 0. Existing warnings remain.

These are scoped structural/portable-owner proofs. They do not complete the
Windows UI/backend composition, sample conversion or full task/Goal acceptance.
No full task close or coordinator-authored receipt is claimed.
