//! Pure video supply contract: original-song timestamps and owned RGBA only.
use std::{collections::VecDeque, sync::Arc};

use beatkernel::time::Timestamp;
use beatkernel_bms::{BgaChannel, ImageId, ScheduledBga};
use beatkernel_bms_runtime::{
    bga::{BgaActivation, BgaState, BgaTimeline},
    texture::RgbaImage,
    video::{
        VideoDecodeEvent, VideoDecoderCapabilities, VideoDecoderPort, VideoFrame,
        VideoFrameAdmission, VideoFrameLimits, VideoFrameQueue, VideoSessionKey, VideoTimeBase,
    },
};

fn ns(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}

fn session(generation: u64) -> VideoSessionKey {
    VideoSessionKey {
        content: 7,
        generation,
        image: ImageId(3),
        channel: BgaChannel::Base,
        activated_at: ns(100),
        ordinal: Some(9),
    }
}

fn limits(count: usize, bytes: u64) -> VideoFrameLimits {
    VideoFrameLimits {
        max_frames: count,
        max_bytes: bytes,
        max_frame_bytes: bytes,
    }
}

fn frame(key: VideoSessionKey, pts: i64, revision: u64, color: u8) -> VideoFrame {
    VideoFrame {
        session: key,
        pts: ns(pts),
        revision,
        image: Arc::new(RgbaImage::new(1, 1, vec![color, 0, 0, 255]).unwrap()),
    }
}

fn accepted(queue: &mut VideoFrameQueue, frame: VideoFrame) {
    assert!(matches!(
        queue.push(frame).unwrap(),
        VideoFrameAdmission::Accepted
    ));
}

fn selected(queue: &mut VideoFrameQueue, target: i64, pts: i64, revision: u64, color: u8) {
    let chosen = queue.select(ns(target)).expect("eligible completed frame");
    assert_eq!((chosen.pts, chosen.revision), (ns(pts), revision));
    assert_eq!(chosen.image.pixels(), &[color, 0, 0, 255]);
}

#[test]
fn activation_retains_timestamp_ordinal_and_repeated_image_identity() {
    let event = |at, ordinal, channel, image| ScheduledBga {
        at: ns(at),
        ordinal,
        channel,
        image: ImageId(image),
    };
    let events = vec![
        event(0, 0, BgaChannel::Base, 3),
        event(10, 1, BgaChannel::Layer, 4),
        event(10, 2, BgaChannel::Layer2, 5),
        event(20, 3, BgaChannel::Base, 3),
        event(20, 4, BgaChannel::Base, 3),
        event(30, 5, BgaChannel::Poor, 6),
    ];
    let timeline = BgaTimeline::new(events.clone(), Some(ImageId(0))).unwrap();
    let at = timeline.activations_at(ns(20));
    assert_eq!(
        at,
        [
            Some(BgaActivation {
                channel: BgaChannel::Base,
                image: ImageId(3),
                activated_at: ns(20),
                ordinal: Some(4)
            }),
            Some(BgaActivation {
                channel: BgaChannel::Layer,
                image: ImageId(4),
                activated_at: ns(10),
                ordinal: Some(1)
            }),
            Some(BgaActivation {
                channel: BgaChannel::Poor,
                image: ImageId(0),
                activated_at: Timestamp::ZERO,
                ordinal: None
            }),
            Some(BgaActivation {
                channel: BgaChannel::Layer2,
                image: ImageId(5),
                activated_at: ns(10),
                ordinal: Some(2)
            }),
        ]
    );
    assert_eq!(timeline.activations_at(ns(20)), at); // Pause has no accumulated clock.
    assert_eq!(timeline.activations_at(ns(19))[0].unwrap().ordinal, Some(0));
    assert_eq!(
        timeline.activations_at(ns(30))[2],
        Some(BgaActivation {
            channel: BgaChannel::Poor,
            image: ImageId(6),
            activated_at: ns(30),
            ordinal: Some(5),
        })
    );
    assert_eq!(
        timeline.state_at(ns(20)),
        BgaState {
            base: Some(ImageId(3)),
            layer: Some(ImageId(4)),
            poor: Some(ImageId(0)),
            layer2: Some(ImageId(5)),
        }
    );
    assert_eq!(timeline.export_events(), events);
    assert_eq!(timeline.initial_poor(), Some(ImageId(0)));
}

