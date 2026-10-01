//! Latest-state presentation bridge; never called from the audio callback.
use crate::{
    competition::ScoreSummary, local_players::PlayerId, local_runtime::PlayerReport,
    player_chart::PlayerChart,
};
use beatkernel::{
    chart::CompiledChart, judge::JudgeEvent, runtime::RuntimeReport, time::Timestamp,
};
use beatkernel_bms::BmsChart;
use std::{
    cell::RefCell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
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

/// One stable local member's actual reports; never an aggregate cohort score.
#[derive(Clone)]
pub struct LocalPlayerSnapshot {
    pub player: PlayerId,
    pub chart: Option<Arc<PlayerChart>>,
    pub song_time: Option<Timestamp>,
    pub score: ScoreSummary,
    pub last_judge: Option<JudgeEvent>,
    pub recent_results: Vec<JudgeEvent>,
}
impl LocalPlayerSnapshot {
    fn new(player: PlayerId, chart: Option<Arc<PlayerChart>>) -> Self {
        Self {
            player,
            chart,
            song_time: None,
            score: ScoreSummary::default(),
            last_judge: None,
            recent_results: Vec::new(),
        }
    }
    fn update_report(&mut self, report: &RuntimeReport) {
        self.song_time = Some(report.song_time);
        if let Some(last) = report.judge_events.last() {
            self.last_judge = Some(*last);
        }
        if report.judge_events.len() >= 128 {
            self.recent_results.clear();
        }
        for event in report
            .judge_events
            .iter()
            .skip(report.judge_events.len().saturating_sub(128))
        {
            if self.recent_results.len() == 128 {
                self.recent_results.remove(0);
            }
            self.recent_results.push(*event);
        }
    }
}

/// Immutable UI copy of actual chart/time/results; no inferred clock relation.
#[derive(Clone)]
pub struct PlayerSnapshot {
    /// All registered local members, bounded to 64 with 128 recent results each.
    pub players: Vec<LocalPlayerSnapshot>,
    pub chart: Option<Arc<PlayerChart>>,
    pub song_time: Option<Timestamp>,
    pub score: ScoreSummary,
    pub last_judge: Option<JudgeEvent>,
    pub recent_results: Vec<JudgeEvent>,
    pub status: PlayerStatus,
    pub cancelled: bool,
}
impl Default for PlayerSnapshot {
    fn default() -> Self {
        Self {
            players: Vec::new(),
            chart: None,
            song_time: None,
            score: ScoreSummary::default(),
            last_judge: None,
            recent_results: Vec::new(),
            status: PlayerStatus::Loading,
            cancelled: false,
        }
    }
}
struct Shared {
    latest: Mutex<Option<PlayerSnapshot>>,
    cancel: AtomicBool,
}
/// Sendable attachment token; native input/window objects never cross threads.
#[derive(Clone)]
pub struct PlayerPublisher(Arc<Shared>);
/// Main-thread latest-state consumer and explicit cancellation source.
pub struct PlayerViewer(Arc<Shared>);

/// Make one bounded latest-state slot; fresh channels isolate restarted sessions.
pub fn channel() -> (PlayerPublisher, PlayerViewer) {
    let shared = Arc::new(Shared {
        latest: Mutex::new(Some(PlayerSnapshot::default())),
        cancel: AtomicBool::new(false),
    });
    (PlayerPublisher(shared.clone()), PlayerViewer(shared))
}
impl PlayerViewer {
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
                }
            }
        }
    }
}
struct Session {
    publisher: PlayerPublisher,
    snapshot: PlayerSnapshot,
    last_publish: Option<Instant>,
    chart_published: bool,
}
thread_local! { static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) }; }

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
            publisher,
            snapshot: PlayerSnapshot::default(),
            last_publish: None,
            chart_published: false,
        });
        Ok::<(), String>(())
    })?;
    // Unwind drops the thread-local session with its game owner; the UI observes
    // JoinHandle's panic separately. No unwind crosses a native callback ABI.
    let result = run();
    SESSION.with(|session| {
        if let Some(mut current) = session.borrow_mut().take() {
            current.snapshot.cancelled = current.publisher.0.cancel.load(Ordering::Acquire);
            current.snapshot.status = match &result {
                Ok(_) => PlayerStatus::Finished,
                Err(error) => PlayerStatus::Failed(error.clone()),
            };
            current.snapshot.sync_legacy();
            // Gameplay and audio owners have already stopped. This final tiny
            // handoff can wait for take_latest, which releases before rendering.
            if let Ok(mut slot) = current.publisher.0.latest.lock() {
                *slot = Some(current.snapshot);
            }
        }
    });
    result
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
    validate_players(players)?;
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
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
        }
        current.snapshot.players = members;
        current.snapshot.chart = Some(prepared);
        current.chart_published = true;
        current.observe_cancellation(false);
        current.publish_latest(true);
        Ok(())
    })
}

