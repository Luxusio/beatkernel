# Six-style Runtime generalization

The additive core example combines absolute or relative axis tracking, two
touch contacts, repeated fresh presses, held prerequisites with a trigger,
pointer tracking with a separate button instant, and caller-owned
orientation-dependent pose tracking through the existing Runtime and replay.
The original seven-object builders and default executable remain available;
`--six-patterns` runs eight objects for each axis mode. Game-specific pose rules
remain in the example, using public evaluator and snapshot hooks.

## Development evidence

Current focused execution passed all 12 derived-pose tests and all 19 legacy
generalization tests. The actual `--six-patterns` route produced eight literal
hits for both modes; `--fixture` retained seven objects. All seven connected
six-style tests also passed, including separate positive/negative outcomes and
active snapshot reconstruction in both modes. Direct default/help/invalid CLI
checks passed 3/3. Logs are under
`target/wf/six-pattern-runtime-generalization/`.

Independent DEEP code review found no defects. QA on `f3e7c8f` passed the full
core suite: 498 tests, zero failures/ignored, including 16 doctests. Release
focused suites passed 38/38 (12 pose, 7 six-style, 19 legacy). Strict core
all-target Clippy, all four changed Rust files' formatting and base whitespace
checks passed. Debug and release direct CLI checks passed 12/12; both profiles
produced matching mode-specific replay hashes. Independent artifacts are under
`target/wf/qa-cli-six-patterns-01a12342-1/`.

WBS04.12 now records the full original six-style software acceptance: 87/193
verified, 106 remaining. Final documentation review and Harness verification/
close follow this evidence. The full BMS player Goal remains active.

## Known ceiling

Known ceiling: software fixture only — upgrade when actual device integration
is authorized.

The overall player Goal already authorizes native integration. This child proves
software composition only; real game compatibility, physical device behavior,
acoustic synchronization and platform/browser performance require their own
existing WBS evidence. The quoted implementation ceiling does not introduce a
new approval requirement.
