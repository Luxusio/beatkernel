//! Actual UI channel through the production host wrappers; not output proof.
use crate::{
    competition::ScoreSummary,
    native_gameplay_bridge::{PlayerGameplayHost, ResolvedGameplayHost},
    native_gameplay_host::{NativeGameplayHost, NativeScoreHost, NoopGameplayHost},
    practice_control::{PracticeAction, PracticeCapability, PracticeReply},
};
use beatkernel::time::Timestamp;

#[test]
fn positive_initial_section_registers_selected_heads_and_replays_original_sound_identity() {
    use crate::{
        competition_live::{CompetitionOptions, LiveCompetition},
        input_sounds::{InputSoundIdentity, InputSoundPlan},
        local_players::PlayerId,
        mine_sounds::MineSoundPlan,
        native_judge::{prepare_section_capture_for_policy, NativeJudgeConfig},
        play_policy::{GaugeSelection, OriginalGaugeContext},
    };
    use beatkernel::{audio::SampleId, time::ClockDomainId};

    // The first visible head is excluded; press selections and audible mines
    // keep their full original timelines so a later backward scrub can restore them.
    let text = "#BPM 60\n#WAV00 blast.wav\n#WAV01 head.wav\n#WAV02 press.wav\n#00011:01000100\n#00031:02000002\n#000D1:001E001E\n";
    let original = beatkernel_bms::parse(text, Default::default()).unwrap();
    let original_chart = original.compile().unwrap().chart;
    let start = Timestamp::from_nanos(2_000_000_000);
    let selected = crate::section_start::source_at(&original, start).unwrap();
    let chart = selected.compile().unwrap().chart;
    assert_eq!(original_chart.objects().len(), 2);
    assert_eq!(chart.objects().len(), 1);
    assert_eq!(chart.objects()[0].id, original_chart.objects()[1].id);
    assert_eq!(chart.objects()[0].time.start, start);
    assert_eq!(selected.invisible, original.invisible);
    assert_eq!(selected.mines, original.mines);
    let full_press = InputSoundPlan::prepare(&original, &[], &[], 16).unwrap();
    assert_eq!(
        full_press
            .markers()
            .iter()
            .map(|m| (m.control.0, m.at.as_nanos(), m.sample.0))
            .collect::<Vec<_>>(),
        [(0x11, 0, 2), (0x11, 3_000_000_000, 2)]
    );
    let full_mines =
        MineSoundPlan::prepare(&original, &[], &[], Some(&full_press.timeline()), 16).unwrap();
    assert_eq!(full_mines.samples(), &[SampleId(0)]);
    assert_eq!(
        full_mines
            .bindings()
            .iter()
            .map(|m| (m.hazard.0, m.sample.0))
            .collect::<Vec<_>>(),
        [(0, 0), (1, 0)]
    );
    assert_eq!(
        original
            .compile_mines()
            .unwrap()
            .iter()
            .map(|m| (m.ordinal, m.at.as_nanos()))
            .collect::<Vec<_>>(),
        [(0, 1_000_000_000), (1, 3_000_000_000)]
    );
    let identity = InputSoundIdentity::from_source(&original).unwrap().unwrap();
    assert_eq!(
        InputSoundIdentity::from_source(&selected).unwrap(),
        Some(identity)
    );

    let config = NativeJudgeConfig {
        early: 10,
        late: 10,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(17),
        end: None,
    };
    let policy = config
        .resolve_play_policy(
            &OriginalGaugeContext::from_source(&original),
            GaugeSelection::BeatKernel,
        )
        .unwrap();
    let judge = config
        .judge_with_policy(&selected, chart.clone(), &policy)
        .unwrap();
    assert!(judge.effective_song_time().is_none());

    // Registration uses the actual attached channel. The mismatched original
    // source must fail before publishing, leaving the same channel usable.
    let directory = std::env::temp_dir().join(format!(
        "beatkernel-positive-initial-section-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("original.bms");
    std::fs::write(&path, text).unwrap();
    let (publisher, viewer) = crate::player::channel();
    crate::player::with_publisher(publisher, || {
        assert!(
            crate::player::publish_native_chart(&path, &original, &chart, &[PlayerId(1)]).is_err()
        );
        crate::player::publish_native_chart(&path, &selected, &chart, &[PlayerId(1)]).unwrap();
        let snapshot = viewer.take_latest().unwrap();
        let visual = snapshot.players[0].chart.as_ref().unwrap();
        assert_eq!(visual.notes.len(), 1);
        assert_eq!(visual.notes[0].object, chart.objects()[0].id);
        assert_eq!(visual.notes[0].start, start);
        assert_eq!(visual.export_visual().mines.len(), 2);
        Ok::<_, String>(())
    })
    .unwrap();
    std::fs::remove_dir_all(directory).unwrap();

    // Exercise policy preparation even when no opponent or recording is enabled.
    assert!(LiveCompetition::prepare_native_section_with_policy(
        &CompetitionOptions::default(),
        &selected,
        &judge,
        &policy,
        config.output,
        start,
        0,
        None,
        0,
    )
    .unwrap()
    .is_none());
    assert!(prepare_section_capture_for_policy(
        &selected,
        &judge,
        &policy,
        config.output,
        start,
        0,
        None,
        None,
    )
    .unwrap()
    .is_none());
    let limits = crate::native_judge::capture_limits(true, 16384, 128)
        .unwrap()
        .unwrap();
    let capture = prepare_section_capture_for_policy(
        &selected,
        &judge,
        &policy,
        config.output,
        start,
        0,
        None,
        Some(limits),
    )
    .unwrap()
    .unwrap();
    let file = capture.into_file();
    let reconstructed =
        crate::replay_playback::validate_section_setup(&original, &file, limits).unwrap();
    assert_eq!(reconstructed.chart().objects(), chart.objects());
    assert_eq!(
        reconstructed.stable_hash().unwrap(),
        judge.stable_hash().unwrap()
    );
    crate::replay_playback::reconstruct_section(&original, file.clone(), limits).unwrap();
    // Independently prove the original press and audible-mine metadata participates
    // in replay identity, even where its marker precedes the selected visible heads.
    let mut missing_press = original.clone();
    missing_press.invisible.clear();
    assert!(crate::replay_playback::validate_section_setup(&missing_press, &file, limits).is_err());
    let mut missing_blast = original.clone();
    missing_blast.samples.remove(&0);
    assert!(crate::replay_playback::validate_section_setup(&missing_blast, &file, limits).is_err());

    let backward = crate::section_start::source_at(&original, Timestamp::ZERO).unwrap();
    assert_eq!(
        backward.compile().unwrap().chart.objects(),
        original_chart.objects()
    );
    assert_eq!(
        InputSoundPlan::prepare(&backward, &[], &[], 16).unwrap(),
        full_press
    );
    assert_eq!(
        MineSoundPlan::prepare(&backward, &[], &[], Some(&full_press.timeline()), 16).unwrap(),
        full_mines
    );
}

#[test]
fn retained_practice_host_wrappers_preserve_actual_ui_request_and_refusal() {
    let (publisher, viewer) = crate::player::channel();
    let mut score = ScoreSummary::default();
    crate::player::with_publisher(publisher, || {
        (|| -> crate::native_gameplay::NativeGameplayResult<()> {
            let mut player = PlayerGameplayHost;
            let mut resolved = ResolvedGameplayHost {
                host: &mut player,
                policies: &[],
            };
            let mut host = NativeScoreHost::new(&mut resolved, &mut score);
            let capability = PracticeCapability {
                generation: 7,
                min_target: Timestamp::ZERO,
                max_target: Timestamp::from_nanos(604_800_000_000_000),
            };
            host.advertise_practice(Some(capability))?;
            let target = Timestamp::from_nanos(72_000_000_000_000);
            let id = viewer.request_practice(PracticeAction::Scrub { target })?;
            let request = host.take_practice_request()?.unwrap();
            assert_eq!(request.id, id);
            assert_eq!(request.generation, 7);
            assert_eq!(request.action, PracticeAction::Scrub { target });
            assert!(host.take_practice_request()?.is_none());
            let reply = PracticeReply {
                id,
                generation: 7,
                result: Err("control capacity exhausted before boundary".into()),
            };
            host.commit_practice_reply(&reply)?;
            assert_eq!(viewer.take_practice_reply()?, Some(reply));
            assert_eq!(viewer.practice_capability()?, Some(capability));
            assert!(!crate::player::cancelled());
            host.advertise_practice(None)?;
            assert!(viewer.practice_capability()?.is_none());
            Ok(())
        })()
        .map_err(|error| error.to_string())
    })
    .unwrap();
}

#[test]
fn unsupported_native_host_cannot_advertise_or_ack_retained_practice() {
    let mut host = NoopGameplayHost;
    host.advertise_practice(None).unwrap();
    assert!(host
        .advertise_practice(Some(PracticeCapability {
            generation: 1,
            min_target: Timestamp::ZERO,
            max_target: Timestamp::from_nanos(1),
        }))
        .is_err());
    assert!(host.take_practice_request().unwrap().is_none());
    assert!(host
        .commit_practice_reply(&PracticeReply {
            id: 1,
            generation: 1,
            result: Err("unsupported".into()),
        })
        .is_err());
}

#[test]
fn practice_presentation_resets_borrowed_score_only_after_host_commit() {
    use crate::{
        local_players::PlayerId,
        play_policy::ResolvedPlayPolicy,
        practice_session::{prepare_attempt, PracticeAttemptConfig},
        session_launch::SessionLaunch,
    };
    use beatkernel::time::ClockDomainId;
    let original = beatkernel_bms::parse(
        "#BPM 120\n#WAV01 key.wav\n#00011:01\n#00111:01\n",
        beatkernel_bms::ParseOptions::default(),
    )
    .unwrap();
    let policy = ResolvedPlayPolicy::builtin(10, 10, 0).unwrap();
    let launch = SessionLaunch::new(vec!["--chart".into(), "original.bms".into()]).unwrap();
    let attempt = prepare_attempt(
        &original,
        &policy,
        &launch,
        PracticeAttemptConfig {
            start: Timestamp::from_nanos(1_000_000_000),
            end: None,
            domain: ClockDomainId(17),
            chart_seed: 0,
            capture_limits: None,
        },
    )
    .unwrap();
    let payload =
        || crate::player::prepare_practice_presentation(1, &[(PlayerId(1), &attempt)]).unwrap();
    let previous = ScoreSummary {
        hits: 13,
        misses: 2,
        combo: 3,
        max_combo: 8,
        ..Default::default()
    };
    let mut score = previous.clone();
    let mut unsupported = NoopGameplayHost;
    {
        let mut host = NativeScoreHost::new(&mut unsupported, &mut score);
        assert!(host.commit_practice_presentation(payload(), 2).is_err());
    }
    assert_eq!(score, previous);
    let mut player = PlayerGameplayHost;
    let mut resolved = ResolvedGameplayHost {
        host: &mut player,
        policies: &[],
    };
    {
        let mut host = NativeScoreHost::new(&mut resolved, &mut score);
        host.commit_practice_presentation(payload(), 2).unwrap();
    }
    assert_eq!(score, ScoreSummary::default());
    // Actual application resets the borrowed business score even when its
    // later visual observer refuses publication.
    score = previous;
    {
        let mut host = NativeScoreHost::new(&mut unsupported, &mut score);
        let mut prepared = payload();
        host.apply_practice_identity(&mut prepared, 2);
        assert!(host.commit_practice_presentation(prepared, 2).is_err());
    }
    assert_eq!(score, ScoreSummary::default());
}
