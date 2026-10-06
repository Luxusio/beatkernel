# Share room progress publication timing

Native and browser room progress publication connect to the common
Rust ProgressCadence policy. A scalar due query lets Worker skip progress DTO
construction when transmission is suppressed. Actual publication still validates
phase, original roster, counters and one-shot final state before admission.

The 50 ms interval applies to ordinary updates; final updates bypass interval
suppression while retaining chronology and protocol checks. The marker records
successful queue admission, not WebTransport write completion or peer receipt.
Old generic progress publication keeps its post-effect clock behavior and legacy
room publication APIs remain compatible.

## Evidence and known ceiling

Both paired writers returned terminal stop reports. Eleven Rust and five JS
fixture groups are authored: common cadence, actual committed driver, injected
native controller and actual Owner/Worker paths. Driver tests keep publication
and write observations at or after the real fixture handshake and preserve exact
relative interval boundaries. A test-only pub(super) helper avoids duplicating
the existing protocol setup; its body/assertions are unchanged. Native eligibility,
including pending publication, remains before clock access. Only eligible cadence
observations contribute to regression checks. No new transport or timer authority
is introduced. Actual async
completion remains in split write/receipt operations. Browser RoomCompetition/
RoomNetworkActor composition and non-room cadence remain outside this increment.
Scoped formatting and whitespace checks completed. After both terminal writer
reports, four sequential compile-only checks exited zero: workspace/all-targets
with WebTransport, no-default-features WebTransport/all-targets, WASM browser/
library and WASM browser-audio/library. Host checks compile Rust fixture children;
WASM library checks compile production binding paths without test children.
Existing unused-code warnings remain. No assertions or JS scripts were executed.
Assertions, JS parsing, runtime, formal review and required QA remain deferred.
Real network delivery, allocator/performance measurements and platform acceptance
remain unproven. Full BMS player Goal stays active.
