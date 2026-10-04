# IME in all existing editable desktop fields

Native IME uses the same selected field and screen instance as ordinary text
editing. Search, settings values/profile, all four display fields, practice
start/end and the record directory share the admission and lifecycle rules.
Composition suppresses ordinary text and shortcuts. A selected field or screen
change, loss of UI admission or native disable clears the preview and requires
fresh enable acknowledgement before accepting subsequent input.

Preedit borrows a visual clone of the base editor with native byte decorations;
it does not update options, search results, persisted settings or record paths.
Commit inserts into the base editor transactionally through the shared editor
publication path. Invalid text preserves the committed draft and reports the
error on that screen. Retained frames display the admitted preview, and input
font preparation admits its characters at the existing UI state boundary.
Shared record-directory publication also invalidates its old catalog, selected
record and preview when the committed path changes, matching ordinary typing.
Preedit/cancellation and cursor-only publication retain the old catalog; this
shared behavior also applies to clipboard completion without additional I/O.

Five deferred groups exercise all seven newly admitted field targets through
actual Desktop event methods, retained preview models, atomic refusal and
shortcut suppression, screen/field/readiness/pending-operation guards, cached
font preparation and record metadata invalidation. They acquire no native
window, clipboard, device or playback owner.

After both writers returned terminal STOPPED, scoped formatting and whitespace
checks succeeded. Rust 1.98.1 compile-only checks completed for workspace/all
targets with WebTransport, headless/all targets with WebTransport, WASM browser
and WASM browser-audio. New fixture bodies compiled in the first configuration
without execution. Existing three WASM native-cadence warnings remain. No
generated bindings, apps or runtime tests were executed.

## Known ceiling

Native events have no composition generation ID; current-target and ordered
enable/disable guards cannot prove rejection of every delayed native event.
Candidate popup positioning, shaping and font fallback remain separate work.
Numeric display/practice editors can preview and retain Unicode text; their
existing Apply parser still rejects unsupported values. Actual native IME,
rendered pixels and platform input behavior remain unverified. Runtime tests,
formal review and QA remain user-deferred; the full player goal stays open.
