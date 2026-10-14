# Retained Target output replacement

Retained practice previously refused Output changes, and acknowledged coordinate
pause returned from the common pumps before an output transaction could finish.
The Target path now stages supported changes through the existing typed Owner/UI
and services them in the real solo/cohort pumps before practice/backlog waits.

## Behavior and boundaries

- The sole converted worker conditionally authorizes retirement against its own
  pending suffix. Pending keeps the original live clock; refusal restores that
  exact owner. Authorization ends old observations before normal drop/join and
  complete converter/Mixer recovery.
- Two fresh progressing original native coordinates qualify the replacement.
  Exact accumulated TargetTime, original Mixer creation pin, source history,
  input merger state, program, attempt, Runtime and capture remain associated.
  Applied capability is validated before atomic authority/pause/output commit.
- Paused input drains only through the acquired original prefix, including while
  output is suspended. HOST bookkeeping supplies no audio advancement. Resume
  waits for publication and later actual native adoption/crossing.
- Practice admission includes pending requests and queued/held receipts. It
  fences new replacement admission without starving an already owned transaction.
  UI WouldBlock retries the same correlated reply without another native change.
- Technical failures retain the existing stop policy, complete recoverable owner
  or pending hold, and original observation/probe/cleanup/recovery errors.

Linux Target adapters use the portable service; OS adapters contain no separate
gameplay policy. Unsupported retained Source replacement remains a cold refusal.
Non-None coordinate NativeEnd is refused; retained section completion continues
through PracticePlayback.end_intent. Existing scalar/finite paths remain separate.

## Development evidence

Implementation checkpoints: `3ecd1bbb` authority, `9e224d61` native association,
`df70e2e5` pause, `1463ccfa` conditional worker retirement, `003e938e` controller,
`ba1398e1` Owner/UI, `6c9ce7c2` connected pumps and `095a1eb3` history-race fix.
The independent acceptance below distinguishes completed software checks from
the remaining native-device acceptance.

| Check | Actual result | Scope |
| --- | --- | --- |
| Original-coordinate controller | 18 PASS | Ownership, cancellation, stale state and atomic publication |
| Owner/UI request and observation filter | 25 PASS, including 8 history regressions | Capability, correlation and retirement-observation/history races |
| Connected practice filter | 3 PASS | Real solo, two-member RuntimeGroup, actual Owner history port and transition receipt states |
| No-default app library | 2682 PASS, 6 ignored | Full portable app regression after history-race fix |
| Desktop/WebTransport app library | 3166 PASS, 8 ignored | Independent final CLI QA at095a1eb3 |
| Desktop binaries | All PASS, 17 ignored | Individual counts 393/53/29/40/3/7/59 |
| Desktop all-bin check | PASS | Actual host type checking |
| External publication allocation regression | 1 PASS | Actual prepared-token clone allocates; held commit requests zero new allocations/reallocations |

Filters overlap and are not added as unique tests. The connected cases render
nonzero BGM PCM, retain a partial held suffix, exercise same and changed rate/
channel/matrix replacement and live refusal, drain paused Down/Up input, and
continue through actual resume, autonomous loop, fractional scrub and section end.
Cold Source fingerprints are compared unconditionally. Real archived/final
capture bytes decode to the accepted live operation/JudgeEvent prefixes, exact
member/attempt metadata and final live member hashes.

The unit suites now register one shared safe native fixture module instead of
compiling four copies of the same backend/controller types. The external
allocation binary keeps its separate System allocator and source inclusion;
the library's unsafe-code prohibition remains. After reuse and project-style
formatting, the no-default library again passes2674 cases and the allocation
regression passes. App unit-build warnings fall from42 to1 pre-existing opaque
fixture-field warning; this does not claim a warning-free workspace or native
latency benchmark. Evidence is ac007-project-formatted-app-lib.log and
ac007-project-formatted-allocation.log in the same work directory.

