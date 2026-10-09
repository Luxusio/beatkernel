use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
fn ms(n: i64) -> Timestamp {
    Timestamp::from_nanos(n * 1_000_000)
}
fn setup(
    source_rate: u32,
    pcm: Vec<f32>,
    start: Timestamp,
    end: Timestamp,
    repeat: bool,
    receipts: usize,
    budget: usize,
) -> (Mixer, CommandProducer, PracticeController) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pl = PcmLimits::new(4096, 8192, 4).unwrap();
    let mut bank = SampleBank::new(format, pl).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(AudioFormat::new(source_rate, 1).unwrap(), pcm, pl).unwrap(),
    )
    .unwrap();
    bank.insert(
        SampleId(2),
        PcmSample::new(format, vec![0.25; 16], pl).unwrap(),
    )
    .unwrap();
    let limits = AudioLimits::new(16, 4, 16, 128, 16).unwrap();
    let (p, c) = command_queue(16).unwrap();
    let program = PreparedPracticeProgram::new(
        &bank,
        vec![PracticeCue {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: ms(0),
            gain: 1.0,
        }],
        PracticeLimits::new(8, 1, budget, 8, receipts).unwrap(),
    )
    .unwrap();
    let (controller, endpoint) = practice_queue(&program).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(format, ClockDomainId(1), ms(0), limits),
        bank,
        c,
    )
    .unwrap();
    mixer
        .install_practice(
            program,
            endpoint,
            PracticeRegion::new(start, end, repeat).unwrap(),
        )
        .unwrap();
    (mixer, p, controller)
}

