# Split-operation room startup through shared waiting

The actual RoomClientDriver::take_start path owns RoomStartWaitState and advances
one finite step over its existing RoomPlayClient. Port observations obtain a
schedule only through genuine take_schedule after the protocol's real commitment
barrier. A transient owned Option preserves the original StartSchedule value
until the shared state returns Ready; extraction consumes it exactly once.
Pending returns None and does not block, park, create timers or add allocations.
Ready repeats without fetching another schedule. No boolean, receipt metadata
or remote self-report can fabricate a committed schedule.

A successfully requested Leave cancels pending startup before schedule extraction
and later take_start calls return None while the owner remains healthy. Driver
failure/close refuses through the existing first-error guard before successful
historical state can leak a schedule. Close releases finite state and cached
value alongside the actual protocol owner. Errors retain existing RoomPlayError
classification; the common state's service uses an infallible local continuation.
No reset or copying of the wait-state authority is exposed.

The existing BrowserRoomClient::take_start binding and Worker observation path
use this driver API unchanged, including exact BigInt original target/song-target
and uncertainty conversion, one-shot callback and actual write/receive evidence.
No new Window work or rendering loop is introduced.

## Known ceiling

JavaScript still owns admission/prepared timers, callback/Promise lifecycle and
cleanup. RoomNetworkActor/RoomCompetition browser integration and full BMS player
requirements remain unfinished. Pure fixtures are authored but not executed;
browser binding generation/execution, actual networking/scheduling/physical sync,
performance, formal review and QA remain deferred. Compile success is not
end-to-end start or complete layer/test reliability evidence.

Three additional pure fixture groups (thirteen driver groups total) are authored
but unexecuted. Four compile-only configurations exited zero after both writers
stopped; see [the evidence scope](../changes/CHANGE__room-driver-start.md).
