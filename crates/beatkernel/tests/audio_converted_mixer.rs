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
    static FAIL_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
    static FAILURE_TRIGGERED: Cell<bool> = const { Cell::new(false) };
}
struct Allocator;
fn fail_allocation() -> bool {
    FAIL_AFTER
        .try_with(|remaining| match remaining.get() {
            None => false,
            Some(0) => {
                remaining.set(None); // One shot: error cleanup remains usable.
                let _ = FAILURE_TRIGGERED.try_with(|fired| fired.set(true));
                true
            }
            Some(count) => {
                remaining.set(Some(count - 1));
                false
            }
        })
        .unwrap_or(false)
}
fn refusing<T>(after: usize, operation: impl FnOnce() -> T) -> (T, bool) {
    struct Disarm;
    impl Drop for Disarm {
        fn drop(&mut self) {
            FAIL_AFTER.with(|remaining| remaining.set(None));
        }
    }
    FAILURE_TRIGGERED.with(|fired| fired.set(false));
    FAIL_AFTER.with(|remaining| remaining.set(Some(after)));
    let guard = Disarm;
    let result = operation();
    drop(guard); // Disarm before assertions, formatting or error inspection.
    (result, FAILURE_TRIGGERED.with(Cell::get))
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
// SAFETY: Operations forward their original valid arguments to System or
// return null for an injected allocation refusal, as GlobalAlloc permits.
// Refused realloc leaves the original allocation intact. Scalar thread-local
// accounting allocates no memory; deallocation always reaches System.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        if fail_allocation() {
            return std::ptr::null_mut();
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        if fail_allocation() {
            return std::ptr::null_mut();
        }
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        if fail_allocation() {
            return std::ptr::null_mut();
        }
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
    TRACK.with(|track| track.set(true));
    let result = operation();
    TRACK.with(|track| track.set(false));
    (result, COUNTS.with(Cell::get))
}
fn format(rate: u32, channels: u16) -> AudioFormat {
    AudioFormat::new(rate, channels).unwrap()
}
fn value(frame: usize) -> f32 {
    (frame % 101) as f32 / 256.0 + 0.125
}
fn rig(rate: u32, capacity: usize, end: Option<u64>) -> (CommandProducer, Mixer) {
    let source = format(rate, 1);
    let pcm = PcmLimits::new(16_384, 16_384, 1).unwrap();
    let mut bank = SampleBank::new(source, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(source, (0..4096).map(value).collect(), pcm).unwrap(),
    )
    .unwrap();
    let limits = AudioLimits::new(16, 4, 16, capacity, 16).unwrap();
    let (producer, consumer) = command_queue(16).unwrap();
    let mut config = MixerConfig::new(source, ClockDomainId(7), Timestamp::ZERO, limits);
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn play(producer: &mut CommandProducer, voice: u64, at: i64) {
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(voice),
            sample: SampleId(1),
            at: Timestamp::from_nanos(at),
            gain: 1.0,
        })
        .unwrap();
}
fn owner(
    mixer: Mixer,
    target: AudioFormat,
    quality: ResampleQuality,
    max: usize,
) -> ConvertedMixer {
    match ConvertedMixer::new(
        mixer,
        target,
        ChannelMatrix::default_mix(1, target.channels()).unwrap(),
        quality,
        max,
    ) {
        Ok(owner) => owner,
        Err(failure) => panic!("unexpected construction refusal: {:?}", failure.error()),
    }
}
fn render(owner: &mut ConvertedMixer, output: &mut [f32]) -> ConvertedRenderReport {
    let (result, counts) = track(|| owner.render(output));
    assert_eq!(
        counts, [0; 3],
        "alloc/realloc/dealloc during active callback"
    );
    result.unwrap()
}
fn held(owner: &mut ConvertedMixer, output: &mut [f32]) -> ConvertedRenderReport {
    let (result, counts) = track(|| owner.render_held(output));
    assert_eq!(counts, [0; 3], "alloc/realloc/dealloc during held callback");
    result.unwrap()
}
fn retarget(owner: &mut ConvertedMixer, rate: u32, channels: u16, max: usize) {
    owner
        .retarget(
            format(rate, channels),
            ChannelMatrix::default_mix(1, channels).unwrap(),
            max,
        )
        .unwrap();
}
fn close(actual: f32, expected: f64) {
    assert!(
        (f64::from(actual) - expected).abs() < 2e-6,
        "actual {actual}, expected {expected}"
    );
}

