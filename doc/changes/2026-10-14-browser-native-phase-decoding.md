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

The first independent DEEP review found two defects: page acquisition could
escape the phase case deadline, and the guide used doubled Bash continuation
backslashes. The controller now starts its single120s deadline before acquisition
and applies it through navigation and evaluation. Owned page closure has its own
3s budget, including pages arriving after acquisition timeout. Marker mode retains
its existing behavior. The guide command now uses single continuations; executing
the copied command with a controlled Node endpoint confirmed one invocation and
all five environment values.

Seven independent tests exercise the actual extracted controller with controlled
promises and time, including acquisition rejection, late arrival, stalled
navigation/evaluation, pending closure, and successful shared-budget cleanup.
The focused helper/controller/Host/Worklet author run passed **113/113**, with no
failures, skips or cancellations. Syntax and diff checks also passed. Artifacts:
`target/wf/browser-native-phase-decoding/author-remediation-node.log`,
`baseline-page-acquisition.log` and `guide-command.log`.

Fresh post-fix DEEP review passed at source HEAD `a01ad09a`, with no remaining
findings. Subsequent independent CLI and actual browser QA also returned PASS.
Ordered hook-owned attestation is separate from these substantive results;
missing receipts leave the child unfinished. These checks do not repair the
earlier strict browser FAIL or promote the WBS/player Goal.

## Independent verification

- CLI QA passed **113/113**, zero failures, skips or cancellations; source
  syntax, base-to-HEAD diff, legacy/production byte equality and the copied
  GUIDE command also passed. Both phase/marker and phase/required-cold conflicts
  exited1 with zero observed HTTPS servers, child spawns and Puppeteer loads.
- Fresh Chrome153.0.8010.12 phase execution passed **11/11**, and the subsequent
  existing marker execution passed **19/19**. Both Node sessions exited0.
  Browser QA captured and visually inspected28 real800x600 diagnostic pages;
  render-only depth is appropriate because these blank pages have no UI controls.
  Actual native scheduling and Host/context/private-port operations ran.
- Each rate again checked **3,145,728** active samples across **24,577** callbacks
  and two index17 resets. Streamed maximum errors matched the author run.
  Separate CLI and browser oracles recomputed512 retained active samples per
  rate; CLI also recomputed16 binary edges from eight fresh marker cases.
- This fresh phase run observed **82,776** callbacks, **219** replies and **144**
  retained raw messages across its nine cases. Positive-stream raw-global
  discrepancies were11/10, distinct from the author's22/17. At48000Hz
  ordinal14080, raw1804288 versus anchored1804544 differed by256 frames.
  No cursor was rebased. Output extents remained128, phase0, zero PCM and
  report-unavailable; production Worklet/WASM hashes matched.
- Positive-stream summaries recorded27 snapshots after closing-marker
  corroboration, before that callback's final raw record; final retained evidence
  contained28 each, within cap32. Scalar held1 arrays after the horizon
  were preserved and refused as active evidence. Auxiliary QA checker assumptions
  about these shapes/counters and intentional404 close-time aborts were corrected
  only in ignored artifacts, with original failure logs retained.
- All recorded owned resources closed. Phase PID1524491 and marker PID1542451
  exited0 and are absent. Fresh kernel checks found no LISTEN8162, and local
  bind/close passed. Eight console404 reports were expected favicon/boundary
  requests; page errors were zero. There was no browser retry or source change.

Fresh evidence is under `target/wf/browser-native-phase-decoding/qa-browser-c2/`,
`qa-marker-c2/` and `qa-cli-c2/`. Read `evidence.json`,
`independent-inspection.json`, `final-ownership-check.json`,
`recomputed-qa-browser-c2.json` and their adjacent logs for the corresponding run.
These directories are ignored local evidence, not committed or portable results.

## Known ceiling

Known ceiling: Single-phase agreement cannot establish absolute chronology — upgrade when independent epoch authority is proved.

See the [diagnostic guide](../platform/GUIDE__browser-native-frame-diagnostic.md)
for the native arithmetic, exact opt-in command, bounded lifecycle and proof
limits. Arithmetic vectors at20hours/oneweek do not establish native-context
age, acoustic presentation, gameplay/replay integrity or performance.
