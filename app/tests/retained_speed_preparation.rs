//! Original PCM, actual callbacks and command accounting of cold retained preparation.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms_runtime::{
    native_audio::{
        prepare_retained_audio, prepare_retained_audio_with_speed, NativeAudioConfig,
        NativePracticeAudioConfig, required_bgm_overlap, required_bgm_overlap_with_speed,
    },
    replay_speed::ReplaySpeed,
};

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn speed(s: &str) -> ReplaySpeed {
    s.parse().unwrap()
}
fn bank() -> SampleBank {
    let f = AudioFormat::new(1000, 1).unwrap();
    // Eight mono f32 frames occupy exactly 32 bytes in this original asset.
    let l = PcmLimits::new(32, 128, 1).unwrap();
    let mut b = SampleBank::new(f, l).unwrap();
    b.insert(
        SampleId(1),
        PcmSample::new(f, vec![0., 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875], l).unwrap(),
    )
    .unwrap();
    b
}
fn config(start: i64, p: i64) -> NativeAudioConfig {
    NativeAudioConfig {
        output_origin: ClockPoint {
            domain: ClockDomainId(72),
            timestamp: ts(0),
        },
        start: ts(start),
        preroll: Duration::from_nanos(p),
        lookahead: Duration::from_nanos(100_000_000),
        voices: 2,
        max_render_frames: 64,
        playback_end_frame: None,
        gated_start: false,
    }
}
fn practice(start: i64, end: i64) -> NativePracticeAudioConfig {
    NativePracticeAudioConfig {
        region: PracticeRegion::new(ts(start), ts(end), false).unwrap(),
        limits: PracticeLimits::new(8, 2, 64, 4, 8).unwrap(),
    }
}
fn play(at: i64, voice: u64) -> AudioCommand {
    AudioCommand::Play {
        at: ts(at),
        voice: VoiceId(voice),
        sample: SampleId(1),
        gain: 1.0,
    }
}

#[test]
fn actual_original_bank_bgm_and_gameplay_heads_share_four_rates_without_fake_credits() {
    for (s, expected) in [
        ("1/2", [0., 0.0625, 0.125, 0.1875]),
        ("1/1", [0., 0.125, 0.25, 0.375]),
        ("2/1", [0., 0.25, 0.5, 0.75]),
        ("3/2", [0., 0.1875, 0.375, 0.5625]),
    ] {
        for bgm in [true, false] {
            let mut p = prepare_retained_audio_with_speed(
                bank(),
                if bgm { vec![play(0, 1)] } else { vec![] },
                config(0, 0),
                practice(0, 20_000_000),
                speed(s),
                Some(ts(20_000_000)),
            )
            .unwrap();
            assert_eq!(p.audio.bgm.report().remaining, 0);
            assert_eq!(p.audio.bgm.report().total_admitted, 0);
            assert_eq!(p.audio.mixer.counters().commands_consumed, 0);
            assert_eq!(p.audio.mixer.rate(), speed(s).rate());
            if !bgm {
                p.audio.producer.try_push(play(0, 2)).unwrap();
            }
            let mut pcm = [99.; 4];
            let report = p.audio.mixer.render(&mut pcm).unwrap();
            assert_eq!(pcm, expected, "{s}, bgm={bgm}");
            assert_eq!(report.counters.commands_consumed, u64::from(!bgm));
            assert_eq!(report.counters.commands_applied, u64::from(!bgm));
        }
    }
}

#[test]
fn odd_whole_preroll_receipt_and_cue_grid_are_original_coordinates() {
    let mut p = prepare_retained_audio_with_speed(
        bank(),
        vec![play(10_000_000, 1)],
        config(10_000_000, 1_500_001),
        practice(10_000_000, 20_000_000),
        speed("3/2"),
        Some(ts(20_000_000)),
    )
    .unwrap();
    let mut pcm = [99.; 5];
    p.audio.mixer.render(&mut pcm).unwrap();
    // P=1.500001ms puts original S cue on output frame2; head starts at its original zero.
    assert_eq!(pcm, [0., 0., 0., 0.1875, 0.375]);
    let receipt = p.practice.try_pop_receipt().unwrap();
    assert_eq!(receipt.requested_song_time, ts(10_000_000));
    assert_eq!(receipt.applied_song_time, ts(7_749_999));
    assert_eq!(receipt.correction_nanos, -2_250_001);
    assert_eq!(receipt.physical_frame, 0);
    assert_eq!(receipt.playback_frame, 0);
}