#[test]
fn empty_program_reserves_no_gameplay_voice_slots() {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(16, 64, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.75; 2], limits).unwrap(),
    )
    .unwrap();
    let program =
        PreparedPracticeProgram::new(&bank, vec![], PracticeLimits::new(1, 0, 8, 2, 4).unwrap())
            .unwrap();
    let (_controller, endpoint) = practice_queue(&program).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push_scoped(
            CommandScope(1),
            AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: ms(0),
                gain: 1.0,
            },
        )
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(1),
            ms(0),
            AudioLimits::new(8, 1, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer
        .install_practice(
            program,
            endpoint,
            PracticeRegion::new(ms(0), ms(8), false).unwrap(),
        )
        .unwrap();
    let mut pcm = [0.0; 2];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.75; 2]);
}
fn play(at: i64) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(100),
        sample: SampleId(2),
        at: ms(at),
        gain: 1.0,
    }
}
#[test]
fn literal_pcm_and_ordered_receipts_repeat_without_any_producer_tick() {
    let (mut mixer, _p, mut c) = setup(
        1000,
        vec![0.1, 0.2, 0.3, 0.4, 0.5],
        ms(1),
        ms(3),
        true,
        16,
        64,
    );
    let mut pcm = [9.0; 10];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.2, 0.3, 0.2, 0.3, 0.2, 0.3, 0.2, 0.3, 0.2, 0.3]);
    for i in 0..5 {
        let r = c.try_pop_receipt().unwrap();
        assert_eq!(
            (
                r.generation,
                r.iteration,
                r.physical_frame,
                r.playback_frame
            ),
            (i + 1, i, i * 2, i * 2)
        );
        assert_eq!(r.requested_song_time, ms(1));
        assert_eq!(r.applied_song_time, ms(1));
        assert_eq!(
            r.kind,
            if i == 0 {
                PracticeBoundaryKind::Started
            } else {
                PracticeBoundaryKind::Looped
            }
        );
    }
    assert_eq!(c.try_pop_receipt(), Err(PracticeError::Empty));
    assert_eq!(mixer.frame_cursor(), 10);
    assert_eq!(mixer.playback_frame_cursor(), 10);
}
#[test]
fn mixed_source_rates_select_original_ceil_head_without_changing_chart_anchor() {
    let (mut mixer, _p, mut c) = setup(
        1500,
        vec![0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
        Timestamp::from_nanos(1_100_000),
        Timestamp::from_nanos(3_100_000),
        true,
        8,
        32,
    );
    let mut pcm = [0.0; 4];
    mixer.render(&mut pcm).unwrap();
    let expected = [0.2, 0.35, 0.2, 0.35];
    for (a, b) in pcm.into_iter().zip(expected) {
        assert!((a - b).abs() < 1e-6);
    }
    let r = c.try_pop_receipt().unwrap();
    assert_eq!(r.applied_song_time, Timestamp::from_nanos(1_100_000));
    assert_eq!(r.correction_nanos, 0);
}
#[test]
fn old_queued_pending_and_late_producer_scope_cannot_enter_new_pass() {
    let (mut mixer, mut p, mut c) = setup(1000, vec![0.1; 16], ms(0), ms(2), true, 16, 64);
    p.set_scope(CommandScope(1));
    p.try_push(play(3)).unwrap();
    p.try_push_scoped(CommandScope(2), play(2)).unwrap();
    let mut pcm = [0.0; 4];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.1, 0.1, 0.35, 0.35]);
    while c.try_pop_receipt().is_ok() {}
    p.try_push(play(4)).unwrap();
    let mut next = [0.0; 2];
    mixer.render(&mut next).unwrap();
    assert_eq!(next, [0.1, 0.1]);
    assert_eq!(p.scope(), CommandScope(1));
}
#[test]
fn pause_preserves_playback_scope_and_moves_only_physical_boundaries() {
    let (mut mixer, mut p, mut c) = setup(1000, vec![0.1, 0.2, 0.3], ms(0), ms(2), true, 16, 32);
    let mut one = [0.0];
    mixer.render(&mut one).unwrap();
    assert_eq!(one, [0.1]);
    c.try_pop_receipt().unwrap();
    p.request_pause(true);
    let mut held = [9.0; 3];
    mixer.render(&mut held).unwrap();
    assert_eq!(held, [0.0; 3]);
    assert_eq!(mixer.playback_frame_cursor(), 1);
    assert_eq!(c.try_pop_receipt(), Err(PracticeError::Empty));
    p.request_pause(false);
    let mut resumed = [0.0; 2];
    mixer.render(&mut resumed).unwrap();
    assert_eq!(resumed, [0.2, 0.1]);
    let r = c.try_pop_receipt().unwrap();
    assert_eq!((r.physical_frame, r.playback_frame), (5, 2));
}
#[test]
fn receipt_and_event_budget_refusals_leave_pcm_commands_and_cursors_unchanged() {
    let (mut mixer, mut p, mut c) = setup(1000, vec![0.1; 8], ms(0), ms(1), true, 1, 64);
    p.set_scope(CommandScope(1));
    p.try_push(play(0)).unwrap();
    let mut pcm = [9.0; 2];
    assert_eq!(mixer.render(&mut pcm), Err(AudioError::PracticeReceiptFull));
    assert_eq!(pcm, [9.0; 2]);
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.counters().commands_consumed
        ),
        (0, 0, 0)
    );
    mixer.render(&mut pcm[..1]).unwrap();
    assert_eq!(pcm[0], 0.35);
    let first = c.try_pop_receipt().unwrap();
    assert_eq!(first.generation, 1);
    mixer.render(&mut pcm[..1]).unwrap();
    assert_eq!(c.try_pop_receipt().unwrap().generation, 2);
    let (mut mixer, _p, _c) = setup(1000, vec![0.1; 8], ms(0), ms(1), true, 8, 1);
    let mut pcm = [9.0];
    assert_eq!(mixer.render(&mut pcm), Err(AudioError::PracticeEventBudget));
    assert_eq!(pcm, [9.0]);
    assert_eq!(mixer.frame_cursor(), 0);
}
#[test]
fn explicit_scrub_then_disable_retains_current_heads_and_emits_real_end() {
    let (mut mixer, _p, mut c) = setup(
        1000,
        vec![0.1, 0.2, 0.3, 0.4, 0.5],
        ms(0),
        ms(2),
        true,
        16,
        64,
    );
    mixer.render(&mut [0.0]).unwrap();
    c.try_pop_receipt().unwrap();
    c.try_request(PracticeRequest {
        id: 1,
        expected_generation: 1,
        at_playback_frame: 1,
        region: PracticeRegion::new(ms(2), ms(4), true).unwrap(),
    })
    .unwrap();
    let mut one = [0.0];
    mixer.render(&mut one).unwrap();
    assert_eq!(one, [0.3]);
    let r = c.try_pop_receipt().unwrap();
    assert_eq!(
        (r.request_id, r.generation, r.kind),
        (1, 2, PracticeBoundaryKind::Requested)
    );
    c.try_disable_loop(2, 2, 2).unwrap();
    let mut rest = [0.0; 3];
    mixer.render(&mut rest).unwrap();
    assert_eq!(rest, [0.4, 0.0, 0.0]);
    let disabled = c.try_pop_receipt().unwrap();
    assert_eq!(
        (disabled.generation, disabled.kind),
        (2, PracticeBoundaryKind::LoopDisabled)
    );
    let end = c.try_pop_receipt().unwrap();
    assert_eq!(
        (end.generation, end.playback_frame, end.kind),
        (3, 3, PracticeBoundaryKind::Ended)
    );
    assert!(mixer.practice_finished());
    assert_eq!(mixer.playback_frame_cursor(), 5);
}
#[test]
fn negative_preroll_and_subframe_song_start_are_not_rounded_away() {
    let (mut mixer, _p, mut c) = setup(
        1000,
        vec![0.1, 0.2, 0.3],
        Timestamp::from_nanos(-1_500_000),
        Timestamp::from_nanos(1_500_000),
        true,
        16,
        64,
    );
    let mut pcm = [0.0; 6];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0, 0.0, 0.1, 0.0, 0.0, 0.1]);
    let r = c.try_pop_receipt().unwrap();
    assert_eq!(r.applied_song_time, Timestamp::from_nanos(-1_500_000));
}
#[test]
fn partition_invariance_with_real_conversion_phase_history_and_unread_pcm() {
    fn run(parts: &[usize]) -> (Vec<f32>, Vec<PracticeReceipt>, SourcePosition) {
        let (mixer, _p, mut c) = setup(1000, vec![0.1, 0.2, 0.3, 0.4], ms(0), ms(3), true, 64, 128);
        let target = AudioFormat::new(1500, 1).unwrap();
        let matrix = ChannelMatrix::default_mix(1, 1).unwrap();
        let mut converted =
            ConvertedMixer::new(mixer, target, matrix, ResampleQuality::Linear, 32).unwrap();
        let mut out = Vec::new();
        for n in parts {
            let mut block = vec![0.0; *n];
            converted.render(&mut block).unwrap();
            out.extend(block);
        }
        let position = converted.source_position();
        let mut receipts = Vec::new();
        while let Ok(r) = c.try_pop_receipt() {
            receipts.push(r);
        }
        (out, receipts, position)
    }
    let (a, ra, pa) = run(&[18]);
    let (b, rb, pb) = run(&[1, 2, 1, 5, 2, 7]);
    assert_eq!(a, b);
    assert_eq!(ra, rb);
    assert_eq!(pa, pb);
    // Continuous 2/3 source stepping interpolates across actual loop boundaries.
    let expected = [
        0.1, 0.16666667, 0.23333333, 0.3, 0.16666667, 0.13333334, 0.2, 0.26666668, 0.23333333,
    ];
    for (x, y) in a[..9].iter().zip(expected) {
        assert!((*x - y).abs() < 1e-6);
    }
}
#[test]
fn cold_validation_and_original_source_selection_are_explicit() {
    let f = AudioFormat::new(1000, 1).unwrap();
    let pl = PcmLimits::new(1024, 4096, 4).unwrap();
    let mut bank = SampleBank::new(f, pl).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(AudioFormat::new(1500, 1).unwrap(), vec![0.0; 8], pl).unwrap(),
    )
    .unwrap();
    let l = PracticeLimits::new(8, 1, 16, 1, 2).unwrap();
    let cue = PracticeCue {
        voice: VoiceId(1),
        sample: SampleId(1),
        at: ms(0),
        gain: 1.0,
    };
    let p = PreparedPracticeProgram::new(&bank, vec![cue], l).unwrap();
    let selected = p
        .source_selection(VoiceId(1), Timestamp::from_nanos(1_100_000))
        .unwrap();
    assert_eq!(
        (
            selected.frame,
            selected.requested_elapsed_nanos,
            selected.applied_elapsed_nanos,
            selected.correction_nanos
        ),
        (2, 1_100_000, 1_333_334, 233_334)
    );
    assert!(matches!(
        PreparedPracticeProgram::new(
            &bank,
            vec![PracticeCue {
                gain: f32::NAN,
                ..cue
            }],
            l
        ),
        Err(PracticeError::InvalidGain)
    ));
    assert!(matches!(
        PreparedPracticeProgram::new(
            &bank,
            vec![PracticeCue {
                sample: SampleId(9),
                ..cue
            }],
            l
        ),
        Err(PracticeError::UnknownSample)
    ));
    assert!(matches!(
        PreparedPracticeProgram::new(&bank, vec![cue, cue], l),
        Err(PracticeError::DuplicateVoice)
    ));
    let (mut controller, _audio) = practice_queue(&p).unwrap();
    let r = PracticeRequest {
        id: 1,
        expected_generation: 1,
        at_playback_frame: 0,
        region: PracticeRegion::new(ms(0), ms(1), true).unwrap(),
    };
    controller.try_request(r).unwrap();
    assert_eq!(
        controller.try_request(PracticeRequest { id: 2, ..r }),
        Err(PracticeError::Full)
    );
    assert_eq!(
        controller.try_request(PracticeRequest {
            id: 3,
            expected_generation: 2,
            ..r
        }),
        Err(PracticeError::StaleGeneration)
    );
}

