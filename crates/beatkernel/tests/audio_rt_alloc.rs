use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

use beatkernel::transport::Rate;
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
        // SAFETY: GlobalAlloc caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: GlobalAlloc caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        // SAFETY: Caller supplies a live System allocation, matching layout and
        // nonzero new size under the GlobalAlloc contract.
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count(2);
        // SAFETY: Caller supplies the live original pointer and layout.
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
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn rig(limits: AudioLimits) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm_limits = PcmLimits::new(64, 64, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.0], pcm_limits).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(limits.queue_capacity()).unwrap();
    let config = MixerConfig::new(format, ClockDomainId(9), Timestamp::ZERO, limits);
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn play(voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(1),
        at: ts(at),
        gain: 1.0,
    }
}
fn render(mixer: &mut Mixer, output: &mut [f32]) -> RenderReport {
    let (result, counts) = track(|| mixer.render(output));
    // Assertions and formatting are outside the measured callback boundary.
    assert_eq!(counts, [0, 0, 0], "alloc/realloc/dealloc during render");
    result.unwrap()
}

#[test]
fn allocator_calibration_observes_each_operation() {
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
fn silence_active_natural_completion_stop_seek_invalid_and_disconnect_are_rt_safe() {
    let (mut producer, mut mixer) = rig(AudioLimits::new(16, 1, 16, 8, 16).unwrap());
    let mut one = [99.0];
    assert_eq!(render(&mut mixer, &mut one).active_voices, 0);
    assert_eq!(one, [0.0]);
    producer.try_push(play(1, 1_000_000)).unwrap();
    assert_eq!(render(&mut mixer, &mut one).active_voices, 1);
    assert_eq!(one, [0.25]);
    let mut completion = [99.0; 4];
    assert_eq!(render(&mut mixer, &mut completion).active_voices, 0);
    assert_eq!(completion, [0.5, 0.75, 1.0, 0.0]);
    producer.try_push(play(1, 6_000_000)).unwrap();
    producer
        .try_push(AudioCommand::Stop {
            voice: VoiceId(1),
            at: ts(7_000_000),
        })
        .unwrap();
    let mut two = [99.0; 2];
    assert_eq!(render(&mut mixer, &mut two).active_voices, 0);
    assert_eq!(two, [0.25, 0.0]);
    producer.try_push(play(1, 8_000_000)).unwrap();
    producer
        .try_push(AudioCommand::Seek {
            song_time: ts(-999),
            at: ts(9_000_000),
        })
        .unwrap();
    let report = render(&mut mixer, &mut two);
    assert_eq!(two, [0.25, 0.0]);
    assert_eq!(report.song_position, ts(-999));
    assert_eq!(report.active_voices, 0);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: ts(10_000_000),
            gain: f32::NAN,
        })
        .unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(99),
            at: ts(10_000_000),
            gain: 1.0,
        })
        .unwrap();
    producer.try_push(play(1, 10_000_000)).unwrap();
    producer.try_push(play(2, 10_000_000)).unwrap();
    let report = render(&mut mixer, &mut one);
    assert_eq!(one, [0.25]);
    assert_eq!(report.counters.invalid_gains, 1);
    assert_eq!(report.counters.unknown_samples, 1);
    assert_eq!(report.counters.voice_full, 1);
    let mut invalid = [99.0; 9];
    let previous = mixer.counters();
    let (result, counts) = track(|| mixer.render(&mut invalid));
    assert_eq!(counts, [0, 0, 0]);
    assert_eq!(result, Err(AudioError::RenderCapacity));
    assert_eq!(invalid, [99.0; 9]);
    assert_eq!(mixer.counters(), previous);
    drop(producer); // Endpoint and asset ownership changes happen off callback.
    let report = render(&mut mixer, &mut completion);
    assert!(report.producer_disconnected);
    assert_eq!(report.active_voices, 0);
    assert_eq!(completion, [0.5, 0.75, 1.0, 0.0]);
    assert_eq!(render(&mut mixer, &mut one).active_voices, 0);
    assert_eq!(one, [0.0]);
    drop(mixer);
}

