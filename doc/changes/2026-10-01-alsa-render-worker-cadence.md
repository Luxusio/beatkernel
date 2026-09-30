# ALSA render-start scheduling measurements

ALSA previously exposed xrun and applied buffer settings without direct
render-worker interval summaries. It now captures actual pre-Mixer monotonic
timestamps and successful render frame identity in a fixed 4096-point prefix.
After join, checked frame-derived residuals report signed extrema and absolute
percentiles with explicit unretained measurement counts. The Linux native audio
example prints the summary beside real native counters; startup fills remain
included and no acoustic/delivery jitter is inferred.

Locked Rust 1.98.1 workspace all-target source compilation passed, including
three authored arithmetic/capacity/chronology fixtures. No tests, examples,
native measurements, review or QA ran. The [contract](../platform/REQ__alsa-render-cadence.md)
records the measurement boundary and remaining acceptance.