#[test]
fn actual_mixer_piecewise_rate_changes_follow_independent_absolute_pcm_positions() {
    // All selected rates divide this common time denominator. This oracle adds
    // source-time increments directly and does not call converter phase helpers.
    const DEN: u128 = 14_112_000;
    let (mut producer, mixer) = rig(44_100, 256, None);
    play(&mut producer, 1, 0);
    let mut converted = owner(mixer, format(48_000, 1), ResampleQuality::Linear, 32);
    let mut position = 0_u128;
    let mut target_count = 0_u64;
    for (rate, channels, count) in [
        (48_000, 1, 7),
        (44_100, 2, 9),
        (32_000, 1, 11),
        (48_000, 2, 13),
    ] {
        assert_eq!(
            DEN % u128::from(rate),
            0,
            "oracle rate must divide denominator exactly"
        );
        retarget(&mut converted, rate, channels, 32);
        for size in [3, count - 3] {
            let mut output = vec![99.0; size * usize::from(channels)];
            let report = render(&mut converted, &mut output);
            for frame in output.chunks_exact(usize::from(channels)) {
                let whole = (position / DEN) as usize;
                let fraction = (position % DEN) as f64 / DEN as f64;
                let expected = f64::from(value(whole)) * (1.0 - fraction)
                    + f64::from(value(whole + 1)) * fraction;
                for sample in frame {
                    close(*sample, expected);
                }
                position += 44_100_u128 * (DEN / u128::from(rate));
            }
            target_count += size as u64;
            assert_eq!(report.target_frames, size);
            assert_eq!(report.target_frame_cursor, target_count);
            assert_eq!(report.state, ConvertedOutputState::Active);
            let pos = report.source_position;
            assert_eq!(
                u128::from(pos.frame) * DEN
                    + u128::from(pos.numerator) * (DEN / u128::from(pos.denominator)),
                position
            );
            assert_eq!(
                report.pulled_source_frame_cursor,
                converted.mixer().frame_cursor()
            );
            assert!(report.pulled_source_frame_cursor >= pos.frame);
            assert_eq!(report.source.unwrap().counters.commands_consumed, 1);
        }
    }
}

#[test]
fn unique_owner_move_keeps_voices_commands_and_pending_lookahead() {
    let build = || {
        let (mut producer, mixer) = rig(44_100, 256, None);
        play(&mut producer, 1, 0);
        (
            producer,
            owner(
                mixer,
                format(48_000, 1),
                ResampleQuality::WindowedSinc { half_taps: 8 },
                64,
            ),
        )
    };
    let (mut producer, mut moved) = build();
    let (mut reference_producer, mut reference) = build();
    let mut prefix = [0.0; 3];
    let mut reference_prefix = [0.0; 3];
    assert_eq!(
        render(&mut moved, &mut prefix),
        render(&mut reference, &mut reference_prefix)
    );
    assert_eq!(prefix, reference_prefix);
    fn transfer(owner: ConvertedMixer) -> ConvertedMixer {
        owner
    }
    let mut moved = transfer(moved);
    play(&mut producer, 2, 2_000_000);
    play(&mut reference_producer, 2, 2_000_000);
    for (rate, channels) in [(44_100, 2), (32_000, 1), (48_000, 1)] {
        retarget(&mut moved, rate, channels, 64);
        retarget(&mut reference, rate, channels, 64);
        let mut a = vec![0.0; 64 * usize::from(channels)];
        let mut b = a.clone();
        assert_eq!(render(&mut moved, &mut a), render(&mut reference, &mut b));
        assert_eq!(a, b);
    }
    assert_eq!(moved.mixer().counters().commands_consumed, 2);
}

