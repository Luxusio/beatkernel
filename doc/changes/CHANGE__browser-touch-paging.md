# Browser local touch paging

The browser Window acquires keyboard, touch, HID and Gamepad input. Mapping,
judging and OffscreenCanvas rendering remain in the gameplay Worker. Local page
selection is an event-driven control operation, not a Window render loop.

Touch no longer locks local play to its initial page. The common TouchRouter can
atomically replace rectangle bounds while retaining the ordered device, surface
and destination identities. Existing bound and unbound contacts retain their
first-Down ownership until an actual release or cancellation. Hidden touch
members keep their existing contacts and acquire fresh contacts as unbound;
returning to a visible page does not bind contacts that are already held.

The Window waits for the genuinely acknowledged input prefix before requesting
a page change. It ignores fresh touch Downs during that transition and continues
acquiring existing contacts' moves and releases. Sequence gaps from filtered
sources do not create an acknowledgement dependency. Cancellation rejects the
pending transition. The Worker applies the actual native touch layout before
publishing the new page and checks its returned visibility against the frozen
roster. Choice errors preserve the previous page; malformed protocol responses
fence the session.

The layout uses the same local field geometry and reserved comparison space as
rendering. Replay capture stores resolved game inputs with original physical
payloads, so replay does not hit-test those inputs against a later layout.
Page changes preserve runtime clocks, judgement, audio, sequence watermarks and
capture ownership.

Deferred fixtures cover core contact retention and atomic invalid remaps, actual
local gameplay with captured replay, Worker page transactions, and Window input
prefix ordering and cancellation. They are authored for later execution under
the user's verification deferral. Cargo check establishes compilation only;
browser execution, generated WASM bindings, device validation, formal review,
QA and task acceptance remain pending. Local network competition integration
remains a separate unfinished milestone.

Source evidence: seven deferred groups were added (core router two, portable
local runtime/replay two, Worker one and Window two). Scoped Rust formatting and
whitespace checks completed. The workspace all-targets check, headless runtime
all-targets check, browser WASM library check and browser-audio WASM library
check each completed with exit code zero. The WASM configurations retain three
existing platform cadence dead-code warnings. These checks do not execute the
fixtures or establish browser/device behavior or acceptance.
