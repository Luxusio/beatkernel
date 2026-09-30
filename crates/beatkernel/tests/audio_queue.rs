use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    thread,
    time::{Duration, Instant},
};

use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioError, AudioLimits, QueuePopError, QueuePushError,
        SampleId, VoiceId,
    },
    time::Timestamp,
    transport::Rate,
};

thread_local! {
    static TRACK_ALLOCATIONS: Cell<bool> = const { Cell::new(false) };
    static ALLOCATION_COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}
struct TrackingAllocator;
fn allocation(kind: usize) {
    let _ = TRACK_ALLOCATIONS.try_with(|enabled| {
        if enabled.get() {
            let _ = ALLOCATION_COUNTS.try_with(|counts| {
                let mut value = counts.get();
                value[kind] += 1;
                counts.set(value);
            });
        }
    });
}
// SAFETY: Every allocation operation delegates its original pointer/layout to
// System. Thread-local scalar counters never allocate or modify returned memory.
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocation(0);
        // SAFETY: GlobalAlloc supplies a valid allocation layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocation(0);
        // SAFETY: GlobalAlloc supplies a valid allocation layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        allocation(1);
        // SAFETY: The caller supplies a live System allocation, its layout, and
        // a nonzero replacement size under the GlobalAlloc contract.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        allocation(2);
        // SAFETY: The caller supplies the original live allocation and layout.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

fn tracked<T>(operation: impl FnOnce() -> T) -> (T, [usize; 3]) {
    ALLOCATION_COUNTS.with(|counts| counts.set([0; 3]));
    TRACK_ALLOCATIONS.with(|enabled| enabled.set(true));
    let result = operation();
    TRACK_ALLOCATIONS.with(|enabled| enabled.set(false));
    (result, ALLOCATION_COUNTS.with(Cell::get))
}

fn command(sequence: u64) -> AudioCommand {
    match sequence % 4 {
        0 => AudioCommand::Play {
            voice: VoiceId(u64::MAX - sequence),
            sample: SampleId(sequence),
            at: Timestamp::MIN,
            gain: f32::from_bits(0x7fc0_1234),
        },
        1 => AudioCommand::Stop {
            voice: VoiceId(sequence),
            at: Timestamp::MAX,
        },
        2 => AudioCommand::SetRate {
            rate: Rate::new(i64::MIN, u64::MAX).unwrap(),
            at: Timestamp::from_nanos(-1),
        },
        _ => AudioCommand::Seek {
            song_time: Timestamp::MIN,
            at: Timestamp::from_nanos(i64::try_from(sequence).unwrap()),
        },
    }
}
fn assert_command(actual: AudioCommand, expected: AudioCommand) {
    match (actual, expected) {
        (
            AudioCommand::Play {
                voice: av,
                sample: asample,
                at: atime,
                gain: again,
            },
            AudioCommand::Play {
                voice: ev,
                sample: esample,
                at: etime,
                gain: egain,
            },
        ) => {
            assert_eq!((av, asample, atime), (ev, esample, etime));
            assert_eq!(again.to_bits(), egain.to_bits());
        }
        (actual, expected) => assert_eq!(actual, expected),
    }
}

#[test]
fn capacities_are_bounded_and_initial_queue_is_empty() {
    for capacity in [0, AudioLimits::MAX_COMMANDS + 1, usize::MAX] {
        assert!(matches!(
            command_queue(capacity),
            Err(AudioError::InvalidCapacity)
        ));
    }
    for capacity in [1, 2, AudioLimits::MAX_COMMANDS] {
        let (producer, mut consumer) = command_queue(capacity).unwrap();
        assert_eq!(producer.capacity(), capacity);
        assert_eq!(consumer.capacity(), capacity);
        assert!(!producer.is_disconnected());
        assert!(!consumer.is_disconnected());
        assert_eq!(consumer.available(), 0);
        assert_eq!(consumer.available_up_to(usize::MAX), 0);
        assert_eq!(consumer.try_pop(), Err(QueuePopError::Empty));
        let counters = producer.counters();
        assert_eq!(
            (counters.accepted, counters.full, counters.disconnected),
            (0, 0, 0)
        );
    }
}

