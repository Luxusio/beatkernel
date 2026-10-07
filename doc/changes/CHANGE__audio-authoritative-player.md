# Audio-authoritative player migration

Status: shared foundation and Step integration implemented; production native
and browser adapter migration, lifecycle integration and independent final QA
remain pending. The existing browser playback failure is not yet resolved.

The core can map two supplied clock observations with explicitly unknown
accuracy, without inventing an error bound. The application's finite authority
joins original acquired HOST input with raw output observations on a separate
logical audio timeline. Shared solo/local Step owners use normal-rate audio
Transport, preserve input provenance and raw scheduling points, and commit
genuine Runtime reports before fallible score/gauge/capture observers. Generic
HOST/PLL entry points reject these new owners before mutation. InputMerger adds
explicit cold-preallocated registration of up to 4096 actual sources while
preserving fixed local rosters, ordering and pending budgets.

Focused evidence: core calibration 11 tests; authority 16; genuine Step audio
integration 11; source registration 7; existing Step regression 119 and input
regression 22. All pass. Native all-target and browser WASM checks pass with the
new shared code. These overlapping filters are not summed as a whole-suite
count. The latest full workspace run belongs to the earlier source revision.

## Known ceiling

Current production BrowserGame/BrowserLocalGame and native pumps still need to
select the new authority and join actual output evidence with acquired prefixes.
Generated frames cannot replace presentation. Browser touch projection and
page-change barriers must retain acquisition geometry through pending input.
Generic estimator APIs remain available for explicit legacy consumers; this is
not permission to keep them as BMS play-time authority. Real browser/native QA,
output replacement/resume, replay compatibility and measured latency remain
required before this migration is complete. No hardware or osu superiority is
established by pure fixtures or type checks.
