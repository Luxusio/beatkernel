//! Literal integration oracles for original-PCM retained fixed-speed playback.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Duration, Timestamp},
    transport::Rate,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}
struct AllocationProbe;
fn count(kind: usize) {
    let _ = TRACK.try_with(|enabled| {
        if enabled.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut n = counts.get();
                n[kind] += 1;
                counts.set(n);
            });
        }
    });
}
// SAFETY: Every original allocation argument is forwarded unchanged to System;
// thread-local instrumentation only mutates allocation-free scalar cells.
unsafe impl GlobalAlloc for AllocationProbe {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        count(1);
        unsafe { System.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        count(2);
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOCATOR: AllocationProbe = AllocationProbe;
fn measured<T>(f: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|n| n.set([0; 3]));
    TRACK.with(|t| t.set(true));
    let result = f();
    TRACK.with(|t| t.set(false));
    (result, COUNTS.with(Cell::get))
}
fn ns(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn rate(n: i64, d: u64) -> Rate {
    Rate::new(n, d).unwrap()
}
fn cue(voice: u64, sample: u64, at: i64) -> PracticeCue {
    PracticeCue {
        voice: VoiceId(voice),
        sample: SampleId(sample),
        at: ns(at),
        gain: 1.0,
    }
}
fn bank(source_hz: u32, pcm: Vec<f32>) -> SampleBank {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(4096, 8192, 4).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    // Powers-of-two scaling is exact and keeps literal BGM + keysound sums
    // below the documented output clamp, so rates remain distinguishable.
    let pcm = pcm.into_iter().map(|sample| sample / 128.0).collect();
    bank.insert(
        SampleId(1),
        PcmSample::new(AudioFormat::new(source_hz, 1).unwrap(), pcm, limits).unwrap(),
    )
    .unwrap();
    bank.insert(
        SampleId(2),
        PcmSample::new(
            format,
            vec![
                10.0 / 128.0,
                20.0 / 128.0,
                30.0 / 128.0,
                40.0 / 128.0,
                50.0 / 128.0,
                60.0 / 128.0,
                70.0 / 128.0,
                80.0 / 128.0,
            ],
            limits,
        )
        .unwrap(),
    )
    .unwrap();
    bank
}
fn limits(receipts: usize, events: usize) -> PracticeLimits {
    PracticeLimits::new(16, 4, events, 8, receipts).unwrap()
}
fn rig(
    source_hz: u32,
    pcm: Vec<f32>,
    cues: Vec<PracticeCue>,
    selected: Rate,
    preroll: i64,
    region: PracticeRegion,
    l: PracticeLimits,
    open: bool,
) -> (Mixer, CommandProducer, PracticeController) {
    let b = bank(source_hz, pcm);
    let program = PreparedPracticeProgram::new_with_speed(
        &b,
        cues,
        l,
        selected,
        Duration::from_nanos(preroll),
        open,
    )
    .unwrap();
    assert_eq!(program.rate(), selected);
    assert_eq!(program.output_preroll(), Duration::from_nanos(preroll));
    let (controller, endpoint) = practice_queue(&program).unwrap();
    let (producer, consumer) = command_queue(16).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            AudioFormat::new(1000, 1).unwrap(),
            ClockDomainId(9),
            ns(0),
            AudioLimits::new(16, 8, 16, 256, 16).unwrap(),
        ),
        b,
        consumer,
    )
    .unwrap();
    mixer.install_practice(program, endpoint, region).unwrap();
    assert_eq!(mixer.rate(), selected);
    assert_eq!(mixer.counters().commands_consumed, 0);
    (mixer, producer, controller)
}
fn region(start: i64, end: i64, repeat: bool) -> PracticeRegion {
    PracticeRegion::new(ns(start), ns(end), repeat).unwrap()
}
fn render(m: &mut Mixer, out: &mut [f32]) -> RenderReport {
    let (result, allocations) = measured(|| m.render(out));
    assert_eq!(
        allocations,
        [0, 0, 0],
        "actual retained render alloc/realloc/dealloc"
    );
    result.unwrap()
}
fn close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
        let scaled = b / 128.0;
        assert!(
            (a - scaled).abs() < 0.0000001,
            "sample {i}: actual {a}, expected {scaled}"
        );
    }
}

#[test]
fn allocator_probe_calibrates_all_three_operations() {
    let (_, counts) = measured(|| {
        let mut v = Vec::with_capacity(1);
        v.push(1u8);
        v.reserve_exact(1024);
        std::hint::black_box(&v);
        drop(v);
    });
    assert!(counts.into_iter().all(|n| n > 0));
}

#[test]
fn bgm_and_real_scoped_keysound_share_literal_half_normal_double_and_three_halves_heads() {
    for (selected, expected) in [
        (rate(1, 2), [1.0, 11.5, 17.0, 22.5]),
        (Rate::NORMAL, [1.0, 12.0, 23.0, 34.0]),
        (rate(2, 1), [1.0, 13.0, 35.0, 57.0]),
        (rate(3, 2), [1.0, 12.5, 29.0, 45.5]),
    ] {
        let (mut mixer, mut producer, mut controller) = rig(
            1000,
            vec![1., 2., 3., 4., 5., 6., 7., 8.],
            vec![cue(1, 1, 0)],
            selected,
            0,
            region(0, 10_000_000, false),
            limits(16, 128),
            false,
        );
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
        let mut out = [99.; 4];
        let report = render(&mut mixer, &mut out);
        close(&out, &expected);
        assert_eq!(report.counters.commands_consumed, 1);
        assert_eq!(report.counters.commands_applied, 1);
        let start = controller.try_pop_receipt().unwrap();
        assert_eq!(
            (
                start.generation,
                start.physical_frame,
                start.playback_frame,
                start.applied_song_time
            ),
            (1, 0, 0, ns(0))
        );
        assert_eq!(mixer.configuration().format().sample_rate(), 1000);
    }
}

