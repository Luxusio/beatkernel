# Actual catalog read accounting

Catalog raw-byte accounting uses actual returned bytes, including failed read,
decode and parse prefixes. Each file receives the remaining aggregate budget as
an additional raw read cap; one counted detection byte can distinguish EOF from
an oversized or growing stream. Aggregate overflow stops further reading and
never parses or publishes that overflowing candidate. Existing synchronous APIs,
parser limits, deterministic ordering and background cancellation stay intact.

## Verification state

Independent deferred fixture source adds four groups: two bounded reader groups
and two actual scanner groups. They cover UTF-8/BOM/Shift_JIS compatibility,
independent raw and decoded limits, short/interrupted/failed reads, counted
detection bytes, zero/overflow budgets, controlled growth/shrink after metadata,
open/decode/parse failures, exact 64 MiB usage, 64 MiB plus one detection byte,
retained deterministic entries and cancellation without partial results.
The scanner's Parsing checkpoint is post-read cancellation/progress evidence;
an overflowing candidate is refused before actual parser invocation. Forced
allocation failure remains unexecuted, and no fixture has run.

Both source writers actually stopped before scoped Rust formatting and
whitespace inspection. Workspace/all-targets with WebTransport (session 52127),
native no-default/all-targets with WebTransport (92776), WASM browser (6921) and
WASM browser-audio (72605) each exited 0. WASM retains three existing cadence
dead-code warnings. No tests, filesystem fixture probes, applications, generated
bindings, device execution or formal review/QA ran. These are compile-only
checks; the full player Goal and Harness task remain open.

## Known ceiling

Accounting covers bytes returned by the reader, not physical driver or disk
traffic. An extra detection byte is explicit: accepted aggregate raw usage is
at most 64 MiB and attempted usage at most 64 MiB plus one byte. Cancellation
cannot interrupt arbitrary filesystem calls or decoder work. Actual filesystem,
native window and measured performance acceptance remains deferred.