#[test]
fn construction_refusal_returns_original_live_mixer_and_queue() {
    let (mut producer, mut mixer) = rig(44_100, 8, None);
    play(&mut producer, 1, 0);
    let mut first = [0.0; 1];
    mixer.render(&mut first).unwrap();
    assert_eq!(first, [value(0)]);
    let failure = match ConvertedMixer::new(
        mixer,
        format(48_000, 1),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        64,
    ) {
        Err(failure) => failure,
        Ok(_) => panic!("accepted insufficient actual mixer capacity"),
    };
    assert_eq!(failure.error(), &AudioError::RenderCapacity);
    let (error, original) = failure.into_parts();
    assert_eq!(error, AudioError::RenderCapacity);
    let mut original = original.expect("construction refusal returns the original mixer");
    assert_eq!(original.frame_cursor(), 1);
    assert_eq!(original.counters().commands_consumed, 1);
    let mut next = [0.0; 2];
    original.render(&mut next).unwrap();
    assert_eq!(next, [value(1), value(2)]);
    producer
        .try_push(AudioCommand::Stop {
            voice: VoiceId(1),
            at: Timestamp::ZERO,
        })
        .unwrap();
    original.render(&mut next).unwrap();
    assert_eq!(next, [0.0; 2]);
    assert_eq!(original.counters().commands_consumed, 2);
}

#[test]
fn nonzero_original_mixer_basis_is_absolute_without_replaying_consumed_pcm() {
    let (mut producer, mut mixer) = rig(44_100, 128, None);
    play(&mut producer, 1, 0);
    let mut consumed = [0.0; 5];
    mixer.render(&mut consumed).unwrap();
    let mut converted = owner(mixer, format(48_000, 1), ResampleQuality::Linear, 16);
    assert_eq!(
        converted.source_position(),
        SourcePosition {
            frame: 5,
            numerator: 0,
            denominator: 1
        }
    );
    assert_eq!(converted.pulled_source_frame_cursor(), 5);
    let mut output = [0.0; 1];
    let report = render(&mut converted, &mut output);
    assert_eq!(output, [value(5)]);
    assert_eq!(report.source.unwrap().start_frame, 5);
    assert_eq!(
        report.source_position,
        SourcePosition {
            frame: 5,
            numerator: 147,
            denominator: 160
        }
    );
}

#[test]
fn retarget_refusals_preserve_exact_state_pcm_and_retry() {
    let build = || {
        let (mut producer, mixer) = rig(44_100, 128, None);
        play(&mut producer, 1, 0);
        (
            producer,
            owner(
                mixer,
                format(48_000, 1),
                ResampleQuality::WindowedSinc { half_taps: 8 },
                32,
            ),
        )
    };
    let (_producer, mut failed) = build();
    let (_reference_producer, mut reference) = build();
    let mut a = [0.0; 7];
    let mut b = [0.0; 7];
    render(&mut failed, &mut a);
    render(&mut reference, &mut b);
    let pos = failed.source_position();
    let pulled = failed.pulled_source_frame_cursor();
    let target = failed.target_frame_cursor();
    let duration = failed.target_time();
    assert_eq!(
        failed.retarget(
            format(32_000, 2),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            32
        ),
        Err(AudioError::ChannelMismatch)
    );
    assert_eq!(
        failed.retarget(
            format(32_000, 1),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            0
        ),
        Err(AudioError::InvalidCapacity)
    );
    assert_eq!(
        failed.retarget(
            format(8_000, 1),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            128
        ),
        Err(AudioError::RenderCapacity)
    );
    assert_eq!(failed.source_position(), pos);
    assert_eq!(failed.pulled_source_frame_cursor(), pulled);
    assert_eq!(failed.target_frame_cursor(), target);
    assert_eq!(failed.target_time(), duration);
    retarget(&mut failed, 44_100, 1, 32);
    retarget(&mut reference, 44_100, 1, 32);
    let mut a = [0.0; 32];
    let mut b = [0.0; 32];
    assert_eq!(render(&mut failed, &mut a), render(&mut reference, &mut b));
    assert_eq!(a, b);
}

