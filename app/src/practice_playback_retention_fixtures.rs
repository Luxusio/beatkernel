//! Retention guards from genuine mixer receipts. Supplied native pairs below
//! exercise the existing presentation port; they do not prove device timing.
use super::*;
use crate::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    native_audio_presentation::TargetNativeAudioSnapshot,
    native_gameplay_host::{NativeGameplayDiagnostic, PauseState},
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPair, Duration},
};
use beatkernel_platform::audio::presentation::validation::{
    NativePresentationValidator, OriginalNativePresentationEvidence,
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}

fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}

fn assert_held(
    actual: Option<(u64, InterpolatedTargetTime)>,
    expected: Option<(u64, InterpolatedTargetTime)>,
) {
    match expected {
        None => assert!(actual.is_none(), "unexpected held retention reference"),
        Some((source, target)) => {
            let (actual_source, actual_target) = actual.expect("missing held retention reference");
            assert_eq!(actual_source, source);
            assert!(
                actual_target.compare_interpolated_time(target).is_eq(),
                "held Target reference differs: actual {actual_target:?}, expected {target:?}"
            );
        }
    }
}

fn assert_queued(
    actual: (Option<u64>, Option<InterpolatedTargetTime>),
    expected: (Option<u64>, Option<InterpolatedTargetTime>),
) {
    assert_eq!(actual.0, expected.0);
    match expected.1 {
        None => assert!(actual.1.is_none(), "unexpected queued Target projection"),
        Some(target) => {
            let actual_target = actual.1.expect("missing queued Target projection");
            assert!(
                actual_target.compare_interpolated_time(target).is_eq(),
                "queued Target reference differs: actual {actual_target:?}, expected {target:?}"
            );
        }
    }
}

fn setup(repeat: bool) -> (PracticePlayback, Mixer) {
    let original = beatkernel_bms::parse(
        "#BPM 120\n#WAV01 key.wav\n#00011:01\n",
        beatkernel_bms::ParseOptions::default(),
    )
    .unwrap();
    let policy = crate::native_judge::NativeJudgeConfig {
        early: 1,
        late: 1,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(7),
        end: None,
    }
    .resolve_play_policy(
        &crate::play_policy::OriginalGaugeContext::from_source(&original),
        crate::play_policy::GaugeSelection::BeatKernel,
    )
    .unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(8, 128, 1).unwrap()).unwrap();
    let program =
        PreparedPracticeProgram::new(&bank, vec![], PracticeLimits::new(8, 1, 64, 8, 16).unwrap())
            .unwrap();
    let (controller, endpoint) = practice_queue(&program).unwrap();
    let (_, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 64, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let region =
        PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(4_000_000), repeat).unwrap();
    mixer.install_practice(program, endpoint, region).unwrap();
    let basis = mixer.output_frame_basis();
    let playback = PracticePlayback::new_with_completion(
        controller,
        original,
        vec![PracticeMember {
            player: PlayerId(1),
            policy,
            launch: SessionLaunch::new(vec!["--chart".into(), "chart.bms".into()]).unwrap(),
            chart_seed: 0,
            capture_limits: None,
        }],
        region,
        Timestamp::from_nanos(20_000_000),
        basis,
        1,
        Some(Timestamp::from_nanos(4_000_000)),
    )
    .unwrap();
    (playback, mixer)
}

fn presentation(source: OutputFrameBasis, target_rate: u32) -> NativeAudioPresentation {
    let epoch = AudioAuthorityEpoch {
        id: 7,
        stream_origin: source.origin(),
        logical_origin: point(3, 5_000_000_000),
        host_domain: ClockDomainId(1),
    };
    let mut presentation = NativeAudioPresentation::new(
        AudioAuthority::new(
            AudioAuthorityConfig {
                history_capacity: 8,
                max_observation_age: Duration::from_nanos(1_000_000_000),
                ..AudioAuthorityConfig::default()
            },
            epoch,
        )
        .unwrap(),
        NativePresentationValidator::new(7, epoch.stream_origin, epoch.host_domain),
    )
    .unwrap();
    let basis = TargetFrameBasis::new(
        source.origin(),
        TargetTime::from_frames(0, target_rate).unwrap(),
        target_rate,
    )
    .unwrap();
    for frame in [0, 35] {
        let raw = basis.point_at_stream_frame(frame).unwrap();
        presentation
            .admit_target(TargetNativeAudioSnapshot {
                epoch: 7,
                basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: raw,
                    target: point(1, raw.timestamp.as_nanos()),
                }),
            })
            .unwrap();
    }
    presentation.pin_practice_mixer_basis(source).unwrap();
    presentation
}

