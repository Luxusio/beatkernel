# Shared HID report profile decoder

Explicit bounded device profiles translate separate-ID raw reports into genuine
physical button/axis events through the existing DeviceAdapter/AdapterRegistry
boundary. Native and browser acquisition can configure the same decoder rather
than duplicate report interpretation per OS. Profiles preserve bit order, signed
values, axis units and original acquisition metadata.

Whole selected reports validate before state/output adoption. Buttons and
absolute axes retain levels; relative axes publish every report. Fixed-capacity
state/scratch avoids decoder storage allocation during report interpretation.
Unknown report IDs do nothing, malformed typed decoding returns refusal details,
and reset never fabricates judge releases.

Five portable fixture groups are authored: both bit orders/full widths,
button/absolute/relative state, setup and atomic refusals, actual per-device
registry ownership, and actual typed HID controls through Runtime/Judge/Mixer.
Workspace all-targets, headless all-targets, WASM browser and WASM browser-audio
cargo check each exited 0 after both writers stopped. WASM retains the existing
three cadence dead-code warnings. Scoped formatting/whitespace are complete;
the parent module retains only its export change. Assertions were not executed. Actual WebHID page/Worker forwarding,
profile selection, logical lane binding and native owner configuration remain
required integration. Tests, driver/device/browser execution, generated bindings
and formal review/QA are deferred. No playable HID or performance acceptance
is established by this component.
