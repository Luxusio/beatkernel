use super::*;
use crate::audio::AudioClockReadingQuality;
use std::{
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration as WallDuration, Instant},
};

fn snapshot() -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Failed { hresult: -1 },
        counters: StreamCounters {
            submitted_frames: u64::MAX,
            buffer_fills: u64::MAX - 1,
            padding_frames: u32::MAX,
            inferred_starvations: u64::MAX - 2,
            inferred_deadline_misses: u64::MAX - 3,
            native_failures: u64::MAX - 4,
        },
        clock: Some(AudioClockSnapshot {
            position: u64::MAX,
            frequency: u64::MAX - 1,
            qpc_100ns: u64::MAX - 2,
            reading_quality: AudioClockReadingQuality::Degraded,
            host_point: Some(ClockPoint {
                domain: ClockDomainId(u32::MAX),
                timestamp: Timestamp::MIN,
            }),
            mapping_quality: ClockMappingQuality::Estimated {
                max_error: Duration::MAX,
            },
        }),
        render: Some(RenderReport {
            start_frame: u64::MAX,
            frames: 4,
            playback_start_frame: u64::MAX - 17,
            playback_frames: 0,
            paused: true,
            playback_end_physical_frame: Some(u64::MAX),
            active_voices: 3,
            pending_commands: 2,
            song_position: Timestamp::from_nanos(-17),
            producer_disconnected: true,
            counters: AudioCounters {
                rendered_frames: u64::MAX,
                commands_consumed: 1,
                commands_applied: 2,
                late_commands: 3,
                pending_full: 4,
                voice_full: 5,
                unknown_samples: 6,
                unknown_stops: 7,
                invalid_gains: 8,
                invalid_rates: 9,
                invalid_times: 10,
            },
        }),
    }
}

#[test]
fn first_running_deadline_observation_ignores_ready_open_and_start_delay() {
    let telemetry = Telemetry::new();
    let mut generation = 0;
    let mut ready = snapshot();
    ready.status = AudioStreamStatus::Ready;
    ready.counters.inferred_deadline_misses = 0;
    ready.clock.as_mut().unwrap().qpc_100ns = 10;
    telemetry.publish(ready, &mut generation);
    assert_eq!(telemetry.read().status, AudioStreamStatus::Ready);
    // Ready metadata is not a processing deadline baseline. The production
    // helper receives only running observations; a long initial gap is benign.
    let mut previous = None;
    let period = Duration::from_nanos(5_000_000);
    assert!(!observe_deadline(&mut previous, 10_000_000, period));
    assert_eq!(previous, Some(10_000_000));
    let mut running = ready;
    running.status = AudioStreamStatus::Running;
    running.clock.as_mut().unwrap().qpc_100ns = 10_000_000;
    telemetry.publish(running, &mut generation);
    assert_eq!(telemetry.read().counters.inferred_deadline_misses, 0);
    assert!(observe_deadline(&mut previous, 10_100_001, period));
}

#[test]
fn running_deadline_interval_is_strict_checked_and_ignores_backward_readings() {
    let period = Duration::from_nanos(5_000_000);
    let mut previous = Some(1_000_000);
    assert!(!observe_deadline(&mut previous, 1_100_000, period)); // Exactly two periods.
    assert!(observe_deadline(&mut previous, 1_200_001, period)); // One hundred ns beyond.
    assert!(!observe_deadline(&mut previous, 1_199_999, period));
    let mut extreme = Some(0);
    assert!(observe_deadline(&mut extreme, u64::MAX, Duration::MAX));
}