#[test]
fn held_silence_preserves_pending_pcm_pause_request_and_unconsumed_queue() {
    let (mut producer, mixer) = rig(44_100, 256, None);
    play(&mut producer, 1, 0);
    let mut converted = owner(mixer, format(48_000, 1), ResampleQuality::Linear, 32);
    let mut prefix = [0.0; 1];
    render(&mut converted, &mut prefix);
    producer.request_pause(true);
    play(&mut producer, 2, 1_000_000_000);
    let position = converted.source_position();
    let pulled = converted.pulled_source_frame_cursor();
    let counters = converted.mixer().counters();
    retarget(&mut converted, 44_100, 2, 32);
    let mut silence = [99.0; 14];
    let report = held(&mut converted, &mut silence);
    assert_eq!(silence, [0.0; 14]);
    assert_eq!(report.source, None);
    assert_eq!(report.state, ConvertedOutputState::Held);
    assert_eq!(report.target_frames, 7);
    assert_eq!(report.target_frame_cursor, 8);
    assert_eq!(report.source_position, position);
    assert_eq!(report.pulled_source_frame_cursor, pulled);
    assert_eq!(converted.mixer().counters(), counters);
    assert!(converted.mixer().pause_requested());
    assert!(
        !converted.mixer().is_paused(),
        "held output does not adopt source pause"
    );
    producer.request_pause(false);
    let mut resumed = [0.0; 2];
    let resumed_report = render(&mut converted, &mut resumed);
    let fraction = 147.0 / 160.0;
    let expected = f64::from(value(0)) * (1.0 - fraction) + f64::from(value(1)) * fraction;
    for sample in resumed {
        close(sample, expected);
    }
    assert_eq!(
        resumed_report.source.unwrap().counters.commands_consumed,
        1,
        "cached target PCM does not consume queued source commands"
    );
    let mut following = [0.0; 8];
    let following_report = render(&mut converted, &mut following);
    assert_eq!(
        following_report.source.unwrap().counters.commands_consumed,
        2
    );
    assert_eq!(following_report.source.unwrap().pending_commands, 1);
}

#[test]
fn paused_source_report_does_not_mean_cached_target_pcm_is_silent() {
    let (mut producer, mixer) = rig(24_000, 128, None);
    play(&mut producer, 1, 0);
    let mut converted = owner(mixer, format(48_000, 1), ResampleQuality::Linear, 16);
    let mut prefix = [0.0; 1];
    render(&mut converted, &mut prefix);
    producer.request_pause(true);
    let mut output = [99.0; 8];
    let report = render(&mut converted, &mut output);
    assert!(report.source.unwrap().paused);
    close(output[0], (f64::from(value(0)) + f64::from(value(1))) / 2.0);
    assert!(output[0] > 0.0);
    assert_eq!(report.state, ConvertedOutputState::Active);
}

#[test]
fn finite_source_endpoint_is_source_evidence_and_target_extent_is_separate() {
    let (mut producer, mixer) = rig(24_000, 128, Some(2));
    play(&mut producer, 1, 0);
    let mut converted = owner(mixer, format(48_000, 1), ResampleQuality::Linear, 16);
    let mut output = [99.0; 8];
    let report = render(&mut converted, &mut output);
    let source = report.source.unwrap();
    assert_eq!(source.playback_end_physical_frame, Some(2));
    assert_eq!(source.playback_frames, 2);
    assert!(source.paused);
    assert_eq!(report.target_frames, 8);
    assert_eq!(report.target_frame_cursor, 8);
    assert_ne!(source.frames, report.target_frames);
    assert_eq!(
        output,
        [
            value(0),
            (value(0) + value(1)) / 2.0,
            value(1),
            value(1) / 2.0,
            0.0,
            0.0,
            0.0,
            0.0
        ]
    );
    assert_eq!(report.state, ConvertedOutputState::Active);
}

