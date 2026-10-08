# Parallel implementation and agent cleanup

The user confirmed on 2026-10-07 that completed agents should be terminated
and their slots returned before starting more parallel development. Observe
each final result and ensure its source writes have stopped, then use
`close_agent` immediately when the host exposes it. Do not keep finished agents
around merely because they might be useful later.

`interrupt_agent` stops a turn but keeps the agent available; it is not evidence
of permanent close or reclaimed capacity. If close is unavailable, report the
actual limitation and verify capacity before allocating another agent. Do not
pretend that a finished/interrupted agent no longer counts. Existing reusable
implementation/test actors may take explicitly scoped follow-up work without
creating extra agents; do not use a developer identity as a formal QA/reviewer
identity or fabricate independent completion evidence.

Freeze shared interfaces and assign disjoint file ownership before writers run.
Keep the coordinator responsible for common integration, task progress and
checks after relevant writers stop. Implementation and test-author pairs own
different source/test paths; native platform wrappers consume common business
interfaces. Preserve current HEAD when creating worktrees and use separate
scratch directories per lane. Source progress does not replace required
independent review and QA.

## Branch cleanup and commit boundaries

When removing obsolete development branches, integrate useful commits into the
main branch first and then delete the branch, or discard the branch directly.
The user explicitly rejects archive branches, recovery tags, bundles and other
backup artifacts for this cleanup. Preserve unrelated current work. Do not
rewrite master history or push unless explicitly authorized.

Group a working feature with its relevant tests and documentation into a
meaningful commit. Avoid commits for every small progress step; keep each
commit reviewable and the code buildable.

## Parent-Goal parallelism

On 2026-10-08 the user requested parallel development across all remaining
requirements, extending earlier within-feature pairing. Survey actual source
and current evidence, assign literal disjoint ownership, and dispatch ready
independent domains up to real host capacity. Do not serialize UI, media,
networking and quality work behind audio or another blocked requirement.
Keep the full requirement/readiness ledger; source presence is not completion.

Freeze shared interfaces before dependent consumers and reserve common exports,
desktop integration and native cohort/bridge files to the coordinator. One
focused root-checkout task hosts independent AC lanes without competing
same-checkout tasks. Preserve the current no-worktree/no-push scope. The
configured eight-lane ceiling requires fresh source/test-pair and quality-role
reservations under the host limit. Capacity refusal requires accounting for
what started, then waiting or explicitly scoped reuse rather than blind retries.

Verify the ownership/dependency table, actual actor inventory, per-layer tests,
combined independent review and applicable GUI/runtime QA. Physical proof and
genuine unresolved product choices remain explicit while other ready code,
research and evidence preparation proceed concurrently.

Native X11 motion development verification uses the ignored binary fixture
`native_x11_window_presents_explicit_control_motion_with_stable_geometry` with
`BEATKERNEL_TEST_NATIVE_UI_WINDOW=1`, `DISPLAY=:99` and, for the software tier,
`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`. Keep one Cargo job and
the existing bounded compiler environment. This is development evidence, not QA.
If `libxkbcommon-x11` cannot load, check its actual dependencies before rerunning.
On the current Ubuntu environment, official archive packages
`libxkbcommon-x11-0_1.6.0-1build1_amd64.deb` and
`libxcb-xkb1_1.15-1ubuntu2_amd64.deb` were extracted with `dpkg-deb -x` into
ignored `target/toolchain/native-ui-runtime`; prepend its
`usr/lib/x86_64-linux-gnu` directory to that test process's `LD_LIBRARY_PATH`.
No system package installation, service or Docker change is needed. Other hosts
must use compatible runtime packages rather than assuming these versions apply.
