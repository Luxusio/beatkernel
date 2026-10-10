//! Latest-state presentation bridge; never called from the audio callback.
use crate::{
    competition::ScoreSummary,
    gameplay::output::domain::control::{
        OutputCapability, OutputControls, OutputReply, OutputRequest,
    },
    gauge::{BmsGauge, GaugeFailure, GaugeProfile},
    judgment_policy::{BmsJudgmentPolicy, BmsScoreSummary},
    local_players::PlayerId,
    local_runtime::PlayerReport,
    mine_damage::MineDamageSummary,
    note_progress::NoteProgress,
    play_policy::ResolvedPlayPolicy,
    play_result::CompletedPlayResult,
    player_chart::PlayerChart,
    pressed_keys::{validate_mask, PressedKeys},
    room_presentation::{
        RoomPresentation, RoomResults, RoomUiAction, RoomUiReply, RoomUiRequest, ROOM_UI_CAPACITY,
    },
};
use beatkernel::{
    chart::CompiledChart,
    input::GameInputEvent,
    judge::{HazardEvent, JudgeEvent},
    runtime::RuntimeReport,
    time::Timestamp,
};
use beatkernel_bms::BmsChart;
use std::{
    cell::RefCell,
    collections::VecDeque,
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// Application-owned gameplay lifecycle, separate from native audio counters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerStatus {
    /// Native chart/assets/device preparation is in progress.
    Loading,
    /// Actual runtime reports have been received.
    Playing,
    /// UI cancellation requested; native cleanup is still pending.
    Stopping,
    /// Native owners have completed cleanup.
    Finished,
    /// Preparation/gameplay/cleanup failed with an explicit diagnostic.
    Failed(String),
}

#[cfg(test)]
use crate::competition::OpponentKind;
pub use crate::native_gameplay_host::PauseState;

pub use crate::competition_presentation::{
    CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus,
};

/// One stable local member's actual reports; never an aggregate cohort score.
#[derive(Clone)]
pub struct LocalPlayerSnapshot {
    pub player: PlayerId,
    pub chart: Option<Arc<PlayerChart>>,
    pub song_time: Option<Timestamp>,
    pub score: ScoreSummary,
    pub bms_score: Option<BmsScoreSummary>,
    /// Exact committed mine evidence, independent of ordinary-note score.
    pub mine_damage: MineDamageSummary,
    /// Independent policy observations for this actual member (default until setup).
    pub gauge: BmsGauge,
    pub last_judge: Option<JudgeEvent>,
    pub recent_results: Vec<JudgeEvent>,
    /// Actual admitted ownership, masked during pause and cleared after gauge failure.
    pub pressed_lanes: u32,
    /// Authoritative full-prefix state for the exact prepared chart, if available.
    pub note_progress: Option<NoteProgress>,
    pub competition: Option<CompetitionSnapshot>,
}
impl LocalPlayerSnapshot {
    fn new(player: PlayerId, chart: Option<Arc<PlayerChart>>) -> Self {
        Self {
            player,
            chart,
            song_time: None,
            score: ScoreSummary::default(),
            bms_score: None,
            mine_damage: MineDamageSummary::default(),
            gauge: BmsGauge::default(),
            last_judge: None,
            recent_results: Vec::new(),
            pressed_lanes: 0,
            note_progress: None,
            competition: None,
        }
    }
    fn update_report(&mut self, report: &RuntimeReport) {
        self.update_results(report.song_time, &report.judge_events);
    }
    fn update_results(&mut self, song: Timestamp, events: &[JudgeEvent]) {
        self.song_time = Some(song);
        if let Some(progress) = &mut self.note_progress {
            progress.apply(events);
        }
        if let Some(last) = events.last() {
            self.last_judge = Some(*last);
        }
        if events.len() >= 128 {
            self.recent_results.clear();
        }
        for event in events.iter().skip(events.len().saturating_sub(128)) {
            if self.recent_results.len() == 128 {
                self.recent_results.remove(0);
            }
            self.recent_results.push(*event);
        }
    }
}

/// Native solo completion uses the actual sole registered player identity.
pub fn publish_completed_solo(
    result: CompletedPlayResult,
) -> Result<(), Box<dyn std::error::Error>> {
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
        if current.snapshot.players.len() != 1 {
            return Err("solo completion requires one registered player".into());
        }
        let player = current.snapshot.players[0].player;
        if current
            .snapshot
            .apply_completed_results(&[(player, result)])?
        {
            current.publish_latest(true);
        }
        Ok(())
    })
}

/// Atomic cohort result publication; unattached execution still returns typed proof.
pub fn publish_completed_local(
    rows: &[(PlayerId, CompletedPlayResult)],
) -> Result<(), Box<dyn std::error::Error>> {
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
        if current.snapshot.apply_completed_results(rows)? {
            current.publish_latest(true);
        }
        Ok(())
    })
}

/// Replace one known member's comparisons on the game owner, never a callback.
/// Validate bounds before mutating; unattached native commands are a no-op.
pub fn publish_competition(
    player: PlayerId,
    snapshot: CompetitionSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    if snapshot.ghosts.len() > 8
        || snapshot.ghosts.iter().any(|ghost| {
            ghost.label.len() > 256
                || ghost.label.chars().count() > 64
                || ghost.label.chars().any(char::is_control)
        })
    {
        return Err("competition presentation exceeds ghost/label bounds".into());
    }
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
        let member = current
            .snapshot
            .players
            .iter_mut()
            .find(|member| member.player == player)
            .ok_or("competition presentation requires a registered player")?;
        member.competition = Some(snapshot);
        current.publish_latest(false);
        Ok(())
    })
}

/// Replace the whole cohort's peer presentation atomically without changing ghosts.
pub fn publish_networks(
    rows: &[(PlayerId, NetworkSnapshot)],
) -> Result<(), Box<dyn std::error::Error>> {
    crate::multiplayer_group::validate_roster(
        &rows.iter().map(|(player, _)| *player).collect::<Vec<_>>(),
    )?;
    for (_, snapshot) in rows {
        if let Some(progress) = snapshot.progress {
            crate::multiplayer_protocol::validate_progress(None, progress)?;
        }
    }
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
        if rows.iter().any(|(player, _)| {
            !current
                .snapshot
                .players
                .iter()
                .any(|member| member.player == *player)
        }) {
            return Err("network presentation requires registered players".into());
        }
        for (player, peer) in rows {
            let member = current
                .snapshot
                .players
                .iter_mut()
                .find(|member| member.player == *player)
                .expect("registered cohort member");
            member
                .competition
                .get_or_insert_with(|| CompetitionSnapshot {
                    ghosts: Vec::new(),
                    network: None,
                })
                .network = Some(peer.clone());
        }
        current.publish_latest(false);
        Ok(())
    })
}

/// Saved opponent updates preserve the independent shared peer comparison.
pub fn publish_saved_competition(
    player: PlayerId,
    ghosts: Vec<GhostSnapshot>,
) -> Result<(), Box<dyn std::error::Error>> {
    let network = SESSION.with(|session| {
        session.borrow().as_ref().and_then(|current| {
            current
                .snapshot
                .players
                .iter()
                .find(|member| member.player == player)
                .and_then(|member| member.competition.as_ref())
                .and_then(|snapshot| snapshot.network.clone())
        })
    });
    publish_competition(player, CompetitionSnapshot { ghosts, network })
}

/// Immutable UI copy of actual chart/time/results; no inferred clock relation.
#[derive(Clone)]
pub struct PlayerSnapshot {
    /// All registered local members, bounded to 64 with 128 recent results each.
    pub players: Vec<LocalPlayerSnapshot>,
    pub chart: Option<Arc<PlayerChart>>,
    /// One immutable CPU image bank shared by the registered local roster.
    pub images: Option<Arc<crate::image_assets::ImageAssets>>,
    /// Prepared movie descriptors and nonblocking IO handle, shared without pixels.
    #[cfg(not(target_arch = "wasm32"))]
    pub movies: Option<Arc<crate::video_native_bank::NativeVideoBank>>,
    pub song_time: Option<Timestamp>,
    pub score: ScoreSummary,
    pub bms_score: Option<BmsScoreSummary>,
    /// Mirrors the sole member only; multiple members have no aggregate damage.
    pub mine_damage: MineDamageSummary,
    /// Mirrors one member; the default value is not a multi-member aggregate.
    pub gauge: BmsGauge,
    pub last_judge: Option<JudgeEvent>,
    pub recent_results: Vec<JudgeEvent>,
    /// Actual admitted button ownership, masked during native pause transitions.
    pub pressed_lanes: u32,
    /// Authoritative full-prefix state for the exact prepared chart, if available.
    pub note_progress: Option<NoteProgress>,
    pub status: PlayerStatus,
    pub cancelled: bool,
    pub pause: PauseState,
    /// Native finite endpoint presented and input drained; cleanup status is separate.
    pub completed_end: Option<Timestamp>,
    /// First proven completion table, retained independently of cleanup status.
    pub completed_results: Option<Vec<(PlayerId, CompletedPlayResult)>>,
    /// Shared admission metadata and only the selected room score page.
    pub room: Option<Arc<RoomPresentation>>,
    /// Immutable room history attached only after the actual network owner joins.
    pub room_results: Option<Arc<RoomResults>>,
}
impl Default for PlayerSnapshot {
    fn default() -> Self {
        Self {
            players: Vec::new(),
            chart: None,
            images: None,
            #[cfg(not(target_arch = "wasm32"))]
            movies: None,
            song_time: None,
            score: ScoreSummary::default(),
            bms_score: None,
            mine_damage: MineDamageSummary::default(),
            gauge: BmsGauge::default(),
            last_judge: None,
            recent_results: Vec::new(),
            pressed_lanes: 0,
            note_progress: None,
            status: PlayerStatus::Loading,
            cancelled: false,
            pause: PauseState::Unavailable,
            completed_end: None,
            completed_results: None,
            room: None,
            room_results: None,
        }
    }
}
struct Shared {
    native_launch: Mutex<Option<crate::session_launch::SessionLaunch>>,
    practice: Mutex<crate::practice_control::PracticeControls>,
    practice_busy: AtomicBool,
    practice_owner: AtomicBool,
    practice_paused: AtomicBool,
    output: Mutex<OutputControls>,
    output_supported: AtomicBool,
    output_closed: AtomicBool,
    output_queued: AtomicBool,
    output_busy: AtomicBool,
    latest: Mutex<Option<PlayerSnapshot>>,
    cancel: AtomicBool,
    pause_requested: AtomicBool,
    room_closed: AtomicBool,
    room: Mutex<RoomControls>,
}
struct RoomControls {
    next_id: Option<u64>,
    presentation: Option<Arc<RoomPresentation>>,
    queued: VecDeque<RoomUiRequest>,
    in_flight: Vec<u64>,
    replies: VecDeque<RoomUiReply>,
    notice: Option<Arc<str>>,
}
impl RoomControls {
    fn settle(&mut self, message: &str) {
        for request in self.queued.drain(..) {
            self.replies.push_back(RoomUiReply {
                id: request.id,
                result: Err(message.into()),
            });
        }
        for id in self.in_flight.drain(..) {
            self.replies.push_back(RoomUiReply {
                id,
                result: Err(message.into()),
            });
        }
    }
}
fn room_lock_error<T>(error: std::sync::TryLockError<T>) -> io::Error {
    match error {
        std::sync::TryLockError::WouldBlock => {
            io::Error::new(io::ErrorKind::WouldBlock, "room controls are busy")
        }
        std::sync::TryLockError::Poisoned(_) => io::Error::other("room controls are unavailable"),
    }
}
fn settle_room(shared: &Shared, controls: &mut RoomControls) -> bool {
    if shared.cancel.load(Ordering::Acquire) {
        controls.settle("room request cancelled");
        true
    } else if shared.room_closed.load(Ordering::Acquire) {
        controls.settle("room controls closed");
        true
    } else {
        false
    }
}
fn settle_output(shared: &Shared, controls: &mut OutputControls) -> bool {
    if shared.output_closed.load(Ordering::Acquire) {
        controls.close("output controls closed");
        true
    } else if shared.cancel.load(Ordering::Acquire) {
        controls.cancel("output controls cancelled");
        true
    } else {
        false
    }
}
/// Sendable attachment token; native input/window objects never cross threads.
#[derive(Clone)]
pub struct PlayerPublisher(Arc<Shared>);
/// Main-thread latest-state consumer and explicit cancellation source.
pub struct PlayerViewer(Arc<Shared>);

