# Browser HID gameplay setup and ingestion

The Rust browser owner gains pre-play HID device/profile setup and canonical
raw-report ingestion. Numeric setup preserves exact source and physical control
identities and validates every profile against existing constructor bindings.
The shared native/browser profile decoder and acquisition-order check provide
report interpretation without a separate OS or browser parser.

Complete report validation precedes ordered typed event admission through the
existing StepGameplay reports, pressed ownership, keysounds and capture. Valid
reports without emitted transitions still enter as original unbound raw input
so runtime chronology remains observed. Gameplay failures keep the committed
prefix and fence further processing, while retained scratch is restored.

Implementation and five independently authored portable fixture groups are
present. Workspace/all-targets, headless runtime/all-targets, browser WASM and
browser-audio WASM cargo checks each completed with exit code zero. The WASM
checks retain the existing three unused cadence diagnostics. The checks compile
fixture sources without executing their assertions. Actual Window/Worker
forwarding, permission/profile UI and native owner configuration remain pending.
Tests, generated bindings, browser/device execution, formal review and QA are
deferred; this API alone does not prove playable HID or measured performance.
