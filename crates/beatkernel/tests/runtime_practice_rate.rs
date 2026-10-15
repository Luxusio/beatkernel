//! Public-controller oracles for actual retained runtime-rate PCM and evidence.
//!
//! Expected values come from original sample positions and exact frame times,
//! rather than the implementation's cursor or deadline helpers.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Duration, Timestamp},
    transport::Rate,
};

const PROGRAM: u64 = 0x14043;

fn ns(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn rate(numerator: i64, denominator: u64) -> Rate {
    Rate::new(numerator, denominator).unwrap()
}
fn region(start: i64, end: i64, repeat: bool) -> PracticeRegion {
    PracticeRegion::new(ns(start), ns(end), repeat).unwrap()
}
fn cue(voice: u64, sample: u64, at: i64) -> PracticeCue {
    PracticeCue {
        voice: VoiceId(voice),
        sample: SampleId(sample),
        at: ns(at),
        gain: 1.0,
    }
}
fn limits(overlap: usize) -> PracticeLimits {
    PracticeLimits::new(32, overlap, 128, 8, 32).unwrap()
}
fn ramp(length: usize, divisor: f32) -> Vec<f32> {
    (0..length).map(|i| i as f32 / divisor).collect()
}

#[allow(clippy::too_many_arguments)]
fn rig(
    output_hz: u32,
    assets: Vec<(u64, u32, Vec<f32>)>,
    cues: Vec<PracticeCue>,
    selected: Rate,
    preroll: i64,
    initial: PracticeRegion,
    bounds: PracticeLimits,
    open_end: bool,
    program_id: u64,
) -> (Mixer, CommandProducer, PracticeController) {
    let format = AudioFormat::new(output_hz, 1).unwrap();
    let pcm_limits = PcmLimits::new(4096, 8192, 4).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    for (id, hz, samples) in assets {
        bank.insert(
            SampleId(id),
            PcmSample::new(AudioFormat::new(hz, 1).unwrap(), samples, pcm_limits).unwrap(),
        )
        .unwrap();
    }
    let program = PreparedPracticeProgram::new_with_runtime_rates(
        &bank,
        cues,
        bounds,
        selected,
        Duration::from_nanos(preroll),
        open_end,
        program_id,
    )
    .unwrap();
    let (controller, endpoint) = practice_queue(&program).unwrap();
    let (producer, consumer) = command_queue(32).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(9),
            ns(0),
            AudioLimits::new(32, 16, 32, 256, 32).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.install_practice(program, endpoint, initial).unwrap();
    (mixer, producer, controller)
}
fn standard(
    selected: Rate,
    initial: PracticeRegion,
) -> (Mixer, CommandProducer, PracticeController) {
    rig(
        1000,
        vec![(1, 1000, ramp(8, 8.0))],
        vec![cue(1, 1, 0)],
        selected,
        0,
        initial,
        limits(1),
        false,
        PROGRAM,
    )
}
fn render(mixer: &mut Mixer, output: &mut [f32]) -> RenderReport {
    mixer.render(output).unwrap()
}
fn close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (actual - expected).abs() <= 0.0000002,
            "sample {index}: actual {actual}, expected {expected}"
        );
    }
}
fn started(controller: &mut PracticeController) -> PracticeReceipt {
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Boundary(boundary) => {
            assert_eq!(boundary.kind, PracticeBoundaryKind::Started);
            boundary
        }
        event => panic!("expected original Started, got {event:?}"),
    }
}
fn changed(controller: &mut PracticeController) -> PracticeRateEvent {
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Rate(event) => event,
        event => panic!("expected actual rate evidence, got {event:?}"),
    }
}
fn apply_rate(controller: &mut PracticeController, id: u64, selected: Rate) {
    let prepared = controller.prepare_rate(selected).unwrap();
    controller
        .try_set_rate_next(id, controller.generation(), &prepared)
        .unwrap();
}

#[test]
fn fractional_live_head_survives_actual_rate_cut_with_a_single_bgm_slot() {
    let (mut mixer, _producer, mut controller) = standard(rate(1, 2), region(0, 20_000_000, false));
    assert_eq!(controller.metadata().unwrap().revision, 0);
    let mut prefix = [99.0; 3];
    render(&mut mixer, &mut prefix);
    assert_eq!(prefix, [0.0, 0.0625, 0.125]);
    let start = started(&mut controller);
    let before = controller.metadata().unwrap();
    apply_rate(&mut controller, 1, rate(2, 1));
    assert_eq!(
        controller.metadata().unwrap(),
        before,
        "admission is not application"
    );
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    let mut suffix = [99.0; 4];
    let report = render(&mut mixer, &mut suffix);
    assert_eq!(suffix, [0.1875, 0.4375, 0.6875, 0.875]);
    let event = changed(&mut controller);
    assert_eq!(event.outcome, PracticeRateOutcome::Applied);
    assert_eq!(event.request_id, 1);
    assert_eq!(
        (event.generation, event.iteration),
        (start.generation, start.iteration)
    );
    assert_eq!((event.request_revision, event.applied_revision), (0, 1));
    assert_eq!((event.physical_frame, event.playback_frame), (3, 3));
    assert_eq!(event.active_elapsed_nanos, 3_000_000);
    assert_eq!(
        (
            event.previous_rate,
            event.requested_rate,
            event.applied_rate
        ),
        (rate(1, 2), rate(2, 1), rate(2, 1))
    );
    assert_eq!(
        controller.rate(),
        rate(1, 2),
        "legacy getter retains cold selection"
    );
    assert_eq!(controller.metadata().unwrap().rate, rate(2, 1));
    assert_eq!(controller.metadata().unwrap().revision, 1);
    assert_eq!(mixer.rate(), rate(2, 1));
    assert_eq!(report.counters.voice_full, 0);
    assert_eq!(
        report.counters.commands_applied, 0,
        "program/rate events do not mint producer credits"
    );
}

#[test]
fn normal_initial_selection_can_change_and_legacy_polls_preserve_the_typed_event_pin() {
    let (mut mixer, _producer, mut controller) =
        standard(Rate::NORMAL, region(0, 20_000_000, false));
    render(&mut mixer, &mut [0.0]);
    started(&mut controller);
    apply_rate(&mut controller, 1, rate(1, 2));
    let mut pcm = [99.0; 2];
    render(&mut mixer, &mut pcm);
    assert_eq!(pcm, [0.125, 0.1875]);
    let pins = controller.history_retention_pins();
    assert_eq!(pins.0, Some(1));
    assert!(pins.1.is_some());
    assert_eq!(
        controller.try_pop_receipt(),
        Err(PracticeError::EventRequiresOptIn)
    );
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::EventRequiresOptIn)
    );
    assert_eq!(
        controller.history_retention_pins(),
        pins,
        "legacy errors cannot release ownership"
    );
    let projected = controller.try_pop_projected_event().unwrap();
    assert_eq!((projected.target_frame, projected.target_rate), (1, 1000));
    assert_eq!(projected.boundary.source_frame, 1);
    assert_eq!(projected.origin.domain, ClockDomainId(9));
    assert!(
        matches!(projected.event, PracticeEvent::Rate(event) if event.outcome == PracticeRateOutcome::Applied)
    );
    assert_eq!(controller.history_retention_pins(), (None, None));
}

#[test]
fn normalized_noop_keeps_nondivisor_fraction_and_pcm_bitwise_equal() {
    fn setup() -> (Mixer, CommandProducer, PracticeController) {
        rig(
            44_100,
            vec![(1, 22_050, ramp(64, 64.0))],
            vec![cue(1, 1, 0)],
            rate(2, 3),
            3,
            region(0, 10_000_000, false),
            limits(1),
            false,
            PROGRAM,
        )
    }
    let (mut changed_mixer, _producer, mut controller) = setup();
    let (mut reference, _reference_producer, mut reference_controller) = setup();
    let mut prefix = [0.0; 5];
    render(&mut changed_mixer, &mut prefix);
    render(&mut reference, &mut [0.0; 5]);
    let initial = started(&mut controller);
    started(&mut reference_controller);
    apply_rate(&mut controller, 1, rate(6, 9));
    let mut actual = [99.0; 15];
    let mut expected = [99.0; 15];
    render(&mut changed_mixer, &mut actual);
    render(&mut reference, &mut expected);
    assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits));
    let event = changed(&mut controller);
    assert_eq!(event.outcome, PracticeRateOutcome::Noop);
    assert_eq!((event.request_revision, event.applied_revision), (0, 1));
    assert_eq!(
        (event.generation, event.iteration),
        (initial.generation, initial.iteration)
    );
    assert_eq!(
        event.active_elapsed_nanos, 113_378,
        "one cumulative floor of 5/44100 seconds"
    );
    assert_eq!(controller.metadata().unwrap().rate, rate(2, 3));
}