/// Make one bounded latest-state slot; fresh channels isolate restarted sessions.
pub fn channel() -> (PlayerPublisher, PlayerViewer) {
    let shared = Arc::new(Shared {
        native_launch: Mutex::new(None),
        practice: Mutex::new(crate::practice_control::PracticeControls::new()),
        practice_busy: AtomicBool::new(false),
        practice_owner: AtomicBool::new(false),
        practice_paused: AtomicBool::new(false),
        output: Mutex::new(OutputControls::new()),
        output_supported: AtomicBool::new(false),
        output_closed: AtomicBool::new(false),
        output_queued: AtomicBool::new(false),
        output_busy: AtomicBool::new(false),
        latest: Mutex::new(Some(PlayerSnapshot::default())),
        cancel: AtomicBool::new(false),
        pause_requested: AtomicBool::new(false),
        room_closed: AtomicBool::new(false),
        room: Mutex::new(RoomControls {
            next_id: Some(1),
            presentation: None,
            queued: VecDeque::with_capacity(ROOM_UI_CAPACITY),
            in_flight: Vec::with_capacity(ROOM_UI_CAPACITY),
            replies: VecDeque::with_capacity(ROOM_UI_CAPACITY),
            notice: None,
        }),
    });
    (PlayerPublisher(shared.clone()), PlayerViewer(shared))
}
fn settle_practice(
    shared: &Shared,
    controls: &mut crate::practice_control::PracticeControls,
) -> bool {
    let closed =
        shared.cancel.load(Ordering::Acquire) || shared.output_closed.load(Ordering::Acquire);
    if closed {
        controls.revoke("practice owner cancelled or closed");
    }
    shared
        .practice_busy
        .store(controls.pending(), Ordering::Release);
    closed
}
impl PlayerPublisher {
    /// Cold native-owner advertisement; capability defaults to unsupported.
    pub fn advertise_practice(
        &self,
        cap: Option<crate::practice_control::PracticeCapability>,
    ) -> io::Result<()> {
        let mut controls = self
            .0
            .practice
            .lock()
            .map_err(|_| io::Error::other("practice controls unavailable"))?;
        if settle_practice(&self.0, &mut controls) {
            return Ok(());
        }
        controls.advertise(cap).map_err(io::Error::other)?;
        if cap.is_some() {
            self.0.practice_owner.store(true, Ordering::Release);
        }
        self.0
            .practice_busy
            .store(controls.pending(), Ordering::Release);
        Ok(())
    }
    pub fn take_practice_request(
        &self,
    ) -> io::Result<Option<crate::practice_control::PracticeRequest>> {
        let mut controls = self.0.practice.try_lock().map_err(room_lock_error)?;
        if settle_practice(&self.0, &mut controls) {
            return Ok(None);
        }
        Ok(controls.take_request())
    }
    pub fn reply_practice(&self, reply: &crate::practice_control::PracticeReply) -> io::Result<()> {
        let mut controls = self.0.practice.try_lock().map_err(room_lock_error)?;
        if settle_practice(&self.0, &mut controls) {
            return Err(io::Error::other("practice owner cancelled or closed"));
        }
        controls.reply(reply).map_err(io::Error::other)
    }
    /// Decided boundary publication on the game owner, never audio callback.
    pub fn commit_practice_reply(
        &self,
        reply: &crate::practice_control::PracticeReply,
    ) -> io::Result<()> {
        let mut controls = self
            .0
            .practice
            .lock()
            .map_err(|_| io::Error::other("practice controls unavailable"))?;
        if settle_practice(&self.0, &mut controls) {
            return Err(io::Error::other("practice owner cancelled or closed"));
        }
        controls.reply(reply).map_err(io::Error::other)
    }
}
pub fn advertise_practice(
    cap: Option<crate::practice_control::PracticeCapability>,
) -> io::Result<()> {
    output_publisher(|p| p.advertise_practice(cap), ())
}
pub fn take_practice_request() -> io::Result<Option<crate::practice_control::PracticeRequest>> {
    output_publisher(|p| p.take_practice_request(), None)
}
pub fn reply_practice(reply: &crate::practice_control::PracticeReply) -> io::Result<()> {
    output_publisher(|p| p.reply_practice(reply), ())
}
pub fn commit_practice_reply(reply: &crate::practice_control::PracticeReply) -> io::Result<()> {
    output_publisher(|p| p.commit_practice_reply(reply), ())
}
/// Pin the desktop invocation before native preparation.
pub fn pin_native_launch(launch: crate::session_launch::SessionLaunch) -> Result<(), String> {
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let current = session
            .as_mut()
            .ok_or("native launch requires an attached owner")?;
        if current.native_launch.is_some() || current.chart_published {
            return Err("native launch is already prepared".into());
        }
        let publication = launch.clone();
        let mut slot = current
            .publisher
            .0
            .native_launch
            .lock()
            .map_err(|_| "native launch publication unavailable")?;
        current.native_launch = Some(launch);
        *slot = Some(publication);
        Ok(())
    })
}
/// Preserve desktop retry lineage; direct native CLI pins its own arguments.
pub fn native_launch(args: &[String]) -> Result<crate::session_launch::SessionLaunch, String> {
    SESSION.with(|session| {
        if let Some(launch) = session
            .borrow()
            .as_ref()
            .and_then(|current| current.native_launch.as_ref())
        {
            if launch.args() != args {
                return Err("native invocation differs from pinned launch".into());
            }
            return Ok(launch.clone());
        }
        crate::session_launch::SessionLaunch::new(args.to_vec())
    })
}
pub fn practice_pending() -> bool {
    output_publisher(|p| Ok(p.0.practice_busy.load(Ordering::Acquire)), false).unwrap_or(true)
}
impl PlayerViewer {
    /// Move the latest boundary's canonical global invocation once. This
    /// mailbox remains readable after native cleanup so joined retry sees the
    /// final ordinal; no argument vector is cloned by frame polling.
    pub fn take_native_launch(&self) -> io::Result<Option<crate::session_launch::SessionLaunch>> {
        self.0
            .native_launch
            .try_lock()
            .map_err(room_lock_error)
            .map(|mut slot| slot.take())
    }
    pub fn practice_capability(
        &self,
    ) -> io::Result<Option<crate::practice_control::PracticeCapability>> {
        let mut controls = self.0.practice.try_lock().map_err(room_lock_error)?;
        if settle_practice(&self.0, &mut controls) {
            return Ok(None);
        }
        Ok(controls.capability())
    }
    /// This channel belonged to a retained owner, even after capability revocation.
    pub fn retained_practice_owner(&self) -> bool {
        self.0.practice_owner.load(Ordering::Acquire)
    }
    pub fn practice_pending(&self) -> bool {
        self.0.practice_busy.load(Ordering::Acquire)
    }
    pub fn pending_practice_request(
        &self,
    ) -> io::Result<Option<crate::practice_control::PracticeRequest>> {
        let controls = self.0.practice.try_lock().map_err(room_lock_error)?;
        Ok(controls.pending_request())
    }
    pub fn request_practice(
        &self,
        action: crate::practice_control::PracticeAction,
    ) -> io::Result<u64> {
        let mut controls = self.0.practice.try_lock().map_err(room_lock_error)?;
        if settle_practice(&self.0, &mut controls) {
            return Err(io::Error::other("practice owner cancelled or closed"));
        }
        if self.output_pending()
            || self.pause_requested()
            || self.0.practice_paused.load(Ordering::Acquire)
        {
            return Err(io::Error::other(
                "practice conflicts with pause or output replacement",
            ));
        }
        let id = controls.request(action).map_err(io::Error::other)?;
        self.0.practice_busy.store(true, Ordering::Release);
        Ok(id)
    }
    pub fn take_practice_reply(
        &self,
    ) -> io::Result<Option<crate::practice_control::PracticeReply>> {
        Ok(self
            .take_practice_response()?
            .map(|response| response.reply))
    }
    /// Consumes the response and its originating action under the same lock.
    /// Contention retains both records for a later UI poll.
    pub fn take_practice_response(
        &self,
    ) -> io::Result<Option<crate::practice_control::PracticeResponse>> {
        let mut controls = self.0.practice.try_lock().map_err(room_lock_error)?;
        settle_practice(&self.0, &mut controls);
        let response = controls.take_response();
        self.0
            .practice_busy
            .store(controls.pending(), Ordering::Release);
        Ok(response)
    }
}
impl PlayerPublisher {
    pub fn advertise_output(&self, capability: Option<OutputCapability>) -> io::Result<()> {
        if self.0.cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        if self.0.output_closed.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "output controls closed",
            ));
        }
        // Cold startup on the game owner, never an audio callback. A transient
        // UI reader must not turn a successfully opened output into failure.
        let mut controls = self
            .0
            .output
            .lock()
            .map_err(|_| io::Error::other("output controls are unavailable"))?;
        if self.0.cancel.load(Ordering::Acquire) {
            controls.cancel("output controls cancelled during setup");
            self.0.output_supported.store(false, Ordering::Release);
            self.0
                .output_queued
                .store(controls.queued(), Ordering::Release);
            self.0
                .output_busy
                .store(controls.pending(), Ordering::Release);
            return Ok(());
        }
        if self.0.output_closed.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "output controls closed",
            ));
        }
        let supported = capability.is_some();
        controls.advertise(capability).map_err(io::Error::other)?;
        self.0.output_supported.store(supported, Ordering::Release);
        Ok(())
    }
    pub fn take_output_request(&self) -> io::Result<Option<OutputRequest>> {
        if !self.0.output_queued.load(Ordering::Acquire) {
            return Ok(None);
        }
        let mut controls = self.0.output.try_lock().map_err(room_lock_error)?;
        settle_output(&self.0, &mut controls);
        let request = controls.take_request();
        self.0
            .output_queued
            .store(controls.queued(), Ordering::Release);
        self.0
            .output_busy
            .store(controls.pending(), Ordering::Release);
        Ok(request)
    }
    pub fn reply_output(&self, reply: &OutputReply) -> io::Result<()> {
        let mut controls = self.0.output.try_lock().map_err(room_lock_error)?;
        self.complete_output_reply(&mut controls, reply)
    }
    /// Cold owner-thread publication; never call from an audio callback.
    /// A decided reply must reach shared state before the gameplay pump exits.
    pub fn commit_output_reply(&self, reply: &OutputReply) -> io::Result<()> {
        let mut controls = self
            .0
            .output
            .lock()
            .map_err(|_| io::Error::other("output controls are unavailable"))?;
        self.complete_output_reply(&mut controls, reply)
    }
    fn complete_output_reply(
        &self,
        controls: &mut OutputControls,
        reply: &OutputReply,
    ) -> io::Result<()> {
        if !self.0.output_closed.load(Ordering::Acquire) {
            controls.reply(reply).map_err(io::Error::other)?;
        }
        // Cancellation cannot supersede the owner's already-decided result.
        let closed = settle_output(&self.0, controls);
        self.0.output_supported.store(
            !closed && controls.capability().is_some(),
            Ordering::Release,
        );
        self.0
            .output_busy
            .store(controls.pending(), Ordering::Release);
        self.0
            .output_queued
            .store(controls.queued(), Ordering::Release);
        Ok(())
    }
    pub fn output_pending(&self) -> bool {
        self.0.output_busy.load(Ordering::Acquire)
    }
}
fn output_publisher<T>(
    run: impl FnOnce(&PlayerPublisher) -> io::Result<T>,
    absent: T,
) -> io::Result<T> {
    SESSION.with(|session| match session.borrow().as_ref() {
        Some(session) => run(&session.publisher),
        None => Ok(absent),
    })
}
pub fn advertise_output(cap: Option<OutputCapability>) -> io::Result<()> {
    output_publisher(|p| p.advertise_output(cap), ())
}
pub fn take_output_request() -> io::Result<Option<OutputRequest>> {
    output_publisher(|p| p.take_output_request(), None)
}
pub fn reply_output(reply: &OutputReply) -> io::Result<()> {
    output_publisher(|p| p.reply_output(reply), ())
}
pub fn commit_output_reply(reply: &OutputReply) -> io::Result<()> {
    output_publisher(|p| p.commit_output_reply(reply), ())
}
pub fn output_pending() -> bool {
    output_publisher(|p| Ok(p.output_pending()), false).unwrap_or(true)
}
impl PlayerViewer {
    pub fn output_supported(&self) -> bool {
        self.0.output_supported.load(Ordering::Acquire)
            && !self.0.cancel.load(Ordering::Acquire)
            && !self.0.output_closed.load(Ordering::Acquire)
    }
    pub fn output_capability(&self) -> io::Result<Option<OutputCapability>> {
        if self.0.cancel.load(Ordering::Acquire) || self.0.output_closed.load(Ordering::Acquire) {
            return Ok(None);
        }
        Ok(self
            .0
            .output
            .try_lock()
            .map_err(room_lock_error)?
            .capability()
            .cloned())
    }
    pub fn output_pending(&self) -> bool {
        self.0.output_busy.load(Ordering::Acquire)
    }
    pub fn request_output(&self, args: Vec<String>) -> io::Result<u64> {
        let practice = self.0.practice.try_lock().map_err(room_lock_error)?;
        if practice.pending() {
            return Err(io::Error::other("practice boundary is pending"));
        }
        let mut controls = self.0.output.try_lock().map_err(room_lock_error)?;
        if settle_output(&self.0, &mut controls) {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "output request cancelled",
            ));
        }
        let id = controls.request(args).map_err(io::Error::other)?;
        self.0.output_busy.store(true, Ordering::Release);
        self.0.output_queued.store(true, Ordering::Release);
        Ok(id)
    }
    pub fn take_output_reply(&self) -> io::Result<Option<OutputReply>> {
        if !self.0.output_busy.load(Ordering::Acquire) {
            return Ok(None);
        }
        let mut controls = self.0.output.try_lock().map_err(room_lock_error)?;
        settle_output(&self.0, &mut controls);
        let reply = controls.take_reply();
        self.0
            .output_busy
            .store(controls.pending(), Ordering::Release);
        self.0
            .output_queued
            .store(controls.queued(), Ordering::Release);
        Ok(reply)
    }

    /// UI identity is distinct from the network owner's command identity.
    pub fn request_room(&self, action: RoomUiAction) -> io::Result<u64> {
        let mut controls = self.0.room.try_lock().map_err(room_lock_error)?;
        if settle_room(&self.0, &mut controls) {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "room controls are cancelled or closed",
            ));
        }
        if !controls
            .presentation
            .as_ref()
            .is_some_and(|room| room.allows(action))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "room action is unavailable in this snapshot",
            ));
        }
        if controls.queued.len() + controls.in_flight.len() + controls.replies.len()
            >= ROOM_UI_CAPACITY
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "room request results are full",
            ));
        }
        let id = controls
            .next_id
            .ok_or_else(|| io::Error::other("room UI identity exhausted"))?;
        controls.queued.push_back(RoomUiRequest { id, action });
        controls.next_id = id.checked_add(1);
        controls.notice = Some(format!("ROOM REQUEST {id} PENDING").into());
        Ok(id)
    }
    pub fn take_room_reply(&self) -> io::Result<Option<RoomUiReply>> {
        let mut controls = self.0.room.try_lock().map_err(room_lock_error)?;
        settle_room(&self.0, &mut controls);
        let reply = controls.replies.pop_front();
        if let Some(reply) = &reply {
            let message = match &reply.result {
                Ok(()) => format!("ROOM REQUEST {} ACCEPTED", reply.id),
                Err(error) => format!(
                    "ROOM REQUEST {}: {}",
                    reply.id,
                    error
                        .chars()
                        .take(512)
                        .map(|ch| if ch.is_control() { ' ' } else { ch })
                        .collect::<String>()
                ),
            };
            controls.notice = Some(message.into());
        }
        Ok(reply)
    }
    pub fn room_pending(&self) -> bool {
        self.0.room.try_lock().map_or(true, |controls| {
            !controls.queued.is_empty()
                || !controls.in_flight.is_empty()
                || !controls.replies.is_empty()
        })
    }
    pub fn room_notice(&self) -> Option<Arc<str>> {
        self.0
            .room
            .try_lock()
            .ok()
            .and_then(|controls| controls.notice.clone())
    }
    /// UI-side desired state; actual boundaries remain native-owner authority.
    pub fn pause_requested(&self) -> bool {
        self.0.pause_requested.load(Ordering::Acquire)
    }
    /// Desired state only; native snapshots acknowledge actual boundaries.
    pub fn request_pause(&self, paused: bool) {
        if let Ok(practice) = self.0.practice.try_lock() {
            if !practice.pending() && !self.0.cancel.load(Ordering::Acquire) {
                self.0.pause_requested.store(paused, Ordering::Release);
            }
        }
    }
    /// Take the current coalesced snapshot; release its lock before any drawing.
    pub fn take_latest(&self) -> Option<PlayerSnapshot> {
        self.0.latest.lock().ok()?.take()
    }
    /// Wake-free cancellation; gameplay checks this independently of UI frames.
    pub fn cancel(&self) {
        self.0.cancel.store(true, Ordering::Release);
        if let Ok(mut slot) = self.0.latest.try_lock() {
            if let Some(snapshot) = slot.as_mut() {
                if !matches!(
                    snapshot.status,
                    PlayerStatus::Finished | PlayerStatus::Failed(_)
                ) {
                    snapshot.status = PlayerStatus::Stopping;
                    snapshot.cancelled = true;
                    snapshot.pressed_lanes = 0;
                    for member in &mut snapshot.players {
                        member.pressed_lanes = 0;
                    }
                }
            }
        }
    }
}
#[derive(Default)]
struct PressedState {
    keys: PressedKeys,
    mask: u32,
}
struct OutputAttachmentGuard(Arc<Shared>);
impl Drop for OutputAttachmentGuard {
    fn drop(&mut self) {
        self.0.output_supported.store(false, Ordering::Release);
        self.0.output_closed.store(true, Ordering::Release);
        if let Ok(mut controls) = self.0.practice.try_lock() {
            controls.revoke("native play owner ended");
            self.0
                .practice_busy
                .store(controls.pending(), Ordering::Release);
        }
        self.0.output_queued.store(false, Ordering::Release);
        if let Ok(mut controls) = self.0.output.try_lock() {
            controls.close("native play owner ended");
            self.0
                .output_busy
                .store(controls.pending(), Ordering::Release);
        }
    }
}
struct Session {
    native_launch: Option<crate::session_launch::SessionLaunch>,
    pressed: Vec<PressedState>,
    publisher: PlayerPublisher,
    snapshot: PlayerSnapshot,
    last_publish: Option<Instant>,
    chart_published: bool,
    replay_policy: Option<GaugeProfile>,
    live_policy_prepared: bool,
    live_classes: Vec<Option<BmsJudgmentPolicy>>,
    practice_generation: u64,
    pause_dirty: bool,
    room_dirty: bool,
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for Session {
    fn drop(&mut self) {
        // Session belongs to the game worker, including unwind/thread exit.
        // Snapshot Arc releases on the UI/render owner never wait for video IO.
        if let Some(bank) = &self.snapshot.movies {
            let _ = bank.join();
        }
    }
}
thread_local! { static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) }; }

