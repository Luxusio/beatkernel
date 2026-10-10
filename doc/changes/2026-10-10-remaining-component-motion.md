# Remaining menu and Results component motion

Settings, Records (including historical details), Players and Devices now expose
retained component-motion façades. Native/browser owners bind actual screen
identities and publish input poses only after successful presentation. Genuine
and frozen readonly Results preserve ordinary headers/actions and cache geometry
between motion frames. Page changes prune absent targets while preserving poses
of surviving nodes. UI staging preserves existing note caches and atomic refusal.

## Independent software evidence

Reviewed implementation: `e0c08e4`, based on `f6d50d3`. Full DEEP review and the
substantive source-fix reviews passed; both discovery cycles completed. Native
Records pruning, Results accepted input, Settings hover and per-request Results
refusal defects were corrected. No implementation or loading-performance ranking
is inferred from test counts.

- CLI: five public-view targets 25 passed; motion/owner filter 54; Scene
  transactions two; component filter 43; browser-menu regressions 29; Results
  regressions 16; native owner seven. The window test is ignored by default and
  was explicitly executed by desktop QA. Filters overlap, so these are not a
  unique-test total. Five Node targets passed 305/305. Scoped formatting, diff
  and WBS checks passed; launcher help returned 0 and invalid mode returned 1.
- Browser: actual headed Chromium/software SwiftShader with current production
  WASM/Workers passed. Four menus, accepted moved hits, repaint, Back/reentry,
  zero extent, actual host Settings editor motion/edit/preedit/stale focus,
  genuine BMS completion and Results motion/resume/retirement/disposal passed.
  All 14 original captures were inspected; no production console/network errors.
  Fixture favicon 404 is unrelated. WASM SHA256:
  `98769dda457cc0ceaee23077c0bd3520834c61565f4d196284f2c735cfed78cc`.
- Desktop: current native fixture passed one test, 64 software Vulkan motion
  frames and 11 inspected PNGs across four menus and genuine Results. Geometry
  identity/revision, headers/footer, accepted hits and retirement passed.
  Same-window MCP binding succeeded on existing :99. Motion depth is
  **window-rendered** because the host fixture does not pump OS events.
- Separate desktop production event-loop UX verified F2, Tab, Players/Add,
  genuine-chart F4 Records, resize and Escape. Missing chart/device feedback
  remains recoverable. The current test-profile production binary SHA256 was
  `1c91bd22a348f6eccad2ee7d89f1deea66cd5e1cf7fc8034525e08bbced27986`.
  Independent browser and desktop UX reviews passed for this explicit capability.
- Owned browsers, Xvfb/application processes and servers ended cleanly; the
  existing shared :99 display was preserved. No push or worktrees were created.

Raw evidence is in ignored local artifacts:
`target/wf/remaining-motion-01a124c2/{qa-browser-final,ux-browser,qa-desktop-current,ux-desktop-current}`
and `target/wf/worklet-chronology-qa-cli-1`. Earlier browser attempts failed on
three acceptance-script assumptions: illegal sibling navigation, clicking before
submitted geometry, and requesting comparisons absent from genuine completion.
Their original failures remain preserved. Reviewed fixes use real parent return,
exact submitted tokens and actual capability; failure observations are retained.

## Limits and remaining work

Genuine solo browser Results has no comparison capability. Comparison and
multi-page pruning are deterministic CLI evidence, not connected browser proof.
Native page-two capture preserves the surviving card's completed clipped pose;
it verifies retirement of previous rows, not visible text of that moved card.
Animated OS hit acquisition, physical IME/device/audio timing, hardware frame
budgets, foreign OS execution and broader accessibility remain unverified.
Small bitmap labels and caller-selected overlapping transforms remain UX backlog.

All required substantive code/QA lenses returned PASS; Harness closure still
requires its ordered hook-owned attestation. These results do not complete
WBS10.16/10.17 or the full player Goal. WBS remains 90/193.

## Code-smell cleanup and loading

Five pose-restoration copies now use existing transactional
`MotionScheduler::restore_poses`, removing three temporary update Vecs and two
individual setter loops. Bounded stack staging preserves completed poses,
elapsed progress, geometry identity and platform-owned lifecycle. Remaining
canonical imports/device contract/keysound indexing/ownership cleanup stays
queued under TASK__player-code-smell-cleanup.

The user's immediate-feeling loading requirement and confirmed source audit are
in [performance and testability](../kernel/REQ__performance-and-testability.md).
Browser import currently reads all supplied files before displaying the catalog;
repeat preparation does not retain decoded PCM across calls; WAV aliases clone
PCM backing. Startup/transition/media cold/warm measurements and optimization
remain pending under TASK__player-loading-latency. This change makes no loading
speed claim.

Contracts: [UI motion](../ui/REQ__ui-motion.md),
[browser retained menus](../ui/REQ__browser-retained-menus.md).