#[test]
fn disable_at_exact_repeat_boundary_ends_at_that_frame_without_extra_sample() {
    let (mut mixer, _p, mut c) = setup(1000, vec![0.1, 0.2, 0.3], ms(0), ms(2), true, 8, 32);
    let mut first = [0.0; 2];
    mixer.render(&mut first).unwrap();
    c.try_pop_receipt().unwrap();
    c.try_disable_loop(1, 1, 2).unwrap();
    let mut end = [9.0];
    mixer.render(&mut end).unwrap();
    assert_eq!(end, [0.0]);
    let disabled = c.try_pop_receipt().unwrap();
    let ended = c.try_pop_receipt().unwrap();
    assert_eq!((disabled.physical_frame, ended.physical_frame), (2, 2));
    assert_eq!((disabled.generation, ended.generation), (1, 2));
    assert_eq!(ended.kind, PracticeBoundaryKind::Ended);
}

#[test]
fn original_multiple_cues_overlap_and_new_scoped_keys_share_same_boundary() {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pl = PcmLimits::new(1024, 4096, 4).unwrap();
    let mut bank = SampleBank::new(format, pl).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.1; 4], pl).unwrap(),
    )
    .unwrap();
    bank.insert(
        SampleId(2),
        PcmSample::new(format, vec![0.2; 4], pl).unwrap(),
    )
    .unwrap();
    let cues = vec![
        PracticeCue {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: ms(0),
            gain: 1.0,
        },
        PracticeCue {
            voice: VoiceId(2),
            sample: SampleId(2),
            at: ms(2),
            gain: 1.0,
        },
    ];
    let p = PreparedPracticeProgram::new(&bank, cues, PracticeLimits::new(2, 2, 32, 2, 8).unwrap())
        .unwrap();
    let (mut controller, endpoint) = practice_queue(&p).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let limits = AudioLimits::new(8, 4, 8, 32, 8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(format, ClockDomainId(1), ms(0), limits),
        bank,
        consumer,
    )
    .unwrap();
    mixer
        .install_practice(
            p,
            endpoint,
            PracticeRegion::new(ms(1), ms(4), true).unwrap(),
        )
        .unwrap();
    producer
        .try_push_scoped(
            CommandScope(2),
            AudioCommand::Play {
                voice: VoiceId(100),
                sample: SampleId(2),
                at: ms(3),
                gain: 1.0,
            },
        )
        .unwrap();
    let mut out = [0.0; 5];
    mixer.render(&mut out).unwrap();
    for (a, b) in out.into_iter().zip([0.1, 0.3, 0.3, 0.3, 0.5]) {
        assert!((a - b).abs() < 1e-6);
    }
    assert_eq!(controller.try_pop_receipt().unwrap().generation, 1);
    let repeat = controller.try_pop_receipt().unwrap();
    assert_eq!((repeat.generation, repeat.playback_frame), (2, 3));
}

