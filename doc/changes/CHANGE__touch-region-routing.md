# Contact-owned spatial input bindings

The shared input layer gains a bounded touch-region router that chooses a game
control on contact Down and retains it through Move, Up and Cancel. Events keep
their actual source, surface, contact, acquisition clock, coordinates, pressure
and provenance. No keyboard event is fabricated.

Regions use finite half-open rectangles with exact-device overrides. Down
outside all regions remains unbound until release; duplicate Down cannot choose
a different lane. Contact limits fail explicitly without dropping existing
owners, and routing storage is reserved during setup.

An optional projected hit position permits Worker presentation coordinates to
select a region while the event retains its original acquisition coordinates.
Both samples are validated without mutating routing state on failure.

The component supports subsequent common Runtime and browser owner integration.
That integration still needs chronology/end validation, partial report handling,
contact restoration, presentation feedback and browser coordinate acquisition.
This change alone does not enable browser touch play or WebHID devices.

Six independent fixture groups are authored for region/device rules, contact
locking, identity/release, atomic failures and provenance, actual common
JudgeEngine/ReplaySession behavior, and ordinary BindingMap compatibility.
Tests and browser/device execution remain deferred, with no runtime acceptance
or measured performance claim.

Rust cargo check completed for workspace all-targets, headless all-targets,
WASM browser and WASM browser-audio. All four exited 0; WASM retains the
existing three cadence dead-code warnings. The checks compile fixture code
without executing assertions. Scoped formatting and whitespace checks completed.
