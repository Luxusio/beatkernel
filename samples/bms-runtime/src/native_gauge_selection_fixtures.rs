use crate::{
    play_policy::{OriginalGaugeContext, GaugeSelection},
    native_judge::NativeJudgeConfig,
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits, SampleBank},
    time::{ClockDomainId, Timestamp},
};
fn config() -> NativeJudgeConfig {
    NativeJudgeConfig {
        early: 7,
        late: 9,
        offset: -3,
        preroll: 0,
        output: ClockDomainId(2),
        end: None,
    }
}
#[test]
fn practice_keeps_original_total_and_stage_count_for_every_selected_gauge() {
    let source = beatkernel_bms::parse(
        "#BPM 120\n#TOTAL 320.5\n#WAV01 note.wav\n#00011:01010101",
        Default::default(),
    )
    .unwrap();
    let context = OriginalGaugeContext::from_source(&source);
    let format = AudioFormat::new(1000, 1).unwrap();
    let prepared = crate::PreparedBms {
        compiled: source.compile().unwrap(),
        source: source.clone(),
        bank: SampleBank::new(format, PcmLimits::new(64, 256, 4).unwrap()).unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    };
    let (selected, report) = crate::section_start::prepare_at(
        prepared,
        Timestamp::from_nanos(1_000_000_000),
        PcmLimits::new(64, 256, 4).unwrap(),
    )
    .unwrap();
    assert_eq!(report.original_gauge, context);
    assert!(selected.source.judged_stage_count() < context.judged_stages());
    for kind in beatkernel_bms::BmsGaugeKind::ALL {
        let full = config()
            .resolve_play_policy(&context, GaugeSelection::Bms(kind))
            .unwrap();
        let practice = config()
            .resolve_play_policy(&report.original_gauge, GaugeSelection::Bms(kind))
            .unwrap();
        assert_eq!(full, practice);
        assert_eq!(practice.judge().max_early().as_nanos(), 7);
        assert_eq!(practice.judge().max_late().as_nanos(), 9);
        assert_eq!(practice.judge().input_offset().as_nanos(), -3);
    }
    let builtin = config()
        .resolve_play_policy(&context, GaugeSelection::BeatKernel)
        .unwrap();
    assert_eq!(builtin.judge(), &config().profile().unwrap());
    assert_eq!(builtin.gauge(), &crate::gauge::GaugeProfile::default());
}
#[test]
fn custom_competition_refuses_before_resources_and_settings_roundtrip_all_hosts() {
    use crate::{
        competition_live::CompetitionOptions,
        competition::OpponentKind,
        settings::{NativeSettings, SettingsHost},
    };
    let mut options = CompetitionOptions::default();
    options
        .ghosts
        .push((OpponentKind::Other, std::path::PathBuf::from("missing.bkr")));
    assert!(
        crate::native_judge::validate_policy_competition(
            GaugeSelection::Bms(beatkernel_bms::BmsGaugeKind::Hard),
            &options
        )
        .is_err()
    );
    crate::native_judge::validate_policy_competition(GaugeSelection::BeatKernel, &options).unwrap();
    for host in [
        SettingsHost::Linux,
        SettingsHost::Windows,
        SettingsHost::Macos,
    ] {
        let args = vec!["--gauge".to_string(), "hard".to_string()];
        let settings = NativeSettings::from_args(&args, host).unwrap();
        assert_eq!(
            settings
                .fields()
                .iter()
                .find(|row| row.flag == "--gauge")
                .unwrap()
                .value,
            "hard"
        );
        assert!(
            settings
                .native_args()
                .windows(2)
                .any(|pair| pair == ["--gauge", "hard"])
        );
    }
}

#[test]
fn selected_cohort_constructor_preserves_policy_and_capture_for_each_original_player() {
    use crate::{
        native_cohort_setup::{CohortPreparation, prepare_cohort_with_policy},
        local_players::PlayerId,
    };
    use beatkernel::input::DeviceId;
    let source = beatkernel_bms::parse(
        "#BPM 120\n#TOTAL 320\n#WAV01 note.wav\n#00011:01010101",
        Default::default(),
    )
    .unwrap();
    let context = OriginalGaugeContext::from_source(&source);
    let prepared = crate::PreparedBms {
        compiled: source.compile().unwrap(),
        source,
        bank: SampleBank::new(
            AudioFormat::new(1000, 1).unwrap(),
            PcmLimits::new(64, 256, 4).unwrap(),
        )
        .unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    };
    let bindings = std::collections::BTreeMap::from([(0x11, 4)]);
    let cfg = CohortPreparation {
        host: ClockDomainId(1),
        output: ClockDomainId(2),
        early: 7,
        late: 9,
        offset: -3,
        preroll: 0,
        start: Timestamp::ZERO,
        end: None,
        chart_seed: 0,
        bindings: &bindings,
        record_replay: Some(std::path::Path::new("not-written.bkr")),
        replay_max_bytes: 65536,
        replay_max_records: 128,
    };
    for kind in beatkernel_bms::BmsGaugeKind::ALL {
        let policy = config()
            .resolve_play_policy(&context, GaugeSelection::Bms(kind))
            .unwrap();
        let cohort = prepare_cohort_with_policy(
            &prepared,
            &[
                (PlayerId(7), DeviceId(1)),
                (PlayerId(u32::MAX), DeviceId(2)),
            ],
            &crate::competition_live::CompetitionOptions::default(),
            &cfg,
            &policy,
        )
        .unwrap();
        for state in &cohort.states {
            assert_eq!(state.gauge.profile(), policy.gauge());
            assert_eq!(
                crate::replay_playback::decode_section_setup(
                    &state.capture.as_ref().unwrap().header().options
                )
                .unwrap()
                .gauge,
                *policy.gauge()
            );
        }
        for member in &cohort.configs {
            assert_eq!(member.judge.profile(), policy.judge());
        }
    }
}