/// Native owner reads desired state without UI locks; cancellation wins.
pub fn pause_requested() -> bool {
    SESSION.with(|session| {
        session.borrow().as_ref().is_some_and(|session| {
            !session.publisher.0.cancel.load(Ordering::Acquire)
                && session.publisher.0.pause_requested.load(Ordering::Acquire)
        })
    })
}

/// Only an integrated native owner announces support and phase changes.
pub fn publish_pause(pause: PauseState) {
    SESSION.with(|session| {
        if let Some(session) = session.borrow_mut().as_mut() {
            session.publisher.0.practice_paused.store(
                !matches!(pause, PauseState::Running | PauseState::Unavailable),
                Ordering::Release,
            );
            if session.snapshot.pause != pause {
                session.snapshot.pause = pause;
                session.pause_dirty = true;
            }
            session.observe_cancellation(false);
            if session.pause_dirty {
                session.publish_latest(true);
            }
        }
    });
}

/// Retry an acknowledgement lost to temporary UI slot contention, without
/// cloning unchanged pause snapshots after successful delivery.
pub fn retry_pause_publication() {
    SESSION.with(|session| {
        if let Some(session) = session.borrow_mut().as_mut() {
            if session.pause_dirty {
                session.publish_latest(true);
            }
        }
    });
}

/// Attach native play on its owner thread. Terminal publication occurs only
/// after the native function returns and its explicit/drop cleanup has run.
pub fn with_publisher<T>(
    publisher: PlayerPublisher,
    run: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    SESSION.with(|session| {
        if session.borrow().is_some() {
            return Err("game presentation is already attached".into());
        }
        *session.borrow_mut() = Some(Session {
            native_launch: None,
            publisher,
            pressed: Vec::new(),
            snapshot: PlayerSnapshot::default(),
            last_publish: None,
            chart_published: false,
            replay_policy: None,
            live_policy_prepared: false,
            live_classes: Vec::new(),
            practice_generation: 1,
            pause_dirty: false,
            room_dirty: false,
        });
        Ok::<(), String>(())
    })?;
    // Unwind drops the thread-local session with its game owner; the UI observes
    // JoinHandle's panic separately. No unwind crosses a native callback ABI.
    let _output_guard = SESSION.with(|session| {
        OutputAttachmentGuard(
            session
                .borrow()
                .as_ref()
                .expect("attached play owner")
                .publisher
                .0
                .clone(),
        )
    });
    let mut result = run();
    SESSION.with(|session| {
        if let Some(mut current) = session.borrow_mut().take() {
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(bank) = &current.snapshot.movies {
                if let Err(reason) = bank.join() {
                    if result.is_ok() {
                        result = Err(reason);
                    }
                }
            }
            current
                .publisher
                .0
                .output_supported
                .store(false, Ordering::Release);
            current
                .publisher
                .0
                .output_closed
                .store(true, Ordering::Release);
            if let Ok(mut output) = current.publisher.0.output.lock() {
                output.close(
                    result
                        .as_ref()
                        .err()
                        .map(String::as_str)
                        .unwrap_or("native play finished"),
                );
                current
                    .publisher
                    .0
                    .output_queued
                    .store(false, Ordering::Release);
                current
                    .publisher
                    .0
                    .output_busy
                    .store(output.pending(), Ordering::Release);
            }
            current
                .publisher
                .0
                .room_closed
                .store(true, Ordering::Release);
            if let Ok(mut controls) = current.publisher.0.room.lock() {
                settle_room(&current.publisher.0, &mut controls);
            }
            current.snapshot.cancelled = current.publisher.0.cancel.load(Ordering::Acquire);
            current.snapshot.status = match &result {
                Ok(_) => PlayerStatus::Finished,
                Err(error) => PlayerStatus::Failed(error.clone()),
            };
            current.clear_pressed();
            current.snapshot.sync_legacy();
            // Gameplay and audio owners have already stopped. This final tiny
            // handoff can wait for take_latest, which releases before rendering.
            if let Ok(mut slot) = current.publisher.0.latest.lock() {
                *slot = Some(std::mem::take(&mut current.snapshot));
            }
        }
    });
    result
}

/// Game-thread UI request acquisition. Cancellation settles queued and already
/// acquired requests; it never turns an old request into a replacement action.
pub fn take_room_request() -> io::Result<Option<RoomUiRequest>> {
    SESSION.with(|session| {
        let session = session.borrow();
        let Some(current) = session.as_ref() else {
            return Ok(None);
        };
        let mut controls = current
            .publisher
            .0
            .room
            .try_lock()
            .map_err(room_lock_error)?;
        if settle_room(&current.publisher.0, &mut controls) {
            return Ok(None);
        }
        let request = controls.queued.pop_front();
        if let Some(request) = request {
            controls.in_flight.push(request.id);
        }
        Ok(request)
    })
}
pub fn reply_room(reply: RoomUiReply) -> io::Result<()> {
    if reply.id == 0
        || reply
            .result
            .as_ref()
            .err()
            .is_some_and(|error| error.len() > 4096)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid bounded room UI result",
        ));
    }
    SESSION.with(|session| {
        let session = session.borrow();
        let Some(current) = session.as_ref() else {
            return Ok(());
        };
        let mut controls = current
            .publisher
            .0
            .room
            .try_lock()
            .map_err(room_lock_error)?;
        if settle_room(&current.publisher.0, &mut controls) {
            return Ok(());
        }
        let index = controls
            .in_flight
            .iter()
            .position(|id| *id == reply.id)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unknown room UI result identity",
                )
            })?;
        controls.in_flight.remove(index);
        controls.replies.push_back(reply);
        Ok(())
    })
}
pub fn close_room_controls() {
    SESSION.with(|session| {
        if let Some(current) = session.borrow().as_ref() {
            current
                .publisher
                .0
                .room_closed
                .store(true, Ordering::Release);
            if let Ok(mut controls) = current.publisher.0.room.try_lock() {
                settle_room(&current.publisher.0, &mut controls);
            }
        }
    });
}
pub fn publish_room(room: Arc<RoomPresentation>) -> Result<(), String> {
    room.validate()?;
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
        if current.snapshot.room_results.is_some() {
            return Err("joined room Results cannot be replaced by live presentation".into());
        }
        let changed = current
            .snapshot
            .room
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, &room));
        if changed {
            current.snapshot.room = Some(room);
            current.room_dirty = true;
        }
        if current.room_dirty {
            current.publish_latest(true);
        }
        Ok(())
    })
}

