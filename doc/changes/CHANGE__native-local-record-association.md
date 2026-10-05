# Native local recording result association

Local recordings need an unambiguous original player association even when every
member has the same replay header. Keep the whole-roster archive and publish
small one-row archives beside original member recordings, with an explicit ID
inside the existing canonical format. Shared pure projection and staged callback
publication let Records use the same bounded decoder/matcher as solo records.

## Known ceiling

Existing local recordings without member sidecars still need explicit selection
or migration. Publication across files is not transactional or crash durable,
and a failed write can leave a partial newly created file. Real filesystem/native
UI acceptance, authentication, race-free containment and performance remain
unverified. Fixture execution and formal review/QA remain deferred.

## Implementation evidence

ResultArchive::for_player validates the whole original archive and fallibly
copies an exact historical row. publish_cohort_sidecars encodes and stages the
whole base file and all individual member files before callbacks, rejects
duplicate destinations, attempts every staged write and returns the first boxed
callback error unchanged. The three native local composition roots select
save_cohort_sidecars; solo saving retains its original publication behavior.
Existing finalizer error precedence remains in place.

Seven independent deferred groups cover 1–64 members, sparse/max IDs, differing
actual profiles, equal headers, invalid later rows, exact callback paths/order,
first boxed error identity across multiple failures, preflight with zero effects,
non-UTF-8 native paths and linear payload size. The sum of one-row version-1
archives equals the whole encoded size plus 16 bytes per additional envelope,
rather than multiplying the whole payload by the player count.

Both writers stopped before scoped Rust formatting and diff whitespace checks.
All four compile-only checks exited zero: workspace/all-targets with
webtransport, runtime/all-targets without defaults with webtransport, wasm32
library with browser and wasm32 library with browser-audio. Existing dead-code
warnings remain. No assertions, filesystem/device/browser apps, benchmarks,
Windows/macOS target checks or formal review/QA ran.
