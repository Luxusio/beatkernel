# Separate browser renderer Worker

Status: implementation in progress; no integration acceptance claimed.

The selected implementation separates GPU/Scene ownership from gameplay and
audio through a direct bounded Worker channel. Cold visual registration retains
complete chart/image data without PCM or gameplay owners. Committed scalar and
COW progress changes use one fully acknowledged snapshot baseline. All current
preview/live/local/replay/history/completed/room modes remain required.

The terminal graphics policy preserves existing behavior: explicit failed stop
with genuine recorded prefixes and joined cleanup. Stalls and surface retries
continue input/audio service. Static presentation failures preserve stored
authority. The integration must repair premature game termination while room
cleanup still owns capture delivery. Headless continuation is unselected.

Original touch geometry and acquisitions remain intact through page/resize
barriers. Atomic state acknowledgement differs from geometry submission.
Concrete byte/count/peak-memory bounds and actual two-Worker QA remain required.
Existing audio startup and late-touch failures are unresolved and unwaived;
this task does not change clocks, timestamps, delivery policy or record formats.

The first implemented primitives expose typed visual chart/image export/import
and atomic cumulative note-page import. Image registration retains original
source IDs, including hidden crop dependencies and unavailable sources, so
source/display identity budgets match preparation. Renderer-local progress
validates page shape, counts, padding, hold states and monotonic state/scalar
updates before publication; unchanged pages preserve COW identity.

Focused development verification passes 11 chart/image transfer tests, all 22
note-progress tests (including nine new importer tests), and three existing
image-assets regression tests. The original crop-only budget test first failed
and now passes with its assertion unchanged. These checks do not establish
actual separate Worker integration; snapshot models, bindings, channel and
lifecycle integration, final independent review and browser QA remain pending.

## Known ceiling

Known ceiling: Window integration and real completed/combined rendering proof remain pending AC007/AC009.

Window integration now passes the 133-test host suite; the completed/combined
whole-application browser portion of this ceiling remains open.

Known ceiling: 실제 두 Worker의 전체 모드·GPU 동작은 미검증 — AC009 브라우저 검증에서 확인해야 함.

Known ceiling: 실제 WASM getter 및 전체 화면 동작 검증 미완료 — AC009 브라우저 검증에서 확인.

The getter portion of this reported ceiling is now resolved by the actual
HEAD `8e47047` development probe. Generated WASM returned preview page zero,
room page zero then one, and genuine five-member local pages one then zero.
Actual renderer submissions carried 640×480 and restored 960×720 extents;
zero extent emitted no geometry success. Genuine 64-member frames submitted
pages fifteen then zero, with the last-page GPU screenshot showing P61–P64.
Whole-application and genuine completed/combined Results acceptance remains
AC009; this probe is not a formal browser QA verdict.

Known ceiling: actual Worker decoding must use the trusted caller’s finite diagnostic budget before allocating or copying — already-owned Rust convenience paths preserve diagnostics.

Portable frame models and display-only frozen history/results/room reconstruction
are implementation candidates. They do not construct gameplay completion,
judgment or gauge owners from visual packets. Explicit image and diagnostic
admission plus checked cold/frame/retained-storage accounting precede the actual
Worker transport. Eight independent model tests now pass, including whole-frame
rejection, the 64-player roster, genuine completion exports, 4,096 historical
grade identities, room pages and explicit UTF-8 diagnostic budgets. Existing
Results and historical regressions pass 33 and 23 tests respectively. These are
development checks; actual Worker integration, independent final review and
browser acceptance remain pending.
The current browser WASM library check also passes, with existing dead-code
warnings. It proves target compatibility rather than runtime Worker isolation.

The WASM visual-only adapters, shared checked byte codec and direct channel/
renderer owner are implemented candidates. The codec preserves six packet kinds
and full integer widths; a shared public progress validator performs only
self-reported-data checks, without changing room or completion authority.
Nine native codec tests pass, including exact aggregate UTF-8 diagnostic budgets
for images, nested frame room errors and every frozen room page. Nineteen actual
JavaScript owner tests pass; the complete web suite passes 559 tests. Shared
native score-panel and gauge-related regressions pass 14 and 68 tests respectively
(these filtered counts overlap). Current WASM check, build and binding generation
succeed with existing warnings.

An actual development probe used separate sender/renderer Workers and WASM
instances with a direct channel and real GPU submission for preview, live,
one-member local, replay, archived history and cancelled room presentation.
It retained original signed preroll state, distinguished state/submission ACKs,
and refused malformed or zero-extent submission success. This probe found a
missing room diagnostic admission check; rebuilt actual WASM now rejects budgets
3, 27 and 53 for two 27-byte room-page errors, accepts exact aggregate 54, and
leaves state/generation floor unchanged after refusal. A genuine local frame with
diagnostic-free waiting room HUD also imports at allowance zero and accepts its
exact state ACK. No actual completed-owner RESULTS
or same-generation combined RESULTS/ROOM runtime acceptance is claimed yet.

Window startup now creates distinct gameplay and renderer Workers with a direct
MessageChannel and renderer-only canvas transfer. Joined capture/audio/input
cleanup precedes disposal and termination, while stale presentation publication
is fenced immediately. The host suite passes all 133 tests. This is an
implemented candidate; final independent review and whole-flow browser QA
remain required.

The ordered local-touch helper retains acquisition page and projected position
on the same bounded input entry. Actual audio-authorized dispatch applies that
page to the relevant touch source before routing; held and unbound contacts
retain ownership. Unchanged pages return before allocating a replacement mapping.
Focused native verification passes all ten browser-local-input fixtures and all
22 input-merger/attachment tests. JavaScript forwarding and Window acquisition
integration remain pending; these checks do not establish browser acceptance.

Gameplay now publishes visual state through the renderer channel without owning
BrowserView, GPU drawing or a render animation loop. Local touch batches validate
and forward their original acquisition page before mutation. Preview retirement
is limited to the discarded presentation owner, and fatal errors cancel retained
room finalization while delaying global shutdown until capture delivery.
Seven focused Worker/protocol suites pass 192 tests, with no failures or skips;
WASM check and build pass with existing warnings. These are development checks,
not final independent review or whole-application browser QA.
