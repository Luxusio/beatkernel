//! Actual source callbacks establish playback history; none establish native ACKs.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}
struct Allocator;
fn count(i: usize) {
    let _ = TRACK.try_with(|t| {
        if t.get() {
            let _ = COUNTS.try_with(|c| {
                let mut v = c.get();
                v[i] += 1;
                c.set(v);
            });
        }
    });
}
// SAFETY: Arguments and pointers pass unchanged to System; instrumentation is scalar TLS.
unsafe impl GlobalAlloc for Allocator {
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
static ALLOCATOR: Allocator = Allocator;
fn tracked<T>(f: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|c| c.set([0; 3]));
    TRACK.with(|t| t.set(true));
    let r = f();
    TRACK.with(|t| t.set(false));
    (r, COUNTS.with(Cell::get))
}
fn format(hz: u32) -> AudioFormat {
    AudioFormat::new(hz, 1).unwrap()
}
fn time(n: u64, d: u64) -> TargetTime {
    TargetTime::new(n / d, n % d, d).unwrap()
}
fn query(t: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(t, 0, 0, 1, 0, 1).unwrap()
}
fn rig(
    source: u32,
    target: u32,
    gated: bool,
    end: Option<u64>,
    prefix: usize,
) -> (CommandProducer, ConvertedMixer) {
    let limits = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format(source), limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format(source), vec![0.25; 128], limits).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = if gated {
        command_queue_with_start_gate(8)
    } else {
        command_queue(8)
    }
    .unwrap();
    let mut config = MixerConfig::new(
        format(source),
        ClockDomainId(3),
        Timestamp::ZERO,
        AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    let mut mixer = Mixer::new(config, bank, consumer).unwrap();
    if prefix > 0 {
        mixer.render(&mut [0.0; 32][..prefix]).unwrap();
    }
    let owner = ConvertedMixer::new(
        mixer,
        format(target),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        32,
    )
    .unwrap_or_else(|f| panic!("fixture refused: {:?}", f.error()));
    (producer, owner)
}
fn opted(source: u32, target: u32, capacity: usize) -> (CommandProducer, ConvertedMixer) {
    let (p, mut o) = rig(source, target, false, None, 0);
    o.enable_phase_history(16).unwrap();
    o.enable_source_playback_history(capacity).unwrap();
    (p, o)
}
fn render(o: &mut ConvertedMixer, n: usize) -> ConvertedRenderReport {
    o.render(&mut [0.0; 32][..n]).unwrap()
}
fn spans(o: &ConvertedMixer) -> Vec<SourcePlaybackSpan> {
    let h = o.source_playback_history().unwrap();
    (0..h.len()).map(|i| h.span(i).unwrap()).collect()
}
fn assert_span(
    s: SourcePlaybackSpan,
    start: u64,
    end: Option<u64>,
    playback: u64,
    state: SourcePlaybackState,
) {
    assert_eq!(
        (
            s.physical_start(),
            s.physical_end(),
            s.playback_start(),
            s.state()
        ),
        (start, end, playback, state)
    );
}
fn play(p: &mut CommandProducer) {
    p.try_push(AudioCommand::Play {
        voice: VoiceId(1),
        sample: SampleId(1),
        at: Timestamp::ZERO,
        gain: 1.0,
    })
    .unwrap();
}

#[test]
fn cold_opt_in_has_no_fabricated_interval_and_empty_callbacks_leave_it_empty() {
    let (_, mut o) = rig(44_100, 48_000, false, None, 0);
    assert!(o.source_playback_history().is_none());
    assert!(o.enable_source_playback_history(0).is_err());
    assert!(o.source_playback_history().is_none());
    let (r, a) = tracked(|| o.enable_source_playback_history(3));
    r.unwrap();
    assert!(a[0] > 0);
    assert_eq!(a[1], 0);
    assert!(o.enable_source_playback_history(3).is_err());
    let r = render(&mut o, 0);
    assert_eq!(r.source.unwrap().frames, 0);
    let h = o.source_playback_history().unwrap();
    assert_eq!(
        (
            h.source_rate(),
            h.initial_physical_frame(),
            h.initial_playback_frame(),
            h.pulled_end_frame()
        ),
        (44_100, 0, 0, 0)
    );
    assert!(h.is_empty());
    assert_eq!(h.len(), 0);
    assert!(h.capacity() >= 3);
    assert!(h.span(0).is_none());
}