#[test]
fn live_keys_and_keys_queued_before_the_cut_and_same_frame_bgm_all_use_new_rate() {
    let (mut mixer, mut producer, mut controller) = rig(
        1000,
        vec![(1, 1000, ramp(16, 128.0)), (2, 1000, ramp(16, 64.0))],
        vec![cue(1, 1, 0), cue(2, 1, 1_500_000)],
        rate(1, 2),
        0,
        region(0, 20_000_000, false),
        limits(2),
        false,
        PROGRAM,
    );
    for (voice, at) in [(100, 0), (101, 3_000_000)] {
        producer
            .try_push_scoped(
                CommandScope(1),
                AudioCommand::Play {
                    voice: VoiceId(voice),
                    sample: SampleId(2),
                    at: ns(at),
                    gain: 1.0,
                },
            )
            .unwrap();
    }
    let before = render(&mut mixer, &mut [0.0; 3]);
    assert_eq!(
        (
            before.counters.commands_consumed,
            before.counters.commands_applied
        ),
        (2, 1)
    );
    started(&mut controller);
    apply_rate(&mut controller, 1, rate(2, 1));
    let mut actual = [99.0; 3];
    let after = render(&mut mixer, &mut actual);
    assert_eq!(actual, [4.5 / 128.0, 16.5 / 128.0, 28.5 / 128.0]);
    assert_eq!(
        changed(&mut controller).outcome,
        PracticeRateOutcome::Applied
    );
    assert_eq!(
        (
            after.counters.commands_consumed,
            after.counters.commands_applied
        ),
        (2, 2)
    );
    assert_eq!(after.counters.voice_full, 0);
}

#[test]
fn paused_pending_rate_waits_for_resume_and_excludes_silent_frames_from_elapsed() {
    let (mut mixer, mut producer, mut controller) =
        standard(Rate::NORMAL, region(0, 20_000_000, false));
    render(&mut mixer, &mut [0.0]);
    started(&mut controller);
    apply_rate(&mut controller, 1, rate(2, 1));
    producer.request_pause(true);
    let mut silence = [99.0; 5];
    let paused = render(&mut mixer, &mut silence);
    assert_eq!(silence, [0.0; 5]);
    assert_eq!(paused.playback_frames, 0);
    assert_eq!(controller.metadata().unwrap().revision, 0);
    assert_eq!(controller.metadata().unwrap().rate, Rate::NORMAL);
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    producer.request_pause(false);
    let mut pcm = [99.0; 3];
    render(&mut mixer, &mut pcm);
    assert_eq!(pcm, [0.125, 0.375, 0.625]);
    let event = changed(&mut controller);
    assert_eq!(
        (
            event.physical_frame,
            event.playback_frame,
            event.active_elapsed_nanos
        ),
        (6, 1, 1_000_000)
    );
    assert_eq!(event.outcome, PracticeRateOutcome::Applied);
}

#[test]
fn portable_preparation_roundtrips_but_malformed_or_wrong_geometry_cannot_bind() {
    let (mut mixer, _producer, mut controller) =
        standard(Rate::NORMAL, region(0, 20_000_000, false));
    render(&mut mixer, &mut [0.0]);
    started(&mut controller);
    let geometry = [PracticeRateCueGeometry {
        at: ns(0),
        source_frames: 8,
        source_hz: 1000,
    }];
    let prepared = prepare_practice_rate(PROGRAM, 1000, &geometry, rate(3, 2)).unwrap();
    let words = prepared.to_words();
    assert_eq!(words.len(), PRACTICE_RATE_PREPARATION_WORDS);
    let decoded = PracticeRatePreparation::from_words(words).unwrap();
    assert_eq!(decoded.to_words(), words);
    let bound = controller.bind_rate_preparation(1, 0, decoded).unwrap();
    for (program, hz, frames) in [
        (PROGRAM + 1, 1000, 8),
        (PROGRAM, 2000, 8),
        (PROGRAM, 1000, 9),
    ] {
        let wrong_geometry = [PracticeRateCueGeometry {
            at: ns(0),
            source_frames: frames,
            source_hz: 1000,
        }];
        let wrong = prepare_practice_rate(program, hz, &wrong_geometry, rate(3, 2)).unwrap();
        assert!(controller.bind_rate_preparation(1, 0, wrong).is_err());
    }
    assert!(PracticeRatePreparation::from_words([0; PRACTICE_RATE_PREPARATION_WORDS]).is_err());
    assert!(
        PracticeRatePreparation::from_words([u64::MAX; PRACTICE_RATE_PREPARATION_WORDS]).is_err()
    );
    let mut wrong_version = words;
    wrong_version[0] = 0;
    assert!(PracticeRatePreparation::from_words(wrong_version).is_err());
    assert!(controller.bind_rate_preparation(2, 0, decoded).is_err());
    assert!(controller.bind_rate_preparation(1, 1, decoded).is_err());
    assert_eq!(controller.metadata().unwrap().revision, 0);
    controller.try_set_rate_next(1, 1, &bound).unwrap();
    render(&mut mixer, &mut [0.0]);
    assert_eq!(
        changed(&mut controller).outcome,
        PracticeRateOutcome::Applied
    );
}

#[test]
fn prepared_owner_generation_revision_and_request_id_are_not_interchangeable() {
    let (mut mixer, _producer, mut controller) =
        standard(Rate::NORMAL, region(0, 20_000_000, false));
    let (mut other, _other_producer, mut other_controller) =
        standard(Rate::NORMAL, region(0, 20_000_000, false));
    render(&mut mixer, &mut [0.0]);
    render(&mut other, &mut [0.0]);
    started(&mut controller);
    started(&mut other_controller);
    let wrong_owner = other_controller.prepare_rate(rate(2, 1)).unwrap();
    assert!(controller.try_set_rate_next(1, 1, &wrong_owner).is_err());
    let prepared = controller.prepare_rate(rate(2, 1)).unwrap();
    assert_eq!(
        controller.try_set_rate_next(1, 2, &prepared),
        Err(PracticeError::StaleGeneration)
    );
    controller.try_set_rate_next(1, 1, &prepared).unwrap();
    assert!(
        controller.try_set_rate_next(2, 1, &prepared).is_err(),
        "one control is outstanding"
    );
    render(&mut mixer, &mut [0.0]);
    changed(&mut controller);
    let after = controller.metadata().unwrap();
    assert!(
        controller.try_set_rate_next(2, 1, &prepared).is_err(),
        "old revision cannot be reused"
    );
    let fresh = controller.prepare_rate(rate(1, 2)).unwrap();
    assert!(
        controller.try_set_rate_next(1, 1, &fresh).is_err(),
        "IDs must increase"
    );
    assert_eq!(controller.metadata().unwrap(), after);
    controller.try_set_rate_next(2, 1, &fresh).unwrap();
    render(&mut mixer, &mut [0.0]);
    assert_eq!(changed(&mut controller).applied_revision, 2);
}

#[test]
fn full_unread_receipt_refuses_admission_without_consuming_request_id_or_evidence() {
    let (mut mixer, _producer, mut controller) = rig(
        1000,
        vec![(1, 1000, ramp(8, 8.0))],
        vec![cue(1, 1, 0)],
        Rate::NORMAL,
        0,
        region(0, 20_000_000, false),
        PracticeLimits::new(8, 1, 128, 8, 1).unwrap(),
        false,
        PROGRAM,
    );
    render(&mut mixer, &mut [0.0]);
    let prepared = controller.prepare_rate(rate(2, 1)).unwrap();
    let before = controller.metadata().unwrap();
    let pins = controller.history_retention_pins();
    assert_eq!(
        controller.try_set_rate_next(1, 1, &prepared),
        Err(PracticeError::Full)
    );
    assert_eq!(controller.metadata().unwrap(), before);
    assert_eq!(controller.history_retention_pins(), pins);
    started(&mut controller);
    controller.try_set_rate_next(1, 1, &prepared).unwrap();
    let mut pcm = [99.0];
    render(&mut mixer, &mut pcm);
    assert_eq!(pcm, [0.125]);
    assert_eq!(changed(&mut controller).request_id, 1);
}