#[test]
fn production_publication_preserves_literal_scalar_widths_signed_times_and_all_counters() {
    let telemetry = Telemetry::new();
    let mut generation = 0;
    let expected = snapshot();
    telemetry.publish(expected, &mut generation);
    assert_eq!(generation, 2);
    assert_eq!(telemetry.read(), expected);
    let values: Vec<u64> = telemetry
        .values
        .iter()
        .map(|value| value.load(Ordering::SeqCst))
        .collect();
    assert_eq!(
        values,
        vec![
            0xffff_ffff_0000_0004,
            u64::MAX,
            u64::MAX - 1,
            u64::from(u32::MAX),
            u64::MAX - 2,
            u64::MAX - 3,
            u64::MAX - 4,
            3,
            u64::MAX,
            u64::MAX - 1,
            u64::MAX - 2,
            0x8000_0000_0000_0000,
            u64::from(u32::MAX),
            0x8000_0000_0000_0000,
            1,
            1,
            u64::MAX,
            4,
            3,
            2,
            0xffff_ffff_ffff_ffef,
            1,
            u64::MAX,
            1,
            2,
            3,
            4,
            5,
            6,
            7,
            8,
            9,
            10,
            u64::MAX - 17,
            0,
            1,
            1,
            u64::MAX
        ]
    );
}

#[test]
fn endpoint_presence_distinguishes_zero_maximum_and_absent_across_publications() {
    let telemetry = Telemetry::new();
    let mut generation = 0;
    for marker in [Some(0), Some(u64::MAX), None, Some(17)] {
        let mut expected = snapshot();
        expected
            .render
            .as_mut()
            .unwrap()
            .playback_end_physical_frame = marker;
        telemetry.publish(expected, &mut generation);
        let actual = telemetry.read();
        assert_eq!(actual, expected);
        assert_eq!(
            telemetry.values[36].load(Ordering::SeqCst),
            u64::from(marker.is_some())
        );
        assert_eq!(
            telemetry.values[37].load(Ordering::SeqCst),
            marker.unwrap_or(0)
        );
    }
}

#[test]
fn pause_and_resume_publication_preserve_distinct_physical_and_playback_grids() {
    let telemetry = Telemetry::new();
    let mut generation = 0;
    let mut expected = snapshot();
    expected.status = AudioStreamStatus::Running;
    let render = expected.render.as_mut().unwrap();
    render.start_frame = 48;
    render.frames = 16;
    render.playback_start_frame = 32;
    render.playback_frames = 0;
    render.paused = true;
    render.counters.rendered_frames = 64;
    telemetry.publish(expected, &mut generation);
    assert_eq!(telemetry.read(), expected);

    let render = expected.render.as_mut().unwrap();
    render.start_frame = 64;
    render.playback_start_frame = 32;
    render.playback_frames = 16;
    render.paused = false;
    render.counters.rendered_frames = 80;
    telemetry.publish(expected, &mut generation);
    assert_eq!(telemetry.read(), expected);
    assert_eq!(generation, 4);
}

#[test]
fn optional_clock_host_render_and_reading_quality_remain_distinct() {
    let telemetry = Telemetry::new();
    let mut generation = 0;
    for quality in [
        AudioClockReadingQuality::Accurate,
        AudioClockReadingQuality::Degraded,
        AudioClockReadingQuality::Unknown,
    ] {
        for mapping in [
            ClockMappingQuality::Exact,
            ClockMappingQuality::Unknown,
            ClockMappingQuality::Estimated {
                max_error: Duration::from_nanos(101),
            },
        ] {
            let mut expected = snapshot();
            let clock = expected.clock.as_mut().unwrap();
            clock.reading_quality = quality;
            clock.mapping_quality = mapping;
            clock.host_point = None;
            telemetry.publish(expected, &mut generation);
            assert_eq!(telemetry.read(), expected);
        }
    }
    let mut expected = snapshot();
    expected.clock = None;
    expected.render = None;
    expected.status = AudioStreamStatus::Stopped;
    telemetry.publish(expected, &mut generation);
    assert_eq!(telemetry.read(), expected);
    assert_eq!(telemetry.values[7].load(Ordering::SeqCst), 0);
    assert_eq!(telemetry.values[15].load(Ordering::SeqCst), 0);
}

