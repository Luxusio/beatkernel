//! Recorded presentation from actual ASIO blocks and supplied host upper bounds.
//! This owner-side queue does not infer native positions or physical precision.
use beatkernel::time::{ClockDomainId, ClockPoint, Timestamp};
use beatkernel_platform::audio::asio::AsioPresentationObservation;
use std::collections::VecDeque;

/// Retains future actual blocks until a fresh host reading reaches their upper
/// presentation bound. Paused blocks still advance this physical output grid;
/// the shared replay pause owner maps it to logical playback separately.
pub struct AsioReplayPresentation {
    origin: ClockPoint,
    host: ClockDomainId,
    rate: u32,
    capacity: usize,
    pending: VecDeque<(ClockPoint, ClockPoint)>,
    last: Option<AsioPresentationObservation>,
    last_now: Option<ClockPoint>,
    presented: Option<ClockPoint>,
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::audio::{
        AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, RenderReport, SampleBank,
        command_queue,
    };
    use beatkernel_platform::audio::asio::MultimediaHostInterval;
    fn output(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn host(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn model(capacity: usize) -> AsioReplayPresentation {
        AsioReplayPresentation::new(output(0), ClockDomainId(1), 1000, capacity).unwrap()
    }
    fn mixer() -> (Mixer, beatkernel::audio::CommandProducer) {
        let format = AudioFormat::new(1000, 1).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(1024, 1024, 2).unwrap()).unwrap();
        let (producer, consumer) = command_queue(8).unwrap();
        (
            Mixer::new(
                MixerConfig::new(
                    format,
                    ClockDomainId(2),
                    Timestamp::ZERO,
                    AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
                ),
                bank,
                consumer,
            )
            .unwrap(),
            producer,
        )
    }
    fn observation(report: RenderReport) -> AsioPresentationObservation {
        let ns = i64::try_from(report.start_frame).unwrap() * 1_000_000;
        AsioPresentationObservation::from_render(
            report,
            1000,
            MultimediaHostInterval {
                before: host(ns),
                after: host(ns),
            },
            10,
            0,
            output(0),
        )
        .unwrap()
    }
    fn same(actual: &AsioReplayPresentation, before: &AsioReplayPresentation) {
        assert_eq!(actual.pending, before.pending);
        assert_eq!(actual.last, before.last);
        assert_eq!(actual.last_now, before.last_now);
        assert_eq!(actual.presented, before.presented);
    }
    fn snapshot(value: &AsioReplayPresentation) -> AsioReplayPresentation {
        AsioReplayPresentation {
            origin: value.origin,
            host: value.host,
            rate: value.rate,
            capacity: value.capacity,
            pending: value.pending.clone(),
            last: value.last,
            last_now: value.last_now,
            presented: value.presented,
        }
    }
    #[test]
    fn constantly_newer_future_blocks_do_not_starve_matured_prefix() {
        let (mut mixer, _producer) = mixer();
        let mut model = model(8);
        for i in 0..12 {
            let report = mixer.render(&mut [0.0; 2]).unwrap();
            let now_ns = i * 2_000_000;
            let presented = model
                .observe(Some(observation(report)), host(now_ns))
                .unwrap();
            assert_eq!(
                presented,
                if i < 5 {
                    None
                } else {
                    Some(output((i - 5) * 2_000_000))
                }
            );
            assert_eq!(model.pending.len(), ((i + 1) as usize).min(5));
        }
        assert_eq!(
            model.observe(None, host(40_000_000)).unwrap(),
            Some(output(22_000_000))
        );
        assert!(model.pending.is_empty());
        assert_eq!(
            model.observe(None, host(100_000_000)).unwrap(),
            Some(output(22_000_000))
        );
    }
    #[test]
    fn exact_upper_duplicates_anchor_refresh_missing_and_plateau() {
        let (mut mixer, _producer) = mixer();
        let mut model = model(2);
        let first = observation(mixer.render(&mut [0.0; 2]).unwrap());
        assert_eq!(model.observe(Some(first), host(0)).unwrap(), None);
        let mut refreshed = first;
        refreshed.host = MultimediaHostInterval {
            before: host(-100),
            after: host(100_000_000),
        };
        assert_eq!(
            model.observe(Some(refreshed), host(9_999_999)).unwrap(),
            None
        );
        assert_eq!(model.pending.len(), 1);
        assert_eq!(
            model.observe(None, host(10_000_000)).unwrap(),
            Some(output(0))
        );
        assert_eq!(
            model.observe(Some(refreshed), host(10_000_000)).unwrap(),
            Some(output(0))
        );
        assert!(model.pending.is_empty());
        let second_report = mixer.render(&mut [0.0; 2]).unwrap();
        let second = AsioPresentationObservation::from_render(
            second_report,
            1000,
            MultimediaHostInterval {
                before: host(0),
                after: host(0),
            },
            10,
            0,
            output(0),
        )
        .unwrap();
        assert_eq!(
            model.observe(Some(second), host(10_000_000)).unwrap(),
            Some(output(2_000_000))
        );
    }
    #[test]
    fn capacity_credits_maturity_and_all_failures_are_atomic() {
        let (mut mixer, _producer) = mixer();
        let mut model = model(1);
        let first = observation(mixer.render(&mut [0.0; 2]).unwrap());
        model.observe(Some(first), host(0)).unwrap();
        let second = observation(mixer.render(&mut [0.0; 2]).unwrap());
        let before = snapshot(&model);
        assert!(model.observe(Some(second), host(0)).is_err());
        same(&model, &before);
        let mut rate = second;
        rate.sample_rate = 999;
        let mut origin = second;
        origin.output_origin = output(1);
        let mut grid = second;
        grid.output = output(2_000_001);
        let mut domain = second;
        domain.host.before.domain = ClockDomainId(9);
        let mut upper_domain = second;
        upper_domain.host.after.domain = ClockDomainId(9);
        let mut interval = second;
        interval.host.before = host(12_000_001);
        let mut counter = second;
        counter.render.counters.rendered_frames -= 1;
        let mut paused = second;
        paused.render.paused = true;
        let mut playback = second;
        playback.render.playback_start_frame = 1;
        let mut extent = second;
        extent.render.playback_frames = 1;
        let mut marker = second;
        marker.render.playback_end_physical_frame = Some(4);
        let mut empty = second;
        empty.render.frames = 0;
        let mut upper_regression = second;
        upper_regression.host = MultimediaHostInterval {
            before: host(0),
            after: host(9_999_999),
        };
        let mut changed_same_block = first;
        changed_same_block.render.active_voices += 1;
        for bad in [
            rate,
            origin,
            grid,
            domain,
            upper_domain,
            interval,
            counter,
            paused,
            playback,
            extent,
            marker,
            empty,
            upper_regression,
            changed_same_block,
        ] {
            assert!(model.observe(Some(bad), host(10_000_000)).is_err());
            same(&model, &before); // Even the matured prefix is untouched.
        }
        assert!(model.observe(None, output(10_000_000)).is_err());
        same(&model, &before);
        assert_eq!(
            model.observe(Some(second), host(10_000_000)).unwrap(),
            Some(output(0))
        );
        assert_eq!(model.pending.len(), 1);
        let before = snapshot(&model);
        assert!(model.observe(None, host(9_999_999)).is_err());
        same(&model, &before);
        assert!(model.observe(Some(first), host(10_000_000)).is_err());
        same(&model, &before);
    }
    #[test]
    fn configuration_and_output_extent_are_checked() {
        for (origin, host_domain, rate, capacity) in [
            (output(0), ClockDomainId(2), 1000, 1),
            (output(0), ClockDomainId(1), 0, 1),
            (output(0), ClockDomainId(1), 1_000_000_001, 1),
            (output(0), ClockDomainId(1), 1000, 0),
            (output(0), ClockDomainId(1), 1000, 4097),
        ] {
            assert!(AsioReplayPresentation::new(origin, host_domain, rate, capacity).is_err());
        }
        assert!(
            AsioReplayPresentation::new(output(0), ClockDomainId(1), 1_000_000_000, 4096).is_ok()
        );
        let (mut mixer, _producer) = mixer();
        let report = mixer.render(&mut [0.0; 2]).unwrap();
        let mut model =
            AsioReplayPresentation::new(output(i64::MAX), ClockDomainId(1), 1000, 1).unwrap();
        let value = AsioPresentationObservation::from_render(
            report,
            1000,
            MultimediaHostInterval {
                before: host(0),
                after: host(0),
            },
            0,
            0,
            output(i64::MAX),
        )
        .unwrap();
        let before = snapshot(&model);
        assert!(model.observe(Some(value), host(0)).is_err());
        same(&model, &before);
    }
    #[test]
    fn actual_replay_completion_waits_for_mature_presentation_not_prepared_pcm() {
        use crate::{bgm::BgmFeedReport, completion::ReplayCompletion};
        let (mut mixer, _producer) = mixer();
        let mut model = model(4);
        let mut completion = ReplayCompletion::new(ClockDomainId(2), 1000);
        let first = mixer.render(&mut [0.0; 2]).unwrap();
        let presented = model.observe(Some(observation(first)), host(0)).unwrap();
        assert!(
            !completion
                .observe(true, BgmFeedReport::default(), Some(first), presented)
                .unwrap()
        );
        let idle = mixer.render(&mut [0.0; 2]).unwrap();
        let presented = model
            .observe(Some(observation(idle)), host(2_000_000))
            .unwrap();
        assert!(
            !completion
                .observe(true, BgmFeedReport::default(), Some(idle), presented)
                .unwrap()
        );
        let crossing = mixer.render(&mut [0.0; 2]).unwrap();
        let presented = model
            .observe(Some(observation(crossing)), host(13_999_999))
            .unwrap();
        assert_eq!(presented, Some(output(2_000_000)));
        assert!(
            !completion
                .observe(true, BgmFeedReport::default(), Some(crossing), presented)
                .unwrap()
        );
        let presented = model.observe(None, host(14_000_000)).unwrap();
        assert_eq!(presented, Some(output(4_000_000)));
        assert!(
            completion
                .observe(true, BgmFeedReport::default(), Some(crossing), presented)
                .unwrap()
        );
    }

    #[test]
    fn paused_blocks_use_original_deadlines_capacity_credit_and_physical_positions() {
        let (mut mixer, mut producer) = mixer();
        let mut model = model(1);
        let running = observation(mixer.render(&mut [0.0; 2]).unwrap());
        assert_eq!(model.observe(Some(running), host(0)).unwrap(), None);
        producer.request_pause(true);
        let mut silence = [1.0; 2];
        let paused = observation(mixer.render(&mut silence).unwrap());
        assert_eq!(silence, [0.0; 2]);
        assert_eq!(paused.render.playback_start_frame, 2);
        assert_eq!(paused.render.playback_frames, 0);
        let before = snapshot(&model);
        assert!(model.observe(Some(paused), host(9_999_999)).is_err());
        same(&model, &before);
        assert_eq!(
            model.observe(Some(paused), host(10_000_000)).unwrap(),
            Some(output(0))
        );
        let before = snapshot(&model);
        let mut changed_same_block = paused;
        changed_same_block.render.active_voices += 1;
        assert!(
            model
                .observe(Some(changed_same_block), host(12_000_000))
                .is_err()
        );
        same(&model, &before);
        let mut refreshed = paused;
        refreshed.host = MultimediaHostInterval {
            before: host(-100),
            after: host(100_000_000),
        };
        assert_eq!(
            model.observe(Some(refreshed), host(11_999_999)).unwrap(),
            Some(output(0))
        );
        assert_eq!(model.last, Some(paused));
        assert_eq!(
            model.pending.front(),
            Some(&(output(2_000_000), host(12_000_000)))
        );
        assert_eq!(
            model.observe(None, host(12_000_000)).unwrap(),
            Some(output(2_000_000))
        );
        let still_paused = observation(mixer.render(&mut silence).unwrap());
        assert_eq!(still_paused.render.playback_start_frame, 2);
        assert_eq!(
            model.observe(Some(still_paused), host(14_000_000)).unwrap(),
            Some(output(4_000_000))
        );
        producer.request_pause(false);
        let resumed = observation(mixer.render(&mut silence).unwrap());
        assert_eq!(resumed.render.playback_start_frame, 2);
        assert_eq!(resumed.render.playback_frames, 2);
        assert_eq!(
            model.observe(Some(resumed), host(16_000_000)).unwrap(),
            Some(output(6_000_000))
        );
    }

    #[test]
    fn skipped_manual_transitions_preserve_monotonic_playback_and_frozen_paused_reports() {
        let (mut mixer, mut producer) = mixer();
        let mut model = model(4);
        let first = observation(mixer.render(&mut [0.0; 2]).unwrap());
        model.observe(Some(first), host(0)).unwrap();
        producer.request_pause(true);
        mixer.render(&mut [0.0; 2]).unwrap(); // Physical 2, playback 2.
        mixer.render(&mut [0.0; 2]).unwrap(); // Physical 4, playback 2.
        producer.request_pause(false);
        mixer.render(&mut [0.0; 2]).unwrap(); // Physical 6, playback 2.
        let active = observation(mixer.render(&mut [0.0; 2]).unwrap());
        assert_eq!(
            (
                active.render.start_frame,
                active.render.playback_start_frame
            ),
            (8, 4)
        );
        assert_eq!(
            model.observe(Some(active), host(18_000_000)).unwrap(),
            Some(output(8_000_000))
        );
        producer.request_pause(true);
        let paused = observation(mixer.render(&mut [0.0; 2]).unwrap());
        assert_eq!(
            (
                paused.render.start_frame,
                paused.render.playback_start_frame
            ),
            (10, 6)
        );
        model.observe(Some(paused), host(20_000_000)).unwrap();
        mixer.render(&mut [0.0; 2]).unwrap();
        let still_paused = observation(mixer.render(&mut [0.0; 2]).unwrap());
        assert_eq!(
            (
                still_paused.render.start_frame,
                still_paused.render.playback_start_frame
            ),
            (14, 6)
        );
        assert_eq!(
            model.observe(Some(still_paused), host(24_000_000)).unwrap(),
            Some(output(14_000_000))
        );
        producer.request_pause(false);
        mixer.render(&mut [0.0; 2]).unwrap();
        let resumed = observation(mixer.render(&mut [0.0; 2]).unwrap());
        assert_eq!(
            (
                resumed.render.start_frame,
                resumed.render.playback_start_frame
            ),
            (18, 8)
        );
        assert_eq!(
            model.observe(Some(resumed), host(28_000_000)).unwrap(),
            Some(output(18_000_000))
        );
    }

    #[test]
    fn malformed_paused_or_resumed_grids_preserve_even_a_mature_pending_prefix() {
        let (mut mixer, mut producer) = mixer();
        let mut model = model(4);
        let first = observation(mixer.render(&mut [0.0; 2]).unwrap());
        model.observe(Some(first), host(0)).unwrap();
        producer.request_pause(true);
        let paused = observation(mixer.render(&mut [0.0; 2]).unwrap());
        model.observe(Some(paused), host(2_000_000)).unwrap();
        let adjacent = observation(mixer.render(&mut [0.0; 2]).unwrap());
        let skipped = observation(mixer.render(&mut [0.0; 2]).unwrap());
        assert_eq!(adjacent.render.start_frame, 4);
        assert_eq!(skipped.render.start_frame, 6);
        let mut active_extent = adjacent;
        active_extent.render.paused = false; // Still has zero playback frames.
        let mut paused_extent = adjacent;
        paused_extent.render.playback_frames = 1;
        let mut ahead = adjacent;
        ahead.render.playback_start_frame = 5;
        let mut adjacent_jump = adjacent;
        adjacent_jump.render.playback_start_frame = 3;
        let mut frozen_jump = skipped;
        frozen_jump.render.playback_start_frame = 3; // Gap can grow, frozen song cannot.
        let mut lost_gap = skipped;
        lost_gap.render.paused = false;
        lost_gap.render.playback_frames = 2;
        lost_gap.render.playback_start_frame = 5; // Cannot recover already silent frames.
        let mut regressed = skipped;
        regressed.render.paused = false;
        regressed.render.playback_frames = 2;
        regressed.render.playback_start_frame = 1;
        let mut terminal = adjacent;
        terminal.render.playback_end_physical_frame = Some(4);
        let mut counter = adjacent;
        counter.render.counters.rendered_frames -= 1;
        let mut overflow = adjacent;
        overflow.render.paused = false;
        overflow.render.playback_start_frame = u64::MAX;
        overflow.render.playback_frames = 2;
        let before = snapshot(&model);
        for bad in [
            active_extent,
            paused_extent,
            ahead,
            adjacent_jump,
            frozen_jump,
            lost_gap,
            regressed,
            terminal,
            counter,
            overflow,
        ] {
            assert!(model.observe(Some(bad), host(20_000_000)).is_err());
            same(&model, &before);
        }
        assert_eq!(
            model.observe(Some(skipped), host(20_000_000)).unwrap(),
            Some(output(6_000_000))
        );
    }

    #[test]
    fn shared_interval_pause_keeps_queued_physical_points_from_rewinding_replay_song() {
        use crate::{
            native_start::StartInterval,
            playback_pause::{PauseIntervalObservation, PausePhase},
            replay_pause::ReplayPause,
        };
        use beatkernel::time::Duration;
        let pause_observation = |value: AsioPresentationObservation| PauseIntervalObservation {
            output_origin: value.output_origin,
            sample_rate: value.sample_rate,
            render: value.render,
            clock: StartInterval::new(value.output, value.host.before, value.host.after).unwrap(),
        };
        let (mut mixer, mut producer) = mixer();
        let mut model = model(4);
        let mut pause = ReplayPause::new(
            output(0),
            ClockDomainId(1),
            1000,
            Timestamp::from_nanos(50_000_000),
            Duration::from_nanos(3_000_000),
        )
        .unwrap();
        let first = observation(mixer.render(&mut [0.0; 2]).unwrap());
        model.observe(Some(first), host(0)).unwrap();
        assert!(
            pause
                .request_interval(true, pause_observation(first))
                .unwrap()
        );
        producer.request_pause(true);
        let paused = observation(mixer.render(&mut [0.0; 2]).unwrap());
        model.observe(Some(paused), host(2_000_000)).unwrap();
        assert!(
            pause
                .observe_interval(Some(pause_observation(paused)), host(11_999_999))
                .unwrap()
                .is_none()
        );
        assert_eq!(pause.phase(), PausePhase::Pausing);
        let frozen = pause
            .observe_interval(None, host(12_000_000))
            .unwrap()
            .unwrap();
        assert!(frozen.paused);
        assert_eq!(frozen.song, Timestamp::from_nanos(49_000_000));
        let point = model.observe(None, host(12_000_000)).unwrap().unwrap();
        assert_eq!(point, output(2_000_000));
        assert_eq!(pause.presentation_song(point).unwrap(), None);

        let still_paused = observation(mixer.render(&mut [0.0; 2]).unwrap());
        model.observe(Some(still_paused), host(12_000_000)).unwrap();
        pause
            .observe_interval(Some(pause_observation(still_paused)), host(12_000_000))
            .unwrap();
        assert!(
            pause
                .request_interval(false, pause_observation(still_paused))
                .unwrap()
        );
        producer.request_pause(false);
        let resumed = observation(mixer.render(&mut [0.0; 2]).unwrap());
        assert_eq!(
            (
                resumed.render.start_frame,
                resumed.render.playback_start_frame
            ),
            (6, 2)
        );
        assert!(
            pause
                .observe_interval(Some(pause_observation(resumed)), host(15_999_999))
                .unwrap()
                .is_none()
        );
        let old_point = model.observe(None, host(15_999_999)).unwrap().unwrap();
        assert_eq!(old_point, output(4_000_000));
        assert_eq!(pause.presentation_song(old_point).unwrap(), None);
        let running = pause
            .observe_interval(None, host(16_000_000))
            .unwrap()
            .unwrap();
        assert!(!running.paused);
        assert_eq!(running.song, Timestamp::from_nanos(49_000_000));
        assert_eq!(pause.phase(), PausePhase::Running);
        assert_eq!(pause.presentation_song(old_point).unwrap(), None);
        let point = model
            .observe(Some(resumed), host(16_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(point, output(6_000_000));
        assert_eq!(
            pause.presentation_song(point).unwrap(),
            Some(Timestamp::from_nanos(49_000_000))
        );
        let later = observation(mixer.render(&mut [0.0; 2]).unwrap());
        let point = model
            .observe(Some(later), host(18_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(point, output(8_000_000));
        assert_eq!(
            pause.presentation_song(point).unwrap(),
            Some(Timestamp::from_nanos(51_000_000))
        );
    }
}
impl AsioReplayPresentation {
    pub fn new(
        origin: ClockPoint,
        host: ClockDomainId,
        rate: u32,
        capacity: usize,
    ) -> Result<Self, String> {
        if origin.domain == host
            || !(1..=1_000_000_000).contains(&rate)
            || !(1..=4096).contains(&capacity)
        {
            return Err(
                "ASIO replay requires distinct clocks, rate 1..1GHz and capacity 1..4096".into(),
            );
        }
        let mut pending = VecDeque::new();
        pending
            .try_reserve_exact(capacity)
            .map_err(|_| "ASIO replay presentation queue allocation failed")?;
        Ok(Self {
            origin,
            host,
            rate,
            capacity,
            pending,
            last: None,
            last_now: None,
            presented: None,
        })
    }
    fn point(&self, frame: u64) -> Result<ClockPoint, String> {
        let nanos = i128::from(self.origin.timestamp.as_nanos())
            + i128::from(frame) * 1_000_000_000 / i128::from(self.rate);
        Ok(ClockPoint {
            domain: self.origin.domain,
            timestamp: Timestamp::from_nanos(
                i64::try_from(nanos).map_err(|_| "ASIO replay output grid timestamp overflow")?,
            ),
        })
    }
    fn validate(&self, observation: AsioPresentationObservation) -> Result<bool, String> {
        let report = observation.render;
        let end = report
            .start_frame
            .checked_add(
                u64::try_from(report.frames).map_err(|_| "ASIO replay block extent overflow")?,
            )
            .ok_or("ASIO replay physical grid overflow")?;
        let playback_end = report
            .playback_start_frame
            .checked_add(
                u64::try_from(report.playback_frames)
                    .map_err(|_| "ASIO replay playback extent overflow")?,
            )
            .ok_or("ASIO replay playback grid overflow")?;
        if observation.sample_rate != self.rate
            || observation.output_origin != self.origin
            || observation.output != self.point(report.start_frame)?
            || report.frames == 0
            || report.playback_start_frame > report.start_frame
            || (report.paused && report.playback_frames != 0)
            || (!report.paused && report.playback_frames != report.frames)
            || report.playback_end_physical_frame.is_some()
            || report.counters.rendered_frames != end
            || observation.host.before.domain != self.host
            || observation.host.after.domain != self.host
            || observation.host.before.timestamp > observation.host.after.timestamp
        {
            return Err(
                "ASIO replay observation has inconsistent configuration, clocks or block grid"
                    .into(),
            );
        }
        self.point(end)?;
        self.point(playback_end)?;
        if let Some(old) = self.last {
            // Anchor refresh may change a repeated block's interval; it must not
            // replace the originally admitted upper or occupy another slot.
            if report == old.render {
                return Ok(false);
            }
            if report.start_frame < old.render.counters.rendered_frames
                || observation.output.timestamp <= old.output.timestamp
                || observation.host.after.timestamp < old.host.after.timestamp
            {
                return Err("ASIO replay actual block or host upper frontier regressed".into());
            }
            let old_playback_end =
                old.render.playback_start_frame + old.render.playback_frames as u64; // Validated on admission.
            let old_gap = old.render.counters.rendered_frames - old_playback_end;
            let gap = report.start_frame - report.playback_start_frame;
            if report.playback_start_frame < old_playback_end
                || gap < old_gap
                || (report.start_frame == old.render.counters.rendered_frames
                    && report.playback_start_frame != old_playback_end)
                || (report.paused
                    && old.render.paused
                    && report.playback_start_frame != old_playback_end)
            {
                return Err("ASIO replay playback or pause gap is inconsistent".into());
            }
        }
        Ok(true)
    }
    /// Errors preserve all evidence. Capacity credits the already matured prefix
    /// before admitting a new block; future blocks cannot starve older maturity.
    /// A valid poll returns the last matured point, or None before any maturity.
    pub fn observe(
        &mut self,
        observation: Option<AsioPresentationObservation>,
        now: ClockPoint,
    ) -> Result<Option<ClockPoint>, String> {
        if now.domain != self.host
            || self
                .last_now
                .is_some_and(|old| now.timestamp < old.timestamp)
        {
            return Err("ASIO replay fresh host clock has wrong domain or regressed".into());
        }
        let new = match observation {
            Some(value) => self.validate(value)?,
            None => false,
        };
        let matured = self
            .pending
            .iter()
            .take_while(|(_, upper)| upper.timestamp <= now.timestamp)
            .count();
        let future =
            new && observation.is_some_and(|value| value.host.after.timestamp > now.timestamp);
        if self.pending.len() - matured + usize::from(future) > self.capacity {
            return Err("ASIO replay presentation queue capacity exceeded".into());
        }
        // Every fallible check precedes these mutations. The preallocated queue
        // cannot grow beyond its caller-selected bound.
        for _ in 0..matured {
            self.presented = Some(self.pending.pop_front().expect("checked matured prefix").0);
        }
        if new {
            let value = observation.expect("validated new observation");
            if future {
                self.pending.push_back((value.output, value.host.after));
            } else {
                self.presented = Some(value.output);
            }
            self.last = Some(value);
        }
        self.last_now = Some(now);
        Ok(self.presented)
    }
}
