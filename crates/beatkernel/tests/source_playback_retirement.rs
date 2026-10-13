//! Explicit owner-qualified pins retire mathematical source history, not native evidence.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, Timestamp},
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
fn count(index: usize) {
    let _ = TRACK.try_with(|tracking| {
        if tracking.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut value = counts.get();
                value[index] += 1;
                counts.set(value);
            });
        }
    });
}
// SAFETY: All arguments and pointers are passed unchanged to System; scalar TLS does not allocate.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count(2);
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn tracked<T>(operation: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|counts| counts.set([0; 3]));
    TRACK.with(|tracking| tracking.set(true));
    let result = operation();
    TRACK.with(|tracking| tracking.set(false));
    (result, COUNTS.with(Cell::get))
}
fn rig() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(3, 1).unwrap();
    let limits = PcmLimits::new(1024, 1024, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 128], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(3),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 64, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn render(mixer: &mut Mixer, frames: usize) -> RenderReport {
    mixer.render(&mut [0.0; 64][..frames]).unwrap()
}
fn cold_nonzero() -> (CommandProducer, Mixer) {
    let (mut producer, mut mixer) = rig();
    render(&mut mixer, 3);
    producer.request_pause(true);
    render(&mut mixer, 2);
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (5, 3)
    );
    producer.request_pause(false);
    mixer.enable_source_playback_history(3).unwrap();
    (producer, mixer)
}
fn three_spans() -> (CommandProducer, Mixer) {
    let (mut producer, mut mixer) = cold_nonzero();
    render(&mut mixer, 2);
    producer.request_pause(true);
    render(&mut mixer, 2);
    producer.request_pause(false);
    render(&mut mixer, 2);
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (11, 7)
    );
    assert_eq!(mixer.source_playback_history().unwrap().len(), 3);
    (producer, mixer)
}
fn query(numerator: u64, denominator: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(
        TargetTime::new(
            numerator / denominator,
            numerator % denominator,
            denominator,
        )
        .unwrap(),
        0,
        0,
        1,
        0,
        1,
    )
    .unwrap()
}
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    initial: (u32, u64, u64),
    frontier: (u64, u64, u64),
    retained: (u64, usize, u64),
    spans: Vec<SourcePlaybackSpan>,
}
fn snapshot(mixer: &Mixer) -> Snapshot {
    let history = mixer.source_playback_history().unwrap();
    Snapshot {
        initial: (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
        ),
        frontier: (
            history.pulled_end_frame(),
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
        ),
        retained: (
            history.retained_start_frame(),
            history.capacity(),
            history.retirement_revision(),
        ),
        spans: (0..history.len())
            .map(|index| history.span(index).unwrap())
            .collect(),
    }
}

