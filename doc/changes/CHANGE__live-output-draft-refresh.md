# Refresh unchanged reopened audio drafts from applied state

Reopening LiveAudio during an in-flight replacement previously retained the old
applied values even after success, because reply delivery only matched the old
child's request ID. The draft now records its loaded values. Successful session
applied metadata can refresh an unchanged active reopened draft without adopting
the old request identity or notice. Independently changed values/editor and new
child messages remain intact; old-child failures do not become new-child errors.
Refresh stages bounded settings and editor before assigning them together.

The clean-reopen regression failed before the fix. All seven live-output desktop
tests passed afterward, including dirty-draft preservation, old-child refusal,
IME/pending hit fencing, retained worker and close lifecycle. Full-main execution
reported 201 passed / 18 failed, all failures present in the prior baseline.
Workspace all-targets WebTransport compilation and whitespace checks exited
zero with only existing unused-code warnings. No GPU/native device acceptance is claimed;
independent reviews and full QA remain pending.
