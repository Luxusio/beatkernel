# Actual WebTransport room software start

Group-mode WebTransport server control ownership composes the actual prepared
registry roster, common clock exchanges and one shared start coordinator. Whole
read and write events retain their original elapsed observations before entering
the bounded actor channel. Additive common observation APIs separate captured
sample time from current processing time; valid queued reads keep original t1/t3
while deadline admission remains conservative. Exact opaque complete-write IDs
and one bounded early matching Accept/Commit response preserve the real write
barriers without rejecting legitimate asynchronous notification order.

Prepared snapshots precede controls. A fixed Prepared handshake deadline uses
the existing setup duration until all real Commit writes complete. Backpressure,
protocol failure, expiry, disconnect and stop release the exact affected room
and preserve unrelated/replacement leases. Deferred fixtures will cover actual
production helpers and original timestamp/receipt/cancellation evidence. Source
and compile-only checks do not prove TLS, native/browser or output acceptance.
Browser/native adapters still need migration to the composed client; gameplay
activation, participant progress and final acknowledgements remain unfinished.
