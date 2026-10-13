# Browser native frame diagnostic

Task: TASK__browser-native-frame-diagnostic. Run: 01a13503-e427-7c1d-a687-015c2839e541.

The earlier real strict browser chronology failure remains unresolved. This
change adds checked diagnostic arithmetic and an opt-in native phase0 fixture
to evaluate an independent sample-grid origin. Production Host, Worklet and
Rust audio-clock logic remain unchanged. This is not a player completion or WBS
promotion.

## Change and proof boundaries

The three pure helpers select an unambiguous fractional event time, validate a
full native interior zero-to-one edge for a known actual onset, and add actual
callback extents with checked safe-integer arithmetic. Spatial shape cannot
authenticate a scheduled onset after native late clamping. Tests preserve the
44100/1664 and48000/896 one-sample double-rounding counterexamples,20-hour/week
positions, every interior edge over variable extents, invalid shapes/metadata,
unchanged inputs and overflow refusal.

The existing browser runner now offers WORKLET_NATIVE_FRAME_PROBE=1. It uses
the actual generated WASM, AudioHost.open and unchanged production phase0
process. A test-server wrapper adds one native a-rate parameter and transfers a
separate probe channel. The first anchor requires a fresh default0 timeline
with exactly one first event; a later independent edge must match every actual
callback extent. Raw global currentFrame remains separate. The fixture does
not finish or arm a Mixer and must observe zero PCM and unavailable reports.
Default and required-cold chronology verification bodies remain intact.

Native-mode serving is limited to the owned document and resolved app/web tree.
Requests, callback observations, deadlines and private ports are bounded and
retired. No dependency, driver, service, product control or persisted schema was
added.

## Author verification

- AC-001 independent test author:18/18 PASS; ROOT retest:18/18 PASS.
- Existing Host/Worklet protocol checks:67/67 PASS.
- Candidate combined Node checks:85/85 PASS,0 failed/skipped/cancelled.
- Helper and fixture node --check PASS.
- Initial no-flag protocol command failed VM imports before behavior. The
  corrected --experimental-vm-modules command passed; this flag was already
  documented in app/web/README.md and is not a new product defect or tool setup.
- First actual native author run:15 checks PASS (eight independent positive
  cases, constant/index0/late/reset/identity/timeout/stop); then the suspend
  scenario exceeded Puppeteer's180-second protocol deadline. Browser/server
  cleanup completed with browser exit0. This is an author fixture FAIL, not a
  full native PASS. The fixture had expected resume on a Host that terminates
  on suspension; the correction must observe that existing lifecycle and bound
  every native await.
- Corrected actual native author run:19/19 checks PASS, exit0, no fatal/page
  errors. Requested and applied44100/48000 matched; measured callback extent was
  128 throughout. Variable extents remain unit-test evidence, not actual native
  observations. All eight positive cases agreed at their second independent
  edge. Callback-limit retirement preserved exactly1024 observations.
- Actual suspend evidence recorded running→suspended→closed, Host failure
  code=state/operation=context and invalidated pending private observation.
  It does not demonstrate same-context resumption or automatic diagnostic-port
  retirement: that port was explicitly retired before the native suspend.
- The first run's native-44100-63 interval recorded ordinal88 rawGlobalFrame13312
  versus extentCursor13440, with first base11520 and independently agreeing
  second base20864. This is a real raw-clock discrepancy within a measured
  interval. It does not trace the lock collision or solve future callback gaps.
- Mechanical source comparison against a7f0d743 confirmed the legacy bootstrap,
  runCase and verify bodies are identical.

Logs: target/wf/browser-native-frame-diagnostic/author/. Native author evidence
is retained under author-native-c1/evidence.json and author-native-c2/evidence.json.
Derived raw observations are in author/raw-observations-c1.json. Generated artifacts are ignored;
the reusable command and interpretation are in the committed
[platform guide](../platform/GUIDE__browser-native-frame-diagnostic.md).

## Native-source interpretation

Pinned Chromium can fail to publish a new Worklet global frame after a graph
try-lock miss. Native parameter automation uses destination sample positions
instead. The prior real PROCESS status6/current161408/expected161536/block128
failure is observed; the specific lock collision is an inference, not an
instrumented fact. Native graph-pull-disabled advancement and conditional
interruption/restart before queued state notification prevent claiming that a
local callback counter is universally authoritative. The guide links primary
sources and distinguishes the conditional source path from a reproduction.

