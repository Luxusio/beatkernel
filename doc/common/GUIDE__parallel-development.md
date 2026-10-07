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