#[test]
fn off_grid_mixed_rate_voice_capacity_accounts_for_discrete_lifetimes() {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm_limits = PcmLimits::new(1024, 4096, 2).unwrap();
    for anchor in [Timestamp::ZERO, Timestamp::from_nanos(370_000)] {
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(
                AudioFormat::new(1500, 1).unwrap(),
                vec![0.1, 0.2],
                pcm_limits,
            )
            .unwrap(),
        )
        .unwrap();
        let cues = vec![
            PracticeCue {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::from_nanos(500_000),
                gain: 1.0,
            },
            PracticeCue {
                voice: VoiceId(2),
                sample: SampleId(1),
                at: Timestamp::from_nanos(1_900_000),
                gain: 1.0,
            },
        ];
        // The original mathematical PCM intervals do not overlap: the first
        // ends at 1.833334ms. Actual Ceil scheduling needs two live slots at
        // frame2 because the first discrete head is still on source frame1.5.
        assert!(matches!(
            PreparedPracticeProgram::new(
                &bank,
                cues.clone(),
                PracticeLimits::new(2, 1, 32, 2, 8).unwrap()
            ),
            Err(PracticeError::Capacity)
        ));
        let program =
            PreparedPracticeProgram::new(&bank, cues, PracticeLimits::new(2, 2, 32, 2, 8).unwrap())
                .unwrap();
        let (_controller, endpoint) = practice_queue(&program).unwrap();
        let (_producer, consumer) = command_queue(2).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(1),
                Timestamp::ZERO,
                AudioLimits::new(2, 2, 2, 8, 2).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        mixer
            .install_practice(
                program,
                endpoint,
                PracticeRegion::new(anchor, Timestamp::from_nanos(5_000_000), false).unwrap(),
            )
            .unwrap();
        let mut out = [0.0; 4];
        mixer.render(&mut out).unwrap();
        for (a, b) in out.into_iter().zip([0.0, 0.1, 0.3, 0.2]) {
            assert!((a - b).abs() < 1e-6);
        }
    }
}

