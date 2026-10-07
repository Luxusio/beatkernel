# Explicit BMS judgment policy and EX projection

Resolved BMS play policies retain opaque-grade meanings in a bounded allocation
free table. Checked score projection reports PGREAT/GREAT/GOOD/BAD/POOR and EX
points from actual committed stage counts, without inferring classes from gauge
deltas. An opt-in policy capture constructor records canonical class identity
around complete gauge/section options. Section decoding and reconstruction retain
and validate that identity; legacy tuple decoders refuse to discard it, and
record comparison distinguishes classified policies. Unclassified recordings
retain their existing bytes and no inferred EX meaning.

Verification: full library tests passed 1,635 with two ignored; executable
regression suites passed 359. Independent QA passed five judgment-policy and
five play-policy tests, including actual Runtime/capture/ReplayVisual for all
six gauges, plus four existing record-catalog regression tests. Workspace
all-target/webtransport and WASM/browser checks exited successfully. The
classified catalog equality guard was reviewed in source; direct classified
catalog selection is not claimed by these regression fixtures. Independent
DEEP code and security reviews passed. Local ignored logs are under
`target/wf/explicit-judgment-*`.

## Known ceiling

Native launchers currently use gauge-only capture. Propagation into live UI,
native/member admission, result archives and multiplayer remains integration
work under the full active Goal. Generic stage combo semantics stay unchanged;
this projection does not implement historical judgment windows, empty POOR,
mine score rules or full LR2 compatibility. Physical/native acceptance and
full task closure are not established by these domain and replay primitives.