/// Summarize one actual solo report once, preserving the existing call shape.
/// Coalescing affects display only. This API cannot modify a local group roster.
pub fn publish_report(report: &RuntimeReport) -> Result<(), Box<dyn std::error::Error>> {
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
        if current.snapshot.players.is_empty() {
            let mut member = LocalPlayerSnapshot::new(PlayerId(1), current.snapshot.chart.clone());
            if !report.judge_events.is_empty() {
                member.score.observe(&report.judge_events)?;
            }
            member.update_report(report);
            current.snapshot.players.push(member);
        } else {
            let member = &mut current.snapshot.players[0];
            if !report.judge_events.is_empty() {
                member.score.observe(&report.judge_events)?;
            }
            member.update_report(report);
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
        // history/Arc/score maps or allocate scratch. All score errors precede
        // any mutation of any member in this batch.
        let mut changed_scores = Vec::new();
        for report in reports {
            if report.report.judge_events.is_empty() {
                continue;
            }
            let index = current
                .snapshot
                .players
                .iter()
                .position(|member| member.player == report.player)
                .expect("validated registered player");
            let mut score = current.snapshot.players[index].score.clone();
            score.observe(&report.report.judge_events)?;
            changed_scores.push((index, score));
        }
        for (index, score) in changed_scores {
            current.snapshot.players[index].score = score;
        }
        for report in reports {
            let member = current
                .snapshot
                .players
                .iter_mut()
                .find(|member| member.player == report.player)
                .expect("validated registered player");
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
    // Populate legacy fields only for actual handoff, avoiding report-frequency
    // cloning of bounded history/grade maps while retaining solo UI call shapes.
    fn sync_legacy(&mut self) {
        if self.players.len() == 1 {
            let member = &self.players[0];
            self.song_time = member.song_time;
            self.score = member.score.clone();
            self.last_judge = member.last_judge;
            self.recent_results = member.recent_results.clone();
        } else {
            self.song_time = None;
            self.score = ScoreSummary::default();
            self.last_judge = None;
            self.recent_results.clear();
        }
    }
}
impl Session {
    fn observe_cancellation(&mut self, playing: bool) {
        if self.publisher.0.cancel.load(Ordering::Acquire) {
            self.snapshot.cancelled = true;
            self.snapshot.status = PlayerStatus::Stopping;
        } else if playing {
            self.snapshot.status = PlayerStatus::Playing;
        }
    }
    fn publish_latest(&mut self, force: bool) {
        if force
            || self
                .last_publish
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(8))
        {
            if let Ok(mut slot) = self.publisher.0.latest.try_lock() {
                self.snapshot.sync_legacy();
                *slot = Some(self.snapshot.clone());
                self.last_publish = Some(Instant::now());
            }
        }
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
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
                audio_at: ClockPoint {
                    domain: ClockDomainId(2),
                    timestamp: song_time,
                },
                input_mapping_quality: ClockMappingQuality::Unknown,
                audio_mapping_quality: ClockMappingQuality::Unknown,
                judge_events,
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
            assert!(
                initial
                    .players
                    .iter()
                    .all(|member| Arc::ptr_eq(member.chart.as_ref().unwrap(), common))
            );
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
}
