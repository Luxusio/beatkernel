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

The corrected rerun passed selected-media error retention, cached recovery and
accepted-library replacement, then failed the catalog ArrowDown journey.
Primary source inspection confirmed that the idle menu has no keyboard selection
bridge: the Window handler returns without an active play session. This is an
existing missing feature, not evidence of a new ownership regression. The
requested navigation remains an acceptance requirement; a portable-owner
selection adapter and real-key rerun are required. The failed evidence remains
under target/wf/player-cleanup-qa-browser-retry1. No page errors were observed;
one console resource 404 lacked a recorded URL and is not classified yet.
The runner now records HTTP error URLs/statuses for the next actual run.

The Window now maps catalog navigation keys to a token-correlated menu-select
request. The Worker uses the existing portable owner's select operation;
highlight movement does not activate or prepare a chart. Independent test
author execution passed seven pure-adapter cases and all 56 actual Worker
tests, including two new connected selection/admission tests. These are
development regressions; fresh formal review and real-browser acceptance
remain required for the changed JavaScript.

Fresh DEEP review and CLI delta QA returned PASS for 04719ef. The CLI lens ran
seven adapter tests, all 56 actual Worker tests and 18 independent cases using
the actual Window keyboard handler. Unchanged Rust, native binary and WASM
retain the earlier executed evidence; no foreign hardware claim follows.

Actual native Display QA and independent same-window UX returned scoped PASS.
They exercised edit/cancel, invalid-value recovery, Done versus separate Apply,
Back/reentry and 800x600/960x720 resize. Artifacts are local under
target/wf/player-cleanup-qa-desktop and target/wf/player-cleanup-ux-desktop.
No physical IME or isolated clipboard proof was available. The UX reviewer
retains small bitmap-label readability at fractional 800x600 scaling as backlog.

The next actual browser runner completed all ten steps, including trusted
catalog keyboard navigation and pending-page retirement, with PASS evidence
under target/wf/player-cleanup-qa-browser-retry2. Its HTTP diagnostic identifies
the resource 404 as favicon.ico; no page error occurred. Owned browser, Xvfb
and server were cleaned up. Independent browser QA inspected all nine captures.
Independent browser UX also inspected pending/error captures and interacted
with the actual catalog: ArrowUp changed selection and revision, and resize
exposed existing 390px-width overflow (document width 429px). Its scoped UX
verdict passed catalog/navigation/feedback; it did not independently replay
the whole loading journey.

The separate shared UX wrapper reached its 150-second hold deadline and records
FAIL for that deadline; this does not replace the original committed-runner
PASS. The invalid-rate follow-up was unexecuted after a Puppeteer chord syntax
error and CDP disconnection at the deadline. Its remaining polling continuation
was terminated as an owned Node process (exit 143) after browser/Xvfb/server
cleanup. No timeout result or cleanup failure was relabeled as PASS.

Remaining UX backlog includes narrow-screen overflow, long settings-heavy
flow, preparation feedback distant from the primary action, catalog wording
and no preparation Cancel action. Loading responsiveness/cancellation and
measurements remain in the queued loading task; this child claims no full UX
or performance completion.
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
