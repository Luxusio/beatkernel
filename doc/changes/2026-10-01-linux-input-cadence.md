# Native Linux cadence source composition

The portable benchmark used generated cadence points; native inspectors printed
events without interval percentiles. Added `linux_input_cadence` with explicit
evdev keyboard usage/state or hidraw report ID, nominal nanoseconds and finite
measurement duration. Actual selected timestamps feed bounded interval and
delivery-age observers. Loss barriers and order/time regression terminate the
segment; source provenance, native counters and retained-tail summaries print
after acquisition closes. Exact losses and physical latency remain unknown.

See the [measurement contract](../platform/REQ__linux-input-cadence.md).
Rust 1.98.1 locked workspace all-target compilation passed; the example was not
run. Native measurements, tests, independent review and QA remain deferred.
