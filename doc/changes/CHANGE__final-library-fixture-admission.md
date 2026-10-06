# Restore final room UI, image and record painter fixture setup

Three graphics-related room fixtures referenced WAV01 without defining it.
Define the keysound resource in their literal BMS source, preserving strict parser
admission and the actual chart/UI geometry assertions. No file IO is added.

The image alias fixture's missing.bmp reference found the inserted missing.PNG
through the established compatible image variant policy. Rename that unrelated
decoy so the genuinely missing case stays missing. Original shared raw/layer Arc,
decoded byte accounting and unavailable reasons remain asserted; production
variant lookup is unchanged.

The full record painter/hit-order test now uses seven selected opponents so
Add Own/Add Other controls can participate in the expected order. Eight is the
established disabled boundary. The same fixture separately checks both Add
controls are unavailable and absent from actual composed hits at exactly eight;
production control availability is not changed.

These are fixture-only changes, not new runtime behavior or graphical/device
acceptance. The broad task remains open with main executable/browser tests,
feature integration, native composition and formal QA still unfinished.

Verification (2026-10-06): the final full runtime library test run with
webtransport reports 1568 passed, 0 failed, exit 0, including the explicit eight
opponent boundary checks. Compared with the preceding room fixture baseline,
all twelve remaining library failures resolve across this and the stepped-score
fixture increment. Workspace all-target webtransport check exits 0; existing
library dead-code warnings remain. This is library test completion, not full
Goal/task completion; main/Worker verification and feature/native QA remain.