#[test]
fn pending_full_rejection_and_future_execution_do_not_allocate_or_free() {
    let (mut producer, mut mixer) = rig(AudioLimits::new(4, 1, 1, 8, 4).unwrap());
    producer.try_push(play(1, 4_000_000)).unwrap();
    producer.try_push(play(2, 0)).unwrap();
    let mut silence = [99.0; 4];
    let report = render(&mut mixer, &mut silence);
    assert_eq!(silence, [0.0; 4]);
    assert_eq!(report.counters.pending_full, 1);
    assert_eq!(report.pending_commands, 1);
    drop(producer);
    let mut asset = [99.0; 4];
    let report = render(&mut mixer, &mut asset);
    assert_eq!(asset, [0.25, 0.5, 0.75, 1.0]);
    assert_eq!(report.active_voices, 0);
    assert!(report.producer_disconnected);
    drop(mixer);
}

#[test]
fn rate_changes_and_rational_overflow_rejection_do_not_allocate_or_free() {
    let (mut producer, mut mixer) = rig(AudioLimits::new(8, 2, 8, 8, 8).unwrap());
    let retained = Rate::new(1, u64::MAX).unwrap();
    producer
        .try_push(AudioCommand::SetRate {
            rate: retained,
            at: ts(0),
        })
        .unwrap();
    producer.try_push(play(1, 0)).unwrap();
    let mut one = [99.0];
    render(&mut mixer, &mut one);
    assert_eq!(one, [0.25]);
    producer
        .try_push(AudioCommand::SetRate {
            rate: Rate::new(1, u64::MAX - 2).unwrap(),
            at: ts(1_000_000),
        })
        .unwrap();
    let report = render(&mut mixer, &mut one);
    assert_eq!(report.counters.invalid_rates, 1);
    assert_eq!(mixer.rate(), retained);
    assert_eq!(one, [0.25]);
    producer
        .try_push(AudioCommand::SetRate {
            rate: Rate::ZERO,
            at: ts(2_000_000),
        })
        .unwrap();
    let mut paused = [99.0; 2];
    render(&mut mixer, &mut paused);
    assert_eq!(paused, [0.0; 2]);
    producer
        .try_push(AudioCommand::SetRate {
            rate: Rate::REVERSE,
            at: ts(4_000_000),
        })
        .unwrap();
    let mut reverse = [99.0; 2];
    let report = render(&mut mixer, &mut reverse);
    assert_eq!(reverse, [0.25, 0.0]);
    assert_eq!(report.active_voices, 0);
    drop(producer);
    drop(mixer);
}

#[test]
fn bounded_consume_budget_and_empty_render_do_not_allocate_or_free() {
    let (mut producer, mut mixer) = rig(AudioLimits::new(4, 2, 4, 8, 1).unwrap());
    for voice in 1..=3 {
        producer.try_push(play(voice, 0)).unwrap();
    }
    let report = render(&mut mixer, &mut []);
    assert_eq!(report.counters.commands_consumed, 0);
    let mut one = [99.0];
    for consumed in 1..=3 {
        let report = render(&mut mixer, &mut one);
        assert_eq!(report.counters.commands_consumed, consumed);
    }
    assert_eq!(mixer.counters().voice_full, 1);
    drop(producer);
    drop(mixer);
}

#[test]
fn requested_pause_silence_and_resume_preserve_rt_storage_and_voice_phase() {
    let (mut producer, mut mixer) = rig(AudioLimits::new(2, 1, 2, 8, 2).unwrap());
    producer.try_push(play(1, 0)).unwrap();
    let mut one = [99.0];
    render(&mut mixer, &mut one);
    assert_eq!(one, [0.25]);
    // Keep both the ring and a live asset owned across the paused callback.
    producer.try_push(play(1, 10_000_000)).unwrap();
    producer.try_push(play(1, 11_000_000)).unwrap();
    let ((), counts) = track(|| producer.request_pause(true));
    assert_eq!(counts, [0, 0, 0]);
    let mut silence = [99.0; 3];
    let paused = render(&mut mixer, &mut silence);
    assert_eq!(silence, [0.0; 3]);
    assert!(paused.paused);
    assert_eq!(paused.playback_start_frame, 1);
    assert_eq!(paused.playback_frames, 0);
    assert_eq!(paused.counters.commands_consumed, 1);
    let ((), counts) = track(|| producer.request_pause(false));
    assert_eq!(counts, [0, 0, 0]);
    let resumed = render(&mut mixer, &mut one);
    assert_eq!(one, [0.5]);
    assert_eq!(resumed.start_frame, 4);
    assert_eq!(resumed.playback_start_frame, 1);
    assert_eq!(resumed.playback_frames, 1);
    assert!(!resumed.paused);
    assert_eq!(resumed.pending_commands, 2);
}