#[test]
fn next_disable_short_loop_precedes_same_frame_autonomous_repeat() {
    let (mut mixer, _producer, mut controller) =
        setup(1000, vec![0.1, 0.2, 0.3], ms(0), ms(2), true, 16, 64);
    let mut preceding = [0.0; 4];
    mixer.render(&mut preceding).unwrap();
    assert_eq!(preceding, [0.1, 0.2, 0.1, 0.2]);
    while controller.try_pop_receipt().is_ok() {}
    controller
        .try_disable_loop_next(1, controller.generation())
        .unwrap();
    let mut output = [9.0; 2];
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0, 0.0]);
    let disabled = controller.try_pop_receipt().unwrap();
    let ended = controller.try_pop_receipt().unwrap();
    assert_eq!(
        (disabled.kind, disabled.generation, disabled.playback_frame),
        (PracticeBoundaryKind::LoopDisabled, 2, 4)
    );
    assert_eq!(
        (ended.kind, ended.generation, ended.playback_frame),
        (PracticeBoundaryKind::Ended, 3, 4)
    );
    assert!(mixer.practice_finished());
}
#[test]
fn next_scrub_mixed_rate_retains_source_ceil_and_old_scope_exclusion() {
    let (mut mixer, mut producer, mut controller) = setup(
        1500,
        vec![0.0, 0.1, 0.2, 0.3, 0.4, 0.5],
        ms(0),
        ms(1),
        true,
        16,
        64,
    );
    mixer.render(&mut [0.0; 3]).unwrap();
    while controller.try_pop_receipt().is_ok() {}
    producer.set_scope(CommandScope(1));
    producer.try_push(play(3)).unwrap();
    controller
        .try_request_next(
            1,
            controller.generation(),
            PracticeRegion::new(
                Timestamp::from_nanos(1_100_000),
                Timestamp::from_nanos(3_100_000),
                true,
            )
            .unwrap(),
        )
        .unwrap();
    let mut output = [0.0; 3];
    mixer.render(&mut output).unwrap();
    for (a, b) in output.into_iter().zip([0.2, 0.35, 0.2]) {
        assert!((a - b).abs() < 1e-6);
    }
    let receipt = controller.try_pop_receipt().unwrap();
    assert_eq!(
        (
            receipt.request_id,
            receipt.generation,
            receipt.playback_frame,
            receipt.applied_song_time
        ),
        (1, 4, 3, Timestamp::from_nanos(1_100_000))
    );
    assert_eq!(producer.scope(), CommandScope(1));
}
#[test]
fn queued_next_edits_refuse_superseded_revision_without_fatal_audio_or_fake_commit() {
    let (mut mixer, _producer, mut controller) = setup(
        1000,
        vec![0.1, 0.2, 0.3, 0.4, 0.5],
        ms(0),
        ms(2),
        true,
        16,
        64,
    );
    controller
        .try_request_next(1, 1, PracticeRegion::new(ms(2), ms(4), true).unwrap())
        .unwrap();
    controller
        .try_request_next(2, 1, PracticeRegion::new(ms(0), ms(1), true).unwrap())
        .unwrap();
    controller.try_disable_loop_next(3, 1).unwrap();
    let mut out = [0.0; 3];
    mixer.render(&mut out).unwrap();
    assert_eq!(out, [0.3, 0.4, 0.3]);
    let applied = controller.try_pop_receipt().unwrap();
    assert_eq!(
        (applied.request_id, applied.generation, applied.kind),
        (1, 2, PracticeBoundaryKind::Requested)
    );
    let rejected = controller.try_pop_receipt().unwrap();
    assert_eq!(
        (rejected.request_id, rejected.generation, rejected.kind),
        (2, 2, PracticeBoundaryKind::ControlRejectedRevision)
    );
    assert_eq!(
        (
            rejected.requested_song_time,
            rejected.applied_song_time,
            rejected.correction_nanos
        ),
        (ms(0), ms(2), 2_000_000)
    );
    let disable = controller.try_pop_receipt().unwrap();
    assert_eq!(
        (disable.request_id, disable.generation, disable.kind),
        (3, 2, PracticeBoundaryKind::ControlRejectedRevision)
    );
    assert_eq!(
        controller.try_pop_receipt().unwrap().kind,
        PracticeBoundaryKind::Looped
    );
}
#[test]
fn next_controls_obey_receipt_preflight_atomicity() {
    let (mut mixer, mut producer, mut controller) =
        setup(1000, vec![0.1; 8], ms(0), ms(2), true, 1, 64);
    producer.try_push_scoped(CommandScope(2), play(0)).unwrap();
    controller
        .try_request_next(1, 1, PracticeRegion::new(ms(2), ms(4), true).unwrap())
        .unwrap();
    controller.try_disable_loop_next(2, 1).unwrap();
    let mut out = [9.0];
    assert_eq!(mixer.render(&mut out), Err(AudioError::PracticeReceiptFull));
    assert_eq!(out, [9.0]);
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.practice_generation(),
            mixer.counters().commands_consumed
        ),
        (0, 0, Some(1), 0)
    );
    assert_eq!(controller.try_pop_receipt(), Err(PracticeError::Empty));
    assert_eq!(controller.generation(), 1);
}
#[test]
fn future_exact_control_retains_fifo_precedence_over_next_control() {
    let (mut mixer, _producer, mut controller) = setup(
        1000,
        vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
        ms(0),
        ms(6),
        true,
        16,
        64,
    );
    controller
        .try_request(PracticeRequest {
            id: 1,
            expected_generation: 1,
            at_playback_frame: 2,
            region: PracticeRegion::new(ms(3), ms(5), true).unwrap(),
        })
        .unwrap();
    controller
        .try_request_next(2, 1, PracticeRegion::new(ms(0), ms(1), true).unwrap())
        .unwrap();
    let mut out = [0.0; 4];
    mixer.render(&mut out).unwrap();
    assert_eq!(out, [0.1, 0.2, 0.4, 0.5]);
    assert_eq!(
        controller.try_pop_receipt().unwrap().kind,
        PracticeBoundaryKind::Started
    );
    let exact = controller.try_pop_receipt().unwrap();
    let next = controller.try_pop_receipt().unwrap();
    assert_eq!(
        (exact.request_id, exact.playback_frame, exact.kind),
        (1, 2, PracticeBoundaryKind::Requested)
    );
    assert_eq!(
        (next.request_id, next.playback_frame, next.kind),
        (2, 2, PracticeBoundaryKind::ControlRejectedRevision)
    );
}