#[test]
fn zero_and_invalid_callbacks_preserve_source_queue_and_both_positions() {
    let (mut producer, mixer) = rig(44_100, 128, None);
    play(&mut producer, 1, 0);
    let mut converted = owner(mixer, format(48_000, 2), ResampleQuality::Linear, 8);
    let initial = converted.source_position();
    let empty = render(&mut converted, &mut []);
    assert_eq!(empty.source.unwrap().frames, 0);
    assert_eq!(empty.source.unwrap().counters.commands_consumed, 0);
    assert_eq!(empty.target_frames, 0);
    assert_eq!(empty.target_frame_cursor, 0);
    let empty_held = held(&mut converted, &mut []);
    assert_eq!(empty_held.source, None);
    let mut odd = [99.0; 3];
    let mut large = [99.0; 18];
    for hold in [false, true] {
        let (result, counts) = track(|| {
            if hold {
                converted.render_held(&mut odd)
            } else {
                converted.render(&mut odd)
            }
        });
        assert_eq!(counts, [0; 3]);
        assert_eq!(result, Err(AudioError::InvalidBuffer));
        let (result, counts) = track(|| {
            if hold {
                converted.render_held(&mut large)
            } else {
                converted.render(&mut large)
            }
        });
        assert_eq!(counts, [0; 3]);
        assert_eq!(result, Err(AudioError::RenderCapacity));
    }
    assert_eq!(odd, [99.0; 3]);
    assert_eq!(large, [99.0; 18]);
    assert_eq!(converted.source_position(), initial);
    assert_eq!(converted.pulled_source_frame_cursor(), 0);
    assert_eq!(converted.target_frame_cursor(), 0);
    let mut output = [0.0; 2];
    assert_eq!(
        render(&mut converted, &mut output)
            .source
            .unwrap()
            .counters
            .commands_consumed,
        1
    );
    assert_eq!(output, [value(0); 2]);
}

#[test]
fn sinc_held_callbacks_after_cold_retargets_do_not_touch_phase_history_or_queue() {
    let build = || {
        let (mut producer, mixer) = rig(44_100, 256, None);
        play(&mut producer, 1, 0);
        (
            producer,
            owner(
                mixer,
                format(48_000, 1),
                ResampleQuality::WindowedSinc { half_taps: 8 },
                64,
            ),
        )
    };
    let (mut producer, mut held_owner) = build();
    let (_reference_producer, mut reference) = build();
    let mut a = [0.0; 7];
    let mut b = [0.0; 7];
    render(&mut held_owner, &mut a);
    render(&mut reference, &mut b);
    assert_eq!(a, b);
    producer.request_pause(true);
    let mut held_frames = 0;
    for (rate, channels, frames) in [(44_100, 2, 5), (32_000, 1, 11), (48_000, 2, 3)] {
        retarget(&mut held_owner, rate, channels, 64);
        retarget(&mut reference, rate, channels, 64);
        let position = held_owner.source_position();
        let frontier = held_owner.pulled_source_frame_cursor();
        let counters = held_owner.mixer().counters();
        let mut output = vec![99.0; frames * usize::from(channels)];
        let report = held(&mut held_owner, &mut output);
        held_frames += frames as u64;
        assert!(output.iter().all(|value| *value == 0.0));
        assert_eq!(report.source, None);
        assert_eq!(report.source_position, position);
        assert_eq!(report.pulled_source_frame_cursor, frontier);
        assert_eq!(held_owner.mixer().counters(), counters);
    }
    producer.request_pause(false);
    let mut a = [0.0; 128];
    let mut b = [0.0; 128];
    let actual = render(&mut held_owner, &mut a);
    let expected = render(&mut reference, &mut b);
    assert_eq!(a, b);
    assert_eq!(actual.source, expected.source);
    assert_eq!(actual.source_position, expected.source_position);
    assert_eq!(
        actual.pulled_source_frame_cursor,
        expected.pulled_source_frame_cursor
    );
    assert_eq!(
        actual.target_frame_cursor,
        expected.target_frame_cursor + held_frames
    );
}

