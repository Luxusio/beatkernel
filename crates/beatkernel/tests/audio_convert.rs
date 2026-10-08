use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    f64::consts::PI,
};

use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
    // Per-thread and one-shot: unrelated concurrently running tests are unaffected.
    static FAIL_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
    static FAILURE_TRIGGERED: Cell<bool> = const { Cell::new(false) };
}
struct Allocator;
fn allocation_refused() -> bool {
    FAIL_AFTER
        .try_with(|remaining| match remaining.get() {
            Some(0) => {
                remaining.set(None);
                let _ = FAILURE_TRIGGERED.try_with(|triggered| triggered.set(true));
                true
            }
            Some(count) => {
                remaining.set(Some(count - 1));
                false
            }
            None => false,
        })
        .unwrap_or(false)
}
fn count(kind: usize) {
    let _ = TRACK.try_with(|track| {
        if track.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut value = counts.get();
                value[kind] += 1;
                counts.set(value);
            });
        }
    });
}
// SAFETY: The allocator forwards unmodified pointer/layout arguments to System;
// bookkeeping only touches allocation-free thread-local scalar cells.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        if allocation_refused() {
            return std::ptr::null_mut();
        }
        // SAFETY: GlobalAlloc caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        if allocation_refused() {
            return std::ptr::null_mut();
        }
        // SAFETY: GlobalAlloc caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        if allocation_refused() {
            return std::ptr::null_mut();
        }
        // SAFETY: GlobalAlloc caller supplies a live allocation, its layout and
        // a valid nonzero replacement size.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count(2);
        // SAFETY: GlobalAlloc caller supplies the original allocation and layout.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn refuse_allocation<T>(successful_allocations: usize, operation: impl FnOnce() -> T) -> (T, bool) {
    struct Disarm;
    impl Drop for Disarm {
        fn drop(&mut self) {
            FAIL_AFTER.with(|remaining| remaining.set(None));
        }
    }
    FAILURE_TRIGGERED.with(|triggered| triggered.set(false));
    FAIL_AFTER.with(|remaining| remaining.set(Some(successful_allocations)));
    let guard = Disarm;
    let result = operation();
    drop(guard); // Assertions and error formatting always run with injection disabled.
    (result, FAILURE_TRIGGERED.with(Cell::get))
}

fn track<T>(operation: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|counts| counts.set([0; 3]));
    TRACK.with(|track| track.set(true));
    let result = operation();
    TRACK.with(|track| track.set(false));
    (result, COUNTS.with(Cell::get))
}

fn format(rate: u32, channels: u16) -> AudioFormat {
    AudioFormat::new(rate, channels).unwrap()
}

/// Deterministic source addressed by absolute source frame and channel.
struct Signal {
    channels: usize,
    next: u64,
    largest_block: usize,
    value: fn(u64, usize) -> f32,
}

impl Signal {
    fn new(channels: u16, value: fn(u64, usize) -> f32) -> Self {
        Self {
            channels: usize::from(channels),
            next: 0,
            largest_block: 0,
            value,
        }
    }
    fn fill(&mut self, block: &mut [f32]) -> Result<usize, AudioError> {
        assert_eq!(block.len() % self.channels, 0);
        let frames = block.len() / self.channels;
        for (offset, frame) in block.chunks_exact_mut(self.channels).enumerate() {
            for (channel, sample) in frame.iter_mut().enumerate() {
                *sample = (self.value)(self.next + offset as u64, channel);
            }
        }
        self.next += frames as u64;
        self.largest_block = self.largest_block.max(frames);
        Ok(frames)
    }
    fn at(&self, frame: i64, channel: usize) -> f64 {
        if frame < 0 {
            0.0
        } else {
            f64::from((self.value)(frame as u64, channel))
        }
    }
}

fn ramp(frame: u64, channel: usize) -> f32 {
    ((frame % 1000) as f32 / 1000.0 - 0.5) * if channel == 0 { 1.0 } else { -0.5 }
}

fn tone(frame: u64, channel: usize) -> f32 {
    (0.5 * (2.0 * PI * 997.0 * frame as f64 / 44_100.0 + channel as f64).sin()) as f32
}

fn render_partitioned(
    converter: &mut FormatConverter,
    signal: &mut Signal,
    total_frames: usize,
    partitions: &[usize],
) -> Vec<f32> {
    let channels = usize::from(converter.target_format().channels());
    let mut output = vec![0.0; total_frames * channels];
    let mut start = 0;
    let mut index = 0;
    while start < total_frames {
        let frames = partitions[index % partitions.len()].min(total_frames - start);
        index += 1;
        let block = &mut output[start * channels..(start + frames) * channels];
        converter
            .render(block, |source| signal.fill(source))
            .unwrap();
        start += frames;
    }
    output
}

fn linear_oracle(signal: &Signal, source_rate: u32, target_rate: u32, frames: usize) -> Vec<f64> {
    let mut expected = Vec::new();
    for frame in 0..frames as u128 {
        let position = frame * u128::from(source_rate);
        let whole = (position / u128::from(target_rate)) as i64;
        let phase = (position % u128::from(target_rate)) as f64 / f64::from(target_rate);
        for channel in 0..signal.channels {
            let low = signal.at(whole, channel);
            let high = signal.at(whole + 1, channel);
            expected.push(low * (1.0 - phase) + high * phase);
        }
    }
    expected
}

fn assert_close(actual: &[f32], expected: &[f64], tolerance: f64) {
    assert_eq!(actual.len(), expected.len());
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (f64::from(*actual) - expected).abs() <= tolerance,
            "sample {index}: {actual} != {expected}"
        );
    }
}