/// Called by the room controller after stop/join and its final retained poll.
/// Archive failure is presentation-only; the caller preserves the real outcome.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn publish_room_results(results: Arc<RoomResults>) -> Result<(), String> {
    let page = Arc::new(results.project(results.initial_page())?);
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
        if !current.publisher.0.room_closed.load(Ordering::Acquire) {
            return Err("room Results require closed live controls".into());
        }
        if let Some(existing) = &current.snapshot.room_results {
            if !Arc::ptr_eq(existing, &results) {
                return Err("room Results archive is already attached".into());
            }
        } else {
            current.snapshot.room_results = Some(results);
            current.snapshot.room = Some(page);
            current.room_dirty = true;
        }
        if current.room_dirty {
            current.publish_latest(true);
        }
        Ok(())
    })
}
pub fn retry_room_publication() {
    SESSION.with(|session| {
        if let Some(current) = session.borrow_mut().as_mut() {
            if current.room_dirty {
                current.publish_latest(true);
            }
        }
    });
}

/// Records an actual native presented/drained finite endpoint. Call only after
/// the owner completion gate; Finished is published separately after cleanup.
/// This does not infer completion from UI time or request owner cancellation.
pub fn publish_section_end(end: Timestamp) {
    SESSION.with(|session| {
        if let Some(current) = session.borrow_mut().as_mut() {
            current.snapshot.completed_end = Some(end);
        }
    });
}

/// Whether this native owner is attached to a graphical player.
pub fn attached() -> bool {
    SESSION.with(|session| session.borrow().is_some())
}
/// Actual UI cancellation state, without mutexes or window/native calls.
pub fn cancelled() -> bool {
    SESSION.with(|session| {
        session
            .borrow()
            .as_ref()
            .is_some_and(|session| session.publisher.0.cancel.load(Ordering::Acquire))
    })
}

/// Publish the exact solo chart; the legacy API uses stable PlayerId(1).
pub fn publish_chart(
    source: &BmsChart,
    chart: &CompiledChart,
) -> Result<(), Box<dyn std::error::Error>> {
    publish_local_chart(source, chart, &[PlayerId(1)])
}

/// Register 1..=64 unique nonzero local IDs and one shared exact prepared chart.
///
/// Registration cannot replace an existing roster or chart. An implicit solo
/// member from publish_report may receive its chart once, preserving its scores.
/// Invalid ID lists fail even unattached; terminal mode otherwise remains a no-op.
pub fn publish_local_chart(
    source: &BmsChart,
    chart: &CompiledChart,
    players: &[PlayerId],
) -> Result<(), Box<dyn std::error::Error>> {
    register_chart(source, chart, players, None, None)
}

/// Registers an attached native chart and referenced images before play starts.
/// Audio-only invocations do not open image files. Registration validation and
/// all preparation finish before any chart, roster or bank is published.
pub fn publish_native_chart(
    chart_path: &std::path::Path,
    source: &BmsChart,
    chart: &CompiledChart,
    players: &[PlayerId],
) -> Result<(), Box<dyn std::error::Error>> {
    register_chart(source, chart, players, Some(chart_path), None)
}

/// Registers the immutable policy of a pristine, fully validated replay.
/// Chart/profile evidence comes from ReplayVisual, never a caller's raw snapshot.
pub fn publish_native_replay_chart(
    chart_path: &std::path::Path,
    source: &BmsChart,
    chart: &CompiledChart,
    visual: &crate::replay_visual::ReplayVisual,
) -> Result<(), Box<dyn std::error::Error>> {
    let (recorded_chart, policy) = visual
        .pristine_presentation_setup()
        .ok_or("replay chart registration requires a pristine visual owner")?;
    if recorded_chart != chart {
        return Err("replay presentation chart differs from its validated owner".into());
    }
    register_chart(
        source,
        chart,
        &[PlayerId(1)],
        Some(chart_path),
        Some(policy),
    )
}

fn register_chart(
    source: &BmsChart,
    chart: &CompiledChart,
    players: &[PlayerId],
    native_path: Option<&std::path::Path>,
    replay_policy: Option<&GaugeProfile>,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_players(players)?;
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
        if replay_policy.is_some() && !current.snapshot.players.is_empty() {
            return Err("replay policy requires a fresh presentation roster".into());
        }
        if current.chart_published {
            return Err("game presentation chart is already registered".into());
        }
        if !current.snapshot.players.is_empty()
            && !current
                .snapshot
                .players
                .iter()
                .map(|member| member.player)
                .eq(players.iter().copied())
        {
            return Err("game presentation roster cannot be replaced".into());
        }
        let prepared = Arc::new(PlayerChart::from_compiled(source, chart)?);
        let progress = NoteProgress::new(Arc::clone(&prepared))?;
        let images = native_path
            .map(|path| -> Result<_, Box<dyn std::error::Error>> {
                let path = std::fs::canonicalize(path)?;
                let root = path.parent().ok_or("native chart has no directory")?;
                Ok(Arc::new(crate::image_assets::ImageAssets::prepare(
                    root,
                    source,
                    crate::image_assets::ImageAssetLimits::default(),
                )?))
            })
            .transpose()?;
        let mut members = if current.snapshot.players.is_empty() {
            players
                .iter()
                .map(|&player| LocalPlayerSnapshot::new(player, Some(Arc::clone(&prepared))))
                .collect()
        } else {
            current.snapshot.players.clone()
        };
        for member in &mut members {
            member.chart = Some(Arc::clone(&prepared));
            member.note_progress = if member.last_judge.is_none() {
                Some(progress.clone())
            } else {
                None
            };
        }
        let admitted_policy = replay_policy.map(GaugeProfile::try_copy).transpose()?;
        if let Some(policy) = &admitted_policy {
            members[0].gauge = BmsGauge::new(policy.try_copy()?);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(old) = &current.snapshot.movies {
            old.join()?;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let movies = native_path
            .map(|path| -> Result<_, Box<dyn std::error::Error>> {
                let path = std::fs::canonicalize(path)?;
                let root = path.parent().ok_or("native chart has no directory")?;
                let assets = Arc::new(crate::video_assets::VideoAssets::prepare(
                    root,
                    source,
                    Default::default(),
                )?);
                if assets.is_empty() {
                    return Ok(None);
                }
                Ok(Some(crate::video_native_bank::NativeVideoBank::prepare(
                    assets,
                )?))
            })
            .transpose()?
            .flatten();
        if current.pressed.is_empty() {
            current.pressed = players.iter().map(|_| PressedState::default()).collect();
        }
        if admitted_policy.is_some() {
            current.snapshot.gauge = members[0].gauge.clone();
        }
        current.replay_policy = admitted_policy;
        current.snapshot.players = members;
        current.snapshot.chart = Some(prepared);
        current.snapshot.images = images;
        #[cfg(not(target_arch = "wasm32"))]
        {
            current.snapshot.movies = movies;
        }
        current.chart_published = true;
        current.observe_cancellation(false);
        current.publish_latest(true);
        Ok(())
    })
}

/// Cold complete presentation for one future retained-output attempt.
/// Its private owner identity prevents installation into a different player.
pub struct PreparedPracticePresentation {
    attached: Option<PreparedAttachedPracticePresentation>,
    expected_generation: u64,
    applied_generation: Option<u64>,
}
struct PreparedAttachedPracticePresentation {
    owner: Arc<Shared>,
    registered_chart: Arc<PlayerChart>,
    snapshot: PlayerSnapshot,
    publication: PlayerSnapshot,
    pressed: Vec<PressedState>,
    classes: Vec<Option<BmsJudgmentPolicy>>,
    next_launch: Option<crate::session_launch::SessionLaunch>,
    launch_publication: Option<crate::session_launch::SessionLaunch>,
}

fn practice_identity_owner_matches(
    current: &Session,
    staged: &PreparedAttachedPracticePresentation,
    expected_generation: u64,
) -> bool {
    Arc::ptr_eq(&current.publisher.0, &staged.owner)
        && current.practice_generation == expected_generation
        && current
            .snapshot
            .chart
            .as_ref()
            .is_some_and(|chart| Arc::ptr_eq(chart, &staged.registered_chart))
        && current
            .snapshot
            .players
            .iter()
            .map(|member| member.player)
            .eq(staged.snapshot.players.iter().map(|member| member.player))
}

fn practice_session_ready(current: &Session, expected_generation: u64) -> bool {
    expected_generation != 0
        && current.practice_generation == expected_generation
        && current.chart_published
        && current.live_policy_prepared
        && current.replay_policy.is_none()
        && current.snapshot.room.is_none()
        && current.snapshot.room_results.is_none()
        && !current.snapshot.players.iter().any(|member| {
            member
                .competition
                .as_ref()
                .is_some_and(|value| value.network.is_some())
        })
        && !current.publisher.0.cancel.load(Ordering::Acquire)
        && !current.publisher.0.output_closed.load(Ordering::Acquire)
        && !current.publisher.0.pause_requested.load(Ordering::Acquire)
        && matches!(
            current.snapshot.pause,
            PauseState::Running | PauseState::Unavailable
        )
        && matches!(
            current.snapshot.status,
            PlayerStatus::Loading | PlayerStatus::Playing
        )
}

/// Prepare every member before changing any live presentation owner. The native
/// pump must supply the exact ordered cold attempts and later qualify the audio
/// boundary and acquired input cut; this constructor provides no clock proof.
/// Audio-only playback keeps the existing unattached bridge's no-op behavior.
pub fn prepare_practice_presentation(
    expected_generation: u64,
    attempts: &[(PlayerId, &crate::practice_session::PreparedPracticeAttempt)],
) -> Result<PreparedPracticePresentation, Box<dyn std::error::Error>> {
    let players: Vec<_> = attempts.iter().map(|(player, _)| *player).collect();
    validate_players(&players)?;
    if expected_generation == 0 {
        return Err("practice presentation requires a nonzero generation".into());
    }
    SESSION.with(|session| {
        let session = session.borrow();
        let Some(current) = session.as_ref() else {
            return Ok(PreparedPracticePresentation { attached: None, expected_generation, applied_generation: None });
        };
        if !practice_session_ready(current, expected_generation)
            || !current.snapshot.players.iter().map(|member| member.player).eq(players.iter().copied())
            || current.live_classes.len() != attempts.len()
        {
            return Err("practice presentation requires an exact registered live roster and generation".into());
        }
        let registered_chart = Arc::clone(current.snapshot.chart.as_ref().ok_or("practice chart missing")?);
        // Keep the unprojected desktop invocation. Cohort member recording
        // paths are distinct projections and must never replace this base.
        let next_launch = current.native_launch.as_ref()
            .map(crate::session_launch::SessionLaunch::retry).transpose()?;
        let launch_publication = next_launch.clone();
        let mut members = Vec::new();
        let mut pressed = Vec::new();
        let mut classes = Vec::new();
        members.try_reserve_exact(attempts.len())?;
        pressed.try_reserve_exact(attempts.len())?;
        classes.try_reserve_exact(attempts.len())?;
        let boundary = attempts[0].1.config;
        for (index, (player, attempt)) in attempts.iter().enumerate() {
            if attempt.config.start != boundary.start
                || attempt.config.end != boundary.end
                || attempt.config.domain != boundary.domain
                || attempt.judge.effective_song_time().is_some()
                || attempt.score != ScoreSummary::default()
                || attempt.judgments != current.live_classes[index]
                || attempt.gauge.profile() != current.snapshot.players[index].gauge.profile()
                || attempt.gauge != BmsGauge::new(attempt.gauge.profile().try_copy()?)
                || &attempt.source.source.compile()? != attempt.judge.chart()
                || next_launch.as_ref().is_some_and(|launch| launch.attempt() != attempt.next_launch.attempt())
            {
                return Err("practice presentation requires fresh coherent selected attempts and pinned policies".into());
            }
            let chart = Arc::new(PlayerChart::from_compiled(&attempt.source, attempt.judge.chart())?);
            let progress = NoteProgress::new(Arc::clone(&chart))?;
            let mut member = LocalPlayerSnapshot::new(*player, Some(chart));
            member.song_time = Some(boundary.start);
            member.gauge = attempt.gauge.clone();
            member.bms_score = attempt.judgments.as_ref().map(|_| BmsScoreSummary::default());
            member.note_progress = Some(progress);
            // Prior ghost progress belongs to the retired attempt. Its owner
            // may publish a newly prepared competition after the common commit.
            members.push(member);
            pressed.push(PressedState::default());
            classes.push(attempt.judgments.clone());
        }
        let mut snapshot = current.snapshot.clone();
        snapshot.players = members;
        snapshot.chart = snapshot.players[0].chart.clone();
        snapshot.completed_end = None;
        snapshot.completed_results = None;
        snapshot.status = PlayerStatus::Playing;
        snapshot.cancelled = false;
        snapshot.sync_legacy();
        // Clone histories/gauge maps while cold, never halfway through commit.
        let publication = snapshot.clone();
        Ok(PreparedPracticePresentation {
            attached: Some(PreparedAttachedPracticePresentation {
                owner: Arc::clone(&current.publisher.0), registered_chart,
                snapshot, publication, pressed, classes, next_launch, launch_publication,
            }),
            expected_generation,
            applied_generation: None,
        })
    })
}

