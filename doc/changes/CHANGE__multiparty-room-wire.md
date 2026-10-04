# Multi-host room admission wire

The common application codec adds a distinct BKMR version 1 envelope for room
admission, assigned participant IDs, complete membership snapshots and explicit
seal/preparation/leave requests. Ordered player IDs remain scoped by their
network participant. Requests carry no self-selected participant lease; a
server must bind each request to its admitted stream. The bilateral BKMP
session and existing two-stream relay remain unchanged.

The incremental decoder admits one bounded frame at a time, retaining partial
input and refusing malformed headers or complete messages. Room snapshots
validate every host, roster, preparation bit, phase and deadline before being
returned. Wire state is neither authorization nor a song-start, write or ACK
receipt.

The decoder initially reserves only its 11-byte header. Once that header is
validated, the next body fragment reserves the full bounded remainder, allowing
later fragments and subsequent frames to reuse capacity. This avoids repeated
buffer growth on tiny transport fragments without allocating an untrusted body.

The envelope is `BKMR`, little-endian u16 version 1, a one-byte tag, and a
little-endian u32 payload length. Its header is 11 bytes and the maximum payload
is 65797 bytes. Room selection belongs to the adapter's validated stream
context, such as the WebTransport request path.

| Tag | Message | Payload |
| --- | --- | --- |
| 1 | Join | u32 identity length, 1..65536 identity bytes, u8 local count, u32 player IDs |
| 2 | Admitted | One positive u64 participant ID |
| 3 | Snapshot | u8 phase, i64 deadline, u8 host count, ordered host rows |
| 4 | Seal | Empty |
| 5 | Ready | Empty |
| 6 | Leave | Empty |

Every integer is little-endian. A snapshot host row contains u64 participant ID,
strict u8 preparation boolean, u8 local-player count and u32 player IDs. Both
counts are bounded to 1..64. Phases are 0 collecting, 1 frozen and 2 prepared.
The first two retain a nonnegative deadline; prepared uses exactly -1 for none.
Collecting contains no prepared members; frozen requires at least two members
and some not prepared; prepared requires at least two members, all prepared.
Participant IDs are unique across rows; player IDs are unique within each row.
The first row retains the creator's sealing authority.

Production and six independent deferred fixture groups are authored. They pin
literal wire bytes, maximum identity and scoped rosters, malformed frames and
messages, fragment/coalesced ownership, retained failures and capacity reuse
under maximum-size single-byte fragmentation. No fixture has been executed.
Scoped Rust formatting and four final locked compile-only checks completed
successfully after the buffer-allocation correction: workspace all targets
(44242), headless app all targets (4963), WASM browser (45307), and WASM
browser-audio (22550), all exit 0. Existing WASM audio cadence dead-code
warnings remain. Initial check results were superseded by this source fix.
Linux/WASM checks do not establish active Windows/macOS or device acceptance.
Runtime
execution, formal reviews/security and required browser/CLI/desktop QA remain
user-deferred and mandatory before eventual close. Actual server/client room
negotiation, shared start, bounded fanout/progress/final ACK and native/browser
integration are unfinished. The player Goal and task remain active/open.