// This oracle sums absolute rational time in u128; it does not use the
// converter's recurrence, phase accessor or target-dependent phase helpers.
#[derive(Clone, Copy)]
struct ExactTime {
    numerator: u128,
    denominator: u128,
}
impl ExactTime {
    fn zero() -> Self {
        Self {
            numerator: 0,
            denominator: 1,
        }
    }
    fn add_frames(&mut self, frames: usize, source: u32, target: u32) {
        self.numerator = self.numerator * u128::from(target)
            + frames as u128 * u128::from(source) * self.denominator;
        self.denominator *= u128::from(target);
        let mut a = self.numerator;
        let mut b = self.denominator;
        while b != 0 {
            (a, b) = (b, a % b);
        }
        self.numerator /= a;
        self.denominator /= a;
    }
    fn check(self, converter: &FormatConverter) {
        let actual = converter.source_position();
        assert_eq!(u128::from(actual.frame), self.numerator / self.denominator);
        assert_eq!(
            u128::from(actual.numerator) * self.denominator,
            (self.numerator % self.denominator) * u128::from(actual.denominator)
        );
        assert!(actual.denominator > 0 && actual.numerator < actual.denominator);
    }
    fn linear(self, signal: &Signal, channel: usize) -> f64 {
        let whole = (self.numerator / self.denominator) as i64;
        let fraction = (self.numerator % self.denominator) as f64 / self.denominator as f64;
        signal.at(whole, channel) * (1.0 - fraction) + signal.at(whole + 1, channel) * fraction
    }
}

fn continuous(quality: ResampleQuality, rate: u32, capacity: usize) -> FormatConverter {
    FormatConverter::new_continuous(
        format(44_100, 2),
        format(rate, 2),
        ChannelMatrix::default_mix(2, 2).unwrap(),
        quality,
        capacity,
    )
    .unwrap()
}

#[test]
fn retarget_piecewise_absolute_rational_and_pcm_oracle() {
    let mut converter = continuous(ResampleQuality::Linear, 48_000, 64);
    let mut signal = Signal::new(2, ramp);
    let mut time = ExactTime::zero();
    for (rate, frames) in [(48_000, 17), (44_100, 19), (32_000, 23), (48_000, 31)] {
        let before = (
            converter.source_position(),
            converter.source_frame_cursor(),
            converter.output_frame_cursor(),
        );
        converter
            .retarget(
                format(rate, 2),
                ChannelMatrix::default_mix(2, 2).unwrap(),
                64,
            )
            .unwrap();
        assert_eq!(
            before,
            (
                converter.source_position(),
                converter.source_frame_cursor(),
                converter.output_frame_cursor()
            )
        );
        let mut expected = Vec::new();
        for _ in 0..frames {
            for channel in 0..2 {
                expected.push(time.linear(&signal, channel));
            }
            time.add_frames(1, 44_100, rate);
        }
        let output = render_partitioned(&mut converter, &mut signal, frames, &[1, 7, 3]);
        assert_close(&output, &expected, 1e-6);
        time.check(&converter);
        assert_eq!(converter.source_frame_cursor(), signal.next);
        assert!(signal.largest_block <= converter.max_source_frames());
    }
    assert_eq!(converter.output_frame_cursor(), 90);
}

#[test]
fn retarget_same_boundary_partitioning_preserves_linear_and_sinc_history() {
    for quality in [
        ResampleQuality::Linear,
        ResampleQuality::WindowedSinc { half_taps: 16 },
    ] {
        let run = |partitions: &[usize]| {
            let mut converter = continuous(quality, 48_000, 128);
            let mut signal = Signal::new(2, tone);
            let mut output = Vec::new();
            let mut time = ExactTime::zero();
            for (rate, frames) in [(48_000, 97), (44_100, 113), (32_000, 127), (48_000, 119)] {
                converter
                    .retarget(
                        format(rate, 2),
                        ChannelMatrix::default_mix(2, 2).unwrap(),
                        128,
                    )
                    .unwrap();
                let block = render_partitioned(&mut converter, &mut signal, frames, partitions);
                if matches!(quality, ResampleQuality::WindowedSinc { .. }) {
                    for (index, sample) in block.iter().enumerate() {
                        let x = time.numerator as f64 / time.denominator as f64;
                        if x > 40.0 {
                            let expected =
                                0.5 * (2.0 * PI * 997.0 * x / 44_100.0 + (index % 2) as f64).sin();
                            assert!(
                                (f64::from(*sample) - expected).abs() < 0.004,
                                "rate {rate}, source {x}, sample {sample}, expected {expected}"
                            );
                        }
                        if index % 2 == 1 {
                            time.add_frames(1, 44_100, rate);
                        }
                    }
                } else {
                    time.add_frames(frames, 44_100, rate);
                }
                time.check(&converter);
                output.extend(block);
            }
            (
                output,
                converter.source_position(),
                converter.source_frame_cursor(),
            )
        };
        assert_eq!(run(&[128]), run(&[1, 31, 7, 53]));
    }
}