#[test]
fn normal_opt_in_preserves_legacy_pcm_complete_receipts_and_command_credits() {
    fn run(opt_in: bool) -> (Vec<f32>, Vec<PracticeReceipt>, RenderReport) {
        let b = bank(1500, vec![1., 2., 3., 4., 5., 6., 7., 8.]);
        let cues = vec![cue(1, 1, 0)];
        let l = limits(32, 128);
        let p = if opt_in {
            PreparedPracticeProgram::new_with_speed(
                &b,
                cues,
                l,
                Rate::NORMAL,
                Duration::ZERO,
                false,
            )
            .unwrap()
        } else {
            PreparedPracticeProgram::new(&b, cues, l).unwrap()
        };
        let (mut controller, endpoint) = practice_queue(&p).unwrap();
        let (mut producer, consumer) = command_queue(16).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                AudioFormat::new(1000, 1).unwrap(),
                ClockDomainId(9),
                ns(0),
                AudioLimits::new(16, 8, 16, 256, 16).unwrap(),
            ),
            b,
            consumer,
        )
        .unwrap();
        mixer
            .install_practice(p, endpoint, region(1_100_000, 3_100_000, true))
            .unwrap();
        producer
            .try_push_scoped(
                CommandScope(2),
                AudioCommand::Play {
                    voice: VoiceId(100),
                    sample: SampleId(2),
                    at: ns(2_000_000),
                    gain: 1.0,
                },
            )
            .unwrap();
        let mut out = vec![0.; 8];
        let report = render(&mut mixer, &mut out);
        let mut receipts = Vec::new();
        while let Ok(r) = controller.try_pop_receipt() {
            receipts.push(r);
        }
        (out, receipts, report)
    }
    let legacy = run(false);
    let speed = run(true);
    assert_eq!(speed, legacy);
    close(&speed.0, &[3., 4.5, 13., 24.5, 3., 4.5, 3., 4.5]);
    assert_eq!(speed.1.len(), 4);
    assert_eq!(speed.1[0].applied_song_time, ns(1_100_000));
}

#[test]
fn odd_physical_preroll_uses_whole_signed_phase_and_exclusive_non_grid_end() {
    // S=1ns, P=1_500_001ns, N/D=3/2. Initial song is -2_250_000ns;
    // at E=2ms it is 749_999ns (not 749_998 from separately rounded terms).
    let (mut mixer, _producer, mut controller) = rig(
        1000,
        vec![1., 2., 3., 4.],
        vec![cue(1, 1, 749_999), cue(2, 2, 3_749_999)],
        rate(3, 2),
        1_500_001,
        region(1, 3_749_999, false),
        limits(16, 128),
        false,
    );
    let mut out = [99.; 6];
    render(&mut mixer, &mut out);
    close(&out, &[0., 0., 1., 2.5, 0., 0.]);
    let first = controller.try_pop_receipt().unwrap();
    assert_eq!(
        (
            first.requested_song_time,
            first.applied_song_time,
            first.correction_nanos
        ),
        (ns(1), ns(-2_250_000), -2_250_001)
    );
    let end = controller.try_pop_receipt().unwrap();
    assert_eq!(
        (end.kind, end.physical_frame, end.playback_frame),
        (PracticeBoundaryKind::Ended, 4, 4)
    );
    assert!(mixer.practice_finished());
}

#[test]
fn original_before_equal_after_cues_keep_ceil_source_selection_at_every_speed() {
    for selected in [rate(1, 2), Rate::NORMAL, rate(2, 1), rate(3, 2)] {
        let b = bank(1500, vec![0., 1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11.]);
        let p = PreparedPracticeProgram::new_with_speed(
            &b,
            vec![cue(1, 1, 0), cue(2, 1, 1_100_000), cue(3, 1, 2_100_000)],
            limits(16, 128),
            selected,
            Duration::ZERO,
            false,
        )
        .unwrap();
        let before = p.source_selection(VoiceId(1), ns(1_100_000)).unwrap();
        assert_eq!(
            (
                before.frame,
                before.requested_elapsed_nanos,
                before.applied_elapsed_nanos,
                before.correction_nanos
            ),
            (2, 1_100_000, 1_333_334, 233_334)
        );
        let equal = p.source_selection(VoiceId(2), ns(1_100_000)).unwrap();
        assert_eq!((equal.frame, equal.correction_nanos), (0, 0));
        assert_eq!(
            p.source_selection(VoiceId(3), ns(1_100_000)),
            Err(PracticeError::InvalidRegion)
        );
        let (mut controller, endpoint) = practice_queue(&p).unwrap();
        let (_producer, consumer) = command_queue(16).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                AudioFormat::new(1000, 1).unwrap(),
                ClockDomainId(9),
                ns(0),
                AudioLimits::new(16, 8, 16, 256, 16).unwrap(),
            ),
            b,
            consumer,
        )
        .unwrap();
        mixer
            .install_practice(p, endpoint, region(1_100_000, 8_000_000, false))
            .unwrap();
        let mut one = [99.];
        render(&mut mixer, &mut one);
        assert_eq!(one, [2. / 128.]);
        assert_eq!(
            controller.try_pop_receipt().unwrap().applied_song_time,
            ns(1_100_000)
        );
    }
}

