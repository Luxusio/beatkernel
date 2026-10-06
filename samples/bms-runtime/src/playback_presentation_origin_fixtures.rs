//! Deferred mapping arithmetic from actual pause/resume acknowledgements.
use super::*;
use beatkernel::audio::*;
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn frame_ns(frame: u64, rate: u32) -> i64 {
    i64::try_from(u128::from(frame) * 1_000_000_000 / u128::from(rate)).unwrap()
}
fn pair(frame: u64, rate: u32, origin: i64) -> ClockPair {
    ClockPair {
        source: point(1, origin + frame_ns(frame, rate)),
        target: point(2, frame_ns(frame, rate) + 100),
    }
}
fn acknowledged(rate: u32, origin: i64, start: Option<u64>) -> NativePause {
    let format = AudioFormat::new(rate, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 128, 1).unwrap()).unwrap();
    let (mut producer, consumer) = if start.is_some() {
        command_queue_with_start_gate(8)
    } else {
        command_queue(8)
    }
    .unwrap();
    if let Some(start) = start {
        producer.schedule_start_at(start).unwrap();
    }
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(1),
            Timestamp::from_nanos(origin),
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut pause = NativePause::new(point(1, origin), ClockDomainId(2), rate).unwrap();
    if let Some(start) = start {
        pause = pause.with_start_frame(start).unwrap();
    }
    mixer
        .render(&mut vec![0.; start.unwrap_or(0) as usize + 1])
        .unwrap();
    pause
        .request(true, pair(mixer.frame_cursor(), rate, origin))
        .unwrap();
    producer.request_pause(true);
    let report = mixer.render(&mut [0.; 1]).unwrap();
    pause
        .observe(Some(report), pair(mixer.frame_cursor(), rate, origin))
        .unwrap()
        .unwrap();
    pause
        .request(false, pair(mixer.frame_cursor(), rate, origin))
        .unwrap();
    producer.request_pause(false);
    let report = mixer.render(&mut [0.; 1]).unwrap();
    pause
        .observe(Some(report), pair(mixer.frame_cursor(), rate, origin))
        .unwrap()
        .unwrap();
    pause
        .request(true, pair(mixer.frame_cursor(), rate, origin))
        .unwrap();
    producer.request_pause(true);
    let report = mixer.render(&mut [0.; 1]).unwrap();
    pause
        .observe(Some(report), pair(mixer.frame_cursor(), rate, origin))
        .unwrap()
        .unwrap();
    assert_eq!(pause.phase(), PausePhase::Paused);
    assert_eq!(pause.gap, start.unwrap_or(0) + 1);
    pause
}
#[test]
fn original_startup_origin_matches_existing_mapping_and_translated_origins_keep_exact_week_or_twenty_hour_song()
 {
    for rate in [3, 48_000] {
        for start in [None, Some(3)] {
            for origin in [-2_000_000_000, 0] {
                let pause = acknowledged(rate, origin, start);
                let startup = origin + frame_ns(start.unwrap_or(0), rate);
                assert_eq!(pause.host_domain(), ClockDomainId(2));
                for original in [72_000_000_000_000, 604_800_000_000_000] {
                    let song = Timestamp::from_nanos(original);
                    assert_eq!(
                        pause
                            .song_origin_for_presentation(song, point(1, startup))
                            .unwrap(),
                        pause.song_origin_after_pause(song).unwrap()
                    );
                    for delta in [0, 1, 333_333_333, 1_666_666_666] {
                        let expected = i128::from(original) - i128::from(frame_ns(1, rate))
                            + i128::from(delta);
                        assert_eq!(
                            pause
                                .song_origin_for_presentation(song, point(1, startup + delta))
                                .unwrap(),
                            Timestamp::from_nanos(i64::try_from(expected).unwrap())
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn wide_combination_allows_intermediate_underflow_cancellation_and_refuses_only_final_overflow() {
    let pause = acknowledged(3, 0, None);
    let gap_ns = 333_333_333;
    assert!(
        pause
            .song_origin_after_pause(Timestamp::from_nanos(i64::MIN))
            .is_err()
    );
    assert_eq!(
        pause
            .song_origin_for_presentation(Timestamp::from_nanos(i64::MIN), point(1, gap_ns))
            .unwrap(),
        Timestamp::from_nanos(i64::MIN)
    );
    assert!(
        pause
            .song_origin_for_presentation(Timestamp::from_nanos(i64::MIN), point(1, gap_ns - 1))
            .is_err()
    );
    assert_eq!(
        pause
            .song_origin_for_presentation(Timestamp::from_nanos(i64::MAX), point(1, gap_ns))
            .unwrap(),
        Timestamp::from_nanos(i64::MAX)
    );
    assert!(
        pause
            .song_origin_for_presentation(Timestamp::from_nanos(i64::MAX), point(1, gap_ns + 1))
            .is_err()
    );
}
#[test]
fn wrong_domain_and_origin_before_original_startup_refuse_without_changing_acknowledged_pause() {
    let pause = acknowledged(3, -2_000_000_000, Some(3));
    let before = format!("{pause:?}");
    let startup = -1_000_000_000;
    assert!(
        pause
            .song_origin_for_presentation(Timestamp::ZERO, point(9, startup))
            .is_err()
    );
    assert!(
        pause
            .song_origin_for_presentation(Timestamp::ZERO, point(1, startup - 1))
            .is_err()
    );
    assert_eq!(format!("{pause:?}"), before);
}