#[test]
fn full_recovers_original_command_and_never_overwrites_fifo() {
    for capacity in [1, 2] {
        let (mut producer, mut consumer) = command_queue(capacity).unwrap();
        for sequence in 0..capacity {
            producer.try_push(command(sequence as u64)).unwrap();
        }
        assert_eq!(consumer.available(), capacity);
        assert_eq!(consumer.available_up_to(0), 0);
        assert_eq!(consumer.available_up_to(1), 1);
        assert_eq!(consumer.available_up_to(usize::MAX), capacity);
        let rejected = command(100);
        let error = producer.try_push(rejected).unwrap_err();
        assert_eq!(error.reason, QueuePushError::Full);
        assert_command(error.command, rejected);
        assert_command(consumer.try_pop().unwrap(), command(0));
        producer.try_push(rejected).unwrap();
        for sequence in 1..capacity {
            assert_command(consumer.try_pop().unwrap(), command(sequence as u64));
        }
        assert_command(consumer.try_pop().unwrap(), rejected);
        assert_eq!(consumer.try_pop(), Err(QueuePopError::Empty));
        let counters = producer.counters();
        assert_eq!(counters.accepted, capacity as u64 + 1);
        assert_eq!(counters.full, 1);
        assert_eq!(counters.disconnected, 0);
    }
}

#[test]
fn scalar_encoding_preserves_all_variants_extreme_widths_and_float_bits() {
    let (mut producer, mut consumer) = command_queue(2).unwrap();
    let commands = [
        command(0),
        command(1),
        command(2),
        command(3),
        AudioCommand::Play {
            voice: VoiceId(0),
            sample: SampleId(u64::MAX),
            at: Timestamp::MAX,
            gain: -0.0,
        },
        AudioCommand::SetRate {
            rate: Rate::ZERO,
            at: Timestamp::MIN,
        },
        AudioCommand::SetRate {
            rate: Rate::new(i64::MAX, 1).unwrap(),
            at: Timestamp::MAX,
        },
        AudioCommand::Seek {
            song_time: Timestamp::MAX,
            at: Timestamp::MIN,
        },
    ];
    for expected in commands {
        producer.try_push(expected).unwrap();
        assert_command(consumer.try_pop().unwrap(), expected);
    }
}

#[test]
fn thousands_of_capacity_one_and_two_wraps_preserve_order_and_recovery() {
    for capacity in [1, 2] {
        let (mut producer, mut consumer) = command_queue(capacity).unwrap();
        for cycle in 0..5000_u64 {
            for slot in 0..capacity {
                producer
                    .try_push(command(cycle * capacity as u64 + slot as u64))
                    .unwrap();
            }
            let error = producer.try_push(command(u64::MAX - 3)).unwrap_err();
            assert_eq!(error.reason, QueuePushError::Full);
            assert_command(error.command, command(u64::MAX - 3));
            for slot in 0..capacity {
                assert_command(
                    consumer.try_pop().unwrap(),
                    command(cycle * capacity as u64 + slot as u64),
                );
            }
            assert_eq!(consumer.available(), 0);
            assert_eq!(consumer.try_pop(), Err(QueuePopError::Empty));
        }
        assert_eq!(producer.counters().accepted, 5000 * capacity as u64);
        assert_eq!(producer.counters().full, 5000);
    }
}

#[test]
fn producer_drop_allows_drain_before_disconnected_and_consumer_drop_returns_payload() {
    let (mut producer, mut consumer) = command_queue(2).unwrap();
    producer.try_push(command(0)).unwrap();
    producer.try_push(command(1)).unwrap();
    drop(producer);
    assert!(consumer.is_disconnected());
    assert_eq!(consumer.available(), 2);
    assert_command(consumer.try_pop().unwrap(), command(0));
    assert_command(consumer.try_pop().unwrap(), command(1));
    assert_eq!(consumer.try_pop(), Err(QueuePopError::Disconnected));
    assert_eq!(consumer.available_up_to(2), 0);

    let (mut producer, consumer) = command_queue(1).unwrap();
    producer.try_push(command(0)).unwrap();
    drop(consumer);
    assert!(producer.is_disconnected());
    let error = producer.try_push(command(4)).unwrap_err();
    assert_eq!(error.reason, QueuePushError::Disconnected);
    assert_command(error.command, command(4));
    let counters = producer.counters();
    assert_eq!(
        (counters.accepted, counters.full, counters.disconnected),
        (1, 0, 1)
    );
    let (producer, mut consumer) = command_queue(1).unwrap();
    drop(producer);
    assert_eq!(consumer.try_pop(), Err(QueuePopError::Disconnected));
}