#[test]
fn late_opt_in_refuses_after_active_or_target_only_held_generation() {
    for held in [false, true] {
        let (_, mut o) = rig(1000, 1000, false, None, 0);
        if held {
            o.render_held(&mut [0.0; 2]).unwrap();
        } else {
            render(&mut o, 2);
        }
        let before = (
            o.target_time(),
            o.source_position(),
            o.pulled_source_frame_cursor(),
        );
        assert!(o.enable_source_playback_history(4).is_err());
        assert!(o.source_playback_history().is_none());
        assert_eq!(
            (
                o.target_time(),
                o.source_position(),
                o.pulled_source_frame_cursor()
            ),
            before
        );
    }
}

#[test]
fn initial_nonzero_mixer_cursor_is_retained_without_fabricating_old_history() {
    let (_, mut o) = rig(1000, 1000, false, None, 5);
    o.enable_source_playback_history(2).unwrap();
    let h = o.source_playback_history().unwrap();
    assert_eq!(
        (
            h.initial_physical_frame(),
            h.initial_playback_frame(),
            h.pulled_end_frame()
        ),
        (5, 5, 5)
    );
    assert!(h.is_empty());
    render(&mut o, 2);
    assert_span(
        o.source_playback_history().unwrap().span(0).unwrap(),
        5,
        None,
        5,
        SourcePlaybackState::Running,
    );
}

#[test]
fn one_actual_callback_retains_startup_running_and_finite_tail_despite_final_paused_report() {
    let (mut p, mut o) = rig(1000, 1000, true, Some(3), 0);
    p.schedule_start_at(2).unwrap();
    play(&mut p);
    o.enable_source_playback_history(3).unwrap();
    let report = render(&mut o, 8);
    let source = report.source.unwrap();
    assert_eq!(source.frames, 9);
    assert_eq!(source.playback_frames, 3);
    assert!(source.paused);
    assert_eq!(p.applied_start_frame(), Some(2));
    assert_eq!(source.counters.commands_consumed, 1);
    let h = o.source_playback_history().unwrap();
    assert_eq!(h.len(), 3);
    assert_eq!(h.pulled_end_frame(), 9);
    assert_span(
        h.span(0).unwrap(),
        0,
        Some(2),
        0,
        SourcePlaybackState::Frozen,
    );
    assert_span(
        h.span(1).unwrap(),
        2,
        Some(5),
        0,
        SourcePlaybackState::Running,
    );
    assert_span(h.span(2).unwrap(), 5, None, 3, SourcePlaybackState::Frozen);
    render(&mut o, 8);
    assert_eq!(o.source_playback_history().unwrap().len(), 3);
}

#[test]
fn exact_same_block_capacity_refusal_preserves_pcm_commands_gate_and_all_cursors() {
    let (mut p, mut o) = rig(1000, 1000, true, Some(3), 0);
    p.schedule_start_at(2).unwrap();
    play(&mut p);
    o.enable_phase_history(4).unwrap();
    o.enable_source_playback_history(2).unwrap();
    let before = (
        o.target_time(),
        o.source_position(),
        o.target_frame_cursor(),
        o.pulled_source_frame_cursor(),
        p.counters(),
    );
    let mut output = [99.0; 8];
    for _ in 0..2 {
        let (r, a) = tracked(|| o.render(&mut output));
        assert_eq!(r, Err(AudioError::RenderCapacity));
        assert_eq!(a, [0; 3]);
        assert_eq!(output, [99.0; 8]);
        assert_eq!(
            (
                o.target_time(),
                o.source_position(),
                o.target_frame_cursor(),
                o.pulled_source_frame_cursor(),
                p.counters()
            ),
            before
        );
        assert_eq!(p.applied_start_frame(), None);
        assert!(o.source_playback_history().unwrap().is_empty());
        assert!(o.phase_history().unwrap().is_empty());
    }
    // A smaller callback uses the same owners and queued command, proving no hidden drain.
    let first = render(&mut o, 3);
    assert_eq!(first.source.unwrap().counters.commands_consumed, 1);
    assert_eq!(p.applied_start_frame(), Some(2));
    assert_eq!(o.source_playback_history().unwrap().len(), 2);
}

