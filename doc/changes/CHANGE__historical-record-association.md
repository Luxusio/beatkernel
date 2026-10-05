# Historical record association and native preview

Saved results need an association policy that does not invent live completion
or choose a player from an ambiguous header. The shared pure matcher checks an
exact canonical replay header and optional original player ID. It returns
historical archive data and refuses missing, mismatched or ambiguous rows.

Native preview also needs the canonical section-aware decoder/reconstructor:
the legacy tuple decoder refuses finite v4 recordings produced by native play.
Preview checks the current draft's complete original extent and setup while
recomputing only accepted replay-prefix scores. Optional adjacent sidecar loading
runs on the metadata worker, and a bad sidecar leaves that valid prefix usable
with a separate diagnostic. The retained Records screen keeps historical
outcome/gauge presentation separate from prefix statistics.

## Known ceiling

Native local whole-roster sidecars use the configured base path; member-record
lookup still needs explicit archive/player selection or durable association
metadata. Header equality can be ambiguous across players and cannot safely
infer an ID. Browser loaded-result UI and rich archive statistics remain later
integration. Actual filesystem/GPU/device acceptance and race-free containment
remain unverified. Execution and formal review/QA remain deferred.

## Implementation evidence

Both paired writers returned terminal `Writes STOPPED` before scoped formatting
and compilation. Eight independent deferred fixture groups were authored:
three exact-header/original-player association groups, three native section
preview groups, and two retained Records geometry/update groups. The existing
desktop IME fixture received only defaults for new preview fields.

Scoped Rust formatting and `git diff --check` completed successfully. The exact
four compile-only checks exited zero: workspace/all-targets with webtransport;
runtime/all-targets without default features with webtransport; wasm32 library
without default features with browser; and wasm32 library without default
features with browser-audio. Existing dead-code warnings remain. These checks
do not execute fixture assertions, generated WASM, browser or native applications,
filesystem adapters, GPU code, or benchmarks, and establish no review/QA PASS.