#[test]
fn concurrent_publication_delivers_exact_fifo_with_bounded_timeout() {
    const COUNT: u64 = 20_000;
    for capacity in [1, 2] {
        let (mut producer, mut consumer) = command_queue(capacity).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let writer = thread::spawn(move || -> Result<(), &'static str> {
            for sequence in 0..COUNT {
                let mut pending = command(sequence);
                loop {
                    if Instant::now() >= deadline {
                        return Err("producer timed out");
                    }
                    match producer.try_push(pending) {
                        Ok(()) => break,
                        Err(error) if error.reason == QueuePushError::Full => {
                            pending = error.command;
                            thread::yield_now();
                        }
                        Err(_) => return Err("consumer disconnected early"),
                    }
                }
            }
            Ok(())
        });
        let mut received = 0;
        while received < COUNT && Instant::now() < deadline {
            match consumer.try_pop() {
                Ok(actual) => {
                    assert_command(actual, command(received));
                    received += 1;
                }
                Err(QueuePopError::Empty) => thread::yield_now(),
                Err(QueuePopError::Disconnected) => break,
            }
        }
        assert_eq!(writer.join().unwrap(), Ok(()));
        assert_eq!(received, COUNT, "consumer timed out or lost commands");
        assert_eq!(consumer.try_pop(), Err(QueuePopError::Disconnected));
    }
}

#[test]
fn allocator_measurement_observes_alloc_realloc_and_dealloc_on_this_thread() {
    let (_, counts) = tracked(|| {
        let mut bytes = Vec::with_capacity(1);
        bytes.push(7_u8);
        bytes.reserve_exact(1024);
        std::hint::black_box(&bytes);
        drop(bytes);
    });
    assert!(counts.iter().all(|count| *count > 0));
}

#[test]
fn push_pop_full_idle_and_disconnected_paths_do_not_allocate_or_free() {
    let (mut producer, mut consumer) = command_queue(1).unwrap();
    let play = command(0);
    let ((idle, pushed, full, available, popped, empty, counters), counts) = tracked(|| {
        let idle = consumer.try_pop();
        let pushed = producer.try_push(play);
        let full = producer.try_push(play);
        let available = consumer.available_up_to(2);
        let popped = consumer.try_pop();
        let empty = consumer.try_pop();
        (
            idle,
            pushed,
            full,
            available,
            popped,
            empty,
            producer.counters(),
        )
    });
    assert_eq!(
        counts,
        [0, 0, 0],
        "alloc/realloc/dealloc in queue operations"
    );
    assert_eq!(idle, Err(QueuePopError::Empty));
    assert!(pushed.is_ok());
    assert_eq!(full.unwrap_err().reason, QueuePushError::Full);
    assert_eq!(available, 1);
    assert_command(popped.unwrap(), play);
    assert_eq!(empty, Err(QueuePopError::Empty));
    assert_eq!((counters.accepted, counters.full), (1, 1));
    producer.try_push(play).unwrap();
    drop(producer);
    let ((last, disconnected, is_disconnected), counts) = tracked(|| {
        (
            consumer.try_pop(),
            consumer.try_pop(),
            consumer.is_disconnected(),
        )
    });
    assert_eq!(counts, [0, 0, 0]);
    assert_command(last.unwrap(), play);
    assert_eq!(disconnected, Err(QueuePopError::Disconnected));
    assert!(is_disconnected);
    drop(consumer);

    let (mut producer, consumer) = command_queue(1).unwrap();
    drop(consumer);
    let ((rejected, disconnected, counters), counts) = tracked(|| {
        (
            producer.try_push(play),
            producer.is_disconnected(),
            producer.counters(),
        )
    });
    assert_eq!(counts, [0, 0, 0]);
    let error = rejected.unwrap_err();
    assert_eq!(error.reason, QueuePushError::Disconnected);
    assert_command(error.command, play);
    assert!(disconnected);
    assert_eq!(counters.disconnected, 1);
    drop(producer);
}
