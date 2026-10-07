# Worklet setup silence and exact armed chronology

Status: native and actual-browser development checks passed; independent final
review/QA are pending.

Before arm, WorkletAudio accepts monotonic nonoverlapping callback intervals,
including forward gaps, while generating setup silence. This corrects the
observed cold sequence frame0..128 then1664..1792. Setup callbacks never consume
queued commands or produce Mixer reports.

Successful arm commits its actual current frame as the exact next callback
frame, after every admission check passes. All armed callbacks must remain
contiguous, including the first callback and silence before the target start.
Missed start, repeated/overlapping/regressing callbacks, invalid extents and
overflow retain their failure behavior. Failed native arm admission preserves
setup state; existing browser control rejection still fences its owner.

The native suite passes 20 tests, including six new regressions covering the
observed setup gap, capacity-one queued Play, sample output from Mixer zero,
exact arm adoption, failed-admission state retention, strict armed prestart
chronology, inert zero blocks and wide/overflowing frames. A report-decoder
fixture now expects the retained successful-arm context while still requiring
an absent Mixer report before output begins.

The fresh separate audio WASM and actual Chromium repair runner passed all six
required scenarios: three healthy cold contexts and existing armed-gap,
late-arm and sample rejection checks. Every cold context naturally skipped
from setup end128 to current1664 without injection; it retained unavailable
Mixer reports across that gap. The actual arm adopted its current frame, and
the first active report began at Mixer zero without crediting setup silence.
All owned browser/server resources closed. Development evidence is under
`target/wf/worklet-unarmed-startup/browser-development/`.

Source timestamps, input-domain policy, presentation authority and delayed-input
frontiers are unchanged. This measured startup integration does not establish
broader player completion, hardware latency or universal scheduler reliability.
