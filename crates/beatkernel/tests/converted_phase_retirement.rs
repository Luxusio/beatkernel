//! Owner-only mathematical retirement; no native admission or presentation claim.
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
// SAFETY: Every pointer and layout is forwarded unchanged to System.
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
fn tracked<T>(f: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|v| v.set([0; 3]));
    TRACK.with(|v| v.set(true));
    let result = f();
    TRACK.with(|v| v.set(false));
    (result, COUNTS.with(Cell::get))
}
fn rig(source: u32, target: u32, opt_in: bool) -> (CommandProducer, ConvertedMixer) {
    let format = AudioFormat::new(source, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(1024, 1024, 1).unwrap()).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
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
    mixer.render(&mut [0.0]).unwrap();
    if opt_in {
        mixer.enable_source_playback_history(3).unwrap();
    }
    let mut owner = ConvertedMixer::new(
        mixer,
        AudioFormat::new(target, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|failure| panic!("fixture: {:?}", failure.error()));
    if opt_in {
        owner.enable_phase_history(3).unwrap();
    }
    (producer, owner)
}
fn render(owner: &mut ConvertedMixer, held: bool, frames: usize) -> ConvertedRenderReport {
    let mut pcm = [99.0; 16];
    if held {
        owner.render_held(&mut pcm[..frames]).unwrap()
    } else {
        owner.render(&mut pcm[..frames]).unwrap()
    }
}
fn words(owner: &ConvertedMixer) -> Vec<[u64; 8]> {
    let h = owner.phase_history().unwrap();
    (0..h.len())
        .map(|i| h.span(i).unwrap().to_words())
        .collect()
}
fn source_descriptor(span: SourcePlaybackSpan) -> (u64, Option<u64>, u64, SourcePlaybackState) {
    (
        span.physical_start(),
        span.physical_end(),
        span.playback_start(),
        span.state(),
    )
}
fn floor_ns(time: TargetTime) -> i128 {
    i128::from(time.seconds()) * 1_000_000_000
        + i128::from(time.numerator()) * 1_000_000_000 / i128::from(time.denominator())
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
fn oracle(time: TargetTime, source: u32, target: u32, state: ConvertedOutputState) -> i128 {
    // Initial source position is 1. Two active frames precede a two-frame hold.
    // Physical duration equals query minus held duration after resume.
    let (n, d) = match state {
        ConvertedOutputState::Active => {
            let n = i128::from(time.seconds()) * i128::from(time.denominator())
                + i128::from(time.numerator());
            (
                n * i128::from(target) - 2 * i128::from(time.denominator()),
                i128::from(time.denominator()) * i128::from(target),
            )
        }
        ConvertedOutputState::Held => (
            i128::from(target) + 2 * i128::from(source),
            i128::from(source) * i128::from(target),
        ),
    };
    n * 1_000_000_000 / d
}

#[test]
fn cold_noop_invalid_pins_and_disabled_delegates_are_readonly() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut owner) = rig(source, target, true);
        let initial = owner.target_time();
        let position = owner.source_position();
        assert_eq!(owner.retire_phase_history_before(initial), Ok(0));
        assert_eq!(owner.retire_source_playback_before(1), Ok(0));
        assert_eq!(
            owner.retire_phase_history_before(TargetTime::from_frames(0, 1).unwrap()),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(
            owner.retire_phase_history_before(initial.checked_add_frames(1, target).unwrap()),
            Err(AudioError::InvalidFormat)
        );
        let h = owner.phase_history().unwrap();
        assert!(h.is_empty());
        assert_eq!(h.start_time(), initial);
        assert_eq!(h.retained_start_time(), initial);
        assert_eq!(h.retirement_revision(), 0);
        assert_eq!(h.end_time(), initial);
        assert_eq!(h.initial_source_position(), position);
        assert_eq!(h.capacity(), 3);
        assert_eq!(
            h.lookup(initial).unwrap().state(),
            ConvertedOutputState::Held
        );
        assert_eq!(
            owner
                .source_playback_history()
                .unwrap()
                .retirement_revision(),
            0
        );
        let (_, mut disabled) = rig(source, target, false);
        assert_eq!(
            disabled.retire_phase_history_before(disabled.target_time()),
            Err(AudioError::InvalidCapacity)
        );
        assert_eq!(
            disabled.retire_source_playback_before(1),
            Err(AudioError::InvalidCapacity)
        );
        assert_eq!(disabled.source_position(), position);
        assert_eq!(disabled.target_time(), initial);
    }
}