struct Host;
impl NativeGameplayHost for Host {
    fn cancelled(&self) -> bool {
        false
    }
    fn pause_requested(&self) -> bool {
        false
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, _: PauseState) {}
    fn publish_section_end(&mut self, _: Timestamp) {}
    fn publish_report(
        &mut self,
        _: &beatkernel::runtime::RuntimeReport,
    ) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn publish_local_reports(
        &mut self,
        _: &[crate::local_runtime::PlayerReport],
    ) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
}

fn prepare(
    owner: &mut PracticePlayback,
    presentation: &NativeAudioPresentation,
) -> (PreparedPracticeBoundary, PreparedPracticeCommit) {
    let now = point(1, 50_000_000);
    let boundary = owner.qualify(presentation, now).unwrap().unwrap();
    let prepared = owner.prepare_boundary(&boundary, &mut Host).unwrap();
    (boundary, prepared)
}

#[test]
fn empty_owner_has_no_pin_and_reading_it_does_not_consume_queued_receipts() {
    let (mut owner, mut mixer) = setup(true);
    for _ in 0..16 {
        assert_held(owner.history_retention_pins(), None);
        assert_queued(owner.queued_history_retention_pins(), (None, None));
    }
    assert_eq!(owner.peek_receipt().unwrap(), None);
    mixer.render(&mut [0.; 10]).unwrap();
    for _ in 0..16 {
        assert_held(owner.history_retention_pins(), None);
        assert_queued(
            owner.queued_history_retention_pins(),
            (
                Some(0),
                Some(exact(TargetTime::from_frames(0, 1000).unwrap())),
            ),
        );
    }
    let first = owner.peek_receipt().unwrap().unwrap();
    assert_eq!(first.kind, PracticeBoundaryKind::Started);
    assert_eq!(
        (first.physical_frame, first.generation, first.iteration),
        (0, 1, 0)
    );
    assert_held(
        owner.history_retention_pins(),
        Some((0, exact(TargetTime::from_frames(0, 1000).unwrap()))),
    );
    assert_queued(
        owner.queued_history_retention_pins(),
        (
            Some(4),
            Some(exact(TargetTime::from_frames(4, 1000).unwrap())),
        ),
    );
}

#[test]
fn first_real_receipt_remains_pinned_while_later_real_laps_are_queued() {
    let (mut owner, mut mixer) = setup(true);
    mixer.render(&mut [0.; 10]).unwrap();
    let first = owner.peek_receipt().unwrap().unwrap();
    let projected = owner.held_projected.unwrap();
    let before = (
        owner.generation,
        owner.iteration,
        owner.last_frames,
        owner.started,
        owner.ended,
        owner.applied_serial,
        owner.owner_id,
    );
    for _ in 0..128 {
        assert_held(
            owner.history_retention_pins(),
            Some((first.physical_frame, exact(projected.boundary.target_time))),
        );
        assert_eq!(owner.peek_receipt().unwrap(), Some(first));
        assert_eq!(owner.held_projected, Some(projected));
    }
    assert_eq!(
        (
            owner.generation,
            owner.iteration,
            owner.last_frames,
            owner.started,
            owner.ended,
            owner.applied_serial,
            owner.owner_id
        ),
        before
    );
    assert_eq!(mixer.frame_cursor(), 10);
}

