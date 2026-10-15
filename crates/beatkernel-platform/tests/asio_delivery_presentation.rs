//! Original report and native delivered-prefix evidence, without an SDK or driver.
use beatkernel::{
    audio::{AudioCounters, RenderReport},
    time::{
        ClockDomainId, ClockInterval, ClockMappingQuality, ClockPoint, Duration,
        ExtrapolationPolicy, Timestamp,
    },
};
use beatkernel_platform::audio::asio::{
    AsioPresentationClock, AsioPresentationError, AsioPresentationObservation,
    MultimediaHostInterval,
};

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn interval(before: i64, after: i64) -> MultimediaHostInterval {
    MultimediaHostInterval {
        before: point(1, before),
        after: point(1, after),
    }
}
fn report(start_frame: u64, frames: usize) -> RenderReport {
    RenderReport {
        start_frame,
        frames,
        playback_start_frame: start_frame,
        playback_frames: frames,
        paused: false,
        playback_end_physical_frame: None,
        active_voices: 2,
        pending_commands: 3,
        song_position: Timestamp::from_nanos(-17),
        producer_disconnected: true,
        counters: AudioCounters {
            rendered_frames: 123,
            commands_consumed: 11,
            commands_applied: 9,
            commands_expired: 2,
            ..AudioCounters::default()
        },
    }
}
fn delivery(
    render: RenderReport,
    first: u64,
    frames: usize,
    hz: u32,
    before: i64,
    after: i64,
) -> AsioPresentationObservation {
    AsioPresentationObservation::from_delivery(
        render,
        first,
        frames,
        hz,
        interval(before, after),
        0,
        0,
        point(2, 0),
    )
    .unwrap()
}

#[test]
fn prefix_grid_and_end_keep_the_whole_original_report_and_finite_marker() {
    let mut render = report(10, 8);
    render.playback_frames = 6;
    render.paused = true;
    render.playback_end_physical_frame = Some(16);
    let prefix = AsioPresentationObservation::from_delivery(
        render,
        14,
        2,
        1_000_000_000,
        interval(100, 110),
        0,
        0,
        point(2, -5),
    )
    .unwrap();
    assert_eq!(prefix.render, render);
    assert_eq!(prefix.delivered_first_frame, 14);
    assert_eq!(prefix.delivered_frames, 2);
    assert_eq!(prefix.delivered_end_frame(), Ok(16));
    assert_eq!(prefix.output, point(2, 9));
    assert_eq!(prefix.host, interval(100, 110));
    // The unsubmitted tail stays represented as original render data.
    assert_eq!(prefix.render.start_frame + prefix.render.frames as u64, 18);
    assert_eq!(prefix.render.playback_end_physical_frame, Some(16));
}

#[test]
fn whole_render_constructor_preserves_existing_grid_latency_and_report() {
    let render = report(13, 8);
    let whole = AsioPresentationObservation::from_render(
        render,
        48_000,
        interval(100, 110),
        5,
        2,
        point(2, -8),
    )
    .unwrap();
    let explicit = AsioPresentationObservation::from_delivery(
        render,
        13,
        8,
        48_000,
        interval(100, 110),
        5,
        2,
        point(2, -8),
    )
    .unwrap();
    assert_eq!(whole, explicit);
    assert_eq!(whole.render, render);
    assert_eq!(whole.output, point(2, 270_825));
    assert_eq!(whole.delivered_end_frame(), Ok(21));
}

#[test]
fn prefix_host_latency_keeps_outward_rounding_and_original_bracket() {
    let prefix = AsioPresentationObservation::from_delivery(
        report(0, 8),
        2,
        2,
        3,
        interval(100, 110),
        1,
        7,
        point(2, 0),
    )
    .unwrap();
    assert_eq!(prefix.output, point(2, 666_666_666));
    assert_eq!(prefix.host.before, point(1, 333_333_426));
    assert_eq!(prefix.host.after, point(1, 333_333_451));
}

#[test]
fn empty_and_outside_delivery_refuse_before_constructing_evidence() {
    for (first, frames) in [(9, 1), (10, 0), (17, 2), (18, 1)] {
        assert_eq!(
            AsioPresentationObservation::from_delivery(
                report(10, 8),
                first,
                frames,
                1_000_000_000,
                interval(100, 110),
                0,
                0,
                point(2, 0),
            ),
            Err(AsioPresentationError::Malformed)
        );
    }
    assert_eq!(
        AsioPresentationObservation::from_delivery(
            report(10, 0),
            10,
            1,
            1_000_000_000,
            interval(100, 110),
            0,
            0,
            point(2, 0),
        ),
        Err(AsioPresentationError::Malformed)
    );
}

