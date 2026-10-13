//! Cold production composition for the selected Linux cohort route. No supplied
//! HOST point below is an original native observation or hardware timing claim.
use super::*;
use super::super::native::{
    coordinate_initial_query, coordinate_initial_transport, prepare_coordinate_target_state,
};
use beatkernel::audio::{
    AudioFormat, AudioCommand, PcmSample, SampleBank, SampleId, VoiceId, PracticeLimits,
    PracticeRegion, CommandScope,
};
use beatkernel::runtime::SoundBinding;
use beatkernel::judge::JudgeStage;
use beatkernel_bms_runtime::{
    PreparedBms,
    live_speed::LiveSpeedSetup,
    native_audio::{prepare_retained_audio_with_speed, NativePracticeAudioConfig},
    native_cohort_setup::{
        prepare_audio_cohort_with_policy_and_speed, activate_audio_coordinate_cohort_with_sounds,
    },
    play_policy::{GaugeSelection, OriginalGaugeContext},
    practice_playback::{PracticeMember, PracticePlayback},
    replay_speed::ReplaySpeed,
    session_launch::SessionLaunch,
};

fn prepared() -> PreparedBms {
    let source = beatkernel_bms::parse(
        "#BPM 3000\n#WAV01 key.wav\n#00011:00010000\n#00001:01\n",
        Default::default(),
    )
    .unwrap();
    let compiled = source.compile().unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5], limits).unwrap(),
    )
    .unwrap();
    let sounds = vec![SoundBinding {
        object: compiled.chart.objects()[0].id,
        stage: JudgeStage::Instant,
        sample: SampleId(1),
        voice: VoiceId(1),
        gain: 1.0,
    }];
    PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands: vec![AudioCommand::Play {
            voice: VoiceId(99),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        }],
    }
}
fn setup(rate: Rate) -> LiveSpeedSetup {
    LiveSpeedSetup::new(
        Timestamp::from_nanos(5_000_000),
        Duration::from_nanos(4_000_000),
        ReplaySpeed::new(rate).unwrap(),
        Some(Timestamp::from_nanos(100_000_000)),
    )
    .unwrap()
}
fn policy(prepared: &PreparedBms) -> beatkernel_bms_runtime::play_policy::ResolvedPlayPolicy {
    policy_for_source(&prepared.source)
}
fn policy_for_source(
    source: &beatkernel_bms::BmsChart,
) -> beatkernel_bms_runtime::play_policy::ResolvedPlayPolicy {
    beatkernel_bms_runtime::native_judge::NativeJudgeConfig {
        early: 1_000_000,
        late: 2_000_000,
        offset: 0,
        preroll: 4_000_000,
        output: OUTPUT,
        end: Some(Timestamp::from_nanos(100_000_000)),
    }
    .resolve_play_policy(
        &OriginalGaugeContext::from_source(source),
        GaugeSelection::BeatKernel,
    )
    .unwrap()
}
fn config<'a>(
    bindings: &'a std::collections::BTreeMap<u8, u16>,
    setup: LiveSpeedSetup,
    policy: &beatkernel_bms_runtime::play_policy::ResolvedPlayPolicy,
) -> CohortPreparation<'a> {
    CohortPreparation {
        host: HOST,
        output: OUTPUT,
        early: policy.judge().max_early().as_nanos(),
        late: policy.judge().max_late().as_nanos(),
        offset: policy.judge().input_offset().as_nanos(),
        preroll: setup.preroll().as_nanos(),
        start: setup.start(),
        end: setup.end(),
        chart_seed: 17,
        bindings,
        record_replay: Some(std::path::Path::new("coordinate-cohort.bkr")),
        replay_max_bytes: 65536,
        replay_max_records: 128,
    }
}

