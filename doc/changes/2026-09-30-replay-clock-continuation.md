# Durable replay and supplied clock calibration

Filled implementation gaps from plan.md: versioned bounded canonical event
serialization, durable replay logs containing runtime/header/calibration data,
and integer affine clock mapping with finite validity/uncertainty. Complete
native provenance, arbitrary raw payloads and IEEE float bits are retained;
unknown versions/tags, oversized lengths, truncated/trailing bytes and invalid
log order/domains fail explicitly. Examples can save actual admitted logs and
decode/re-encode real files at new destinations.

Section restart now has an example/test surface connecting fresh output owners
to a supplied-pair clock mapping while retaining unknown measurement error.
Native observation collection, device output reset and physical restart timing
remain application integration and verification work; no absolute sync or
hardware latency guarantee is claimed.

Scoped formatting and cargo check --workspace --all-targets passed. Tests were
authored, including codec round trips/adversarial extents and clock arithmetic
boundaries, but were not executed. Formal review, QA, benchmark and native
execution remain deferred by the user. Task and full Goal remain incomplete.
