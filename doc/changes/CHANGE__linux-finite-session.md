# Linux solo finite native playback

Linux solo playback accepts --end-ns strictly after --start-ns. Setup maps the
original endpoint and preroll once to an immutable mixer frame end, and installs
the same original timestamp in Runtime. Finite NativePause accepts validated
terminal render evidence alongside manual pause/resume; a resume ending in one
block recovers its actual manual gap from the retained physical endpoint.

The owner stops supplying BGM when output reaches the fence, waits for actual
native presentation and drains evdev input. Inputs at/after the acknowledged
terminal host boundary do not enter gameplay. Pending resume is reconciled before
completion, and the safe lag watermark must reach both that boundary and the
logical endpoint. Transport discipline, provenance, input-loss errors, capture
prefix and native stop/join cleanup remain active. The owner neither waits for
frozen voices/commands to drain nor fabricates remaining notes or full scores.
Default playback still uses whole-song completion. Diagnostic cutoff and cancel
can end earlier with a valid prefix.

Known ceiling: Linux solo and local cohorts now support finite playback without
network competition; Windows/macOS and UI practice-region intent still
need corresponding endpoint ownership. Those combinations reject the new finite
option instead of silently ignoring it. A first actual clock observation after
the endpoint was already presented cannot supply a missing lower bracket and
fails explicitly. Native host interpolation has Unknown physical accuracy;
gapless restart and acoustic precision are not claimed. Portable actual-mixer
pause/end compositions, malformed evidence, parse/mapping and completion-frontier
fixtures are authored/compiled only. Native device/GUI/acoustic acceptance stays
separate and unverified; execution and formal close gates remain deferred.