#[test]
fn selected_cohort_cold_route_preserves_one_phase_and_original_sparse_roster() {
    for count in [2usize, 4] {
        for rate in [Rate::NORMAL, Rate::new(3, 2).unwrap()] {
            let prepared = prepared();
            let policy = policy(&prepared);
            let setup = setup(rate);
            let original = prepared.source.clone();
            let assignments = (0..count)
                .map(|i| (PlayerId(7 + 11 * i as u32), DeviceId(100 + i as u64)))
                .collect::<Vec<_>>();
            let bindings = std::collections::BTreeMap::from([(0x11, 4)]);
            let cohort = prepare_audio_cohort_with_policy_and_speed(
                &prepared,
                &assignments,
                &CompetitionOptions::default(),
                &config(&bindings, setup, &policy),
                LOGICAL,
                &policy,
                setup,
            )
            .unwrap();
            for ((member, state), &(player, device)) in
                cohort.configs.iter().zip(&cohort.states).zip(&assignments)
            {
                assert_eq!(member.player, player);
                assert_eq!(member.device, Some(device));
                assert_eq!(state.player, player);
                assert_eq!(state.last_song, setup.song_origin());
                assert_eq!(state.gauge.profile(), policy.gauge());
                // A finite diagnostic section uses its explicit endpoint, not
                // the full-song completion/drain owner.
                assert!(state.completion.is_none());
                let capture = state.capture.as_ref().unwrap();
                assert_eq!(capture.header().normalized_clock, LOGICAL);
                let recorded = beatkernel_bms_runtime::replay_playback::decode_section_setup(
                    &capture.header().options,
                )
                .unwrap();
                assert_eq!(recorded.source_speed, setup.speed());
                assert_eq!(recorded.start, setup.start());
                assert_eq!(recorded.end, setup.end());
                assert_eq!(recorded.chart_seed, 17);
                assert_eq!(&recorded.gauge, policy.gauge());
                assert!(capture.records().is_empty());
            }
            let region = PracticeRegion::new(setup.start(), setup.end().unwrap(), false).unwrap();
            let retained = prepare_retained_audio_with_speed(
                prepared.bank,
                prepared.bgm_commands,
                NativeAudioConfig {
                    output_origin: output_origin(),
                    start: setup.start(),
                    preroll: setup.preroll(),
                    lookahead: Duration::from_nanos(8_000_000),
                    voices: 8,
                    max_render_frames: 64,
                    playback_end_frame: None,
                    gated_start: false,
                },
                NativePracticeAudioConfig {
                    region,
                    limits: PracticeLimits::new(8, 1, 64, 8, 64).unwrap(),
                },
                setup.speed(),
                setup.end(),
            )
            .unwrap();
            let mixer_basis = retained.audio.mixer.output_frame_basis();
            let members = assignments
                .iter()
                .map(|&(player, _)| PracticeMember {
                    player,
                    policy: policy_for_source(&original),
                    launch: SessionLaunch::new(vec![
                        "--chart".into(),
                        "practice.bms".into(),
                        "--speed".into(),
                        format!("{}/{}", rate.numerator(), rate.denominator()),
                    ])
                    .unwrap(),
                    chart_seed: 17,
                    capture_limits: None,
                })
                .collect::<Vec<_>>();
            for member in &members {
                assert_eq!(member.policy.judge(), policy.judge());
                assert_eq!(member.policy.gauge(), policy.gauge());
                assert_eq!(member.policy.timing(), policy.timing());
                assert_eq!(member.policy.judgments(), policy.judgments());
            }
            let practice = PracticePlayback::new_with_speed(
                retained.practice,
                original,
                members,
                region,
                setup.end().unwrap(),
                mixer_basis,
                0,
                setup.end(),
                setup,
            )
            .unwrap();
            assert_eq!(practice.initial_setup(), Some(setup));
            assert_eq!(practice.speed(), setup.speed());
            assert_eq!(
                practice
                    .members()
                    .iter()
                    .map(|m| m.player)
                    .collect::<Vec<_>>(),
                assignments.iter().map(|a| a.0).collect::<Vec<_>>()
            );
            let target = DeviceFormat::new(1500, 1, SampleEncoding::Float32, None).unwrap();
            let (mut output, identity) =
                prepare_coordinate_target_state(retained.audio.mixer, target, 8, 64).unwrap();
            let cold_target = output.target_frame_basis();
            let cold_source = output.converter_owner().source_playback_history().unwrap();
            assert_eq!(identity.hz(), cold_source.source_rate());
            assert_eq!(
                identity.initial_physical(),
                cold_source.initial_physical_frame()
            );
            assert_eq!(
                identity.initial_playback(),
                cold_source.initial_playback_frame()
            );
            assert!(output.phase_history().unwrap().is_empty());
            let query = coordinate_initial_query(cold_target).unwrap();
            assert!(query
                .compare_target_time(cold_target.time_at_stream_frame(0).unwrap())
                .is_eq());
            let logical_origin = ClockPoint {
                domain: LOGICAL,
                timestamp: Timestamp::from_nanos(9_000_000),
            };
            let transport =
                coordinate_initial_transport(29, identity, logical_origin, setup).unwrap();
            let basis = transport.output_phase_basis().unwrap();
            assert_eq!(basis.owner(), 29);
            assert_eq!(basis.generation(), 1);
            assert_eq!(basis.active_origin_frame(), identity.initial_playback());
            assert_eq!(basis.source_hz(), identity.hz());
            let phase = basis.point_from_elapsed_nanos(logical_origin, 0).unwrap();
            assert_eq!(
                transport.position_at_output_phase(phase).unwrap(),
                setup.song_origin()
            );
            let mut producer = retained.audio.producer;
            producer.set_scope(CommandScope(0));
            let (group, merger) = activate_audio_coordinate_cohort_with_sounds(
                cohort.configs,
                &cohort.reserved,
                ClockPoint {
                    domain: HOST,
                    timestamp: Timestamp::from_nanos(100),
                },
                OUTPUT,
                logical_origin,
                transport,
                producer,
                setup.end(),
                cohort.input_sounds,
                cohort.hazard_sounds,
            )
            .unwrap();
            assert_eq!(merger.source_count(), count);
            assert_eq!(merger.pending(), 0);
            assert_eq!(group.audio_scope(), CommandScope(1));
            assert_eq!(group.transport().output_phase_basis(), Some(basis));
            assert_eq!(group.transport().anchor().rate, rate);
            for &(player, _) in &assignments {
                assert!(group
                    .member_judge(player)
                    .unwrap()
                    .effective_song_time()
                    .is_none());
            }
            let generated = output.render_pending(8).unwrap();
            assert!(!output.phase_history().unwrap().is_empty());
            let history = output.converter_owner().source_playback_history().unwrap();
            assert_eq!(
                history.initial_physical_frame(),
                identity.initial_physical()
            );
            assert_eq!(
                history.initial_playback_frame(),
                identity.initial_playback()
            );
            assert_eq!(output.pending_frames(), generated.target_frames);
            output.admit(generated.target_frames).unwrap();
            assert_eq!(output.pending_frames(), 0);
            // Generation/admission did not pretend to acquire a HOST event.
            assert_eq!(merger.pending(), 0);
            assert_eq!(group.transport().output_phase_basis(), Some(basis));
        }
    }
}

