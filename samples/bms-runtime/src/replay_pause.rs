//! Output-only replay pause projected onto the exact recorded song grid.
//! Native boundary interpolation retains Unknown mapping quality; this model
//! neither reads clocks nor invents operations beyond the recorded prefix.
use crate::playback_pause::{NativePause, PauseError, PausePhase};
use beatkernel::{
    audio::RenderReport,
    time::{ClockDomainId, ClockPair, ClockPoint, Duration, Timestamp},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayPauseBoundary {
    pub paused: bool,
    pub host: ClockPoint,
    pub song: Timestamp,
}

pub struct ReplayPause {
    native: NativePause,
    origin: ClockPoint,
    rate: u32,
    base: Timestamp,
}
impl ReplayPause {
    pub fn new(
        origin: ClockPoint,
        host: ClockDomainId,
        rate: u32,
        start: Timestamp,
        preroll: Duration,
    ) -> Result<Self, PauseError> {
        if preroll.as_nanos() < 0 {
            return Err(PauseError("replay pause preroll must be nonnegative"));
        }
        let base = start.checked_sub(preroll).ok_or(PauseError(
            "recorded start minus preroll overflows song time",
        ))?;
        Ok(Self {
            native: NativePause::new(origin, host, rate)?,
            origin,
            rate,
            base,
        })
    }
    pub fn phase(&self) -> PausePhase {
        self.native.phase()
    }
    pub fn request(&mut self, paused: bool, reference: ClockPair) -> Result<bool, PauseError> {
        self.native.request(paused, reference)
    }
    pub fn last_render_report(&self) -> Option<RenderReport> {
        self.native.last_render_report()
    }
    /// Frozen boundary song time uses its first actual playback frame, even
    /// when the coalesced resume report already contains later active frames.
    pub fn observe(
        &mut self,
        report: Option<RenderReport>,
        pair: ClockPair,
    ) -> Result<Option<ReplayPauseBoundary>, PauseError> {
        let mut native = self.native.clone();
        let boundary = native.observe(report, pair)?;
        let mapped = boundary
            .map(|boundary| -> Result<ReplayPauseBoundary, PauseError> {
                let offset =
                    i128::from(boundary.playback_frame) * 1_000_000_000 / i128::from(self.rate);
                let song = i128::from(self.base.as_nanos())
                    .checked_add(offset)
                    .and_then(|value| i64::try_from(value).ok())
                    .ok_or(PauseError("replay pause boundary song time overflow"))?;
                Ok(ReplayPauseBoundary {
                    paused: boundary.paused,
                    host: boundary.host,
                    song: Timestamp::from_nanos(song),
                })
            })
            .transpose()?;
        self.native = native;
        Ok(mapped)
    }
    /// Native physical presentation advances visual song time only while the
    /// acknowledged state is Running. Cumulative pause frames are rounded once.
    pub fn presentation_song(&self, source: ClockPoint) -> Result<Option<Timestamp>, PauseError> {
        if source.domain != self.origin.domain || source.timestamp < self.origin.timestamp {
            return Err(PauseError(
                "replay presentation has wrong output domain or precedes origin",
            ));
        }
        if self.phase() != PausePhase::Running {
            return Ok(None);
        }
        let base = self.native.song_origin_after_pause(self.base)?;
        let physical =
            i128::from(source.timestamp.as_nanos()) - i128::from(self.origin.timestamp.as_nanos());
        let song = i128::from(base.as_nanos())
            .checked_add(physical)
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(PauseError("replay presentation song time overflow"))?;
        Ok(Some(Timestamp::from_nanos(song)))
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::audio::AudioCounters;
    fn point(domain: u32, ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn pair(ns: i64) -> ClockPair {
        ClockPair {
            source: point(1, ns),
            target: point(2, ns + 10_000),
        }
    }
    fn report(physical: u64, playback: u64, frames: usize, paused: bool) -> RenderReport {
        RenderReport {
            start_frame: physical,
            frames,
            playback_start_frame: playback,
            playback_frames: if paused { 0 } else { frames },
            paused,
            playback_end_physical_frame: None,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        }
    }
    #[test]
    fn actual_straddling_mixer_end_freezes_replay_song_only_after_native_crossing() {
        use beatkernel::audio::{
            AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample,
            SampleBank, SampleId, VoiceId, command_queue,
        };
        let format = AudioFormat::new(1000, 1).unwrap();
        let limits = AudioLimits::new(4, 2, 4, 16, 4).unwrap();
        let pcm = PcmLimits::new(64, 128, 1).unwrap();
        let mut bank = SampleBank::new(format, pcm).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.5; 16], pcm).unwrap(),
        )
        .unwrap();
        let (mut producer, consumer) = command_queue(4).unwrap();
        for (voice, at) in [(1, 0), (2, 4_000_000)] {
            producer
                .try_push(AudioCommand::Play {
                    voice: VoiceId(voice),
                    sample: SampleId(1),
                    at: Timestamp::from_nanos(at),
                    gain: 1.0,
                })
                .unwrap();
        }
        let config = MixerConfig::new(format, ClockDomainId(1), Timestamp::ZERO, limits)
            .with_playback_end_frame(4);
        let mut mixer = Mixer::new(config, bank, consumer).unwrap();
        let mut pause = ReplayPause::new(
            point(1, 0),
            ClockDomainId(2),
            1000,
            Timestamp::from_nanos(50_000_000),
            Duration::from_nanos(3_000_000),
        )
        .unwrap();
        pause.request(true, pair(0)).unwrap();
        let mut output = [99.0; 10];
        let rendered = mixer.render(&mut output).unwrap();
        assert_eq!(output, [0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(rendered.playback_frames, 4);
        assert!(rendered.paused);
        assert_eq!(rendered.counters.commands_applied, 1);
        assert_eq!(rendered.pending_commands, 1);
        assert_eq!(
            pause.observe(Some(rendered), pair(3_000_000)).unwrap(),
            None
        );
        assert_eq!(pause.presentation_song(point(1, 3_000_000)).unwrap(), None);
        let boundary = pause.observe(None, pair(4_000_000)).unwrap().unwrap();
        assert_eq!(boundary.song, Timestamp::from_nanos(51_000_000));
        assert!(boundary.paused);
        assert_eq!(
            pause.observe(Some(rendered), pair(9_000_000)).unwrap(),
            None
        );
        let mut silence = [99.0; 3];
        let frozen = mixer.render(&mut silence).unwrap();
        assert_eq!(silence, [0.0; 3]);
        assert_eq!(frozen.playback_start_frame, 4);
        assert_eq!(frozen.playback_frames, 0);
        assert_eq!(pause.observe(Some(frozen), pair(12_000_000)).unwrap(), None);
        producer.request_pause(false);
        assert_eq!(mixer.render(&mut silence).unwrap().playback_frames, 0);
        assert_eq!(silence, [0.0; 3]);
        assert_eq!(pause.presentation_song(point(1, 15_000_000)).unwrap(), None);
    }
    #[test]
    fn coalesced_resume_preserves_frozen_song_prefix_but_later_presentation_can_advance() {
        let mut pause = ReplayPause::new(
            point(1, 0),
            ClockDomainId(2),
            1000,
            Timestamp::from_nanos(50_000_000),
            Duration::from_nanos(3_000_000),
        )
        .unwrap();
        assert_eq!(
            pause.presentation_song(point(1, 0)).unwrap(),
            Some(Timestamp::from_nanos(47_000_000))
        );
        pause.request(true, pair(0)).unwrap();
        assert_eq!(pause.presentation_song(point(1, 8_000_000)).unwrap(), None);
        assert_eq!(
            pause
                .observe(Some(report(12, 10, 4, true)), pair(8_000_000))
                .unwrap(),
            None
        );
        let frozen = pause.observe(None, pair(12_000_000)).unwrap().unwrap();
        assert!(frozen.paused);
        assert_eq!(frozen.song, Timestamp::from_nanos(57_000_000));
        assert_eq!(pause.presentation_song(point(1, 14_000_000)).unwrap(), None);
        pause.request(false, pair(14_000_000)).unwrap();
        assert_eq!(pause.presentation_song(point(1, 16_000_000)).unwrap(), None);
        let resumed = pause
            .observe(Some(report(20, 12, 2, false)), pair(20_000_000))
            .unwrap()
            .unwrap();
        assert!(!resumed.paused);
        assert_eq!(resumed.song, frozen.song);
        assert_eq!(pause.phase(), PausePhase::Running);
        assert_eq!(
            pause.presentation_song(point(1, 20_000_000)).unwrap(),
            Some(Timestamp::from_nanos(59_000_000))
        );
        assert_eq!(pause.last_render_report(), Some(report(20, 12, 2, false)));
    }
    #[test]
    fn noninteger_frame_grid_and_repeated_pauses_round_the_total_gap_once() {
        let mut pause = ReplayPause::new(
            point(1, 0),
            ClockDomainId(2),
            3,
            Timestamp::ZERO,
            Duration::ZERO,
        )
        .unwrap();
        let mut reference = pair(0);
        for index in 0..3u64 {
            let play = index + 1;
            let physical = play + index;
            pause.request(true, reference).unwrap();
            let crossing = pair((physical * 1_000_000_000 / 3) as i64 + 10);
            let frozen = pause
                .observe(Some(report(physical, play, 1, true)), crossing)
                .unwrap()
                .unwrap();
            assert_eq!(frozen.song.as_nanos(), (play * 1_000_000_000 / 3) as i64);
            pause.request(false, crossing).unwrap();
            reference = pair(((physical + 1) * 1_000_000_000 / 3) as i64 + 10);
            let resumed = pause
                .observe(Some(report(physical + 1, play, 1, false)), reference)
                .unwrap()
                .unwrap();
            assert_eq!(resumed.song, frozen.song);
        }
        assert_eq!(
            pause.presentation_song(point(1, 2_000_000_000)).unwrap(),
            Some(Timestamp::from_nanos(1_000_000_000))
        );
    }
    #[test]
    fn boundary_song_overflow_does_not_commit_ack_or_cached_report() {
        let mut pause = ReplayPause::new(
            point(1, 0),
            ClockDomainId(2),
            1000,
            Timestamp::from_nanos(i64::MAX),
            Duration::ZERO,
        )
        .unwrap();
        pause.request(true, pair(0)).unwrap();
        assert!(
            pause
                .observe(Some(report(1, 1, 1, true)), pair(2_000_000))
                .is_err()
        );
        assert_eq!(pause.phase(), PausePhase::Pausing);
        assert_eq!(pause.last_render_report(), None);
        assert_eq!(pause.presentation_song(point(1, 2_000_000)).unwrap(), None);
        assert!(pause.presentation_song(point(9, 0)).is_err());
        let running = ReplayPause::new(
            point(1, -10),
            ClockDomainId(2),
            1000,
            Timestamp::from_nanos(i64::MAX),
            Duration::ZERO,
        )
        .unwrap();
        assert!(running.presentation_song(point(1, -9)).is_err());
        assert!(running.presentation_song(point(1, -11)).is_err());
        assert!(
            ReplayPause::new(
                point(1, 0),
                ClockDomainId(2),
                0,
                Timestamp::ZERO,
                Duration::ZERO
            )
            .is_err()
        );
        assert!(
            ReplayPause::new(
                point(1, 0),
                ClockDomainId(2),
                1000,
                Timestamp::ZERO,
                Duration::from_nanos(-1)
            )
            .is_err()
        );
        assert!(
            ReplayPause::new(
                point(1, 0),
                ClockDomainId(2),
                1000,
                Timestamp::from_nanos(i64::MIN),
                Duration::from_nanos(1)
            )
            .is_err()
        );
    }
}
