# Native finite replay endpoint

Finite replay plans expose their original relative playback endpoint including
preroll. The native command uses the same endpoint in Mixer and ReplayPause,
clamps display song time at the recorded end and finishes only from actual
endpoint-render execution, exact feeder admission counts and native presentation
crossing the retained physical marker. Manual pauses shift physical output;
completion preserves that actual marker and rejects changed/regressed reports,
counters, producer state and post-end state before visual publication. Missing
render/presentation evidence waits, and no extra replay operations are synthesized.
Unlimited drain behavior and explicit wall truncation retain their old semantics.
Terminal paused reports retire actual playback credits without new admission;
exact execution checks precede visual publication. The irreversible endpoint
may finish despite a pending manual pause, without requesting resume.

Actual Mixer fixtures cover partial ending, manual pause gaps, still-active frozen
voices, fractional sample thresholds, unavailable evidence and atomic refusal.
Independent code and security review passed after fixing terminal feeder credit
retirement. Documentation review passed. Independent QA ran 1,603 library,
242 application and 21 replay-command tests with no failures (2 existing ignored).
Workspace, browser WASM and Windows/macOS all-targets source checks exited 0;
cross checks used C stubs, not SDK/HAL execution. Actual help/invalid/unlimited
wall-cutoff commands behaved as expected.

An actual command with a 4-second WAV and 10,416ns finite section reached physical
endpoint frame 1, retained one frozen voice, retired all credits and reported
admitted/consumed/applied counts of 1 with zero execution errors. It exited 0
under an explicit one-second wall cutoff. This proves native endpoint execution
and retirement, not natural completion.

## Known ceiling

Known ceiling: native natural-completion QA is BLOCKED_ENV on this host. Three
commands without a wall cutoff reached an external 30-second timeout. An
independent actual AlsaStream probe found ALSA `null` in PREPARED (state 2), with
no estimated played frames or presentation pair. Completion correctly waits for
the unavailable native frontier; render/wall timestamps must not replace it.
Continue this acceptance check with a backend/device that supplies genuine
played-frame evidence. Acoustic timing, physical drivers/GPU, gapless loops and
the full player remain unverified or unfinished.