#[test]
fn selected_full_song_cohort_prepares_every_original_member_completion_owner() {
    let prepared = prepared();
    let policy = policy(&prepared);
    let finite = setup(Rate::new(3, 2).unwrap());
    let full = LiveSpeedSetup::new(finite.start(), finite.preroll(), finite.speed(), None).unwrap();
    let bindings = std::collections::BTreeMap::from([(0x11, 4)]);
    for count in [2usize, 4] {
        let assignments = (0..count)
            .map(|i| (PlayerId(7 + 11 * i as u32), DeviceId(100 + i as u64)))
            .collect::<Vec<_>>();
        let cohort = prepare_audio_cohort_with_policy_and_speed(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &config(&bindings, full, &policy),
            LOGICAL,
            &policy,
            full,
        )
        .unwrap();
        for ((member, state), &(player, device)) in
            cohort.configs.iter().zip(&cohort.states).zip(&assignments)
        {
            assert_eq!(member.player, player);
            assert_eq!(member.device, Some(device));
            assert_eq!(state.player, player);
            assert!(state.completion.is_some());
            assert_eq!(state.last_song, full.song_origin());
            assert_eq!(state.gauge.profile(), policy.gauge());
            let capture = state.capture.as_ref().unwrap();
            assert!(capture.records().is_empty());
            assert_eq!(capture.header().normalized_clock, LOGICAL);
            let recorded = beatkernel_bms_runtime::replay_playback::decode_section_setup(
                &capture.header().options,
            )
            .unwrap();
            assert_eq!(recorded.source_speed, full.speed());
            assert_eq!(recorded.start, full.start());
            assert_eq!(recorded.end, None);
            assert_eq!(recorded.chart_seed, 17);
            assert_eq!(&recorded.gauge, policy.gauge());
        }
    }
}

