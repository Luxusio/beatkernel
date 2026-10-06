# Align Worker lifecycle/cadence fixtures with delegated ownership

Replacement tests now verify that a new gameplay binding is not constructed
before the previous room's actual asynchronous cleanup joins. After the held
write/read/open completes, they still check the new owner stays untouched by old
continuations. Existing RPC settlement, capture and one-time close/free checks
remain intact.

The fake WASM publication boundary retains its last accepted elapsed marker and
final-prefix admission. Its default due hint follows the existing 250 ms cadence;
scripted hints/admission refusals still override it, and rejected admission never
advances the marker. This lets real Worker scalar-before-word acquisition tests
exercise suppression, rather than an always-true fake hint. Rust cadence policy
is independently tested; the script is not its implementation proof.

Timeout tests explicitly advance the controlled clock to the 10-second deadline
before invoking the actual setup or 1 ms drain polling callback. Drain cases
check the elapsed argument passed to the WASM boundary and current wrapped drain
failure message, retaining transport/cleanup separation and completed local
capture evidence.

Room Results setup now forwards its requested roster/page through the real
commit helper. Successful disposal is checked with actual game/session free
counts, rather than a success-message field that is absent from the current wire
shape. Failure messages still retain their declared released=false evidence.

Only test scripts/expectations change. Production owner joining, publication
cadence, deadlines and terminal messages remain unchanged. Real WASM, browser,
WebTransport and whole-task QA remain unfinished.

Verification (2026-10-06): Worker tests with experimental VM modules report
107 passed, 2 failed versus the preceding 99/10 baseline. Exactly eight existing
failures resolve and there are no new failing names. Remaining cases concern
local saved setup and pointer preparation ownership. The command still exits 1;
no full Worker or task PASS is claimed.
