use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}
struct Allocator;
fn count(kind: usize) {
    let _ = TRACK.try_with(|tracking| {
        if tracking.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut value = counts.get();
                value[kind] += 1;
                counts.set(value);
            });
        }
    });
}
// SAFETY: All arguments pass unchanged to System; bookkeeping uses scalar TLS.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count(2);
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn track<T>(operation: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|counts| counts.set([0; 3]));
    TRACK.with(|tracking| tracking.set(true));
    let result = operation();
    TRACK.with(|tracking| tracking.set(false));
    (result, COUNTS.with(Cell::get))
}
fn format(rate: u32) -> AudioFormat {
    AudioFormat::new(rate, 1).unwrap()
}
fn rig(source: u32, target: u32) -> (CommandProducer, ConvertedMixer) {
    rig_after_frames(source, target, 0)
}
fn rig_after_frames(source: u32, target: u32, prefix: usize) -> (CommandProducer, ConvertedMixer) {
    let pcm = PcmLimits::new(1024, 1024, 1).unwrap();
    let mut bank = SampleBank::new(format(source), pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format(source), vec![0.25; 128], pcm).unwrap(),
    )
    .unwrap();
    let limits = AudioLimits::new(8, 4, 8, 64, 8).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(format(source), ClockDomainId(3), Timestamp::ZERO, limits),
        bank,
        consumer,
    )
    .unwrap();
    if prefix != 0 {
        mixer.render(&mut [0.0; 16][..prefix]).unwrap();
    }
    let owner = match ConvertedMixer::new(
        mixer,
        format(target),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    ) {
        Ok(owner) => owner,
        Err(failure) => panic!("fixture conversion refused: {:?}", failure.error()),
    };
    (producer, owner)
}
fn opted(source: u32, target: u32, capacity: usize) -> ConvertedMixer {
    let (_, mut owner) = rig(source, target);
    owner.enable_phase_history(capacity).unwrap();
    owner
}
fn render(owner: &mut ConvertedMixer, frames: usize) -> ConvertedRenderReport {
    owner.render(&mut [0.0; 16][..frames]).unwrap()
}
fn held(owner: &mut ConvertedMixer, frames: usize) -> ConvertedRenderReport {
    owner.render_held(&mut [1.0; 16][..frames]).unwrap()
}
fn retarget(owner: &mut ConvertedMixer, rate: u32) {
    owner
        .retarget(format(rate), ChannelMatrix::default_mix(1, 1).unwrap(), 16)
        .unwrap();
}
fn elapsed(owner: &ConvertedMixer, time: TargetTime, cut: u64) -> i128 {
    owner
        .phase_history()
        .unwrap()
        .lookup(time)
        .unwrap()
        .elapsed_nanos_since(cut)
        .unwrap()
}

#[test]
fn forward_conversion_preserves_fraction_before_elapsed_floor() {
    let mut owner = opted(44_100, 48_000, 2);
    let report = render(&mut owner, 3);
    assert_eq!(
        report.source_position,
        SourcePosition {
            frame: 2,
            numerator: 121,
            denominator: 160
        }
    );
    let point = owner
        .phase_history()
        .unwrap()
        .lookup(TargetTime::new(0, 1, 16_000).unwrap())
        .unwrap();
    assert_eq!(point.elapsed_nanos_since(2), Ok(17_148));
    assert_eq!(point.source_rate(), 44_100);
    assert_eq!(point.state(), ConvertedOutputState::Active);
    assert_eq!(point.target_time(), report.target_end_time);
}

#[test]
fn reverse_conversion_preserves_fraction_before_elapsed_floor() {
    let mut owner = opted(48_000, 44_100, 2);
    let report = render(&mut owner, 2);
    assert_eq!(
        report.source_position,
        SourcePosition {
            frame: 2,
            numerator: 26,
            denominator: 147
        }
    );
    assert_eq!(
        elapsed(&owner, TargetTime::new(0, 1, 22_050).unwrap(), 2),
        3_684
    );
}

#[test]
fn held_history_freezes_source_and_does_not_erase_earlier_active_mapping() {
    let mut owner = opted(44_100, 48_000, 3);
    let active = render(&mut owner, 4);
    assert_eq!(
        active.source_position,
        SourcePosition {
            frame: 3,
            numerator: 27,
            denominator: 40
        }
    );
    let silent = held(&mut owner, 3);
    assert_eq!(silent.source, None);
    assert_eq!(silent.source_position, active.source_position);
    let after = render(&mut owner, 2);
    assert_eq!(
        after.source_position,
        SourcePosition {
            frame: 5,
            numerator: 41,
            denominator: 80
        }
    );
    let mid_hold = TargetTime::new(0, 1, 9_600).unwrap();
    assert_eq!(elapsed(&owner, mid_hold, 3), 15_306);
    assert_eq!(
        owner
            .phase_history()
            .unwrap()
            .lookup(mid_hold)
            .unwrap()
            .state(),
        ConvertedOutputState::Held
    );
    assert_eq!(
        elapsed(&owner, TargetTime::new(0, 1, 16_000).unwrap(), 2),
        17_148
    );
    assert_eq!(elapsed(&owner, after.target_end_time, 5), 11_621);
    assert_eq!(owner.phase_history().unwrap().len(), 3);
}

