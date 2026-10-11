//! Named public publication work. Count requests, never elapsed-time performance.
use beatkernel::{
    chart::ObjectId,
    input::GameControlId,
    judge::{
        HazardEvent, HazardId, HazardOutcome, JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage,
    },
    runtime::RuntimeReport,
    time::{ClockDomainId, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms_runtime::{
    competition::ScoreSummary,
    gauge::{BmsGauge, GaugeProfile, GradeDelta},
    local_players::PlayerId,
    local_runtime::PlayerReport,
    mine_damage::MineDamageSummary,
    player::{self, PauseState, PlayerSnapshot, PlayerStatus},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Requests {
    count: usize,
    bytes: usize,
    largest: usize,
}
thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static REQUESTS: Cell<Requests> = const { Cell::new(Requests { count: 0, bytes: 0, largest: 0 }) };
}
fn record(bytes: usize) {
    let _ = ACTIVE.try_with(|active| {
        if active.get() {
            let _ = REQUESTS.try_with(|requests| {
                let old = requests.get();
                requests.set(Requests {
                    count: old.count.saturating_add(1),
                    bytes: old.bytes.saturating_add(bytes),
                    largest: old.largest.max(bytes),
                });
            });
        }
    });
}
struct Allocator;
// SAFETY: All pointers/layouts are forwarded unchanged to System. Scalar TLS
// hooks never allocate, unwind, inspect or retain allocation data.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller supplies a valid allocation layout.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record(layout.size());
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller supplies a valid allocation layout.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record(layout.size());
        }
        pointer
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: The caller supplies a live System allocation and valid size.
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            record(size);
        }
        result
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: The caller supplies the original allocation and layout.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
struct Measurement;
impl Drop for Measurement {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(false));
    }
}
fn measure<T>(operation: impl FnOnce() -> T) -> (T, Requests) {
    ACTIVE.with(|active| assert!(!active.get(), "nested measurement"));
    REQUESTS.with(|requests| requests.set(Requests::default()));
    ACTIVE.with(|active| active.set(true));
    let guard = Measurement;
    let result = operation();
    drop(guard);
    (result, REQUESTS.with(Cell::get))
}
struct TempDirectory(std::path::PathBuf);
impl TempDirectory {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..1000 {
            let path = std::env::temp_dir().join(format!(
                "beatkernel-player-publication-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("temporary BMS directory: {error}"),
            }
        }
        panic!("temporary BMS directory collision budget exhausted")
    }
}
impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn profile(custom: bool) -> GaugeProfile {
    if !custom {
        return GaugeProfile::default();
    }
    GaugeProfile::new(
        20_000_000,
        80_000_000,
        17,
        -1_000_000,
        true,
        (0..64)
            .rev()
            .map(|index| GradeDelta {
                grade: JudgeGrade(index + 1),
                delta: index as i64 + 1,
            })
            .collect(),
    )
    .unwrap()
}
fn report(count: usize, offset: u64, hazard_only: bool) -> RuntimeReport {
    let song_time = Timestamp::from_nanos(offset as i64);
    RuntimeReport {
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
        judge_events: (0..count)
            .map(|index| JudgeEvent {
                object: ObjectId(offset + index as u64),
                stage: JudgeStage::Instant,
                outcome: JudgeOutcome::Hit {
                    grade: JudgeGrade(1),
                    delta: Duration::ZERO,
                },
                at: song_time,
                input: None,
            })
            .collect(),
        hazard_events: if hazard_only {
            vec![HazardEvent {
                id: HazardId(offset),
                at: song_time,
                control: GameControlId(0x11),
                value: 1,
                outcome: HazardOutcome::Triggered,
                input: None,
            }]
        } else {
            Vec::new()
        },
        judge_error: None,
        audio_commands: Vec::new(),
        audio_failures: Vec::new(),
    }
}
fn force(viewer: &player::PlayerViewer, pause: PauseState) -> (PlayerSnapshot, Requests) {
    let (_, stats) = measure(|| player::publish_pause(pause));
    (
        viewer
            .take_latest()
            .expect("changed pause forces actual publication"),
        stats,
    )
}
fn assert_member(
    snapshot: &PlayerSnapshot,
    index: usize,
    policy: &GaugeProfile,
    reports: &[&RuntimeReport],
) {
    let mut score = ScoreSummary::default();
    let mut gauge = BmsGauge::new(policy.clone());
    let mut mines = MineDamageSummary::default();
    let mut events = Vec::new();
    for report in reports {
        score.observe(&report.judge_events).unwrap();
        gauge
            .observe(&report.judge_events, &report.hazard_events)
            .unwrap();
        mines.observe(&report.hazard_events).unwrap();
        events.extend_from_slice(&report.judge_events);
    }
    let history = &events[events.len().saturating_sub(128)..];
    let member = &snapshot.players[index];
    assert_eq!(member.score, score);
    assert_eq!(member.gauge, gauge);
    assert_eq!(member.mine_damage, mines);
    assert_eq!(
        member.song_time,
        reports.last().map(|report| report.song_time)
    );
    assert_eq!(member.last_judge, events.last().copied());
    assert_eq!(member.recent_results, history);
    assert_eq!(member.pressed_lanes, 0);
    if snapshot.players.len() == 1 {
        assert_eq!(snapshot.score, score);
        assert_eq!(snapshot.gauge, gauge);
        assert_eq!(snapshot.recent_results, history);
    } else {
        assert_eq!(snapshot.score, ScoreSummary::default());
        assert_eq!(snapshot.gauge, BmsGauge::default());
        assert!(snapshot.recent_results.is_empty());
    }
}

