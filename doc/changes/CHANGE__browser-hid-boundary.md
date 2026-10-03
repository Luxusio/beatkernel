# Bounded WebHID acquisition and canonical reports

An optional native WebHID owner acquires exact input reports and manages
bounded authorized interfaces, browser permission gestures and asynchronous
opening/cleanup. Acquisition preserves original browser event time, full
adapter source identity and the host's shared sequence. It copies only the
actual DataView slice and retains the separate report ID.

The canonical raw-report encoder keeps that provenance and exact payload,
including an ID-like first data byte. Report ID zero becomes the core
unnumbered representation. No button/keyboard event or lane binding is
inferred from raw reports. The original
[WebHID specification](https://wicg.github.io/webhid/) defines this framing.

Seven new fixture groups are authored: three acquisition-owner groups, two
canonical JavaScript helper groups and two portable Rust codec/framing groups.
Workspace all-targets, headless all-targets, WASM browser and WASM browser-audio
cargo check each exited 0 after both writers stopped. WASM retains the existing
three cadence dead-code warnings. Rust assertions and JavaScript fixtures were
not executed; JavaScript parsing was not run. Page/Worker forwarding, descriptor/profile
interpretation, gameplay permission controls and disconnect stop behavior
remain required integration. Tests, generated bindings, browser/device execution
and formal review/QA are deferred; no runtime acceptance is claimed.

Permission filter snapshots preserve unsigned 32-bit vendorId and unsigned
16-bit product/usage fields with their required dependencies. Empty filter lists
are permitted; individual empty dictionaries are refused. Native setup/cleanup
promises have no cancellation contract here, so terminal cleanup awaits them
without inventing a timeout-based ownership release.
