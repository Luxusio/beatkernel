# Player cleanup and demand-loaded browser catalog

Browser imports now admit a bounded canonical metadata inventory and publish
the chart catalog before File byte acquisition. Preview, live/local/section,
policy and replay preparation acquire the chart and its actual seeded resource
plan. Context retirement fences late reads; genuine play/settings reservations
permanently invalidate older preview and record requests. Runtime indexes its
existing immutable sound Vec without another index. Production native devices
and output imports use canonical ports while preserving external legacy
compatibility. Retained layout painters borrow one snapshot instead of cloning
per node, and native Display draft/view share an existing PanelScope lifetime.

## Development evidence

- Kernel existing Runtime/input/fence/stop: 23 passed; new indexed fanout,
  actual hold/custom stages, failures, replacement and 100k-binding fixtures:
  8 passed. Initial large-case fixture omitted the required gameplay fence;
  the fixture now performs that genuine transition before testing Stop.
- Portable file inventory: 10 passed; actual seeded/replay resource planner:
  8 passed; public canonical and legacy native device integration: 4 passed.
- Retained layout: 9 passed; motion regression filter: 54 passed.
- Native Display owner: 6 passed; clipboard: 15 passed; IME filter: 33 passed.
  Filters overlap and are not a unique total. A Practice IME assertion now
  compares the complete geometry identity/epoch pair used by the renderer;
  cold Scene replacement does not promise a globally increasing numeric epoch.
  Related owning code is unchanged from the baseline; baseline execution of
  that particular failing assertion was not performed.
- Actual Worker source in Node: 54 passed. Includes four transient-owner ABA
  regressions and genuine pending-settings admission after metadata-only import.
  Generated menu bindings in that development run used the prior WASM; current
  WASM compilation and connected browser acceptance remain separate evidence.
- Changed Rust files passed scoped rustfmt with skip_children=true and
  reorder_modules=false. WBS remains 90/193; no completion status was promoted.

The development evidence above is distinct from the independent results below.

## Independent verification checkpoint

DEEP code review and CLI QA returned PASS for source revision a8cec5f. CLI QA
executed the indexed-runtime, inventory/planner, canonical-device, retained UI,
Display, clipboard/IME and actual Worker-source tests. It also exercised the
public binary's help, genuine BMS/WAV render through a Unicode-and-space path,
invalid inputs and existing-output refusal. Logs are local ignored artifacts
under target/wf/player-cleanup-qa-cli; test filters overlap, so their counts
are not a unique total.

Linux native all-target Rust checking passed with desktop,webtransport. Windows
and macOS equivalent all-target checks also passed; those two use C stubs and
prove Rust types, not native SDK linking or device execution. Current WASM built
successfully; its SHA256 is
6904a82bf069d4e059c8b926aec6827eeebd949c4e1ff9c0953e41d415b71d8c.

The first independent browser run genuinely demonstrated metadata-only catalog,
selected BMS/WAV preparation, delayed acquisition, Window keyboard-event
delivery and recovery with rendered preview captures. It then returned FAIL
because the verification fixture expected an error CSS class instead of the
production data-error attribute. The real selected-media error and retained
preview were delivered. The fixture correction preserves production behavior
and also collects console errors. A fresh review and meaningful browser rerun
are required; the failing evidence remains in target/wf/player-cleanup-qa-browser.
Owned browser, Xvfb and HTTP resources were cleaned up.

Full browser/native journeys and independent live UX are not yet complete.
Ordered hook attestation and task closure are not claimed. WBS remains 90/193.

## Known ceiling

Cold setup sorting may allocate. Runtime reports and the complete judgment
pipeline are not proved allocation-free. Retained model publication clones the
memo value once and allocates an Rc; geometry/motion painting adds no deep model
clone. The isolated painter fixture proves model-storage borrow release, while
normal compose/relayout painters stay pure and retain outer packet borrows.

The new planner and existing preparation currently repeat chart parsing and
compilation. Loaded File bytes are reused within one library, but decoded PCM
and image/movie preparation are repeated. Selected media acquisition remains
serial. These are confirmed remaining work, with no measured latency impact
claimed yet. Unabortable retired reads may retain their JS context/File until
settlement while all WASM access is fenced. The queued loading-latency task
retains cold/warm first usable frame, preparation/reload measurements, reuse and
responsiveness work; broader allocation/screen ownership cleanup is also queued.

No instantaneous loading, hardware latency, physical input/IME, foreign OS
execution, universally zero-cost abstraction or whole-player completion claim
follows from these functional tests.
