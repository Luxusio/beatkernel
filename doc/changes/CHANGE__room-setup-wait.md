# Common setup deadline arithmetic and browser phase waiting

RoomDeadline supplies fixed signed-nanosecond expiration arithmetic to native
actor setup checks and the browser staged RoomSetupWaitState. The existing
native overall setup scope and browser admission/lobby/prepared scope remain
explicit. Browser lifecycle transitions use actual Rust protocol evidence;
active deadlines precede transitions and commitment acceptance, preventing
late callback scheduling from turning expired history into timely startup.
The Worker schedules a whole remaining delay and asks Rust again when woken,
instead of declaring timeout independently. IO credit and cleanup ownership
remain adapters.

## Known ceiling

Native actor drain arithmetic is unchanged; the existing native and browser
timeout scopes are still distinct. Shared
complete RoomNetworkActor/RoomCompetition browser orchestration and broader BMS
player requirements remain unfinished. JavaScript owns callback/Promise lifetime,
frame timers and platform cleanup. Unit/adaptor fixtures, source compilation and
text inspection do not establish actual networking, scheduling, sync or measured
performance. Test execution, JS parsing/generated bindings/browser/native
acceptance, formal review and QA remain deferred.

Seven pure deadline/phase fixture groups, two added actual driver groups (fifteen
total) and five added Worker-owner adapter groups are authored but unexecuted.
Pure tests cover bounds, overflow, long/exact integer times, inclusive expiry,
phase transitions, deadline-before-evidence and sealing. Driver tests retain real
late admission history while refusing timely success and observe genuine Commit
without consuming its schedule. Adapter tests script WASM outputs for timer
control, callback ordering, malformed results and cleanup/completion guards.
Legacy mock compatibility is not an independent protocol proof.

Browser admission is anchored to elapsed origin 0 and ends at participant plus
first real snapshot; Admitted alone remains pending. Completed setup skips its
extra clock/port calls during gameplay IO. Native setup still uses original
overall bounds and IO error precedence; native drain arithmetic is unchanged.

Both writers stopped before scoped Rust formatting and four sequential
compile-only checks, all exit zero: workspace/all-targets with WebTransport,
no-default-features WebTransport/all-targets, WASM browser/library and WASM
browser-audio/library. Existing unused-code warnings remain. Host checks compile
portable and existing native actor fixtures; WASM checks compile actual binding
methods but not fixture children or generated JS. JS was inspected as text only.
No assertions, runtime, formal review/QA, verify or close gates were executed.