#[test]
fn parsed_bmp00_is_initial_selection_and_zero_markers_do_not_restart() {
    let chart = beatkernel_bms::parse(
        "#BPM 120\n#BMP00 poor.png\n#BMP01 clip.mp4\n#00004:0100\n#00104:0001\n",
        beatkernel_bms::ParseOptions::default(),
    )
    .unwrap();
    let timeline = BgaTimeline::from_chart(&chart).unwrap();
    let initial = timeline.activations_at(ns(-1));
    assert!(initial[0].is_none());
    assert_eq!(initial[2].unwrap().ordinal, None);
    assert_eq!(initial[2].unwrap().activated_at, Timestamp::ZERO);
    assert_eq!(
        timeline.activations_at(ns(2_999_999_999))[0]
            .unwrap()
            .activated_at,
        Timestamp::ZERO
    );
    let repeated = timeline.activations_at(ns(3_000_000_000))[0].unwrap();
    assert_eq!(repeated.image, ImageId(1));
    assert_eq!(repeated.activated_at, ns(3_000_000_000));
    assert_ne!(
        repeated.ordinal,
        timeline.activations_at(Timestamp::ZERO)[0].unwrap().ordinal
    );
}

#[test]
fn rational_pts_uses_floor_preserves_gaps_and_retains_source_origin() {
    let thirds = VideoTimeBase::new(1, 3).unwrap();
    assert_eq!(thirds.timestamp(101, 100).unwrap(), ns(333_333_333));
    assert_eq!(thirds.timestamp(102, 100).unwrap(), ns(666_666_666));
    assert_eq!(thirds.timestamp(104, 100).unwrap(), ns(1_333_333_333));
    assert_eq!(thirds.timestamp(99, 100).unwrap(), ns(-333_333_334));
    assert_eq!(thirds.timestamp(98, 100).unwrap(), ns(-666_666_667));
    assert_eq!(thirds.timestamp(100, 100).unwrap(), Timestamp::ZERO);
    // A seek still uses the first source presentation origin, not its first output.
    assert_eq!(thirds.timestamp(106, 100).unwrap(), ns(2_000_000_000));
    assert!(VideoTimeBase::new(0, 1).is_err());
    assert!(VideoTimeBase::new(1, 0).is_err());
}

#[test]
fn pts_boundaries_are_checked_after_i128_origin_subtraction() {
    let nanos = VideoTimeBase::new(1, 1_000_000_000).unwrap();
    assert_eq!(nanos.timestamp(i64::MAX, 0).unwrap(), ns(i64::MAX));
    assert_eq!(nanos.timestamp(i64::MIN, 0).unwrap(), ns(i64::MIN));
    assert_eq!(nanos.timestamp(i64::MIN + 5, i64::MIN).unwrap(), ns(5));
    assert!(nanos.timestamp(i64::MAX, i64::MIN).is_err());
    assert!(nanos.timestamp(i64::MIN, i64::MAX).is_err());
    let large = VideoTimeBase::new(u32::MAX, 1).unwrap();
    assert!(large.timestamp(i64::MAX, i64::MIN).is_err());
    assert_eq!(
        large.timestamp(i64::MAX, i64::MAX).unwrap(),
        Timestamp::ZERO
    );
}