#[test]
fn backward_request_reads_original_suffix_on_same_output_and_rate() {
    let mut p = prepare_retained_audio_with_speed(
        bank(),
        vec![play(0, 1)],
        config(2_000_000, 0),
        practice(2_000_000, 20_000_000),
        speed("3/2"),
        Some(ts(20_000_000)),
    )
    .unwrap();
    let mut first = [99.; 2];
    p.audio.mixer.render(&mut first).unwrap();
    assert_eq!(first, [0.25, 0.4375]);
    p.practice.try_pop_receipt().unwrap();
    p.practice
        .try_request(PracticeRequest {
            id: 7,
            expected_generation: 1,
            at_playback_frame: 2,
            region: PracticeRegion::new(ts(0), ts(20_000_000), false).unwrap(),
        })
        .unwrap();
    let mut second = [99.; 3];
    p.audio.mixer.render(&mut second).unwrap();
    assert_eq!(second, [0., 0.1875, 0.375]);
    let r = p.practice.try_pop_receipt().unwrap();
    assert_eq!(r.generation, 2);
    assert_eq!(r.physical_frame, 2);
    assert_eq!(r.playback_frame, 2);
    assert_eq!(r.applied_song_time, ts(0));
    assert_eq!(p.audio.mixer.rate(), speed("3/2").rate());
    assert_eq!(p.audio.mixer.counters().commands_consumed, 0);
}

#[test]
fn normal_opt_in_zero_preroll_preserves_legacy_pcm_reports_and_receipts() {
    let mut old = prepare_retained_audio(
        bank(),
        vec![play(0, 1)],
        config(0, 0),
        practice(0, 8_000_000),
    )
    .unwrap();
    let mut new = prepare_retained_audio_with_speed(
        bank(),
        vec![play(0, 1)],
        config(0, 0),
        practice(0, 8_000_000),
        speed("8/8"),
        Some(ts(8_000_000)),
    )
    .unwrap();
    for _ in 0..3 {
        let mut a = [99.; 4];
        let mut b = [99.; 4];
        assert_eq!(
            old.audio.mixer.render(&mut a).unwrap(),
            new.audio.mixer.render(&mut b).unwrap()
        );
        assert_eq!(a, b);
        assert_eq!(
            old.practice.try_pop_receipt(),
            new.practice.try_pop_receipt()
        );
    }
}

#[test]
fn conceptual_open_max_accepts_slow_song_but_explicit_finite_overflow_refuses() {
    let mut open = prepare_retained_audio_with_speed(
        bank(),
        vec![play(0, 1)],
        config(0, 0),
        practice(0, i64::MAX),
        speed("1/2"),
        None,
    )
    .unwrap();
    let mut pcm = [99.; 4];
    open.audio.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0., 0.0625, 0.125, 0.1875]);
    assert!(prepare_retained_audio_with_speed(
        bank(),
        vec![],
        config(0, 0),
        practice(0, i64::MAX),
        speed("1/2"),
        Some(ts(i64::MAX))
    )
    .is_err());
}

