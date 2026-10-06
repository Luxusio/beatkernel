# Commit resumed clocks only after genuine device seeding

The actual common solo and cohort loops must stage the resumed Transport and
presentation owner before replacing either. Keep the current paused transport,
its historical anchors and current presentation owner if origin calculation,
construction, epoch restoration or device seeding fails. Preserve original
typed device errors. Seeding a temporary owner and then failing cannot publish
that owner's observations. A successful seed must leave an accepted latest
pair; a no-op success without evidence is refused before clock publication.

After all fallible preparation succeeds, replace the transport and presentation
owner and update the local resume cutoff/pause bookkeeping together, with no
new fallible effect between these commits. Original live-pause preparation,
input/capture ordering, same-stream epoch/config preservation, device evidence
and successful resume behavior remain unchanged. Business policy uses static
ports and contains no platform-specific branches or invented audio samples.

This guarantees software clock publication order, not reversal of native
audio effects: the device may already have resumed and NativePause may already
have acknowledged that boundary. Those effects cannot be undone by assigning
old clocks. Errors propagate to existing caller cleanup. A full live backend
handoff/recovery controller, callback fencing and failed-open rollback remain
required. No whole gameplay rollback or acoustic timing guarantee is claimed.

Author fault-injected actual solo/cohort memory-pump cases using original core
presentation and capture/PCM evidence, including seed failure after temporary
admission and seed success without observations. Verify preserved paused
transport/history/epoch/config, original error identity and no post-resume
judgment or reconciled release publication. Assertions/runtime/formal review/QA
remain deferred; scoped Rustfmt and four sequential compile-only checks follow
both paired writer terminal stops. Full BMS Goal remains active and incomplete.
