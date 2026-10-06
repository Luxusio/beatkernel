# Preserve paused Play when opening its live-audio child

F2 previously entered LiveAudio and immediately dropped Game because route cleanup
recognized only Play/Results/Closing. A live native worker was consequently
cancelled and joined. Cleanup now follows retained Play ancestry; Closing keeps
the owner until explicit cancellation and join. Background assets use the same
ancestry so a child does not discard its parent's render resources.

The four existing live-output desktop tests failed before the change and passed
afterward. A fifth regression uses an actual worker thread and Player channel:
F2/Back twice preserves it, and explicit close cancels and joins it. All five
passed. The complete main executable suite reports 199 passed / 18 failed,
compared with the supplied baseline 194 passed / 22 failed; the four resolved
failures are the live-output group, plus the new passing test. Other failures
remain pending, not waived. Full-library test compilation initially hit an LLVM
output allocation error; targeting the main executable with two build jobs
completed and executed the tests. No desktop/GPU/device acceptance follows.

The user lifted verification deferral; the matching REQ is updated accordingly.
New parallel agents await another user parallel instruction. Formal independent
review/QA and full task close remain outstanding.