#[test]
fn both_original_report_and_delivery_extents_are_checked_for_overflow() {
    for (render, first, frames) in [
        (report(u64::MAX - 1, 2), u64::MAX - 1, 1),
        (report(u64::MAX - 2, 2), u64::MAX - 1, 2),
    ] {
        assert_eq!(
            AsioPresentationObservation::from_delivery(
                render,
                first,
                frames,
                u32::MAX,
                interval(100, 110),
                0,
                0,
                point(2, 0),
            ),
            Err(AsioPresentationError::Overflow)
        );
    }
    let mut forged = delivery(report(10, 8), 10, 2, 1_000_000_000, 100, 110);
    forged.render.start_frame = u64::MAX - 1;
    forged.render.frames = 2;
    assert_eq!(
        forged.delivered_end_frame(),
        Err(AsioPresentationError::Overflow)
    );
}

#[test]
fn prefix_rejects_invalid_rate_domains_intervals_and_timestamp_narrowing() {
    for (hz, host, origin, expected) in [
        (
            0,
            interval(100, 110),
            point(2, 0),
            AsioPresentationError::Malformed,
        ),
        (
            1,
            interval(111, 110),
            point(2, 0),
            AsioPresentationError::Malformed,
        ),
        (
            1,
            interval(100, 110),
            point(1, 0),
            AsioPresentationError::Malformed,
        ),
        (
            1,
            interval(100, 110),
            point(2, i64::MAX),
            AsioPresentationError::Overflow,
        ),
    ] {
        assert_eq!(
            AsioPresentationObservation::from_delivery(report(0, 8), 1, 1, hz, host, 0, 0, origin,),
            Err(expected)
        );
    }
    let mismatched = MultimediaHostInterval {
        before: point(1, 100),
        after: point(3, 110),
    };
    assert_eq!(
        AsioPresentationObservation::from_delivery(
            report(0, 8),
            1,
            1,
            1,
            mismatched,
            0,
            0,
            point(2, 0),
        ),
        Err(AsioPresentationError::Malformed)
    );
}

#[test]
fn adjacent_prefixes_from_one_report_calibrate_using_delivered_frame_order() {
    let render = report(64, 8);
    let first = delivery(render, 64, 4, 1_000_000_000, 100, 104);
    let second = delivery(render, 68, 4, 1_000_000_000, 108, 112);
    let clock = AsioPresentationClock::from_observations(
        first,
        second,
        ClockInterval {
            start: Timestamp::from_nanos(64),
            end: Timestamp::from_nanos(68),
        },
        ExtrapolationPolicy::Forbid,
        None,
    )
    .unwrap();
    assert_eq!(clock.observations(), (first, second));
    assert_eq!(clock.quality(), ClockMappingQuality::Unknown);
    assert_eq!(clock.map_host(point(1, 106)), Ok(point(2, 66)));
    assert!(clock.map_host(point(1, 111)).is_err());
    assert!(clock.map_host(point(3, 106)).is_err());
    assert!(clock.transport(Timestamp::ZERO).is_err());
}

#[test]
fn calibration_revalidates_delivery_overlap_grid_and_original_extent() {
    let render = report(64, 16);
    let first = delivery(render, 64, 4, 1_000_000_000, 100, 104);
    let second = delivery(render, 68, 4, 1_000_000_000, 108, 112);
    let validity = ClockInterval {
        start: Timestamp::from_nanos(64),
        end: Timestamp::from_nanos(68),
    };
    let mut wrong_grid = second;
    wrong_grid.output.timestamp = Timestamp::from_nanos(69);
    let mut empty = second;
    empty.delivered_frames = 0;
    let mut outside = second;
    outside.delivered_frames = 13;
    let overlapping = delivery(render, 67, 4, 1_000_000_000, 108, 112);
    let mut original_overflow = second;
    original_overflow.render.start_frame = u64::MAX - 1;
    original_overflow.render.frames = 2;
    for forged in [wrong_grid, empty, outside, overlapping, original_overflow] {
        assert!(AsioPresentationClock::from_observations(
            first,
            forged,
            validity,
            ExtrapolationPolicy::Forbid,
            Some(Duration::ZERO),
        )
        .is_err());
    }
}

#[test]
fn rounded_equal_output_points_cannot_invent_a_finite_affine_slope() {
    let render = report(0, 4);
    let first = delivery(render, 0, 1, u32::MAX, 100, 104);
    let second = delivery(render, 1, 1, u32::MAX, 108, 112);
    assert_eq!(first.output, second.output);
    assert_eq!(
        AsioPresentationClock::from_observations(
            first,
            second,
            ClockInterval {
                start: Timestamp::ZERO,
                end: Timestamp::from_nanos(1)
            },
            ExtrapolationPolicy::Forbid,
            None,
        ),
        Err(AsioPresentationError::Malformed)
    );
}