Tested browser: Chrome153.0.8010.12. Production Worklet SHA256:
5263660c246739e6e1daf29c4aa2ea4b8d9e338cdc75d71e4a654dbad15266c8.
Reused WASM SHA256:
34c0bda380ad673c29298e0cec0873009cdfe274772d55904537dff8acef0b18.
Both author runs closed their owned browser and HTTPS server; browser exit0,
PIDs701006/726021 absent on final inspection. Completed author cases closed
their own contexts and private endpoints. The timed-out first suspend did not
return a per-context result; browser-level closure is the evidence for that
failed case, not a fabricated suspend PASS.

## Known ceiling

Known ceiling: markerFrame은 알려진 실제 onset만 처리하며 예정 시각의 정확성을 인증하지 않음 — upgrade when 호출자가 실제 native onset 보장을 증명할 때

Known ceiling: native 마커는 관측한 두 지점 사이만 검증한다 — 제품 시계 변경 전 실제 콜백 누락과 lifecycle 근거가 추가되어야 한다

Known ceiling: suspend 직전에 진단 포트를 명시적으로 retire한다 — 자동 suspend retirement를 제품에 도입할 때 별도 검증해야 한다

Known ceiling: 재사용 WASM의 phase0만 실행한다 — 실제 Rust 재빌드와 플레이 검증 시 확장해야 한다

Known ceiling: suspend는 실제 Host terminal·닫힘을 검증하며 같은 Context 재개를 요구하지 않는다 — 제품의 suspend/resume 정책을 변경할 때 별도 검증해야 한다

## Independent review and QA

Formal code review completed PASS with0 findings at b30bfc60. Review depth was
DEEP with both correctness and contract/test hunters; both returned empty
candidate arrays. The reviewer independently checked the complete five-path
scope, native scheduling semantics, actual evidence, default acceptance and
cleanup. A surplus GUIDE EOF blank was removed before the final clean diff
check; it changed no source or evidence content.

The later independent qa-cli and qa-browser finals both returned PASS:

- CLI executed85 focused tests and3329 independent direct helper assertions,
  including BigInt origins, long durations, precision/overflow/invalid-input
  refusal and input conservation. Syntax and complete base-to-HEAD diff checks
  passed. It independently recalculated the fresh browser artifact rather than
  starting a competing browser.
- Browser executed19/19 checks across17 actual graph cases. Independent raw
  validation passed310 assertions over2451 actual callbacks. All eight marker
  pairs agreed, measured extents were128, production remained phase0 with zero
  PCM and unavailable reports, and expected negative/lifecycle cases passed.
- Same-page capture recorded18 screenshots at800×600, DOM and network state.
  Browser depth was render-only-browser because no product UI changed; native
  parameter/port/stop/suspend interactions were executed. Page errors were0;
  four expected404 console messages came from deliberate server-boundary
  requests and the fixture's absent favicon.
- This fresh run independently recorded native-44100-17, generation12:
  first base11136/ordinal77, second base20480/ordinal150. The73 actual128-frame
  callbacks agreed with both edges. Ordinal91 reported rawGlobalFrame12800
  while the independently based cursor was12928. This corroborates raw-clock
  discrepancy in another measured interval, not the exact cause of the old
  strict gameplay failure.
- All owned contexts/pages/browser/server closed; browser exit0, PID803710
  absent and8162 local rebind succeeded then closed. Actual kernel LISTEN was
  absent and HTTPS returned EOF. This environment's bare connect_ex also
  succeeds on unused control ports, so it was rejected as a leak oracle.

Browser artifacts/transcript/raw checks and18PNG captures:
target/wf/browser-native-frame-diagnostic/qa-browser-b30/.
CLI commands/logs/recalculation:
target/wf/browser-native-frame-diagnostic/qa-cli-b30/.
Both validate the same production Worklet/WASM hashes stated above.

Failed QA tooling attempts remain in those directories: the first capture
preload tried assigning a read-only Puppeteer ESM namespace before any browser
started; the corrected QA require wrapper captured actual pages. Auxiliary
raw validators also corrected absent-field/field-name assumptions and the bare
TCP-connect oracle. These are preserved QA-tool errors, not hidden product
failures; tracked source changed0 during QA.

Reusable learning: When checking owned listener cleanup, verify normal close,
owned PID disappearance and actual kernel listener absence; use a closed local
rebind or protocol response to distinguish a transport proxy from the owned
server. Bare TCP-connect success alone is insufficient in this environment.
This rule is captured here; manifest browser-support cleanup and repeating-ramp
clock research remain separate parent work, not extra changes in this task.

## Remaining work

Independent code review actually preceded both QA runs. Final post-QA document
review and hook-owned ordered receipt verification remain; narrative results
cannot authorize task_close. The full BMS-player Goal stays active. Original browser FAIL,
per-callback native authority, lifecycle correctness, physical/presentation
timing, replay/input integrity and platform-device acceptance remain parent
obligations.