/// Record an already applied, ordered native boundary independently of screen
/// publication. All strings were prepared cold; this only moves complete owners.
/// Cancellation, closing and visual lock poison cannot rewind recording identity.
/// The native caller must qualify and order the actual boundary before calling;
/// this metadata operation is not admission or proof of audio presentation.
pub fn apply_practice_identity(prepared: &mut PreparedPracticePresentation, generation: u64) {
    if generation <= prepared.expected_generation || prepared.applied_generation.is_some() {
        return;
    }
    prepared.applied_generation = Some(generation);
    let Some(staged) = prepared.attached.as_mut() else {
        return;
    };
    if let Some(publication) = staged.launch_publication.take() {
        // This mutex contains only a complete Option swap. Unwinding cannot
        // leave a partially valid SessionLaunch, so metadata poison is recoverable.
        let mut slot = staged
            .owner
            .native_launch
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *slot = Some(publication);
        staged.owner.native_launch.clear_poison();
    }
    SESSION.with(|session| {
        if let Some(current) = session.borrow_mut().as_mut() {
            if Arc::ptr_eq(&current.publisher.0, &staged.owner)
                && current
                    .snapshot
                    .chart
                    .as_ref()
                    .is_some_and(|chart| Arc::ptr_eq(chart, &staged.registered_chart))
            {
                if let Some(launch) = staged.next_launch.take() {
                    current.native_launch = Some(launch);
                }
            }
        }
    });
}

/// Install only after the native owner proves the retained boundary. All checks
/// and the publication lock precede the first swap; no member copy or allocation
/// occurs between swaps. Practice mailboxes and publisher identity are retained.
pub fn commit_practice_presentation(
    mut prepared: PreparedPracticePresentation,
    generation: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    if generation <= prepared.expected_generation {
        return Err("practice presentation generation did not advance".into());
    }
    if prepared.applied_generation.is_none() {
        // Direct convenience callers do not carry separate native qualification.
        // Reject stale/foreign payloads before auto-applying identity, while
        // cancellation and observer failure still preserve a genuine boundary.
        let current_owner = SESSION.with(|session| {
            let session = session.borrow();
            match (session.as_ref(), prepared.attached.as_ref()) {
                (None, None) => true,
                (Some(current), Some(staged)) => {
                    practice_identity_owner_matches(current, staged, prepared.expected_generation)
                }
                _ => false,
            }
        });
        if !current_owner {
            return Err("practice identity requires its current prepared owner".into());
        }
    }
    apply_practice_identity(&mut prepared, generation);
    if prepared.applied_generation != Some(generation) {
        return Err("practice presentation differs from its applied identity generation".into());
    }
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        match (session.as_mut(), prepared.attached) {
            (None, None) => Ok(()),
            (Some(current), Some(staged)) => {
                if !Arc::ptr_eq(&current.publisher.0, &staged.owner)
                    || !practice_session_ready(current, prepared.expected_generation)
                    || !current
                        .snapshot
                        .chart
                        .as_ref()
                        .is_some_and(|chart| Arc::ptr_eq(chart, &staged.registered_chart))
                    || !current
                        .snapshot
                        .players
                        .iter()
                        .map(|member| member.player)
                        .eq(staged.snapshot.players.iter().map(|member| member.player))
                {
                    return Err("practice presentation owner, roster or generation changed".into());
                }
                let mut latest = staged
                    .owner
                    .latest
                    .lock()
                    .map_err(|_| "practice publication unavailable")?;
                current.snapshot = staged.snapshot;
                current.pressed = staged.pressed;
                current.live_classes = staged.classes;
                current.practice_generation = generation;
                current.pause_dirty = false;
                current.room_dirty = false;
                current.last_publish = Some(Instant::now());
                *latest = Some(staged.publication);
                Ok(())
            }
            _ => Err("practice presentation attachment changed".into()),
        }
    })
}

/// Installs the entire registered roster's initial policies once before actual reports.
/// Fallible full copies complete before any member or publication is changed.
pub fn prepare_native_policies(
    policies: &[(PlayerId, &GaugeProfile)],
) -> Result<(), Box<dyn std::error::Error>> {
    prepare_native_policy_rows(policies, Vec::new())
}

/// Cold complete selected policies, independently of replay recording.
pub fn prepare_native_play_policies(
    policies: &[(PlayerId, &ResolvedPlayPolicy)],
) -> Result<(), Box<dyn std::error::Error>> {
    crate::native_gameplay_host::validate_play_policy_members(policies)?;
    let mut gauges = Vec::new();
    let mut classes = Vec::new();
    gauges.try_reserve_exact(policies.len())?;
    classes.try_reserve_exact(policies.len())?;
    for (player, policy) in policies {
        gauges.push((*player, policy.gauge()));
        classes.push(policy.judgments().cloned());
    }
    prepare_native_policy_rows(&gauges, classes)
}

fn prepare_native_policy_rows(
    policies: &[(PlayerId, &GaugeProfile)],
    classes: Vec<Option<BmsJudgmentPolicy>>,
) -> Result<(), Box<dyn std::error::Error>> {
    crate::native_gameplay_host::validate_policy_members(policies)?;
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
        if !current.chart_published
            || current.live_policy_prepared
            || current.replay_policy.is_some()
            || current.snapshot.completed_end.is_some()
            || current.snapshot.completed_results.is_some()
            || !current
                .snapshot
                .players
                .iter()
                .map(|member| member.player)
                .eq(policies.iter().map(|(player, _)| *player))
            || current.snapshot.players.iter().any(|member| {
                member.song_time.is_some()
                    || member.last_judge.is_some()
                    || !member.recent_results.is_empty()
                    || member.score != ScoreSummary::default()
                    || member.mine_damage != MineDamageSummary::default()
                    || member.pressed_lanes != 0
                    || member.gauge != BmsGauge::default()
            })
        {
            return Err("native policies require an exact fresh registered live roster".into());
        }
        let mut gauges = Vec::new();
        gauges.try_reserve_exact(policies.len())?;
        for (_, profile) in policies {
            gauges.push(BmsGauge::new(profile.try_copy()?));
        }
        for (member, gauge) in current.snapshot.players.iter_mut().zip(gauges) {
            member.gauge = gauge;
        }
        for (member, class) in current.snapshot.players.iter_mut().zip(&classes) {
            member.bms_score = class.as_ref().map(|_| BmsScoreSummary::default());
        }
        current.live_classes = classes;
        current.live_policy_prepared = true;
        current.observe_cancellation(false);
        current.publish_latest(true);
        Ok(())
    })
}

/// Summarize one actual solo report once, preserving the existing call shape.
/// Coalescing affects display only. This API cannot modify a local group roster.
pub fn publish_report(report: &RuntimeReport) -> Result<(), Box<dyn std::error::Error>> {
    publish_solo(
        report.song_time,
        &report.judge_events,
        &report.bound_inputs,
        None,
        &report.hazard_events,
        None,
        None,
    )
}

/// Publishes actual incremental replay results through the existing solo bridge.
/// The supplied song is presentation progress, not a fabricated runtime report.
/// Group rejection precedes any score, history or lifecycle mutation.
pub fn publish_replay_prefix(
    song: Timestamp,
    events: &[JudgeEvent],
) -> Result<(), Box<dyn std::error::Error>> {
    publish_replay_prefix_with_pressed(song, events, 0)
}

/// Publishes an actual replay prefix and its independently reconstructed ownership.
/// Without an authoritative gauge, only normal-stage gauge changes are observed;
/// use the full gauge API for mine-aware replay publication.
pub fn publish_replay_prefix_with_pressed(
    song: Timestamp,
    events: &[JudgeEvent],
    mask: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_mask(mask)?;
    publish_solo(song, events, &[], Some(mask), &[], None, None)
}

/// Publishes incremental replay judgments with the actual cumulative mine
/// summary. Equal summaries are assigned without adding their damage again.
/// The summary cannot reconstruct ordered hazard gauge changes; this legacy API
/// observes only normal stages for gauge state. Actual replay uses the full API.
pub fn publish_replay_prefix_with_mines(
    song: Timestamp,
    events: &[JudgeEvent],
    mask: u32,
    summary: MineDamageSummary,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_mask(mask)?;
    validate_mine_summary(MineDamageSummary::default(), summary)?;
    publish_solo(song, events, &[], Some(mask), &[], Some(summary), None)
}

/// Publishes the authoritative replay gauge without observing its results again.
/// The registered replay policy is immutable; legacy registration uses default.
/// A failed gauge clears display ownership regardless of the supplied valid mask.
pub fn publish_replay_prefix_with_gauge(
    song: Timestamp,
    events: &[JudgeEvent],
    mask: u32,
    summary: MineDamageSummary,
    gauge: &BmsGauge,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_mask(mask)?;
    validate_mine_summary(MineDamageSummary::default(), summary)?;
    validate_replay_gauge(None, summary, gauge, gauge.profile())?;
    publish_solo(
        song,
        events,
        &[],
        Some(mask),
        &[],
        Some(summary),
        Some(gauge),
    )
}

fn validate_replay_gauge(
    previous: Option<&BmsGauge>,
    summary: MineDamageSummary,
    gauge: &BmsGauge,
    expected: &GaugeProfile,
) -> Result<(), &'static str> {
    if gauge.profile() != expected {
        return Err("replay presentation gauge differs from its registered policy");
    }
    let snapshot = gauge.snapshot();
    let failure_valid = match snapshot.failure {
        None => !summary.instant_death,
        Some(GaugeFailure::InstantDeath) => summary.instant_death && snapshot.level_units == 0,
        Some(GaugeFailure::Depleted) => expected.fail_on_empty() && snapshot.level_units == 0,
    };
    let default_consistent = expected != &GaugeProfile::default()
        || summary.instant_death == (snapshot.failure == Some(GaugeFailure::InstantDeath));
    if !failure_valid || !default_consistent {
        return Err("replay gauge failure differs from its cumulative mine summary");
    }
    if previous.is_some_and(|previous| {
        previous.snapshot().failure.is_some() && previous.snapshot() != snapshot
    }) {
        return Err("replay gauge changed after its latched failure");
    }
    Ok(())
}

fn validate_mine_summary(
    previous: MineDamageSummary,
    next: MineDamageSummary,
) -> Result<(), &'static str> {
    if next.triggered < previous.triggered
        || next.avoided < previous.avoided
        || next.half_percent_damage < previous.half_percent_damage
        || (previous.instant_death && !next.instant_death)
    {
        return Err("replay mine summary regressed");
    }
    let nonfatal_bound = next
        .triggered
        .checked_sub(u64::from(next.instant_death))
        .ok_or("replay mine death requires a triggered marker")?;
    if u128::from(next.half_percent_damage) > u128::from(nonfatal_bound) * 1294 {
        return Err("replay mine damage exceeds its triggered marker bound");
    }
    Ok(())
}

