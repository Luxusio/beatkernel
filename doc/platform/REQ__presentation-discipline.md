# Bounded presentation-clock discipline

PresentationDiscipline is a pure off-thread observer of coherent running native
clock snapshots. It takes an explicit output frame-zero ClockPoint, host domain
and applied song origin. It uses the existing checked native position/frequency
conversion. It reads no native clock and makes no device, accuracy or hardware
latency claim. Reported mapping quality is always Unknown.

Configuration validates before allocation: capacity 2..1024 (default 64),
positive retention interval, minimum span, update interval, correction horizon,
maximum observation age and maximum phase error. The configured retained span
(capacity minus one times retention interval) must cover the minimum span.
Defaults are 100 ms retention, 1 s minimum/update spans, 10 s correction horizon,
2 s maximum age, 250 ms maximum phase error and 1000 ppm maximum rate deviation.
Rate deviation must be positive and below one million ppm, ensuring all applied
rates are positive. Retained storage is preallocated and bounded.

Every progressing snapshot updates freshness; only sufficiently spaced samples
enter the retained ring. Exact duplicates and unchanged device position return
Unchanged without refreshing freshness or changing retained state. Increasing
positions require strictly increasing associated host timestamp and QPC. Domain,
frequency, regression/reset, unavailable or degraded readings reject atomically.
The helper has no implicit reset: a new stream/domain requires a new observer.

Host validation checks domain and freshness against the latest accepted progress.
Historical queries are permitted; Transport independently rejects timestamps
before its initial origin. Update calls also obey successful-update chronology.
Warmup and update-interval skips return explicit statuses without mutations.
Paused/reverse transport is unsupported for this unit-song-time discipline.

Over at least the minimum span, the oldest retained/latest accepted output and
host differences estimate the base rate. Checked i128 arithmetic rounds to the
nearest ppm (ties away from zero). Base ppm is a signed deviation from normal
1,000,000 ppm; a base outside configured bounds rejects. Desired song at the
latest observation equals applied song origin plus output offset from frame zero.
Phase is desired song minus the historical transport position at that host
observation. Excessive phase rejects. Correction ppm is phase times one million
divided by the correction horizon, rounded with the same rule. Base plus
correction is clamped to the configured positive range; Applied reports phase,
base/correction/applied ppm and whether limiting occurred.

All fallible arithmetic and policy checks precede Transport.set_rate(now).
Successful changes preserve integer song continuity, retain historical input
mapping and never seek or mutate a judge. Transport control-thread history can
grow with successive changes; bounded observation storage is not a bounded
real-time mutation guarantee. Caller mutation of transport remains caller-owned;
this helper does not identify arbitrary historical seeks. Native initialization
uses an observed frame-zero host relation with Rate::NORMAL, rather than using
a noisy short-pair inverse slope as the initial song rate.

Authored fixtures cover synthetic drift, correction signs and clamp, convergence,
continuity/history, rounded native positions/extreme arithmetic, stale and
atomic failures, warmup/decimation and invalid configuration. During the user's
verification deferral only scoped formatting/compilation is performed; no tests,
native execution, QA or review results are asserted.