#[test]
fn next_disable_after_autonomous_end_acknowledges_actual_terminal_anchor() {
    let (mut mixer, _producer, mut controller) =
        setup(1000, vec![0.1, 0.2], ms(0), ms(1), false, 8, 64);
    let mut out = [0.0; 2];
    mixer.render(&mut out).unwrap();
    assert_eq!(out, [0.1, 0.0]);
    while controller.try_pop_receipt().is_ok() {}
    controller.try_disable_loop_next(1, 2).unwrap();
    mixer.render(&mut [9.0]).unwrap();
    let r = controller.try_pop_receipt().unwrap();
    assert_eq!(
        (
            r.kind,
            r.generation,
            r.requested_song_time,
            r.applied_song_time,
            r.correction_nanos
        ),
        (PracticeBoundaryKind::LoopDisabled, 2, ms(1), ms(1), 0)
    );
    assert!(mixer.practice_finished());
}

fn converted_practice(receipts: usize) -> (ConvertedMixer, CommandProducer, PracticeController) {
    let (mixer, producer, controller) = setup(
        1000,
        vec![0.1, 0.2, 0.3, 0.4],
        ms(0),
        ms(1),
        true,
        receipts,
        128,
    );
    let converted = ConvertedMixer::new(
        mixer,
        AudioFormat::new(1500, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        32,
    )
    .unwrap();
    (converted, producer, controller)
}
#[test]
fn projected_receipts_hide_source_lookahead_and_exclusive_target_edge_through_hold() {
    let (mut converted, _producer, mut controller) = converted_practice(16);
    let mut one = [0.0];
    converted.render(&mut one).unwrap();
    assert_eq!(one, [0.1]);
    let start = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (
            start.receipt.physical_frame,
            start.target_frame,
            start.boundary.target_frame_offset,
            start.boundary.target_time
        ),
        (0, 0, 0, TargetTime::from_frames(0, 1500).unwrap())
    );
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    let source = converted.source_position();
    let mut hold = [9.0; 3];
    converted.render_held(&mut hold).unwrap();
    assert_eq!(hold, [0.0; 3]);
    assert_eq!(converted.source_position(), source);
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    let cached = converted.render(&mut one).unwrap();
    assert_eq!(cached.source.unwrap().frames, 0);
    assert_eq!(one, [0.1]);
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    converted.render(&mut one).unwrap();
    assert_eq!(one, [0.1]);
    let boundary = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (
            boundary.receipt.generation,
            boundary.receipt.physical_frame,
            boundary.target_frame,
            boundary.target_rate
        ),
        (2, 1, 5, 1500)
    );
    assert_eq!(boundary.boundary.target_frame_offset, 0);
    assert_eq!(
        boundary.boundary.target_time,
        TargetTime::from_frames(5, 1500).unwrap()
    );
    assert_eq!(boundary.origin.domain, ClockDomainId(1));
    assert_eq!(boundary.origin.timestamp, Timestamp::ZERO);
}
#[test]
fn raw_and_projected_backlog_share_one_capacity_and_preflight_retains_conversion_state() {
    let (mut converted, _producer, mut controller) = converted_practice(2);
    converted.render(&mut [0.0]).unwrap();
    converted.render(&mut [0.0]).unwrap();
    let position = converted.source_position();
    let frontier = converted.pulled_source_frame_cursor();
    let mut untouched = [9.0];
    assert_eq!(
        converted.render(&mut untouched),
        Err(AudioError::PracticeReceiptFull)
    );
    assert_eq!(untouched, [9.0]);
    assert_eq!(converted.source_position(), position);
    assert_eq!(converted.pulled_source_frame_cursor(), frontier);
    let start = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(start.receipt.generation, 1);
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    converted.render(&mut untouched).unwrap();
    assert_eq!(untouched, [0.1]);
    let next = controller.try_pop_projected_receipt().unwrap();
    assert_eq!((next.receipt.generation, next.target_frame), (2, 2));
    assert_eq!(
        next.boundary.target_time,
        TargetTime::from_frames(2, 1500).unwrap()
    );
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
}
#[test]
fn deferred_projection_survives_rate_retarget_and_held_pcm_without_phase_reset() {
    let (mixer, _producer, mut controller) =
        setup(1000, vec![0.1, 0.2, 0.3, 0.4], ms(1), ms(3), true, 16, 128);
    let mut converted = ConvertedMixer::new(
        mixer,
        AudioFormat::new(1500, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        32,
    )
    .unwrap();
    converted.render(&mut [0.0; 6]).unwrap();
    let first = controller.try_pop_projected_receipt().unwrap();
    let second = controller.try_pop_projected_receipt().unwrap();
    assert_eq!((first.target_frame, second.target_frame), (0, 3));
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    let position = converted.source_position();
    assert_eq!((position.frame, position.numerator), (4, 0));
    converted
        .retarget(
            AudioFormat::new(2000, 1).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            32,
        )
        .unwrap();
    assert_eq!(converted.source_position(), position);
    converted.render_held(&mut [0.0; 2]).unwrap();
    assert_eq!(converted.source_position(), position);
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    let mut output = [0.0; 4];
    converted.render(&mut output).unwrap();
    for (a, b) in output.into_iter().zip([0.2, 0.25, 0.3, 0.25]) {
        assert!((a - b).abs() < 1e-6);
    }
    let boundary = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (
            boundary.receipt.physical_frame,
            boundary.target_frame,
            boundary.target_rate
        ),
        (4, 8, 2000)
    );
    assert_eq!(
        boundary.boundary.target_time,
        TargetTime::from_frames(5, 1000).unwrap()
    );
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
}
#[test]
fn projected_absolute_coordinates_are_partition_invariant_on_nonintegral_rates() {
    type Projection = (u64, u64, TargetTime, u32);
    fn run(parts: &[usize]) -> (Vec<f32>, Vec<Projection>) {
        let (mut converted, _producer, mut controller) = converted_practice(64);
        let mut pcm = Vec::new();
        for n in parts {
            let mut block = vec![0.0; *n];
            converted.render(&mut block).unwrap();
            pcm.extend(block);
        }
        let mut proofs = Vec::new();
        while let Ok(p) = controller.try_pop_projected_receipt() {
            proofs.push((
                p.receipt.generation,
                p.target_frame,
                p.boundary.target_time,
                p.target_rate,
            ));
        }
        (pcm, proofs)
    }
    let (a, pa) = run(&[12]);
    let (b, pb) = run(&[1, 2, 1, 3, 5]);
    assert_eq!(a, [0.1; 12]);
    assert_eq!(a, b);
    assert_eq!(pa, pb);
    for (generation, frame, time, rate) in pa {
        let source_frame = generation - 1;
        let target = (source_frame * 3).div_ceil(2);
        assert_eq!(frame, target);
        assert_eq!(time, TargetTime::from_frames(target, 1500).unwrap());
        assert_eq!(rate, 1500);
    }
}
#[test]
fn direct_projection_uses_physical_frame_basis_even_with_negative_song_preroll() {
    let (mut mixer, _producer, mut controller) =
        setup(1000, vec![0.1, 0.2], ms(-1), ms(1), true, 8, 64);
    let mut output = [0.0; 3];
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0, 0.1, 0.0]);
    let start = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(start.receipt.applied_song_time, ms(-1));
    assert_eq!(
        (
            start.target_frame,
            start.boundary.source_frame,
            start.boundary.target_time
        ),
        (0, 0, TargetTime::from_frames(0, 1000).unwrap())
    );
    let repeat = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (
            repeat.receipt.generation,
            repeat.target_frame,
            repeat.boundary.target_time
        ),
        (2, 2, TargetTime::from_frames(2, 1000).unwrap())
    );
    assert_eq!(repeat.origin.timestamp, Timestamp::ZERO);
}