#[test]
fn whole_callback_event_budget_failure_keeps_pcm_control_heads_and_metadata_untouched() {
    let (mut mixer, mut producer, mut controller) = rig(
        1000,
        vec![(1, 1000, Vec::new()), (2, 1000, ramp(8, 8.0))],
        vec![cue(1, 1, 1_000_000), cue(2, 1, 3_000_000)],
        Rate::NORMAL,
        0,
        region(0, 20_000_000, false),
        PracticeLimits::new(8, 0, 2, 8, 8).unwrap(),
        false,
        PROGRAM,
    );
    render(&mut mixer, &mut [0.0]);
    started(&mut controller);
    producer
        .try_push_scoped(
            CommandScope(1),
            AudioCommand::Play {
                voice: VoiceId(100),
                sample: SampleId(2),
                at: ns(1_000_000),
                gain: 1.0,
            },
        )
        .unwrap();
    apply_rate(&mut controller, 1, rate(2, 1));
    let before = mixer.render(&mut []).unwrap();
    let metadata = controller.metadata().unwrap();
    let mut pcm = [99.0; 2];
    assert_eq!(mixer.render(&mut pcm), Err(AudioError::PracticeEventBudget));
    assert_eq!(pcm, [99.0; 2]);
    assert_eq!(mixer.render(&mut []).unwrap(), before);
    assert_eq!(controller.metadata().unwrap(), metadata);
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    assert_eq!(
        mixer.render(&mut pcm),
        Err(AudioError::PracticeEventBudget),
        "failed block retains FIFO request"
    );
    assert_eq!(mixer.render(&mut []).unwrap(), before);
    // The control and first empty cue fit exactly in one frame. The second
    // cue belongs to frame2, so only the oversized block exceeds the limit.
    let mut retry = [99.0];
    let report = render(&mut mixer, &mut retry);
    assert_eq!(retry, [0.0]);
    assert_eq!(report.counters.commands_consumed, 1);
    assert_eq!(report.counters.commands_applied, 1);
    let event = changed(&mut controller);
    assert_eq!(event.outcome, PracticeRateOutcome::Applied);
    assert_eq!(
        (event.request_id, event.generation, event.iteration),
        (1, 1, 0)
    );
    assert_eq!((event.request_revision, event.applied_revision), (0, 1));
    assert_eq!((event.physical_frame, event.playback_frame), (1, 1));
    assert_eq!(event.active_elapsed_nanos, 1_000_000);
    assert_eq!(controller.history_retention_pins(), (None, None));
    let mut suffix = [99.0; 2];
    render(&mut mixer, &mut suffix);
    assert_eq!(
        suffix,
        [0.25, 0.5],
        "queued key advances from its original head"
    );
    assert_eq!(controller.metadata().unwrap().revision, 1);
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
}

#[test]
fn minimum_budget_refuses_due_cue_rate_noop_and_restart_before_owning_a_control() {
    for nonempty in [false, true] {
        let samples = if nonempty {
            vec![0.125, 0.25, 0.375, 0.5]
        } else {
            Vec::new()
        };
        let (mut mixer, _producer, mut controller) = rig(
            1000,
            vec![(1, 1000, samples)],
            vec![cue(1, 1, 1_000_000)],
            Rate::NORMAL,
            0,
            region(0, 20_000_000, false),
            PracticeLimits::new(8, usize::from(nonempty), 1, 8, 8).unwrap(),
            false,
            PROGRAM,
        );
        let mut first = [99.0];
        render(&mut mixer, &mut first);
        assert_eq!(first, [0.0]);
        started(&mut controller);
        let before = mixer.render(&mut []).unwrap();
        let metadata = controller.metadata().unwrap();
        for requested in [rate(2, 1), rate(2, 2)] {
            let prepared = controller.prepare_rate(requested).unwrap();
            assert_eq!(
                controller.try_set_rate_next(1, 1, &prepared),
                Err(PracticeError::EventBudget)
            );
            assert_eq!(
                controller.try_restart_next(1, 1, region(8_000_000, 20_000_000, false), &prepared),
                Err(PracticeError::EventBudget)
            );
            assert_eq!(controller.metadata().unwrap(), metadata);
            assert_eq!(mixer.render(&mut []).unwrap(), before);
            assert_eq!(controller.history_retention_pins(), (None, None));
            assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
        }
        let mut due = [99.0];
        let report = render(&mut mixer, &mut due);
        assert_eq!(due, [if nonempty { 0.125 } else { 0.0 }]);
        assert_eq!(
            (report.playback_start_frame, report.playback_frames),
            (1, 1)
        );
        assert_eq!(report.counters.voice_full, 0);
        assert_eq!(mixer.rate(), Rate::NORMAL);
        assert_eq!(controller.metadata().unwrap(), metadata);
        assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
        // Reusing id1 proves neither request identity nor pending ownership
        // was consumed. This region has no cue or carried sample at its start.
        controller
            .try_request_next(1, 1, region(8_000_000, 20_000_000, false))
            .unwrap();
        let mut following = [99.0];
        render(&mut mixer, &mut following);
        assert_eq!(following, [0.0]);
        let projected = controller.try_pop_projected_event().unwrap();
        assert_eq!((projected.target_frame, projected.target_rate), (2, 1000));
        match projected.event {
            PracticeEvent::Boundary(boundary) => {
                assert_eq!(boundary.kind, PracticeBoundaryKind::Requested);
                assert_eq!((boundary.request_id, boundary.generation), (1, 2));
                assert_eq!((boundary.physical_frame, boundary.playback_frame), (2, 2));
                assert_eq!(boundary.applied_song_time, ns(8_000_000));
            }
            event => panic!("expected fresh request after cold refusal, got {event:?}"),
        }
        let metadata = controller.metadata().unwrap();
        assert_eq!(
            (metadata.generation, metadata.revision, metadata.rate),
            (2, 1, Rate::NORMAL)
        );
        assert_eq!(controller.history_retention_pins(), (None, None));
        assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    }
}

#[test]
fn exact_two_event_budget_admits_due_empty_and_nonempty_cues_at_the_original_cut() {
    for nonempty in [false, true] {
        for requested in [rate(2, 1), rate(2, 2)] {
            let samples = if nonempty {
                vec![0.125, 0.25, 0.375, 0.5]
            } else {
                Vec::new()
            };
            let (mut mixer, _producer, mut controller) = rig(
                1000,
                vec![(1, 1000, samples)],
                vec![cue(1, 1, 1_000_000)],
                Rate::NORMAL,
                0,
                region(0, 20_000_000, false),
                PracticeLimits::new(8, usize::from(nonempty), 2, 8, 8).unwrap(),
                false,
                PROGRAM,
            );
            render(&mut mixer, &mut [0.0]);
            let start = started(&mut controller);
            let metadata = controller.metadata().unwrap();
            apply_rate(&mut controller, 1, requested);
            assert_eq!(controller.metadata().unwrap(), metadata);
            let mut cut = [99.0];
            let report = render(&mut mixer, &mut cut);
            assert_eq!(cut, [if nonempty { 0.125 } else { 0.0 }]);
            assert_eq!(report.counters.commands_applied, 0);
            assert_eq!(report.counters.voice_full, 0);
            assert_eq!(controller.history_retention_pins().0, Some(1));
            let event = changed(&mut controller);
            assert_eq!(
                event.outcome,
                if requested == Rate::NORMAL {
                    PracticeRateOutcome::Noop
                } else {
                    PracticeRateOutcome::Applied
                }
            );
            assert_eq!(
                (event.request_id, event.generation, event.iteration),
                (1, start.generation, start.iteration)
            );
            assert_eq!((event.request_revision, event.applied_revision), (0, 1));
            assert_eq!((event.physical_frame, event.playback_frame), (1, 1));
            assert_eq!(event.active_elapsed_nanos, 1_000_000);
            assert_eq!(
                (
                    event.previous_rate,
                    event.requested_rate,
                    event.applied_rate
                ),
                (Rate::NORMAL, requested, requested)
            );
            let mut suffix = [99.0];
            render(&mut mixer, &mut suffix);
            assert_eq!(
                suffix,
                [if !nonempty {
                    0.0
                } else if requested == Rate::NORMAL {
                    0.25
                } else {
                    0.375
                }]
            );
            let metadata = controller.metadata().unwrap();
            assert_eq!(
                (metadata.generation, metadata.revision, metadata.rate),
                (1, 1, requested)
            );
            assert_eq!(controller.history_retention_pins(), (None, None));
            assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
        }
    }
}

