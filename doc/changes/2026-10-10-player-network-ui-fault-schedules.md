# Player network and UI fault schedules

The portable room controller now records a fatal immediate UI reply failure,
closes controls and stops processing later actions in that service pass. Temporary
contention retains the exact reply without repeating its network effect, matching
the existing delayed-reply policy. Seven connected tests exercise actual
Runtime prefixes through publication, client/relay wire receipts and room UI
coordination; the [fault isolation contract](../runtime/REQ__player-fault-isolation.md)
records the real red reproduction and final verification evidence.

## Known ceiling

Injected portable I/O and bounded schedules do not prove native sockets/devices,
GPU/browser/desktop rendering, acoustic timing, real callback cessation or the
remaining allocation/short-write/spawn/panic fault matrix. Full player and
WBS13.04 acceptance remain open.
