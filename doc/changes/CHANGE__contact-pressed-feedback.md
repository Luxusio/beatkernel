# Common contact pressed feedback

The existing bounded pressed-lane component gains genuine contact ownership,
shared by live browser/native report presentation and recorded replay visuals.
Contact Down adds an idempotent owner and matching Up/Cancel releases it. Move
does not acquire or relocate a lane. Full source, physical surface, game control
and contact ID distinguish owners, with button identity kept independent.

The lane mask stays set until the last matching button or contact owner releases.
Actual admitted bound inputs supply the destination, without fake keyboard
events, coordinate hit testing or hit-grade dependence. Existing atomic batch
capacity refusal, retained scratch and single-event fast path remain required.

Four portable fixture groups are authored: full owner identity and sharing,
mixed 4096-owner atomic refusal, actual routed gameplay report feedback, and
typed captured ReplayVisual prefix parity. Both writers stopped before scoped
formatting and compilation. Workspace all-targets, headless all-targets, WASM
browser and WASM browser-audio cargo check each exited 0; WASM retains the
existing three cadence dead-code warnings. These checks compile Rust fixtures
without running assertions. Tests, generated bindings,
browser/device execution, formal review and QA remain deferred; no runtime
acceptance or measured performance is claimed. WebHID and full native/contact
record compatibility remain separate work under the active player Goal.