#[test]
fn non_grid_loops_select_original_heads_without_rounding_accumulation() {
    for (selected, lap, expected) in [
        (rate(1, 2), 5usize, vec![2., 2.75, 3.5, 4.25, 5.]),
        (Rate::NORMAL, 3, vec![2., 3.5, 5.]),
        (rate(2, 1), 2, vec![2., 5.]),
        (rate(3, 2), 2, vec![2., 4.25]),
    ] {
        let (mut mixer, _producer, mut controller) = rig(
            1500,
            (0..20).map(|n| n as f32).collect(),
            vec![cue(1, 1, 0)],
            selected,
            0,
            region(1_100_000, 3_400_000, true),
            limits(256, 1024),
            false,
        );
        let mut frame = [99.];
        for pass in 0..40u64 {
            for value in &expected {
                render(&mut mixer, &mut frame);
                close(&frame, &[*value]);
            }
            let r = controller.try_pop_receipt().unwrap();
            assert_eq!(
                (
                    r.generation,
                    r.iteration,
                    r.physical_frame,
                    r.playback_frame
                ),
                (pass + 1, pass, pass * lap as u64, pass * lap as u64)
            );
            assert_eq!(r.applied_song_time, ns(1_100_000));
            assert_eq!(r.correction_nanos, 0);
            assert_eq!(
                r.kind,
                if pass == 0 {
                    PracticeBoundaryKind::Started
                } else {
                    PracticeBoundaryKind::Looped
                }
            );
            assert_eq!(mixer.frame_cursor(), (pass + 1) * lap as u64);
            assert_eq!(mixer.playback_frame_cursor(), (pass + 1) * lap as u64);
        }
        assert_eq!(controller.try_pop_receipt(), Err(PracticeError::Empty));
    }
}

#[test]
fn scrub_and_disable_keep_current_speed_original_heads_and_real_end_receipt() {
    let (mut mixer, _producer, mut c) = rig(
        1000,
        vec![1., 2., 3., 4., 5., 6., 7., 8.],
        vec![cue(1, 1, 0)],
        rate(3, 2),
        0,
        region(0, 4_000_000, true),
        limits(32, 128),
        false,
    );
    render(&mut mixer, &mut [0.]);
    c.try_pop_receipt().unwrap();
    c.try_request(PracticeRequest {
        id: 1,
        expected_generation: 1,
        at_playback_frame: 1,
        region: region(2_000_000, 5_100_000, true),
    })
    .unwrap();
    let mut one = [0.];
    render(&mut mixer, &mut one);
    assert_eq!(one, [3. / 128.]);
    let r = c.try_pop_receipt().unwrap();
    assert_eq!(
        (r.request_id, r.generation, r.kind, r.playback_frame),
        (1, 2, PracticeBoundaryKind::Requested, 1)
    );
    c.try_disable_loop(2, 2, 2).unwrap();
    let mut rest = [99.; 4];
    render(&mut mixer, &mut rest);
    close(&rest, &[4.5, 6., 0., 0.]);
    let disabled = c.try_pop_receipt().unwrap();
    let ended = c.try_pop_receipt().unwrap();
    assert_eq!(
        (disabled.kind, disabled.generation, disabled.playback_frame),
        (PracticeBoundaryKind::LoopDisabled, 2, 2)
    );
    assert_eq!(
        (ended.kind, ended.generation, ended.playback_frame),
        (PracticeBoundaryKind::Ended, 3, 4)
    );
    assert_eq!(mixer.rate(), rate(3, 2));
    assert!(mixer.practice_finished());
}

#[test]
fn stale_revision_reports_original_correlated_refusal_without_an_extra_commit() {
    let (mut mixer, _producer, mut c) = rig(
        1000,
        vec![1., 2., 3., 4., 5., 6.],
        vec![cue(1, 1, 0)],
        rate(2, 1),
        0,
        region(0, 4_000_000, true),
        limits(16, 128),
        false,
    );
    c.try_request_next(1, 1, region(2_000_000, 6_000_000, true))
        .unwrap();
    c.try_request_next(2, 1, region(0, 2_000_000, true))
        .unwrap();
    c.try_disable_loop_next(3, 1).unwrap();
    let mut out = [0.; 2];
    render(&mut mixer, &mut out);
    assert_eq!(out, [3. / 128., 5. / 128.]);
    let applied = c.try_pop_receipt().unwrap();
    assert_eq!(
        (applied.request_id, applied.generation, applied.kind),
        (1, 2, PracticeBoundaryKind::Requested)
    );
    for id in [2, 3] {
        let refused = c.try_pop_receipt().unwrap();
        assert_eq!(
            (refused.request_id, refused.generation, refused.kind),
            (id, 2, PracticeBoundaryKind::ControlRejectedRevision)
        );
    }
    assert_eq!(
        c.try_request_next(4, 1, region(0, 2_000_000, true)),
        Err(PracticeError::StaleGeneration)
    );
    assert_eq!(mixer.practice_generation(), Some(2));
}

