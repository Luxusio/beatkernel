# Remaining menu and Results component motion

Settings, Records (including historical details), Players and Devices now have
the same retained component-motion façades as Selection, Display and Practice.
The native and browser adapters connect these façades to screen-owned motion
tracks and submitted input poses. Genuine and frozen Results use readonly
mounted-node targets, preserve ordinary headers and controls, and cache geometry
between motion frames. Browser Results ownership checks both generation and
content; replacement retires the previous owner, while page changes remove
targets that are no longer displayed. Rendering candidates stage UI changes
before publication and retain existing note caches.

## Development evidence

- Five public view test targets: 25 passed.
- Graphics component regression filter: 34 passed before final adapter changes,
  including Results geometry identity rollover and source/inherited clip cases.
- Current Worker/host/protocol Node tests: 305 passed, exit 0. Results
  invalid-request regressions preserve existing animation and subsequent requests.
- The first portable Results fixture run failed all five cases because its
  disconnected test queue could not provide admitted completion. Retaining its
  original producer through the actual Mixer drains fixed the fixture; all five
  now pass through the production Results owner.
- Current combined motion regressions: 54 passed, including seven shared
  transactional pose-restoration regressions, six remaining-menu
  adapter cases and five portable Results owner cases. Scene transaction tests:
  two passed, including mismatched/failed/timed candidate atomic rejection.
- First native adapter run compiled successfully: two passed, four failed.
  Fixture corrections preserve inherited clipping and existing ordinary Results
  controls. Independent discovery additionally found native Records target
  pruning and submitted-pose interaction issues; those were corrected. The
  resulting seven native owner tests and two Scene transaction tests passed.
- Native software-window development test before the shared restoration cleanup
  passed on llvmpipe/Vulkan:
  four menus and genuine Results, 12–14 submitted frames per surface, 11 original
  PNG captures, preserved headers/ordinary controls and a partial Results page.
  The owned test and Xvfb processes both exited 0. This is development evidence.
- Independent final DEEP code review passed after fixing the browser acceptance
  script to advance the actual owner revision before publishing a repaint.
  Independent CLI/browser/native QA remains pending. The first actual browser
  run passed Settings and Records motion/lifecycle interactions, then stopped
  because the acceptance script attempted an illegal sibling route transition.
  The script now returns through the actual Settings parent before Players and
  Devices. The second actual browser run passed all four menu motion/lifecycle
  and zero-extent checks, then stopped when its production canvas click raced
  the accepted geometry publication. A separate real production diagnostic
  reached visible, matching geometry without Worker/renderer errors; this is
  diagnostic evidence, not a browser QA PASS or loading-speed benchmark.
  Renewed review and browser QA are required after synchronizing that click.

These development checks and the code review do not
complete WBS10.16/10.17 or the full BMS player Goal; WBS remains 90/193.

## Known ceiling

Actual current-production browser interactions and native software-window
acceptance remain pending. Prior direct PNG/window rendering evidence and the
known desktop MCP window-binding limitation belong to the earlier Practice
task; they do not establish acceptance of this change. Motion-only geometry
reuse does not establish a physical device frame budget or cross-platform
latency guarantee.

Contracts: [UI motion](../ui/REQ__ui-motion.md) and
[browser retained menus](../ui/REQ__browser-retained-menus.md).

## Code-smell cleanup

Five duplicated pose-restoration blocks now use the existing scheduler’s
transactional `restore_poses`. It removes three temporary update Vecs and two
individual setter loops, validates a bounded stack batch before publication,
and preserves completed poses, elapsed progress and platform-owned lifecycles.
The broader cleanup remains queued as TASK__player-code-smell-cleanup; this
change does not claim that all source smells are resolved.

The user’s immediate-feeling loading requirement is recorded in
[performance and testability](../kernel/REQ__performance-and-testability.md).
Startup, screen-transition and media-preparation latency measurements remain
pending under TASK__player-loading-latency; this motion change makes no loading
speed claim.