#[test]
fn finite_program_end_does_not_install_immutable_mixer_end() {
    let mut p = prepare_retained_audio_with_speed(
        bank(),
        vec![play(0, 1)],
        config(0, 0),
        practice(0, 4_000_000),
        speed("2/1"),
        Some(ts(4_000_000)),
    )
    .unwrap();
    let mut first = [99.; 4];
    p.audio.mixer.render(&mut first).unwrap();
    assert_eq!(first, [0., 0.25, 0., 0.]);
    let started = p.practice.try_pop_receipt().unwrap();
    assert_eq!(started.kind, PracticeBoundaryKind::Started);
    assert_eq!(started.generation, 1);
    let ended = p.practice.try_pop_receipt().unwrap();
    assert_eq!(ended.kind, PracticeBoundaryKind::Ended);
    assert_eq!(ended.generation, 2);
    p.practice
        .try_request(PracticeRequest {
            id: 9,
            expected_generation: ended.generation,
            at_playback_frame: 4,
            region: PracticeRegion::new(ts(0), ts(4_000_000), false).unwrap(),
        })
        .unwrap();
    let mut next = [99.; 4];
    p.audio.mixer.render(&mut next).unwrap();
    assert_eq!(next, first);
    let restarted = p.practice.try_pop_receipt().unwrap();
    assert_eq!(restarted.kind, PracticeBoundaryKind::Requested);
    assert_eq!(restarted.generation, 3);
    assert_eq!(restarted.physical_frame, 4);
}

#[test]
fn cold_contract_mismatch_and_invalid_original_commands_refuse_before_ownership() {
    let mut finite = config(0, 0);
    finite.playback_end_frame = Some(4);
    assert!(prepare_retained_audio_with_speed(
        bank(),
        vec![],
        finite,
        practice(0, 4_000_000),
        speed("3/2"),
        Some(ts(4_000_000))
    )
    .is_err());
    assert!(prepare_retained_audio_with_speed(
        bank(),
        vec![],
        config(1, 0),
        practice(0, 4_000_000),
        speed("3/2"),
        Some(ts(4_000_000))
    )
    .is_err());
    assert!(prepare_retained_audio_with_speed(
        bank(),
        vec![],
        config(0, 0),
        practice(0, 4_000_000),
        speed("3/2"),
        Some(ts(4_000_001))
    )
    .is_err());
    assert!(prepare_retained_audio_with_speed(
        bank(),
        vec![],
        config(0, 0),
        practice(0, 4_000_000),
        speed("3/2"),
        None
    )
    .is_err());
    assert!(prepare_retained_audio_with_speed(
        bank(),
        vec![play(-1, 1)],
        config(0, 0),
        practice(0, 4_000_000),
        speed("3/2"),
        Some(ts(4_000_000))
    )
    .is_err());
}

fn cold_owner_with_program(
    program_rate: &str,
    program_preroll: i64,
    program_open: bool,
    owner_rate: &str,
    owner_preroll: i64,
    owner_open: bool,
) -> bool {
    use beatkernel_bms_runtime::{
        live_speed::LiveSpeedSetup,
        local_players::PlayerId,
        play_policy::ResolvedPlayPolicy,
        practice_playback::{PracticeMember, PracticePlayback},
        session_launch::SessionLaunch,
    };
    let program_end = if program_open { i64::MAX } else { 20_000_000 };
    let owner_end = if owner_open { i64::MAX } else { 20_000_000 };
    let prepared = prepare_retained_audio_with_speed(
        bank(),
        vec![],
        config(0, program_preroll),
        practice(0, program_end),
        speed(program_rate),
        (!program_open).then_some(ts(program_end)),
    )
    .unwrap();
    // This is an actually installed program/controller pair, not handcrafted metadata.
    assert_eq!(prepared.practice.rate(), speed(program_rate).rate());
    assert_eq!(
        prepared.practice.output_preroll(),
        Duration::from_nanos(program_preroll)
    );
    assert_eq!(prepared.practice.has_open_end(), program_open);
    let source =
        beatkernel_bms::parse("#BPM 120\n#WAV01 key.wav\n#00011:01\n", Default::default()).unwrap();
    let members = vec![PracticeMember {
        player: PlayerId(1),
        policy: ResolvedPlayPolicy::builtin(1, 1, 0).unwrap(),
        launch: SessionLaunch::new(vec![
            "--chart".into(),
            "original.bms".into(),
            "--speed".into(),
            owner_rate.into(),
        ])
        .unwrap(),
        chart_seed: 0,
        capture_limits: None,
    }];
    let setup = LiveSpeedSetup::new(
        ts(0),
        Duration::from_nanos(owner_preroll),
        speed(owner_rate),
        (!owner_open).then_some(ts(owner_end)),
    )
    .unwrap();
    let result = PracticePlayback::new_with_speed(
        prepared.practice,
        source,
        members,
        practice(0, owner_end).region,
        ts(owner_end),
        OutputFrameBasis::new(config(0, 0).output_origin, 1000, 0).unwrap(),
        1,
        setup.end(),
        setup,
    );
    // Rejected app publication has not changed the real output's rate or fabricated credits.
    assert_eq!(prepared.audio.mixer.rate(), speed(program_rate).rate());
    assert_eq!(prepared.audio.mixer.counters().commands_consumed, 0);
    assert_eq!(prepared.audio.mixer.counters().commands_applied, 0);
    if let Ok(owner) = &result {
        assert_eq!(owner.speed(), speed(owner_rate));
        assert_eq!(owner.initial_setup(), Some(setup));
        assert_eq!(owner.generation(), 1);
    }
    result.is_ok()
}