#[test]
fn collision_and_generation_exhaustion_report_unavailable_without_false_zero_measurements() {
    let telemetry = Telemetry::new();
    let mut generation = 0;
    telemetry.publish(snapshot(), &mut generation);
    telemetry.version.store(3, Ordering::SeqCst);
    telemetry
        .status
        .store(status_code(AudioStreamStatus::Running), Ordering::SeqCst);
    let expected = AudioStreamSnapshot {
        telemetry_available: false,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        clock: None,
        render: None,
    };
    let start = Instant::now();
    assert_eq!(telemetry.read(), expected);
    assert!(start.elapsed() < WallDuration::from_secs(1));
    generation = u64::MAX - 1;
    let mut incoming = snapshot();
    incoming.status = AudioStreamStatus::WorkerPanicked;
    telemetry.publish(incoming, &mut generation);
    assert_eq!(generation, u64::MAX - 1);
    assert_eq!(telemetry.version.load(Ordering::SeqCst), u64::MAX);
    let exhausted = telemetry.read();
    assert!(!exhausted.telemetry_available);
    assert_eq!(exhausted.status, AudioStreamStatus::WorkerPanicked);
    assert_eq!(exhausted.counters, StreamCounters::default());
    assert!(exhausted.clock.is_none());
    assert!(exhausted.render.is_none());
    telemetry.publish(snapshot(), &mut generation);
    assert!(!telemetry.read().telemetry_available);
}

#[test]
fn independently_published_panic_status_overrides_coherent_measurements() {
    let telemetry = Telemetry::new();
    let mut generation = 0;
    let mut expected = snapshot();
    telemetry.publish(expected, &mut generation);
    telemetry.status.store(
        status_code(AudioStreamStatus::WorkerPanicked),
        Ordering::SeqCst,
    );
    expected.status = AudioStreamStatus::WorkerPanicked;
    assert_eq!(telemetry.read(), expected);
    for status in [
        AudioStreamStatus::Ready,
        AudioStreamStatus::Running,
        AudioStreamStatus::Stopped,
        AudioStreamStatus::Failed { hresult: i32::MIN },
        AudioStreamStatus::Failed { hresult: i32::MAX },
    ] {
        expected.status = status;
        telemetry.publish(expected, &mut generation);
        assert_eq!(telemetry.read(), expected);
    }
}

#[test]
fn concurrent_production_publications_never_return_torn_counter_clock_tuples() {
    let telemetry = Arc::new(Telemetry::new());
    let done = Arc::new(AtomicBool::new(false));
    let barrier = Arc::new(Barrier::new(2));
    let publish = |telemetry: &Telemetry, generation: &mut u64, n: u64| {
        let mut value = snapshot();
        value.status = AudioStreamStatus::Running;
        value.render = None;
        value.counters.submitted_frames = n;
        value.counters.buffer_fills = n ^ u64::MAX;
        let clock = value.clock.as_mut().unwrap();
        clock.position = n;
        clock.qpc_100ns = n * 100;
        telemetry.publish(value, generation);
    };
    publish(&telemetry, &mut 0, 0);
    let writer = {
        let telemetry = Arc::clone(&telemetry);
        let done = Arc::clone(&done);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            let mut generation = 2;
            barrier.wait();
            for n in 1..=10_000 {
                publish(&telemetry, &mut generation, n);
            }
            done.store(true, Ordering::Release);
        })
    };
    barrier.wait();
    let deadline = Instant::now() + WallDuration::from_secs(10);
    let mut reads = 0;
    loop {
        let value = telemetry.read();
        if value.telemetry_available {
            let n = value.counters.submitted_frames;
            assert_eq!(value.counters.buffer_fills, n ^ u64::MAX);
            let clock = value.clock.unwrap();
            assert_eq!(clock.position, n);
            assert_eq!(clock.qpc_100ns, n * 100);
            reads += 1;
        }
        if done.load(Ordering::Acquire) && value.telemetry_available {
            break;
        }
        assert!(Instant::now() < deadline, "telemetry writer did not finish");
    }
    writer.join().unwrap();
    assert!(reads > 0);
    assert_eq!(telemetry.read().counters.submitted_frames, 10_000);
}