#[test]
fn cold_empty_and_disabled_retirement_have_no_effects() {
    let (_, mut disabled) = rig();
    assert_eq!(
        disabled.retire_source_playback_before(0),
        Err(AudioError::InvalidCapacity)
    );
    assert!(disabled.source_playback_history().is_none());
    assert_eq!(
        (disabled.frame_cursor(), disabled.playback_frame_cursor()),
        (0, 0)
    );
    let (_, mut mixer) = cold_nonzero();
    let before = snapshot(&mixer);
    assert_eq!(before.initial, (3, 5, 3));
    assert_eq!(before.retained, (5, 3, 0));
    assert_eq!(mixer.retire_source_playback_before(5), Ok(0));
    for pin in [0, 4, 6, u64::MAX] {
        assert_eq!(
            mixer.retire_source_playback_before(pin),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(snapshot(&mixer), before);
    }
    assert_eq!(snapshot(&mixer), before);
    assert_eq!(render(&mut mixer, 2).playback_frames, 2);
}

#[test]
fn covering_span_is_kept_at_inside_pin_and_exact_transition_equality() {
    for (pin, removed, retained) in [
        (5, 0, 5),
        (6, 0, 5),
        (7, 1, 7),
        (8, 1, 7),
        (9, 2, 9),
        (11, 2, 9),
    ] {
        let (_, mut mixer) = three_spans();
        let before = snapshot(&mixer);
        assert_eq!(mixer.retire_source_playback_before(pin), Ok(removed));
        let after = snapshot(&mixer);
        assert_eq!(after.initial, before.initial);
        assert_eq!(after.frontier, before.frontier);
        assert_eq!(after.retained, (retained, 3, u64::from(removed > 0)));
        assert_eq!(after.spans, before.spans[removed..]);
        assert!(!after.spans.is_empty());
        let stable = snapshot(&mixer);
        assert_eq!(mixer.retire_source_playback_before(pin), Ok(0));
        assert_eq!(snapshot(&mixer), stable);
    }
}

#[test]
fn invalid_pins_before_and_after_frontiers_preserve_live_state_and_real_rendering() {
    let (mut producer, mut mixer) = three_spans();
    assert_eq!(mixer.retire_source_playback_before(7), Ok(1));
    let before = snapshot(&mixer);
    for pin in [0, 5, 6, 12, u64::MAX] {
        assert_eq!(
            mixer.retire_source_playback_before(pin),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(snapshot(&mixer), before);
    }
    producer.request_pause(true);
    let report = render(&mut mixer, 2);
    assert_eq!(report.playback_frames, 0);
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (13, 7)
    );
    assert_eq!(
        mixer
            .source_playback_history()
            .unwrap()
            .span(2)
            .unwrap()
            .physical_start(),
        11
    );
}

#[test]
fn retained_source_queries_preserve_running_and_frozen_affine_results() {
    let (_, mut mixer) = three_spans();
    // Coordinates are physical frames / 3; use thirds of frames to test fractional positions.
    let before: Vec<_> = [21, 22, 26, 27, 28, 33]
        .iter()
        .map(|&frame_thirds| {
            let q = query(frame_thirds, 9);
            let span = mixer
                .source_playback_history()
                .unwrap()
                .lookup_at_source_time(q)
                .unwrap();
            (q, span, span.elapsed_nanos_at_source_time(q, 3, 3).unwrap())
        })
        .collect();
    assert_eq!(mixer.retire_source_playback_before(7), Ok(1));
    for (q, old_span, old_elapsed) in before {
        let span = mixer
            .source_playback_history()
            .unwrap()
            .lookup_at_source_time(q)
            .unwrap();
        assert_eq!(span, old_span);
        assert_eq!(span.elapsed_nanos_at_source_time(q, 3, 3), Ok(old_elapsed));
    }
    for (frame_thirds, playback_thirds) in
        [(21, 15), (22, 15), (26, 15), (27, 15), (28, 16), (33, 21)]
    {
        let q = query(frame_thirds, 9);
        let span = mixer
            .source_playback_history()
            .unwrap()
            .lookup_at_source_time(q)
            .unwrap();
        // Independent oracle: frozen playback=5; resumed playback=physical-4, relative cut=3.
        let expected = i128::from(playback_thirds - 9) * 1_000_000_000 / 9;
        assert_eq!(span.elapsed_nanos_at_source_time(q, 3, 3), Ok(expected));
    }
}

#[test]
fn exact_rational_retained_lower_rejects_old_floor_alias_and_preserves_pull_frontier() {
    let (_, mut mixer) = three_spans();
    let below = query(70_000_000_000 - 1, 30_000_000_000);
    let equal = query(7, 3);
    let above = query(70_000_000_000 + 1, 30_000_000_000);
    let origin = ClockPoint {
        domain: ClockDomainId(3),
        timestamp: Timestamp::ZERO,
    };
    assert_eq!(below.point(origin).unwrap(), equal.point(origin).unwrap());
    assert_eq!(above.point(origin).unwrap(), equal.point(origin).unwrap());
    let history = mixer.source_playback_history().unwrap();
    assert_eq!(
        history.lookup_at_source_time(below).unwrap().state(),
        SourcePlaybackState::Running
    );
    assert_eq!(
        history.lookup_at_source_time(equal).unwrap().state(),
        SourcePlaybackState::Frozen
    );
    assert_eq!(
        history.lookup_at_source_time(above).unwrap().state(),
        SourcePlaybackState::Frozen
    );
    assert_eq!(mixer.retire_source_playback_before(7), Ok(1));
    let history = mixer.source_playback_history().unwrap();
    assert_eq!(
        history.lookup_at_source_time(below),
        Err(ConvertedPhaseError::BeforeRetainedRange)
    );
    assert_eq!(
        history.lookup_at_source_time(query(5, 3)),
        Err(ConvertedPhaseError::BeforeRetainedRange)
    );
    assert_eq!(
        history
            .lookup_at_source_time(equal)
            .unwrap()
            .physical_start(),
        7
    );
    assert_eq!(
        history
            .lookup_at_source_time(above)
            .unwrap()
            .physical_start(),
        7
    );
    assert!(history.lookup_at_source_time(query(11, 3)).is_ok());
    assert_eq!(
        history.lookup_at_source_time(query(110_000_000_001, 30_000_000_000)),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    );
}

#[test]
fn converted_exact_physical_lookup_obeys_same_retained_range() {
    let (_, raw) = rig();
    let mut converter = ConvertedMixer::new(
        raw,
        AudioFormat::new(7, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        64,
    )
    .unwrap_or_else(|failure| panic!("fixture: {:?}", failure.error()));
    converter.enable_phase_history(3).unwrap();
    let report = converter.render(&mut [0.0; 28]).unwrap();
    let converted = converter.phase_history().unwrap().span(0).unwrap();
    let (_, mut mixer) = three_spans();
    let kept_queries = [
        query(7, 3),
        query(8, 3),
        query(9, 3),
        query(10, 3),
        query(11, 3),
    ];
    let before: Vec<_> = kept_queries
        .iter()
        .map(|&q| {
            mixer
                .source_playback_history()
                .unwrap()
                .lookup_at_interpolated_time(converted, q, report.target_end_time)
                .unwrap()
        })
        .collect();
    assert_eq!(mixer.retire_source_playback_before(7), Ok(1));
    let history = mixer.source_playback_history().unwrap();
    for (&q, expected) in kept_queries.iter().zip(before) {
        assert_eq!(
            history.lookup_at_interpolated_time(converted, q, report.target_end_time),
            Ok(expected)
        );
    }
    for q in [query(5, 3), query(70_000_000_000 - 1, 30_000_000_000)] {
        assert_eq!(
            history.lookup_at_interpolated_time(converted, q, report.target_end_time),
            Err(ConvertedPhaseError::BeforeRetainedRange)
        );
    }
    assert_eq!(
        history.lookup_at_interpolated_time(
            converted,
            query(110_000_000_001, 30_000_000_000),
            report.target_end_time
        ),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    );
}

#[test]
fn fixed_capacity_repeated_real_pause_resume_cycles_match_unretired_pcm_and_reports() {
    let (mut producer, mut mixer) = cold_nonzero();
    let (mut reference_producer, mut reference) = rig();
    render(&mut reference, 3);
    reference_producer.request_pause(true);
    render(&mut reference, 2);
    reference_producer.request_pause(false);
    let mut revisions = 0;
    for cycle in 0..24 {
        for paused in [false, true] {
            producer.request_pause(paused);
            reference_producer.request_pause(paused);
            let mut actual_pcm = [99.0; 2];
            let mut reference_pcm = [99.0; 2];
            let (result, allocations) = tracked(|| mixer.render(&mut actual_pcm));
            assert_eq!(allocations, [0; 3]);
            assert_eq!(
                result.unwrap(),
                reference.render(&mut reference_pcm).unwrap()
            );
            assert_eq!(actual_pcm, reference_pcm);
            let history = mixer.source_playback_history().unwrap();
            assert!(history.len() <= 3);
            assert_eq!(history.capacity(), 3);
            // This test retains only the latest span: no outstanding reader owns earlier queries.
            let safe_pin = history.span(history.len() - 1).unwrap().physical_start();
            let (result, allocations) = tracked(|| mixer.retire_source_playback_before(safe_pin));
            assert_eq!(allocations, [0; 3]);
            let removed = result.unwrap();
            if removed > 0 {
                revisions += 1;
            }
            let history = mixer.source_playback_history().unwrap();
            assert_eq!(history.len(), 1);
            assert_eq!(history.retirement_revision(), revisions);
            assert_eq!(
                (
                    history.initial_physical_frame(),
                    history.initial_playback_frame()
                ),
                (5, 3)
            );
        }
        assert_eq!(mixer.frame_cursor(), 5 + (cycle + 1) * 4);
        assert_eq!(mixer.playback_frame_cursor(), 3 + (cycle + 1) * 2);
    }
}

#[test]
fn positive_noop_and_refused_retirement_allocate_nothing_after_cold_setup() {
    let (_, mut mixer) = three_spans();
    for (pin, expected) in [
        (7, Ok(1)),
        (7, Ok(0)),
        (6, Err(AudioError::InvalidFormat)),
        (12, Err(AudioError::InvalidFormat)),
        (11, Ok(1)),
        (11, Ok(0)),
    ] {
        let (result, allocations) = tracked(|| mixer.retire_source_playback_before(pin));
        assert_eq!(result, expected);
        assert_eq!(allocations, [0; 3]);
    }
    assert_eq!(
        mixer
            .source_playback_history()
            .unwrap()
            .retirement_revision(),
        2
    );
    assert_eq!(render(&mut mixer, 2).playback_frames, 2);
}