#[test]
fn retarget_keeps_old_end_new_origin_and_fraction_identical() {
    let mut owner = opted(44_100, 48_000, 1);
    let old = render(&mut owner, 4);
    retarget(&mut owner, 44_100);
    assert_eq!(owner.target_time(), old.target_end_time);
    let next = render(&mut owner, 1);
    assert_eq!(next.target_start_time, old.target_end_time);
    assert_eq!(next.source_start_position, old.source_position);
    assert_eq!(
        next.source_position,
        SourcePosition {
            frame: 4,
            numerator: 27,
            denominator: 40
        }
    );
    assert_eq!(
        next.target_end_time,
        TargetTime::new(0, 187, 1_764_000).unwrap()
    );
    assert_eq!(elapsed(&owner, next.target_end_time, 4), 15_306);
    assert_eq!(owner.phase_history().unwrap().len(), 1);
}

#[test]
fn wide_accepted_query_fraction_does_not_require_source_position_denominator() {
    let mut owner = opted(1, 3, 3);
    render(&mut owner, 1);
    retarget(&mut owner, 5);
    held(&mut owner, 1);
    retarget(&mut owner, 3);
    let end = render(&mut owner, 1);
    assert_eq!(end.target_end_time, TargetTime::new(0, 13, 15).unwrap());
    assert_eq!(
        end.source_position,
        SourcePosition {
            frame: 0,
            numerator: 2,
            denominator: 3
        }
    );
    let query = TargetTime::new(0, 12_297_829_382_473_034_409, 18_446_744_073_709_551_614).unwrap();
    assert_eq!(elapsed(&owner, query, 0), 466_666_666);
    assert_eq!(
        owner
            .phase_history()
            .unwrap()
            .lookup(query)
            .unwrap()
            .elapsed_nanos_since(1),
        Err(ConvertedPhaseError::NegativeElapsed)
    );
}

#[test]
fn high_limb_fraction_carry_preserves_independent_rational_elapsed_oracles() {
    let mut owner = opted(4_294_967_291, 4_294_967_279, 3);
    render(&mut owner, 1);
    retarget(&mut owner, 4_294_967_231);
    render(&mut owner, 1);
    held(&mut owner, 1);
    render(&mut owner, 16);

    // Independent Fraction reference: physical source = sourceHz *
    // (query - 1/targetB), accounting for exactly one held targetB frame.
    // The final active anchor has time 12884901789/18446743721522234449
    // and source 36893487752282112410/18446743721522234449. Combining
    // that source residual with these query fractions exercises high limbs.
    let cases = [
        (13_958_643_875, 0, 0, None),
        (15_032_385_715, 0, 0, None),
        (17_179_869_396, 0, 0, None),
        (47_244_640_923, 2, 1, Some(0)),
        (80_530_637_970, 4, 3, Some(1)),
    ];
    for (numerator, since_zero, since_two, since_ten) in cases {
        let query = TargetTime::new(0, numerator, u64::MAX).unwrap();
        let point = owner.phase_history().unwrap().lookup(query).unwrap();
        assert_eq!(point.elapsed_nanos_since(0), Ok(since_zero));
        assert_eq!(point.elapsed_nanos_since(2), Ok(since_two));
        match since_ten {
            Some(expected) => assert_eq!(point.elapsed_nanos_since(10), Ok(expected)),
            None => assert_eq!(
                point.elapsed_nanos_since(10),
                Err(ConvertedPhaseError::NegativeElapsed)
            ),
        }
    }
}

#[test]
fn many_active_blocks_and_retargets_share_one_segment() {
    let mut owner = opted(44_100, 48_000, 1);
    for index in 0..64 {
        render(&mut owner, 1);
        retarget(&mut owner, if index % 2 == 0 { 44_100 } else { 48_000 });
    }
    assert_eq!(owner.phase_history().unwrap().len(), 1);
    assert_eq!(owner.phase_history().unwrap().capacity(), 1);
    assert!(elapsed(&owner, owner.target_time(), 0) > 0);
}