#[test]
fn cold_owner_refuses_program_rate_mismatch_even_when_zero_preroll_receipt_is_identical() {
    // At P=0 both rates would advertise requested=applied=0 in Started.
    // Immutable controller metadata must reject before any such receipt exists.
    assert!(!cold_owner_with_program("1/1", 0, false, "3/2", 0, false));
    assert!(!cold_owner_with_program("3/2", 0, false, "1/2", 0, false));
}

#[test]
fn cold_owner_refuses_program_preroll_mismatch_at_same_rate() {
    assert!(!cold_owner_with_program(
        "3/2", 1_500_001, false, "3/2", 1_500_000, false
    ));
    assert!(!cold_owner_with_program("3/2", 0, false, "3/2", 1, false));
}

#[test]
fn cold_owner_refuses_program_open_intent_mismatch_in_both_directions() {
    assert!(!cold_owner_with_program("1/2", 0, true, "1/2", 0, false));
    assert!(!cold_owner_with_program("3/2", 0, false, "3/2", 0, true));
}

#[test]
fn cold_owner_accepts_matching_finite_open_and_canonical_program_settings() {
    assert!(cold_owner_with_program(
        "6/4", 1_500_001, false, "3/2", 1_500_001, false
    ));
    assert!(cold_owner_with_program("1/2", 0, true, "1/2", 0, true));
}

fn one_frame_bgm_bank() -> SampleBank {
    let format = AudioFormat::new(48_000, 1).unwrap();
    let limits = PcmLimits::new(4, 16, 2).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25], limits).unwrap(),
    )
    .unwrap();
    bank
}

