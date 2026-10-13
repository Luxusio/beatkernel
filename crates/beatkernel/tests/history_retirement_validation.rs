//! Readonly prevalidation uses actual owned histories; it grants no native permission.
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
// SAFETY: Pointers, sizes and layouts pass unchanged to System; scalar TLS does not allocate.
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

fn cold_mixer(source: u32) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(source, 1).unwrap();
    let limits = PcmLimits::new(1024, 1024, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            format,
            (0..128).map(|i| (i % 13) as f32 / 16.0).collect(),
            limits,
        )
        .unwrap(),
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
    let mut mixer = Mixer::new(
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
    mixer.render(&mut [0.0; 3]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 2]).unwrap();
    producer.request_pause(false);
    mixer.enable_source_playback_history(3).unwrap();
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (5, 3)
    );
    (producer, mixer)
}
fn source_three() -> (CommandProducer, Mixer) {
    let (mut producer, mut mixer) = cold_mixer(3);
    for paused in [false, true, false] {
        producer.request_pause(paused);
        mixer.render(&mut [0.0; 2]).unwrap();
    }
    assert_eq!(mixer.source_playback_history().unwrap().len(), 3);
    (producer, mixer)
}
fn cold_converter(source: u32, target: u32) -> (CommandProducer, ConvertedMixer) {
    let (producer, mixer) = cold_mixer(source);
    let mut owner = ConvertedMixer::new(
        mixer,
        AudioFormat::new(target, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|failure| panic!("fixture: {:?}", failure.error()));
    owner.enable_phase_history(3).unwrap();
    (producer, owner)
}
fn converted_render(
    owner: &mut ConvertedMixer,
    held: bool,
    frames: usize,
) -> (ConvertedRenderReport, [f32; 16]) {
    let mut pcm = [99.0; 16];
    let report = if held {
        owner.render_held(&mut pcm[..frames])
    } else {
        owner.render(&mut pcm[..frames])
    }
    .unwrap();
    (report, pcm)
}
fn phase_three(source: u32, target: u32) -> (CommandProducer, ConvertedMixer) {
    let (producer, mut owner) = cold_converter(source, target);
    converted_render(&mut owner, false, 2);
    converted_render(&mut owner, true, 2);
    converted_render(&mut owner, false, 2);
    assert_eq!(owner.phase_history().unwrap().len(), 3);
    (producer, owner)
}

#[derive(Debug, PartialEq, Eq)]
struct SourceSnapshot {
    initial: (u32, u64, u64),
    retained: (u64, u64, usize, u64),
    spans: Vec<SourcePlaybackSpan>,
    cursors: (u64, u64),
    counters: AudioCounters,
    control: (
        bool,
        bool,
        Option<Option<u64>>,
        Option<u64>,
        Option<u64>,
        bool,
    ),
}
fn source_snapshot(mixer: &Mixer) -> SourceSnapshot {
    let h = mixer.source_playback_history().unwrap();
    SourceSnapshot {
        initial: (
            h.source_rate(),
            h.initial_physical_frame(),
            h.initial_playback_frame(),
        ),
        retained: (
            h.retained_start_frame(),
            h.pulled_end_frame(),
            h.capacity(),
            h.retirement_revision(),
        ),
        spans: (0..h.len()).map(|i| h.span(i).unwrap()).collect(),
        cursors: (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        counters: mixer.counters(),
        control: (
            mixer.pause_requested(),
            mixer.is_paused(),
            mixer.start_gate_frame(),
            mixer.applied_start_frame(),
            mixer.practice_generation(),
            mixer.practice_finished(),
        ),
    }
}
#[derive(Debug, PartialEq, Eq)]
struct OwnerSnapshot {
    source: SourceSnapshot,
    initial: (u32, TargetTime, SourcePosition),
    retained: (TargetTime, TargetTime, usize, u64),
    spans: Vec<[u64; 8]>,
    cursors: (u64, TargetTime, SourcePosition, u64),
}
fn owner_snapshot(owner: &ConvertedMixer) -> OwnerSnapshot {
    let h = owner.phase_history().unwrap();
    OwnerSnapshot {
        source: source_snapshot(owner.mixer()),
        initial: (h.source_rate(), h.start_time(), h.initial_source_position()),
        retained: (
            h.retained_start_time(),
            h.end_time(),
            h.capacity(),
            h.retirement_revision(),
        ),
        spans: (0..h.len())
            .map(|i| h.span(i).unwrap().to_words())
            .collect(),
        cursors: (
            owner.target_frame_cursor(),
            owner.target_time(),
            owner.source_position(),
            owner.pulled_source_frame_cursor(),
        ),
    }
}
fn nearby(time: TargetTime, delta: i64) -> TargetTime {
    let scale = 10_000_000_000_u64;
    TargetTime::new(
        time.seconds(),
        (i128::from(time.numerator()) * i128::from(scale) + i128::from(delta)) as u64,
        time.denominator() * scale,
    )
    .unwrap()
}
fn floor_ns(time: TargetTime) -> u128 {
    u128::from(time.seconds()) * 1_000_000_000
        + u128::from(time.numerator()) * 1_000_000_000 / u128::from(time.denominator())
}

#[test]
fn cold_empty_source_and_phase_validation_preserves_original_creation() {
    let (_, mut mixer) = cold_mixer(3);
    let before = source_snapshot(&mixer);
    for (pin, expected) in [
        (5, Ok(())),
        (4, Err(AudioError::InvalidFormat)),
        (6, Err(AudioError::InvalidFormat)),
        (u64::MAX, Err(AudioError::InvalidFormat)),
    ] {
        let (result, counts) = tracked(|| {
            mixer
                .source_playback_history()
                .unwrap()
                .validate_retirement_before(pin)
        });
        assert_eq!(result, expected);
        assert_eq!(counts, [0; 3]);
        assert_eq!(source_snapshot(&mixer), before);
    }
    assert_eq!(mixer.retire_source_playback_before(5), Ok(0));
    assert_eq!(source_snapshot(&mixer), before);
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut owner) = cold_converter(source, target);
        let before = owner_snapshot(&owner);
        let initial = owner.target_time();
        for (pin, expected) in [
            (initial, Ok(())),
            (
                TargetTime::from_frames(0, 1).unwrap(),
                Err(AudioError::InvalidFormat),
            ),
            (
                initial.checked_add_frames(1, target).unwrap(),
                Err(AudioError::InvalidFormat),
            ),
        ] {
            let (result, counts) = tracked(|| {
                owner
                    .phase_history()
                    .unwrap()
                    .validate_retirement_before(pin)
            });
            assert_eq!(result, expected);
            assert_eq!(counts, [0; 3]);
            assert_eq!(owner_snapshot(&owner), before);
        }
        assert_eq!(owner.retire_phase_history_before(initial), Ok(0));
        assert_eq!(owner_snapshot(&owner), before);
    }
}

