//! Records routing, real metadata acquisition, and injected playback effects.
use super::*;
use beatkernel_bms_runtime::{
    competition::ScoreSummary,
    gauge::BmsGauge,
    local_players::PlayerId,
    play_result::{CompletedPlayResult, PlayResultOutcome, PlayResultScope},
    result_archive::{ArchivedResult, ArchivedScore, ResultArchive},
};
fn historical(path: PathBuf) -> RecordPreview {
    let result = ArchivedResult {
        scope: PlayResultScope::FullSong,
        outcome: PlayResultOutcome::BelowClearThreshold,
        gauge: *BmsGauge::default().snapshot(),
    };
    let score = ScoreSummary {
        hits: 2,
        combo: 2,
        max_combo: 2,
        grades: [(u32::MAX, 2)].into_iter().collect(),
        ..Default::default()
    };
    RecordPreview {
        path,
        records: 0,
        recorded_until: None,
        start: beatkernel::time::Timestamp::ZERO,
        end: None,
        historical: Some((PlayerId(u32::MAX), result)),
        historical_score: Some(Arc::new(ArchivedScore::from_summary(&score).unwrap())),
        bms_score: None,
        historical_bms_score: None,
        historical_comparison: None,
        archive_error: None,
        score: Default::default(),
    }
}
fn prepared() -> Desktop {
    let mut app = super::tests::lifecycle_fixture();
    app.open_settings();
    app.open_records();
    let records = app.records.as_mut().unwrap();
    records.catalog = Some(RecordCatalog {
        entries: (0..25)
            .map(|index| PathBuf::from(format!("record{index}.bkr")))
            .collect(),
        truncated: false,
    });
    records.select(11);
    records.preview = Some(historical(PathBuf::from("record11.bkr")));
    app.sync_ime();
    app
}
#[test]
fn detail_button_or_nonrepeat_d_opens_under_same_parent_and_first_back_only_closes_subview() {
    for back in 0..3 {
        let mut app = prepared();
        let parent = app.navigator.back_target().unwrap();
        let child = app.navigator.active_id();
        if back == 0 {
            app.records.as_mut().unwrap().directory_focused = true;
            app.sync_ime();
            app.ime_event(Ime::Enabled);
            app.ime_event(Ime::Preedit("音".into(), None));
            assert!(app.ime.preview.is_some());
            app.activate(ControlId(66));
            assert!(app.ime.preview.is_none());
        } else {
            app.records_key(KeyCode::KeyD, false);
        }
        assert!(app.records.as_ref().unwrap().details);
        assert!(!app.records.as_ref().unwrap().directory_focused);
        assert!(app.text_target().is_none());
        assert_eq!(app.navigator.active_id(), child);
        app.draw().unwrap();
        assert_eq!(
            app.hits.iter().map(|hit| hit.0).collect::<Vec<_>>(),
            [ControlId(66)]
        );
        match back {
            0 => app.activate(ControlId(66)),
            1 => app.records_key(KeyCode::Escape, false),
            _ => app.back(),
        }
        let records = app.records.as_ref().unwrap();
        assert!(!records.details);
        assert_eq!(records.selected, Some(11));
        assert_eq!(records.first, 10);
        assert_eq!(app.navigator.active_id(), child);
        assert!(records.valid_preview().is_some());
        app.back();
        assert_eq!(app.navigator.route(), parent);
        assert!(app.records.is_none());
    }
}
#[test]
fn open_detail_mode_ignores_catalog_watch_opponent_paging_text_and_ime_actions() {
    let mut app = prepared();
    app.activate(ControlId(66));
    app.draw().unwrap();
    let directory = app.records.as_ref().unwrap().directory.clone();
    let selected = app.records.as_ref().unwrap().selected;
    let score = app
        .records
        .as_ref()
        .unwrap()
        .preview
        .as_ref()
        .unwrap()
        .historical_score
        .as_ref()
        .unwrap()
        .clone();
    let opponents = saved_opponents(&app.settings.as_ref().unwrap().values);
    for id in [50, 51, 52, 53, 54, 56, 57, 58, 59, 60, 61, 50012] {
        app.activate(ControlId(id));
    }
    for key in [
        KeyCode::KeyW,
        KeyCode::Enter,
        KeyCode::Tab,
        KeyCode::PageDown,
        KeyCode::ArrowDown,
        KeyCode::Backspace,
    ] {
        app.records_key(key, false);
    }
    app.keyboard_input(
        PhysicalKey::Code(KeyCode::KeyX),
        &Key::Character("new".into()),
        Some("new"),
        false,
    );
    app.ime_event(Ime::Preedit("音".into(), None));
    app.ime_event(Ime::Commit("音".into()));
    let records = app.records.as_ref().unwrap();
    assert!(records.details);
    assert_eq!(records.directory, directory);
    assert_eq!(records.selected, selected);
    assert_eq!(records.first, 10);
    assert!(Arc::ptr_eq(
        records
            .preview
            .as_ref()
            .unwrap()
            .historical_score
            .as_ref()
            .unwrap(),
        &score
    ));
    assert_eq!(
        saved_opponents(&app.settings.as_ref().unwrap().values),
        opponents
    );
    assert!(app.profile_io.is_none());
    assert!(app.game.is_none());
    assert!(app.text_target().is_none());
}
#[test]
fn repeated_d_directory_focus_stale_missing_and_pending_metadata_cannot_open_details() {
    for refusal in 0..5 {
        let mut app = prepared();
        match refusal {
            0 => {
                app.records_key(KeyCode::KeyD, true);
            }
            1 => {
                app.records.as_mut().unwrap().directory_focused = true;
                app.records_key(KeyCode::KeyD, false);
            }
            2 => {
                app.records.as_mut().unwrap().preview.as_mut().unwrap().path =
                    PathBuf::from("foreign.bkr");
                app.activate(ControlId(66));
            }
            3 => {
                app.records
                    .as_mut()
                    .unwrap()
                    .preview
                    .as_mut()
                    .unwrap()
                    .historical = None;
                app.activate(ControlId(66));
            }
            _ => {
                let (owner, permit) = app.metadata_scope().unwrap();
                app.profile_io = Some(ProfileOperation {
                    owner,
                    permit,
                    worker: None,
                });
                app.activate(ControlId(66));
                app.records_key(KeyCode::KeyD, false);
            }
        }
        assert!(!app.records.as_ref().unwrap().details);
        assert_eq!(app.navigator.route(), ScreenRoute::Records);
        assert_eq!(app.records.as_ref().unwrap().selected, Some(11));
    }
}
#[test]
fn selection_and_directory_replacement_invalidate_metadata_and_parent_disposal_releases_shared_details(
) {
    let mut app = prepared();
    app.activate(ControlId(66));
    app.draw().unwrap();
    let weak = Arc::downgrade(
        app.records
            .as_ref()
            .unwrap()
            .preview
            .as_ref()
            .unwrap()
            .historical_score
            .as_ref()
            .unwrap(),
    );
    app.back();
    app.records.as_mut().unwrap().select(12);
    assert!(!app.records.as_ref().unwrap().details);
    assert!(app.records.as_ref().unwrap().preview.is_none());
    app.draw().unwrap();
    let records = app.records.as_mut().unwrap();
    records.directory_focused = true;
    records.edit(None, Some("replacement/"));
    assert!(records.catalog.is_none());
    assert!(records.preview.is_none());
    assert!(!records.details);
    app.request_close();
    assert!(app.records.is_none());
    assert!(app.records_view.is_none());
    assert!(weak.upgrade().is_none());
}