Reproduce using the commands in the
[runtime contract](../runtime/REQ__retained-practice-output-replacement.md#verification).
Development logs, including earlier failed attempts, are under ignored
`target/wf/retained-practice-output-replacement/`; they are not guaranteed to
survive a fresh clone. Source tests and these commands are the reproducible proof.

## Independent review remediation

The independent DEEP review of `015256a3` returned FAIL for one verified race.
The converted worker can authorize retirement after a mathematical history
query but before the old stream accepts its history-retirement request. Its
stopped-stream error then aborts the common pump before replacement publication.
The connected fixture had used the default no-op history-retirement port and
therefore did not cover that production boundary.

The old API reproduced the failure in `review-history-red.log` (0 PASS/1 FAIL,
exit101). The conditional-aware Owner service reuses the existing query/pin
calculation and re-probes only raw backend failures. Confirmed authorization
defers old-stream IO; other errors and complete probe/cleanup/recovery ownership
remain visible. Both Linux delegates use that service.

After the fix, `review-history-green.log` passes25 request/observation cases,
including8 new history cases; `review-history-connected.log` passes3 connected
cases with the actual Owner history port. Both solo/cohort cases inject native
request authorization after an actual old packet, then continue through original
replacement, resume, loop/scrub, PCM/capture/member/attempt checks.
`review-history-full-app.log` passes2682 cases with6 ignored and
`review-history-allocation.log` passes the actual zero-allocation publication
regression. All38 scoped Rust format checks and scoped diff checks pass.
WBS07.09.03/07.08.06 return to V; the acceptance below records remaining gates.

## Independent acceptance at095a1eb3

The fresh full DEEP review returned PASS after the fix. Independent CLI QA then
returned PASS: desktop/WebTransport lib3166/0/8ignored; bins584/0/17ignored
(393/53/29/40/3/7/59); platform387/0/2ignored; external allocation1 PASS; browser
WASM library check, scoped38-file format and diff checks all exit0. The opted
ALSA-null retirement case also passes1; it proves software lifecycle only.
These commands overlap and are not added as unique tests.

Actual public CLI help and invalid paths pass. A3s/48kHz mono offline render
produced144000 finite nonzero frames/576000bytes; duplicate output, zero duration
and missing native input fail explicitly. Windows/macOS no-default app-library
Rust typing exits0 using cached stub compilers, without foreign linking or
execution. Clippy exits0 with non-denied app348/platform109/BMS5/kernel2 warnings;
no strict lint or warning-free workspace claim is made. Initial cached artifact
permission failures and exact narrow continuations remain in the CLI logs;
no file ownership or permission changes were made.

Independent desktop QA reached interactive-desktop on the existing :99 display:
real window, selection, keyboard navigation, resize, Settings44.1kHz edit/Apply/
reopen and audio picker were exercised. Solo and explicit two-member local Start
both reached actual backend errors before play: missing native input and ALSA
default card/PCM respectively. The required acknowledged retained pause, actual
Output capability and same-capture resume were unavailable. Desktop returned
BLOCKED_ENV, not PASS; both owned GUI processes exited cleanly and the external
display was preserved.

A diagnostic concurrent launch through /proc/PID/exe exited139 before rendering;
it did not reproduce using the ordinarily rebuilt executable. The unsupported
launch-method anomaly is retained in the desktop transcript without claiming a
normal launch regression or using it as successful evidence.

Reproducible commands/tests remain in source. Full transcripts/screenshots and
first-failure logs are under ignored qa-cli-095a1eb3/ and qa-desktop-095a1eb3/
within the same work directory. Runtime verification also reports missing
hook-owned completions; actual review/CLI finals are NON-ATTESTING. They are not
a close authorization, and the actual desktop environment blocker takes
precedence over an attestation-only park. No lens is rerun solely for receipts.

## Known ceiling and remaining scope

This host has DISPLAY=:99 and local X11 tools, but `/dev/input`, `/dev/uinput`
and `/dev/snd` are absent. Portable scripted native ports and ALSA null cannot
prove an acknowledged native GUI pause/change/resume or acoustic timing.
Final independent desktop verification must report the actual tier reached.

All38 task-edited Rust paths pass the workspace's edition2021/default formatter.
The additional whole-workspace format check reports414 unrelated paths unchanged
since the task base; ac007-workspace-format-baseline.json records that comparison.
This existing style debt remains WBS14.05 work, without a native-task reformat of
unrelated files. Platform/non-test/integration warnings also remain separate from
the reduced app unit-build warning count above.

Foreign SDK/native execution, direct-source retained replacement, browser parity,
physical devices and full player acceptance remain separate Goal obligations.
WBS07.08.05,07.08.06 and07.09.03 retain V until their remaining independent gates;
supporting results do not complete the structural parents or the whole player.

The full scope is preserved in the [WBS](../kernel/REQ__bms-player-work-breakdown.md),
[gapless contract](../kernel/REQ__gapless-practice.md) and runtime contract.
