# Native browser sample-phase evidence

The existing owned Chromium fixture now has an opt-in finite native ramp probe.
It validates every active sample in the Worklet, preserves a fresh first-marker
origin, checks interior resets and a conditioned closing endpoint, and refuses
the final held value. This prepares the browser clock repair; production
Host/Worklet/WASM, absolute report coordinates and strict chronology remain
unchanged. The full player Goal and WBS02.07/11.08 are still incomplete.

## Implementation and author checks

- `d0db81eb` adds three checked diagnostic helpers and 21 independent tests,
  preserving the 18 existing marker cases. Focused helper/Host/Worklet execution
  passed **106/106**, with no failures, skips or cancellations.
- `ba97a56b` adds native streaming validation to the existing runner. Syntax and
  diff checks passed. Legacy `bootstrap`, `runCase` and `verify` are byte-identical
  to `b91ca50e`, as are production `audio-host.mjs` and `audio-worklet.js`.
- Author phase execution passed **11/11** on Chrome153.0.8010.12. Both requested
  rates were applied, all measured output extents were 128 frames, and every
  observation remained phase0, zero PCM and report-unavailable.
- At each rate exactly **3,145,728** active samples across **24,577** callbacks
  agreed. Both interior resets were at index17. Maximum ideal error was
  `0.06250000046566129` at44100Hz and `0.06250000034924597` at48000Hz;
  all samples met the strict error bound below0.25.
- Fresh zero prefixes, first phase samples, reset arrays, horizon refusal and
  closing reset/endpoint raw arrays are retained. The finite schedule ends with
  a same-time reset to1, then holds1. These post-horizon values are refused.
- Constant, late scheduling, generation/id/operation refusal, active timeout,
  stop and terminal suspend passed. The actual fixed **32,768-callback** cap
  retired its pending observation. Total observed callbacks across nine cases:
  **82,780**. No per-callback stream is retained; raw snapshot cap32 and reply
  cap128 are independent of this callback count.
- Existing marker mode was rerun and passed **19/19**. New conflicting mode
  flags returned exit1 before acquiring a browser or server.

The raw Worklet global still lagged the anchored cursor:22 discrepancies in the
44100Hz measurement and17 at48000Hz. At44100Hz ordinal4938, raw634368 versus
cursor634624 differed by256 frames. This is an actual observation; the exact
native lock collision is not instrumented. No globals were overwritten and no
cursor was rebased.

Author artifacts: `target/wf/browser-native-phase-decoding/author-phase-c1/`
and `author-marker-c1/`, with adjacent execution logs. The phase browser
PID1354389 exited0 and is absent; all owned contexts/pages/ports closed, the
server closed, kernel LISTEN8161 was absent and a subsequent local bind closed
successfully. Marker PID1370489 also reported normal browser/server closure.
Generated WASM was reused at74651 bytes, SHA
`34c0bda380ad673c29298e0cec0873009cdfe274772d55904537dff8acef0b18`;
production Worklet SHA is
`5263660c246739e6e1daf29c4aa2ea4b8d9e338cdc75d71e4a654dbad15266c8`.
These are artifact-specific results, not fresh Rust compilation evidence.

One auxiliary legacy extractor initially used an obsolete exact main-block
delimiter; correcting its delimiter confirmed all three identical bodies. One
intentional mode-conflict command used the old protected default evidence path
and failed with EACCES; the isolated output rerun preserved the intended
conflict refusal. Neither required a product source change.

## Verification status

Implementation and author verification are committed. Independent DEEP code
review, subsequent CLI/browser QA, final documentation review and ordered
receipt verification are pending. Author PASS alone does not close this child,
repair the earlier strict browser FAIL or promote the WBS/player Goal.

## Known ceiling

Known ceiling: Single-phase agreement cannot establish absolute chronology — upgrade when independent epoch authority is proved.

See the [diagnostic guide](../platform/GUIDE__browser-native-frame-diagnostic.md)
for the native arithmetic, exact opt-in command, bounded lifecycle and proof
limits. Arithmetic vectors at20hours/oneweek do not establish native-context
age, acoustic presentation, gameplay/replay integrity or performance.