#[test]
fn cold_conversion_transfer_refuses_pending_identity_proof_and_preserves_drained_retry() {
    let (mut mixer, mut producer, mut controller) = setup(
        1000,
        vec![0.1, 0.2, 0.3, 0.4, 0.5],
        ms(0),
        ms(5),
        true,
        16,
        64,
    );
    producer.set_scope(CommandScope(1));
    producer.try_push(play(100)).unwrap();
    let mut direct = [0.0; 2];
    mixer.render(&mut direct).unwrap();
    assert_eq!(direct, [0.1, 0.2]);
    let before = mixer.render(&mut []).unwrap();
    let config = mixer.configuration();
    assert_eq!(before.pending_commands, 1);
    assert_eq!(
        mixer.validate_practice_projection_transfer(),
        Err(AudioError::PracticeProjectionPending)
    );
    let failure = match ConvertedMixer::new(
        mixer,
        AudioFormat::new(1500, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    ) {
        Ok(_) => panic!("pending original-output proof must refuse fresh conversion"),
        Err(failure) => failure,
    };
    assert_eq!(*failure.error(), AudioError::PracticeProjectionPending);
    let (_, recovered) = failure.into_parts();
    let mut mixer = recovered.unwrap();
    assert_eq!(mixer.configuration(), config);
    assert_eq!(mixer.render(&mut []).unwrap(), before);
    assert_eq!(mixer.practice_generation(), Some(1));
    assert_eq!(producer.scope(), CommandScope(1));
    let old = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (
            old.receipt.generation,
            old.target_frame,
            old.target_rate,
            old.boundary.target_time
        ),
        (1, 0, 1000, TargetTime::from_frames(0, 1000).unwrap())
    );
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    assert_eq!(mixer.validate_practice_projection_transfer(), Ok(()));
    let mut converted = ConvertedMixer::new(
        mixer,
        AudioFormat::new(1500, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap();
    assert_eq!(converted.source_position().frame, 2);
    let mut first = [0.0];
    converted.render(&mut first).unwrap();
    assert_eq!(first, [0.3]);
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    assert_eq!(converted.mixer().practice_generation(), Some(1));
    assert_eq!(
        converted.mixer().counters().commands_consumed,
        before.counters.commands_consumed
    );
}
#[test]
fn cold_conversion_allows_paused_practice_without_receipts_and_retains_physical_origin() {
    let (mut mixer, mut producer, mut controller) = setup(
        1000,
        vec![0.1, 0.2, 0.3, 0.4, 0.5],
        ms(0),
        ms(5),
        true,
        16,
        64,
    );
    producer.request_pause(true);
    let mut held = [9.0; 3];
    mixer.render(&mut held).unwrap();
    assert_eq!(held, [0.0; 3]);
    assert_eq!(mixer.playback_frame_cursor(), 0);
    assert_eq!(mixer.validate_practice_projection_transfer(), Ok(()));
    let mut converted = ConvertedMixer::new(
        mixer,
        AudioFormat::new(1500, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap();
    producer.request_pause(false);
    let mut active = [0.0];
    converted.render(&mut active).unwrap();
    assert_eq!(active, [0.1]);
    let started = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (
            started.receipt.physical_frame,
            started.receipt.playback_frame,
            started.target_frame
        ),
        (3, 0, 0)
    );
    assert_eq!(
        started.boundary.target_time,
        TargetTime::from_frames(3, 1000).unwrap()
    );
}
#[test]
fn cold_conversion_of_advanced_program_free_mixer_keeps_legacy_pcm_head() {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pl = PcmLimits::new(128, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, pl).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.1, 0.2, 0.3, 0.4, 0.5], pl).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(4).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(1),
            Timestamp::ZERO,
            AudioLimits::new(4, 1, 4, 32, 4).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!(mixer.validate_practice_projection_transfer(), Ok(()));
    let mut converted = ConvertedMixer::new(
        mixer,
        AudioFormat::new(1500, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap();
    let mut first = [0.0];
    converted.render(&mut first).unwrap();
    assert_eq!(first, [0.3]);
    assert_eq!(converted.mixer().practice_generation(), None);
}