#[test]
fn coordinate_cohort_admission_refuses_missing_or_unscoped_phase() {
    let prepared = prepared();
    let policy = policy(&prepared);
    let selected = setup(Rate::new(3, 2).unwrap());
    let bindings = std::collections::BTreeMap::from([(0x11, 4)]);
    let assignments = [(PlayerId(7), DeviceId(100)), (PlayerId(18), DeviceId(101))];
    let logical = ClockPoint {
        domain: LOGICAL,
        timestamp: Timestamp::ZERO,
    };
    let zero_scope =
        beatkernel::transport::OutputPhaseBasis::new(LOGICAL, 29, 0, 13, 1000).unwrap();
    let candidates = [
        selected.transport(logical.timestamp).unwrap(),
        Transport::new_with_output_phase(
            zero_scope,
            logical,
            selected.start(),
            selected.preroll(),
            selected.speed().rate(),
            selected.end(),
        )
        .unwrap(),
    ];
    for transport in candidates {
        let cohort = prepare_audio_cohort_with_policy_and_speed(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &config(&bindings, selected, &policy),
            LOGICAL,
            &policy,
            selected,
        )
        .unwrap();
        assert!(cohort
            .states
            .iter()
            .all(|s| s.capture.as_ref().unwrap().records().is_empty()));
        let (producer, _) = beatkernel::audio::command_queue(64).unwrap();
        assert!(activate_audio_coordinate_cohort_with_sounds(
            cohort.configs,
            &cohort.reserved,
            ClockPoint {
                domain: HOST,
                timestamp: Timestamp::ZERO
            },
            OUTPUT,
            logical,
            transport,
            producer,
            selected.end(),
            cohort.input_sounds,
            cohort.hazard_sounds
        )
        .is_err());
        // Cold prepared records and score/gauge state have not become a partial
        // playing roster because activation refused its original phase contract.
        for state in &cohort.states {
            assert_eq!(state.last_song, selected.song_origin());
            assert!(state.capture.as_ref().unwrap().records().is_empty());
            assert_eq!(state.gauge.profile(), policy.gauge());
        }
    }
}

#[test]
fn selected_cohort_refuses_mismatched_section_or_duplicate_ids_before_members_exist() {
    let prepared = prepared();
    let policy = policy(&prepared);
    let setup = setup(Rate::new(3, 2).unwrap());
    let bindings = std::collections::BTreeMap::from([(0x11, 4)]);
    let original_hash = prepared.compiled.chart.objects().to_vec();
    for assignments in [
        vec![(PlayerId(7), DeviceId(100)), (PlayerId(7), DeviceId(101))],
        vec![(PlayerId(7), DeviceId(100)), (PlayerId(18), DeviceId(100))],
    ] {
        assert!(prepare_audio_cohort_with_policy_and_speed(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &config(&bindings, setup, &policy),
            LOGICAL,
            &policy,
            setup
        )
        .is_err());
    }
    let assignments = [(PlayerId(7), DeviceId(100)), (PlayerId(18), DeviceId(101))];
    let mut mismatched = config(&bindings, setup, &policy);
    mismatched.preroll += 1;
    assert!(prepare_audio_cohort_with_policy_and_speed(
        &prepared,
        &assignments,
        &CompetitionOptions::default(),
        &mismatched,
        LOGICAL,
        &policy,
        setup
    )
    .is_err());
    assert_eq!(prepared.compiled.chart.objects(), original_hash.as_slice());
    let valid = prepare_audio_cohort_with_policy_and_speed(
        &prepared,
        &assignments,
        &CompetitionOptions::default(),
        &config(&bindings, setup, &policy),
        LOGICAL,
        &policy,
        setup,
    )
    .unwrap();
    assert_eq!(valid.states.len(), 2);
    assert!(valid
        .states
        .iter()
        .all(|s| s.capture.as_ref().unwrap().records().is_empty()));
}

#[test]
fn cohort_initial_phase_refuses_wrong_logical_axis_and_scalar_activation_stays_guarded() {
    let identity =
        beatkernel_bms_runtime::gameplay::output::ports::SourcePlaybackIdentity::new(1000, 19, 13)
            .unwrap();
    let selected = setup(Rate::new(3, 2).unwrap());
    assert!(coordinate_initial_transport(
        29,
        identity,
        ClockPoint {
            domain: HOST,
            timestamp: Timestamp::ZERO
        },
        selected
    )
    .is_err());
    let prepared = prepared();
    let policy = policy(&prepared);
    let bindings = std::collections::BTreeMap::from([(0x11, 4)]);
    let assignments = [(PlayerId(7), DeviceId(100)), (PlayerId(18), DeviceId(101))];
    let cohort = prepare_audio_cohort_with_policy_and_speed(
        &prepared,
        &assignments,
        &CompetitionOptions::default(),
        &config(&bindings, selected, &policy),
        LOGICAL,
        &policy,
        selected,
    )
    .unwrap();
    let logical = ClockPoint {
        domain: LOGICAL,
        timestamp: Timestamp::ZERO,
    };
    let transport = coordinate_initial_transport(29, identity, logical, selected).unwrap();
    let (producer, _) = beatkernel::audio::command_queue(64).unwrap();
    assert!(activate_audio_cohort_with_sounds(
        cohort.configs,
        &cohort.reserved,
        ClockPoint {
            domain: HOST,
            timestamp: Timestamp::ZERO
        },
        OUTPUT,
        logical,
        transport,
        producer,
        selected.end(),
        cohort.input_sounds,
        cohort.hazard_sounds
    )
    .is_err());
}