#[test]
fn pressure_refusal_preserves_pcm_cursors_controls_and_actual_command_credits() {
    for (receipts, events, error) in [
        (1, 128, AudioError::PracticeReceiptFull),
        (16, 1, AudioError::PracticeEventBudget),
    ] {
        let (mut mixer, mut producer, mut c) = rig(
            1000,
            vec![1.; 8],
            vec![cue(1, 1, 0)],
            rate(2, 1),
            0,
            region(0, 1_000_000, true),
            limits(receipts, events),
            false,
        );
        producer
            .try_push_scoped(
                CommandScope(1),
                AudioCommand::Play {
                    voice: VoiceId(100),
                    sample: SampleId(2),
                    at: ns(0),
                    gain: 1.0,
                },
            )
            .unwrap();
        let before = mixer.counters();
        let mut untouched = [99.; 2];
        let (result, allocations) = measured(|| mixer.render(&mut untouched));
        assert_eq!(allocations, [0, 0, 0]);
        assert_eq!(result, Err(error));
        assert_eq!(untouched, [99.; 2]);
        assert_eq!(mixer.counters(), before);
        assert_eq!(
            (
                mixer.frame_cursor(),
                mixer.playback_frame_cursor(),
                mixer.practice_generation()
            ),
            (0, 0, Some(1))
        );
        assert_eq!(c.try_pop_receipt(), Err(PracticeError::Empty));
        if receipts == 1 {
            let mut one = [99.];
            let report = render(&mut mixer, &mut one);
            assert_eq!(one, [11. / 128.]);
            assert_eq!(report.counters.commands_consumed, 1);
            assert_eq!(report.counters.commands_applied, 1);
            assert_eq!(c.try_pop_receipt().unwrap().generation, 1);
        }
    }
    let (mut mixer, _producer, mut c) = rig(
        1000,
        vec![1.; 8],
        vec![cue(1, 1, 0)],
        rate(1, 2),
        0,
        region(0, 2_000_000, true),
        PracticeLimits::new(16, 4, 128, 1, 16).unwrap(),
        false,
    );
    c.try_request_next(1, 1, region(1_000_000, 3_000_000, true))
        .unwrap();
    assert_eq!(
        c.try_request_next(2, 1, region(0, 2_000_000, true)),
        Err(PracticeError::Full)
    );
    let mut one = [0.];
    render(&mut mixer, &mut one);
    assert_eq!(one, [1. / 128.]);
    assert_eq!(c.try_pop_receipt().unwrap().request_id, 1);
}

#[test]
fn actual_pause_silence_freezes_bgm_keysound_and_playback_then_resumes_selected_rate() {
    let (mut mixer, mut p, mut c) = rig(
        1000,
        vec![1., 2., 3., 4., 5., 6.],
        vec![cue(1, 1, 0)],
        rate(1, 2),
        0,
        region(0, 2_000_000, true),
        limits(32, 128),
        false,
    );
    p.try_push_scoped(
        CommandScope(1),
        AudioCommand::Play {
            voice: VoiceId(100),
            sample: SampleId(2),
            at: ns(0),
            gain: 1.0,
        },
    )
    .unwrap();
    let mut one = [0.];
    render(&mut mixer, &mut one);
    assert_eq!(one, [11. / 128.]);
    c.try_pop_receipt().unwrap();
    p.request_pause(true);
    let mut silence = [99.; 3];
    let held = render(&mut mixer, &mut silence);
    assert_eq!(silence, [0.; 3]);
    assert_eq!(held.playback_frames, 0);
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (4, 1)
    );
    assert_eq!(c.try_pop_receipt(), Err(PracticeError::Empty));
    p.request_pause(false);
    render(&mut mixer, &mut one);
    assert_eq!(one, [16.5 / 128.]);
    assert_eq!(mixer.rate(), rate(1, 2));
    render(&mut mixer, &mut [0.; 3]);
    let looped = c.try_pop_receipt().unwrap();
    assert_eq!(
        (
            looped.physical_frame,
            looped.playback_frame,
            looped.generation
        ),
        (7, 4, 2)
    );
}

#[test]
fn installed_arbitrary_rate_seek_and_oversized_render_refuse_without_relabeling_program() {
    let (mut mixer, mut p, mut c) = rig(
        1000,
        vec![1., 2., 3., 4.],
        vec![cue(1, 1, 0)],
        rate(1, 2),
        0,
        region(0, 4_000_000, false),
        limits(16, 128),
        false,
    );
    p.try_push(AudioCommand::SetRate {
        rate: rate(2, 1),
        at: ns(0),
    })
    .unwrap();
    p.try_push(AudioCommand::Seek {
        song_time: ns(3_000_000),
        at: ns(0),
    })
    .unwrap();
    let mut out = [0.; 2];
    let report = render(&mut mixer, &mut out);
    close(&out, &[1., 1.5]);
    assert_eq!(report.counters.commands_consumed, 2);
    assert_eq!(report.counters.commands_applied, 0);
    assert_eq!(mixer.rate(), rate(1, 2));
    c.try_pop_receipt().unwrap();
    let before = mixer.render(&mut []).unwrap();
    let mut too_large = [99.; 257];
    let (result, allocations) = measured(|| mixer.render(&mut too_large));
    assert_eq!(allocations, [0, 0, 0]);
    assert_eq!(result, Err(AudioError::RenderCapacity));
    assert_eq!(too_large, [99.; 257]);
    assert_eq!(mixer.render(&mut []).unwrap(), before);
}