#[test]
fn twenty_hour_and_week_pts_have_no_float_or_cumulative_drift() {
    let ticks = VideoTimeBase::new(1, 90_000).unwrap();
    let origin = 8_123_456_789;
    assert_eq!(
        ticks.timestamp(origin + 6_480_000_000, origin).unwrap(),
        ns(72_000_000_000_000)
    );
    assert_eq!(
        ticks.timestamp(origin + 54_432_000_000, origin).unwrap(),
        ns(604_800_000_000_000)
    );
    assert_eq!(
        ticks.timestamp(origin + 54_432_000_001, origin).unwrap(),
        ns(604_800_000_011_111)
    );
}

#[test]
fn target_is_exact_song_minus_activation_including_poor_miss_time() {
    let key = session(1);
    assert_eq!(key.target(ns(130)).unwrap(), ns(30));
    assert_eq!(key.target(ns(130)).unwrap(), ns(30));
    assert_eq!(key.target(ns(90)).unwrap(), ns(-10));
    let poor = VideoSessionKey {
        channel: BgaChannel::Poor,
        activated_at: ns(123),
        ordinal: None,
        ..key
    };
    assert_eq!(poor.target(ns(130)).unwrap(), ns(7));
    let boundary = VideoSessionKey {
        activated_at: ns(i64::MIN),
        ..key
    };
    assert!(boundary.target(ns(i64::MAX)).is_err());
}

#[test]
fn poor_session_requires_accepted_miss_and_other_channels_keep_marker_time() {
    let poor = BgaActivation {
        channel: BgaChannel::Poor,
        image: ImageId(0),
        activated_at: Timestamp::ZERO,
        ordinal: None,
    };
    assert!(VideoSessionKey::from_activation(7, 2, poor, None).is_none());
    let visible = VideoSessionKey::from_activation(7, 2, poor, Some(ns(87))).unwrap();
    assert_eq!(
        (
            visible.content,
            visible.generation,
            visible.image,
            visible.channel,
            visible.activated_at,
            visible.ordinal
        ),
        (7, 2, ImageId(0), BgaChannel::Poor, ns(87), None)
    );
    assert_eq!(visible.target(ns(100)).unwrap(), ns(13));
    let timed_poor = BgaActivation {
        image: ImageId(6),
        activated_at: ns(50),
        ordinal: Some(8),
        ..poor
    };
    let visible = VideoSessionKey::from_activation(7, 3, timed_poor, Some(ns(90))).unwrap();
    assert_eq!(
        (visible.image, visible.activated_at, visible.ordinal),
        (ImageId(6), ns(90), Some(8))
    );
    for channel in [BgaChannel::Base, BgaChannel::Layer, BgaChannel::Layer2] {
        let marker = BgaActivation {
            channel,
            image: ImageId(3),
            activated_at: ns(50),
            ordinal: Some(8),
        };
        let visible = VideoSessionKey::from_activation(7, 3, marker, Some(ns(90))).unwrap();
        assert_eq!(
            (visible.channel, visible.activated_at, visible.ordinal),
            (channel, ns(50), Some(8))
        );
        assert!(VideoSessionKey::from_activation(7, 3, marker, None).is_some());
    }
}

#[test]
fn reorder_requires_completion_and_selects_greatest_nonfuture_pts() {
    let key = session(1);
    let mut queue = VideoFrameQueue::new(key, limits(3, 12)).unwrap();
    accepted(&mut queue, frame(key, 90, 1, 9));
    accepted(&mut queue, frame(key, 10, 2, 1));
    accepted(&mut queue, frame(key, 50, 3, 5));
    assert!(queue.select(ns(80)).is_none());
    assert!(queue.watermark(key, ns(50)).unwrap());
    selected(&mut queue, 80, 50, 3, 5);
    selected(&mut queue, 80, 50, 3, 5);
    assert!(queue.watermark(key, ns(90)).unwrap());
    selected(&mut queue, 90, 90, 1, 9);
    assert!(queue.buffered_len() <= 3);
    assert!(queue.buffered_bytes() <= 12);
}

