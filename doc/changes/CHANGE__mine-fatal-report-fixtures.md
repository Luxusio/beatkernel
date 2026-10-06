# Verify fatal mine sound-stop and exact capture prefixes

The older damage and sound fixtures predated the gameplay failure sound-stop
policy. They expected no Stop commands at a fatal mine and treated later input
and empty advances as continuing recorded gameplay.

The queue-full damage case now verifies the complete original report: one failed
ordinary Play followed by one failed Stop for each of the three prepared note
voices, all with Full admission diagnostics at the actual fatal output time.
Committed hazard damage and ordinary judgment score are still checked separately;
subsequent technical-failure refusal and the retained capture remain asserted.

The audible solo case expects exact ordered Stop commands for all five prepared
note/invisible/mine voices at the fatal output time, no new Play and no audio
failure. Shared BGM voice 90 stays outside this per-player stop list, as required
by the sound-stop ownership contract. Numeric gauge termination with accepted stops is distinct from technical
session failure. Local disjoint voice and original-bank checks remain intact.

The display-time replay case retains the fatal chart and expects exactly its
three-record prefix (Down, duplicate Down, fatal Advance). Later source mines
remain unconsumed, so avoided count stays zero. It continues to compare live,
visual replay and stepped replay totals under repeated, later and regressing
display targets; no display-time judgment or post-fence capture is introduced.

Only fixture expectations are changed. Production stop scheduling, capture
fencing and numeric/technical failure admission rules remain unchanged. Portable
common-owner evidence does not establish native acoustic stop latency. The broad
task remains open with independent review and required QA pending.

Verification (2026-10-06): full runtime library with webtransport reports
1519 passed, 49 failed against the preceding 1516/52 baseline. Exactly the three
modified fatal damage/sound/display-time replay cases resolve, with no new
failing names. All changed fixtures compiled and executed. The full command
still exits 101 for remaining failures; the task is not PASS or closed.