#[test]
fn cold_admission_reserves_the_current_cue_burst_even_for_a_slower_requested_rate() {
    let (mut mixer, _producer, mut controller) = rig(
        1000,
        vec![(1, 1000, Vec::new())],
        vec![cue(1, 1, 1_000_000), cue(2, 1, 2_000_000)],
        rate(2, 1),
        0,
        region(0, 10_000_000, false),
        PracticeLimits::new(8, 0, 2, 8, 8).unwrap(),
        false,
        PROGRAM,
    );
    render(&mut mixer, &mut [0.0]);
    started(&mut controller);
    let before = mixer.render(&mut []).unwrap();
    let metadata = controller.metadata().unwrap();
    let slower = controller.prepare_rate(rate(1, 2)).unwrap();
    assert_eq!(
        controller.try_set_rate_next(1, 1, &slower),
        Err(PracticeError::EventBudget)
    );
    assert_eq!(controller.metadata().unwrap(), metadata);
    assert_eq!(mixer.render(&mut []).unwrap(), before);
    assert_eq!(controller.history_retention_pins(), (None, None));
    let mut due = [99.0];
    render(&mut mixer, &mut due);
    assert_eq!(
        due,
        [0.0],
        "the two original due cues still fit without a control"
    );
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    controller
        .try_request_next(1, 1, region(5_000_000, 10_000_000, false))
        .unwrap();
    render(&mut mixer, &mut due);
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Boundary(boundary) => {
            assert_eq!(boundary.kind, PracticeBoundaryKind::Requested);
            assert_eq!((boundary.request_id, boundary.generation), (1, 2));
            assert_eq!((boundary.physical_frame, boundary.playback_frame), (2, 2));
            assert_eq!(boundary.applied_song_time, ns(5_000_000));
        }
        event => panic!("expected usable control after burst refusal, got {event:?}"),
    }
    assert_eq!(controller.metadata().unwrap().rate, rate(2, 1));
    assert_eq!(controller.metadata().unwrap().revision, 1);
}

#[test]
fn minimum_budget_preserves_a_finite_end_and_allows_event_only_controls_afterward() {
    let original = region(5_000_000, 6_000_000, false);
    let (mut mixer, _producer, mut controller) = rig(
        1000,
        vec![(1, 1000, vec![0.125, 0.25, 0.375, 0.5])],
        vec![cue(1, 1, 0)],
        Rate::NORMAL,
        0,
        original,
        PracticeLimits::new(8, 1, 1, 8, 8).unwrap(),
        false,
        PROGRAM,
    );
    let mut silence = [99.0];
    render(&mut mixer, &mut silence);
    assert_eq!(silence, [0.0]);
    started(&mut controller);
    let metadata = controller.metadata().unwrap();
    let prepared = controller.prepare_rate(rate(2, 1)).unwrap();
    assert_eq!(
        controller.try_set_rate_next(1, 1, &prepared),
        Err(PracticeError::EventBudget)
    );
    assert_eq!(
        controller.try_restart_next(1, 1, original, &prepared),
        Err(PracticeError::EventBudget)
    );
    assert_eq!(controller.metadata().unwrap(), metadata);
    assert_eq!(controller.history_retention_pins(), (None, None));
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    render(&mut mixer, &mut silence);
    assert_eq!(silence, [0.0]);
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Boundary(end) => {
            assert_eq!(end.kind, PracticeBoundaryKind::Ended);
            assert_eq!((end.generation, end.iteration), (2, 1));
            assert_eq!((end.physical_frame, end.playback_frame), (1, 1));
            assert_eq!(end.applied_song_time, original.end);
        }
        event => panic!("expected original finite end, got {event:?}"),
    }
    assert!(mixer.practice_finished());
    let ended = controller.metadata().unwrap();
    assert!(!ended.active);
    assert_eq!(
        (ended.generation, ended.revision, ended.rate),
        (2, 0, Rate::NORMAL)
    );
    // The end already owns its real boundary. A later refusal needs only its
    // single event, so a minimum budget must not disable all runtime controls.
    let prepared = controller.prepare_rate(rate(2, 1)).unwrap();
    controller.try_set_rate_next(1, 2, &prepared).unwrap();
    render(&mut mixer, &mut silence);
    assert_eq!(silence, [0.0]);
    let event = changed(&mut controller);
    assert_eq!(
        event.outcome,
        PracticeRateOutcome::Refused(PracticeRateRefusal::Ended)
    );
    assert_eq!((event.request_id, event.generation), (1, 2));
    assert_eq!((event.request_revision, event.applied_revision), (0, 0));
    assert_eq!((event.physical_frame, event.playback_frame), (2, 2));
    assert_eq!(
        (
            event.previous_rate,
            event.requested_rate,
            event.applied_rate
        ),
        (Rate::NORMAL, rate(2, 1), Rate::NORMAL)
    );
    assert_eq!(controller.metadata().unwrap(), ended);
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    let restore = controller.prepare_rate(Rate::NORMAL).unwrap();
    // A fresh target with a live anchor voice needs event1+reseed1. Refusing
    // it must preserve the ended attempt and leave id2 available.
    assert_eq!(
        controller.try_restart_next(2, 2, region(0, 4_000_000, false), &restore),
        Err(PracticeError::EventBudget)
    );
    assert_eq!(controller.metadata().unwrap(), ended);
    assert_eq!(controller.history_retention_pins(), (None, None));
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    controller
        .try_restart_next(2, 2, original, &restore)
        .unwrap();
    render(&mut mixer, &mut silence);
    assert_eq!(silence, [0.0]);
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Restart(restart) => {
            assert_eq!(restart.outcome, PracticeRateOutcome::Applied);
            assert_eq!(
                (restart.boundary.request_id, restart.boundary.generation),
                (2, 3)
            );
            assert_eq!(
                (
                    restart.boundary.physical_frame,
                    restart.boundary.playback_frame
                ),
                (3, 3)
            );
            assert_eq!(restart.boundary.applied_song_time, original.start);
            assert_eq!((restart.request_revision, restart.applied_revision), (0, 1));
            assert_eq!(
                (
                    restart.previous_rate,
                    restart.requested_rate,
                    restart.applied_rate
                ),
                (Rate::NORMAL, Rate::NORMAL, Rate::NORMAL)
            );
        }
        event => panic!("expected exact empty-target restart, got {event:?}"),
    }
    let metadata = controller.metadata().unwrap();
    assert!(metadata.active);
    assert_eq!(
        (metadata.generation, metadata.revision, metadata.rate),
        (3, 1, Rate::NORMAL)
    );
    assert!(!mixer.practice_finished());
    assert_eq!(controller.history_retention_pins(), (None, None));
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
}

#[test]
fn exact_three_event_budget_preserves_rate_refusal_loop_and_anchor_at_one_cut() {
    let (mut mixer, _producer, mut controller) = rig(
        1000,
        vec![(1, 1000, vec![0.125, 0.25, 0.375, 0.5])],
        vec![cue(1, 1, 0)],
        Rate::NORMAL,
        0,
        region(0, 2_000_000, true),
        PracticeLimits::new(8, 1, 3, 8, 8).unwrap(),
        false,
        PROGRAM,
    );
    let mut prefix = [99.0; 2];
    render(&mut mixer, &mut prefix);
    assert_eq!(prefix, [0.125, 0.25]);
    started(&mut controller);
    apply_rate(&mut controller, 1, rate(2, 1));
    let mut cut = [99.0];
    let report = render(&mut mixer, &mut cut);
    assert_eq!(
        cut,
        [0.125],
        "the original loop reseeds at its exact boundary"
    );
    assert_eq!(report.counters.voice_full, 0);
    assert_eq!(report.counters.commands_applied, 0);
    let event = changed(&mut controller);
    assert_eq!(
        event.outcome,
        PracticeRateOutcome::Refused(PracticeRateRefusal::Ended)
    );
    assert_eq!(
        (event.request_id, event.generation, event.iteration),
        (1, 1, 0)
    );
    assert_eq!((event.request_revision, event.applied_revision), (0, 0));
    assert_eq!((event.physical_frame, event.playback_frame), (2, 2));
    assert_eq!(event.active_elapsed_nanos, 2_000_000);
    assert_eq!(
        (
            event.previous_rate,
            event.requested_rate,
            event.applied_rate
        ),
        (Rate::NORMAL, rate(2, 1), Rate::NORMAL)
    );
    assert_eq!(controller.history_retention_pins().0, Some(2));
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Boundary(boundary) => {
            assert_eq!(boundary.kind, PracticeBoundaryKind::Looped);
            assert_eq!(
                (boundary.request_id, boundary.generation, boundary.iteration),
                (0, 2, 1)
            );
            assert_eq!((boundary.physical_frame, boundary.playback_frame), (2, 2));
            assert_eq!(boundary.applied_song_time, ns(0));
        }
        event => panic!("expected original same-cut loop, got {event:?}"),
    }
    let metadata = controller.metadata().unwrap();
    assert_eq!(
        (metadata.generation, metadata.revision, metadata.rate),
        (2, 0, Rate::NORMAL)
    );
    assert_eq!(controller.history_retention_pins(), (None, None));
    render(&mut mixer, &mut cut);
    assert_eq!(cut, [0.25]);
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
}