#[test]
fn preroll_latest_before_target_is_retained_across_variable_rate_gap() {
    let key = session(1);
    let mut queue = VideoFrameQueue::new(key, limits(3, 12)).unwrap();
    accepted(&mut queue, frame(key, 0, 1, 1));
    accepted(&mut queue, frame(key, 40, 2, 4));
    accepted(&mut queue, frame(key, 200, 3, 2));
    queue.watermark(key, ns(200)).unwrap();
    selected(&mut queue, 150, 40, 2, 4);
    selected(&mut queue, 199, 40, 2, 4);
    assert_eq!(queue.buffered_len(), 2);
    assert_eq!(queue.buffered_bytes(), 8);
    selected(&mut queue, 200, 200, 3, 2);
    assert_eq!(queue.buffered_len(), 1);
    assert_eq!(queue.buffered_bytes(), 4); // Selected pixels are part of the cap.
}

#[test]
fn future_only_full_queue_returns_original_unconsumed_frame() {
    let key = session(1);
    let mut queue = VideoFrameQueue::new(key, limits(2, 8)).unwrap();
    accepted(&mut queue, frame(key, 50, 1, 5));
    accepted(&mut queue, frame(key, 100, 2, 1));
    queue.watermark(key, ns(100)).unwrap();
    assert!(queue.select(ns(49)).is_none());
    let incoming = frame(key, 150, 3, 7);
    let pixels = Arc::clone(&incoming.image);
    let pending = match queue.push(incoming).unwrap() {
        VideoFrameAdmission::Backpressure(pending) => pending,
        _ => panic!("a full future-only queue must preserve pending ownership"),
    };
    assert!(Arc::ptr_eq(&pixels, &pending.image));
    assert_eq!(
        (pending.session, pending.pts, pending.revision),
        (key, ns(150), 3)
    );
    assert_eq!((queue.buffered_len(), queue.buffered_bytes()), (2, 8));
    selected(&mut queue, 100, 100, 2, 1);
    accepted(&mut queue, pending);
    queue.watermark(key, ns(150)).unwrap();
    selected(&mut queue, 150, 150, 3, 7);
}

#[test]
fn byte_pressure_preserves_selected_frame_and_checks_real_pixel_size() {
    let key = session(1);
    let mut queue = VideoFrameQueue::new(key, limits(3, 8)).unwrap();
    accepted(&mut queue, frame(key, 0, 1, 1));
    queue.watermark(key, ns(0)).unwrap();
    selected(&mut queue, 0, 0, 1, 1);
    let wide = VideoFrame {
        image: Arc::new(RgbaImage::new(2, 1, vec![8; 8]).unwrap()),
        ..frame(key, 20, 2, 2)
    };
    assert!(matches!(
        queue.push(wide).unwrap(),
        VideoFrameAdmission::Backpressure(_)
    ));
    selected(&mut queue, 10, 0, 1, 1);
    assert_eq!((queue.buffered_len(), queue.buffered_bytes()), (1, 4));
    let too_large = VideoFrame {
        image: Arc::new(RgbaImage::new(3, 1, vec![9; 12]).unwrap()),
        ..frame(key, 30, 3, 3)
    };
    assert!(queue.push(too_large).is_err());
    assert_eq!(queue.buffered_bytes(), 4);
}

#[test]
fn equal_pts_higher_revision_replaces_pixels_without_extra_slot() {
    let key = session(1);
    let mut queue = VideoFrameQueue::new(key, limits(1, 4)).unwrap();
    accepted(&mut queue, frame(key, 10, 5, 5));
    queue.watermark(key, ns(10)).unwrap();
    selected(&mut queue, 10, 10, 5, 5);
    assert!(matches!(
        queue.push(frame(key, 10, 4, 4)).unwrap(),
        VideoFrameAdmission::Stale
    ));
    assert!(matches!(
        queue.push(frame(key, 10, 5, 8)).unwrap(),
        VideoFrameAdmission::Stale
    ));
    accepted(&mut queue, frame(key, 10, 6, 6));
    selected(&mut queue, 10, 10, 6, 6);
    assert_eq!((queue.buffered_len(), queue.buffered_bytes()), (1, 4));
}