#[test]
fn source_validation_is_readonly_and_following_retirement_keeps_exact_predecessor() {
    for (pin, removed) in [(5, 0), (6, 0), (7, 1), (8, 1), (9, 2), (11, 2)] {
        let (_, mut mixer) = source_three();
        let before = source_snapshot(&mixer);
        for _ in 0..3 {
            assert_eq!(
                mixer
                    .source_playback_history()
                    .unwrap()
                    .validate_retirement_before(pin),
                Ok(())
            );
            assert_eq!(source_snapshot(&mixer), before);
        }
        assert_eq!(mixer.retire_source_playback_before(pin), Ok(removed));
        let after = source_snapshot(&mixer);
        assert_eq!(after.initial, before.initial);
        assert_eq!(after.cursors, before.cursors);
        assert_eq!(after.counters, before.counters);
        assert_eq!(after.control, before.control);
        assert_eq!(after.retained.1, before.retained.1);
        assert_eq!(after.retained.2, 3);
        assert_eq!(after.retained.3, u64::from(removed > 0));
        assert_eq!(after.spans, before.spans[removed..]);
        assert_eq!(
            mixer
                .source_playback_history()
                .unwrap()
                .validate_retirement_before(pin),
            Ok(())
        );
        assert_eq!(mixer.retire_source_playback_before(pin), Ok(0));
        assert_eq!(source_snapshot(&mixer), after);
    }
}

