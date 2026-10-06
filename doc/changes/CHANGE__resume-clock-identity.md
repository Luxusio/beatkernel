# Keep the current output clock identity across gameplay resume

Common solo and local-cohort resume previously reconstructed the presentation
owner with default discipline settings and epoch zero. Stage reconstruction
through the business presentation port using the current owner's configuration
and epoch, then seed from original native evidence before replacing that owner.
Same-stream resume does not consume a new epoch; max-u64 remains representable.
Custom epoch-capable owners must explicitly support identity restoration.

## Evidence

Implementation and six independent tests are authored: four port cases and one
actual pure-memory solo/cohort case each, reusing the original core-port fixture
owners. Only child registrations were appended to those existing fixture files;
their headers and assertions are preserved. Actual loops stage the configured
owner and keep the same epoch before original native seeding and replacement.
Scoped Rustfmt and whitespace checks completed. Four sequential compile-only
checks exited zero: workspace/all-targets WebTransport, headless runtime
all-targets WebTransport, WASM browser lib and WASM browser-audio lib.
The first workspace check found four invalid default replay-limit constructors
in new tests; the author replaced them with the existing validated native
fixture limits before the successful retry. Existing unused-code warnings
remain. These checks do not execute the six tests or platform devices.

## Known ceiling

Earlier transport resume is not rolled back on later refusal.
Device-seeding failure, whole-transport rollback, and physical-device acceptance
remain unverified. Assertions, runtime, formal review and QA remain deferred.
Backend/buffer live handoff, physical output fences and new first-output anchors
remain pending. Full BMS player Goal stays active.