#[test]
fn retarget_capacity_refusal_and_channel_matrix_changes_are_atomic() {
    let mut changed = continuous(ResampleQuality::Linear, 48_000, 64);
    let mut baseline = continuous(ResampleQuality::Linear, 48_000, 64);
    let mut a = Signal::new(2, ramp);
    let mut b = Signal::new(2, ramp);
    assert_eq!(
        render_partitioned(&mut changed, &mut a, 17, &[17]),
        render_partitioned(&mut baseline, &mut b, 17, &[17])
    );
    let before = (
        changed.source_position(),
        changed.source_frame_cursor(),
        changed.output_frame_cursor(),
        changed.target_format(),
    );
    for capacity in [0, AudioLimits::MAX_RENDER_FRAMES + 1] {
        assert!(changed
            .retarget(
                format(32_000, 2),
                ChannelMatrix::default_mix(2, 2).unwrap(),
                capacity
            )
            .is_err());
        assert_eq!(
            before,
            (
                changed.source_position(),
                changed.source_frame_cursor(),
                changed.output_frame_cursor(),
                changed.target_format()
            )
        );
    }
    assert!(changed
        .retarget(
            format(32_000, 1),
            ChannelMatrix::default_mix(2, 2).unwrap(),
            64
        )
        .is_err());
    assert_eq!(
        render_partitioned(&mut changed, &mut a, 41, &[13, 1]),
        render_partitioned(&mut baseline, &mut b, 41, &[13, 1])
    );
    for capacity in [128, 16, 64] {
        changed
            .retarget(
                format(48_000, 2),
                ChannelMatrix::default_mix(2, 2).unwrap(),
                capacity,
            )
            .unwrap();
        baseline
            .retarget(
                format(48_000, 2),
                ChannelMatrix::default_mix(2, 2).unwrap(),
                64,
            )
            .unwrap();
        assert_eq!(
            render_partitioned(&mut changed, &mut a, 31, &[7, 1]),
            render_partitioned(&mut baseline, &mut b, 31, &[7, 1])
        );
    }
    let position = changed.source_position();
    let time = ExactTime {
        numerator: u128::from(position.frame) * u128::from(position.denominator)
            + u128::from(position.numerator),
        denominator: u128::from(position.denominator),
    };
    changed
        .retarget(
            format(32_000, 1),
            ChannelMatrix::new(2, 1, &[0.25, 0.75]).unwrap(),
            64,
        )
        .unwrap();
    let output = render_partitioned(&mut changed, &mut a, 1, &[1]);
    assert_close(
        &output,
        &[time.linear(&a, 0) * 0.25 + time.linear(&a, 1) * 0.75],
        1e-6,
    );
    assert_eq!(changed.source_format(), format(44_100, 2));
}

#[test]
fn retarget_source_refusal_and_zero_extent_preserve_converter_position() {
    let mut converter = continuous(ResampleQuality::Linear, 48_000, 64);
    let mut signal = Signal::new(2, ramp);
    render_partitioned(&mut converter, &mut signal, 17, &[17]);
    converter
        .retarget(
            format(32_000, 2),
            ChannelMatrix::default_mix(2, 2).unwrap(),
            64,
        )
        .unwrap();
    let before = (
        converter.source_position(),
        converter.source_frame_cursor(),
        converter.output_frame_cursor(),
    );
    let mut failed_calls = 0;
    assert_eq!(
        converter.render(&mut [0.0; 64], |_| {
            failed_calls += 1;
            Err::<(), _>(AudioError::InvalidBuffer)
        }),
        Err(AudioError::InvalidBuffer)
    );
    assert_eq!(failed_calls, 1); // The source's own effects are not rolled back.
    assert_eq!(
        before,
        (
            converter.source_position(),
            converter.source_frame_cursor(),
            converter.output_frame_cursor()
        )
    );
    converter
        .render(&mut [], |block| {
            assert!(block.is_empty());
            Ok::<_, AudioError>(())
        })
        .unwrap();
    assert_eq!(
        before,
        (
            converter.source_position(),
            converter.source_frame_cursor(),
            converter.output_frame_cursor()
        )
    );
    let mut time = ExactTime {
        numerator: u128::from(before.0.frame) * u128::from(before.0.denominator)
            + u128::from(before.0.numerator),
        denominator: u128::from(before.0.denominator),
    };
    let mut expected = Vec::new();
    for _ in 0..32 {
        for channel in 0..2 {
            expected.push(time.linear(&signal, channel));
        }
        time.add_frames(1, 44_100, 32_000);
    }
    let output = render_partitioned(&mut converter, &mut signal, 32, &[32]);
    assert_close(&output, &expected, 1e-6);
    time.check(&converter);
}

#[test]
fn retarget_large_coprime_rates_refuse_exact_denominator_overflow() {
    let source = format(1, 1);
    let rates = [4_294_967_291, 4_294_967_279, 4_294_967_231];
    let mut converter = FormatConverter::new_continuous(
        source,
        format(rates[0], 1),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        1,
    )
    .unwrap();
    let mut signal = Signal::new(1, ramp);
    let mut oracle = ExactTime::zero();
    for rate in &rates[..2] {
        converter
            .retarget(
                format(*rate, 1),
                ChannelMatrix::default_mix(1, 1).unwrap(),
                1,
            )
            .unwrap();
        render_partitioned(&mut converter, &mut signal, 1, &[1]);
        oracle.add_frames(1, 1, *rate);
        oracle.check(&converter);
    }
    let before = (
        converter.source_position(),
        converter.source_frame_cursor(),
        converter.output_frame_cursor(),
    );
    assert!(converter
        .retarget(
            format(rates[2], 1),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            1
        )
        .is_err());
    assert_eq!(
        before,
        (
            converter.source_position(),
            converter.source_frame_cursor(),
            converter.output_frame_cursor()
        )
    );
    let mut output = [0.0];
    converter
        .render(&mut output, |block| signal.fill(block))
        .unwrap();
    assert_close(&output, &[oracle.linear(&signal, 0)], 1e-6);
    oracle.add_frames(1, 1, rates[1]);
    oracle.check(&converter);
    let frontier = converter.source_frame_cursor();
    assert!(converter
        .retarget(format(1, 1), ChannelMatrix::default_mix(1, 1).unwrap(), 1)
        .is_ok());
    assert_eq!(converter.source_frame_cursor(), frontier);
    // The opposite extreme is refused cold rather than attempting a huge pull.
    let mut huge = FormatConverter::new_continuous(
        format(u32::MAX, 1),
        format(u32::MAX, 1),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        1,
    )
    .unwrap();
    assert!(huge
        .retarget(format(1, 1), ChannelMatrix::default_mix(1, 1).unwrap(), 1)
        .is_err());
    assert_eq!(huge.output_frame_cursor(), 0);
}