#[test]
fn held_blocks_and_retargets_coalesce_without_source_pull() {
    let mut owner = opted(44_100, 48_000, 1);
    for index in 0..64 {
        held(&mut owner, 1);
        retarget(&mut owner, if index % 2 == 0 { 44_100 } else { 48_000 });
    }
    assert_eq!(owner.phase_history().unwrap().len(), 1);
    assert_eq!(owner.pulled_source_frame_cursor(), 0);
    assert_eq!(elapsed(&owner, owner.target_time(), 0), 0);
}

#[test]
fn empty_callbacks_do_not_add_or_switch_segments() {
    let mut owner = opted(1_000, 1_000, 1);
    render(&mut owner, 1);
    let time = owner.target_time();
    held(&mut owner, 0);
    render(&mut owner, 0);
    assert_eq!(owner.target_time(), time);
    assert_eq!(owner.phase_history().unwrap().len(), 1);
    render(&mut owner, 1);
    assert_eq!(owner.phase_history().unwrap().len(), 1);
}

#[test]
fn empty_history_retains_initial_point_and_rejects_future() {
    let owner = opted(1_000, 1_000, 1);
    let history = owner.phase_history().unwrap();
    assert!(history.is_empty());
    assert_eq!(history.start_time(), TargetTime::new(0, 0, 1).unwrap());
    assert_eq!(history.end_time(), history.start_time());
    assert_eq!(elapsed(&owner, history.start_time(), 0), 0);
    assert!(matches!(
        history.lookup(TargetTime::new(0, 1, u64::MAX).unwrap()),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    ));
}

#[test]
fn generated_range_is_exact_and_inclusive_at_both_ends() {
    let mut owner = opted(1, 3, 1);
    render(&mut owner, 1);
    assert_eq!(elapsed(&owner, TargetTime::new(0, 0, 1).unwrap(), 0), 0);
    assert_eq!(
        elapsed(&owner, TargetTime::new(0, 1, 3).unwrap(), 0),
        333_333_333
    );
    let beyond = TargetTime::new(0, 6_148_914_691_236_517_206, u64::MAX).unwrap();
    assert!(matches!(
        owner.phase_history().unwrap().lookup(beyond),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    ));
    let inside = TargetTime::new(0, 6_148_914_691_236_517_204, u64::MAX).unwrap();
    assert_eq!(elapsed(&owner, inside, 0), 333_333_333);
    assert_eq!(
        owner
            .phase_history()
            .unwrap()
            .lookup(inside)
            .unwrap()
            .elapsed_nanos_since(u64::MAX),
        Err(ConvertedPhaseError::NegativeElapsed)
    );
}

#[test]
fn actual_nonzero_mixer_origin_refuses_before_retained_range() {
    let (_, mut owner) = rig_after_frames(3, 3, 2);
    owner.enable_phase_history(1).unwrap();
    let start = TargetTime::new(0, 2, 3).unwrap();
    assert_eq!(owner.phase_history().unwrap().start_time(), start);
    assert_eq!(elapsed(&owner, start, 2), 0);
    assert!(matches!(
        owner
            .phase_history()
            .unwrap()
            .lookup(TargetTime::new(0, 1, 3).unwrap()),
        Err(ConvertedPhaseError::BeforeRetainedRange)
    ));
    render(&mut owner, 1);
    assert_eq!(
        elapsed(&owner, TargetTime::new(1, 0, 1).unwrap(), 2),
        333_333_333
    );
}

#[test]
fn enable_is_cold_once_and_invalid_requests_preserve_existing_history() {
    let (_, mut owner) = rig(1_000, 1_000);
    assert_eq!(
        owner.enable_phase_history(0),
        Err(AudioError::InvalidCapacity)
    );
    assert!(owner.phase_history().is_none());
    render(&mut owner, 0);
    held(&mut owner, 0);
    owner.enable_phase_history(2).unwrap();
    assert_eq!(
        owner.enable_phase_history(3),
        Err(AudioError::InvalidCapacity)
    );
    assert_eq!(owner.phase_history().unwrap().capacity(), 2);
    assert!(owner.phase_history().unwrap().is_empty());
}

#[test]
fn late_enable_refuses_after_active_or_held_generation() {
    for is_held in [false, true] {
        let (_, mut owner) = rig(1_000, 1_000);
        if is_held {
            held(&mut owner, 1);
        } else {
            render(&mut owner, 1);
        }
        let time = owner.target_time();
        let source = owner.source_position();
        assert_eq!(
            owner.enable_phase_history(2),
            Err(AudioError::InvalidCapacity)
        );
        assert!(owner.phase_history().is_none());
        assert_eq!(owner.target_time(), time);
        assert_eq!(owner.source_position(), source);
    }
}

