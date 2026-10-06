# Require accepted presentation evidence for output resume

GameplayOutputOwner::seed_resume previously returned success when its backend
returned success without admitting any clock pair into the fresh observer.
It now requires an accepted latest pair before reporting successful seeding.
The existing staged pump transaction can therefore refuse before committing
resumed transport and presentation ownership. Backend errors retain their
original error path; no clock point is fabricated and no observation is relabelled.

A real Mixer/backend regression first failed with the old source. It now proves
a quiet successful backend poll is refused, live observer/transport anchors and
physical pause remain unchanged, and the subsequent genuine pair permits a seed.
All eight output-owner tests executed and passed. Seven real owner/controller
solo/cohort and correlated UI bridge integration tests passed. Workspace
all-targets WebTransport compilation and whitespace checks exited zero with only
existing unused-code warnings. Actual native device/acoustic acceptance is not
established.

The separate waiting-expiry issue remains pending: old committed observations
must not be refreshed or have their age rewritten. A later fix must distinguish
a genuinely suspended published-output clock during held replacement from an
ordinary active/queued/reply-pending output, preserve input domain/chronology and
frozen playback, and let the controller enforce its explicit wait budget in both
actual pumps. A generic pending flag alone is insufficient to bypass freshness.
Full independent reviews, QA and Goal completion remain outstanding.
