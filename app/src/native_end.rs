//! Retained render endpoint plus actual native presentation; no device or wall clock.
use beatkernel::{
    audio::RenderReport,
    time::{ClockDomainId, ClockPair, ClockPoint, Timestamp},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndError(pub &'static str);
impl std::fmt::Display for EndError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for EndError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndBoundary {
    pub host: ClockPoint,
    pub output: ClockPoint,
    pub physical_frame: u64,
    pub playback_frame: u64,
}

/// One fresh finite session. Host interpolation has Unknown physical accuracy.
/// An actual lower clock bracket must be observed before presentation crosses.
#[derive(Clone, Debug)]
pub struct NativeEnd {
    origin: ClockPoint,
    host: ClockDomainId,
    rate: u32,
    end: u64,
    lower: Option<ClockPair>,
    last_pair: Option<ClockPair>,
    last_report: Option<RenderReport>,
    physical: Option<u64>,
    emitted: bool,
    deferred_boundary: Option<EndBoundary>,
    start_frame: u64,
    start_configured: bool,
}
impl NativeEnd {
    pub fn new(
        origin: ClockPoint,
        host: ClockDomainId,
        rate: u32,
        end: u64,
    ) -> Result<Self, EndError> {
        if origin.domain == host || rate == 0 || rate > 1_000_000_000 {
            return Err(EndError(
                "end requires distinct clocks and a representable nonzero frame grid",
            ));
        }
        let result = Self {
            origin,
            host,
            rate,
            end,
            lower: None,
            last_pair: None,
            last_report: None,
            physical: None,
            emitted: false,
            deferred_boundary: None,
            start_frame: 0,
            start_configured: false,
        };
        result.point(end)?;
        Ok(result)
    }
    /// Preserve an original startup observation without discarding early completion.
    pub fn prime(
        &mut self,
        report: Option<RenderReport>,
        pair: ClockPair,
    ) -> Result<(), EndError> {
        if self.emitted || self.deferred_boundary.is_some() {
            return Err(EndError("finite startup boundary is already committed"));
        }
        let mut next = self.clone();
        next.deferred_boundary = next.observe(report, pair)?;
        *self = next;
        Ok(())
    }

    /// Preserve full ASIO startup evidence and defer any actual boundary delivery.
    pub fn prime_asio(
        &mut self,
        observation: beatkernel_platform::audio::asio::AsioPresentationObservation,
    ) -> Result<(), EndError> {
        if self.emitted || self.deferred_boundary.is_some() {
            return Err(EndError("finite startup boundary is already committed"));
        }
        let mut next = self.clone();
        next.deferred_boundary = next.observe_asio(observation)?;
        *self = next;
        Ok(())
    }

    /// Stages a fresh stream relation while retaining the original finite frame grid.
    /// Reached endpoints and regressed actual render/host history refuse atomically.
    pub fn restart_for_output(
        &self,
        basis: beatkernel::audio::OutputFrameBasis,
        report: RenderReport,
        pair: ClockPair,
    ) -> Result<Self, EndError> {
        self.validate_output_restart(basis, report, pair)?;
        let mut next = self.restart_relation();
        next.observe(Some(report), pair)?;
        Ok(next)
    }

    /// Stages an ASIO stream relation with its original render and HOST upper frontier.
    /// The active owner remains unchanged if any configuration or chronology check fails.
    pub fn restart_for_output_asio(
        &self,
        basis: beatkernel::audio::OutputFrameBasis,
        observation: beatkernel_platform::audio::asio::AsioPresentationObservation,
    ) -> Result<Self, EndError> {
        self.validate_output_restart(
            basis,
            observation.render,
            ClockPair {
                source: observation.output,
                target: observation.host.after,
            },
        )?;
        let mut next = self.restart_relation();
        next.observe_asio(observation)?;
        Ok(next)
    }

    fn restart_relation(&self) -> Self {
        let mut next = self.clone();
        next.lower = None;
        next.last_pair = None;
        next.last_report = None;
        next
    }

    fn validate_output_restart(
        &self,
        basis: beatkernel::audio::OutputFrameBasis,
        report: RenderReport,
        pair: ClockPair,
    ) -> Result<(), EndError> {
        if self.physical.is_some()
            || self.emitted
            || report.playback_end_physical_frame.is_some()
            || basis.origin() != self.origin
            || basis.sample_rate() != self.rate
            || report.start_frame < basis.start_physical_frame()
            || !report.paused
            || report.frames == 0
            || report.playback_frames != 0
            || report.playback_start_frame >= self.end
        {
            return Err(EndError(
                "finite output replacement changed or reached endpoint",
            ));
        }
        self.check_pair(pair)?;
        if pair.source.timestamp
            < basis
                .point_at_stream_frame(0)
                .map_err(|_| EndError("replacement frame timestamp overflow"))?
                .timestamp
        {
            return Err(EndError("replacement pair precedes new stream"));
        }
        self.check_report(report)?;
        Ok(())
    }
    /// Configures an immutable initial physical-frame prefix before observation.
    pub fn with_start_frame(mut self, frame: u64) -> Result<Self, EndError> {
        if self.start_configured || self.last_pair.is_some() || self.last_report.is_some() {
            return Err(EndError("end startup is already configured or observed"));
        }
        self.point(
            frame
                .checked_add(self.end)
                .ok_or(EndError("startup endpoint overflow"))?,
        )?;
        self.start_frame = frame;
        self.start_configured = true;
        Ok(self)
    }
    fn startup_prefix(&self, report: RenderReport) -> Result<u64, EndError> {
        Ok(u64::try_from(report.frames)
            .map_err(|_| EndError("physical extent overflow"))?
            .min(self.start_frame.saturating_sub(report.start_frame)))
    }
    fn report_gap(&self, report: RenderReport) -> Result<u64, EndError> {
        report
            .start_frame
            .checked_add(self.startup_prefix(report)?)
            .and_then(|frame| frame.checked_sub(report.playback_start_frame))
            .ok_or(EndError("playback exceeds physical startup grid"))
    }
    fn point(&self, frame: u64) -> Result<ClockPoint, EndError> {
        let nanos = i128::from(frame) * 1_000_000_000 / i128::from(self.rate);
        let timestamp = i128::from(self.origin.timestamp.as_nanos())
            .checked_add(nanos)
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(EndError("end frame timestamp overflow"))?;
        Ok(ClockPoint {
            domain: self.origin.domain,
            timestamp: Timestamp::from_nanos(timestamp),
        })
    }
    fn check_pair(&self, pair: ClockPair) -> Result<(), EndError> {
        if pair.source.domain != self.origin.domain
            || pair.target.domain != self.host
            || pair.source.timestamp < self.origin.timestamp
        {
            return Err(EndError(
                "end clock pair has wrong domain or precedes output origin",
            ));
        }
        if self.last_pair.is_some_and(|old| {
            pair.source.timestamp < old.source.timestamp
                || pair.target.timestamp < old.target.timestamp
        }) {
            return Err(EndError("end clock pair regressed"));
        }
        Ok(())
    }
    fn grids(&self, report: RenderReport) -> Result<(u64, u64), EndError> {
        let physical = report
            .start_frame
            .checked_add(
                u64::try_from(report.frames).map_err(|_| EndError("physical extent overflow"))?,
            )
            .ok_or(EndError("physical grid overflow"))?;
        let playback = report
            .playback_start_frame
            .checked_add(
                u64::try_from(report.playback_frames)
                    .map_err(|_| EndError("playback extent overflow"))?,
            )
            .ok_or(EndError("playback grid overflow"))?;
        let prefix = self.startup_prefix(report)?;
        let frames =
            u64::try_from(report.frames).map_err(|_| EndError("physical extent overflow"))?;
        let startup_held = self.start_frame > 0
            && report.start_frame <= self.start_frame
            && physical <= self.start_frame;
        let gap = self.report_gap(report)?;
        if report.playback_start_frame > report.start_frame
            || report.playback_frames as u64 > frames - prefix
            || (!report.paused && report.playback_frames as u64 != frames - prefix)
            || (report.start_frame < self.start_frame && report.playback_start_frame != 0)
            || (startup_held && (playback != 0 || (frames > 0 && !report.paused)))
            || (!startup_held && gap < self.start_frame)
            || (self.start_configured
                && self.start_frame > 0
                && report.playback_start_frame == 0
                && report.playback_frames > 0
                && gap != self.start_frame)
            || (prefix > 0
                && prefix < frames
                && report.paused
                && report.playback_end_physical_frame.is_none())
            || report.counters.rendered_frames != physical
        {
            return Err(EndError("end render grids or counters are inconsistent"));
        }
        Ok((physical, playback))
    }
    fn check_report(&self, report: RenderReport) -> Result<Option<u64>, EndError> {
        let (physical, playback) = self.grids(report)?;
        self.point(physical)?;
        if playback > self.end {
            return Err(EndError("render exceeded configured playback end"));
        }
        if let Some(old) = self.last_report {
            let (old_physical, old_playback) = self.grids(old)?;
            if report.start_frame < old.start_frame
                || physical < old_physical
                || playback < old_playback
                || self.report_gap(report)? < self.report_gap(old)?
            {
                return Err(EndError("end render grid regressed"));
            }
        }
        let prefix = self.startup_prefix(report)?;
        let active_start = report
            .start_frame
            .checked_add(prefix)
            .ok_or(EndError("startup prefix overflow"))?;
        let earliest_end = self
            .start_frame
            .checked_add(self.end)
            .ok_or(EndError("startup endpoint overflow"))?;
        let startup_held = self.start_frame > 0
            && report.start_frame <= self.start_frame
            && physical <= self.start_frame;
        let marker = report.playback_end_physical_frame;
        if self.physical.is_some() && marker != self.physical {
            return Err(EndError(
                "retained physical endpoint changed or disappeared",
            ));
        }
        if let Some(frame) = marker {
            if !report.paused
                || playback != self.end
                || frame < earliest_end
                || frame > physical
                || (report.playback_frames > 0
                    && frame != active_start + report.playback_frames as u64)
                || (report.playback_frames == 0 && frame > active_start)
            {
                return Err(EndError(
                    "physical endpoint does not match the finite playback grid",
                ));
            }
            self.point(frame)?;
        } else if report.frames > 0 && !startup_held && playback == self.end {
            return Err(EndError(
                "finite end render lacks physical endpoint evidence",
            ));
        }
        Ok(marker)
    }
    /// Acknowledges an ASIO crossing block at its conservative host upper
    /// frontier, including the observation's supplied latency/error bounds.
    /// Prepared render endpoints alone never substitute for presentation. This
    /// does not establish physical accuracy or an interpolated exact host time.
    pub fn observe_asio(
        &mut self,
        observation: beatkernel_platform::audio::asio::AsioPresentationObservation,
    ) -> Result<Option<EndBoundary>, EndError> {
        if observation.sample_rate != self.rate
            || observation.output_origin != self.origin
            || observation.output != self.point(observation.render.start_frame)?
            || observation.render.frames == 0
            || observation.host.before.domain != self.host
            || observation.host.after.domain != self.host
            || observation.host.before.timestamp > observation.host.after.timestamp
        {
            return Err(EndError(
                "ASIO end observation has inconsistent configuration, grid or host interval",
            ));
        }
        let mut next = self.clone();
        let boundary = next.observe_inner(
            Some(observation.render),
            ClockPair {
                source: observation.output,
                target: observation.host.after,
            },
            true,
        )?;
        *self = next;
        Ok(boundary)
    }
    /// Unavailable render telemetry preserves prior evidence. Invalid observations
    /// commit nothing. A boundary is emitted once after actual native crossing.
    pub fn observe(
        &mut self,
        report: Option<RenderReport>,
        pair: ClockPair,
    ) -> Result<Option<EndBoundary>, EndError> {
        let mut next = self.clone();
        let boundary = next.observe_inner(report, pair, false)?;
        *self = next;
        Ok(boundary)
    }
    fn observe_inner(
        &mut self,
        report: Option<RenderReport>,
        pair: ClockPair,
        upper_frontier: bool,
    ) -> Result<Option<EndBoundary>, EndError> {
        self.check_pair(pair)?;
        if let Some(report) = report {
            self.physical = self.check_report(report)?;
            self.last_report = Some(report);
        }
        self.lower.get_or_insert(pair);
        self.last_pair = Some(pair);
        if let Some(boundary) = self.deferred_boundary.take() {
            return Ok(Some(boundary));
        }
        let Some(frame) = self.physical else {
            return Ok(None);
        };
        let output = self.point(frame)?;
        if self.emitted {
            return Ok(None);
        }
        if pair.source.timestamp < output.timestamp {
            // Once the physical marker is known, keep the closest actual lower
            // bracket. Before that, preserve the first pair across telemetry gaps.
            self.lower = Some(pair);
            return Ok(None);
        }
        let lower = self
            .lower
            .ok_or(EndError("end lacks native lower bracket"))?;
        if lower.source.timestamp > output.timestamp {
            return Err(EndError(
                "endpoint precedes the first actual native clock observation",
            ));
        }
        let host = if upper_frontier || pair.source.timestamp == output.timestamp {
            pair.target.timestamp
        } else {
            let source_delta = i128::from(pair.source.timestamp.as_nanos())
                - i128::from(lower.source.timestamp.as_nanos());
            let host_delta = i128::from(pair.target.timestamp.as_nanos())
                - i128::from(lower.target.timestamp.as_nanos());
            if source_delta <= 0 || host_delta <= 0 {
                return Err(EndError(
                    "end interpolation requires progress in both clocks",
                ));
            }
            let offset = i128::from(output.timestamp.as_nanos())
                - i128::from(lower.source.timestamp.as_nanos());
            Timestamp::from_nanos(
                offset
                    .checked_mul(host_delta)
                    .map(|value| value / source_delta)
                    .and_then(|value| {
                        value.checked_add(i128::from(lower.target.timestamp.as_nanos()))
                    })
                    .and_then(|value| i64::try_from(value).ok())
                    .ok_or(EndError("end host interpolation overflow"))?,
            )
        };
        self.emitted = true;
        Ok(Some(EndBoundary {
            host: ClockPoint {
                domain: self.host,
                timestamp: host,
            },
            output,
            physical_frame: frame,
            playback_frame: self.end,
        }))
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::audio::{
        AudioCommand, AudioCounters, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits,
        PcmSample, SampleBank, SampleId, VoiceId, command_queue,
    };
    fn output(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn pair(ns: i64) -> ClockPair {
        ClockPair {
            source: output(ns),
            target: ClockPoint {
                domain: ClockDomainId(1),
                timestamp: Timestamp::from_nanos(ns + 1000),
            },
        }
    }
    fn mixer(end: u64) -> (Mixer, beatkernel::audio::CommandProducer) {
        mixer_queue(end, false)
    }
    fn mixer_queue(end: u64, gated: bool) -> (Mixer, beatkernel::audio::CommandProducer) {
        let format = AudioFormat::new(1000, 1).unwrap();
        let pcm = PcmLimits::new(1024, 1024, 2).unwrap();
        let mut bank = SampleBank::new(format, pcm).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.5; 16], pcm).unwrap(),
        )
        .unwrap();
        let (mut producer, consumer) = if gated {
            beatkernel::audio::command_queue_with_start_gate(8)
        } else {
            command_queue(8)
        }
        .unwrap();
        producer
            .try_push(AudioCommand::Play {
                at: Timestamp::ZERO,
                sample: SampleId(1),
                voice: VoiceId(1),
                gain: 1.0,
            })
            .unwrap();
        (
            Mixer::new(
                MixerConfig::new(
                    format,
                    ClockDomainId(2),
                    Timestamp::ZERO,
                    AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
                )
                .with_playback_end_frame(end),
                bank,
                consumer,
            )
            .unwrap(),
            producer,
        )
    }
    #[test]
    fn gated_first_block_endpoint_retains_actual_native_lower_bracket() {
        for end in [0, 3] {
            let (mut mixer, mut producer) = mixer_queue(end, true);
            let mut observer = NativeEnd::new(output(0), ClockDomainId(1), 1000, end)
                .unwrap()
                .with_start_frame(4)
                .unwrap();
            observer.observe(None, pair(0)).unwrap();
            let held = mixer.render(&mut [99.0; 2]).unwrap();
            assert_eq!(observer.observe(Some(held), pair(1_000_000)).unwrap(), None);
            assert_eq!(held.playback_end_physical_frame, None);
            producer.schedule_start_at(4).unwrap();
            let crossed = mixer.render(&mut [99.0; 6]).unwrap();
            assert_eq!(crossed.playback_end_physical_frame, Some(4 + end));
            assert_eq!(
                observer.observe(Some(crossed), pair(3_000_000)).unwrap(),
                None
            );
            let endpoint = (4 + end) as i64 * 1_000_000;
            let boundary = observer.observe(None, pair(endpoint)).unwrap().unwrap();
            assert_eq!(
                (boundary.physical_frame, boundary.playback_frame),
                (4 + end, end)
            );
            assert_eq!(boundary.output, output(endpoint));
            assert_eq!(
                boundary.host.timestamp,
                Timestamp::from_nanos(endpoint + 1000)
            );
            assert_eq!(observer.observe(None, pair(endpoint + 1)).unwrap(), None);
            let mut default = NativeEnd::new(output(0), ClockDomainId(1), 1000, end).unwrap();
            assert!(default.observe(Some(crossed), pair(3_000_000)).is_err());
            assert!(default.last_report.is_none());
        }
    }
    #[test]
    fn configured_start_accepts_unpaused_crossing_and_default_rejects_it() {
        let (mut mixer, mut producer) = mixer_queue(16, true);
        let mut observer = NativeEnd::new(output(0), ClockDomainId(1), 1000, 16)
            .unwrap()
            .with_start_frame(4)
            .unwrap();
        let held = mixer.render(&mut [99.0; 2]).unwrap();
        observer.observe(Some(held), pair(0)).unwrap();
        producer.schedule_start_at(4).unwrap();
        let crossing = mixer.render(&mut [99.0; 4]).unwrap();
        assert!(!crossing.paused);
        assert_eq!(crossing.playback_frames, 2);
        assert_eq!(
            observer.observe(Some(crossing), pair(3_000_000)).unwrap(),
            None
        );
        let mut default = NativeEnd::new(output(0), ClockDomainId(1), 1000, 16).unwrap();
        assert!(default.observe(Some(crossing), pair(3_000_000)).is_err());
        let before = observer.clone();
        let mut misplaced = crossing;
        misplaced.playback_start_frame = 1;
        assert!(observer.observe(Some(misplaced), pair(4_000_000)).is_err());
        assert_eq!(observer.last_report, before.last_report);
    }
    #[test]
    fn startup_end_setup_shape_and_regression_rejections_are_atomic() {
        let fresh = || NativeEnd::new(output(0), ClockDomainId(1), 1000, 3).unwrap();
        assert!(
            fresh()
                .with_start_frame(0)
                .unwrap()
                .with_start_frame(0)
                .is_err()
        );
        assert!(fresh().with_start_frame(u64::MAX).is_err());
        let mut observed = fresh();
        observed.observe(None, pair(0)).unwrap();
        assert!(observed.with_start_frame(4).is_err());
        let (mut mixer, mut producer) = mixer_queue(3, true);
        producer.schedule_start_at(4).unwrap();
        let held = mixer.render(&mut [99.0; 2]).unwrap();
        let crossed = mixer.render(&mut [99.0; 6]).unwrap();
        let mut observer = fresh().with_start_frame(4).unwrap();
        observer.observe(Some(held), pair(0)).unwrap();
        let before = observer.clone();
        let mut malformed = crossed;
        malformed.playback_frames += 1;
        assert!(observer.observe(Some(malformed), pair(1_000_000)).is_err());
        assert_eq!(observer.last_report, before.last_report);
        observer.observe(Some(crossed), pair(3_000_000)).unwrap();
        let before = observer.clone();
        assert!(observer.observe(Some(held), pair(4_000_000)).is_err());
        assert_eq!(observer.last_report, before.last_report);
        assert_eq!(observer.physical, before.physical);
    }
    fn asio(
        render: RenderReport,
        before: i64,
        after: i64,
        latency_frames: u32,
        error_ns: u64,
    ) -> beatkernel_platform::audio::asio::AsioPresentationObservation {
        use beatkernel_platform::audio::asio::{AsioPresentationObservation, MultimediaHostInterval};
        AsioPresentationObservation::from_render(
            render,
            1000,
            MultimediaHostInterval {
                before: ClockPoint {
                    domain: ClockDomainId(1),
                    timestamp: Timestamp::from_nanos(before),
                },
                after: ClockPoint {
                    domain: ClockDomainId(1),
                    timestamp: Timestamp::from_nanos(after),
                },
            },
            latency_frames,
            error_ns,
            output(0),
        )
        .unwrap()
    }
    fn evidence_unchanged(actual: &NativeEnd, before: &NativeEnd) {
        assert_eq!(actual.lower, before.lower);
        assert_eq!(actual.last_pair, before.last_pair);
        assert_eq!(actual.last_report, before.last_report);
        assert_eq!(actual.physical, before.physical);
        assert_eq!(actual.emitted, before.emitted);
    }
    #[test]
    fn asio_prepared_marker_waits_crossing_block_and_returns_latency_error_upper_frontier_once() {
        let (mut mixer, _producer) = mixer(4);
        let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 4).unwrap();
        let first = mixer.render(&mut [0.0; 2]).unwrap();
        end.observe_asio(asio(first, 10_000, 11_000, 3, 100))
            .unwrap();
        let mut samples = [9.0; 4];
        let prepared = mixer.render(&mut samples).unwrap();
        assert_eq!(samples, [0.5, 0.5, 0.0, 0.0]);
        assert_eq!(prepared.playback_end_physical_frame, Some(4));
        let prepared_observation = asio(prepared, 2_010_000, 2_011_000, 3, 100);
        assert_eq!(prepared_observation.output, output(2_000_000));
        assert_eq!(end.observe_asio(prepared_observation).unwrap(), None);
        assert!(!end.emitted);
        let silence = mixer.render(&mut [9.0; 2]).unwrap();
        assert_eq!(silence.playback_start_frame, 4);
        assert_eq!(silence.playback_frames, 0);
        assert_eq!(silence.playback_end_physical_frame, Some(4));
        let crossing = asio(silence, 6_010_000, 6_011_000, 3, 100);
        let boundary = end.observe_asio(crossing).unwrap().unwrap();
        assert_eq!(boundary.host, crossing.host.after);
        assert_eq!(boundary.host.timestamp.as_nanos(), 9_011_100);
        assert_eq!(boundary.output, output(4_000_000));
        assert_eq!((boundary.physical_frame, boundary.playback_frame), (4, 4));
        assert_ne!(boundary.host.timestamp.as_nanos(), 7_011_100); // No interpolated midpoint/endpoint estimate.
        let later = mixer.render(&mut [9.0; 2]).unwrap();
        assert_eq!(
            end.observe_asio(asio(later, 8_010_000, 8_011_000, 3, 100))
                .unwrap(),
            None
        );
        assert_eq!(mixer.playback_frame_cursor(), 4);
        assert!(end.emitted);
    }
    #[test]
    fn asio_invalid_rate_origin_grid_interval_and_regression_preserve_all_evidence() {
        let (mut mixer, _producer) = mixer(4);
        let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 4).unwrap();
        let first = mixer.render(&mut [0.0; 2]).unwrap();
        end.observe_asio(asio(first, 10_000, 11_000, 0, 0)).unwrap();
        let prepared = mixer.render(&mut [0.0; 4]).unwrap();
        let valid = asio(prepared, 2_010_000, 2_011_000, 0, 0);
        let mut wrong_rate = valid;
        wrong_rate.sample_rate = 1001;
        let mut wrong_origin = valid;
        wrong_origin.output_origin = output(1);
        let mut wrong_grid = valid;
        wrong_grid.output = output(2_000_001);
        let mut wrong_domain = valid;
        wrong_domain.host.before.domain = ClockDomainId(9);
        let mut wrong_upper_domain = valid;
        wrong_upper_domain.host.after.domain = ClockDomainId(9);
        let mut reversed = valid;
        reversed.host.before.timestamp = Timestamp::from_nanos(2_011_001);
        let mut upper_regression = valid;
        upper_regression.host.before.timestamp = Timestamp::from_nanos(0);
        upper_regression.host.after.timestamp = Timestamp::from_nanos(10_999);
        let mut wrong_counter = valid;
        wrong_counter.render.counters.rendered_frames -= 1;
        let mut empty = valid;
        empty.render.frames = 0;
        for malformed in [
            wrong_rate,
            wrong_origin,
            wrong_grid,
            wrong_domain,
            wrong_upper_domain,
            reversed,
            upper_regression,
            wrong_counter,
            empty,
        ] {
            let before = end.clone();
            assert!(end.observe_asio(malformed).is_err());
            evidence_unchanged(&end, &before);
        }
        assert_eq!(end.observe_asio(valid).unwrap(), None);
        let silent = mixer.render(&mut [0.0; 2]).unwrap();
        let crossing = asio(silent, 6_010_000, 6_011_000, 0, 0);
        let mut unseeded = NativeEnd::new(output(0), ClockDomainId(1), 1000, 4).unwrap();
        let before = unseeded.clone();
        assert!(unseeded.observe_asio(crossing).is_err());
        evidence_unchanged(&unseeded, &before);
        assert!(end.observe_asio(crossing).unwrap().is_some());
    }
    #[test]
    fn asio_crossing_accepts_coarse_upper_plateau_without_changing_generic_interpolation() {
        let (mut mixer, _producer) = mixer(4);
        let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 4).unwrap();
        let first = mixer.render(&mut [0.0; 2]).unwrap();
        assert_eq!(
            end.observe_asio(asio(first, 5_000, 10_000, 0, 0)).unwrap(),
            None
        );
        let prepared = mixer.render(&mut [0.0; 4]).unwrap();
        assert_eq!(
            end.observe_asio(asio(prepared, 9_000, 10_000, 0, 0))
                .unwrap(),
            None
        );
        let silent = mixer.render(&mut [0.0; 2]).unwrap();
        let crossing = asio(silent, 10_000, 10_000, 0, 0);
        assert_eq!(crossing.output, output(6_000_000));
        let mut generic = end.clone();
        let before = generic.clone();
        assert!(
            generic
                .observe(
                    Some(silent),
                    ClockPair {
                        source: crossing.output,
                        target: crossing.host.after,
                    }
                )
                .is_err()
        );
        evidence_unchanged(&generic, &before);
        let boundary = end.observe_asio(crossing).unwrap().unwrap();
        assert_eq!(boundary.host, crossing.host.after);
        assert_eq!(boundary.output, output(4_000_000));
        assert_eq!(mixer.playback_frame_cursor(), 4);
        assert_eq!(end.observe_asio(crossing).unwrap(), None);
    }
    #[test]
    fn asio_wide_crossing_interval_uses_supplied_upper_without_interpolation() {
        let (mut mixer, _producer) = mixer(4);
        let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 4).unwrap();
        let first = mixer.render(&mut [0.0; 2]).unwrap();
        end.observe_asio(asio(first, i64::MIN, i64::MIN, 0, 0))
            .unwrap();
        let prepared = mixer.render(&mut [0.0; 4]).unwrap();
        assert_eq!(
            end.observe_asio(asio(prepared, i64::MIN, -1, 0, 0))
                .unwrap(),
            None
        );
        let silent = mixer.render(&mut [0.0; 2]).unwrap();
        let crossing = asio(silent, i64::MIN, i64::MAX, 0, 0);
        let boundary = end.observe_asio(crossing).unwrap().unwrap();
        assert_eq!(boundary.host, crossing.host.after);
        assert_eq!(boundary.host.timestamp.as_nanos(), i64::MAX);
        assert_eq!(boundary.output, output(4_000_000));
    }
    #[test]
    fn short_resume_endpoint_survives_coalescing_and_waits_for_real_native_presentation() {
        let (mut mixer, mut producer) = mixer(4);
        let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 4).unwrap();
        let first = mixer.render(&mut [0.0]).unwrap();
        assert_eq!(first.playback_end_physical_frame, None);
        assert_eq!(end.observe(Some(first), pair(0)).unwrap(), None);
        producer.request_pause(true);
        let mut silence = [1.0; 3];
        let paused = mixer.render(&mut silence).unwrap();
        assert_eq!(silence, [0.0; 3]);
        assert_eq!(end.observe(Some(paused), pair(1_000_000)).unwrap(), None);
        producer.request_pause(false);
        let mut resumed = [1.0; 5];
        let crossing = mixer.render(&mut resumed).unwrap();
        assert_eq!(resumed, [0.5, 0.5, 0.5, 0.0, 0.0]);
        assert_eq!(crossing.playback_end_physical_frame, Some(7));
        let latest = mixer.render(&mut [1.0; 2]).unwrap();
        assert_eq!(
            (
                latest.start_frame,
                latest.playback_start_frame,
                latest.playback_frames
            ),
            (9, 4, 0)
        );
        assert_eq!(latest.playback_end_physical_frame, Some(7));
        // Owner never saw the resume/straddling report. Latest scalar evidence is sufficient.
        assert_eq!(end.observe(Some(latest), pair(6_000_000)).unwrap(), None);
        assert_eq!(end.observe(None, pair(6_500_000)).unwrap(), None);
        let boundary = end.observe(None, pair(9_000_000)).unwrap().unwrap();
        assert_eq!(boundary.physical_frame, 7);
        assert_eq!(boundary.playback_frame, 4);
        assert_eq!(boundary.output, output(7_000_000));
        assert_eq!(boundary.host.timestamp, Timestamp::from_nanos(7_001_000));
        assert_eq!(end.observe(Some(latest), pair(10_000_000)).unwrap(), None);
        producer.request_pause(false);
        let frozen = mixer.render(&mut [1.0]).unwrap();
        assert_eq!(end.observe(Some(frozen), pair(11_000_000)).unwrap(), None);
    }
    #[test]
    fn malformed_or_regressing_native_evidence_is_error_atomic() {
        let (mut mixer, _producer) = mixer(4);
        let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 4).unwrap();
        end.observe(None, pair(0)).unwrap();
        let crossing = mixer.render(&mut [0.0; 6]).unwrap();
        assert_eq!(end.observe(Some(crossing), pair(3_000_000)).unwrap(), None);
        let latest = mixer.render(&mut [0.0]).unwrap();
        let before_report = end.last_report;
        let before_pair = end.last_pair;
        for bad in [
            RenderReport {
                playback_end_physical_frame: None,
                ..latest
            },
            RenderReport {
                playback_end_physical_frame: Some(5),
                ..latest
            },
            RenderReport {
                paused: false,
                ..latest
            },
            RenderReport {
                playback_start_frame: 5,
                ..latest
            },
            RenderReport {
                counters: AudioCounters::default(),
                ..latest
            },
        ] {
            assert!(end.observe(Some(bad), pair(5_000_000)).is_err());
            assert_eq!(end.last_report, before_report);
            assert_eq!(end.last_pair, before_pair);
            assert!(!end.emitted);
        }
        assert!(end.observe(Some(latest), pair(2_000_000)).is_err());
        let mut wrong = pair(5_000_000);
        wrong.source.domain = ClockDomainId(99);
        assert!(end.observe(Some(latest), wrong).is_err());
        assert!(
            end.observe(Some(latest), pair(4_000_000))
                .unwrap()
                .is_some()
        );
    }
    #[test]
    fn extreme_host_interpolation_overflow_preserves_the_actual_lower_bracket() {
        let origin = output(i64::MIN);
        let lower = ClockPair {
            source: origin,
            target: ClockPoint {
                domain: ClockDomainId(1),
                timestamp: Timestamp::MIN,
            },
        };
        let mut end =
            NativeEnd::new(origin, ClockDomainId(1), 1_000_000_000, u64::MAX - 1).unwrap();
        end.observe(None, lower).unwrap();
        let report = RenderReport {
            start_frame: u64::MAX,
            frames: 0,
            playback_start_frame: u64::MAX - 1,
            playback_frames: 0,
            paused: true,
            playback_end_physical_frame: Some(u64::MAX - 1),
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters {
                rendered_frames: u64::MAX,
                ..AudioCounters::default()
            },
        };
        let upper = ClockPair {
            source: output(i64::MAX),
            target: ClockPoint {
                domain: ClockDomainId(1),
                timestamp: Timestamp::MAX,
            },
        };
        assert_eq!(
            end.observe(Some(report), upper),
            Err(EndError("end host interpolation overflow"))
        );
        assert_eq!(end.last_pair, Some(lower));
        assert!(end.last_report.is_none() && end.physical.is_none() && !end.emitted);
    }

    #[test]
    fn zero_end_needs_valid_render_and_late_initial_clock_cannot_invent_a_bracket() {
        let (mut mixer, _producer) = mixer(0);
        let mut end = NativeEnd::new(output(0), ClockDomainId(1), 1000, 0).unwrap();
        let empty = mixer.render(&mut []).unwrap();
        assert_eq!(empty.playback_end_physical_frame, None);
        assert_eq!(end.observe(Some(empty), pair(0)).unwrap(), None);
        let rendered = mixer.render(&mut [1.0]).unwrap();
        assert_eq!(
            end.observe(Some(rendered), pair(0))
                .unwrap()
                .unwrap()
                .physical_frame,
            0
        );
        let mut late = NativeEnd::new(output(0), ClockDomainId(1), 1000, 0).unwrap();
        assert!(late.observe(Some(rendered), pair(1_000_000)).is_err());
        assert!(late.last_report.is_none() && late.last_pair.is_none() && !late.emitted);
        assert!(NativeEnd::new(output(0), ClockDomainId(1), 0, 1).is_err());
        assert!(NativeEnd::new(output(0), ClockDomainId(2), 1000, 1).is_err());
        assert!(NativeEnd::new(output(0), ClockDomainId(1), 1, u64::MAX).is_err());
    }
}