#[test]
fn retargeted_callbacks_do_not_allocate_reallocate_or_deallocate() {
    for quality in [
        ResampleQuality::Linear,
        ResampleQuality::WindowedSinc { half_taps: 16 },
    ] {
        let mut converter = continuous(quality, 48_000, 64);
        let mut signal = Signal::new(2, tone);
        let mut output = [0.0; 128];
        converter
            .render(&mut output[..34], |block| signal.fill(block))
            .unwrap();
        for rate in [44_100, 32_000, 48_000] {
            converter
                .retarget(
                    format(rate, 2),
                    ChannelMatrix::default_mix(2, 2).unwrap(),
                    64,
                )
                .unwrap();
            let (_, counts) = track(|| {
                for frames in [0, 1, 64, 7, 31] {
                    converter
                        .render(&mut output[..frames * 2], |block| signal.fill(block))
                        .unwrap();
                }
            });
            assert_eq!(counts, [0, 0, 0]);
        }
    }
}

#[test]
fn continuous_initial_equal_rate_retains_sinc_history_for_later_conversion() {
    let mut converter = continuous(ResampleQuality::WindowedSinc { half_taps: 16 }, 44_100, 64);
    let mut signal = Signal::new(2, tone);
    let equal = render_partitioned(&mut converter, &mut signal, 53, &[19, 1]);
    let expected: Vec<_> = (0..53)
        .flat_map(|frame| (0..2).map(move |channel| f64::from(tone(frame, channel))))
        .collect();
    assert_close(&equal, &expected, 0.0);
    converter
        .retarget(
            format(48_000, 2),
            ChannelMatrix::default_mix(2, 2).unwrap(),
            64,
        )
        .unwrap();
    let converted = render_partitioned(&mut converter, &mut signal, 31, &[1, 7]);
    for (index, value) in converted.iter().enumerate() {
        let position = 53.0 + (index / 2) as f64 * 44_100.0 / 48_000.0;
        let expected = 0.5 * (2.0 * PI * 997.0 * position / 44_100.0 + (index % 2) as f64).sin();
        assert!((f64::from(*value) - expected).abs() < 0.004);
    }
}

#[test]
fn legacy_fixed_rate_converter_refuses_retarget_without_losing_pcm() {
    let mut converter = FormatConverter::new(
        format(44_100, 2),
        format(48_000, 2),
        ChannelMatrix::default_mix(2, 2).unwrap(),
        ResampleQuality::Linear,
        64,
    )
    .unwrap();
    assert_eq!(
        converter.retarget(
            format(44_100, 2),
            ChannelMatrix::default_mix(2, 2).unwrap(),
            64
        ),
        Err(AudioError::InvalidCapacity)
    );
    let mut signal = Signal::new(2, ramp);
    let output = render_partitioned(&mut converter, &mut signal, 31, &[13, 1]);
    assert_close(&output, &linear_oracle(&signal, 44_100, 48_000, 31), 1e-6);
}