fn publish_solo(
    song: Timestamp,
    events: &[JudgeEvent],
    inputs: &[GameInputEvent],
    replay_mask: Option<u32>,
    hazards: &[HazardEvent],
    replay_mines: Option<MineDamageSummary>,
    replay_gauge: Option<&BmsGauge>,
) -> Result<(), Box<dyn std::error::Error>> {
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
        if !current.snapshot.players.is_empty()
            && (current.snapshot.players.len() != 1
                || current.snapshot.players[0].player != PlayerId(1))
        {
            return Err("solo report cannot modify a local group presentation".into());
        }
        let mut fresh = current.snapshot.players.is_empty().then(|| {
            (
                LocalPlayerSnapshot::new(PlayerId(1), current.snapshot.chart.clone()),
                PressedState::default(),
            )
        });
        let (member, pressed) = if let Some((member, pressed)) = fresh.as_mut() {
            (member, pressed)
        } else {
            (&mut current.snapshot.players[0], &mut current.pressed[0])
        };
        let gauge = if let Some(gauge) = replay_gauge {
            validate_replay_gauge(
                Some(&member.gauge),
                replay_mines.expect("full replay gauge publication supplies a mine summary"),
                gauge,
                current
                    .replay_policy
                    .as_ref()
                    .unwrap_or(&GaugeProfile::default()),
            )?;
            Some(gauge.clone())
        } else if events.is_empty() && hazards.is_empty() {
            None
        } else {
            let mut gauge = member.gauge.clone();
            gauge.observe(events, hazards)?;
            Some(gauge)
        };
        let score = if events.is_empty() {
            None
        } else {
            let mut score = member.score.clone();
            score.observe(events)?;
            Some(score)
        };
        let bms_score = if let Some(score) = &score {
            current
                .live_classes
                .first()
                .and_then(Option::as_ref)
                .map(|policy| policy.project(score))
                .transpose()?
        } else {
            member.bms_score
        };
        let mine_damage = if let Some(summary) = replay_mines {
            validate_mine_summary(member.mine_damage, summary)?;
            summary
        } else {
            let mut summary = member.mine_damage;
            summary.observe(hazards)?;
            summary
        };
        let prepared = pressed.keys.prepare(inputs)?;
        if gauge
            .as_ref()
            .unwrap_or(&member.gauge)
            .snapshot()
            .failure
            .is_some()
        {
            pressed.keys.clear();
            pressed.mask = 0;
        } else {
            if let Some(mask) = prepared {
                pressed.keys.commit(mask);
                pressed.mask = mask;
            }
            if let Some(mask) = replay_mask {
                pressed.mask = mask;
            }
        }
        if let Some(score) = score {
            member.score = score;
        }
        member.bms_score = bms_score;
        member.mine_damage = mine_damage;
        if let Some(gauge) = gauge {
            member.gauge = gauge;
        }
        member.update_results(song, events);
        if let Some((member, pressed)) = fresh {
            current.snapshot.players.push(member);
            current.pressed.push(pressed);
        }
        current.observe_cancellation(true);
        current.publish_latest(false);
        Ok(())
    })
}

/// Apply member-tagged actual reports, validating the whole batch before mutation.
///
/// IDs must be registered and unique within a batch. Counter overflow leaves
/// every member unchanged. Empty batches do not invent a playing state. This is
/// called only by the native game owner; no audio or UI timestamps are created.
/// Unattached terminal execution has no publication state and remains a no-op.
pub fn publish_local_reports(reports: &[PlayerReport]) -> Result<(), Box<dyn std::error::Error>> {
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
        if reports.is_empty() {
            return Ok(());
        }
        if reports.len() > 64 {
            return Err("local presentation report batch exceeds 64 members".into());
        }
        for (index, report) in reports.iter().enumerate() {
            if report.player.0 == 0
                || reports[..index]
                    .iter()
                    .any(|prior| prior.player == report.player)
            {
                return Err("local presentation report IDs must be unique and nonzero".into());
            }
            if !current
                .snapshot
                .players
                .iter()
                .any(|member| member.player == report.player)
            {
                return Err("local presentation report has an unregistered player".into());
            }
        }
        // Prepare only changed scores: empty 1ms deadline reports do not clone
        // history/Arc/score maps or clone gauge profiles. All score/damage/gauge errors precede
        // any mutation of any member in this batch.
        let mut changed_scores = Vec::new();
        let mut changed_mines: [Option<MineDamageSummary>; 64] = [None; 64];
        let mut changed_gauges: [Option<BmsGauge>; 64] = std::array::from_fn(|_| None);
        for report in reports {
            if report.report.judge_events.is_empty() && report.report.hazard_events.is_empty() {
                continue;
            }
            let index = current
                .snapshot
                .players
                .iter()
                .position(|member| member.player == report.player)
                .expect("validated registered player");
            let mut gauge = current.snapshot.players[index].gauge.clone();
            gauge.observe(&report.report.judge_events, &report.report.hazard_events)?;
            changed_gauges[index] = Some(gauge);
            if !report.report.judge_events.is_empty() {
                let mut score = current.snapshot.players[index].score.clone();
                score.observe(&report.report.judge_events)?;
                let bms_score = current
                    .live_classes
                    .get(index)
                    .and_then(Option::as_ref)
                    .map(|policy| policy.project(&score))
                    .transpose()?;
                changed_scores.push((index, score, bms_score));
            }
            if !report.report.hazard_events.is_empty() {
                let mut summary = current.snapshot.players[index].mine_damage;
                summary.observe(&report.report.hazard_events)?;
                changed_mines[index] = Some(summary);
            }
        }
        let mut changed_pressed = Vec::new();
        for report in reports {
            let index = current
                .snapshot
                .players
                .iter()
                .position(|member| member.player == report.player)
                .expect("validated registered player");
            if let Some(mask) = current.pressed[index]
                .keys
                .prepare(&report.report.bound_inputs)?
            {
                changed_pressed.push((index, mask));
            }
        }
        for (index, mask) in changed_pressed {
            if changed_gauges[index]
                .as_ref()
                .unwrap_or(&current.snapshot.players[index].gauge)
                .snapshot()
                .failure
                .is_none()
            {
                current.pressed[index].keys.commit(mask);
                current.pressed[index].mask = mask;
            }
        }
        for (index, score, bms_score) in changed_scores {
            current.snapshot.players[index].score = score;
            current.snapshot.players[index].bms_score = bms_score;
        }
        for report in reports {
            let (index, member) = current
                .snapshot
                .players
                .iter_mut()
                .enumerate()
                .find(|(_, member)| member.player == report.player)
                .expect("validated registered player");
            if let Some(summary) = changed_mines[index] {
                member.mine_damage = summary;
            }
            if let Some(gauge) = changed_gauges[index].take() {
                member.gauge = gauge;
            }
            if member.gauge.snapshot().failure.is_some() {
                current.pressed[index].keys.clear();
                current.pressed[index].mask = 0;
            }
            member.update_report(&report.report);
        }
        current.observe_cancellation(true);
        current.publish_latest(false);
        Ok(())
    })
}

fn validate_players(players: &[PlayerId]) -> Result<(), Box<dyn std::error::Error>> {
    if !(1..=64).contains(&players.len()) {
        return Err("local presentation requires 1..64 players".into());
    }
    for (index, player) in players.iter().enumerate() {
        if player.0 == 0 || players[..index].contains(player) {
            return Err("local presentation IDs must be unique and nonzero".into());
        }
    }
    Ok(())
}
impl PlayerSnapshot {
    /// Validate the complete registered roster before committing any result.
    /// Input order is normalized to registered player order; exact repeats are idempotent.
    pub fn apply_completed_results(
        &mut self,
        rows: &[(PlayerId, CompletedPlayResult)],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.players.is_empty() || self.players.len() > 64 || rows.len() != self.players.len() {
            return Err("completed results require the whole registered roster".into());
        }
        let scope = rows[0].1.scope();
        for (index, (player, result)) in rows.iter().enumerate() {
            if player.0 == 0 || rows[..index].iter().any(|(previous, _)| previous == player) {
                return Err("completed results duplicate a player".into());
            }
            let member = self
                .players
                .iter()
                .find(|member| member.player == *player)
                .ok_or("completed results require registered players")?;
            if result.scope() != scope || result.gauge() != *member.gauge.snapshot() {
                return Err("completed results disagree with scope or registered gauge".into());
            }
        }
        for (index, member) in self.players.iter().enumerate() {
            if member.player.0 == 0
                || self.players[..index]
                    .iter()
                    .any(|previous| previous.player == member.player)
                || !rows.iter().any(|(player, _)| *player == member.player)
            {
                return Err("completed results require a unique complete registered roster".into());
            }
        }
        if let Some(previous) = &self.completed_results {
            if previous.len() == self.players.len()
                && previous
                    .iter()
                    .zip(&self.players)
                    .all(|((player, result), member)| {
                        *player == member.player
                            && rows.iter().any(|row| row == &(*player, *result))
                    })
            {
                return Ok(false);
            }
            return Err("completed results cannot replace the first completion table".into());
        }
        let mut staged = Vec::new();
        staged.try_reserve_exact(self.players.len())?;
        for member in &self.players {
            let row = rows
                .iter()
                .find(|(player, _)| *player == member.player)
                .ok_or("completed results omitted a registered player")?;
            staged.push(*row);
        }
        self.completed_results = Some(staged);
        Ok(true)
    }

    // Populate legacy fields only for actual handoff, avoiding report-frequency
    // cloning of bounded history/grade maps while retaining solo UI call shapes.
    fn sync_legacy(&mut self) {
        if self.players.len() == 1 {
            let member = &self.players[0];
            self.note_progress = member.note_progress.clone();
            self.pressed_lanes = member.pressed_lanes;
            self.song_time = member.song_time;
            self.score = member.score.clone();
            self.bms_score = member.bms_score;
            self.mine_damage = member.mine_damage;
            self.gauge.clone_from(&member.gauge);
            self.last_judge = member.last_judge;
            self.recent_results = member.recent_results.clone();
        } else {
            self.note_progress = None;
            self.pressed_lanes = 0;
            self.song_time = None;
            self.score = ScoreSummary::default();
            self.bms_score = None;
            self.mine_damage = MineDamageSummary::default();
            self.gauge = BmsGauge::default();
            self.last_judge = None;
            self.recent_results.clear();
        }
    }
}
impl Session {
    fn clear_pressed(&mut self) {
        for state in &mut self.pressed {
            state.keys.clear();
            state.mask = 0;
        }
        self.snapshot.pressed_lanes = 0;
        for member in &mut self.snapshot.players {
            member.pressed_lanes = 0;
        }
    }
    fn sync_pressed(&mut self) {
        let visible = !self.snapshot.cancelled
            && matches!(
                self.snapshot.pause,
                PauseState::Running | PauseState::Unavailable
            );
        for (member, state) in self.snapshot.players.iter_mut().zip(&self.pressed) {
            member.pressed_lanes = if visible { state.mask } else { 0 };
        }
        self.snapshot.pressed_lanes = if self.snapshot.players.len() == 1 {
            self.snapshot.players[0].pressed_lanes
        } else {
            0
        };
    }
    fn observe_cancellation(&mut self, playing: bool) {
        if self.publisher.0.cancel.load(Ordering::Acquire) {
            self.snapshot.cancelled = true;
            self.snapshot.status = PlayerStatus::Stopping;
            self.clear_pressed();
        } else if playing {
            self.snapshot.status = PlayerStatus::Playing;
        }
        self.sync_pressed();
    }
    fn publish_latest(&mut self, force: bool) {
        let room_published = if self.room_dirty {
            if let Ok(mut controls) = self.publisher.0.room.try_lock() {
                controls.presentation = self.snapshot.room.clone();
                true
            } else {
                false
            }
        } else {
            true
        };
        if force
            || self
                .last_publish
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(8))
        {
            if let Ok(mut slot) = self.publisher.0.latest.try_lock() {
                self.snapshot.sync_legacy();
                *slot = Some(self.snapshot.clone());
                self.last_publish = Some(Instant::now());
                self.pause_dirty = false;
                self.room_dirty &= !room_published;
            }
        }
    }
}

#[cfg(test)]
#[path = "gauge_pressed_native_fixtures.rs"]
mod gauge_pressed_fixtures;

