# Retained browser Settings policy and profile admission

Current verification status: AC-005 remains FAIL. At committed source HEAD
`3df14d7380723bbf2ac5fdc2a463581f69c9c5a8`, one strict run passed and the unchanged
confirmation again reported audio-WASM CDP/Puppeteer `ERR_ABORTED`. Both declared
and consumed exactly 62,096 bytes and completed all seven genuine flows with
replay/archive/Results ACK. Exact Content-Length is insufficient to remove the
intermittent diagnostic. Neither final run reproduced the earlier isolated
status-6 playback failure; its cause is still unconfirmed. Retain both records
under `target/wf/settings-policy-qa-browser-12a58-{3,3b}/`.
Current independent CLI checks passed: 45 mapping/keyboard/audio-diagnostic,
51 focused host/Worker, 45 Rust menu and 14 shared Settings tests. Product Rust,
JavaScript and unit-test files are unchanged since the retained full-suite runs
(2,405 Rust / 978 Node); those counts are prior runs, not new full-suite runs.
Code review passed; CLI's overall verdict is FAIL because it incorporates the
delegated AC-005 failure despite its own 155 passing checks. Stable browser
acceptance is unproven. This child remains
unfinished and WBS is not promoted. Following the user's standing direction to
work on other available items when stuck, retain this investigation and continue
independent Goal development rather than weakening checks or retrying indefinitely.

The retained Settings menu now exposes numerical timing preset, rank precedence
and gauge alongside the thirteen existing scalar fields. Shared pure conversion
and validation preserve legacy Apply omission, absent full-profile defaults and
latent precedence. Practice retains the parent policy, Players Done returns to
the original parent, and browser-only unavailable native controls have muted
presentation and no hit or motion target. Native controls keep their default
capabilities. This extends the existing retained UI without a virtual DOM, new
dependency or additional Worker.

Full profile loads use the existing settings operation with bounded candidate
admission. Correlated pending and completion revisions synchronize the retained
Settings ancestor, retained Practice and Window acquisition mirrors. Cancellation
before admission retains an unabortable read until settlement; uncertain results
after admission require joined owner reset. Full eighteen-lane bindings remain
part of the profile codec. See [the owning requirement](../ui/REQ__browser-retained-menus.md).
Valid profile import also repairs unfinished empty timing or duplicate-key
acquisition drafts. Incoming profiles remain strictly validated; uncertain
results restore the original bounded form rather than requiring it to be a
valid exported profile.

Verification evidence at source HEAD `a45522ef9fba0f1629e0575346fbdb6eb047014d`:

- Settings mapping: 12 Node tests passed; keyboard intent: 18 passed.
- Shared Rust menus: 45 tests passed; Settings views: 14 passed.
- Window host: all 207 tests passed; Worker: all 85 passed using genuine WASM.
  Tests include failed/disposed owners, invalid post-admission menu state,
  profile repair and restoration of an unfinished original form.
  Independent code review also reproduced an initial Settings entry regression:
  unfinished timing or duplicate keys prevented draft publication. Initial
  publication now uses the existing bounded raw-field order; Apply/profile
  validation stays strict. Two initial-entry regressions failed before the fix.
- DEEP code review and independent security review passed after that correction.
- Full app graphics library: 2,405 passed, zero failed, five ignored.
  Full Node suite: 978 passed, zero failed/skipped/cancelled. Changed JavaScript
  syntax checks and diff whitespace checks passed.
- Browser release build and generated bindings passed. WASM SHA256:
  `7a386e90245be7f00c50aa9b1b96d82f6cbe621ce479a17aa5b5ad835ac1bd5a`.
- Actual Chromium completed seven production flows: trusted policy editing,
  disabled native controls, composition bridge, Practice, Players Done,
  complete profile roundtrip and genuine selected-policy playback. Mixer output
  frontier advanced from 495 to 40559 frames; the one-note chart completed with
  one miss, original replay/archive and a genuine Results packet/ACK.
- Browser, Xvfb, loopback server and temporary profile/cache cleanup completed.
  No product page, console or network errors were observed.

Independent browser evidence lives in ignored
`target/wf/settings-policy-qa-browser-12a58-1/evidence.json`; full CLI logs live
in `target/wf/settings-policy-qa-cli-12a58-1/`. Scoped development logs remain in
`target/wf/browser-retained-settings-policy/`. Actual independent review and QA
finals do not themselves replace Harness-owned completion receipts.
Two earlier development runs completed all functional stages but were rejected
by the strict probe for an audio-WASM `ERR_ABORTED` request. Bounded request,
server, reader and compilation observations were added without relaxing the
network assertion. The independent required browser QA passed with no errors.
An additional UX execution reproduced that diagnostic despite consuming all
62,096 bytes, reaching end-of-stream, compiling the module and completing actual
playback. The failure record is retained in
`target/wf/settings-policy-ux-browser-12a58-1/evidence.json`. One bounded unchanged
rerun at `target/wf/settings-policy-ux-browser-12a58-2/evidence.json` repeated the
same strict diagnostic failure while again completing the actual user flows.
Its cause remains unconfirmed. No assertion was relaxed or source fix claimed
for that diagnostic. The scoped UX judgment passed, with long preset-ID clipping
and editor focus-release ergonomics recorded as follow-up improvements.
The independent browser QA reassessed both supplemental failures and returned
FAIL for AC-005's strict zero-network-errors criterion. The original passing
record is retained, but does not establish a stable final PASS.
One NetLog execution is retained in
`target/wf/settings-policy-netlog-12a58-1/evidence.json` and
`target/wf/settings-policy-netlog-launch-12a58-1/chromium-netlog.json`.
Its native network request recorded normal complete body reads and no network
error, while CDP reported cancellation near completion. No cancellation caller
was established. That execution also returned a separate audio processor
terminal status 6 during real playback, with incomplete replay and no completed
Results. Its relationship to the fetch diagnostic is unproven; browser shutdown
required termination after a close timeout. The runner now also preserves the
business Worker's bounded records on failure, where the original failure path
only preserved Window and renderer observations. The forwarding observer also
retains the known versioned terminal diagnostic fields (origin, actual/expected
frames, start and successful arm) instead of dropping those original facts.
Syntax checking passed; at that checkpoint the diagnostic addition awaited
independent review and runtime verification.
The diagnostic addition passed independent static review; a subsequent actual
committed run again completed all seven flows but failed the network criterion.
An otherwise identical, one-trial diagnostic copy explicitly serving the owned
static bytes with exact Content-Length passed every strict criterion, with normal
CDP/Puppeteer request completion. The probe server now declares that exact length;
At that checkpoint the framing remediation awaited independent review and
committed runtime QA. Those completed later: review passed, but unchanged
browser stability confirmation failed as recorded at the top of this note.
The counterfactual supports the framing hypothesis, not a proven Chromium root
cause or a claimed remedy for the earlier separate status-6 failure. Evidence:
`target/wf/settings-policy-qa-browser-12a58-2/` and
`target/wf/settings-policy-content-length-12a58-1/` (ignored, original bytes/hash
and variant diff retained). No continuity guard or success assertion changed.
This child is not verified complete.

## Known ceiling

Physical GPU performance, native device latency and physical OS IME are not
established by Chromium software rendering or injected composition. Keyboard
binding acquisition/editor remains in Window; this child does not complete the
whole browser-menu WBS leaf or the overall player Goal. WBS remains 91/193
verified at this checkpoint.
