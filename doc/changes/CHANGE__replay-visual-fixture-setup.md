# Restore replay visual fixture source and mode-aware consumers

Four replay visual fixtures referenced WAV01 without defining it in their BMS
source. The strict parser correctly refused the source before replay assertions.
Define the fixture resource path; no file is opened and parser missing-definition
admission remains unchanged. Equal-time input, inclusive edge, pause/resume,
pressed-owner release and invalid whole-log checks remain intact.

The gauge/pressed-owner replay fixture captures ButtonOrContact v5 metadata.
Use reconstruct_section and ReplayVisual::new_section for that mode-aware log;
legacy tuple reconstruction and visual creation still reject v5. Retain the
checks that gauge failure hides pressed owners while later legacy operations
still reconstruct their original judgment results and stable hash.

Only fixture setup/consumer selection changes. Production replay compatibility
and input-mode validation rules remain strict. The broad task stays open.

Independent review of the accumulated 7aff757..c3a6ae6 change scope completed:
review-code PASS (DEEP bounded formal-only, no findings), review-security PASS
(no findings; JS physical-input independently 11/11 passed). These are actual
reviewer results for that fixed scope, excluding this replay fixture increment.
They do not prove full task completion or substitute for required browser,
CLI and desktop QA, native execution or remaining functional work. No receipt
is written by the coordinator.

Verification (2026-10-06): full runtime library with webtransport reports
1535 passed, 33 failed versus the preceding 1530/38 baseline. Exactly four
legacy visual fixtures and one contact gauge/pressed-owner fixture resolve;
there are no new failing names. All changed test paths compiled and executed.
The full command still exits 101 for remaining failures; no task PASS/close.
