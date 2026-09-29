# Runtime critic playbook

- Run the declared Cargo checks and confirm their exit status and expected output.
- Verify examples against the acceptance criteria, including timestamps and input provenance.
- Check integer overflow, input ordering, device lifecycle, and deterministic behavior when affected.
- Verify real-time paths avoid allocation, locks, and I/O where the API promises that behavior.
- Label virtual fixtures and platform stubs accurately; require hardware evidence for native backend claims.

PASS when evidence proves both operation and the user's intended behavior. Keep missing runtime attestation separate from substantive verification results.