#[test]
fn allocator_calibration_observes_all_three_operations() {
    let (_, counts) = track(|| {
        let mut data = Vec::with_capacity(1);
        data.push(1_u8);
        data.reserve_exact(1024);
        std::hint::black_box(&data);
        drop(data);
    });
    assert!(counts.iter().all(|count| *count > 0));
}

#[test]
fn construction_allocation_refusal_returns_the_original_mixer_for_equivalent_retry() {
    // new_continuous prepares its retained window/table and then invokes the
    // legacy initializer, which prepares another window/table before replacement.
    // Unequal-rate linear has two allocations; sinc has all four. Exercise each
    // refusal, including failures in the later initializer, with original ownership.
    for (quality, failure_index) in [
        (ResampleQuality::Linear, 0),
        (ResampleQuality::Linear, 1),
        (ResampleQuality::WindowedSinc { half_taps: 8 }, 0),
        (ResampleQuality::WindowedSinc { half_taps: 8 }, 1),
        (ResampleQuality::WindowedSinc { half_taps: 8 }, 2),
        (ResampleQuality::WindowedSinc { half_taps: 8 }, 3),
    ] {
        let (mut producer, mut mixer) = rig(44_100, 256, None);
        let (mut control_producer, mut control_mixer) = rig(44_100, 256, None);
        play(&mut producer, 1, 0);
        play(&mut control_producer, 1, 0);
        let mut prefix = [0.0; 3];
        let mut control_prefix = [0.0; 3];
        assert_eq!(
            mixer.render(&mut prefix).unwrap(),
            control_mixer.render(&mut control_prefix).unwrap()
        );
        assert_eq!(prefix, control_prefix);
        play(&mut producer, 2, 1_000_000_000);
        play(&mut control_producer, 2, 1_000_000_000);
        let target = format(48_000, 2);
        let matrix = ChannelMatrix::default_mix(1, 2).unwrap();
        let mut raw = [0.0; 2];
        let mut control_raw = [0.0; 2];
        let mut output = [0.0; 64];
        let mut control_output = [0.0; 64];
        let (attempt, fired) = refusing(failure_index, || {
            ConvertedMixer::new(mixer, target, matrix, quality, 32)
        });
        assert!(
            fired,
            "constructor allocation {failure_index} was not exercised"
        );
        let failure = match attempt {
            Err(failure) => failure,
            Ok(_) => panic!("injected allocation failure was accepted"),
        };
        assert_eq!(failure.error(), &AudioError::AllocationFailed);
        let (error, original) = failure.into_parts();
        assert_eq!(error, AudioError::AllocationFailed);
        let mut mixer = original.expect("failed construction must return the original mixer");
        assert_eq!(mixer.frame_cursor(), control_mixer.frame_cursor());
        assert_eq!(mixer.counters(), control_mixer.counters());
        assert_eq!(
            mixer.render(&mut raw).unwrap(),
            control_mixer.render(&mut control_raw).unwrap()
        );
        assert_eq!(raw, control_raw);
        assert_eq!(raw, [value(3), value(4)]);
        assert_eq!(
            mixer.counters().commands_consumed,
            2,
            "queued command survives refused ownership transfer"
        );
        let mut retry = owner(mixer, target, quality, 32);
        let mut control = owner(control_mixer, target, quality, 32);
        assert_eq!(
            render(&mut retry, &mut output),
            render(&mut control, &mut control_output)
        );
        assert_eq!(output, control_output);
        assert_eq!(retry.mixer().counters().commands_consumed, 2);
    }
}

