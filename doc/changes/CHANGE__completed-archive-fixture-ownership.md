# Keep completed-result fixture output connected and read the right seed field

Three completed presentation/archive/comparison helpers returned only a Mixer
and immediately dropped the command producer. Completion requires connected
output, so these fixtures failed before testing frozen result proof, roster
atomicity or archive/capture ownership. Helpers now return producer plus mixer;
callers retain a named producer binding throughout output observation. Disconnected
output rejection remains unchanged.

Native cohort finalizer fixtures intentionally encode their test member ID in
the capture's chart-seed option. ReplayHeader.seed remains zero under the capture
contract and is not that field. Read chart_seed through the section setup decoder
and checked u32 conversion before checking replay paths and ordered save attempts.
Production player identity still comes from the explicit roster; this test-only
encoding does not establish any player-ID convention.

Existing completed-result scope/header identity, entire original roster,
comparison attachment, disabled/missing capture refusals and original typed error
preservation assertions remain in place. Only fixture setup/metadata access changes;
production completion and archive admission rules remain unchanged.

These tests use actual portable Mixer completion evidence and application archive
logic. Native device execution and required independent review/QA remain separate
unfinished work; the broad Harness task stays open.

Verification (2026-10-06): full runtime library with webtransport reports
1530 passed, 38 failed versus the preceding 1519/49 baseline. Exactly eleven
existing failures resolve: three completed presentation, three result archive,
three archived comparison and two native cohort finalizer cases. There are no
new failing names. All changed fixtures compiled and executed. The full command
still exits 101 for remaining failures; no whole-task PASS is claimed.