#[test]
fn older_than_retained_preroll_cannot_replace_selection_or_grow_budget() {
    let key = session(1);
    let mut queue = VideoFrameQueue::new(key, limits(3, 12)).unwrap();
    accepted(&mut queue, frame(key, 50, 1, 5));
    queue.watermark(key, ns(50)).unwrap();
    selected(&mut queue, 80, 50, 1, 5);
    assert!(matches!(
        queue.push(frame(key, 10, 9, 1)).unwrap(),
        VideoFrameAdmission::Obsolete
    ));
    selected(&mut queue, 80, 50, 1, 5);
    assert_eq!((queue.buffered_len(), queue.buffered_bytes()), (1, 4));
}

#[test]
fn backward_seek_resets_generation_and_stale_frames_or_watermarks_cannot_win() {
    let old = session(1);
    let new = session(2);
    let mut queue = VideoFrameQueue::new(old, limits(3, 12)).unwrap();
    accepted(&mut queue, frame(old, 100, 1, 1));
    queue.finish(Some(ns(110))).unwrap();
    selected(&mut queue, 120, 100, 1, 1);
    queue.reset(new);
    assert_eq!((queue.buffered_len(), queue.buffered_bytes()), (0, 0));
    assert!(queue.select(ns(20)).is_none());
    assert!(matches!(
        queue.push(frame(old, 10, 99, 9)).unwrap(),
        VideoFrameAdmission::Stale
    ));
    assert!(!queue.watermark(old, ns(i64::MAX)).unwrap());
    accepted(&mut queue, frame(new, 10, 1, 2));
    assert!(queue.select(ns(20)).is_none());
    queue.watermark(new, ns(20)).unwrap();
    selected(&mut queue, 20, 10, 1, 2);
    let other_content = VideoSessionKey { content: 8, ..new };
    assert!(matches!(
        queue.push(frame(other_content, 20, 2, 8)).unwrap(),
        VideoFrameAdmission::Stale
    ));
}

#[test]
fn end_completes_reordered_output_and_holds_last_without_looping() {
    let key = session(1);
    let mut queue = VideoFrameQueue::new(key, limits(3, 12)).unwrap();
    accepted(&mut queue, frame(key, 70, 2, 7));
    accepted(&mut queue, frame(key, 0, 1, 1));
    queue.finish(Some(ns(100))).unwrap();
    selected(&mut queue, 69, 0, 1, 1);
    selected(&mut queue, 70, 70, 2, 7);
    selected(&mut queue, 1_000_000, 70, 2, 7);
    selected(&mut queue, i64::MAX, 70, 2, 7);
    assert_eq!((queue.buffered_len(), queue.buffered_bytes()), (1, 4));
    queue.reset(session(2));
    queue.finish(None).unwrap();
    assert!(queue.select(ns(i64::MAX)).is_none());
}

#[test]
fn default_limits_and_invalid_limits_are_explicit() {
    let default = VideoFrameLimits::default();
    assert_eq!(
        (
            default.max_frames,
            default.max_bytes,
            default.max_frame_bytes
        ),
        (3, 192 * 1024 * 1024, 64 * 1024 * 1024)
    );
    assert!(default.validate().is_ok());
    for invalid in [
        limits(0, 4),
        limits(1, 0),
        VideoFrameLimits {
            max_frames: 1,
            max_bytes: 4,
            max_frame_bytes: 0,
        },
        VideoFrameLimits {
            max_frames: 1,
            max_bytes: 4,
            max_frame_bytes: 8,
        },
    ] {
        assert!(invalid.validate().is_err());
        assert!(VideoFrameQueue::new(session(1), invalid).is_err());
    }
}