#[test]
fn full_capacity_allows_ordinary_running_merge_and_refuses_new_pause_without_adoption() {
    let (mut p, mut o) = opted(1000, 1000, 1);
    play(&mut p);
    let first = render(&mut o, 4);
    assert_eq!(first.source.unwrap().counters.commands_consumed, 1);
    for _ in 0..8 {
        let (r, a) = tracked(|| o.render(&mut [0.0; 4]));
        r.unwrap();
        assert_eq!(a, [0; 3]);
    }
    assert_eq!(o.source_playback_history().unwrap().len(), 1);
    let before = (
        o.target_time(),
        o.source_position(),
        o.pulled_source_frame_cursor(),
        spans(&o),
    );
    p.request_pause(true);
    let mut output = [99.0; 4];
    assert_eq!(o.render(&mut output), Err(AudioError::RenderCapacity));
    assert_eq!(output, [99.0; 4]);
    assert_eq!(
        (
            o.target_time(),
            o.source_position(),
            o.pulled_source_frame_cursor(),
            spans(&o)
        ),
        before
    );
    p.request_pause(false);
    let next = render(&mut o, 4);
    assert!(!next.source.unwrap().paused);
    assert_eq!(o.source_playback_history().unwrap().len(), 1);
}

#[test]
fn repeated_actual_pause_resume_cycles_retain_historical_physical_and_playback_mapping() {
    let (mut p, mut o) = opted(1000, 1000, 5);
    render(&mut o, 4);
    p.request_pause(true);
    render(&mut o, 4);
    render(&mut o, 4);
    p.request_pause(false);
    render(&mut o, 4);
    p.request_pause(true);
    render(&mut o, 4);
    p.request_pause(false);
    let final_report = render(&mut o, 4);
    let h = o.source_playback_history().unwrap();
    assert_eq!(h.len(), 5);
    for (i, start, end, playback, state) in [
        (0, 0, Some(5), 0, SourcePlaybackState::Running),
        (1, 5, Some(13), 5, SourcePlaybackState::Frozen),
        (2, 13, Some(17), 5, SourcePlaybackState::Running),
        (3, 17, Some(21), 9, SourcePlaybackState::Frozen),
        (4, 21, None, 9, SourcePlaybackState::Running),
    ] {
        assert_span(h.span(i).unwrap(), start, end, playback, state);
    }
    let converted = o.phase_history().unwrap().span(0).unwrap();
    for (physical, playback) in [
        (4, 4),
        (5, 5),
        (12, 5),
        (13, 5),
        (16, 8),
        (17, 9),
        (21, 9),
        (24, 12),
    ] {
        let q = query(time(physical, 1000));
        let selected = h
            .lookup_at_interpolated_time(converted, q, final_report.target_end_time)
            .unwrap();
        assert_eq!(
            converted.playback_elapsed_nanos_at_interpolated_time(
                q,
                final_report.target_end_time,
                selected,
                0
            ),
            Ok(i128::from(playback) * 1_000_000)
        );
    }
}

#[test]
fn target_held_does_not_adopt_source_pause_and_complete_owner_retarget_preserves_history() {
    let (mut p, mut o) = opted(44_100, 48_000, 4);
    render(&mut o, 4);
    let before = (
        spans(&o),
        o.source_playback_history().unwrap().pulled_end_frame(),
    );
    p.request_pause(true);
    let held = o.render_held(&mut [1.0; 4]).unwrap();
    assert!(held.source.is_none());
    assert_eq!(
        (
            spans(&o),
            o.source_playback_history().unwrap().pulled_end_frame()
        ),
        before
    );
    o.retarget(
        format(32_000),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        32,
    )
    .unwrap();
    let mut moved = o;
    assert_eq!(spans(&moved), before.0);
    let actual = render(&mut moved, 4);
    assert!(actual.source.unwrap().paused);
    assert_eq!(moved.source_playback_history().unwrap().len(), 2);
    assert_eq!(
        moved.source_playback_history().unwrap().source_rate(),
        44_100
    );
}