#[test]
fn source_invalid_validation_uses_retained_lower_not_immutable_cold_header() {
    let (_, mut mixer) = source_three();
    assert_eq!(mixer.retire_source_playback_before(7), Ok(1));
    let before = source_snapshot(&mixer);
    assert_eq!(before.initial, (3, 5, 3));
    for pin in [0, 5, 6, 12, u64::MAX] {
        assert_eq!(
            mixer
                .source_playback_history()
                .unwrap()
                .validate_retirement_before(pin),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(source_snapshot(&mixer), before);
        assert_eq!(
            mixer.retire_source_playback_before(pin),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(source_snapshot(&mixer), before);
    }
}

#[test]
fn phase_exact_fraction_validation_then_retirement_preserves_covering_descriptors() {
    for (source, target) in [(3, 7), (7, 3)] {
        for offset in [-1, 0, 1] {
            let (_, mut owner) = phase_three(source, target);
            let boundary = owner.phase_history().unwrap().span(1).unwrap().to_words();
            // The held interval starts two target frames after the immutable initial point.
            let cut = owner
                .phase_history()
                .unwrap()
                .start_time()
                .checked_add_frames(2, target)
                .unwrap();
            let pin = nearby(cut, offset);
            assert_eq!(floor_ns(pin), floor_ns(cut));
            let before = owner_snapshot(&owner);
            assert_eq!(
                owner
                    .phase_history()
                    .unwrap()
                    .validate_retirement_before(pin),
                Ok(())
            );
            assert_eq!(owner_snapshot(&owner), before);
            let removed = usize::from(offset >= 0);
            assert_eq!(owner.retire_phase_history_before(pin), Ok(removed));
            let after = owner_snapshot(&owner);
            assert_eq!(after.source, before.source);
            assert_eq!(after.initial, before.initial);
            assert_eq!(after.cursors, before.cursors);
            assert_eq!(after.spans, before.spans[removed..]);
            assert_eq!(after.retained.1, before.retained.1);
            assert_eq!(after.retained.2, 3);
            assert_eq!(after.retained.3, u64::from(removed > 0));
            if removed > 0 {
                assert_eq!(after.spans[0], boundary);
            }
        }
    }
}

#[test]
fn phase_lower_and_generated_end_validation_are_exact_and_readonly() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut owner) = phase_three(source, target);
        let cut = owner
            .phase_history()
            .unwrap()
            .start_time()
            .checked_add_frames(2, target)
            .unwrap();
        assert_eq!(owner.retire_phase_history_before(cut), Ok(1));
        let end = owner.target_time();
        let before = owner_snapshot(&owner);
        for (pin, expected) in [
            (nearby(cut, -1), Err(AudioError::InvalidFormat)),
            (cut, Ok(())),
            (end, Ok(())),
            (nearby(end, 1), Err(AudioError::InvalidFormat)),
        ] {
            let (result, allocations) = tracked(|| {
                owner
                    .phase_history()
                    .unwrap()
                    .validate_retirement_before(pin)
            });
            assert_eq!(result, expected);
            assert_eq!(allocations, [0; 3]);
            assert_eq!(owner_snapshot(&owner), before);
        }
        assert_eq!(owner.retire_phase_history_before(end), Ok(1));
        let after = owner_snapshot(&owner);
        assert_eq!(after.spans, before.spans[1..]);
        assert_eq!(after.retained.3, 2);
        assert_eq!(after.cursors, before.cursors);
        assert_eq!(after.source, before.source);
        assert_eq!(
            owner
                .phase_history()
                .unwrap()
                .validate_retirement_before(end),
            Ok(())
        );
        assert_eq!(owner.retire_phase_history_before(end), Ok(0));
        assert_eq!(owner_snapshot(&owner), after);
    }
}