#[test]
fn selected_overlap_budget_reaches_real_retained_pcm_with_ample_total_voices() {
    let commands = vec![play(0, 1), play(25_000, 2)];
    for (selected, budget, expected) in [
        ("1/1", 1, [0.25, 0., 0.25, 0.]),
        ("3/2", 2, [0.25, 0.25, 0., 0.]),
        ("2/1", 2, [0.25, 0.25, 0., 0.]),
    ] {
        let bank = one_frame_bgm_bank();
        let original_commands = commands.clone();
        let original_pcm = bank.get(SampleId(1)).unwrap().samples().to_vec();
        assert_eq!(required_bgm_overlap(&bank, &commands).unwrap(), 1);
        assert_eq!(
            required_bgm_overlap_with_speed(&bank, &commands, speed(selected)).unwrap(),
            budget
        );
        assert_eq!(commands, original_commands);
        assert_eq!(bank.get(SampleId(1)).unwrap().samples(), original_pcm);
        assert_eq!(bank.get(SampleId(1)).unwrap().frames(), 1);
        assert_eq!(
            bank.get(SampleId(1)).unwrap().format(),
            AudioFormat::new(48_000, 1).unwrap()
        );

        let mut audio_config = config(0, 0);
        audio_config.voices = 256;
        let practice_config = NativePracticeAudioConfig {
            region: PracticeRegion::new(ts(0), ts(1_000_000), false).unwrap(),
            limits: PracticeLimits::new(2, budget, 64, 4, 8).unwrap(),
        };
        let mut prepared = prepare_retained_audio_with_speed(
            bank,
            commands.clone(),
            audio_config,
            practice_config,
            speed(selected),
            Some(ts(1_000_000)),
        )
        .unwrap();
        assert_eq!(prepared.audio.bgm.report().remaining, 0);
        assert_eq!(prepared.audio.bgm.report().total_admitted, 0);
        let mut pcm = [99.; 4];
        let report = prepared.audio.mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, expected, "selected={selected}, BGM budget={budget}");
        assert_eq!(report.counters.commands_consumed, 0);
        assert_eq!(report.counters.commands_applied, 0);
        assert_eq!(report.counters.voice_full, 0);
        let started = prepared.practice.try_pop_receipt().unwrap();
        assert_eq!(started.kind, PracticeBoundaryKind::Started);
        assert_eq!(started.generation, 1);
        assert_eq!(started.requested_song_time, ts(0));
        assert_eq!(started.applied_song_time, ts(0));
        assert_eq!(started.physical_frame, 0);
        assert_eq!(started.playback_frame, 0);
        assert_eq!(
            prepared.practice.try_pop_receipt(),
            Err(PracticeError::Empty)
        );

        if budget == 2 {
            // Ample global capacity cannot repair a wrong cold BGM reservation.
            assert!(matches!(
                PreparedPracticeProgram::new_with_speed(
                    &one_frame_bgm_bank(),
                    vec![
                        PracticeCue {
                            at: ts(0),
                            voice: VoiceId(1),
                            sample: SampleId(1),
                            gain: 1.0
                        },
                        PracticeCue {
                            at: ts(25_000),
                            voice: VoiceId(2),
                            sample: SampleId(1),
                            gain: 1.0
                        }
                    ],
                    PracticeLimits::new(2, 1, 64, 4, 8).unwrap(),
                    speed(selected).rate(),
                    Duration::ZERO,
                    false,
                ),
                Err(PracticeError::Capacity)
            ));
        }
    }
}

#[test]
fn selected_overlap_budget_uses_original_mixed_hz_pcm_and_discrete_output_extent() {
    let output = AudioFormat::new(1000, 1).unwrap();
    let source = AudioFormat::new(1500, 1).unwrap();
    let pcm_limits = PcmLimits::new(8, 32, 2).unwrap();
    let mut bank = SampleBank::new(output, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(source, vec![0.125, 0.25], pcm_limits).unwrap(),
    )
    .unwrap();
    let commands = vec![play(250_000, 1), play(1_650_000, 2)];
    assert_eq!(required_bgm_overlap(&bank, &commands).unwrap(), 2);
    let budget = required_bgm_overlap_with_speed(&bank, &commands, speed("1/2")).unwrap();
    assert_eq!(budget, 2);
    assert_eq!(bank.get(SampleId(1)).unwrap().format(), source);
    assert_eq!(bank.get(SampleId(1)).unwrap().samples(), [0.125, 0.25]);
    let mut audio_config = config(200_000, 0);
    audio_config.voices = 256;
    let mut prepared = prepare_retained_audio_with_speed(
        bank,
        commands,
        audio_config,
        NativePracticeAudioConfig {
            region: PracticeRegion::new(ts(200_000), ts(5_000_000), false).unwrap(),
            limits: PracticeLimits::new(2, budget, 64, 4, 8).unwrap(),
        },
        speed("1/2"),
        Some(ts(5_000_000)),
    )
    .unwrap();
    let mut pcm = [99.; 7];
    let report = prepared.audio.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0., 0.125, 0.21875, 0.375, 0.21875, 0.25, 0.]);
    assert_eq!(report.counters.voice_full, 0);
    assert_eq!(report.counters.commands_consumed, 0);
    assert_eq!(report.counters.commands_applied, 0);
    assert_eq!(
        prepared
            .practice
            .try_pop_receipt()
            .unwrap()
            .applied_song_time,
        ts(200_000)
    );
}