#[test]
fn retarget_allocation_refusal_preserves_owner_pcm_commands_and_successful_retry() {
    // retarget prepares its replacement window and then its replacement sinc
    // table before publishing any target configuration or rational phase.
    for failure_index in [0, 1] {
        let build = || {
            let (mut producer, mixer) = rig(44_100, 256, None);
            play(&mut producer, 1, 0);
            (
                producer,
                owner(
                    mixer,
                    format(48_000, 1),
                    ResampleQuality::WindowedSinc { half_taps: 8 },
                    32,
                ),
            )
        };
        let (mut producer, mut converted) = build();
        let (mut control_producer, mut control) = build();
        let mut prefix = [0.0; 7];
        let mut control_prefix = [0.0; 7];
        assert_eq!(
            render(&mut converted, &mut prefix),
            render(&mut control, &mut control_prefix)
        );
        assert_eq!(prefix, control_prefix);
        play(&mut producer, 2, 1_000_000_000);
        play(&mut control_producer, 2, 1_000_000_000);
        let position = converted.source_position();
        let frontier = converted.pulled_source_frame_cursor();
        let target_cursor = converted.target_frame_cursor();
        let duration = converted.target_time();
        let counters = converted.mixer().counters();
        let source_format = converted.converter().source_format();
        let old_target = converted.converter().target_format();
        let target = format(32_000, 2);
        let matrix = ChannelMatrix::default_mix(1, 2).unwrap();
        let mut unchanged = [0.0; 16];
        let mut control_unchanged = [0.0; 16];
        let mut output = [0.0; 64];
        let mut control_output = [0.0; 64];
        let (attempt, fired) = refusing(failure_index, || converted.retarget(target, matrix, 32));
        assert!(
            fired,
            "retarget allocation {failure_index} was not exercised"
        );
        assert_eq!(attempt, Err(AudioError::AllocationFailed));
        assert_eq!(converted.source_position(), position);
        assert_eq!(converted.pulled_source_frame_cursor(), frontier);
        assert_eq!(converted.target_frame_cursor(), target_cursor);
        assert_eq!(converted.target_time(), duration);
        assert_eq!(converted.mixer().counters(), counters);
        assert_eq!(converted.converter().source_format(), source_format);
        assert_eq!(converted.converter().target_format(), old_target);
        assert_eq!(
            render(&mut converted, &mut unchanged),
            render(&mut control, &mut control_unchanged)
        );
        assert_eq!(unchanged, control_unchanged);
        assert_eq!(converted.mixer().counters().commands_consumed, 2);
        retarget(&mut converted, 32_000, 2, 32);
        retarget(&mut control, 32_000, 2, 32);
        assert_eq!(
            render(&mut converted, &mut output),
            render(&mut control, &mut control_output)
        );
        assert_eq!(output, control_output);
        assert_eq!(
            converted.mixer().counters().commands_consumed,
            2,
            "retry must not consume a command twice"
        );
    }
}

#[test]
fn allocation_failure_injection_is_one_shot_and_thread_local() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let begin = Arc::new(AtomicBool::new(false));
    let end = Arc::new(AtomicBool::new(false));
    let child_begin = Arc::clone(&begin);
    let child_end = Arc::clone(&end);
    let child = std::thread::spawn(move || {
        while !child_begin.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }
        let mut unrelated = Vec::<u8>::new();
        let result = unrelated.try_reserve_exact(1024);
        child_end.store(true, Ordering::Release);
        result
    });
    let mut first = Vec::<u8>::new();
    let mut second = Vec::<u8>::new();
    let ((refused, successful), fired) = refusing(0, || {
        begin.store(true, Ordering::Release);
        while !end.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }
        let refused = first.try_reserve_exact(1024);
        let successful = second.try_reserve_exact(1024);
        (refused, successful)
    });
    assert!(fired);
    assert!(refused.is_err());
    assert!(successful.is_ok());
    assert!(
        child.join().unwrap().is_ok(),
        "another thread must not inherit injected failure"
    );
}