#[test]
fn exact_fraction_pin_preserves_covering_span_despite_shared_nanosecond_floor() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut owner) = rig(source, target, true);
        let initial = owner.target_time();
        render(&mut owner, false, 2);
        let hold = render(&mut owner, true, 2);
        render(&mut owner, false, 2);
        let before = words(&owner);
        let boundary = hold.target_start_time;
        let below = nearby(boundary, -1);
        assert_eq!(floor_ns(below), floor_ns(boundary));
        assert_eq!(owner.retire_phase_history_before(below), Ok(0));
        assert_eq!(words(&owner), before);
        assert_eq!(owner.retire_phase_history_before(boundary), Ok(1));
        assert_eq!(words(&owner), before[1..]);
        assert_eq!(owner.phase_history().unwrap().start_time(), initial);
        assert_eq!(
            owner.phase_history().unwrap().retained_start_time(),
            boundary
        );
        assert_eq!(owner.phase_history().unwrap().retirement_revision(), 1);
        assert_eq!(
            owner.phase_history().unwrap().lookup(below).unwrap_err(),
            ConvertedPhaseError::BeforeRetainedRange
        );
        assert_eq!(
            owner
                .phase_history()
                .unwrap()
                .lookup(boundary)
                .unwrap()
                .state(),
            ConvertedOutputState::Held
        );
    }
}

#[test]
fn retained_mapping_matches_independent_rational_oracle_before_and_after_retirement() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut owner) = rig(source, target, true);
        let initial = owner.target_time();
        let position = owner.source_position();
        render(&mut owner, false, 2);
        let hold = render(&mut owner, true, 2);
        let resumed = render(&mut owner, false, 2);
        let queries = [
            hold.target_start_time,
            hold.target_start_time
                .checked_add_frames(1, target)
                .unwrap(),
            hold.target_end_time,
            hold.target_end_time.checked_add_frames(1, target).unwrap(),
            resumed.target_end_time,
        ];
        let expected: Vec<_> = queries
            .iter()
            .map(|&q| {
                let p = owner.phase_history().unwrap().lookup(q).unwrap();
                let elapsed = p.elapsed_nanos_since(0).unwrap();
                assert_eq!(elapsed, oracle(q, source, target, p.state()));
                (p.state(), elapsed)
            })
            .collect();
        let spans = words(&owner);
        let end = owner.target_time();
        let end_source = owner.source_position();
        assert_eq!(owner.retire_phase_history_before(queries[0]), Ok(1));
        for (&query, &(state, elapsed)) in queries.iter().zip(&expected) {
            let p = owner.phase_history().unwrap().lookup(query).unwrap();
            assert_eq!(p.state(), state);
            assert_eq!(p.elapsed_nanos_since(0), Ok(elapsed));
        }
        assert_eq!(words(&owner), spans[1..]);
        let h = owner.phase_history().unwrap();
        assert_eq!(h.start_time(), initial);
        assert_eq!(h.initial_source_position(), position);
        assert_eq!(h.source_rate(), source);
        assert_eq!(h.end_time(), end);
        assert_eq!(h.capacity(), 3);
        assert_eq!(owner.source_position(), end_source);
    }
}

#[test]
fn invalid_retirement_preserves_generated_state_and_all_remaining_spans() {
    let (_, mut owner) = rig(3, 7, true);
    render(&mut owner, false, 2);
    let hold = render(&mut owner, true, 2);
    render(&mut owner, false, 2);
    assert_eq!(
        owner.retire_phase_history_before(hold.target_start_time),
        Ok(1)
    );
    let spans = words(&owner);
    let end = owner.target_time();
    let pos = owner.source_position();
    for pin in [
        nearby(hold.target_start_time, -1),
        end.checked_add_frames(1, 7).unwrap(),
    ] {
        let (result, allocations) = tracked(|| owner.retire_phase_history_before(pin));
        assert_eq!(result, Err(AudioError::InvalidFormat));
        assert_eq!(allocations, [0; 3]);
        assert_eq!(words(&owner), spans);
        assert_eq!(owner.target_time(), end);
        assert_eq!(owner.source_position(), pos);
        assert_eq!(owner.phase_history().unwrap().retirement_revision(), 1);
    }
    assert_eq!(
        owner.retire_phase_history_before(hold.target_start_time),
        Ok(0)
    );
    assert_eq!(owner.phase_history().unwrap().retirement_revision(), 1);
}