#[test]
fn minimum_budget_empty_open_pass_admits_applied_noop_and_compound_restart_controls() {
    let original = region(0, i64::MAX, false);
    let (mut mixer, _producer, mut controller) = rig(
        1000,
        Vec::new(),
        Vec::new(),
        Rate::NORMAL,
        0,
        original,
        PracticeLimits::new(8, 0, 1, 8, 8).unwrap(),
        true,
        PROGRAM,
    );
    render(&mut mixer, &mut [0.0]);
    started(&mut controller);
    for (id, requested, outcome) in [
        (1, rate(2, 1), PracticeRateOutcome::Applied),
        (2, rate(4, 2), PracticeRateOutcome::Noop),
    ] {
        let before = controller.metadata().unwrap();
        apply_rate(&mut controller, id, requested);
        assert_eq!(controller.metadata().unwrap(), before);
        let mut silence = [99.0];
        let report = render(&mut mixer, &mut silence);
        assert_eq!(silence, [0.0]);
        assert_eq!(report.counters.commands_applied, 0);
        let event = changed(&mut controller);
        assert_eq!(event.outcome, outcome);
        assert_eq!(
            (event.request_id, event.generation, event.iteration),
            (id, 1, 0)
        );
        assert_eq!(
            (event.request_revision, event.applied_revision),
            (id - 1, id)
        );
        assert_eq!((event.physical_frame, event.playback_frame), (id, id));
        assert_eq!(event.active_elapsed_nanos, i128::from(id) * 1_000_000);
        assert_eq!(event.applied_rate, rate(2, 1));
        assert_eq!(controller.history_retention_pins(), (None, None));
        assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    }
    let restore = controller.prepare_rate(Rate::NORMAL).unwrap();
    controller
        .try_restart_next(3, 1, original, &restore)
        .unwrap();
    let mut silence = [99.0];
    render(&mut mixer, &mut silence);
    assert_eq!(silence, [0.0]);
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Restart(restart) => {
            assert_eq!(restart.outcome, PracticeRateOutcome::Applied);
            assert_eq!(
                (restart.boundary.request_id, restart.boundary.generation),
                (3, 2)
            );
            assert_eq!(
                (
                    restart.boundary.physical_frame,
                    restart.boundary.playback_frame
                ),
                (3, 3)
            );
            assert_eq!((restart.request_revision, restart.applied_revision), (2, 3));
            assert_eq!(
                (
                    restart.previous_rate,
                    restart.requested_rate,
                    restart.applied_rate
                ),
                (rate(2, 1), Rate::NORMAL, Rate::NORMAL)
            );
        }
        event => panic!("expected minimum-budget open restart, got {event:?}"),
    }
    let metadata = controller.metadata().unwrap();
    assert!(metadata.active);
    assert_eq!(
        (metadata.generation, metadata.revision, metadata.rate),
        (2, 3, Rate::NORMAL)
    );
    assert_eq!(controller.history_retention_pins(), (None, None));
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
}

#[test]
fn coalesced_future_cues_refuse_capacity_and_old_pcm_continues_without_drops() {
    let (mut mixer, _producer, mut controller) = rig(
        1000,
        vec![(1, 1000, vec![0.125])],
        vec![cue(1, 1, 0), cue(2, 1, 2_000_000), cue(3, 1, 4_000_000)],
        Rate::NORMAL,
        0,
        region(0, 20_000_000, false),
        limits(1),
        false,
        PROGRAM,
    );
    let mut first = [99.0];
    render(&mut mixer, &mut first);
    assert_eq!(first, [0.125]);
    started(&mut controller);
    apply_rate(&mut controller, 1, rate(4, 1));
    let mut suffix = [99.0; 5];
    let report = render(&mut mixer, &mut suffix);
    assert_eq!(suffix, [0.0, 0.125, 0.0, 0.125, 0.0]);
    let event = changed(&mut controller);
    assert_eq!(
        event.outcome,
        PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity)
    );
    assert_eq!(
        (
            event.previous_rate,
            event.requested_rate,
            event.applied_rate
        ),
        (Rate::NORMAL, rate(4, 1), Rate::NORMAL)
    );
    assert_eq!((event.request_revision, event.applied_revision), (0, 0));
    assert_eq!(controller.metadata().unwrap().revision, 0);
    assert_eq!(mixer.rate(), Rate::NORMAL);
    assert_eq!(report.counters.voice_full, 0);
}

#[test]
fn retiring_carried_voice_allows_exact_capacity_serial_future_cues() {
    let (mut mixer, _producer, mut controller) = rig(
        1000,
        vec![(1, 1000, vec![0.125, 0.25, 0.375, 0.5])],
        vec![cue(1, 1, 0), cue(2, 1, 4_000_000)],
        Rate::NORMAL,
        0,
        region(0, 20_000_000, false),
        limits(1),
        false,
        PROGRAM,
    );
    render(&mut mixer, &mut [0.0]);
    started(&mut controller);
    apply_rate(&mut controller, 1, rate(2, 1));
    let mut suffix = [99.0; 4];
    let report = render(&mut mixer, &mut suffix);
    assert_eq!(suffix, [0.25, 0.5, 0.125, 0.375]);
    assert_eq!(
        changed(&mut controller).outcome,
        PracticeRateOutcome::Applied
    );
    assert_eq!(report.counters.voice_full, 0);
}

#[test]
fn rate_after_last_cue_checks_future_repeat_capacity_independently_of_block_partition() {
    fn run(
        parts: &[usize],
        request_change: bool,
    ) -> (Vec<f32>, Vec<(PracticeEvent, u64, u32)>, PracticeMetadata) {
        let (mut mixer, _producer, mut controller) = rig(
            1000,
            vec![(1, 1000, vec![0.125])],
            vec![cue(1, 1, 1_000_000), cue(2, 1, 2_000_000)],
            Rate::NORMAL,
            0,
            region(0, 10_000_000, true),
            limits(1),
            false,
            PROGRAM,
        );
        let mut prefix = [99.0; 4];
        render(&mut mixer, &mut prefix);
        assert_eq!(prefix, [0.0, 0.125, 0.125, 0.0]);
        started(&mut controller);
        if request_change {
            apply_rate(&mut controller, 1, rate(2, 1));
        }
        let mut output = Vec::new();
        let mut events = Vec::new();
        for count in parts {
            let mut pcm = vec![99.0; *count];
            let report = render(&mut mixer, &mut pcm);
            assert_eq!(report.counters.voice_full, 0);
            output.extend(pcm);
            loop {
                match controller.try_pop_projected_event() {
                    Ok(projected) => events.push((
                        projected.event,
                        projected.target_frame,
                        projected.target_rate,
                    )),
                    Err(PracticeError::Empty) => break,
                    Err(error) => panic!("unexpected event failure: {error:?}"),
                }
            }
        }
        (output, events, controller.metadata().unwrap())
    }
    // At frame4 the current pass has no remaining cues or live voices.
    // Its next repeat still needs two slots at2x: both cues are due at the
    // first frame whose song phase reaches2ms. A suffix count of0 is not
    // a capacity proof for the repeating program.
    let whole = run(&[20], true);
    let partitioned = run(&[1, 4, 5, 10], true);
    let reference = run(&[20], false);
    assert_eq!(
        whole, partitioned,
        "the cut decision cannot depend on whether its block crosses a loop"
    );
    assert_eq!(whole.0, reference.0);
    let expected: Vec<f32> = (4..24)
        .map(|frame| {
            if matches!(frame % 10, 1 | 2) {
                0.125
            } else {
                0.0
            }
        })
        .collect();
    assert_eq!(
        whole.0, expected,
        "all later original cues survive each repeat"
    );
    let rates: Vec<PracticeRateEvent> = whole
        .1
        .iter()
        .filter_map(|(event, _, _)| match event {
            PracticeEvent::Rate(rate) => Some(*rate),
            _ => None,
        })
        .collect();
    assert_eq!(rates.len(), 1);
    let refused = rates[0];
    assert_eq!(
        refused.outcome,
        PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity)
    );
    assert_eq!(
        (
            refused.request_id,
            refused.physical_frame,
            refused.playback_frame
        ),
        (1, 4, 4)
    );
    assert_eq!(refused.active_elapsed_nanos, 4_000_000);
    assert_eq!(
        (
            refused.previous_rate,
            refused.requested_rate,
            refused.applied_rate
        ),
        (Rate::NORMAL, rate(2, 1), Rate::NORMAL)
    );
    assert_eq!((refused.request_revision, refused.applied_revision), (0, 0));
    let boundaries: Vec<_> = whole
        .1
        .iter()
        .copied()
        .filter(|(event, _, _)| matches!(event, PracticeEvent::Boundary(_)))
        .collect();
    assert_eq!(boundaries, reference.1);
    assert_eq!(
        whole.2, reference.2,
        "refusal preserves all semantic state across later loops"
    );
}