#[cfg(test)]
#[path = "room_presentation_fixtures.rs"]
mod room_presentation_fixtures;

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn native_endpoint_publication_is_independent_of_ui_time_and_cleanup_result() {
        let end = Timestamp::from_nanos(604_800_000_000_001);
        let (publisher, viewer) = channel();
        assert_eq!(viewer.take_latest().unwrap().completed_end, None);
        with_publisher(publisher, || {
            assert_eq!(
                SESSION.with(|session| session.borrow().as_ref().unwrap().snapshot.completed_end),
                None
            );
            publish_section_end(end);
            assert_eq!(
                SESSION.with(|session| session.borrow().as_ref().unwrap().snapshot.status.clone()),
                PlayerStatus::Loading
            );
            Ok(())
        })
        .unwrap();
        let final_state = viewer.take_latest().unwrap();
        assert_eq!(final_state.completed_end, Some(end));
        assert_eq!(final_state.status, PlayerStatus::Finished);
        assert!(!final_state.cancelled);
        let (publisher, viewer) = channel();
        with_publisher(publisher, || Ok(())).unwrap(); // Diagnostic/ordinary return.
        assert_eq!(viewer.take_latest().unwrap().completed_end, None);
        let (publisher, viewer) = channel();
        assert!(with_publisher::<()>(publisher, || {
            publish_section_end(end);
            Err("cleanup fixture".into())
        })
        .is_err());
        let failed = viewer.take_latest().unwrap();
        assert_eq!(failed.completed_end, Some(end)); // Keep actual prefix provenance.
        assert_eq!(
            failed.status,
            PlayerStatus::Failed("cleanup fixture".into())
        );
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            publish_section_end(end);
            viewer.cancel();
            Ok(())
        })
        .unwrap();
        let cancelled = viewer.take_latest().unwrap();
        assert!(cancelled.cancelled);
        assert_eq!(cancelled.completed_end, Some(end));
    }

    #[test]
    fn pause_requests_require_owner_ack_and_cancellation_wins_across_fresh_channels() {
        let (publisher, viewer) = channel();
        assert_eq!(viewer.take_latest().unwrap().pause, PauseState::Unavailable);
        viewer.request_pause(true);
        with_publisher(publisher, || {
            assert!(pause_requested());
            let held_slot = viewer.0.latest.lock().unwrap();
            publish_pause(PauseState::Running);
            drop(held_slot);
            retry_pause_publication();
            assert_eq!(viewer.take_latest().unwrap().pause, PauseState::Running);
            publish_pause(PauseState::Pausing);
            assert_eq!(viewer.take_latest().unwrap().pause, PauseState::Pausing);
            publish_pause(PauseState::Paused);
            assert_eq!(viewer.take_latest().unwrap().pause, PauseState::Paused);
            viewer.request_pause(false);
            assert!(!pause_requested());
            publish_pause(PauseState::Resuming);
            assert_eq!(viewer.take_latest().unwrap().pause, PauseState::Resuming);
            viewer.cancel();
            viewer.request_pause(true);
            assert!(!pause_requested());
            publish_pause(PauseState::Paused);
            let stopped = viewer.take_latest().unwrap();
            assert!(stopped.cancelled);
            assert_eq!(stopped.status, PlayerStatus::Stopping);
            Ok(())
        })
        .unwrap();
        assert!(viewer.take_latest().unwrap().cancelled);
        let (fresh, fresh_viewer) = channel();
        with_publisher(fresh, || {
            assert!(!pause_requested());
            assert_eq!(
                fresh_viewer.take_latest().unwrap().pause,
                PauseState::Unavailable
            );
            Ok(())
        })
        .unwrap();
    }
    fn comparisons() -> CompetitionSnapshot {
        CompetitionSnapshot {
            ghosts: vec![GhostSnapshot {
                kind: OpponentKind::Own,
                label: "old.bkr".into(),
                hits: 12,
                misses: 2,
                combo: 3,
                max_combo: 8,
                recorded_until: Some(Timestamp::from_nanos(99)),
            }],
            network: Some(NetworkSnapshot {
                status: NetworkStatus::Disconnected,
                progress: Some(crate::multiplayer::Progress {
                    song_ns: 1_000_000_000,
                    hits: 7,
                    misses: 1,
                    combo: 2,
                    max_combo: 5,
                }),
            }),
        }
    }
    #[test]
    fn competition_is_member_specific_bounded_and_retained_through_cleanup() {
        let (source, chart) = chart_fixture();
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            publish_local_chart(&source, &chart, &[PlayerId(3), PlayerId(9)]).unwrap();
            let value = comparisons();
            publish_competition(PlayerId(9), value.clone()).unwrap();
            assert!(publish_competition(PlayerId(1), value.clone()).is_err());
            let mut oversized = value.clone();
            oversized.ghosts = vec![value.ghosts[0].clone(); 9];
            assert!(publish_competition(PlayerId(9), oversized).is_err());
            let mut bad_label = value;
            bad_label.ghosts[0].label = "bad\nlabel".into();
            assert!(publish_competition(PlayerId(9), bad_label).is_err());
            publish_local_reports(&[report(3, 50, 1, 0), report(9, 50, 2, 0)]).unwrap();
            viewer.cancel();
            Ok(())
        })
        .unwrap();
        let final_state = viewer.take_latest().unwrap();
        assert_eq!(final_state.players[0].competition, None);
        assert_eq!(final_state.players[1].competition, Some(comparisons()));
        assert_eq!(final_state.players[1].score.hits, 2);
        assert!(final_state.cancelled);
        assert_eq!(final_state.status, PlayerStatus::Finished);
    }
    fn chart_fixture() -> (BmsChart, CompiledChart) {
        let source = beatkernel_bms::parse(
            "#TITLE Local\n#BPM 120\n#WAV01 tap.wav\n#00011:01\n",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let chart = source.compile().unwrap().chart;
        (source, chart)
    }
    fn report(player: u32, nanos: i64, hit_count: usize, misses: usize) -> PlayerReport {
        use beatkernel::{
            chart::ObjectId,
            judge::{JudgeGrade, JudgeOutcome, JudgeStage, MissReason},
            time::{ClockDomainId, ClockMappingQuality, ClockPoint},
        };
        let song_time = Timestamp::from_nanos(nanos);
        let judge_events = (0..hit_count + misses)
            .map(|index| JudgeEvent {
                object: ObjectId(index as u64 + 1),
                stage: JudgeStage::Instant,
                outcome: if index < hit_count {
                    JudgeOutcome::Hit {
                        grade: JudgeGrade(1),
                        delta: beatkernel::time::Duration::ZERO,
                    }
                } else {
                    JudgeOutcome::Miss {
                        reason: MissReason::HeadTimeout,
                    }
                },
                at: song_time,
                input: None,
            })
            .collect();
        PlayerReport {
            player: PlayerId(player),
            report: RuntimeReport {
                input: None,
                bound_inputs: Vec::new(),
                song_time,
                song_end_reached: false,
                audio_at: ClockPoint {
                    domain: ClockDomainId(2),
                    timestamp: song_time,
                },
                input_mapping_quality: ClockMappingQuality::Unknown,
                audio_mapping_quality: ClockMappingQuality::Unknown,
                judge_events,
                hazard_events: Vec::new(),
                judge_error: None,
                audio_commands: Vec::new(),
                audio_failures: Vec::new(),
            },
        }
    }
    fn member_state() -> Vec<(PlayerId, ScoreSummary, Option<Timestamp>, usize)> {
        SESSION.with(|session| {
            session
                .borrow()
                .as_ref()
                .unwrap()
                .snapshot
                .players
                .iter()
                .map(|member| {
                    (
                        member.player,
                        member.score.clone(),
                        member.song_time,
                        member.recent_results.len(),
                    )
                })
                .collect()
        })
    }

    #[test]
    fn four_player_reports_keep_independent_results_shared_chart_and_cancelled_members() {
        let (source, chart) = chart_fixture();
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            publish_local_chart(
                &source,
                &chart,
                &[PlayerId(1), PlayerId(2), PlayerId(3), PlayerId(4)],
            )
            .map_err(|error| error.to_string())?;
            let initial = viewer.take_latest().unwrap();
            assert_eq!(initial.players.len(), 4);
            let common = initial.chart.as_ref().unwrap();
            assert!(initial
                .players
                .iter()
                .all(|member| Arc::ptr_eq(member.chart.as_ref().unwrap(), common)));
            publish_local_reports(&[
                report(1, 1, 2, 0),
                report(2, 2, 0, 1),
                report(3, 3, 1, 0),
                report(4, 4, 0, 0),
            ])
            .map_err(|error| error.to_string())?;
            viewer.cancel();
            assert!(cancelled());
            Ok(())
        })
        .unwrap();
        let terminal = viewer.take_latest().unwrap();
        assert_eq!(terminal.status, PlayerStatus::Finished);
        assert!(terminal.cancelled);
        assert_eq!(
            terminal
                .players
                .iter()
                .map(|member| (member.score.hits, member.score.misses))
                .collect::<Vec<_>>(),
            vec![(2, 0), (0, 1), (1, 0), (0, 0)]
        );
        assert_eq!(
            terminal
                .players
                .iter()
                .map(|member| member.song_time.unwrap().as_nanos())
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(terminal.score, ScoreSummary::default());
        assert_eq!(terminal.song_time, None);
        assert!(terminal.recent_results.is_empty());
    }

    #[test]
    fn unknown_duplicate_overflow_and_solo_reports_leave_whole_local_batch_unchanged() {
        let (source, chart) = chart_fixture();
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            publish_local_chart(&source, &chart, &[PlayerId(1), PlayerId(2)])
                .map_err(|error| error.to_string())?;
            let baseline = member_state();
            assert!(publish_local_reports(&[report(1, 10, 1, 0), report(99, 10, 1, 0)]).is_err());
            assert_eq!(member_state(), baseline);
            assert!(publish_local_reports(&[report(1, 10, 1, 0), report(1, 10, 1, 0)]).is_err());
            assert_eq!(member_state(), baseline);
            assert!(publish_report(&report(1, 10, 1, 0).report).is_err());
            assert_eq!(member_state(), baseline);
            assert!(publish_local_chart(&source, &chart, &[PlayerId(1)]).is_err());
            assert_eq!(member_state(), baseline);
            SESSION.with(|session| {
                session.borrow_mut().as_mut().unwrap().snapshot.players[1]
                    .score
                    .hits = u64::MAX
            });
            let before_overflow = member_state();
            assert!(publish_local_reports(&[report(1, 20, 1, 0), report(2, 20, 1, 0)]).is_err());
            assert_eq!(member_state(), before_overflow);
            Ok(())
        })
        .unwrap();
        assert_eq!(viewer.take_latest().unwrap().players.len(), 2);
    }

    #[test]
    fn registration_bounds_history_and_solo_legacy_fields_are_preserved() {
        let (source, chart) = chart_fixture();
        assert!(publish_local_chart(&source, &chart, &[]).is_err());
        assert!(publish_local_chart(&source, &chart, &[PlayerId(0)]).is_err());
        assert!(publish_local_chart(&source, &chart, &[PlayerId(1), PlayerId(1)]).is_err());
        let over_capacity = (1..=65).map(PlayerId).collect::<Vec<_>>();
        assert!(publish_local_chart(&source, &chart, &over_capacity).is_err());
        assert!(validate_players(&(1..=64).map(PlayerId).collect::<Vec<_>>()).is_ok());
        assert!(validate_players(&[PlayerId(u32::MAX)]).is_ok());
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            // Preserve legacy report-before-chart behavior without changing IDs.
            publish_report(&report(1, 100, 300, 0).report).map_err(|error| error.to_string())?;
            publish_chart(&source, &chart).map_err(|error| error.to_string())?;
            publish_report(&report(1, 200, 1, 1).report).map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();
        let terminal = viewer.take_latest().unwrap();
        let member = &terminal.players[0];
        assert_eq!(member.player, PlayerId(1));
        assert_eq!(member.score.hits, 301);
        assert_eq!(member.score.misses, 1);
        assert_eq!(member.recent_results.len(), 128);
        assert_eq!(member.recent_results.first().unwrap().object.0, 175);
        assert_eq!(member.recent_results.last(), member.last_judge.as_ref());
        assert_eq!(terminal.score, member.score);
        assert_eq!(terminal.song_time, member.song_time);
        assert_eq!(terminal.last_judge, member.last_judge);
        assert_eq!(terminal.recent_results, member.recent_results);
        assert!(Arc::ptr_eq(
            terminal.chart.as_ref().unwrap(),
            member.chart.as_ref().unwrap()
        ));
    }

    #[test]
    fn empty_deadline_reports_do_not_clone_chart_or_history_before_publication_lock() {
        let (source, chart) = chart_fixture();
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            publish_local_chart(&source, &chart, &[PlayerId(1), PlayerId(2)])
                .map_err(|error| error.to_string())?;
            let common = SESSION.with(|session| {
                Arc::clone(
                    session
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .snapshot
                        .chart
                        .as_ref()
                        .unwrap(),
                )
            });
            let before = Arc::strong_count(&common);
            let lock = viewer.0.latest.lock().unwrap();
            SESSION.with(|session| session.borrow_mut().as_mut().unwrap().last_publish = None);
            publish_local_reports(&[report(1, 100, 0, 0), report(2, 100, 0, 0)])
                .map_err(|error| error.to_string())?;
            assert_eq!(Arc::strong_count(&common), before);
            assert_eq!(
                member_state()
                    .iter()
                    .map(|member| member.2.unwrap().as_nanos())
                    .collect::<Vec<_>>(),
                vec![100, 100]
            );
            drop(lock);
            Ok(())
        })
        .unwrap();
        assert_eq!(viewer.take_latest().unwrap().players.len(), 2);
    }

    #[test]
    fn cancellation_and_terminal_state_belong_to_one_session() {
        let (publisher, viewer) = channel();
        assert_eq!(viewer.take_latest().unwrap().status, PlayerStatus::Loading);
        with_publisher(publisher, || {
            assert!(attached());
            viewer.cancel();
            assert!(cancelled());
            Ok(())
        })
        .unwrap();
        let snapshot = viewer.take_latest().unwrap();
        assert!(snapshot.cancelled);
        assert_eq!(snapshot.status, PlayerStatus::Finished);
        assert!(!attached());
        let (_, next) = channel();
        assert!(!next.take_latest().unwrap().cancelled);
    }
    #[test]
    fn failure_is_published_without_panicking_or_leaking_attachment() {
        let (publisher, viewer) = channel();
        let result: Result<(), String> =
            with_publisher(publisher, || Err("device rejected".into()));
        assert!(result.is_err());
        assert_eq!(
            viewer.take_latest().unwrap().status,
            PlayerStatus::Failed("device rejected".into())
        );
        assert!(!attached());
    }
    #[test]
    fn replay_prefix_bridge_counts_once_bounds_history_and_rejects_groups_atomically() {
        let (source, chart) = chart_fixture();
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            publish_chart(&source, &chart).unwrap();
            let actual = report(1, 150_000_000, 300, 1).report.judge_events;
            publish_replay_prefix(Timestamp::from_nanos(150_000_000), &actual).unwrap();
            publish_replay_prefix(Timestamp::from_nanos(200_000_000), &[]).unwrap();
            viewer.cancel();
            Ok(())
        })
        .unwrap();
        let terminal = viewer.take_latest().unwrap();
        assert_eq!((terminal.score.hits, terminal.score.misses), (300, 1));
        assert_eq!(terminal.song_time, Some(Timestamp::from_nanos(200_000_000)));
        assert_eq!(terminal.recent_results.len(), 128);
        assert_eq!(terminal.last_judge, terminal.recent_results.last().copied());
        assert!(terminal.cancelled);
        let (publisher, _) = channel();
        with_publisher(publisher, || {
            publish_local_chart(&source, &chart, &[PlayerId(1), PlayerId(2)]).unwrap();
            let before = member_state();
            assert!(publish_replay_prefix(
                Timestamp::ZERO,
                &report(1, 0, 1, 0).report.judge_events
            )
            .is_err());
            assert_eq!(member_state(), before);
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn admitted_buttons_are_member_specific_and_pause_preserves_ownership() {
        use crate::pressed_keys::fixtures::button;
        use beatkernel::input::ButtonState;
        let (source, chart) = chart_fixture();
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            publish_local_chart(&source, &chart, &[PlayerId(3), PlayerId(u32::MAX)]).unwrap();
            let mut a = report(3, 10, 0, 0);
            a.report.bound_inputs = vec![
                button(1, 4, 0x11, ButtonState::Down),
                button(2, 4, 0x11, ButtonState::Down),
            ];
            let mut b = report(u32::MAX, 10, 0, 0);
            b.report.bound_inputs = vec![button(1, 4, 0x29, ButtonState::Down)];
            publish_local_reports(&[a, b]).unwrap();
            publish_pause(PauseState::Running);
            let active = viewer.take_latest().unwrap();
            assert_eq!(
                active
                    .players
                    .iter()
                    .map(|p| p.pressed_lanes)
                    .collect::<Vec<_>>(),
                vec![1, 1 << 17]
            );
            assert_eq!(active.pressed_lanes, 0);
            for phase in [
                PauseState::Pausing,
                PauseState::Paused,
                PauseState::Resuming,
            ] {
                publish_pause(phase);
                assert!(viewer
                    .take_latest()
                    .unwrap()
                    .players
                    .iter()
                    .all(|p| p.pressed_lanes == 0));
            }
            let mut release = report(3, 20, 0, 0);
            release.report.bound_inputs = vec![button(1, 4, 0x11, ButtonState::Up)];
            publish_local_reports(&[release]).unwrap();
            publish_pause(PauseState::Running);
            assert_eq!(viewer.take_latest().unwrap().players[0].pressed_lanes, 1);
            viewer.cancel();
            publish_pause(PauseState::Unavailable);
            assert!(viewer
                .take_latest()
                .unwrap()
                .players
                .iter()
                .all(|p| p.pressed_lanes == 0));
            Ok(())
        })
        .unwrap();
        assert!(viewer
            .take_latest()
            .unwrap()
            .players
            .iter()
            .all(|p| p.pressed_lanes == 0));
    }
    #[test]
    fn ownership_overflow_is_atomic_across_scores_history_and_members() {
        use crate::pressed_keys::fixtures::button;
        use beatkernel::input::ButtonState;
        let (source, chart) = chart_fixture();
        let (publisher, _viewer) = channel();
        with_publisher(publisher, || {
            publish_local_chart(&source, &chart, &[PlayerId(1), PlayerId(2)]).unwrap();
            let mut full = report(2, 1, 0, 0);
            full.report.bound_inputs = (0..4096)
                .map(|i| button(i, 4, 0x11, ButtonState::Down))
                .collect();
            publish_local_reports(&[full]).unwrap();
            let before = member_state();
            let mut first = report(1, 2, 1, 0);
            first.report.bound_inputs = vec![button(1, 4, 0x29, ButtonState::Down)];
            let mut overflow = report(2, 2, 1, 0);
            overflow.report.bound_inputs = vec![button(5000, 4, 0x29, ButtonState::Down)];
            assert!(publish_local_reports(&[first, overflow]).is_err());
            assert_eq!(member_state(), before);
            SESSION.with(|s| {
                let s = s.borrow();
                let s = s.as_ref().unwrap();
                assert_eq!(s.pressed[0].keys.mask(), 0);
                assert_eq!(s.pressed[1].keys.mask(), 1);
            });
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn solo_bound_inputs_and_replay_mask_restore_then_terminal_clear() {
        use crate::pressed_keys::fixtures::button;
        use beatkernel::input::ButtonState;
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            let mut actual = report(1, 1, 0, 0);
            actual.report.bound_inputs = vec![button(1, 4, 0x21, ButtonState::Down)];
            publish_report(&actual.report).unwrap();
            publish_pause(PauseState::Running);
            assert_eq!(viewer.take_latest().unwrap().pressed_lanes, 1 << 9);
            publish_replay_prefix_with_pressed(Timestamp::from_nanos(2), &[], 1 << 17).unwrap();
            publish_pause(PauseState::Paused);
            assert_eq!(viewer.take_latest().unwrap().pressed_lanes, 0);
            publish_pause(PauseState::Running);
            assert_eq!(viewer.take_latest().unwrap().pressed_lanes, 1 << 17);
            assert!(publish_replay_prefix_with_pressed(Timestamp::ZERO, &[], 1 << 18).is_err());
            Ok(())
        })
        .unwrap();
        let terminal = viewer.take_latest().unwrap();
        assert_eq!(terminal.pressed_lanes, 0);
        assert_eq!(terminal.players[0].pressed_lanes, 0);
        let (fresh, viewer) = channel();
        with_publisher(fresh, || {
            publish_replay_prefix(Timestamp::ZERO, &[]).unwrap();
            publish_pause(PauseState::Running);
            assert_eq!(viewer.take_latest().unwrap().pressed_lanes, 0);
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn failed_owner_return_clears_admitted_presses_after_cleanup() {
        use crate::pressed_keys::fixtures::button;
        use beatkernel::input::ButtonState;
        let (publisher, viewer) = channel();
        let result = with_publisher::<()>(publisher, || {
            let mut admitted = report(1, 1, 0, 0);
            admitted.report.bound_inputs = vec![button(1, 4, 0x11, ButtonState::Down)];
            publish_report(&admitted.report).unwrap();
            publish_pause(PauseState::Running);
            assert_eq!(viewer.take_latest().unwrap().pressed_lanes, 1);
            Err("owner cleanup failure".into())
        });
        assert!(result.is_err());
        let failed = viewer.take_latest().unwrap();
        assert_eq!(
            failed.status,
            PlayerStatus::Failed("owner cleanup failure".into())
        );
        assert_eq!(failed.pressed_lanes, 0);
        assert_eq!(failed.players[0].pressed_lanes, 0);
    }
    #[test]
    fn progress_keeps_full_prefix_and_independent_members_through_pause_and_cleanup() {
        use crate::note_progress::NoteState;
        let (source, chart) = chart_fixture();
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            publish_local_chart(&source, &chart, &[PlayerId(3), PlayerId(u32::MAX)]).unwrap();
            let prepared = SESSION.with(|s| {
                s.borrow().as_ref().unwrap().snapshot.players[0]
                    .chart
                    .clone()
                    .unwrap()
            });
            let old = SESSION.with(|s| {
                s.borrow().as_ref().unwrap().snapshot.players[0]
                    .note_progress
                    .clone()
                    .unwrap()
            });
            let mut many = report(3, 100, 129, 0);
            many.report.judge_events[0].object = prepared.notes[0].object;
            publish_local_reports(&[many]).unwrap();
            publish_pause(PauseState::Paused);
            let snapshot = viewer.take_latest().unwrap();
            assert_eq!(snapshot.players[0].recent_results.len(), 128);
            assert_eq!(
                snapshot.players[0].note_progress.as_ref().unwrap().state(0),
                Some(NoteState::Completed)
            );
            assert_eq!(
                snapshot.players[1].note_progress.as_ref().unwrap().state(0),
                Some(NoteState::Pending)
            );
            assert_eq!(old.state(0), Some(NoteState::Pending));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            viewer.take_latest().unwrap().players[0]
                .note_progress
                .as_ref()
                .unwrap()
                .state(0),
            Some(NoteState::Completed)
        );
    }
    #[test]
    fn chart_after_results_cannot_reconstruct_history_but_empty_reports_allow_fresh_state() {
        let (source, chart) = chart_fixture();
        for prior_results in [false, true] {
            let (publisher, viewer) = channel();
            with_publisher(publisher, || {
                let actual = report(1, 0, usize::from(prior_results), 0);
                publish_report(&actual.report).unwrap();
                publish_chart(&source, &chart).unwrap();
                let snapshot = viewer.take_latest().unwrap();
                assert_eq!(snapshot.players[0].note_progress.is_some(), !prior_results);
                assert_eq!(snapshot.note_progress.is_some(), !prior_results);
                Ok(())
            })
            .unwrap();
        }
    }
}

#[cfg(test)]
#[path = "player_completed_result_fixtures.rs"]
mod completed_result_fixtures;

#[cfg(test)]
mod live_output_channel_fixtures {
    crate::live_output_control::fixtures::player_channel_tests!();
}

#[cfg(test)]
#[path = "player_output_startup_fixtures.rs"]
mod output_startup_fixtures;

#[cfg(test)]
#[path = "native_live_class_fixtures.rs"]
mod native_live_class_fixtures;

#[cfg(test)]
#[path = "practice_control_fixtures.rs"]
mod practice_control_fixtures;
