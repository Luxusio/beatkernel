# Restore mine pipeline fixture admission and observable PCM checks

Assigned local players now receive bindings for their exact device, as required
by resolved input ownership; unassigned solo tests retain Any-device bindings.
The source-only completion fixture retains its command producer for the entire
scope instead of dropping it via a wildcard binding before presenting output.
Connected-output completion admission remains strict. The completion case
advances one nanosecond past the inclusive last marker before checking the
subsequent drained output frontier; it does not claim completion at that marker.

Practice selection retains both original mine markers (1 s and 2 s), including
their ordinals, rather than deleting the earlier marker at a 1.5 s start. Existing
fresh-owner hazard/capture/audio assertions still verify the later 2 s marker.
The mine sound semantic-identity mutation selects a genuinely different lane;
parser sorting had made the old hard-coded index select the same lane. The
capture-identity test explicitly stops acquisition before exporting its accepted
prefix, as required by the current replay export lifecycle; header and source
compatibility assertions remain intact.

The 1/2/64-player shared-bank fixture uses VOLWAV 1.5625 percent (exact 1/64 gain)
to keep the aggregate PCM below clipping. Expected note and mine amplitudes now
retain each player's observable contribution. Unique voice IDs, command counts,
original bank use and two asset reads remain asserted. This is stronger than
merely expecting clipped output at 64 players, which could hide missing voices.

The native occupancy fixture anchors transport one nanosecond earlier with song
-1 at host ORIGIN-1. Existing mappings at and after ORIGIN remain unchanged, and
its deliberate pre-mine press is now inside the declared transport timeline.
Core BeforeOrigin rejection is preserved.

Only fixture preparation/expectations are changed. These portable actual-mixer
and common-owner tests do not establish hardware or browser input acceptance.
The broad task remains open with required independent review and QA pending.

Verification (2026-10-06): full runtime library with webtransport reports
1516 passed, 52 failed versus the preceding 1510/58 baseline. Exactly six existing
failures resolve: shared 64-player PCM, three loaded mine pipeline cases, audible
mine capture identity and native pre-mine occupancy. No new failing names appear.
All changed fixture paths compiled and executed. The command still exits 101 for
remaining failures, so no full-project or task PASS is claimed.
