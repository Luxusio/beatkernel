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