#[test]
fn readonly_polling_preserves_actual_prepared_boundary_and_apply_releases_pin() {
    let (mut owner, mut mixer) = setup(false);
    let original_basis = mixer.output_frame_basis();
    mixer.render(&mut [0.; 6]).unwrap();
    assert_queued(
        owner.queued_history_retention_pins(),
        (
            Some(0),
            Some(exact(TargetTime::from_frames(0, 1000).unwrap())),
        ),
    );
    let presentation = presentation(original_basis, 1000);
    let (boundary, mut prepared) = prepare(&mut owner, &presentation);
    let original_receipt = prepared.receipt;
    let pins = owner.history_retention_pins().unwrap();
    for _ in 0..128 {
        assert_held(owner.history_retention_pins(), Some(pins));
        assert_queued(
            owner.queued_history_retention_pins(),
            (
                Some(4),
                Some(exact(TargetTime::from_frames(4, 1000).unwrap())),
            ),
        );
        assert_eq!(owner.peek_receipt().unwrap(), Some(original_receipt));
        assert_eq!(
            owner.qualify(&presentation, point(1, 50_000_000)).unwrap(),
            Some(boundary)
        );
    }
    let applied = owner.apply_boundary(&mut prepared).unwrap();
    assert_eq!(applied.receipt(), original_receipt);
    assert_eq!(
        owner.last_frames,
        Some((
            original_receipt.physical_frame,
            original_receipt.playback_frame
        ))
    );
    assert_held(owner.history_retention_pins(), None);
    assert_queued(
        owner.queued_history_retention_pins(),
        (
            Some(4),
            Some(exact(TargetTime::from_frames(4, 1000).unwrap())),
        ),
    );
    assert!(owner.apply_boundary(&mut prepared).is_err());
    assert_held(owner.history_retention_pins(), None);
    let end = owner.peek_receipt().unwrap().unwrap();
    assert_eq!(
        (end.kind, end.physical_frame, end.generation),
        (PracticeBoundaryKind::Ended, 4, 2)
    );
    assert_held(
        owner.history_retention_pins(),
        Some((4, exact(TargetTime::from_frames(4, 1000).unwrap()))),
    );
    assert_queued(owner.queued_history_retention_pins(), (None, None));
    let (_, mut end_prepared) = prepare(&mut owner, &presentation);
    owner.apply_boundary(&mut end_prepared).unwrap();
    assert!(owner.terminal_ready());
    assert_eq!(owner.last_frames, Some((4, 4)));
    assert_held(owner.history_retention_pins(), None);
    assert_eq!(owner.peek_receipt().unwrap(), None);
    assert_queued(owner.queued_history_retention_pins(), (None, None));
}