#[test]
fn cold_retarget_allocation_refusal_preserves_complete_state_pcm_and_retry() {
    // Retarget allocates its new source window first, then the sinc table.
    // Index 1 therefore fails after retained PCM has been copied into the new
    // window, exercising refusal late in preparation before atomic commit.
    for (quality, allocation_index) in [
        (ResampleQuality::Linear, 0),
        (ResampleQuality::WindowedSinc { half_taps: 16 }, 0),
        (ResampleQuality::WindowedSinc { half_taps: 16 }, 1),
    ] {
        let mut changed = continuous(quality, 48_000, 64);
        let mut control = continuous(quality, 48_000, 64);
        let dirty_signal = |frame: u64, channel: usize| {
            if frame == 2 && channel == 0 {
                f32::NAN
            } else {
                tone(frame, channel)
            }
        };
        let mut a = Signal::new(2, dirty_signal);
        let mut b = Signal::new(2, dirty_signal);
        assert_eq!(
            render_partitioned(&mut changed, &mut a, 17, &[17]),
            render_partitioned(&mut control, &mut b, 17, &[17])
        );
        assert!(changed.source_position().numerator > 0);
        assert!(changed.sanitized_samples() > 0);
        let before = (
            changed.source_position(),
            changed.source_frame_cursor(),
            changed.output_frame_cursor(),
            changed.sanitized_samples(),
            changed.source_format(),
            changed.target_format(),
            changed.quality(),
            changed.max_output_frames(),
            changed.max_source_frames(),
            changed.source_lookahead_frames(),
        );
        let old_matrix = changed.matrix().coefficients().to_vec();
        let target = format(32_000, 1);
        // Matrix allocation occurs before injection, leaving exactly the
        // converter window and optional kernel allocations under observation.
        let rejected_matrix = ChannelMatrix::new(2, 1, &[0.25, 0.75]).unwrap();
        let retry_matrix = ChannelMatrix::new(2, 1, &[0.25, 0.75]).unwrap();
        let control_matrix = ChannelMatrix::new(2, 1, &[0.25, 0.75]).unwrap();
        let source_before = a.next;
        let (result, triggered) = refuse_allocation(allocation_index, || {
            changed.retarget(target, rejected_matrix, 128)
        });
        assert!(
            triggered,
            "cold allocation index {allocation_index} was not reached"
        );
        assert_eq!(result, Err(AudioError::AllocationFailed));
        assert_eq!(
            before,
            (
                changed.source_position(),
                changed.source_frame_cursor(),
                changed.output_frame_cursor(),
                changed.sanitized_samples(),
                changed.source_format(),
                changed.target_format(),
                changed.quality(),
                changed.max_output_frames(),
                changed.max_source_frames(),
                changed.source_lookahead_frames(),
            )
        );
        assert_eq!(changed.matrix().source_channels(), 2);
        assert_eq!(changed.matrix().target_channels(), 2);
        assert_eq!(changed.matrix().coefficients(), old_matrix);
        assert_eq!(a.next, source_before);
        // Compare audible continuation before retry, including the old pending
        // PCM and past sinc history, rather than only checking scalar getters.
        assert_eq!(
            render_partitioned(&mut changed, &mut a, 37, &[7, 1, 13]),
            render_partitioned(&mut control, &mut b, 37, &[7, 1, 13])
        );
        changed.retarget(target, retry_matrix, 128).unwrap();
        control.retarget(target, control_matrix, 128).unwrap();
        assert_eq!(
            render_partitioned(&mut changed, &mut a, 91, &[1, 31, 9]),
            render_partitioned(&mut control, &mut b, 91, &[1, 31, 9])
        );
        assert_eq!(changed.source_position(), control.source_position());
        assert_eq!(changed.source_frame_cursor(), control.source_frame_cursor());
        assert_eq!(changed.output_frame_cursor(), control.output_frame_cursor());
        assert_eq!(changed.sanitized_samples(), control.sanitized_samples());
        assert_eq!(a.next, b.next);
    }
}

#[test]
fn kernel_width_is_not_actual_pulled_source_progress() {
    for (source_rate, target_rate, pulled) in [(48_000, 8_000, 6), (24_000, 48_000, 2)] {
        let mut converter = FormatConverter::new(
            format(source_rate, 1),
            format(target_rate, 1),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            1,
        )
        .unwrap();
        let mut output = [0.0];
        let actual = converter
            .render(&mut output, |block| {
                block.fill(0.25);
                Ok::<_, AudioError>(block.len())
            })
            .unwrap();
        assert_eq!(actual, pulled);
        assert_eq!(converter.source_frame_cursor(), pulled as u64);
        assert_eq!(converter.output_frame_cursor(), 1);
        assert_eq!(converter.source_lookahead_frames(), 1);
        assert_eq!(output, [0.25]);
    }
}