#[test]
fn exhausted_transition_capacity_refuses_before_active_output_or_command_consumption() {
    let (mut producer, mut owner) = rig(1_000, 1_000);
    owner.enable_phase_history(1).unwrap();
    held(&mut owner, 2);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let source = owner.source_position();
    let time = owner.target_time();
    let counters = owner.mixer().counters();
    let mut output = [0.875; 2];
    assert_eq!(owner.render(&mut output), Err(AudioError::RenderCapacity));
    assert_eq!(output, [0.875; 2]);
    assert_eq!(owner.source_position(), source);
    assert_eq!(owner.target_time(), time);
    assert_eq!(owner.target_frame_cursor(), 2);
    assert_eq!(owner.pulled_source_frame_cursor(), 0);
    assert_eq!(owner.mixer().counters(), counters);
    assert_eq!(owner.phase_history().unwrap().len(), 1);
    assert_eq!(elapsed(&owner, time, 0), 0);
    assert!(matches!(
        owner
            .phase_history()
            .unwrap()
            .lookup(TargetTime::new(0, 3, 1_000).unwrap()),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    ));
    held(&mut owner, 1);
    assert_eq!(owner.mixer().counters(), counters);
}

#[test]
fn exhausted_transition_capacity_refuses_before_held_output_or_state_change() {
    let mut owner = opted(1_000, 1_000, 1);
    render(&mut owner, 2);
    let source = owner.source_position();
    let time = owner.target_time();
    let pull = owner.pulled_source_frame_cursor();
    let counters = owner.mixer().counters();
    let mut output = [0.875; 2];
    assert_eq!(
        owner.render_held(&mut output),
        Err(AudioError::RenderCapacity)
    );
    assert_eq!(output, [0.875; 2]);
    assert_eq!(owner.source_position(), source);
    assert_eq!(owner.target_time(), time);
    assert_eq!(owner.pulled_source_frame_cursor(), pull);
    assert_eq!(owner.mixer().counters(), counters);
    assert_eq!(elapsed(&owner, time, 0), 2_000_000);
    render(&mut owner, 1);
    assert_eq!(owner.phase_history().unwrap().len(), 1);
}

#[test]
fn disabled_owner_matches_opted_owner_pcm_and_reports() {
    let (mut p1, mut plain) = rig(44_100, 48_000);
    let (mut p2, mut opted) = rig(44_100, 48_000);
    opted.enable_phase_history(4).unwrap();
    for producer in [&mut p1, &mut p2] {
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            })
            .unwrap();
    }
    for is_held in [false, false, true, true, false] {
        let mut a = [99.0; 3];
        let mut b = [99.0; 3];
        let ra = if is_held {
            plain.render_held(&mut a)
        } else {
            plain.render(&mut a)
        }
        .unwrap();
        let rb = if is_held {
            opted.render_held(&mut b)
        } else {
            opted.render(&mut b)
        }
        .unwrap();
        assert_eq!(ra, rb);
        assert_eq!(a, b);
    }
    assert!(plain.phase_history().is_none());
}

#[test]
fn moving_complete_owner_retains_both_prior_segments_and_future_continuity() {
    let mut owner = opted(44_100, 48_000, 3);
    render(&mut owner, 4);
    held(&mut owner, 3);
    let mut moved = Some(owner).take().unwrap();
    retarget(&mut moved, 44_100);
    render(&mut moved, 1);
    assert_eq!(
        elapsed(&moved, TargetTime::new(0, 1, 16_000).unwrap(), 2),
        17_148
    );
    assert_eq!(
        elapsed(&moved, TargetTime::new(0, 1, 9_600).unwrap(), 3),
        15_306
    );
    assert_eq!(elapsed(&moved, moved.target_time(), 4), 15_306);
}

#[test]
fn opted_callbacks_and_historical_lookup_do_not_allocate_or_destroy() {
    let mut owner = opted(1_000, 1_000, 3);
    let mut output = [99.0; 2];
    let (result, counts) = track(|| owner.render(&mut output));
    assert!(result.is_ok());
    assert_eq!(counts, [0; 3]);
    let (result, counts) = track(|| owner.render_held(&mut output));
    assert!(result.is_ok());
    assert_eq!(counts, [0; 3]);
    let (result, counts) = track(|| owner.render(&mut output));
    assert!(result.is_ok());
    assert_eq!(counts, [0; 3]);
    let query = TargetTime::new(0, 3, 1_000).unwrap();
    let (result, counts) = track(|| {
        owner
            .phase_history()
            .unwrap()
            .lookup(query)
            .unwrap()
            .elapsed_nanos_since(1)
    });
    assert_eq!(result, Ok(1_000_000));
    assert_eq!(counts, [0; 3]);
}
