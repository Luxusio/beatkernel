//! Retention comes from real ordered receipts, including unprojected lookahead.
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
// SAFETY: All allocator arguments and results pass unchanged to System;
// the scalar thread-local instrumentation does not allocate.
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

fn prepared(capacity: Option<usize>) -> (Mixer, Option<PracticeController>) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(64, 64, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.1, 0.2, 0.3, 0.4], limits).unwrap(),
    )
    .unwrap();
    let practice = capacity.map(|capacity| {
        let program = PreparedPracticeProgram::new(
            &bank,
            vec![PracticeCue {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            }],
            PracticeLimits::new(8, 1, 128, 8, capacity).unwrap(),
        )
        .unwrap();
        let (controller, endpoint) = practice_queue(&program).unwrap();
        (program, controller, endpoint)
    });
    let (_, consumer) = command_queue(16).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(16, 4, 16, 128, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let controller = practice.map(|(program, controller, endpoint)| {
        mixer
            .install_practice(
                program,
                endpoint,
                PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(1_000_000), true)
                    .unwrap(),
            )
            .unwrap();
        controller
    });
    (mixer, controller)
}
fn mixer() -> Mixer {
    prepared(None).0
}
fn practice(capacity: usize) -> (Mixer, PracticeController) {
    let (mixer, controller) = prepared(Some(capacity));
    (mixer, controller.unwrap())
}
fn converted(capacity: usize) -> (ConvertedMixer, PracticeController) {
    let (mixer, controller) = practice(capacity);
    (
        ConvertedMixer::new(
            mixer,
            AudioFormat::new(1500, 1).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            32,
        )
        .unwrap(),
        controller,
    )
}
fn time(frame: u64) -> TargetTime {
    TargetTime::from_frames(frame, 1500).unwrap()
}

#[test]
fn absent_and_cold_practice_have_no_live_receipt_pins() {
    assert_eq!(mixer().practice_history_retention_pins(), (None, None));
    let (mixer, controller) = practice(8);
    assert_eq!(mixer.practice_history_retention_pins(), (None, None));
    assert_eq!(controller.history_retention_pins(), (None, None));
}

#[test]
fn raw_source_lookahead_is_retained_without_inventing_target_projection() {
    let (mut output, mut controller) = converted(16);
    output.render(&mut [0.0]).unwrap();
    assert_eq!(
        output.mixer().practice_history_retention_pins(),
        (Some(0), Some(time(0)))
    );
    let start = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (start.receipt.physical_frame, start.boundary.target_time),
        (0, time(0))
    );
    assert_eq!(
        output.mixer().practice_history_retention_pins(),
        (Some(1), None)
    );
    assert_eq!(controller.history_retention_pins(), (Some(1), None));
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    let position = output.source_position();
    output.render_held(&mut [0.0; 3]).unwrap();
    assert_eq!(output.source_position(), position);
    assert_eq!(
        output.mixer().practice_history_retention_pins(),
        (Some(1), None)
    );
    assert_eq!(controller.history_retention_pins(), (Some(1), None));
    assert_eq!(output.render(&mut [0.0]).unwrap().source.unwrap().frames, 0);
    assert_eq!(
        output.mixer().practice_history_retention_pins(),
        (Some(1), None)
    );
    output.render(&mut [0.0]).unwrap();
    assert_eq!(
        output.mixer().practice_history_retention_pins(),
        (Some(1), Some(time(5)))
    );
    let next = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (next.receipt.physical_frame, next.boundary.target_time),
        (1, time(5))
    );
}

#[test]
fn polling_multiple_projected_receipts_preserves_order_and_advances_only_on_pop() {
    let (mut output, mut controller) = converted(16);
    output.render(&mut [0.0; 3]).unwrap();
    let before = (
        output.source_position(),
        output.target_time(),
        output.mixer().frame_cursor(),
    );
    for _ in 0..64 {
        assert_eq!(
            output.mixer().practice_history_retention_pins(),
            (Some(0), Some(time(0)))
        );
        assert_eq!(
            controller.history_retention_pins(),
            (Some(0), Some(time(0)))
        );
    }
    assert_eq!(
        before,
        (
            output.source_position(),
            output.target_time(),
            output.mixer().frame_cursor()
        )
    );
    let first = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (first.receipt.generation, first.receipt.physical_frame),
        (1, 0)
    );
    assert_eq!(
        output.mixer().practice_history_retention_pins(),
        (Some(1), Some(time(2)))
    );
    let second = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (
            second.receipt.generation,
            second.receipt.physical_frame,
            second.boundary.target_time
        ),
        (2, 1, time(2))
    );
    assert_eq!(
        output.mixer().practice_history_retention_pins(),
        (Some(2), None)
    );
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
}