#[test]
fn slow_conceptual_open_end_accepts_and_plays_real_final_cue_tail() {
    let (mut mixer, _producer, mut c) = rig(
        1000,
        vec![1., 2., 3., 4.],
        vec![cue(1, 1, 1_000_000)],
        rate(1, 2),
        0,
        region(0, i64::MAX, false),
        limits(16, 128),
        true,
    );
    let mut out = [99.; 12];
    let report = render(&mut mixer, &mut out);
    close(&out, &[0., 0., 1., 1.5, 2., 2.5, 3., 3.5, 4., 4., 0., 0.]);
    assert_eq!(report.active_voices, 0);
    assert!(!mixer.practice_finished());
    assert_eq!(
        c.try_pop_receipt().unwrap().kind,
        PracticeBoundaryKind::Started
    );
    assert_eq!(c.try_pop_receipt(), Err(PracticeError::Empty));
}

#[test]
fn cold_nonpositive_negative_preroll_and_explicit_finite_overflow_refuse_checked() {
    let b = bank(1000, vec![1., 2., 3.]);
    for selected in [Rate::ZERO, Rate::REVERSE] {
        assert!(PreparedPracticeProgram::new_with_speed(
            &b,
            vec![cue(1, 1, 0)],
            limits(16, 128),
            selected,
            Duration::ZERO,
            false
        )
        .is_err());
    }
    assert!(PreparedPracticeProgram::new_with_speed(
        &b,
        vec![cue(1, 1, 0)],
        limits(16, 128),
        Rate::NORMAL,
        Duration::from_nanos(-1),
        false
    )
    .is_err());
    let finite = PreparedPracticeProgram::new_with_speed(
        &b,
        vec![cue(1, 1, 0)],
        limits(16, 128),
        rate(1, 2),
        Duration::ZERO,
        false,
    )
    .unwrap();
    assert_eq!(finite.validate_region(region(0,i64::MAX,false)),Ok(()),"a relative duration larger than i64 still fits the u64 frame domain; absolute Mixer origin decides representability");
    let extreme = PreparedPracticeProgram::new_with_speed(
        &b,
        vec![cue(1, 1, 0)],
        limits(16, 128),
        rate(1, u64::MAX),
        Duration::ZERO,
        false,
    )
    .unwrap();
    assert!(
        extreme
            .validate_region(region(i64::MIN, i64::MAX, false))
            .is_err(),
        "checked products cannot wrap into a short pass"
    );
    let phase = PreparedPracticeProgram::new_with_speed(
        &b,
        vec![cue(1, 1, 0)],
        limits(16, 128),
        rate(i64::MAX, 1),
        Duration::MAX,
        false,
    )
    .unwrap();
    assert!(
        phase.validate_region(region(0, 1_000_000, false)).is_err(),
        "unrepresentable initial song phase cannot saturate"
    );
    // u32 Hz × positive i64/u64 rate is representable by the existing rational
    // head. The largest denominator itself must not be spuriously rejected.
    let (mut mixer, _p, mut c) = rig(
        1000,
        vec![1., 2., 3.],
        vec![cue(1, 1, 0)],
        rate(1, u64::MAX),
        0,
        region(0, i64::MAX, false),
        limits(16, 128),
        true,
    );
    let mut out = [99.; 3];
    render(&mut mixer, &mut out);
    assert_eq!(out, [1. / 128.; 3]);
    assert_eq!(c.try_pop_receipt().unwrap().generation, 1);
}

