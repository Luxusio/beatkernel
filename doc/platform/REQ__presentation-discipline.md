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

## Explicit supplied clock pairs and Linux conversion

`observe_clock_pair` admits caller-owned output/host ClockPairs into the same
bounded retention and continuous correction logic. Both declared domains must
match construction. Source provenance is explicit: WASAPI observations retain
native frequency/position/QPC, while supplied pairs contain no fabricated
native counters. An observer cannot mix the two sources; it rejects with
ObservationSourceChanged and retains all prior state. A new source requires a
new observer.

Supplied pairs reject output or host regression. Exact duplicates and unchanged
output with nondecreasing host return Unchanged without refreshing accepted
progress. Increasing output requires strictly increasing host. The first output
point may be zero; no WASAPI nonzero-position requirement is invented for a
supplied pair. Admission errors occur before retention/freshness mutation, and
all existing correction, continuity, freshness and Unknown-quality rules apply.

The Linux `alsa_presentation_pair(snapshot, output_origin, sample_rate)` helper
uses the native monotonic timestamp and checked estimated played-frame count,
not submitted/rendered position or userspace receipt time. A nonzero explicitly
supplied applied sample rate is required. Missing native association or played
estimate returns None. Because snapshot fields are public, a provided estimate
and association are independently checked against native RUNNING, exact applied
ENABLE/MONOTONIC configuration, Unknown quality, native timespec conversion,
query-domain/order metadata and submitted-minus-nonnegative-delay arithmetic.
Contradictory provided fields return InvalidConfiguration, not a trusted pair.
The native timestamp may precede the query bracket; that is not rejected or
replaced. Age validation remains caller owned.

Output source time is output_origin plus floor(played_frames * 1e9 / rate),
computed in checked i128 and narrowed only after adding the origin. A valid zero
played-frame estimate maps exactly to output frame zero. Representational
failure returns Overflow; no saturating timestamp is manufactured. The helper
performs no native calls and makes no acoustic accuracy claim. Generic pair,
source-provenance and ALSA frame-grid fixtures are authored but not executed.