#[test]
fn exact_generated_end_retains_last_span_and_next_render_continuity() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut owner) = rig(source, target, true);
        render(&mut owner, false, 2);
        render(&mut owner, true, 2);
        let last = render(&mut owner, false, 2);
        let last_span = words(&owner)[2];
        assert_eq!(
            owner.retire_phase_history_before(last.target_end_time),
            Ok(2)
        );
        assert_eq!(words(&owner), [last_span]);
        assert_eq!(
            owner
                .phase_history()
                .unwrap()
                .lookup(last.target_end_time)
                .unwrap()
                .state(),
            ConvertedOutputState::Active
        );
        let next = render(&mut owner, false, 1);
        assert_eq!(next.target_start_time, last.target_end_time);
        assert_eq!(next.source_start_position, last.source_position);
        assert_eq!(owner.phase_history().unwrap().len(), 1);
        assert_eq!(owner.phase_history().unwrap().retirement_revision(), 1);
    }
}

#[test]
fn bounded_repeated_active_held_resume_cycles_retire_without_heap_or_capacity_growth() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (_, mut owner) = rig(source, target, true);
        let initial = owner.target_time();
        let initial_source = owner.source_position();
        let mut previous = render(&mut owner, false, 1);
        for cycle in 0..16 {
            let held = render(&mut owner, true, 1);
            assert_eq!(held.source_position, previous.source_position);
            assert_eq!(held.source, None);
            let resumed = render(&mut owner, false, 1);
            assert_eq!(resumed.target_start_time, held.target_end_time);
            assert_eq!(resumed.source_start_position, held.source_position);
            assert_eq!(owner.phase_history().unwrap().len(), 3);
            let (result, counts) =
                tracked(|| owner.retire_phase_history_before(resumed.target_start_time));
            assert_eq!(result, Ok(2));
            assert_eq!(counts, [0; 3]);
            let (result, counts) =
                tracked(|| owner.retire_phase_history_before(resumed.target_start_time));
            assert_eq!(result, Ok(0));
            assert_eq!(counts, [0; 3]);
            let h = owner.phase_history().unwrap();
            assert_eq!(h.capacity(), 3);
            assert_eq!(h.len(), 1);
            assert_eq!(h.retirement_revision(), cycle + 1);
            assert_eq!(h.start_time(), initial);
            assert_eq!(h.initial_source_position(), initial_source);
            previous = resumed;
        }
    }
}

#[test]
fn source_retirement_delegate_operates_on_the_same_real_paused_mixer_history() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (mut producer, mut owner) = rig(source, target, true);
        render(&mut owner, false, target as usize);
        producer.request_pause(true);
        render(&mut owner, false, target as usize);
        producer.request_pause(false);
        render(&mut owner, false, target as usize);
        let h = owner.source_playback_history().unwrap();
        assert_eq!(h.len(), 3);
        assert_eq!(h.span(0).unwrap().state(), SourcePlaybackState::Running);
        assert_eq!(h.span(1).unwrap().state(), SourcePlaybackState::Frozen);
        assert_eq!(h.span(2).unwrap().state(), SourcePlaybackState::Running);
        let last = source_descriptor(h.span(2).unwrap());
        let pin = h.span(2).unwrap().physical_start();
        let end = h.pulled_end_frame();
        let initial_playback = h.initial_playback_frame();
        let converter_pos = owner.source_position();
        let target_time = owner.target_time();
        let phases = words(&owner);
        let (result, counts) = tracked(|| owner.retire_source_playback_before(pin));
        assert_eq!(result, Ok(2));
        assert_eq!(counts, [0; 3]);
        let h = owner.source_playback_history().unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(source_descriptor(h.span(0).unwrap()), last);
        assert_eq!(h.initial_physical_frame(), 1);
        assert_eq!(h.initial_playback_frame(), initial_playback);
        assert_eq!(h.source_rate(), source);
        assert_eq!(h.pulled_end_frame(), end);
        assert_eq!(h.retirement_revision(), 1);
        assert_eq!(owner.source_position(), converter_pos);
        assert_eq!(owner.target_time(), target_time);
        assert_eq!(words(&owner), phases);
        let next = render(&mut owner, false, target as usize);
        assert_eq!(next.source_start_position, converter_pos);
        assert_eq!(next.target_start_time, target_time);
        assert_eq!(owner.source_playback_history().unwrap().len(), 1);
    }
}