/// Files and replay operations are genuine; playback callbacks below stop before
/// opening a device. This fixture claims metadata and launch routing only.
struct SavedRecord {
    directory: PathBuf,
    chart: PathBuf,
    record: PathBuf,
}
impl SavedRecord {
    fn new(variable: bool) -> Self {
        use beatkernel::{
            audio::command_queue,
            input::BindingMap,
            judge::JudgeEngine,
            runtime::{Runtime, RuntimeProcessingClock},
            time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
            transport::{Rate, Transport},
        };
        use beatkernel_bms_runtime::{
            play_policy::ResolvedPlayPolicy,
            replay_capture::LiveReplayCapture,
            replay_speed::ReplaySpeed,
            replay_speed_policy::{wrap_header_with_schedule, SourceRateCut, SourceRateSchedule},
        };
        use std::sync::atomic::{AtomicUsize, Ordering};

        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "beatkernel-record-history-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let saved = Self {
            chart: directory.join("曲.bms"),
            record: directory.join("variable.bkr"),
            directory,
        };
        let text = "#BPM 60\n#WAV01 note.wav\n#00011:01\n#00111:01\n";
        std::fs::write(&saved.chart, text).unwrap();
        let source = beatkernel_bms::parse_seeded(text, Default::default(), 13).unwrap();
        let policy = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
        let judge = JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
            policy.judge().clone(),
        )
        .unwrap();
        let domain = ClockDomainId(17);
        let limits = beatkernel_bms_runtime::competition_live::replay_limits().unwrap();
        let mut capture = LiveReplayCapture::new_with_policy_and_speed(
            &judge,
            domain,
            limits,
            Timestamp::ZERO,
            13,
            None,
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            &policy,
            ReplaySpeed::new(Rate::NORMAL).unwrap(),
        )
        .unwrap();
        let (producer, _consumer) = command_queue(8).unwrap();
        let mut runtime = Runtime::new(
            domain,
            domain,
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            BindingMap::from_bindings([]).unwrap(),
            judge,
            producer,
            vec![],
            0,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        struct Exact;
        impl ClockMapper for Exact {
            fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
                (point.domain == target).then_some(point.timestamp)
            }
            fn quality(&self) -> ClockMappingQuality {
                ClockMappingQuality::Exact
            }
        }
        let end = ClockPoint {
            domain,
            timestamp: Timestamp::from_nanos(1),
        };
        capture
            .record_report(&runtime.advance_to(end, &Exact, end).unwrap())
            .unwrap();
        let mut file = capture.into_file();
        if variable {
            let mut schedule = SourceRateSchedule::fixed(ReplaySpeed::new(Rate::NORMAL).unwrap());
            schedule.try_reserve_change().unwrap();
            schedule
                .try_push(SourceRateCut::new(0, 1, 2, "3/2".parse().unwrap()).unwrap())
                .unwrap();
            file.header = wrap_header_with_schedule(file.header, &schedule, limits).unwrap();
        }
        let prefix =
            RecordPreview::from_file_for_history(&saved.record, &source, file.clone()).unwrap();
        assert_eq!(prefix.records, 1);
        assert_eq!(prefix.recorded_until, Some(Timestamp::from_nanos(1)));
        let player = PlayerId(u32::MAX);
        let completed = completed_record_result();
        let archive = ResultArchive::from_completed_with_scores(
            &[(player, completed)],
            &[(player, file.header.clone(), policy.gauge().clone())],
            &[(player, &prefix.score)],
        )
        .unwrap();
        std::fs::write(
            &saved.record,
            beatkernel::replay::codec::encode_replay(&file, limits).unwrap(),
        )
        .unwrap();
        beatkernel_bms_runtime::native_result_archive::save_sidecar(&archive, &saved.record)
            .unwrap();
        saved
    }
    fn open(&self, seed: u64, speed: &str) -> Desktop {
        let mut app = super::tests::lifecycle_fixture();
        app.entries[0].path = self.chart.clone();
        app.open_settings();
        let settings = app.settings.as_mut().unwrap();
        settings.values = NativeSettings::from_args(
            &[
                "--chart-seed".into(),
                seed.to_string(),
                "--early-ns".into(),
                "0".into(),
                "--late-ns".into(),
                "0".into(),
                "--speed".into(),
                speed.into(),
                "--record-replay".into(),
                self.directory.join("new.bkr").to_str().unwrap().into(),
            ],
            settings_host(),
        )
        .unwrap();
        settings.refresh_selected().unwrap();
        app.open_records();
        app.activate(ControlId(50));
        collect_record_metadata(&mut app);
        let records = app.records.as_ref().unwrap();
        assert_eq!(records.selected_path(), Some(&self.record));
        assert!(records.preview.is_none());
        app.activate(ControlId(51));
        collect_record_metadata(&mut app);
        app
    }
}
impl Drop for SavedRecord {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}
fn completed_record_result() -> CompletedPlayResult {
    use beatkernel::{
        audio::{
            command_queue, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank,
        },
        input::BindingMap,
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    };
    use beatkernel_bms_runtime::step_gameplay::{StepGameplay, StepGameplayConfig};

    // Obtain the classification from an actual portable owner and real idle
    // Mixer blocks, preserving the library's private completion constructor.
    let source = beatkernel_bms::parse("#BPM 120\n", Default::default()).unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let prepared = beatkernel_bms_runtime::PreparedBms {
        compiled: source.compile().unwrap(),
        source,
        bank: SampleBank::new(format, PcmLimits::new(64, 256, 1).unwrap()).unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    };
    let point = |domain, ns| ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    };
    let (mut owner, bank) = StepGameplay::new_section(
        prepared,
        StepGameplayConfig {
            host_origin: point(1, 0),
            output_origin: point(2, 0),
            preroll: Duration::ZERO,
            early_ns: 0,
            late_ns: 0,
            offset_ns: 0,
            command_capacity: 8,
            bgm_pending: 4,
            bgm_lookahead: Duration::from_nanos(1_000_000_000),
            telemetry_capacity: 8,
        },
        BindingMap::from_bindings([]).unwrap(),
        Timestamp::ZERO,
        None,
    )
    .unwrap();
    struct Domains;
    impl ClockMapper for Domains {
        fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
            (point.domain == target).then_some(point.timestamp)
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Unknown
        }
    }
    assert!(owner.completed_result().is_none());
    owner.activate(point(1, 0)).unwrap();
    owner
        .advance_to(point(1, 1_000_000), &Domains, point(2, 0))
        .unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let first = mixer.render(&mut [0.0; 10]).unwrap();
    assert!(!owner
        .observe_completion(Some(first), Some(point(2, 10_000_000)))
        .unwrap());
    let second = mixer.render(&mut [0.0; 10]).unwrap();
    assert!(owner
        .observe_completion(Some(second), Some(point(2, 20_000_000)))
        .unwrap());
    *owner.completed_result().unwrap()
}
fn collect_record_metadata(app: &mut Desktop) {
    let operation = app
        .profile_io
        .as_ref()
        .expect("metadata worker was scheduled");
    assert_eq!(
        operation.worker.as_ref().unwrap().thread().name(),
        Some("bms-profile")
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.profile_io.is_some() {
        assert!(
            Instant::now() < deadline,
            "metadata worker failed to complete"
        );
        app.collect_profile();
        thread::yield_now();
    }
}

#[test]
fn actual_variable_metadata_details_and_watch_keep_selected_record_without_fixed_admission() {
    #[derive(Debug)]
    struct Effect {
        step: &'static str,
        args: Vec<String>,
        worker: thread::ThreadId,
    }
    static EFFECTS: std::sync::Mutex<Vec<Effect>> = std::sync::Mutex::new(Vec::new());
    fn validate(args: &[String]) -> Result<(), Box<dyn Error>> {
        EFFECTS.lock().unwrap().push(Effect {
            step: "validate",
            args: args.to_vec(),
            worker: thread::current().id(),
        });
        Ok(())
    }
    fn prepare(args: &[String], replay: bool) -> Result<Vec<String>, String> {
        assert!(replay);
        assert_eq!(thread::current().name(), Some("bms-game"));
        assert!(player::attached());
        EFFECTS.lock().unwrap().push(Effect {
            step: "prepare",
            args: args.to_vec(),
            worker: thread::current().id(),
        });
        Ok(args.to_vec())
    }
    fn replay(args: &[String]) -> Result<(), Box<dyn Error>> {
        assert_eq!(thread::current().name(), Some("bms-game"));
        assert!(player::attached());
        assert_eq!(player::native_launch(args)?.args(), args);
        EFFECTS.lock().unwrap().push(Effect {
            step: "replay",
            args: args.to_vec(),
            worker: thread::current().id(),
        });
        Ok(())
    }

    EFFECTS.lock().unwrap().clear();
    let saved = SavedRecord::new(true);
    let original_bytes = std::fs::read(&saved.record).unwrap();
    let mut app = saved.open(13, "5/4");
    app.validate_replay = validate;
    app.prepare_native = Some(prepare);
    app.replay = replay;
    let preview = app.records.as_ref().unwrap().valid_preview().unwrap();
    assert_eq!(preview.path, saved.record);
    assert_eq!(preview.records, 1);
    assert_eq!(preview.historical.as_ref().unwrap().0, PlayerId(u32::MAX));
    assert!(preview.historical_score.is_some());
    assert!(preview.archive_error.is_none());
    let draft = app.settings.as_ref().unwrap().values.native_args();
    for control in [52, 53] {
        app.activate(ControlId(control));
        assert_eq!(app.settings.as_ref().unwrap().values.native_args(), draft);
        assert_eq!(saved_opponents(&app.settings.as_ref().unwrap().values), 0);
        assert!(app
            .records
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("variable"));
        assert!(app.records.as_ref().unwrap().valid_preview().is_some());
    }
    app.activate(ControlId(66));
    assert!(app.records.as_ref().unwrap().details);
    app.draw().unwrap();
    app.back();
    assert!(!app.records.as_ref().unwrap().details);
    assert_eq!(
        app.records.as_ref().unwrap().selected_path(),
        Some(&saved.record)
    );
    app.activate(ControlId(59));
    assert!(
        app.profile_io.is_none(),
        "WATCH syntax preflight performs no metadata IO"
    );
    assert_eq!(app.navigator.route(), ScreenRoute::Play { replay: true });
    let game = app
        .game
        .as_ref()
        .expect("WATCH selected the actual replay owner");
    assert!(game.replay);
    let launch = game.launch.args().to_vec();
    for (flag, expected) in [
        ("--chart", saved.chart.to_str().unwrap()),
        ("--replay", saved.record.to_str().unwrap()),
        ("--speed", "5/4"),
    ] {
        assert!(launch
            .chunks_exact(2)
            .any(|pair| pair[0] == flag && pair[1] == expected));
    }
    assert!(!launch.iter().any(|arg| arg == "--record-replay"));
    assert_eq!(game.launch.retry().unwrap().args(), launch);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app
        .game
        .as_ref()
        .unwrap()
        .worker
        .as_ref()
        .unwrap()
        .is_finished()
    {
        assert!(Instant::now() < deadline, "WATCH worker failed to finish");
        thread::yield_now();
    }
    app.collect_game();
    let effects = EFFECTS.lock().unwrap();
    assert_eq!(
        effects.iter().map(|effect| effect.step).collect::<Vec<_>>(),
        ["validate", "prepare", "replay"]
    );
    for effect in effects.iter() {
        assert_eq!(effect.args, launch);
    }
    assert_eq!(effects[0].worker, thread::current().id());
    assert_ne!(effects[1].worker, thread::current().id());
    assert_ne!(effects[0].worker, effects[1].worker);
    assert_eq!(effects[1].worker, effects[2].worker);
    assert!(app.game.as_ref().unwrap().joined);
    assert!(app.window.is_none() && app.renderer.is_none());
    assert_eq!(std::fs::read(&saved.record).unwrap(), original_bytes);
    assert!(!saved.directory.join("new.bkr").exists());
}

#[test]
fn actual_history_of_incompatible_fixed_record_refuses_own_and_other_without_settings_mutation() {
    let saved = SavedRecord::new(false);
    for (seed, speed) in [(12, "1/1"), (13, "3/2")] {
        let mut app = saved.open(seed, speed);
        assert!(app.records.as_ref().unwrap().valid_preview().is_some());
        assert!(RecordPreview::inspect(
            &saved.record,
            &saved.chart,
            &app.settings.as_ref().unwrap().values
        )
        .is_err());
        let draft = app.settings.as_ref().unwrap().values.native_args();
        for control in [52, 53] {
            app.activate(ControlId(control));
            assert_eq!(app.settings.as_ref().unwrap().values.native_args(), draft);
            assert_eq!(saved_opponents(&app.settings.as_ref().unwrap().values), 0);
            let error = app.records.as_ref().unwrap().error.as_deref().unwrap();
            assert!(
                error.contains("seed") || error.contains("profile") || error.contains("speed"),
                "{error}"
            );
            assert!(app.game.is_none() && app.profile_io.is_none());
        }
    }
    let mut matching = saved.open(13, "1/1");
    assert!(RecordPreview::inspect(
        &saved.record,
        &saved.chart,
        &matching.settings.as_ref().unwrap().values
    )
    .is_ok());
    for (index, control) in [52, 53].into_iter().enumerate() {
        matching.activate(ControlId(control));
        assert_eq!(
            saved_opponents(&matching.settings.as_ref().unwrap().values),
            index + 1
        );
        assert!(matching.records.as_ref().unwrap().error.is_none());
    }
}

#[test]
fn actual_metadata_keeps_archive_error_separate_and_refuses_broken_recording() {
    let saved = SavedRecord::new(true);
    let sidecar =
        beatkernel_bms_runtime::native_result_archive::sidecar_path(&saved.record).unwrap();
    std::fs::write(sidecar, b"broken archive").unwrap();
    let mut app = saved.open(13, "1/1");
    let preview = app.records.as_ref().unwrap().valid_preview().unwrap();
    assert_eq!(preview.records, 1);
    assert!(preview.archive_error.is_some());
    assert!(preview.historical.is_none());
    app.activate(ControlId(66));
    assert!(!app.records.as_ref().unwrap().details);
    std::fs::write(&saved.record, b"broken replay").unwrap();
    app.activate(ControlId(51));
    collect_record_metadata(&mut app);
    assert!(app.records.as_ref().unwrap().valid_preview().is_none());
    assert!(app.records.as_ref().unwrap().error.is_some());
    let draft = app.settings.as_ref().unwrap().values.native_args();
    for control in [52, 53, 59] {
        app.activate(ControlId(control));
        assert!(app.game.is_none() && app.profile_io.is_none());
        assert_eq!(app.settings.as_ref().unwrap().values.native_args(), draft);
    }
}

#[test]
fn actual_watch_preflight_refusal_preserves_selected_variable_history_and_settings() {
    fn refuse(args: &[String]) -> Result<(), Box<dyn Error>> {
        assert!(args.chunks_exact(2).any(|pair| pair[0] == "--replay"));
        Err("injected syntax preflight refusal".into())
    }
    let saved = SavedRecord::new(true);
    let mut app = saved.open(13, "1/1");
    let draft = app.settings.as_ref().unwrap().values.native_args();
    let score = app
        .records
        .as_ref()
        .unwrap()
        .valid_preview()
        .unwrap()
        .score
        .clone();
    app.validate_replay = refuse;
    app.activate(ControlId(59));
    assert_eq!(app.navigator.route(), ScreenRoute::Records);
    assert!(app.game.is_none() && app.profile_io.is_none());
    assert_eq!(app.settings.as_ref().unwrap().values.native_args(), draft);
    let records = app.records.as_ref().unwrap();
    assert_eq!(records.selected_path(), Some(&saved.record));
    assert_eq!(records.valid_preview().unwrap().score, score);
    assert!(records
        .error
        .as_deref()
        .unwrap()
        .contains("preflight refusal"));
}
