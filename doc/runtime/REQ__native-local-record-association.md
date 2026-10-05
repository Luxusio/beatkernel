# Native local recording result association

Local completion retains the whole-roster archive at the configured replay base
path. Each original member replay additionally receives an adjacent `.bkresult`
archive containing exactly that member's original nonzero u32 ID, canonical
header, actual gauge profile and historical result. These bounded one-row
archives use the existing version-1 format. Equal headers across players cannot
cause ambiguity because the member sidecar explicitly preserves the original
ID. Lookup never infers IDs or parses suffixes from selected filenames.

Pure archive member projection preserves historical data without constructing
CompletedPlayResult. Zero/unknown IDs refuse projection. Publication prepares
all bounded member projections, canonical bytes and destinations before any
filesystem write; invalid paths or projection/encoding failure have no effect.
Whole and member sidecars use exclusive create, never overwrite existing files,
and attempt every prepared destination even after an earlier write failure.
The first exact publication error is returned after attempts. Existing finalizer
precedence continues to retain the original typed gameplay/cleanup/replay error.
Only actual complete original-roster evidence authorizes this publication path;
cancelled/prefix-only play creates no completed sidecar.

The sum of member archive payloads grows with the original roster's actual
headers and profiles. Each member file contains one row, not a duplicate entire
roster. Original OsString parent/name handling is retained on all native hosts.
Linux/Windows/macOS composition roots use the same shared policy and outer
filesystem adapter. The Records metadata worker uses existing bounded whole
archive decoding and exact association, so no new rendering filesystem effects
or per-note allocation/dispatch/lock are introduced.

Independent deferred fixtures cover sparse/max IDs, equal headers, exact member
projection, invalid IDs and preflight, callback attempt/error precedence,
non-UTF-8 filename construction and historical header association. Tests, filesystem
and platform applications remain deferred. After both writers stop, scoped
formatting and the exact four compile-only checks establish compilation only.

## Known ceiling

Already-saved local recordings without member sidecars still need explicit
archive/member selection or migration. Sidecar publication is not transactional
across files; partial exclusive-created files can remain after write/flush
failure. Crash durability, authentication, symlink races, actual native file/UI
acceptance and performance remain unverified. Rich archive statistics are
separate work.

## Implementation evidence

Both paired writers returned terminal `Writes STOPPED` before scoped formatting
and the four compile-only checks. Workspace/all-targets with webtransport,
runtime/all-targets without defaults with webtransport, wasm32 library with
browser and wasm32 library with browser-audio all exited zero. Existing
dead-code warnings remain. Seven deferred fixture groups were authored: three
pure member projection groups and four injected publication groups, including
the Unix non-UTF-8 path case. These compile checks execute no assertions, file
adapter, native/browser application, benchmark, formal review or QA.
