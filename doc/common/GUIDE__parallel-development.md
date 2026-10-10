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

## Throughput workflow — user decision, 2026-10-09

Develop a connected feature through its actual application path as one work
package rather than repeatedly stopping after isolated helpers. Declare stable
shared interfaces and disjoint files first, then implement ready independent
domain/UI/platform adapters in parallel within available capacity. Keep blocked
hardware or tooling work separate and continue other ready implementation.

Each implementation author must run focused checks of their own changed paths
before reporting the lane ready. In particular, fixtures must reach the behavior
being tested rather than fail during unrelated setup. Integration starts only
after these focused checks pass and all relevant writers stop. Reserve one
heavy compiler/full-suite/browser owner at a time; lightweight author checks
may run when they do not conflict with that lease or changing source.

Run the appropriate broad regression once after the connected batch is ready.
For a failure, inspect the actual error and run the failing focused path while
fixing it; do not repeat already-green unrelated suites to discover the next
fixture error. Broaden again only when production changes, interactions or
unresolved findings justify it. Keep required independent final review and QA;
neither author checks nor speed goals substitute for those results.

Keep compiler time/memory bounds specific to compiler processes. Do not reuse
a compiler virtual-address limit for Node/WASM or Chromium: reserved address
space is distinct from physical memory and an incompatible limit can prevent
otherwise valid WebAssembly instances. Retain bounded execution and one-owner
resource scheduling without changing services, drivers or Docker.

Commit a coherent working feature with its tests/docs, not each small step.
Report completed behavior, actual validation and the next remaining dependency;
do not turn routine intermediate work into repeated completion cycles or
receipt-only reruns. A focused implementation task may span turns while the
full player Goal remains active.

## Parent-Goal parallelism

On 2026-10-09 the user explicitly asked to stop security-driven interruptions.
For BeatKernel development, use ordinary code review and functional QA;
do not introduce a separate security-review gate unless the user requests it.
Do not add security features, speculative threat checklists or confirmation
rounds to routine game/runtime development. Malformed chart/replay/packet
handling, exact data preservation, resource bounds and reproducible crashes
remain concrete correctness tests. Report those results in plain language.
Keep progressing on implementation rather than pausing for hypothetical risks.

The user reiterated this on 2026-10-09 while the continuous fuzz task was
active. Defer that task's remaining campaigns/minimization/infrastructure work
and prioritize unfinished player features. Existing committed tests remain;
do not start more dedicated audit/fuzz infrastructure without a user request.
Run ordinary regression checks needed for the feature being implemented.

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

## Code-smell cleanup during player development

On 2026-10-10 the user requested cleaning code smells while continuing the full
player Goal. Prefer removing duplicated algorithms, forwarding-only contracts
and obsolete internal import paths over adding frameworks. Preserve genuine
runtime behavior, audio/input boundaries, submitted UI poses and existing tests.
Use small cohesive changes with explicit ownership and focused regressions;
source presence or an advisory audit does not establish acceptance.

The audit identified the following cleanup batches. They remain pending until
their implementation and relevant verification actually complete:

- Current motion feature: shared pose restoration is software-verified; platform
  admission identities and lifecycle ownership remain local to their adapters.
- Application output: migrate internal imports to canonical domain paths and
  remove redundant native/common trait forwarding with compatibility accounted.
- Core input/audio: index keysound bindings by object/stage while preserving
  fanout order, partial success and failure reports. Measure allocations before
  changing report buffers or built-in/custom interaction storage.
- UI/application: reduce repeated repaint clones and clarify cache ownership;
  group cohesive screen state rather than introducing a universal controller.

Keep the existing four crate boundaries and native output configuration.
Current boxed judge interactions and reactive UI callbacks have real costs;
do not describe the complete implementation as zero-cost without evidence.

## Reuse verified builds for scoped checks

A current-source integration-test build may already produce the runnable
application binary. Record its path, SHA and build provenance before another
profile build; functional native QA can reuse that binary without claiming
release performance. Warm test filters may similarly run the exact already
built test executable after establishing its source/build identity, avoiding
repeated Cargo metadata walks. A source/configuration change requires fresh
build verification. Keep one heavy compiler/browser/GPU lease.

Scoped Rust formatting uses `skip_children=true,reorder_modules=false` when
preserving the reviewed module layout; do not recursively reformat unrelated
modules. Record unrelated initial-check findings rather than silently editing
those files.

Current remaining-menu/Results native QA used a fresh owned window on the
existing live `DISPLAY=:99`, matching the device-lab X11 backend. Same-window
MCP enumeration and screenshots succeeded. Preserve that shared server; clean
up only owned application processes. A display without an EWMH window manager
may reject the focus helper, while direct X11 focus of an owned production
window still permits genuine keyboard/mouse event-loop checks. Host-driven
motion fixtures and production OS-input checks remain separate evidence tiers.