#[test]
fn actual_converter_raw_lookahead_keeps_source_without_fabricating_a_target_pin() {
    let (mut owner, mixer) = setup(false);
    let source = mixer.output_frame_basis();
    let mut converted = ConvertedMixer::new(
        mixer,
        AudioFormat::new(1500, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap();
    converted.render(&mut [0.; 6]).unwrap();
    assert!(converted.pulled_source_frame_cursor() > 4);
    assert_queued(
        owner.queued_history_retention_pins(),
        (
            Some(0),
            Some(exact(TargetTime::from_frames(0, 1500).unwrap())),
        ),
    );
    let presentation = presentation(source, 1500);
    let (_, mut started) = prepare(&mut owner, &presentation);
    assert_eq!(started.receipt.kind, PracticeBoundaryKind::Started);
    assert_queued(owner.queued_history_retention_pins(), (Some(4), None));
    owner.apply_boundary(&mut started).unwrap();
    let before = (
        converted.source_position(),
        converted.target_time(),
        converted.pulled_source_frame_cursor(),
    );
    for _ in 0..128 {
        assert_queued(owner.queued_history_retention_pins(), (Some(4), None));
        assert_held(owner.history_retention_pins(), None);
        assert_eq!(owner.peek_receipt().unwrap(), None);
    }
    assert_eq!(
        (
            converted.source_position(),
            converted.target_time(),
            converted.pulled_source_frame_cursor()
        ),
        before
    );
    converted.render_held(&mut [0.; 3]).unwrap();
    assert_queued(owner.queued_history_retention_pins(), (Some(4), None));
    assert_eq!(owner.peek_receipt().unwrap(), None);
    converted.render(&mut [0.; 1]).unwrap();
    let end_time = exact(TargetTime::from_frames(9, 1500).unwrap());
    for _ in 0..128 {
        assert_queued(
            owner.queued_history_retention_pins(),
            (Some(4), Some(end_time)),
        );
        assert_held(owner.history_retention_pins(), None);
    }
    let end = owner.peek_receipt().unwrap().unwrap();
    assert_eq!(
        (end.kind, end.physical_frame, end.generation),
        (PracticeBoundaryKind::Ended, 4, 2)
    );
    assert_queued(owner.queued_history_retention_pins(), (None, None));
    assert_held(owner.history_retention_pins(), Some((4, end_time)));
    let (_, mut ended) = prepare(&mut owner, &presentation);
    owner.apply_boundary(&mut ended).unwrap();
    assert_held(owner.history_retention_pins(), None);
    assert_queued(owner.queued_history_retention_pins(), (None, None));
    assert!(owner.terminal_ready());
}

#[test]
fn refused_commit_cannot_release_the_current_real_receipt() {
    let (mut owner, mut mixer) = setup(false);
    let (mut foreign, mut foreign_mixer) = setup(false);
    let original_basis = mixer.output_frame_basis();
    let foreign_basis = foreign_mixer.output_frame_basis();
    mixer.render(&mut [0.; 6]).unwrap();
    foreign_mixer.render(&mut [0.; 6]).unwrap();
    owner.peek_receipt().unwrap().unwrap();
    let presentation = presentation(original_basis, 1000);
    let foreign_presentation = self::presentation(foreign_basis, 1000);
    let (_, mut foreign_prepared) = prepare(&mut foreign, &foreign_presentation);
    let pins = owner.history_retention_pins().unwrap();
    let receipt = owner.peek_receipt().unwrap().unwrap();
    assert!(owner.apply_boundary(&mut foreign_prepared).is_err());
    assert_held(owner.history_retention_pins(), Some(pins));
    assert_eq!(owner.peek_receipt().unwrap(), Some(receipt));
    let (_, mut current) = prepare(&mut owner, &presentation);
    owner.apply_boundary(&mut current).unwrap();
    assert_held(owner.history_retention_pins(), None);
}

#[test]
fn converted_held_target_fraction_is_retained_exactly_through_real_start_and_end() {
    let (mut owner, mixer) = setup(false);
    let source = mixer.output_frame_basis();
    let mut converted = ConvertedMixer::new(
        mixer,
        AudioFormat::new(700, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap();
    converted.render_held(&mut [0.; 2]).unwrap();
    converted.render(&mut [0.; 10]).unwrap();
    let started = owner.peek_receipt().unwrap().unwrap();
    assert_eq!(started.physical_frame, 0);
    let start_time = TargetTime::from_frames(2, 700).unwrap();
    assert_ne!(
        (u128::from(start_time.numerator()) * 1_000_000_000) % u128::from(start_time.denominator()),
        0
    );
    assert_held(owner.history_retention_pins(), Some((0, exact(start_time))));
    assert_eq!(
        owner.held_projected.unwrap().boundary.target_time,
        start_time
    );
    let presentation = presentation(source, 700);
    let (_, mut prepared) = prepare(&mut owner, &presentation);
    for _ in 0..128 {
        assert_held(owner.history_retention_pins(), Some((0, exact(start_time))));
    }
    owner.apply_boundary(&mut prepared).unwrap();
    assert_held(owner.history_retention_pins(), None);
    let end = owner.peek_receipt().unwrap().unwrap();
    assert_eq!(
        (end.kind, end.physical_frame, end.playback_frame),
        (PracticeBoundaryKind::Ended, 4, 4)
    );
    let end_time = TargetTime::from_frames(5, 700).unwrap();
    assert_eq!(owner.held_projected.unwrap().boundary.target_time, end_time);
    assert_held(owner.history_retention_pins(), Some((4, exact(end_time))));
    let (_, mut prepared) = prepare(&mut owner, &presentation);
    owner.apply_boundary(&mut prepared).unwrap();
    assert_held(owner.history_retention_pins(), None);
    assert_eq!(owner.peek_receipt().unwrap(), None);
}
