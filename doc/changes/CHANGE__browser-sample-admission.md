# Bounded direct sample-upload admission

## Problem and owner contract

Removing the whole-bank timeout permits larger valid sample banks, but also
leaves Window waiting indefinitely when Worker never adopts its endpoint.
Keep a 10-second deadline only until one correlated play-samples-admitted notice
proves that the same Worker owner adopted AudioSampleClient and launched its
first bounded sample or end request. Thereafter the actual client times each
sample/end ACK independently. No whole-bank timer or per-sample Window progress
is introduced; PCM and audio command buffers remain off Window.

Admission preserves the pending setup RPC. Only the final exact count/bytes
samples-uploaded receipt after genuine insertion and EOS ACKs permits finish
and command handoff. Duplicate or malformed current admission and premature
success fail and join cleanup. Stale identities cannot affect current timers,
and cancellation clears the pending waiter before replacement. Empty banks
still adopt and launch the bounded end request before admission.

## Evidence and limits

Both producer and independent author returned actual terminal STOPPED finals.
Source inspection confirms the initial admission timer, pending count snapshot,
correlated one-time notice, premature success rejection and unchanged individual
client deadlines. Worker checks first sample byte bounds and detached backing
before reporting admission, without PCM copying or value scanning.

Three deferred Host groups were added (104 total); all101 Worker groups remain,
with the existing four direct-upload groups extended for actual notification
order, exactly one notice, initial failures, EOS waiting and cancellation.
Host fixtures cover missing admission with normal or forced cleanup, malformed
and duplicate notices, premature success, stale identities and replacement.
The prior202 Host/Worker groups are preserved. Whitespace inspection reported
no diagnostics. These are authored source fixtures, not executed results.

No JavaScript
parsing, test/assertion, browser/app/audio/device/network or generated-binding
execution is authorized for this slice. Rust is unchanged; no Cargo checks.
Formal code/security review and browser/CLI/desktop QA remain required before
eventual task close. The full BMS player Goal remains active.