#[test]
fn finite_max_half_speed_checks_actual_absolute_origin_before_install_mutation() {
    fn prepared() -> (
        SampleBank,
        PreparedPracticeProgram,
        PracticeController,
        PracticeAudioEndpoint,
    ) {
        // One ns per physical sample avoids unrelated Ceil grid overshoot.
        let f = AudioFormat::new(1_000_000_000, 1).unwrap();
        let l = PcmLimits::new(64, 64, 1).unwrap();
        let mut b = SampleBank::new(f, l).unwrap();
        b.insert(
            SampleId(1),
            PcmSample::new(f, vec![0.125, 0.25, 0.375], l).unwrap(),
        )
        .unwrap();
        let p = PreparedPracticeProgram::new_with_speed(
            &b,
            vec![cue(1, 1, 0)],
            PracticeLimits::new(1, 1, 32, 4, 8).unwrap(),
            rate(1, 2),
            Duration::ZERO,
            false,
        )
        .unwrap();
        assert_eq!(p.validate_region(region(0, i64::MAX, false)), Ok(()));
        let (controller, endpoint) = practice_queue(&p).unwrap();
        (b, p, controller, endpoint)
    }
    fn mixer(b: SampleBank, origin: i64) -> (Mixer, CommandProducer) {
        let (producer, consumer) = command_queue(4).unwrap();
        (
            Mixer::new(
                MixerConfig::new(
                    AudioFormat::new(1_000_000_000, 1).unwrap(),
                    ClockDomainId(9),
                    ns(origin),
                    AudioLimits::new(4, 2, 4, 16, 4).unwrap(),
                ),
                b,
                consumer,
            )
            .unwrap(),
            producer,
        )
    }
    let (b, p, mut c, endpoint) = prepared();
    let (mut m, _producer) = mixer(b, 0);
    let before = m.render(&mut []).unwrap();
    let config = m.configuration();
    assert!(m
        .install_practice(p, endpoint, region(0, i64::MAX, false))
        .is_err());
    assert_eq!(m.render(&mut []).unwrap(), before);
    assert_eq!(m.configuration(), config);
    assert_eq!(m.practice_generation(), None);
    assert_eq!(m.rate(), Rate::NORMAL);
    assert!(matches!(
        c.try_pop_receipt(),
        Err(PracticeError::Empty) | Err(PracticeError::Disconnected)
    ));
    // A failed cold install leaves this same owner usable for a valid install.
    let f = AudioFormat::new(1_000_000_000, 1).unwrap();
    let l = PcmLimits::new(64, 64, 1).unwrap();
    let mut matching = SampleBank::new(f, l).unwrap();
    matching
        .insert(
            SampleId(1),
            PcmSample::new(f, vec![0.125, 0.25, 0.375], l).unwrap(),
        )
        .unwrap();
    let short = PreparedPracticeProgram::new_with_speed(
        &matching,
        vec![cue(1, 1, 0)],
        PracticeLimits::new(1, 1, 32, 4, 8).unwrap(),
        rate(1, 2),
        Duration::ZERO,
        false,
    )
    .unwrap();
    let (mut short_controller, short_endpoint) = practice_queue(&short).unwrap();
    m.install_practice(short, short_endpoint, region(0, 2, false))
        .unwrap();
    let mut actual = [99.; 3];
    render(&mut m, &mut actual);
    assert_eq!(actual, [0.125, 0.1875, 0.25]);
    assert_eq!(short_controller.try_pop_receipt().unwrap().generation, 1);
    // Exact widened arithmetic: MIN + 2*MAX = MAX-1, so this absolute
    // endpoint is representable even though the relative duration is not i64.
    let (b, p, mut c, endpoint) = prepared();
    let (mut m, _producer) = mixer(b, i64::MIN);
    m.install_practice(p, endpoint, region(0, i64::MAX, false))
        .unwrap();
    let mut out = [99.; 3];
    let report = render(&mut m, &mut out);
    assert_eq!(out, [0.125, 0.1875, 0.25]);
    assert_eq!(report.counters.commands_consumed, 0);
    let actual = c.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (
            actual.receipt.generation,
            actual.receipt.physical_frame,
            actual.receipt.playback_frame,
            actual.target_frame,
            actual.target_rate
        ),
        (1, 0, 0, 0, 1_000_000_000)
    );
    assert_eq!(actual.origin.domain, ClockDomainId(9));
    assert_eq!(actual.origin.timestamp, ns(i64::MIN));
    assert_eq!(actual.boundary.source_frame, 0);
    assert_eq!(
        actual.boundary.target_time,
        TargetTime::from_frames(0, 1_000_000_000).unwrap()
    );
    assert_eq!(actual.receipt.applied_song_time, ns(0));
    assert!(!m.practice_finished());
}