#[test]
fn rate_after_last_empty_cue_refuses_future_repeat_event_budget_without_wedging() {
    fn run(
        request_change: bool,
        probe_oversized_cut_block: bool,
    ) -> (Vec<f32>, Vec<(PracticeEvent, u64, u32)>, PracticeMetadata) {
        let (mut mixer, _producer, mut controller) = rig(
            1000,
            vec![(1, 1000, Vec::new())],
            vec![
                cue(1, 1, 1_000_000),
                cue(2, 1, 2_000_000),
                cue(3, 1, 3_000_000),
            ],
            Rate::NORMAL,
            0,
            region(0, 10_000_000, true),
            PracticeLimits::new(8, 0, 2, 8, 16).unwrap(),
            false,
            PROGRAM,
        );
        // Every original callback uses at most one event: Started at0,
        // then the three zero-frame cues separately at1,2 and3.
        for _ in 0..4 {
            let mut silence = [99.0];
            render(&mut mixer, &mut silence);
            assert_eq!(silence, [0.0]);
        }
        started(&mut controller);
        if request_change {
            apply_rate(&mut controller, 1, rate(3, 1));
        }
        if probe_oversized_cut_block {
            assert!(request_change);
            let before = mixer.render(&mut []).unwrap();
            let metadata = controller.metadata().unwrap();
            // Rate refusal, the next loop and its first empty cue exceed a
            // whole-block budget of2 even at the unchanged speed. The invalid
            // large callback must preserve its request for a valid small retry.
            let mut untouched = [99.0; 8];
            assert_eq!(
                mixer.render(&mut untouched),
                Err(AudioError::PracticeEventBudget)
            );
            assert_eq!(untouched, [99.0; 8]);
            assert_eq!(mixer.render(&mut []).unwrap(), before);
            assert_eq!(controller.metadata().unwrap(), metadata);
            assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
        }
        let mut output = Vec::new();
        let mut events = Vec::new();
        for _ in 4..24 {
            let mut silence = [99.0];
            let report = render(&mut mixer, &mut silence);
            assert_eq!(report.counters.voice_full, 0);
            output.extend(silence);
            loop {
                match controller.try_pop_projected_event() {
                    Ok(projected) => events.push((
                        projected.event,
                        projected.target_frame,
                        projected.target_rate,
                    )),
                    Err(PracticeError::Empty) => break,
                    Err(error) => panic!("unexpected event failure: {error:?}"),
                }
            }
        }
        (output, events, controller.metadata().unwrap())
    }
    // The current suffix is empty at frame4. At3x the next loop maps all three
    // zero-frame cues onto one frame, requiring three events despite needing
    // zero voice slots. Accepting the rate would wedge that future callback.
    let small_callbacks = run(true, false);
    let retried_after_large_block = run(true, true);
    let reference = run(false, false);
    assert_eq!(small_callbacks, retried_after_large_block);
    assert_eq!(small_callbacks.0, vec![0.0; 20]);
    assert_eq!(small_callbacks.0, reference.0);
    let rates: Vec<_> = small_callbacks
        .1
        .iter()
        .filter_map(|(event, _, _)| match event {
            PracticeEvent::Rate(rate) => Some(*rate),
            _ => None,
        })
        .collect();
    assert_eq!(rates.len(), 1);
    let refused = rates[0];
    assert_eq!(
        refused.outcome,
        PracticeRateOutcome::Refused(PracticeRateRefusal::EventBudget)
    );
    assert_eq!(
        (
            refused.request_id,
            refused.physical_frame,
            refused.playback_frame
        ),
        (1, 4, 4)
    );
    assert_eq!(refused.active_elapsed_nanos, 4_000_000);
    assert_eq!(
        (
            refused.previous_rate,
            refused.requested_rate,
            refused.applied_rate
        ),
        (Rate::NORMAL, rate(3, 1), Rate::NORMAL)
    );
    assert_eq!((refused.request_revision, refused.applied_revision), (0, 0));
    let boundaries: Vec<_> = small_callbacks
        .1
        .iter()
        .copied()
        .filter(|(event, _, _)| matches!(event, PracticeEvent::Boundary(_)))
        .collect();
    assert_eq!(
        boundaries, reference.1,
        "both later loops must complete at their original boundaries"
    );
    assert_eq!(small_callbacks.2, reference.2);
}

#[test]
fn only_last_live_voice_denominator_failure_cannot_change_earlier_heads() {
    let old_rate = rate(1, u64::MAX - 1);
    let requested = rate(1, (1_u64 << 63) + 3);
    let a = (1_u128 << 63) - 1;
    let b = (1_u128 << 63) + 3;
    // After one 1Hz output frame the first 2Hz key has phase1/A, the
    // second 1Hz key has phase1/(2A). Since gcd(A,B)=1, only the latter
    // needs a common denominator beyond signed i128 when changing to1/B.
    assert!(a * b <= i128::MAX as u128);
    assert!(2 * a * b > i128::MAX as u128);
    let setup = || {
        let (mixer, mut producer, controller) = rig(
            1,
            vec![(1, 2, ramp(8, 8.0)), (2, 1, ramp(8, 16.0))],
            vec![],
            old_rate,
            1_000_000_000,
            region(0, i64::MAX, false),
            limits(0),
            true,
            PROGRAM,
        );
        for (voice, sample) in [(100, 1), (101, 2)] {
            producer
                .try_push_scoped(
                    CommandScope(1),
                    AudioCommand::Play {
                        voice: VoiceId(voice),
                        sample: SampleId(sample),
                        at: ns(0),
                        gain: 1.0,
                    },
                )
                .unwrap();
        }
        (mixer, producer, controller)
    };
    let (mut mixer, _producer, mut controller) = setup();
    let (mut reference, _reference_producer, mut reference_controller) = setup();
    render(&mut mixer, &mut [0.0]);
    render(&mut reference, &mut [0.0]);
    started(&mut controller);
    started(&mut reference_controller);
    // At this cut E=P, so the song phase is exactly0 and its u64 fraction
    // capacity cannot mask the intended failure in the last live key.
    apply_rate(&mut controller, 1, requested);
    let mut suffix = [99.0; 2];
    let mut expected = [99.0; 2];
    let report = render(&mut mixer, &mut suffix);
    render(&mut reference, &mut expected);
    assert!(expected.iter().all(|sample| *sample > 0.0));
    assert_eq!(suffix.map(f32::to_bits), expected.map(f32::to_bits));
    assert!(
        suffix[1] > suffix[0],
        "unchanged tiny-rate heads still advance"
    );
    let event = changed(&mut controller);
    assert_eq!(
        event.outcome,
        PracticeRateOutcome::Refused(PracticeRateRefusal::VoiceDenominatorCapacity)
    );
    assert_eq!(event.active_elapsed_nanos, 1_000_000_000);
    assert_eq!(event.physical_frame, 1);
    assert_eq!(
        (
            event.previous_rate,
            event.requested_rate,
            event.applied_rate
        ),
        (old_rate, requested, old_rate)
    );
    assert_eq!(event.applied_revision, 0);
    assert_eq!(mixer.rate(), old_rate);
    assert_eq!(report.counters.commands_applied, 2);
}