#[test]
fn nonzero_source_origin_piecewise_active_and_held_duration_is_exact_and_allocation_free() {
    const DEN: u128 = 14_112_000;
    let (mut producer, mut mixer) = rig(44_100, 256, None);
    play(&mut producer, 1, 0);
    mixer.render(&mut [0.0; 5]).unwrap();
    let mut converted = owner(mixer, format(48_000, 1), ResampleQuality::Linear, 32);
    let mut duration_ticks = 5 * (DEN / 44_100);
    let mut position_ticks = 5 * DEN;
    let mut generated = 0_u64;
    for (rate, count, hold) in [
        (48_000, 3, false),
        (32_000, 5, true),
        (44_100, 7, false),
        (48_000, 11, true),
    ] {
        retarget(&mut converted, rate, 1, 32);
        let start = converted.target_time();
        let basis = converted.target_frame_basis();
        assert_eq!(basis.start_time(), start);
        assert_eq!(basis.sample_rate(), rate);
        let mut output = [99.0; 32];
        let report = if hold {
            held(&mut converted, &mut output[..count])
        } else {
            render(&mut converted, &mut output[..count])
        };
        duration_ticks += count as u128 * (DEN / u128::from(rate));
        generated += count as u64;
        if hold {
            assert_eq!(&output[..count], &vec![0.0; count]);
            assert_eq!(report.source, None);
        } else {
            position_ticks += count as u128 * 44_100 * (DEN / u128::from(rate));
        }
        let duration = converted.target_time();
        assert_eq!(
            u128::from(duration.seconds()) * DEN * u128::from(duration.denominator())
                + u128::from(duration.numerator()) * DEN,
            duration_ticks * u128::from(duration.denominator())
        );
        let position = converted.source_position();
        assert_eq!(
            u128::from(position.frame) * DEN
                + u128::from(position.numerator) * (DEN / u128::from(position.denominator)),
            position_ticks
        );
        assert_eq!(report.target_start_time, start);
        assert_eq!(report.target_end_time, duration);
        assert_eq!(report.target_rate, rate);
        assert_eq!(report.target_frame_cursor, generated);
        assert_eq!(basis.time_at_stream_frame(count as u64).unwrap(), duration);
        assert_eq!(
            basis.time_at_stream_frame(1).unwrap(),
            start.checked_add_frames(1, rate).unwrap()
        );
    }
}

#[test]
fn cold_duration_denominator_refusal_preserves_owner_queue_and_equivalent_retry() {
    let build = || {
        let (mut producer, mixer) = rig(44_100, 256, None);
        play(&mut producer, 1, 0);
        (
            producer,
            owner(mixer, format(1_000_003, 1), ResampleQuality::Linear, 8),
        )
    };
    let (_producer, mut failed) = build();
    let (_control_producer, mut control) = build();
    for rate in [1_000_003, 1_000_033, 1_000_037] {
        retarget(&mut failed, rate, 1, 8);
        retarget(&mut control, rate, 1, 8);
        assert_eq!(
            held(&mut failed, &mut [99.0; 1]),
            held(&mut control, &mut [99.0; 1])
        );
    }
    let before = (
        failed.target_time(),
        failed.target_frame_cursor(),
        failed.source_position(),
        failed.pulled_source_frame_cursor(),
        failed.mixer().counters(),
        failed.converter().target_format(),
    );
    assert_eq!(
        failed.retarget(
            format(1_000_039, 1),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            8
        ),
        Err(AudioError::Overflow)
    );
    assert_eq!(
        (
            failed.target_time(),
            failed.target_frame_cursor(),
            failed.source_position(),
            failed.pulled_source_frame_cursor(),
            failed.mixer().counters(),
            failed.converter().target_format()
        ),
        before
    );
    let mut actual = [99.0; 4];
    let mut expected = [99.0; 4];
    assert_eq!(
        render(&mut failed, &mut actual),
        render(&mut control, &mut expected)
    );
    assert_eq!(actual, expected);
    assert_eq!(failed.mixer().counters().commands_consumed, 1);
    // A representable retry retains the accepted voice and exact buffered PCM.
    retarget(&mut failed, 1_000_033, 1, 8);
    retarget(&mut control, 1_000_033, 1, 8);
    assert_eq!(
        render(&mut failed, &mut actual),
        render(&mut control, &mut expected)
    );
    assert_eq!(actual, expected);
}
