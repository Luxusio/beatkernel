# Explicit BMS button/contact input mode

The browser main thread acquires keyboard, touch/pointer and HID events;
continuous gameplay, judging and rendering belong to Worker, and audio to
Worklet. Browser permissions, user gestures and lifecycle still require Window
participation. Input adapters preserve original acquisition timestamps and
physical source/contact identity.

This implementation slice connects the common contact press evaluators to an
explicit BMS input mode. The default remains button-only. Contact mode has a
distinct replay setup version and rules identity, so typed replay reconstruction
can restore exactly the same judging rules and finite section metadata.
Legacy tuple decoders reject metadata they cannot represent.

Actual browser touch acquisition/lane routing and WebHID permission/report
handling remain subsequent work. A physical packet ingestion API alone does
not establish hardware playability.

Record catalogs and legacy offline/native replay entrypoints still use tuple
setup decoders. They intentionally refuse contact metadata until explicitly
migrated to typed mode-aware ownership. Touch coordinate-to-lane routing also
needs a contact-aware binding component: mapping one surface to every lane
would fan out each contact and cannot stand in for spatial routing.

Six portable fixture groups are authored for actual adapter rules, canonical
metadata and budgets, legacy rejection, live capture and StepReplay/Mixer PCM,
contact cancellation/provenance and competition identity. They have not run.
Rust cargo check completed for workspace all-targets, headless all-targets,
WASM browser and WASM browser-audio. All four exited 0; WASM retains the
existing three cadence dead-code warnings. Compilation includes the authored
portable fixtures but does not execute their assertions.
Tests and browser/device execution remain deferred. No runtime acceptance,
performance measurement or Harness completion is claimed.