#[test]
fn odd_preroll_nondivisor_grid_uses_new_phase_for_future_cue_and_finite_end() {
    let (mut mixer, _producer, mut controller) = rig(
        44_100,
        vec![(1, 22_050, ramp(64, 64.0)), (2, 22_050, vec![0.25; 64])],
        vec![cue(1, 1, 0), cue(2, 2, 80_000)],
        rate(2, 3),
        3,
        region(0, 140_000, false),
        limits(2),
        false,
        PROGRAM,
    );
    let mut prefix = [99.0; 3];
    render(&mut mixer, &mut prefix);
    close(&prefix, &[0.0, 0.0, 1.0 / 192.0]);
    started(&mut controller);
    apply_rate(&mut controller, 1, rate(1, 2));
    let mut actual = [99.0; 11];
    render(&mut mixer, &mut actual);
    let mut expected = [0.0; 11];
    // Ecut=floor(3e9/44100)=68027, Qcut=(68027-3)*2/3.
    // At frame6 Q=79362+5/6 <80000; frame7 reaches the cue.
    // At frame11 Q=136052+1/3 <140000; frame12 reaches the end.
    for (offset, value) in expected.iter_mut().enumerate().take(9) {
        let frame = offset + 3;
        *value = (2.0 / 3.0 + offset as f32 / 4.0) / 64.0;
        if frame >= 7 {
            *value += 0.25;
        }
    }
    close(&actual, &expected);
    let event = changed(&mut controller);
    assert_eq!(
        (event.physical_frame, event.active_elapsed_nanos),
        (3, 68_027)
    );
    assert_eq!(event.outcome, PracticeRateOutcome::Applied);
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Boundary(end) => {
            assert_eq!(end.kind, PracticeBoundaryKind::Ended);
            assert_eq!(end.physical_frame, 12);
        }
        event => panic!("expected finite end, got {event:?}"),
    }
    assert!(mixer.practice_finished());
}

#[test]
fn queued_change_at_finite_end_is_correlated_refusal_and_never_revives_playback() {
    let (mut mixer, _producer, mut controller) =
        standard(Rate::NORMAL, region(0, 2_000_000, false));
    render(&mut mixer, &mut [0.0; 2]);
    started(&mut controller);
    apply_rate(&mut controller, 1, rate(2, 1));
    let mut suffix = [99.0; 3];
    render(&mut mixer, &mut suffix);
    assert_eq!(suffix, [0.0; 3]);
    let event = changed(&mut controller);
    assert_eq!(
        event.outcome,
        PracticeRateOutcome::Refused(PracticeRateRefusal::Ended)
    );
    assert_eq!(
        (
            event.request_revision,
            event.applied_revision,
            event.physical_frame
        ),
        (0, 0, 2)
    );
    assert_eq!(controller.metadata().unwrap().rate, Rate::NORMAL);
    assert!(
        matches!(controller.try_pop_event().unwrap(), PracticeEvent::Boundary(end) if end.kind == PracticeBoundaryKind::Ended)
    );
    assert!(mixer.practice_finished());
}

#[test]
fn compound_original_restart_after_finite_end_restores_pcm_rate_and_active_attempt() {
    let original = region(0, 4_000_000, false);
    let selected = rate(1, 2);
    let (mut mixer, _producer, mut controller) = standard(selected, original);
    render(&mut mixer, &mut [0.0; 3]);
    started(&mut controller);
    apply_rate(&mut controller, 1, rate(2, 1));
    let mut tail = [99.0; 3];
    render(&mut mixer, &mut tail);
    assert_eq!(tail, [0.1875, 0.4375, 0.0]);
    assert_eq!(
        changed(&mut controller).outcome,
        PracticeRateOutcome::Applied
    );
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Boundary(end) => {
            assert_eq!(end.kind, PracticeBoundaryKind::Ended);
            assert_eq!(end.generation, 2, "terminal boundary advances generation");
        }
        event => panic!("expected finite end, got {event:?}"),
    }
    assert!(mixer.practice_finished());
    let ended = controller.metadata().unwrap();
    assert!(!ended.active);
    assert_eq!((ended.generation, ended.revision), (2, 1));
    let restore = controller.prepare_rate(selected).unwrap();
    controller
        .try_restart_next(2, 2, original, &restore)
        .unwrap();
    let mut restarted = [99.0; 3];
    render(&mut mixer, &mut restarted);
    assert_eq!(restarted, [0.0, 0.0625, 0.125]);
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Restart(event) => {
            assert_eq!(event.outcome, PracticeRateOutcome::Applied);
            assert_eq!(event.boundary.request_id, 2);
            assert_eq!(event.boundary.generation, 3);
            assert_eq!(event.boundary.applied_song_time, original.start);
            assert_eq!((event.request_revision, event.applied_revision), (1, 2));
            assert_eq!(
                (
                    event.previous_rate,
                    event.requested_rate,
                    event.applied_rate
                ),
                (rate(2, 1), selected, selected)
            );
        }
        event => panic!("expected one post-end restart, got {event:?}"),
    }
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    assert_eq!(mixer.rate(), selected);
    assert!(!mixer.practice_finished());
    let metadata = controller.metadata().unwrap();
    assert!(metadata.started && metadata.active);
    assert_eq!(
        (metadata.generation, metadata.revision, metadata.rate),
        (3, 2, selected)
    );
}

#[test]
fn scrub_and_autonomous_loops_use_current_rate_without_reintroducing_preroll() {
    let (mut mixer, _producer, mut controller) = standard(rate(1, 2), region(0, 4_000_000, true));
    render(&mut mixer, &mut [0.0]);
    started(&mut controller);
    apply_rate(&mut controller, 1, rate(2, 1));
    render(&mut mixer, &mut [0.0]);
    changed(&mut controller);
    controller
        .try_request_next(2, 1, region(2_000_000, 6_000_000, true))
        .unwrap();
    let mut first = [99.0];
    render(&mut mixer, &mut first);
    assert_eq!(first, [0.25]);
    assert!(
        matches!(controller.try_pop_event().unwrap(), PracticeEvent::Boundary(boundary) if boundary.kind == PracticeBoundaryKind::Requested && boundary.generation == 2)
    );
    let revision = controller.metadata().unwrap().revision;
    let mut following = [99.0; 3];
    render(&mut mixer, &mut following);
    assert_eq!(following, [0.5, 0.25, 0.5]);
    assert!(
        matches!(controller.try_pop_event().unwrap(), PracticeEvent::Boundary(boundary) if boundary.kind == PracticeBoundaryKind::Looped && boundary.generation == 3)
    );
    assert_eq!(controller.metadata().unwrap().rate, rate(2, 1));
    assert_eq!(
        controller.metadata().unwrap().revision,
        revision,
        "autonomous loop is not an explicit edit"
    );
    assert_eq!(controller.rate(), rate(1, 2));
}

#[test]
fn scrub_admission_uses_current_faster_rate_instead_of_initial_deadline_bound() {
    let initial_rate = rate(1, 1_000_000_000_000);
    // The initial1ns section occupies1000s of output and is representable.
    let (mut mixer, _producer, mut controller) = standard(initial_rate, region(0, 1, false));
    render(&mut mixer, &mut [0.0]);
    started(&mut controller);
    apply_rate(&mut controller, 1, Rate::NORMAL);
    render(&mut mixer, &mut [0.0]);
    assert_eq!(
        changed(&mut controller).outcome,
        PracticeRateOutcome::Applied
    );
    // This20,000s original section occupies20,000,000 frames at current1x.
    // Applying the obsolete initial rate would incorrectly demand2e19 frames.
    let larger = region(0, 20_000_000_000_000, false);
    controller.try_request_next(2, 1, larger).unwrap();
    let mut pcm = [99.0; 3];
    render(&mut mixer, &mut pcm);
    assert_eq!(pcm, [0.0, 0.125, 0.25]);
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Boundary(boundary) => {
            assert_eq!(boundary.kind, PracticeBoundaryKind::Requested);
            assert_eq!((boundary.request_id, boundary.generation), (2, 2));
            assert_eq!((boundary.physical_frame, boundary.playback_frame), (2, 2));
            assert_eq!(boundary.applied_song_time, larger.start);
        }
        event => panic!("expected current-rate region admission, got {event:?}"),
    }
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    let metadata = controller.metadata().unwrap();
    assert_eq!(
        (metadata.generation, metadata.revision, metadata.rate),
        (2, 2, Rate::NORMAL)
    );
    assert!(metadata.active);
    assert_eq!(controller.rate(), initial_rate);
    assert!(!mixer.practice_finished());
}

