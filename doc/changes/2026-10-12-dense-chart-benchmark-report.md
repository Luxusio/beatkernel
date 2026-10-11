# Dense chart process benchmark reporting

Status: source and independent contract tests implemented. Coordinator
development checks passed43 new/existing benchmark tests, zero failures, exit
zero; CLI help also ran successfully. Independent implementation review and
actual release collector QA have not run. Full WBS13.05/13.08 remain unfinished.

The opt-in Python collector joins the unchanged real dense-chart stress
example to the existing owned-process CPU/RSS and atomic JSON publication
helpers. It fixes two configurations, validates the original nine-line summary
against independent chart/prefix/visibility expectations, and compares
deterministic facts within repetitions of each case. It preserves binary
identity, raw child output, exact arguments and unavailable metric declarations.
The [contract](../kernel/REQ__dense-chart-stress.md) defines admission and scope.

Independent plan review confirmed existing source compatibility and required
separate u128 timing admission, per-case consistency, literal test fixtures,
source/build identity for actual release QA and bounded process-group cleanup
claims. Implementation and independent tests have separate file ownership;
the coordinator alone runs verification after both writers stop.

## Known ceiling

Whole-child CPU/RSS includes startup, chart construction, recording, independent
verification and reporting. It is distinct from the Rust seek/projection phase
timers. Wall time also includes supervisor overhead and its 10ms polling
interval; parent-side count validation is excluded from child resources.
Hash consistency does not authenticate an executable or establish its
source correspondence. Lanes are not multiplayer members, timestamp origin is
not wall-clock soak, and no GPU/audio/native/physical latency or comparative
ranking is inferred. Cleanup covers the owned process/POSIX process group,
not deliberately detached descendants. The complete player Goal stays active.