#[test]
fn default_channel_matrices_are_layout_agnostic_and_validated() {
    let identity = ChannelMatrix::default_mix(2, 2).unwrap();
    assert!(identity.is_identity());
    assert_eq!(identity.coefficients(), &[1.0, 0.0, 0.0, 1.0]);
    let up = ChannelMatrix::default_mix(1, 4).unwrap();
    assert_eq!(up.coefficients(), &[1.0, 1.0, 0.0, 0.0]);
    assert!(!up.is_identity());
    assert_eq!(
        ChannelMatrix::default_mix(3, 1).unwrap().coefficients(),
        &[1.0 / 3.0; 3]
    );
    assert_eq!(
        ChannelMatrix::default_mix(2, 3).unwrap().coefficients(),
        &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
    );
    assert_eq!(
        ChannelMatrix::default_mix(3, 2).unwrap().coefficients(),
        &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
    );
    let mono = ChannelMatrix::default_mix(1, 1).unwrap();
    assert!(mono.is_identity());
    assert_eq!((mono.source_channels(), mono.target_channels()), (1, 1));

    assert_eq!(
        ChannelMatrix::default_mix(0, 2),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(
        ChannelMatrix::default_mix(1, 0),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(
        ChannelMatrix::default_mix(33, 2),
        Err(AudioError::InvalidFormat)
    );
    assert!(ChannelMatrix::default_mix(32, 32).unwrap().is_identity());
    assert_eq!(
        ChannelMatrix::new(2, 2, &[1.0; 3]),
        Err(AudioError::InvalidBuffer)
    );
    assert_eq!(
        ChannelMatrix::new(1, 1, &[f32::NAN]),
        Err(AudioError::NonFiniteSample)
    );
    assert!(!ChannelMatrix::new(2, 2, &[1.0, 0.0, 0.0, 0.5])
        .unwrap()
        .is_identity());
}

#[test]
fn matching_formats_render_directly_without_lookahead() {
    let stereo = format(48_000, 2);
    let mut converter = FormatConverter::new(
        stereo,
        stereo,
        ChannelMatrix::default_mix(2, 2).unwrap(),
        ResampleQuality::Linear,
        8,
    )
    .unwrap();
    assert_eq!(converter.source_lookahead_frames(), 0);
    assert_eq!(converter.max_source_frames(), 8);
    let mut signal = Signal::new(2, ramp);
    let output = render_partitioned(&mut converter, &mut signal, 21, &[8, 1, 5, 0, 7]);
    let expected: Vec<f64> = (0..21)
        .flat_map(|frame| [signal.at(frame, 0), signal.at(frame, 1)])
        .collect();
    assert_close(&output, &expected, 0.0);
    assert_eq!(converter.output_frame_cursor(), 21);
    assert_eq!(converter.source_frame_cursor(), 21);
    assert_eq!(signal.next, 21);
}

#[test]
fn channel_only_conversion_mixes_each_frame_and_clamps_once() {
    let mut down = FormatConverter::new(
        format(48_000, 2),
        format(48_000, 1),
        ChannelMatrix::default_mix(2, 1).unwrap(),
        ResampleQuality::Linear,
        4,
    )
    .unwrap();
    let mut output = [0.0; 3];
    let frames = down
        .render(&mut output, |block| {
            block.copy_from_slice(&[1.0, 0.5, -1.0, -1.0, 0.25, -0.75]);
            Ok::<_, AudioError>(block.len() / 2)
        })
        .unwrap();
    assert_eq!(frames, 3);
    assert_eq!(output, [0.75, -1.0, -0.25]);

    let gain = ChannelMatrix::new(1, 2, &[2.0, -0.5]).unwrap();
    let mut up = FormatConverter::new(
        format(44_100, 1),
        format(44_100, 2),
        gain,
        ResampleQuality::Linear,
        2,
    )
    .unwrap();
    let mut output = [0.0; 4];
    up.render(&mut output, |block| {
        block.copy_from_slice(&[0.75, -0.25]);
        Ok::<_, AudioError>(())
    })
    .unwrap();
    assert_eq!(output, [1.0, -0.375, -0.5, 0.125]);
}

#[test]
fn linear_44100_to_48000_matches_rational_oracle_across_partitions() {
    let (source, target) = (format(44_100, 2), format(48_000, 2));
    let build = || {
        FormatConverter::new(
            source,
            target,
            ChannelMatrix::default_mix(2, 2).unwrap(),
            ResampleQuality::Linear,
            512,
        )
        .unwrap()
    };
    let mut whole = build();
    assert_eq!(whole.source_lookahead_frames(), 1);
    let mut signal = Signal::new(2, ramp);
    let single = render_partitioned(&mut whole, &mut signal, 4_000, &[512]);
    assert!(signal.largest_block <= whole.max_source_frames());
    assert_close(
        &single,
        &linear_oracle(&signal, 44_100, 48_000, 4_000),
        1e-6,
    );

    for partitions in [&[1usize][..], &[3, 0, 160, 511, 7], &[147, 160, 2]] {
        let mut converter = build();
        let mut partitioned_signal = Signal::new(2, ramp);
        let partitioned =
            render_partitioned(&mut converter, &mut partitioned_signal, 4_000, partitions);
        assert_eq!(partitioned, single, "partition {partitions:?}");
        assert!(partitioned_signal.largest_block <= converter.max_source_frames());
        assert_eq!(converter.output_frame_cursor(), 4_000);
        // Pulled source stays contiguous and only one frame of lookahead ahead.
        assert_eq!(converter.source_frame_cursor(), partitioned_signal.next);
        let consumed = 4_000u64 * 44_100 / 48_000;
        assert!(partitioned_signal.next > consumed);
        assert!(partitioned_signal.next <= consumed + 2);
    }
}

#[test]
fn downsampling_pulls_skipped_source_frames_contiguously() {
    for (source_rate, target_rate) in [(48_000, 22_050), (192_000, 8_000), (96_000, 44_100)] {
        let mut converter = FormatConverter::new(
            format(source_rate, 1),
            format(target_rate, 1),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            64,
        )
        .unwrap();
        let mut signal = Signal::new(1, ramp);
        let single = render_partitioned(&mut converter, &mut signal, 900, &[64, 1, 17]);
        assert!(signal.largest_block <= converter.max_source_frames());
        assert_eq!(converter.source_frame_cursor(), signal.next);
        assert_close(
            &single,
            &linear_oracle(&signal, source_rate, target_rate, 900),
            1e-6,
        );
    }
}

#[test]
fn windowed_sinc_preserves_dc_and_band_limited_tone() {
    let quality = ResampleQuality::WindowedSinc { half_taps: 16 };
    let mut converter = FormatConverter::new(
        format(44_100, 1),
        format(48_000, 1),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        quality,
        256,
    )
    .unwrap();
    assert_eq!(converter.source_lookahead_frames(), 16);
    assert_eq!(converter.quality(), quality);
    let mut dc = Signal::new(1, |_, _| 0.5);
    let output = render_partitioned(&mut converter, &mut dc, 2_000, &[256, 3]);
    // Frames whose taps all follow source frame zero carry exact unity DC gain.
    for sample in &output[20..] {
        assert!((sample - 0.5).abs() < 1e-6, "{sample}");
    }

    let build = || {
        FormatConverter::new(
            format(44_100, 2),
            format(48_000, 2),
            ChannelMatrix::default_mix(2, 2).unwrap(),
            quality,
            256,
        )
        .unwrap()
    };
    let mut converter = build();
    let mut signal = Signal::new(2, tone);
    let single = render_partitioned(&mut converter, &mut signal, 4_800, &[256]);
    let mut worst = 0.0f64;
    for (index, sample) in single.iter().enumerate().skip(2 * 40) {
        let (frame, channel) = (index / 2, index % 2);
        let time = frame as f64 / 48_000.0;
        let expected = 0.5 * (2.0 * PI * 997.0 * time + channel as f64).sin();
        worst = worst.max((f64::from(*sample) - expected).abs());
    }
    assert!(worst < 2e-3, "worst error {worst}");
    let mut partitioned = build();
    let mut partitioned_signal = Signal::new(2, tone);
    let split = render_partitioned(
        &mut partitioned,
        &mut partitioned_signal,
        4_800,
        &[1, 255, 31],
    );
    assert_eq!(split, single);
}

#[test]
fn windowed_sinc_downsampling_attenuates_tones_above_target_nyquist() {
    let mut converter = FormatConverter::new(
        format(96_000, 1),
        format(48_000, 1),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::WindowedSinc { half_taps: 32 },
        480,
    )
    .unwrap();
    let mut signal = Signal::new(1, |frame, _| {
        (0.5 * (2.0 * PI * 40_000.0 * frame as f64 / 96_000.0).sin()) as f32
    });
    let output = render_partitioned(&mut converter, &mut signal, 4_800, &[480]);
    let rms = (output[100..]
        .iter()
        .map(|v| f64::from(*v).powi(2))
        .sum::<f64>()
        / (output.len() - 100) as f64)
        .sqrt();
    // Input RMS is about 0.354; linear decimation would alias it at full level.
    assert!(rms < 0.01, "alias rms {rms}");
}

#[test]
fn invalid_buffers_and_source_errors_leave_phase_unchanged() {
    let build = || {
        FormatConverter::new(
            format(44_100, 2),
            format(48_000, 1),
            ChannelMatrix::default_mix(2, 1).unwrap(),
            ResampleQuality::WindowedSinc { half_taps: 4 },
            32,
        )
        .unwrap()
    };
    let mut converter = build();
    let mut signal = Signal::new(2, tone);
    let mut called = false;
    let mut odd = [0.0; 33];
    assert_eq!(
        converter.render(&mut odd, |_| {
            called = true;
            Ok::<_, AudioError>(())
        }),
        Err(AudioError::RenderCapacity)
    );
    assert!(!called);
    let mut first = [0.0; 10];
    converter
        .render(&mut first, |block| signal.fill(block))
        .unwrap();
    let mut failed = [0.25; 13];
    let before = signal.next;
    assert_eq!(
        converter.render(&mut failed, |block| {
            block.fill(0.75);
            Err::<usize, _>(AudioError::Overflow)
        }),
        Err(AudioError::Overflow)
    );
    assert_eq!(signal.next, before);
    assert_eq!(converter.output_frame_cursor(), 10);
    let mut rest = [0.0; 30];
    converter
        .render(&mut rest, |block| signal.fill(block))
        .unwrap();

    let mut reference = build();
    let mut reference_signal = Signal::new(2, tone);
    let expected = render_partitioned(&mut reference, &mut reference_signal, 40, &[32]);
    assert_eq!(&expected[..10], &first);
    assert_eq!(&expected[10..], &rest);

    let mut stereo = FormatConverter::new(
        format(44_100, 2),
        format(48_000, 2),
        ChannelMatrix::default_mix(2, 2).unwrap(),
        ResampleQuality::Linear,
        4,
    )
    .unwrap();
    let mut misaligned = [0.0; 3];
    assert_eq!(
        stereo.render(&mut misaligned, |_| Ok::<_, AudioError>(())),
        Err(AudioError::InvalidBuffer)
    );
    assert_eq!(stereo.output_frame_cursor(), 0);
}

#[test]
fn setup_rejects_mismatched_matrices_bad_quality_and_unbounded_source_blocks() {
    let source = format(48_000, 2);
    let target = format(44_100, 2);
    let stereo = || ChannelMatrix::default_mix(2, 2).unwrap();
    assert_eq!(
        FormatConverter::new(
            source,
            format(44_100, 1),
            stereo(),
            ResampleQuality::Linear,
            8
        )
        .unwrap_err(),
        AudioError::ChannelMismatch
    );
    for half_taps in [0, 1, 33] {
        assert_eq!(
            FormatConverter::new(
                source,
                source,
                stereo(),
                ResampleQuality::WindowedSinc { half_taps },
                8
            )
            .unwrap_err(),
            AudioError::InvalidCapacity
        );
    }
    for frames in [0, AudioLimits::MAX_RENDER_FRAMES + 1] {
        assert_eq!(
            FormatConverter::required_source_frames(
                source,
                target,
                ResampleQuality::Linear,
                frames
            ),
            Err(AudioError::InvalidCapacity)
        );
    }
    assert_eq!(
        FormatConverter::required_source_frames(
            format(u32::MAX, 1),
            format(1, 1),
            ResampleQuality::Linear,
            1
        ),
        Err(AudioError::RenderCapacity)
    );
    assert_eq!(
        FormatConverter::required_source_frames(
            format(1, 1),
            format(u32::MAX, 1),
            ResampleQuality::WindowedSinc { half_taps: 32 },
            AudioLimits::MAX_RENDER_FRAMES
        ),
        Ok(65)
    );
    // 48 kHz to 44.1 kHz: ceil(511 * 160 / 147) + 2 frames for 512 outputs.
    assert_eq!(
        FormatConverter::required_source_frames(source, target, ResampleQuality::Linear, 512),
        Ok(559)
    );
}

#[test]
fn non_finite_source_samples_become_counted_silence() {
    let mut converter = FormatConverter::new(
        format(48_000, 1),
        format(48_000, 1),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        4,
    )
    .unwrap();
    let mut output = [0.0; 3];
    converter
        .render(&mut output, |block| {
            block.copy_from_slice(&[f32::NAN, 0.5, f32::INFINITY]);
            Ok::<_, AudioError>(())
        })
        .unwrap();
    assert_eq!(output, [0.0, 0.5, 0.0]);
    assert_eq!(converter.sanitized_samples(), 2);
}

fn mixer(rate: u32, channels: u16, max_render_frames: usize) -> (CommandProducer, Mixer) {
    let format = format(rate, channels);
    let pcm_limits = PcmLimits::new(1 << 16, 1 << 16, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    let samples = vec![0.5; 400 * usize::from(channels)];
    bank.insert(
        SampleId(1),
        PcmSample::new(format, samples, pcm_limits).unwrap(),
    )
    .unwrap();
    let limits = AudioLimits::new(4, 2, 4, max_render_frames, 4).unwrap();
    let config = MixerConfig::new(format, ClockDomainId(1), Timestamp::ZERO, limits);
    let (producer, consumer) = command_queue(4).unwrap();
    (producer, Mixer::new(config, bank, consumer).unwrap())
}

#[test]
fn mixer_output_publishes_to_a_different_device_rate_and_layout() {
    let target = format(48_000, 2);
    let quality = ResampleQuality::WindowedSinc { half_taps: 8 };
    let required =
        FormatConverter::required_source_frames(format(44_100, 1), target, quality, 128).unwrap();
    let (_, small) = mixer(44_100, 1, required - 1);
    assert_eq!(
        FormatConverter::for_mixer(
            small.configuration(),
            target,
            ChannelMatrix::default_mix(1, 2).unwrap(),
            quality,
            128
        )
        .unwrap_err(),
        AudioError::RenderCapacity
    );

    let (mut producer, mut mixer) = mixer(44_100, 1, required);
    let mut converter = FormatConverter::for_mixer(
        mixer.configuration(),
        target,
        ChannelMatrix::default_mix(1, 2).unwrap(),
        quality,
        128,
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
    let mut output = vec![0.0; 128 * 2];
    let mut physical = 0;
    let mut reports = Vec::new();
    for _ in 0..5 {
        let report = converter
            .render(&mut output, |block| mixer.render(block))
            .unwrap();
        assert_eq!(report.start_frame, physical);
        physical += report.frames as u64;
        reports.push((report, output.clone()));
    }
    assert_eq!(converter.source_frame_cursor(), physical);
    assert_eq!(mixer.frame_cursor(), physical);
    // Steady state inside the 400-frame asset is unity DC on both channels.
    let (_, block) = &reports[1];
    for frame in block.as_chunks::<2>().0 {
        assert!(
            (frame[0] - 0.5).abs() < 1e-6 && frame[0] == frame[1],
            "{frame:?}"
        );
    }
    // The asset ends near output frame 400 * 48000 / 44100 = 435; every tap of
    // the last block follows it.
    assert!(reports[4].1.iter().all(|sample| sample.abs() < 1e-6));
    assert_eq!(reports[4].0.active_voices, 0);
}

#[test]
fn conversion_render_paths_do_not_allocate() {
    let mut cases = [
        FormatConverter::new(
            format(48_000, 2),
            format(48_000, 2),
            ChannelMatrix::default_mix(2, 2).unwrap(),
            ResampleQuality::Linear,
            64,
        )
        .unwrap(),
        FormatConverter::new(
            format(48_000, 2),
            format(48_000, 6),
            ChannelMatrix::default_mix(2, 6).unwrap(),
            ResampleQuality::Linear,
            64,
        )
        .unwrap(),
        FormatConverter::new(
            format(44_100, 2),
            format(48_000, 1),
            ChannelMatrix::default_mix(2, 1).unwrap(),
            ResampleQuality::Linear,
            64,
        )
        .unwrap(),
        FormatConverter::new(
            format(96_000, 2),
            format(44_100, 2),
            ChannelMatrix::default_mix(2, 2).unwrap(),
            ResampleQuality::WindowedSinc { half_taps: 32 },
            64,
        )
        .unwrap(),
    ];
    let mut output = vec![0.0; 65 * 6];
    for converter in &mut cases {
        let mut signal = Signal::new(2, tone);
        let channels = usize::from(converter.target_format().channels());
        let (_, counts) = track(|| {
            for frames in [64, 0, 1, 63, 17] {
                converter
                    .render(&mut output[..frames * channels], |block| signal.fill(block))
                    .unwrap();
            }
            let _ = converter.render(&mut output[..65 * channels], |block| signal.fill(block));
            let _ = converter.render(&mut output[..3], |block| signal.fill(block));
        });
        assert_eq!(counts, [0; 3]);
    }

    let target = format(48_000, 2);
    let quality = ResampleQuality::WindowedSinc { half_taps: 8 };
    let required =
        FormatConverter::required_source_frames(format(44_100, 1), target, quality, 64).unwrap();
    let (mut producer, mut mixer) = mixer(44_100, 1, required);
    let mut converter = FormatConverter::for_mixer(
        mixer.configuration(),
        target,
        ChannelMatrix::default_mix(1, 2).unwrap(),
        quality,
        64,
    )
    .unwrap();
    let (_, counts) = track(|| {
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            })
            .unwrap();
        for _ in 0..10 {
            converter
                .render(&mut output[..128], |block| mixer.render(block))
                .unwrap();
        }
    });
    assert_eq!(counts, [0; 3]);
}