#[test]
fn cold_warm_report_and_actual_forced_publication_requests() {
    let source = beatkernel_bms::parse(
        "#TITLE Allocation\n#BPM 120\n#WAV01 tap.wav\n#00011:01\n",
        Default::default(),
    )
    .unwrap();
    let chart = source.compile().unwrap().chart;
    for members in [1, 2] {
        for custom in [false, true] {
            let policy = profile(custom);
            for count in [0, 1, 128, 129] {
                for hazard_only in [false, true] {
                    if hazard_only && count != 0 {
                        continue;
                    }
                    for warm in [false, true] {
                        let seed = report(128, 1, false);
                        let work = report(count, 1000, hazard_only);
                        let rows: Vec<_> = (1..=members)
                            .map(|id| PlayerReport {
                                player: PlayerId(id),
                                report: work.clone(),
                            })
                            .collect();
                        let seeds: Vec<_> = (1..=members)
                            .map(|id| PlayerReport {
                                player: PlayerId(id),
                                report: seed.clone(),
                            })
                            .collect();
                        let ids: Vec<_> = (1..=members).map(PlayerId).collect();
                        let policies: Vec<_> = ids.iter().map(|id| (*id, &policy)).collect();
                        let (publisher, viewer) = player::channel();
                        viewer.take_latest();
                        player::with_publisher(publisher, || {
                            player::publish_local_chart(&source, &chart, &ids).unwrap();
                            player::prepare_native_policies(&policies).unwrap();
                            viewer.take_latest();
                            if warm {
                                if members == 1 { player::publish_report(&seed).unwrap(); }
                                else { player::publish_local_reports(&seeds).unwrap(); }
                                force(&viewer, PauseState::Running);
                            }
                            viewer.take_latest();
                            let (result, report_stats) = measure(|| {
                                if members == 1 { player::publish_report(&work) }
                                else { player::publish_local_reports(&rows) }
                            });
                            result.unwrap();
                            // Host scheduling may cross the existing 8ms boundary. Keep
                            // actual delivered work distinct from throttled staging.
                            let automatic = viewer.take_latest();
                            let delivery = if automatic.is_some() { "published" } else { "staged" };
                            let (snapshot, forced_stats) = force(&viewer, PauseState::Paused);
                            let prefix = if warm { vec![&seed, &work] } else { vec![&work] };
                            for index in 0..members as usize { assert_member(&snapshot, index, &policy, &prefix); }
                            if let Some(automatic) = automatic {
                                for index in 0..members as usize { assert_member(&automatic, index, &policy, &prefix); }
                            }
                            assert_eq!(snapshot.status, PlayerStatus::Playing);
                            assert!(forced_stats.count > 0, "owned outgoing roster needs storage");
                            let mut owned = snapshot.clone();
                            if !owned.players[0].recent_results.is_empty() { owned.players[0].recent_results[0].object = ObjectId(u64::MAX); }
                            owned.players[0].recent_results.clear();
                            assert_member(&snapshot, 0, &policy, &prefix);
                            let (_, unchanged) = measure(|| player::publish_pause(PauseState::Paused));
                            assert_eq!(unchanged, Requests::default());
                            assert!(viewer.take_latest().is_none());
                            println!("player members={members} custom={custom} judgments={count} hazard_only={hazard_only} warm={warm} report={delivery}:{report_stats:?} forced={forced_stats:?} score_hits={} history={} gauge={:?} mines={:?}", snapshot.players[0].score.hits, snapshot.players[0].recent_results.len(), snapshot.players[0].gauge.snapshot(), snapshot.players[0].mine_damage);
                            Ok(())
                        }).unwrap();
                        let terminal = viewer.take_latest().unwrap();
                        assert_eq!(terminal.status, PlayerStatus::Finished);
                        let prefix = if warm {
                            vec![&seed, &work]
                        } else {
                            vec![&work]
                        };
                        for index in 0..members as usize {
                            assert_member(&terminal, index, &policy, &prefix);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn unwinding_measurement_disables_the_meter() {
    assert!(std::panic::catch_unwind(|| measure(|| panic!("meter fixture"))).is_err());
    assert!(!ACTIVE.with(Cell::get));
    assert_eq!(measure(|| ()).1, Requests::default());
}

#[test]
fn registered_custom_replay_prefix_and_forced_owned_delivery_requests() {
    use beatkernel::{
        input::CodecLimits,
        judge::{JudgeProfile, JudgeWindow},
        replay::codec::ReplayCodecLimits,
    };
    use beatkernel_bms_runtime::{replay_capture::LiveReplayCapture, replay_visual::ReplayVisual};
    let directory = TempDirectory::new();
    let chart_path = directory.0.join("allocation.bms");
    let chart_text = "#BPM 120\n#WAV01 tap.wav\n#00011:01\n";
    std::fs::write(&chart_path, chart_text).unwrap();
    let source = beatkernel_bms::parse(chart_text, Default::default()).unwrap();
    let chart = source.compile().unwrap().chart;
    let input_mode = beatkernel_bms::BmsInputMode::ButtonOrContact;
    let judge = beatkernel_bms_runtime::mine_plan::prepare_judge(
        &source,
        chart.clone(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
        input_mode,
        beatkernel_bms::ParseOptions::default().max_objects,
    )
    .unwrap();
    let limits =
        ReplayCodecLimits::new(65536, 4096, 16384, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    let policy = profile(true);
    let capture = LiveReplayCapture::new_with_gauge(
        &judge,
        ClockDomainId(17),
        limits,
        Timestamp::ZERO,
        0,
        None,
        input_mode,
        None,
        &policy,
    )
    .unwrap();
    let visual = ReplayVisual::new_section(&source, &capture.into_file(), limits).unwrap();
    for count in [0, 1, 128, 129] {
        for warm in [false, true] {
            let seed = report(128, 1, false);
            let work = report(count, 1000, false);
            let mut gauge = BmsGauge::new(policy.clone());
            let (publisher, viewer) = player::channel();
            viewer.take_latest();
            player::with_publisher(publisher, || {
                player::publish_native_replay_chart(&chart_path, &source, &chart, &visual).unwrap();
                viewer.take_latest();
                if warm {
                    gauge.observe(&seed.judge_events, &[]).unwrap();
                    player::publish_replay_prefix_with_gauge(seed.song_time, &seed.judge_events, 0, MineDamageSummary::default(), &gauge).unwrap();
                    force(&viewer, PauseState::Running);
                }
                gauge.observe(&work.judge_events, &[]).unwrap();
                let (result, stats) = measure(|| player::publish_replay_prefix_with_gauge(work.song_time, &work.judge_events, 0, MineDamageSummary::default(), &gauge));
                result.unwrap();
                let automatic = viewer.take_latest();
                let delivery = if automatic.is_some() { "published" } else { "staged" };
                let (snapshot, forced) = force(&viewer, PauseState::Paused);
                let prefix = if warm { vec![&seed, &work] } else { vec![&work] };
                assert_member(&snapshot, 0, &policy, &prefix);
                if let Some(automatic) = automatic { assert_member(&automatic, 0, &policy, &prefix); }
                println!("player custom_replay judgments={count} warm={warm} report={delivery}:{stats:?} forced={forced:?} score_hits={} history={} gauge={:?}", snapshot.score.hits, snapshot.recent_results.len(), snapshot.gauge.snapshot());
                Ok(())
            }).unwrap();
            assert_eq!(viewer.take_latest().unwrap().status, PlayerStatus::Finished);
        }
    }
}
