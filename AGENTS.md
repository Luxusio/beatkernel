@CONTRACTS.md
## Harness routing
<!-- harness:routing-injected -->
- On Codex, every repo-mutating request → invoke `$harness:run` before editing; it loads the internal canonical workflow, syncs a native Goal when present, and otherwise opens/resumes a Harness task
- On Claude Code, run the public cycle (task start → plan → develop → QA → close) through native `/goal` for explicit goals or the runtime's canonical task route for plain repo-mutating requests; review and task_verify remain internal close gates
- Bootstrap harness in a new project / repair existing → `Skill(harness:setup)`
- Plan-only requests → sync/create Goal and stop after the internal plan phase if the user explicitly asks not to implement
- Implement an approved PLAN.md / develop only → resume the active Goal child task through the internal develop path
- Contract drift / post-upgrade cleanup → continuous maintenance flow in the active/next Goal child task
- Read-only question or explanation → answer directly, no Harness run skill

### Durable Decision Documentation Gate

A user-stated durable decision is not handled until it is documented under `doc/`.
If the user establishes, corrects, or confirms a lasting product, design,
architecture, domain, workflow, or implementation rule, update the matching
`doc/` file before finalizing. Conversation history is not durable memory. If
no matching document exists, create one under the appropriate `doc/` area; if no
doc is needed, record the specific no-doc rationale in the PLAN durable-doc
decision.
<!-- /harness:routing-injected -->