#[test]
fn scrub_unrepresentable_at_current_slower_rate_rejects_without_wedging_valid_retry() {
    let slow = rate(1, 1_000_000_000_000);
    let (mut mixer, _producer, mut controller) =
        standard(Rate::NORMAL, region(0, 2_000_000, false));
    render(&mut mixer, &mut [0.0]);
    started(&mut controller);
    apply_rate(&mut controller, 1, slow);
    let mut cut = [99.0];
    render(&mut mixer, &mut cut);
    assert_eq!(cut, [0.125]);
    assert_eq!(
        changed(&mut controller).outcome,
        PracticeRateOutcome::Applied
    );
    let before = mixer.render(&mut []).unwrap();
    let metadata = controller.metadata().unwrap();
    // The initial1x rate could play this region, but current1/1e12 cannot
    // represent its2e19-frame duration or its physical output deadline.
    assert!(controller
        .try_request_next(2, 1, region(0, 20_000_000_000_000, false))
        .is_err());
    assert_eq!(controller.metadata().unwrap(), metadata);
    assert_eq!(mixer.render(&mut []).unwrap(), before);
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    let mut continuing = [99.0];
    render(&mut mixer, &mut continuing);
    assert_eq!(
        continuing,
        [0.125],
        "rejected region leaves old live head running"
    );
    assert_eq!(controller.metadata().unwrap(), metadata);
    // Reuse the refused request id: neither FIFO ownership nor id credit
    // may have been consumed. A1ns section fits even at the current rate.
    controller
        .try_request_next(2, 1, region(0, 1, false))
        .unwrap();
    let mut restarted = [99.0];
    render(&mut mixer, &mut restarted);
    assert_eq!(restarted, [0.0]);
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Boundary(boundary) => {
            assert_eq!(boundary.kind, PracticeBoundaryKind::Requested);
            assert_eq!((boundary.request_id, boundary.generation), (2, 2));
            assert_eq!((boundary.physical_frame, boundary.playback_frame), (3, 3));
        }
        event => panic!("expected valid retry after refusal, got {event:?}"),
    }
    let after = controller.metadata().unwrap();
    assert_eq!((after.generation, after.revision, after.rate), (2, 2, slow));
    assert!(after.active);
}

#[test]
fn original_restart_is_one_compound_boundary_with_pinned_rate_region_and_zero_preroll() {
    let original = region(2_000_000, 6_000_000, false);
    let (mut mixer, _producer, mut controller) = rig(
        1000,
        vec![(1, 1000, ramp(8, 8.0))],
        vec![cue(1, 1, 0)],
        rate(1, 2),
        2_000_000,
        original,
        limits(1),
        false,
        PROGRAM,
    );
    let mut initial = [99.0; 3];
    render(&mut mixer, &mut initial);
    // S=2ms, P=2ms, r=1/2 gives initial song1ms. The cue at0 is
    // already playing there; preroll is original-song lead-in, not mute.
    assert_eq!(initial, [0.125, 0.1875, 0.25]);
    let start = started(&mut controller);
    assert_eq!(start.requested_song_time, ns(2_000_000));
    assert_eq!(start.applied_song_time, ns(1_000_000));
    assert_eq!(start.correction_nanos, -1_000_000);
    apply_rate(&mut controller, 1, rate(2, 1));
    let mut old = [99.0];
    render(&mut mixer, &mut old);
    assert_eq!(old, [0.3125]);
    changed(&mut controller);
    let restore = controller.prepare_rate(rate(1, 2)).unwrap();
    controller
        .try_restart_next(2, 1, original, &restore)
        .unwrap();
    let mut restarted = [99.0; 2];
    render(&mut mixer, &mut restarted);
    assert_eq!(restarted, [0.25, 0.3125]);
    assert_eq!(
        controller.try_pop_receipt(),
        Err(PracticeError::EventRequiresOptIn)
    );
    match controller.try_pop_event().unwrap() {
        PracticeEvent::Restart(event) => {
            assert_eq!(event.outcome, PracticeRateOutcome::Applied);
            assert_eq!(
                (event.boundary.request_id, event.boundary.generation),
                (2, 2)
            );
            assert_eq!(event.boundary.applied_song_time, original.start);
            assert_eq!((event.request_revision, event.applied_revision), (1, 2));
            assert_eq!(
                (
                    event.previous_rate,
                    event.requested_rate,
                    event.applied_rate
                ),
                (rate(2, 1), rate(1, 2), rate(1, 2))
            );
        }
        event => panic!("expected one compound restart, got {event:?}"),
    }
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    assert_eq!(mixer.rate(), rate(1, 2));
    assert_eq!(controller.metadata().unwrap().generation, 2);
    assert_eq!(
        controller.output_preroll(),
        Duration::from_nanos(2_000_000),
        "original setup stays immutable"
    );
}

#[test]
fn converted_raw_rate_lookahead_needs_actual_target_generation_and_held_output_cannot_ack() {
    let (mixer, _producer, mut controller) = standard(Rate::NORMAL, region(0, 20_000_000, false));
    let mut converted = ConvertedMixer::new(
        mixer,
        AudioFormat::new(5000, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        32,
    )
    .unwrap();
    converted.render(&mut [0.0]).unwrap();
    started(&mut controller);
    apply_rate(&mut controller, 1, rate(2, 1));
    let mut first = [99.0; 5];
    converted.render(&mut first).unwrap();
    close(&first, &[0.025, 0.05, 0.075, 0.1, 0.125]);
    assert_eq!(
        controller.try_pop_projected_event(),
        Err(PracticeError::Empty)
    );
    assert_eq!(controller.history_retention_pins(), (Some(2), None));
    let position = converted.source_position();
    let mut held = [99.0; 3];
    converted.render_held(&mut held).unwrap();
    assert_eq!(held, [0.0; 3]);
    assert_eq!(converted.source_position(), position);
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    let mut middle = [99.0; 4];
    converted.render(&mut middle).unwrap();
    close(&middle, &[0.15, 0.175, 0.2, 0.225]);
    assert_eq!(
        controller.try_pop_projected_event(),
        Err(PracticeError::Empty)
    );
    converted.render(&mut [0.0]).unwrap();
    let projected = controller.try_pop_projected_event().unwrap();
    assert_eq!(
        (projected.target_frame, projected.target_rate),
        (13, 5000),
        "held target frames retain physical output indexing"
    );
    assert_eq!(projected.boundary.source_frame, 2);
    assert!(
        matches!(projected.event, PracticeEvent::Rate(event) if event.physical_frame == 2 && event.outcome == PracticeRateOutcome::Applied)
    );
}

#[test]
fn converted_partitions_preserve_actual_cut_complete_pcm_and_finite_silent_suffix() {
    fn run(
        initial_parts: &[usize],
        parts: &[usize],
    ) -> (Vec<f32>, Vec<(PracticeEvent, u64, u32)>, SourcePosition) {
        let (mixer, _producer, mut controller) = standard(rate(1, 2), region(0, 8_000_000, false));
        let mut converted = ConvertedMixer::new(
            mixer,
            AudioFormat::new(1500, 1).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            32,
        )
        .unwrap();
        let mut output = Vec::new();
        for count in initial_parts {
            let mut pcm = vec![99.0; *count];
            converted.render(&mut pcm).unwrap();
            output.extend(pcm);
        }
        started(&mut controller);
        apply_rate(&mut controller, 1, rate(2, 1));
        let mut events = Vec::new();
        for count in parts {
            let mut pcm = vec![99.0; *count];
            converted.render(&mut pcm).unwrap();
            output.extend(pcm);
            while let Ok(projected) = controller.try_pop_projected_event() {
                events.push((
                    projected.event,
                    projected.target_frame,
                    projected.target_rate,
                ));
            }
        }
        (output, events, converted.source_position())
    }
    let whole = run(&[5], &[18]);
    let partitioned = run(&[1, 2, 2], &[1, 2, 3, 1, 5, 6]);
    assert_eq!(whole, partitioned);
    assert_eq!(
        whole
            .1
            .iter()
            .filter(|(event, _, _)| matches!(event, PracticeEvent::Rate(_)))
            .count(),
        1
    );
    assert!(whole.1.iter().any(|(event, _, _)| matches!(event, PracticeEvent::Boundary(end) if end.kind == PracticeBoundaryKind::Ended)));
    assert!(
        whole.0[whole.0.len() - 6..]
            .iter()
            .all(|sample| *sample == 0.0),
        "complete post-end suffix is retained"
    );
}
