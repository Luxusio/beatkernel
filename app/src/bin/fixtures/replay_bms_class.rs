//! Inspect real canonical live captures through the CLI's actual execution path.
use super::*;
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeGrade, JudgeWindow},
    replay::codec::encode_replay,
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsGaugeKind, BmsInputMode, BmsJudgment};
use beatkernel_bms_runtime::{
    play_policy::{ClassifiedWindow, ResolvedPlayPolicy},
    replay_capture::LiveReplayCapture,
};
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

const CHART: &str = "#BPM 60\n#TOTAL 320\n#WAV01 x.wav\n#00011:0101\n";
const END: i64 = 2_000_000_010;
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(ns),
    }
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (point.domain == target).then_some(point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
struct Fixture {
    dir: PathBuf,
    chart: PathBuf,
    replay: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
impl Fixture {
    fn new(finite: bool, great_only: bool, classified: bool, contact: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "beatkernel-replay-classes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let chart = dir.join("chart.bms");
        let replay = dir.join("capture.bkr");
        fs::write(&chart, CHART).unwrap();
        let source = load_chart_with_seed(&chart, 0).unwrap();
        let windows = [
            ClassifiedWindow {
                judgment: BmsJudgment::PGreat,
                window: JudgeWindow {
                    grade: JudgeGrade(91),
                    early: Duration::ZERO,
                    late: Duration::ZERO,
                },
            },
            ClassifiedWindow {
                judgment: BmsJudgment::Great,
                window: JudgeWindow {
                    grade: JudgeGrade(7),
                    early: Duration::from_nanos(2),
                    late: Duration::from_nanos(2),
                },
            },
        ];
        let policy = ResolvedPlayPolicy::bms(&source, BmsGaugeKind::Hard, &windows, 0).unwrap();
        let input_mode = if contact {
            BmsInputMode::ButtonOrContact
        } else {
            BmsInputMode::ButtonOnly
        };
        let judge = beatkernel_bms_runtime::mine_plan::prepare_judge(
            &source,
            source.compile().unwrap().chart,
            policy.judge().clone(),
            input_mode,
            1024,
        )
        .unwrap();
        let limits =
            ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap())
                .unwrap();
        let end = finite.then_some(Timestamp::from_nanos(END));
        let mut capture = if classified {
            LiveReplayCapture::new_with_policy(
                &judge,
                ClockDomainId(17),
                limits,
                Timestamp::ZERO,
                0,
                end,
                input_mode,
                None,
                &policy,
            )
            .unwrap()
        } else if !finite && !contact {
            LiveReplayCapture::new(&judge, ClockDomainId(17), limits).unwrap()
        } else {
            LiveReplayCapture::new_with_gauge(
                &judge,
                ClockDomainId(17),
                limits,
                Timestamp::ZERO,
                0,
                end,
                input_mode,
                None,
                policy.gauge(),
            )
            .unwrap()
        };
        let bindings = BindingMap::from_bindings([Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(7u16),
            game_control: GameControlId(0x11),
        }])
        .unwrap();
        let (producer, _consumer) = command_queue(8).unwrap();
        let mut runtime = Runtime::new(
            ClockDomainId(17),
            ClockDomainId(17),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            bindings,
            judge,
            producer,
            vec![],
            0,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        let times: &[i64] = if great_only {
            &[1]
        } else {
            &[0, 2_000_000_000]
        };
        for (index, &ns) in times.iter().enumerate() {
            for (offset, state) in [ButtonState::Down, ButtonState::Up].into_iter().enumerate() {
                let report = runtime
                    .process_input(
                        PhysicalInputEvent::Button(ButtonEvent {
                            meta: EventMeta::new(
                                DeviceId(9),
                                point(ns),
                                (index * 2 + offset) as u64,
                            ),
                            control: PhysicalControlId::keyboard(7u16),
                            state,
                        }),
                        &Identity,
                        point(ns),
                    )
                    .unwrap();
                assert!(report.judge_error.is_none());
                capture.record_report(&report).unwrap();
            }
        }
        let report = runtime
            .advance_to(point(END), &Identity, point(END))
            .unwrap();
        capture.record_report(&report).unwrap();
        fs::write(&replay, capture.into_bytes().unwrap()).unwrap();
        Self { dir, chart, replay }
    }
    fn args(&self, extra: &[&str]) -> Vec<String> {
        let mut args = vec![
            "--chart".into(),
            self.chart.to_string_lossy().into_owned(),
            "--replay".into(),
            self.replay.to_string_lossy().into_owned(),
        ];
        args.extend(extra.iter().map(|s| (*s).to_owned()));
        args
    }
    fn inspect(&self, extra: &[&str]) -> Inspection {
        inspect(parse(&self.args(extra)).unwrap()).unwrap()
    }
}
#[test]
fn classified_actual_capture_scores_and_zero_cursor_are_inspected() {
    let export_dir = std::env::var_os("BEATKERNEL_RECORDED_SCORE_QA_DIR").map(PathBuf::from);
    if let Some(dir) = &export_dir {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("chart.bms"), CHART).unwrap();
    }
    for great_only in [false, true] {
        let fixture = Fixture::new(false, great_only, true, false);
        let full = fixture.inspect(&[]);
        let score = full.bms_score.unwrap();
        assert_eq!(
            (score.pgreat, score.great, score.poor, score.ex_score),
            if great_only {
                (0, 1, 1, 1)
            } else {
                (2, 0, 0, 4)
            }
        );
        assert_eq!(full.score.hits, if great_only { 1 } else { 2 });
        let prefix = fixture.inspect(&["--cursor", "0"]);
        assert_eq!(prefix.session.cursor(), 0);
        assert!(prefix.session.results().is_empty());
        assert_eq!(prefix.bms_score.unwrap(), Default::default());
        run_args(&fixture.args(&[])).unwrap();
        if let Some(dir) = &export_dir {
            fs::copy(
                &fixture.replay,
                dir.join(if great_only {
                    "classified-great.bkr"
                } else {
                    "classified-pgreat.bkr"
                }),
            )
            .unwrap();
        }
    }
    if let Some(dir) = &export_dir {
        for (name, finite, classified, contact) in [
            ("finite-contact.bkr", true, true, true),
            ("legacy-unclassified.bkr", false, false, false),
        ] {
            let fixture = Fixture::new(finite, false, classified, contact);
            fixture.inspect(&[]);
            fs::copy(&fixture.replay, dir.join(name)).unwrap();
        }
    }
}
#[test]
fn finite_contact_capture_accepts_endpoint_and_refuses_later_time_before_chart_read() {
    let fixture = Fixture::new(true, false, true, true);
    let inspected = fixture.inspect(&["--song-ns", "2000000010"]);
    assert_eq!(inspected.bms_score.unwrap().pgreat, 2);
    fs::remove_file(&fixture.chart).unwrap();
    let err = inspect(parse(&fixture.args(&["--song-ns", "2000000011"])).unwrap())
        .err()
        .unwrap();
    assert!(err.to_string().contains("finite endpoint"), "{err}");
    assert!(run_args(&fixture.args(&["--song-ns", "2000000011"])).is_err());
}
#[test]
fn mismatched_chart_and_malformed_recorded_classes_refuse() {
    let fixture = Fixture::new(false, false, true, false);
    fs::write(&fixture.chart, "#BPM 60\n#WAV01 x.wav\n#00012:0101\n").unwrap();
    assert!(inspect(parse(&fixture.args(&[])).unwrap()).is_err());
    fs::write(&fixture.chart, CHART).unwrap();
    let limits =
        ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    let mut file = read_replay(&mut File::open(&fixture.replay).unwrap(), limits).unwrap();
    *file.header.options.last_mut().unwrap() = 255;
    fs::write(&fixture.replay, encode_replay(&file, limits).unwrap()).unwrap();
    assert!(inspect(parse(&fixture.args(&[])).unwrap()).is_err());
    assert!(run_args(&fixture.args(&[])).is_err());
}
#[test]
fn legacy_unclassified_record_preserves_counts_without_guessing_classes() {
    let fixture = Fixture::new(false, false, false, false);
    let inspected = fixture.inspect(&[]);
    assert_eq!(inspected.score.hits, 2);
    assert_eq!(inspected.bms_score, None);
    run_args(&fixture.args(&[])).unwrap();
}