#[test]
fn validation_after_actual_source_pause_and_target_hold_does_not_change_pcm_or_reports() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (mut producer, mut owner) = cold_converter(source, target);
        let (mut reference_producer, mut reference) = cold_converter(source, target);
        for (paused, held) in [
            (false, false),
            (true, false),
            (true, true),
            (false, false),
            (false, true),
            (false, false),
        ] {
            producer.request_pause(paused);
            reference_producer.request_pause(paused);
            let before = owner_snapshot(&owner);
            let source_pin = owner.source_playback_history().unwrap().pulled_end_frame();
            let target_pin = owner.target_time();
            for _ in 0..8 {
                assert_eq!(
                    owner
                        .source_playback_history()
                        .unwrap()
                        .validate_retirement_before(source_pin),
                    Ok(())
                );
                assert_eq!(
                    owner
                        .phase_history()
                        .unwrap()
                        .validate_retirement_before(target_pin),
                    Ok(())
                );
            }
            assert_eq!(owner_snapshot(&owner), before);
            // Release only this fixture's old mathematical queries, after checking readonly validation.
            owner.retire_source_playback_before(source_pin).unwrap();
            owner.retire_phase_history_before(target_pin).unwrap();
            reference.retire_source_playback_before(source_pin).unwrap();
            reference.retire_phase_history_before(target_pin).unwrap();
            assert_eq!(
                converted_render(&mut owner, held, target as usize),
                converted_render(&mut reference, held, target as usize)
            );
            assert_eq!(owner_snapshot(&owner), owner_snapshot(&reference));
        }
    }
}

#[test]
fn fixed_capacity_validation_and_real_retirement_cycles_allocate_nothing() {
    let (mut producer, mut mixer) = cold_mixer(3);
    let (mut reference_producer, mut reference) = cold_mixer(3);
    for cycle in 0..16 {
        for paused in [false, true] {
            producer.request_pause(paused);
            reference_producer.request_pause(paused);
            let mut pcm = [99.0; 2];
            let mut reference_pcm = [99.0; 2];
            assert_eq!(
                mixer.render(&mut pcm).unwrap(),
                reference.render(&mut reference_pcm).unwrap()
            );
            assert_eq!(pcm, reference_pcm);
            let h = mixer.source_playback_history().unwrap();
            let pin = h.span(h.len() - 1).unwrap().physical_start();
            let expected = h.len() - 1;
            let before = source_snapshot(&mixer);
            for (query, result) in [
                (pin, Ok(())),
                (mixer.frame_cursor() + 1, Err(AudioError::InvalidFormat)),
            ] {
                let (actual, allocations) = tracked(|| {
                    mixer
                        .source_playback_history()
                        .unwrap()
                        .validate_retirement_before(query)
                });
                assert_eq!(actual, result);
                assert_eq!(allocations, [0; 3]);
                assert_eq!(source_snapshot(&mixer), before);
            }
            let (result, allocations) = tracked(|| mixer.retire_source_playback_before(pin));
            assert_eq!(result, Ok(expected));
            assert_eq!(allocations, [0; 3]);
            reference.retire_source_playback_before(pin).unwrap();
            assert_eq!(source_snapshot(&mixer), source_snapshot(&reference));
            assert_eq!(mixer.source_playback_history().unwrap().capacity(), 3);
        }
        assert_eq!(mixer.frame_cursor(), 5 + (cycle + 1) * 4);
        assert_eq!(mixer.playback_frame_cursor(), 3 + (cycle + 1) * 2);
    }
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut owner) = cold_converter(source, target);
        converted_render(&mut owner, false, 1);
        for _ in 0..16 {
            converted_render(&mut owner, true, 1);
            let resumed = converted_render(&mut owner, false, 1).0;
            let before = owner_snapshot(&owner);
            let pin = resumed.target_start_time;
            let (result, allocations) = tracked(|| {
                owner
                    .phase_history()
                    .unwrap()
                    .validate_retirement_before(pin)
            });
            assert_eq!(result, Ok(()));
            assert_eq!(allocations, [0; 3]);
            assert_eq!(owner_snapshot(&owner), before);
            let (result, allocations) = tracked(|| owner.retire_phase_history_before(pin));
            assert_eq!(result, Ok(2));
            assert_eq!(allocations, [0; 3]);
            let after = owner_snapshot(&owner);
            let (result, allocations) = tracked(|| {
                owner
                    .phase_history()
                    .unwrap()
                    .validate_retirement_before(pin)
            });
            assert_eq!(result, Ok(()));
            assert_eq!(allocations, [0; 3]);
            assert_eq!(owner_snapshot(&owner), after);
            assert_eq!(owner.phase_history().unwrap().capacity(), 3);
        }
    }
}
