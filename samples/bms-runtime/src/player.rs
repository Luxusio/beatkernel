//! Latest-state presentation bridge; never called from the audio callback.
use crate::{competition::ScoreSummary, player_chart::PlayerChart};
use beatkernel::{
    chart::CompiledChart, judge::JudgeEvent, runtime::RuntimeReport, time::Timestamp,
};
use beatkernel_bms::BmsChart;
use std::{
    cell::RefCell,
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

/// Immutable UI copy of actual chart/time/results; no inferred clock relation.
#[derive(Clone)]
pub struct PlayerSnapshot {
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

/// Publish the exact chart prepared by this native session before audio starts.
pub fn publish_chart(
    source: &BmsChart,
    chart: &CompiledChart,
) -> Result<(), Box<dyn std::error::Error>> {
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
        current.snapshot.chart = Some(Arc::new(PlayerChart::from_compiled(source, chart)?));
        if let Ok(mut slot) = current.publisher.0.latest.try_lock() {
            *slot = Some(current.snapshot.clone());
        }
        Ok(())
    })
}

/// Summarize actual reports once. Coalescing affects display only, never judging.
pub fn publish_report(report: &RuntimeReport) -> Result<(), Box<dyn std::error::Error>> {
    SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(current) = session.as_mut() else {
            return Ok(());
        };
        current.snapshot.score.observe(&report.judge_events)?;
        current.snapshot.song_time = Some(report.song_time);
        current.snapshot.status = if current.publisher.0.cancel.load(Ordering::Acquire) {
            PlayerStatus::Stopping
        } else {
            PlayerStatus::Playing
        };
        for event in &report.judge_events {
            current.snapshot.last_judge = Some(*event);
            if current.snapshot.recent_results.len() == 128 {
                current.snapshot.recent_results.remove(0);
            }
            current.snapshot.recent_results.push(*event);
        }
        if current
            .last_publish
            .is_none_or(|last| last.elapsed() >= Duration::from_millis(8))
        {
            if let Ok(mut slot) = current.publisher.0.latest.try_lock() {
                *slot = Some(current.snapshot.clone());
                current.last_publish = Some(Instant::now());
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod fixtures {
    use super::*;
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
