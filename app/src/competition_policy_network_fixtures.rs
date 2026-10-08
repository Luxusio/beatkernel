//! Finite network envelopes preserve the actual selected replay setup.
use crate::play_policy::{ClassifiedWindow, ResolvedPlayPolicy};
use beatkernel::{
    judge::{JudgeEngine, JudgeGrade, JudgeWindow},
    replay::codec::{decode_replay, encode_replay, ReplayFile},
    time::{ClockDomainId, Duration, Timestamp},
};

fn source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#TOTAL 320\n#WAV01 note.wav\n#00011:0101",
        Default::default(),
    )
    .unwrap()
}
fn policy(
    source: &beatkernel_bms::BmsChart,
    kind: beatkernel_bms::BmsGaugeKind,
) -> ResolvedPlayPolicy {
    ResolvedPlayPolicy::bms(
        source,
        kind,
        &[ClassifiedWindow {
            judgment: beatkernel_bms::BmsJudgment::Great,
            window: JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::from_nanos(7),
                late: Duration::from_nanos(9),
            },
        }],
        -3,
    )
    .unwrap()
}

#[test]
fn selected_finite_network_envelope_has_no_inner_end_and_capture_keeps_original_policy() {
    let source = source();
    let limits = crate::competition_live::replay_limits().unwrap();
    let start = Timestamp::from_nanos(1_000_000_000);
    let end = Timestamp::from_nanos(2_500_000_000);
    for kind in beatkernel_bms::BmsGaugeKind::ALL {
        let policy = policy(&source, kind);
        let selected = crate::section_start::source_at(&source, start).unwrap();
        let judge = JudgeEngine::new(
            selected.compile().unwrap().chart,
            selected.rules(),
            policy.judge().clone(),
        )
        .unwrap();
        let capture = crate::native_judge::prepare_section_capture_for_policy(
            &source,
            &judge,
            &policy,
            ClockDomainId(17),
            start,
            71,
            Some(end),
            Some(limits),
        )
        .unwrap()
        .unwrap();
        let captured = capture.header().clone();
        let full = crate::native_judge::prepare_policy_header(
            &source,
            &judge,
            &policy,
            ClockDomainId(17),
            start,
            71,
            Some(end),
        )
        .unwrap();
        assert_eq!(full, captured);
        let network = crate::native_judge::prepare_policy_header(
            &source,
            &judge,
            &policy,
            ClockDomainId(17),
            start,
            71,
            None,
        )
        .unwrap();
        let identity = crate::native_judge::prepare_policy_competition_identity(
            &source,
            &judge,
            &policy,
            ClockDomainId(17),
            start,
            71,
            Some(end),
        )
        .unwrap();
        assert_eq!(
            identity,
            crate::multiplayer::competition_identity_for_section(
                &network,
                env!("CARGO_PKG_VERSION"),
                limits,
                Some(end)
            )
            .unwrap()
        );
        let prefix = b"bms-competition-section/v1:";
        assert!(identity.starts_with(prefix));
        assert_eq!(
            &identity[prefix.len()..prefix.len() + 8],
            &end.as_nanos().to_le_bytes()
        );
        let file = decode_replay(&identity[prefix.len() + 8..], limits).unwrap();
        let setup = crate::replay_playback::decode_section_setup(&file.header.options).unwrap();
        assert_eq!(file.header.normalized_clock, ClockDomainId(0));
        assert_eq!(setup.end, None);
        assert_eq!(setup.start, start);
        assert_eq!(setup.chart_seed, 71);
        assert_eq!(setup.profile, *policy.judge());
        assert_eq!(setup.gauge, *policy.gauge());
        assert_eq!(setup.judgments.as_ref(), policy.judgments());
        let retained = crate::replay_playback::decode_section_setup(&captured.options).unwrap();
        assert_eq!(retained.end, Some(end));
        assert_eq!(retained.judgments.as_ref(), policy.judgments());
        assert_eq!(capture.header(), &captured);
        // Feeding the finite local header directly into an outer envelope is
        // invalid; endpoint-free metadata must be derived without mutating it.
        assert!(crate::multiplayer::competition_identity_for_section(
            &captured,
            env!("CARGO_PKG_VERSION"),
            limits,
            Some(end)
        )
        .is_err());
    }
}

#[test]
fn default_canonical_network_bytes_equal_original_replay_encoding_for_all_capture_domains() {
    let source = source();
    let limits = crate::competition_live::replay_limits().unwrap();
    let policy = ResolvedPlayPolicy::builtin(7, 9, -3).unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        policy.judge().clone(),
    )
    .unwrap();
    let mut original =
        crate::replay_capture::setup_header(&judge, ClockDomainId(0), limits, Timestamp::ZERO, 71)
            .unwrap();
    original.normalized_clock = ClockDomainId(0);
    let mut file = ReplayFile::new(original, vec![]);
    file.runtime_version = env!("CARGO_PKG_VERSION").into();
    let expected = encode_replay(&file, limits).unwrap();
    let end = Timestamp::from_nanos(2_500_000_000);
    let mut expected_finite = b"bms-competition-section/v1:".to_vec();
    expected_finite.extend_from_slice(&end.as_nanos().to_le_bytes());
    expected_finite.extend_from_slice(&expected);
    for domain in [ClockDomainId(0), ClockDomainId(17), ClockDomainId(u32::MAX)] {
        let header = crate::native_judge::prepare_policy_header(
            &source,
            &judge,
            &policy,
            domain,
            Timestamp::ZERO,
            71,
            None,
        )
        .unwrap();
        assert_eq!(
            crate::multiplayer::competition_identity_for_section(
                &header,
                env!("CARGO_PKG_VERSION"),
                limits,
                None
            )
            .unwrap(),
            expected
        );
        assert_eq!(
            crate::native_judge::prepare_policy_competition_identity(
                &source,
                &judge,
                &policy,
                domain,
                Timestamp::ZERO,
                71,
                None
            )
            .unwrap(),
            expected
        );
        let full = crate::native_judge::prepare_policy_header(
            &source,
            &judge,
            &policy,
            domain,
            Timestamp::ZERO,
            71,
            Some(end),
        )
        .unwrap();
        let full_setup = crate::replay_playback::decode_section_setup(&full.options).unwrap();
        assert_eq!(full_setup.end, Some(end));
        assert_eq!(full_setup.profile, *policy.judge());
        assert_eq!(full_setup.gauge, *policy.gauge());
        assert_eq!(full_setup.judgments, None);
        assert_eq!(
            crate::native_judge::prepare_policy_competition_identity(
                &source,
                &judge,
                &policy,
                domain,
                Timestamp::ZERO,
                71,
                Some(end),
            )
            .unwrap(),
            expected_finite
        );
    }
}