// This dummy checks the static adapter protocol only; it does not simulate a codec.
struct ProtocolPort {
    requests: Vec<(VideoSessionKey, Timestamp)>,
    retired: Vec<VideoSessionKey>,
    events: VecDeque<VideoDecodeEvent>,
}

impl VideoDecoderPort for ProtocolPort {
    fn capabilities(&self) -> VideoDecoderCapabilities {
        VideoDecoderCapabilities {
            available: false,
            reason: Some("test protocol has no codec".into()),
        }
    }
    fn request(&mut self, key: VideoSessionKey, target: Timestamp) -> Result<(), String> {
        self.requests.push((key, target));
        Ok(())
    }
    fn try_next(&mut self) -> Option<VideoDecodeEvent> {
        self.events.pop_front()
    }
    fn retire(&mut self, key: VideoSessionKey) {
        self.retired.push(key);
    }
}

#[test]
fn static_decoder_port_carries_exact_demand_and_completion_metadata() {
    fn exercise<P: VideoDecoderPort>(port: &mut P, key: VideoSessionKey) {
        let capabilities = port.capabilities();
        assert!(!capabilities.available);
        assert_eq!(
            capabilities.reason.as_deref(),
            Some("test protocol has no codec")
        );
        port.request(key, ns(123)).unwrap();
        match port.try_next().unwrap() {
            VideoDecodeEvent::Watermark { session, through } => {
                assert_eq!((session, through), (key, ns(123)))
            }
            _ => panic!("expected presentation completion metadata"),
        }
        match port.try_next().unwrap() {
            VideoDecodeEvent::End { session, end } => {
                assert_eq!((session, end), (key, Some(ns(200))))
            }
            _ => panic!("expected explicit EOF metadata"),
        }
        assert!(port.try_next().is_none());
        port.retire(key);
    }
    let key = session(1);
    let mut port = ProtocolPort {
        requests: Vec::new(),
        retired: Vec::new(),
        events: VecDeque::from([
            VideoDecodeEvent::Watermark {
                session: key,
                through: ns(123),
            },
            VideoDecodeEvent::End {
                session: key,
                end: Some(ns(200)),
            },
        ]),
    };
    exercise(&mut port, key);
    assert_eq!(port.requests, vec![(key, ns(123))]);
    assert_eq!(port.retired, vec![key]);
}