#[test]
fn actual_nonzero_cut_control_horizon_overflow_preserves_pcm_fifo_and_credits() {
    let f = AudioFormat::new(1_000_000_000, 1).unwrap();
    let l = PcmLimits::new(64, 64, 1).unwrap();
    let mut b = SampleBank::new(f, l).unwrap();
    b.insert(
        SampleId(1),
        PcmSample::new(f, vec![0.125, 0.25, 0.375], l).unwrap(),
    )
    .unwrap();
    let p = PreparedPracticeProgram::new_with_speed(
        &b,
        vec![cue(1, 1, 0)],
        PracticeLimits::new(1, 1, 32, 4, 8).unwrap(),
        rate(1, 2),
        Duration::ZERO,
        false,
    )
    .unwrap();
    let (mut c, endpoint) = practice_queue(&p).unwrap();
    let (mut producer, consumer) = command_queue(4).unwrap();
    let mut m = Mixer::new(
        MixerConfig::new(
            f,
            ClockDomainId(9),
            ns(i64::MAX - 10),
            AudioLimits::new(4, 2, 4, 16, 4).unwrap(),
        ),
        b,
        consumer,
    )
    .unwrap();
    m.install_practice(p, endpoint, region(0, 2, false))
        .unwrap();
    let mut one = [99.];
    render(&mut m, &mut one);
    assert_eq!(one, [0.125]);
    c.try_pop_receipt().unwrap();
    c.try_request(PracticeRequest {
        id: 1,
        expected_generation: 1,
        at_playback_frame: 1,
        region: region(0, i64::MAX, false),
    })
    .unwrap();
    producer
        .try_push_scoped(
            CommandScope(1),
            AudioCommand::Play {
                voice: VoiceId(100),
                sample: SampleId(1),
                at: ns(i64::MAX - 9),
                gain: 1.0,
            },
        )
        .unwrap();
    let before = m.render(&mut []).unwrap();
    let cursor = (
        m.frame_cursor(),
        m.playback_frame_cursor(),
        m.practice_generation(),
        c.generation(),
    );
    let mut unchanged = [99.; 2];
    let (result, allocations) = measured(|| m.render(&mut unchanged));
    assert_eq!(allocations, [0, 0, 0]);
    assert_eq!(
        result,
        Err(AudioError::PracticeControl),
        "actual cut plus finite duration exceeds output ClockPoint domain"
    );
    assert_eq!(unchanged, [99.; 2]);
    assert_eq!(m.render(&mut []).unwrap(), before);
    assert_eq!(
        (
            m.frame_cursor(),
            m.playback_frame_cursor(),
            m.practice_generation(),
            c.generation()
        ),
        cursor
    );
    assert_eq!(c.try_pop_receipt(), Err(PracticeError::Empty));
    // There is no public cancellation of an admitted FIFO head. Repeating the
    // refused callback must retain that head and the unconsumed audio command.
    let (again, allocations) = measured(|| m.render(&mut unchanged));
    assert_eq!(allocations, [0, 0, 0]);
    assert_eq!(again, Err(AudioError::PracticeControl));
    assert_eq!(m.render(&mut []).unwrap(), before);
    assert_eq!(unchanged, [99.; 2]);
    producer.request_pause(true);
    let paused = render(&mut m, &mut unchanged);
    assert_eq!(unchanged, [0.; 2]);
    assert_eq!(paused.playback_frames, 0);
    let mut expected_paused_counters = before.counters;
    expected_paused_counters.rendered_frames = expected_paused_counters
        .rendered_frames
        .checked_add(2)
        .unwrap();
    assert_eq!(paused.counters, expected_paused_counters);
    assert_eq!(m.playback_frame_cursor(), 1);
    producer.request_pause(false);
    unchanged.fill(99.);
    let frozen = m.render(&mut []).unwrap();
    assert_eq!(m.render(&mut unchanged), Err(AudioError::PracticeControl));
    assert_eq!(unchanged, [99.; 2]);
    assert_eq!(m.render(&mut []).unwrap(), frozen);
}

#[test]
fn control_admission_and_many_backward_resets_are_allocation_free_on_original_owner() {
    let (mut mixer, _producer, mut c) = rig(
        1000,
        vec![1., 2., 3., 4., 5., 6.],
        vec![cue(1, 1, 0)],
        rate(3, 2),
        0,
        region(0, 4_000_000, true),
        limits(32, 128),
        false,
    );
    let mut one = [0.];
    render(&mut mixer, &mut one);
    c.try_pop_receipt().unwrap();
    for id in 1..=40u64 {
        let generation = c.generation();
        let (result, allocations) =
            measured(|| c.try_request_next(id, generation, region(1_000_000, 4_100_000, true)));
        assert_eq!(allocations, [0, 0, 0]);
        result.unwrap();
        render(&mut mixer, &mut one);
        assert_eq!(one, [2. / 128.]);
        let receipt = c.try_pop_receipt().unwrap();
        assert_eq!(
            (
                receipt.request_id,
                receipt.generation,
                receipt.physical_frame,
                receipt.playback_frame
            ),
            (id, id + 1, id, id)
        );
        assert_eq!(receipt.kind, PracticeBoundaryKind::Requested);
        assert_eq!(
            (
                receipt.requested_song_time,
                receipt.applied_song_time,
                receipt.correction_nanos
            ),
            (ns(1_000_000), ns(1_000_000), 0)
        );
        assert_eq!(mixer.rate(), rate(3, 2));
    }
}

#[test]
fn finite_exclusive_cue_never_executes_at_or_after_non_grid_fence_at_all_rates() {
    for (selected, expected, fence) in [
        (rate(1, 2), vec![1., 1.5, 2., 2.5, 3., 0.], 5),
        (Rate::NORMAL, vec![1., 2., 3., 0., 0., 0.], 3),
        (rate(2, 1), vec![1., 3., 0., 0., 0., 0.], 2),
        (rate(3, 2), vec![1., 2.5, 0., 0., 0., 0.], 2),
    ] {
        let (mut mixer, _producer, mut c) = rig(
            1000,
            vec![1., 2., 3., 4., 5.],
            vec![cue(1, 1, 0), cue(2, 2, 2_100_001), cue(3, 2, 2_200_000)],
            selected,
            0,
            region(0, 2_100_001, false),
            limits(16, 128),
            false,
        );
        let mut out = [99.; 6];
        render(&mut mixer, &mut out);
        close(&out, &expected);
        c.try_pop_receipt().unwrap();
        let ended = c.try_pop_receipt().unwrap();
        assert_eq!(
            (ended.kind, ended.physical_frame, ended.playback_frame),
            (PracticeBoundaryKind::Ended, fence, fence)
        );
        assert_eq!(c.try_pop_receipt(), Err(PracticeError::Empty));
        assert_eq!(
            mixer.counters().commands_applied,
            0,
            "program cues are not fake producer credits"
        );
    }
}

