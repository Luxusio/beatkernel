# Ordered font fallback integration

Integrates `wf/text-fallback-fix` (`d44dc7c`) into current base `ef2ba86`,
preserving the already-correct native `--chart` argument expectation rather
than restoring its stale branch fixture.

The existing FontAtlas admits one primary font and up to seven ordered fallback
fonts. Character lookup selects the first mapped glyph, otherwise the primary
glyph zero flagged missing. Placements include font identity so equal glyph
IDs from different fonts do not alias. One atlas, existing transactional text
extension and one renderer texture serve the whole chain. The primary font
sets ascent/baseline. Cached text drawing does not search or rasterize fonts.

Desktop accepts repeatable `--fallback-font PATH` only with `--title-font`.
Bounded file reads and parsing occur on the existing catalog preparation worker
before catalog publication/native play. The window and renderer may start
earlier in loading state. Invalid fonts and overlong chains fail explicitly.
The paths are UI-owned and never added to native/replay/profile arguments.
See [player requirements](../kernel/REQ__bms-player.md) and README usage.

Verification for this integration:

- Font-related library fixtures: 31 passed, 0 failed; main fixtures: 222 passed,
  0 failed. Full library regression: 1,574 passed, 0 failed. Actual exits 0.
- Workspace all-targets with webtransport and WASM browser library check: exits
  0. Existing dead-code warnings remain.
- Actual built `player --help`: exit 0 and fallback usage present. Missing
  primary, empty/missing value and eight fallbacks: exit 1 with explicit errors.
- Independent security review: scoped PASS. Initial code review found inaccurate
  startup/fallback documentation; corrected current documents and fresh actual
  remediation review returned scoped PASS. Independent CLI QA: executed-command
  scoped PASS.

Actual native GUI diagnostics used isolated Xvfb :193 and Vulkan lavapipe.
DejaVuSans plus DroidSansFallbackFull rendered Japanese titles/artists; Hangul
was missing in that supplied chain. Adding NotoSansCJK-Bold.ttc as the second
fallback rendered both `日本語 한국어` and `漢字 한글`. F3 search text/caret,
Escape back and Exit were exercised. Missing font paths showed explicit
CATALOG UNAVAILABLE without publishing a catalog. All GUI sessions closed with
exit 0. The QA agent cleaned its owned Xvfb process.

Initial startup failed because libxkbcommon-x11 was absent. Local extraction of
libxkbcommon-x11/libxcb-xkb packages into ignored diagnostic storage and
LD_LIBRARY_PATH recovered the run; no host package installation or source
workaround was needed. Font files are caller-provided, not bundled.

Ignored evidence: `target/wf/font-fallback-{lib,main}.log`,
`font-fallback-qa-cli-{build,lib,workspace,wasm}.log`, cases/exits and
`font-fallback-qa-cli-{cjk,noto,search,back,missing-path}.png`.
The GUI diagnostics use xdotool/libX11; they are not formal qa-desktop receipt
attestation or whole-task approval. No task verify/close is attempted.

Shaping, kerning and automatic system font discovery remain unsupported.
Browser configuration has no new font picker; physical GPU/device, Windows/macOS
and full player acceptance are not implied by this bounded Linux smoke.
