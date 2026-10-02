# Common song target with individual prerolls

Network startup now agrees a common nominal song-start target while permitting
different native prerolls. Previously a common software-start call followed by
different prerolls produced different nominal song times. Wireversion6 carries
the participant's nonnegative preroll nanoseconds in its8-byte clock-ready
payload. The host proposes `now + lead + max(local_preroll, peer_preroll)` as
the common song target. Each participant subtracts its own preroll to obtain its
software-start target. No matching output rate, buffer or preroll is required.
Versions1..5 are incompatible; the existing proposal/accept/commit barriers apply.

The join participant retains both converted clock-offset endpoints before
subtracting its preroll. Earliest remaining lead, observation age and uncertainty
checks apply to the software-start target; its midpoint determines release.
Schedules retain the local nominal song target separately from the software
target and preserve the selected interval width. Checked wide arithmetic rejects
negative geometry, overflow and deadlines too close to allow preparation.

Linux ALSA, macOS CoreAudio and the shared Windows WASAPI/optionalASIO solo path
now pass actual native `--preroll-ns` through native competition preparation.
Invalid negative preroll rejects before replay files or socket acquisition.
Preroll does not change replay identity or judgment time. Offline/ghost-only
paths retain local startup, and local cohorts remain offline. Library callers
can select nonnegative i64 preroll through the new model/native preparation
entry points; legacy model construction uses zero preroll. The overall setup
timeout must allow lead plus any delay caused by different prerolls.

Inline fixtures prepare verification of different prerolls in both directions,
signed/asymmetric clock offsets, zero and long spans, overflow, negative peers,
full-write commits, exact wire length and early native validation. Fixtures are
authored/compiled only; execution and formal acceptance remain deferred.

Compile-only evidence: runtime all-target checks for WindowsGNU and macOS,
runtime all-target `--no-default-features`, and the WASM graphics library check
reported exit0. The workspace all-target check reported Cargo's finished dev
profile; its handle was absent on the subsequent poll, so a numeric exit was
unavailable and the command was not restarted. Scoped rustfmt and
`git diff --check` completed. Existing macOS block0.1.6 future-compatibility and
WASM cadence dead-code warnings remain. No fixture/socket/device execution or
formal review/QA/acceptance gate was performed.

## Known ceiling

Known ceiling: These targets align nominal software-start time plus preroll.
Backend start latency, hardware output-zero, sample quantization, OS scheduling,
clock drift and physical presentation are not measured or corrected here.
Native calibration/presentation pairs still establish each local transport;
changing judgment timestamps without aligning actual audio would conceal drift
and is not done. Explicit output-zero targeting and later device/socket execution
remain required. Disconnects can still produce asymmetric start commitments.