#[test]
fn selected_solo_policy_mismatch_precedes_network_setup() {
    let source = source();
    let policy = policy(&source, beatkernel_bms::BmsGaugeKind::Hard);
    let wrong_profile = ResolvedPlayPolicy::builtin(7, 9, -2).unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        wrong_profile.judge().clone(),
    )
    .unwrap();
    let (options, _) = crate::competition_live::CompetitionOptions::extract(&[
        "--mp-host".into(),
        "127.0.0.1:34567".into(),
    ])
    .unwrap();
    let error = match crate::competition_live::LiveCompetition::prepare_native_section_with_policy(
        &options,
        &source,
        &judge,
        &policy,
        ClockDomainId(17),
        Timestamp::ZERO,
        71,
        None,
        0,
    ) {
        Err(error) => error,
        Ok(_) => panic!("mismatched selected profile must fail before networking"),
    };
    assert_eq!(
        error.to_string(),
        "policy-aware competition requires a pristine matching judge"
    );
    assert!(judge.effective_song_time().is_none());
}

#[test]
fn actual_selected_identities_refuse_class_gauge_profile_section_seed_or_end_mismatch_at_session_hello(
) {
    use crate::{
        multiplayer_protocol::{FrameDecoder, Session, WriteStep},
        multiplayer_start::{StartPolicy, StartRole},
    };
    let source = source();
    let hard = policy(&source, beatkernel_bms::BmsGaugeKind::Hard);
    let hazard = policy(&source, beatkernel_bms::BmsGaugeKind::Hazard);
    let make_class = |class, offset| {
        ResolvedPlayPolicy::bms(
            &source,
            beatkernel_bms::BmsGaugeKind::Hard,
            &[ClassifiedWindow {
                judgment: class,
                window: JudgeWindow {
                    grade: JudgeGrade(1),
                    early: Duration::from_nanos(7),
                    late: Duration::from_nanos(9),
                },
            }],
            offset,
        )
        .unwrap()
    };
    let different_class = make_class(beatkernel_bms::BmsJudgment::PGreat, -3);
    assert_eq!(hard.gauge(), different_class.gauge());
    let different_profile = make_class(beatkernel_bms::BmsJudgment::Great, -2);
    let identity = |policy: &ResolvedPlayPolicy, start: Timestamp, seed, end| {
        let selected = crate::section_start::source_at(&source, start).unwrap();
        let judge = JudgeEngine::new(
            selected.compile().unwrap().chart,
            selected.rules(),
            policy.judge().clone(),
        )
        .unwrap();
        crate::native_judge::prepare_policy_competition_identity(
            &source,
            &judge,
            policy,
            ClockDomainId(17),
            start,
            seed,
            end,
        )
        .unwrap()
    };
    let end = Some(Timestamp::from_nanos(2_500_000_000));
    let expected = identity(&hard, Timestamp::ZERO, 71, end);
    let mismatches = [
        identity(&different_class, Timestamp::ZERO, 71, end),
        identity(&hazard, Timestamp::ZERO, 71, end),
        identity(&different_profile, Timestamp::ZERO, 71, end),
        identity(&hard, Timestamp::from_nanos(1), 71, end),
        identity(&hard, Timestamp::ZERO, 72, end),
        identity(
            &hard,
            Timestamp::ZERO,
            71,
            Some(Timestamp::from_nanos(2_500_000_001)),
        ),
    ];
    for remote_identity in std::iter::once(expected.clone()).chain(mismatches) {
        let matching = remote_identity == expected;
        let mut local =
            Session::new(expected.clone(), StartRole::Host, StartPolicy::default(), 0).unwrap();
        let mut remote =
            Session::new(remote_identity, StartRole::Join, StartPolicy::default(), 0).unwrap();
        let frame = match remote.poll_write(0).unwrap() {
            WriteStep::Frame(frame) => frame,
            step => panic!("new session must offer its actual identity hello: {step:?}"),
        };
        let mut decoder = FrameDecoder::new();
        let mut consumed = 0;
        while consumed < frame.bytes.len() {
            let count = decoder.push(&frame.bytes[consumed..]).unwrap();
            assert!(count > 0 && count <= frame.bytes.len() - consumed);
            consumed += count;
        }
        assert_eq!(consumed, frame.bytes.len());
        let (tag, payload) = decoder.take().unwrap().unwrap();
        let result = local.receive(tag, &payload, 0);
        assert_eq!(result.is_ok(), matching);
        // This is the production portable protocol without credential, socket,
        // native or publication effects; a matching hello alone is not Ready.
        assert!(!matches!(
            local.poll_event(),
            Some(crate::multiplayer_protocol::MultiplayerEvent::Ready)
        ));
    }
}
