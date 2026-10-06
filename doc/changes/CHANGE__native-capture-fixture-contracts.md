# Restore native capture, contact audio and fatal-path fixture assumptions

Source-aware capture byte-budget tests used a header cap larger than their exact
file cap, so ReplayCodecLimits rejected setup rather than exercising file limits.
Use valid header/file caps together, retaining the exact-fit, one-byte-short and
header-too-small assertions. Cohort capture expectations now include the configured
original-song endpoint, matching the finite v4 capture contract. Finite cohort files use the section-aware replay validator; the legacy validator
still refuses finite files. Entire header identity, original player/device IDs
and pristine judge checks remain intact.

The contact live/replay parity test expected three keysounds for an instant note
and one LNTYPE1 hold. Its hold tail is judged but deliberately mute under the
adapter contract. Expect two commands and no third PCM burst, retaining three
successful judgment stages and exact live/replay event, hash and PCM parity.

Fatal native fixture helpers now permit the 42-frame inspection renders their
consumers actually perform (64-frame maximum instead of 32). The post-fence
sequence rejection uses a regressing sequence 1 after sequence 2; equal acquisition
sequences remain valid. The assertion checks the precise SequenceRegression
payload (last 2, received 1), rather than accepting any error. Stop prefixes, capture, judge and PCM assertions remain. When the one-slot queue
admits only a partial command prefix, the group runtime remains poisoned and
rejects subsequent input. The host fixture now verifies that typed refusal and
unchanged report count, capture and judge state; the complete-prefix case still
checks successful post-fence release reconciliation.

The room completion fixture exposes its first genuine render report before
hiding subsequent reads. This exercises transient missing evidence and reuse of
the last validated report. Hiding all initial evidence correctly blocks judging
under the logical schedule contract and cannot support the fixture's intended
completed-judge assertion. Room completion still must wait for output drain.

Only test setup and expectations are changed; production input, capture, mixer
and completion admission rules remain unchanged. These portable common-owner
fixtures do not establish real native-device or multiplayer transport acceptance.
The broad task remains open with independent review and required QA pending.

Verification (2026-10-06): full runtime library with webtransport reported
1503 passed, 65 failed versus the preceding WebHID baseline of 1496/72.
No new failing names were found. Resolved failures comprise source capture byte
limits, finite cohort capture identity, contact live/replay parity, fatal gauge
sequence validation, competition/host refusal prefixes and room completion.
All changed fixtures compiled and ran in this full suite. The full command still
exits 101 for the 65 remaining failures; it is not a whole-task PASS.
