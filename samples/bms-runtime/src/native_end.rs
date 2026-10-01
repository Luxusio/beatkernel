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
        };
        result.point(end)?;
        Ok(result)
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
    fn grids(report: RenderReport) -> Result<(u64, u64), EndError> {
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
        if report.playback_start_frame > report.start_frame
            || report.playback_frames > report.frames
            || (!report.paused && report.playback_frames != report.frames)
            || report.counters.rendered_frames != physical
        {
            return Err(EndError("end render grids or counters are inconsistent"));
        }
        Ok((physical, playback))
    }
    fn check_report(&self, report: RenderReport) -> Result<Option<u64>, EndError> {
        let (physical, playback) = Self::grids(report)?;
        self.point(physical)?;
        if playback > self.end {
            return Err(EndError("render exceeded configured playback end"));
        }
        if let Some(old) = self.last_report {
            let (old_physical, old_playback) = Self::grids(old)?;
            if report.start_frame < old.start_frame
                || physical < old_physical
                || playback < old_playback
                || report.start_frame - report.playback_start_frame
                    < old.start_frame - old.playback_start_frame
            {
                return Err(EndError("end render grid regressed"));
            }
        }
        let marker = report.playback_end_physical_frame;
        if self.physical.is_some() && marker != self.physical {
            return Err(EndError(
                "retained physical endpoint changed or disappeared",
            ));
        }
        if let Some(frame) = marker {
            if !report.paused
                || playback != self.end
                || frame < self.end
                || frame > physical
                || (report.playback_frames > 0
                    && frame != report.start_frame + report.playback_frames as u64)
                || (report.playback_frames == 0 && frame > report.start_frame)
            {
                return Err(EndError(
                    "physical endpoint does not match the finite playback grid",
                ));
            }
            self.point(frame)?;
        } else if report.frames > 0 && playback == self.end {
            return Err(EndError(
                "finite end render lacks physical endpoint evidence",
            ));
        }
        Ok(marker)
    }
    /// Unavailable render telemetry preserves prior evidence. Invalid observations
    /// commit nothing. A boundary is emitted once after actual native crossing.
    pub fn observe(
        &mut self,
        report: Option<RenderReport>,
        pair: ClockPair,
    ) -> Result<Option<EndBoundary>, EndError> {
        let mut next = self.clone();
        let boundary = next.observe_inner(report, pair)?;
        *self = next;
        Ok(boundary)
    }
    fn observe_inner(
        &mut self,
        report: Option<RenderReport>,
        pair: ClockPair,
    ) -> Result<Option<EndBoundary>, EndError> {
        self.check_pair(pair)?;
        if let Some(report) = report {
            self.physical = self.check_report(report)?;
            self.last_report = Some(report);
        }
        self.lower.get_or_insert(pair);
        self.last_pair = Some(pair);
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
        let host = if pair.source.timestamp == output.timestamp {
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
        let format = AudioFormat::new(1000, 1).unwrap();
        let pcm = PcmLimits::new(1024, 1024, 2).unwrap();
        let mut bank = SampleBank::new(format, pcm).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.5; 16], pcm).unwrap(),
        )
        .unwrap();
        let (mut producer, consumer) = command_queue(8).unwrap();
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
