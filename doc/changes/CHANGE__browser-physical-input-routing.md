# Route browser keyboard input through the common physical API

The live page explicitly selects physical input and requires matching Worker
preparation metadata before transferring PCM. Replay stays on its recorded
operation path. Worker compatibility callers can retain the legacy keyboard
route; explicit physical setup refuses absent APIs without fallback or retry.

Encode a complete bounded keyboard batch on Worker before entering gameplay.
Keep original acquisition time, sequence and output scheduling. Historical key
IDs are browser adapter codes, so the new route uses Native controls in the
browser keyboard namespace rather than claiming those IDs are HID usages.
Source 1 denotes the Window keyboard aggregate, not a distinct hardware device.
Canonical events retain acquisition clock provenance; receipt time is irrelevant.

The same consuming gameplay owner preserves configured section endpoints,
capture, input watermarks, command ACKs and joined cleanup. Hardware touch/HID
adapters and their permission, contact and disconnect behavior remain pending.

Source and six independent deferred JS protocol groups are authored, plus a
matching actual Rust codec packet fixture. Helper/Worker/Window JS totals are
2/31/47; preview linker retains 14 groups and portable Rust input totals 7. Integration
also preserves no-note/all-Unbound playback: empty physical bindings are allowed
only for charts without prepared lanes and still validate input budgets. This
Rust refinement was formatted after both writers reported STOPPED. Workspace,
headless and both WASM feature cargo checks exited 0; three existing platform
cadence warnings remain on WASM. No parsing,
tests, browser/runtime or generated bindings are executed; Rust compilation is
not JS acceptance. The
Goal remains active; required independent reviews and QA must precede close.