#[test]
fn actual_pulled_frontier_is_inclusive_but_fractional_future_is_refused() {
    let (_, mut o) = opted(44_100, 8000, 2);
    let report = render(&mut o, 1);
    assert_eq!(report.pulled_source_frame_cursor, 5);
    assert_eq!(
        report.source_position,
        SourcePosition {
            frame: 5,
            numerator: 41,
            denominator: 80
        }
    );
    let converted = o.phase_history().unwrap().span(0).unwrap();
    let h = o.source_playback_history().unwrap();
    assert!(h
        .lookup_at_interpolated_time(converted, query(time(5, 44_100)), report.target_end_time)
        .is_ok());
    assert_eq!(
        h.lookup_at_interpolated_time(
            converted,
            query(report.target_end_time),
            report.target_end_time
        ),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    );
    let tiny =
        InterpolatedTargetTime::new(time(5, 44_100), 0, 1, u64::MAX, 1, u64::MAX - 1).unwrap();
    assert_eq!(
        h.lookup_at_interpolated_time(converted, tiny, report.target_end_time),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    );
    let later = render(&mut o, 1);
    assert!(o
        .source_playback_history()
        .unwrap()
        .lookup_at_interpolated_time(
            converted,
            query(report.target_end_time),
            later.target_end_time
        )
        .is_ok());
}

#[test]
fn tiny_exact_source_transition_selects_new_interval_before_any_nanos_floor() {
    let (mut p, mut o) = opted(44_100, 48_000, 3);
    let first = render(&mut o, 4);
    let adoption = first.pulled_source_frame_cursor;
    p.request_pause(true);
    let paused = render(&mut o, 4);
    let converted = o.phase_history().unwrap().span(0).unwrap();
    let h = o.source_playback_history().unwrap();
    for (offset, state) in [
        (-1, SourcePlaybackState::Running),
        (0, SourcePlaybackState::Frozen),
        (1, SourcePlaybackState::Frozen),
    ] {
        let q = InterpolatedTargetTime::new(
            time(adoption, 44_100),
            0,
            1,
            u64::MAX,
            offset,
            u64::MAX - 1,
        )
        .unwrap();
        let selected = h
            .lookup_at_interpolated_time(converted, q, paused.target_end_time)
            .unwrap();
        assert_eq!(selected.state(), state);
    }
}

#[test]
fn lookup_rejects_no_actual_span_wrong_source_rate_and_target_range_errors() {
    let (_, mut o) = opted(1000, 1000, 2);
    render(&mut o, 0);
    // Structural descriptors below are math-only and authenticate no generation or ACK.
    let span = ConvertedPhaseSpan::from_words([1000, 0, 0, 1, 0, 0, 1, 0]).unwrap();
    assert_eq!(
        o.source_playback_history()
            .unwrap()
            .lookup_at_interpolated_time(span, query(time(0, 1)), time(0, 1)),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    );
    let report = render(&mut o, 4);
    let h = o.source_playback_history().unwrap();
    let wrong = ConvertedPhaseSpan::from_words([1001, 0, 0, 1, 0, 0, 1, 0]).unwrap();
    assert!(h
        .lookup_at_interpolated_time(wrong, query(time(0, 1)), report.target_end_time)
        .is_err());
    let before = InterpolatedTargetTime::new(time(0, 1), 0, 1, 1000, -1, 1).unwrap();
    assert_eq!(
        h.lookup_at_interpolated_time(span, before, report.target_end_time),
        Err(ConvertedPhaseError::BeforeRetainedRange)
    );
    assert_eq!(
        h.lookup_at_interpolated_time(span, query(time(5, 1000)), report.target_end_time),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    );
}

#[test]
fn actual_render_commit_and_lookup_are_allocation_free_after_cold_setup() {
    let (_, mut o) = opted(44_100, 48_000, 2);
    let (r, a) = tracked(|| o.render(&mut [0.0; 8]));
    let r = r.unwrap();
    assert_eq!(a, [0; 3]);
    let h = o.source_playback_history().unwrap();
    let converted = o.phase_history().unwrap().span(0).unwrap();
    let q = query(time(3, 48_000));
    let (result, a) = tracked(|| h.lookup_at_interpolated_time(converted, q, r.target_end_time));
    assert!(result.is_ok());
    assert_eq!(a, [0; 3]);
    let (result, a) =
        tracked(|| h.lookup_at_interpolated_time(converted, query(time(1, 1)), r.target_end_time));
    assert!(result.is_err());
    assert_eq!(a, [0; 3]);
}