#[test]
fn mixed_hz_discrete_overlap_capacity_accepts_exact_bound_and_refuses_one_less() {
    let b = bank(1500, vec![1., 2.]);
    // At 1/2 speed source step=3/4; each voice lives three output
    // frames. With original anchor0.2ms cues ceil to frames1 and3,
    // overlapping at frame3 despite disjoint mathematical source lifetimes.
    let cues = vec![cue(1, 1, 250_000), cue(2, 1, 1_650_000)];
    assert!(matches!(
        PreparedPracticeProgram::new_with_speed(
            &b,
            cues.clone(),
            PracticeLimits::new(2, 1, 128, 8, 16).unwrap(),
            rate(1, 2),
            Duration::ZERO,
            false
        ),
        Err(PracticeError::Capacity)
    ));
    let p = PreparedPracticeProgram::new_with_speed(
        &b,
        cues,
        PracticeLimits::new(2, 2, 128, 8, 16).unwrap(),
        rate(1, 2),
        Duration::ZERO,
        false,
    )
    .unwrap();
    let (mut c, endpoint) = practice_queue(&p).unwrap();
    let (_producer, consumer) = command_queue(16).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            AudioFormat::new(1000, 1).unwrap(),
            ClockDomainId(9),
            ns(0),
            AudioLimits::new(16, 2, 16, 256, 16).unwrap(),
        ),
        b,
        consumer,
    )
    .unwrap();
    mixer
        .install_practice(p, endpoint, region(200_000, 5_000_000, false))
        .unwrap();
    let mut out = [99.; 7];
    render(&mut mixer, &mut out);
    close(&out, &[0., 1., 1.75, 3., 1.75, 2., 0.]);
    assert_eq!(c.try_pop_receipt().unwrap().generation, 1);
    assert_eq!(mixer.counters().voice_full, 0);
}

#[test]
fn public_bgm_extent_has_literal_discrete_envelopes_and_typed_refusals() {
    // A one-frame 48kHz asset occupies one output frame at all these rates.
    // The envelope is expressed back in original song coordinates, including
    // the whole output frame; the fractional-speed extent is not 20,834ns.
    for (selected, extent) in [
        (Rate::NORMAL, 20_834),
        (rate(3, 2), 31_250),
        (rate(2, 1), 41_667),
        (rate(1, 2), 20_834),
    ] {
        assert_eq!(
            practice_bgm_voice_extent(1, 48_000, 48_000, selected),
            Ok(extent)
        );
        assert_eq!(
            practice_bgm_voice_extent(0, 48_000, 48_000, selected),
            Ok(0)
        );
    }
    // The existing mixed-Hz fixture above has three physical output frames
    // per two-frame source voice at half speed: a 1.5ms original envelope.
    assert_eq!(
        practice_bgm_voice_extent(2, 1500, 1000, rate(1, 2)),
        Ok(1_500_000)
    );
    for (source_hz, output_hz, selected) in [
        (0, 48_000, Rate::NORMAL),
        (48_000, 0, Rate::NORMAL),
        (48_000, 48_000, Rate::ZERO),
        (48_000, 48_000, Rate::REVERSE),
    ] {
        for frames in [0, 1] {
            assert_eq!(
                practice_bgm_voice_extent(frames, source_hz, output_hz, selected),
                Err(PracticeError::InvalidRegion)
            );
        }
    }
}

#[test]
fn real_converted_projection_is_partition_invariant_and_held_pcm_does_not_advance_source() {
    fn run(parts: &[usize]) -> (Vec<f32>, Vec<(u64, u64, u64, u32)>, SourcePosition) {
        let (mixer, _producer, mut c) = rig(
            1000,
            vec![1., 2., 3., 4., 5.],
            vec![cue(1, 1, 0)],
            rate(3, 2),
            0,
            region(0, 3_100_000, true),
            limits(64, 256),
            false,
        );
        let mut converted = ConvertedMixer::new(
            mixer,
            AudioFormat::new(1500, 1).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            32,
        )
        .unwrap();
        let mut out = Vec::new();
        for n in parts {
            let mut pcm = vec![99.; *n];
            let (result, counts) = measured(|| converted.render(&mut pcm));
            assert_eq!(counts, [0, 0, 0]);
            result.unwrap();
            out.extend(pcm);
        }
        let before = converted.source_position();
        let mut held = [99.; 3];
        let (result, counts) = measured(|| converted.render_held(&mut held));
        assert_eq!(counts, [0, 0, 0]);
        result.unwrap();
        assert_eq!(held, [0.; 3]);
        assert_eq!(converted.source_position(), before);
        let mut receipts = Vec::new();
        while let Ok(p) = c.try_pop_projected_receipt() {
            receipts.push((
                p.receipt.generation,
                p.receipt.physical_frame,
                p.target_frame,
                p.target_rate,
            ));
        }
        (out, receipts, before)
    }
    let a = run(&[18]);
    let b = run(&[1, 2, 1, 5, 2, 7]);
    assert_eq!(a, b);
    close(&a.0[..9], &[1., 2., 3., 4., 2., 1.5, 2.5, 3.5, 3.]);
    for (generation, source, target, hz) in a.1 {
        assert_eq!(source, (generation - 1) * 3);
        assert_eq!(target, (source * 3 + 1) / 2);
        assert_eq!(hz, 1500);
    }
}
