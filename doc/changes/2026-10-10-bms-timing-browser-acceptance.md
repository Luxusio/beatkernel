# Selected BMS timing policy: current browser acceptance

Independent browser QA completes the remaining selected numerical-preset flow
on current production Worker/WASM: capture, visible stored history and pages,
replay with conflicting live selectors, preview seek, settings/reset, missing
rank recovery, selected stop/retry and two-member keyboard/touch capture.
Live/replay score matches `1/4/0/1`; all sixteen recorded stage-window rows and
numerical/Hold versions match. Thirteen screenshots were visually inspected;
there were no console/page errors or unexpected Worker failures. Both bounded
commands and their owned Chromium/Xvfb exited zero and private resources were
removed. Prior failures remain preserved. Evidence lives at
`target/wf/bms-timing-resume-20261010/REPORT.md` and companion traces/screenshots.

This turn changes verification documentation, not production code or defaults.
The current artifact and the preceding independent code review/CLI96 together
support WBS08.07, now D; total WBS is89/193. Detailed behavior and hashes remain
in [the timing contract](../kernel/REQ__bms-timing-presets.md).

## Known ceiling

Acceptance covers Linux headed software-GPU browser behavior and preceding
software/native CLI coverage. It does not certify physical latency, foreign-OS
devices, full beatoraja interaction compatibility or complete player delivery.
Actual lens PASS and hook-owned task closure are separate; closure remains
subject to ordered Harness evidence.