#[test]
fn wrapped_reclaimed_slots_do_not_pin_old_generations() {
    let (mut output, mut controller) = converted(4);
    for round in 0..16u64 {
        output.render(&mut [0.0; 3]).unwrap();
        assert_eq!(
            output.mixer().practice_history_retention_pins(),
            (Some(2 * round), Some(time(3 * round)))
        );
        assert_eq!(
            controller.history_retention_pins(),
            (Some(2 * round), Some(time(3 * round)))
        );
        let first = controller.try_pop_projected_receipt().unwrap();
        assert_eq!(
            (first.receipt.physical_frame, first.boundary.target_time),
            (2 * round, time(3 * round))
        );
        assert_eq!(
            output.mixer().practice_history_retention_pins(),
            (Some(2 * round + 1), Some(time(3 * round + 2)))
        );
        let second = controller.try_pop_projected_receipt().unwrap();
        assert_eq!(
            (second.receipt.physical_frame, second.boundary.target_time),
            (2 * round + 1, time(3 * round + 2))
        );
        assert_eq!(
            output.mixer().practice_history_retention_pins(),
            (Some(2 * round + 2), None)
        );
        assert_eq!(
            controller.history_retention_pins(),
            (Some(2 * round + 2), None)
        );
        assert_eq!(
            controller.try_pop_projected_receipt(),
            Err(PracticeError::Empty)
        );
    }
}

#[test]
fn full_receipt_queue_and_retention_poll_do_not_consume_credits() {
    let (mut output, mut controller) = converted(2);
    output.render(&mut [0.0]).unwrap();
    output.render(&mut [0.0]).unwrap();
    let before = (
        output.source_position(),
        output.target_time(),
        output.pulled_source_frame_cursor(),
    );
    for _ in 0..16 {
        assert_eq!(
            output.mixer().practice_history_retention_pins(),
            (Some(0), Some(time(0)))
        );
        assert_eq!(
            controller.history_retention_pins(),
            (Some(0), Some(time(0)))
        );
    }
    let mut untouched = [9.0];
    assert_eq!(
        output.render(&mut untouched),
        Err(AudioError::PracticeReceiptFull)
    );
    assert_eq!(untouched, [9.0]);
    assert_eq!(
        before,
        (
            output.source_position(),
            output.target_time(),
            output.pulled_source_frame_cursor()
        )
    );
    assert_eq!(
        controller
            .try_pop_projected_receipt()
            .unwrap()
            .receipt
            .physical_frame,
        0
    );
    output.render(&mut untouched).unwrap();
    assert_eq!(
        output.mixer().practice_history_retention_pins(),
        (Some(1), Some(time(2)))
    );
}

#[test]
fn real_queued_receipt_polling_uses_no_heap() {
    let (_, calibration) = tracked(|| {
        let mut values = Vec::<u8>::with_capacity(1);
        values.push(1);
        values.reserve_exact(128);
        std::hint::black_box(&values);
        drop(values);
    });
    assert!(calibration.into_iter().all(|n| n > 0));
    let (mut output, mut controller) = converted(8);
    output.render(&mut [0.0; 3]).unwrap();
    let (last, calls) = tracked(|| {
        let mut last = (None, None);
        for _ in 0..128 {
            last = std::hint::black_box(output.mixer().practice_history_retention_pins());
            std::hint::black_box(controller.history_retention_pins());
        }
        last
    });
    assert_eq!(calls, [0; 3]);
    assert_eq!(last, (Some(0), Some(time(0))));
    assert_eq!(
        controller
            .try_pop_projected_receipt()
            .unwrap()
            .receipt
            .physical_frame,
        0
    );
}
