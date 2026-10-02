# Shared native solo finalization

Linux, Windows and macOS solo sessions now use native_finish::finish_solo after
their existing native cleanup attempts. One owner finishes optional competition,
attempts capture publication even after gameplay/cleanup failure, and returns
the original first error in gameplay/output/input/save order. Native diagnostics,
resource retention, cancellation classification and boxed error identity stay
unchanged. The exclusive-create capture publisher moves unchanged into the same
module and remains available through the cohort module's compatibility export.
Local cohorts retain their existing all-members finalization policy.

Three prepared fixture groups cover all sixteen combinations of gameplay,
output cleanup, input cleanup and save failure, including original error
identity and exactly-once publication; disabled capture with a supplied path;
and actual Runtime hit/advance capture with section, seed, encoded bytes and
replay reconstruction preserved through failed finalization. Publication is
injected in memory. Fixtures were compiled, not executed.

## Compile evidence

All five first cargo checks completed with exit zero: Linux workspace/all
targets, Windows GNU app/all targets, macOS app/all targets, headless app/all
targets and WASM graphics library. Evidence is in target/ac156-{host,windows,
macos,headless,wasm}.exit. Scoped rustfmt/diff checks passed, and source comparison
confirmed the relocated capture publisher is unchanged. Existing macOS block
future-compatibility and WASM cadence warnings remain. These checks do not
establish SDK-enabled ASIO or runtime acceptance.

## Known ceiling

This extraction does not establish file publication, socket cleanup or native
device acceptance. Native cleanup operations and remaining stream resource
composition stay with their platform owners. ASIO manual pause, broader player
and browser functionality, performance and actual end-to-end acceptance remain
unfinished. No new crate, dependency or licensing change is introduced.