#[test]
fn positive_cached_target_at_full_capacity_does_not_adopt_pending_source_pause() {
    let (mut p, mut o) = opted(3, 7, 1);
    let first = render(&mut o, 1);
    assert_eq!(first.source.unwrap().frames, 2);
    assert_eq!(first.pulled_source_frame_cursor, 2);
    let before = spans(&o);
    p.request_pause(true);
    let (result, allocations) = tracked(|| o.render(&mut [99.0; 1]));
    let cached = result.unwrap();
    assert_eq!(allocations, [0; 3]);
    assert_eq!(cached.target_frames, 1);
    assert_eq!(cached.source.unwrap().frames, 0);
    assert!(!cached.source.unwrap().paused);
    assert_eq!(cached.pause_source_frame, None);
    assert_eq!(cached.pulled_source_frame_cursor, 2);
    assert_eq!(spans(&o), before);
    assert_eq!(o.source_playback_history().unwrap().pulled_end_frame(), 2);
    // Cancel the unadopted request. The next real pull must still merge at capacity.
    p.request_pause(false);
    let next = render(&mut o, 2);
    assert!(next.source.unwrap().frames > 0);
    assert!(!next.source.unwrap().paused);
    assert_eq!(o.source_playback_history().unwrap().len(), 1);
}

#[test]
fn history_capacity_refusal_preserves_actual_retained_practice_control_and_receipts() {
    use beatkernel::{time::Duration, transport::Rate};
    let pcm_limits = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format(1000), pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format(1000), vec![0.25; 128], pcm_limits).unwrap(),
    )
    .unwrap();
    let program = PreparedPracticeProgram::new_with_speed(
        &bank,
        vec![PracticeCue {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        }],
        PracticeLimits::new(8, 4, 128, 8, 16).unwrap(),
        Rate::new(3, 2).unwrap(),
        Duration::ZERO,
        false,
    )
    .unwrap();
    let (mut controller, endpoint) = practice_queue(&program).unwrap();
    let (mut p, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format(1000),
            ClockDomainId(3),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer
        .install_practice(
            program,
            endpoint,
            PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(100_000_000), true).unwrap(),
        )
        .unwrap();
    let mut o = ConvertedMixer::new(
        mixer,
        format(1000),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        32,
    )
    .unwrap_or_else(|f| panic!("fixture refused: {:?}", f.error()));
    o.enable_phase_history(4).unwrap();
    o.enable_source_playback_history(1).unwrap();
    let initial = render(&mut o, 2);
    assert_eq!(initial.source.unwrap().frames, 3);
    assert_eq!(initial.source.unwrap().playback_frames, 3);
    let initial_receipt = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(initial_receipt.receipt.generation, 1);
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    controller
        .try_request_next(
            7,
            1,
            PracticeRegion::new(
                Timestamp::from_nanos(1_000_000),
                Timestamp::from_nanos(50_000_000),
                true,
            )
            .unwrap(),
        )
        .unwrap();
    play(&mut p);
    p.request_pause(true);
    let before = (
        o.target_time(),
        o.source_position(),
        o.target_frame_cursor(),
        o.pulled_source_frame_cursor(),
        spans(&o),
        p.counters(),
    );
    let mut output = [99.0; 4];
    let (result, allocations) = tracked(|| o.render(&mut output));
    assert_eq!(result, Err(AudioError::RenderCapacity));
    assert_eq!(allocations, [0; 3]);
    assert_eq!(output, [99.0; 4]);
    assert_eq!(
        (
            o.target_time(),
            o.source_position(),
            o.target_frame_cursor(),
            o.pulled_source_frame_cursor(),
            spans(&o),
            p.counters()
        ),
        before
    );
    assert_eq!(controller.generation(), 1);
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    assert_eq!(o.phase_history().unwrap().len(), 1);
    // Same owners: the previously admitted request and command must execute once.
    p.request_pause(false);
    let resumed = render(&mut o, 4);
    assert_eq!(
        resumed.source.unwrap().counters.commands_consumed,
        initial.source.unwrap().counters.commands_consumed + 1
    );
    let applied = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(applied.receipt.request_id, 7);
    assert_eq!(applied.receipt.generation, 2);
    assert_eq!(applied.receipt.kind, PracticeBoundaryKind::Requested);
    assert_eq!(
        (
            applied.receipt.physical_frame,
            applied.receipt.playback_frame
        ),
        (3, 3)
    );
    assert_eq!(
        applied.receipt.requested_song_time,
        Timestamp::from_nanos(1_000_000)
    );
    assert_eq!(controller.generation(), 2);
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    assert_eq!(o.source_playback_history().unwrap().len(), 1);
}