#[test]
fn adapter_byte_preflight_counts_replacements_and_consumed_obsolete_frames() {
    let key = session(1);
    let mut queue = VideoFrameQueue::new(key, limits(1, 4)).unwrap();
    accepted(&mut queue, frame(key, 10, 1, 1));
    queue.watermark(key, ns(10)).unwrap();
    selected(&mut queue, 10, 10, 1, 1);
    assert_eq!(queue.admission_bytes(ns(10), 2, 4).unwrap(), Some(4));
    assert_eq!(queue.admission_bytes(ns(10), 1, 4).unwrap(), Some(4));
    assert_eq!(queue.admission_bytes(ns(5), 2, 4).unwrap(), Some(4));
    assert_eq!(queue.admission_bytes(ns(20), 2, 4).unwrap(), None);
    assert!(queue.admission_bytes(ns(20), 2, 5).is_err());
    assert_eq!(queue.buffered_bytes(), 4);
}
#[test]
fn completed_pixels_replace_full_selected_capacity_atomically() {
    let key = session(2);
    let mut queue = VideoFrameQueue::new(key, limits(1, 4)).unwrap();
    accepted(&mut queue, frame(key, 0, 1, 1));
    queue.watermark(key, ns(0)).unwrap();
    selected(&mut queue, 0, 0, 1, 1);
    assert_eq!(
        queue
            .completed_admission_bytes(ns(10), 2, 4, ns(10), ns(30))
            .unwrap(),
        Some(4)
    );
    assert!(matches!(
        queue
            .push_completed(frame(key, 10, 2, 2), ns(10), ns(30))
            .unwrap(),
        VideoFrameAdmission::Accepted
    ));
    selected(&mut queue, 10, 10, 2, 2);
    assert_eq!(queue.buffered_bytes(), 4);
    // Other still-pending pixels do not become complete merely by this retry.
    assert_eq!(queue.completed_through(), Some(ns(10)));
}
#[test]
fn future_or_unproven_pixels_preserve_the_full_selected_frame() {
    let key = session(3);
    let mut queue = VideoFrameQueue::new(key, limits(1, 4)).unwrap();
    accepted(&mut queue, frame(key, 0, 1, 1));
    queue.watermark(key, ns(0)).unwrap();
    for (target, through) in [(9, 30), (10, 9)] {
        assert!(matches!(
            queue
                .push_completed(frame(key, 10, 2, 2), ns(target), ns(through))
                .unwrap(),
            VideoFrameAdmission::Backpressure(_)
        ));
        selected(&mut queue, target, 0, 1, 1);
        assert_eq!(queue.completed_through(), Some(ns(0)));
    }
}
#[test]
fn reordered_completion_keeps_later_known_eligible_pixels_and_future_pixels() {
    let key = session(4);
    let mut queue = VideoFrameQueue::new(key, limits(3, 12)).unwrap();
    accepted(&mut queue, frame(key, 0, 1, 1));
    accepted(&mut queue, frame(key, 20, 2, 2));
    accepted(&mut queue, frame(key, 40, 3, 3));
    queue.watermark(key, ns(0)).unwrap();
    selected(&mut queue, 0, 0, 1, 1);
    assert!(matches!(
        queue
            .push_completed(frame(key, 10, 4, 4), ns(25), ns(30))
            .unwrap(),
        VideoFrameAdmission::Obsolete
    ));
    selected(&mut queue, 25, 20, 2, 2);
    assert_eq!(queue.buffered_len(), 2);
    assert_eq!(queue.completed_through(), Some(ns(20)));
    queue.watermark(key, ns(40)).unwrap();
    selected(&mut queue, 40, 40, 3, 3);
}
#[test]
fn completed_admission_consumes_stale_packets_without_mutating_current_generation() {
    let key = session(5);
    let mut queue = VideoFrameQueue::new(key, limits(1, 4)).unwrap();
    accepted(&mut queue, frame(key, 0, 2, 2));
    queue.watermark(key, ns(0)).unwrap();
    assert!(matches!(
        queue
            .push_completed(frame(session(4), 10, 3, 3), ns(10), ns(10))
            .unwrap(),
        VideoFrameAdmission::Stale
    ));
    assert!(matches!(
        queue
            .push_completed(frame(key, 0, 1, 1), ns(0), ns(30))
            .unwrap(),
        VideoFrameAdmission::Stale
    ));
    selected(&mut queue, 0, 0, 2, 2);
    assert_eq!(queue.completed_through(), Some(ns(0)));
}
#[test]
fn completed_replacement_preserves_not_yet_eligible_future_storage() {
    let key = session(6);
    let mut queue = VideoFrameQueue::new(key, limits(2, 8)).unwrap();
    accepted(&mut queue, frame(key, 0, 1, 1));
    accepted(&mut queue, frame(key, 40, 2, 2));
    queue.watermark(key, ns(0)).unwrap();
    assert_eq!(
        queue
            .completed_admission_bytes(ns(10), 3, 4, ns(10), ns(40))
            .unwrap(),
        Some(8)
    );
    assert!(matches!(
        queue
            .push_completed(frame(key, 10, 3, 3), ns(10), ns(40))
            .unwrap(),
        VideoFrameAdmission::Accepted
    ));
    selected(&mut queue, 10, 10, 3, 3);
    assert_eq!(queue.buffered_len(), 2);
    assert_eq!(queue.completed_through(), Some(ns(10)));
    queue.watermark(key, ns(40)).unwrap();
    selected(&mut queue, 40, 40, 2, 2);
}
