# WASAPI live mode and shared policy

Paused WASAPI settings now advertise shared/exclusive mode and shared
engine/legacy initialization policy. The typed mapper validates the final
requested mode, including default-period-only legacy shared, before queueing
replacement. Original source format/rate, channel matrix, negotiation permission
and common pause/epoch/Mixer/pending lifecycle remain unchanged. Shared policy
has no effect on exclusive initialization; exclusive replies advertise engine
as the next shared-mode default.

The retained output panel accepts up to eight bounded fields and shows four
rows around the selected field. Arrow/tab navigation and Previous/Next or
PageUp/PageDown reach the remaining rows. Hidden rows have no hit rectangles;
all edits remain in one correlated Apply request and the same screen scope.
Page controls disappear during pending replacement. Row clicks now admit the
fourth and later valid fields, fixing the old three-row-only activation range.
Input-font preparation also covers the actual bounded field count, including
Unicode IME previews in later pages, without committing preview text.

Portable tests cover mode transitions, final-mode period validation, unchanged
source/policy, six-field paging and last-row pointer activation, pending controls
and atomic full-draft submission. Independent review found the old fixed
three-field glyph preparation; the bounded actual count and last-page Unicode
IME regression fixed it, then fresh code/security reviews passed.

Scoped QA passed: library 1,588 passed / 2 ignored, main 237 passed, Windows
binary 36 passed, and actual ALSA settings diagnostic 1 passed, zero failures.
Workspace, WASM, Windows normal and isolated SDK Rust all-targets checks exited
0. CLI help exited 0; initial-launch matrix input returned the expected exit 1.
Six-field paging/click, pending, final-mode and last-field IME regressions all
executed. Rust SDK cfg checking does not prove native bridge linking or ABI.

Real Windows
shared/exclusive initialization, SDK/driver routing and acoustic timing remain
unverified on this Linux host; driver/backend switching and full player
acceptance remain unfinished.
