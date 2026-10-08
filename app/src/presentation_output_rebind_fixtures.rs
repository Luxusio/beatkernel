//! Deferred cold combined staging on actual paused software output state.
use super::*;
use crate::playback_pause::{NativePause, PausePhase};
use beatkernel::{audio::*, time::Duration, transport::Rate};
use beatkernel_platform::audio::presentation::discipline::PresentationDiscipline;
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn pair(frame: u64) -> ClockPair {
    let ns = (frame * 1_000_000_000 / 3) as i64;
    ClockPair {
        source: point(1, ns),
        target: point(2, ns + 100),
    }
}
fn config() -> DisciplineConfig {
    DisciplineConfig {
        capacity: 8,
        min_span: Duration::from_nanos(500_000_000),
        correction_horizon: Duration::from_nanos(9_000_000_000),
        ..Default::default()
    }
}
fn paused() -> (CommandProducer, Mixer, NativePause) {
    let format = AudioFormat::new(3, 1).unwrap();
    let limits = PcmLimits::new(64, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(u64::MAX),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 0.5,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(1),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut pause = NativePause::new(point(1, 0), ClockDomainId(2), 3).unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    pause.request(true, pair(2)).unwrap();
    producer.request_pause(true);
    let report = mixer.render(&mut [0.; 1]).unwrap();
    pause.observe(Some(report), pair(3)).unwrap().unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    (producer, mixer, pause)
}

#[test]
fn complete_owner_timing_uses_first_unsent_frame_and_preserves_source_frontier() {
    use beatkernel_platform::audio::{DeviceFormat, NativeOutputState, SampleEncoding};
    let (_producer, mixer, pause) = paused();
    let mut state = match NativeOutputState::new(
        mixer,
        DeviceFormat::new(3, 1, SampleEncoding::Float32, None).unwrap(),
        None,
        4,
    ) {
        Ok(state) => state,
        Err(_) => panic!("valid direct owner"),
    };
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let current =
        PresentationEstimator::new(config(), point(1, 0), ClockDomainId(2), Timestamp::ZERO)
            .unwrap();
    let before = format!("{pause:?}");
    let staged =
        prepare_output_timing_rebind_state(&current, &pause, 1, &state, Timestamp::ZERO).unwrap();
    assert_eq!(staged.basis.start_physical_frame(), 6);
    assert_eq!(state.mixer().frame_cursor(), 9);
    assert_eq!(staged.playback_origin, point(1, 2_000_000_000));
    assert_eq!(state.pending_samples(), &[0.0; 3]);
    assert_eq!(staged.presentation.latest_pair(), None);
    assert_eq!(staged.pause.last_render_report(), None);
    assert_eq!(format!("{pause:?}"), before);
    assert_eq!(current.epoch(), 0);
}
#[test]
fn actual_core_staging_preserves_old_owners_and_mixer_but_new_epoch_has_no_samples_and_requires_warmup(
) {
    for epoch in [7, u64::MAX] {
        let (_producer, mixer, pause) = paused();
        let old_pause = format!("{pause:?}");
        let basis = mixer.output_frame_basis();
        let counters = mixer.counters();
        let mut current =
            PresentationEstimator::new(config(), point(1, 0), ClockDomainId(2), Timestamp::ZERO)
                .unwrap();
        current.observe_clock_pair(pair(3)).unwrap();
        let old_pair = current.latest_pair();
        let mut prepared = prepare_output_timing_rebind(
            &current,
            &pause,
            epoch,
            &mixer,
            Timestamp::from_nanos(604_800_000_000_000),
        )
        .unwrap();
        assert_eq!(prepared.basis, basis);
        assert_eq!(prepared.playback_origin, point(1, 1_666_666_666));
        assert_eq!(prepared.pause.epoch(), epoch);
        assert_eq!(prepared.pause.phase(), PausePhase::Paused);
        assert_eq!(prepared.presentation.epoch(), epoch);
        assert_eq!(prepared.presentation.config(), config());
        assert_eq!(prepared.presentation.latest_pair(), None);
        assert_eq!(prepared.presentation.retained_len(), 0);
        assert_eq!(format!("{pause:?}"), old_pause);
        assert_eq!(current.latest_pair(), old_pair);
        assert_eq!(current.epoch(), 0);
        assert_eq!(mixer.counters(), counters);
        assert_eq!(mixer.output_frame_basis(), basis);
        assert!(prepared
            .presentation
            .observe_clock_pair_in_epoch(0, pair(5))
            .is_err());
        assert_eq!(prepared.presentation.latest_pair(), None);
        prepared
            .presentation
            .observe_clock_pair_in_epoch(epoch, pair(5))
            .unwrap();
        let mut transport = Transport::new(
            pair(5).target.timestamp,
            Timestamp::from_nanos(604_801_666_666_666),
            Rate::NORMAL,
        );
        assert_eq!(
            prepared
                .presentation
                .update(pair(5).target, &mut transport)
                .unwrap(),
            DisciplineUpdate::Warmup { span_ns: 0 }
        );
    }
}
#[test]
fn actual_native_bridge_uses_same_basis_config_and_epoch_staging_without_backend_evidence() {
    let (_producer, mixer, pause) = paused();
    let mut current =
        PresentationDiscipline::new(config(), point(1, 0), ClockDomainId(2), Timestamp::ZERO)
            .unwrap();
    current.observe_clock_pair(pair(3)).unwrap();
    let original = current.latest_pair();
    let mut staged = prepare_output_timing_rebind(
        &current,
        &pause,
        9,
        &mixer,
        Timestamp::from_nanos(72_000_000_000_000),
    )
    .unwrap();
    assert_eq!(staged.presentation.epoch(), 9);
    assert_eq!(staged.presentation.config(), config());
    assert_eq!(staged.presentation.latest_pair(), None);
    assert_eq!(staged.playback_origin, point(1, 1_666_666_666));
    assert!(staged
        .presentation
        .observe_clock_pair_in_epoch(0, pair(5))
        .is_err());
    assert_eq!(staged.presentation.latest_pair(), None);
    assert_eq!(current.latest_pair(), original);
}
#[test]
fn genuine_resume_after_staging_reanchors_song_mapping_and_retains_original_pcm_commands() {
    let (mut producer, mut mixer, pause) = paused();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(7),
            sample: SampleId(1),
            at: Timestamp::from_nanos(1_000_000_000),
            gain: 0.5,
        })
        .unwrap();
    let current =
        PresentationEstimator::new(config(), point(1, 0), ClockDomainId(2), Timestamp::ZERO)
            .unwrap();
    let song = Timestamp::from_nanos(604_800_000_000_000);
    let mut staged = prepare_output_timing_rebind(&current, &pause, 1, &mixer, song).unwrap();
    staged.pause.request_in_epoch(1, false, pair(5)).unwrap();
    producer.request_pause(false);
    let mut samples = [0.; 1];
    let report = mixer.render(&mut samples).unwrap();
    assert_eq!(samples, [0.375]);
    staged
        .pause
        .observe_in_epoch(1, Some(report), pair(6))
        .unwrap()
        .unwrap();
    let mapped = staged
        .pause
        .song_origin_for_presentation(song, staged.playback_origin)
        .unwrap();
    assert_eq!(mapped, Timestamp::from_nanos(604_800_666_666_666));
    let mut fresh = staged
        .presentation
        .restart_for_resume(
            staged.playback_origin,
            staged.playback_origin,
            ClockDomainId(2),
            mapped,
        )
        .unwrap();
    assert_eq!(fresh.epoch(), 1);
    assert_eq!(fresh.config(), config());
    fresh.observe_clock_pair_in_epoch(1, pair(5)).unwrap();
    mixer.render(&mut samples).unwrap();
    assert_eq!(samples, [0.625]);
    mixer.render(&mut samples).unwrap();
    assert_eq!(samples, [0.25]);
    assert_eq!(mixer.frame_cursor(), 8);
    fresh
        .observe_clock_pair_in_epoch(1, pair(mixer.frame_cursor()))
        .unwrap();
    let mut transport = Transport::new(pair(5).target.timestamp, mapped, Rate::NORMAL);
    assert!(matches!(
        fresh.update(pair(8).target, &mut transport).unwrap(),
        DisciplineUpdate::Applied {
            phase_error_ns: 0,
            ..
        }
    ));
    assert_eq!(
        transport.position_at(pair(6).target.timestamp).unwrap(),
        Timestamp::from_nanos(604_801_000_000_000)
    );
}
static CONSTRUCTIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct Spy {
    epoch: Option<u64>,
    inner: PresentationEstimator,
}
impl GameplayPresentationPort for Spy {
    fn new_with_playback_origin(
        c: DisciplineConfig,
        o: ClockPoint,
        p: ClockPoint,
        h: ClockDomainId,
        s: Timestamp,
    ) -> NativeGameplayResult<Self> {
        CONSTRUCTIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Self {
            epoch: Some(0),
            inner: PresentationEstimator::new_with_playback_origin(c, o, p, h, s)?,
        })
    }
    fn config(&self) -> DisciplineConfig {
        self.inner.config()
    }
    fn epoch(&self) -> Option<u64> {
        self.epoch
    }
    fn latest_pair(&self) -> Option<ClockPair> {
        self.inner.latest_pair()
    }
    fn quality(&self) -> ClockMappingQuality {
        self.inner.quality()
    }
    fn validate_host(&self, p: ClockPoint) -> NativeGameplayResult<()> {
        Ok(self.inner.validate_host(p)?)
    }
    fn update(
        &mut self,
        p: ClockPoint,
        t: &mut Transport,
    ) -> NativeGameplayResult<DisciplineUpdate> {
        Ok(self.inner.update(p, t)?)
    }
}
#[test]
fn pending_resume_epoch_mismatch_legacy_and_invalid_phase_refuse_before_constructing_or_publishing_candidates(
) {
    use std::sync::atomic::Ordering::SeqCst;
    let (mut producer, mixer, pause) = paused();
    let before = format!("{pause:?}");
    let mut current = Spy {
        epoch: Some(0),
        inner: PresentationEstimator::new(config(), point(1, 0), ClockDomainId(2), Timestamp::ZERO)
            .unwrap(),
    };
    current.inner.observe_clock_pair(pair(3)).unwrap();
    let pair_before = current.latest_pair();
    CONSTRUCTIONS.store(0, SeqCst);
    producer.request_pause(false);
    assert!(mixer.is_paused());
    assert!(!mixer.pause_requested());
    assert!(prepare_output_timing_rebind(&current, &pause, 1, &mixer, Timestamp::ZERO).is_err());
    assert_eq!(CONSTRUCTIONS.load(SeqCst), 0);
    producer.request_pause(true);
    for token in [None, Some(9)] {
        current.epoch = token;
        assert!(
            prepare_output_timing_rebind(&current, &pause, 10, &mixer, Timestamp::ZERO).is_err()
        );
        assert_eq!(CONSTRUCTIONS.load(SeqCst), 0);
    }
    current.epoch = None;
    assert!(current
        .restart_for_output(
            10,
            point(1, 0),
            point(1, 0),
            ClockDomainId(2),
            Timestamp::ZERO
        )
        .is_err());
    assert_eq!(CONSTRUCTIONS.load(SeqCst), 0);
    let mut last_pause = pause.clone();
    last_pause.rebind_output(u64::MAX, &mixer).unwrap();
    current.epoch = Some(u64::MAX);
    assert!(
        prepare_output_timing_rebind(&current, &last_pause, u64::MAX, &mixer, Timestamp::ZERO)
            .is_err()
    );
    assert_eq!(CONSTRUCTIONS.load(SeqCst), 0);
    current.epoch = Some(0);
    assert!(current
        .restart_for_output(
            0,
            point(1, 0),
            point(1, 0),
            ClockDomainId(2),
            Timestamp::ZERO
        )
        .is_err());
    assert_eq!(CONSTRUCTIONS.load(SeqCst), 0);
    let running = NativePause::new(point(1, 0), ClockDomainId(2), 3).unwrap();
    assert!(prepare_output_timing_rebind(&current, &running, 1, &mixer, Timestamp::ZERO).is_err());
    assert_eq!(CONSTRUCTIONS.load(SeqCst), 0);
    assert!(prepare_output_timing_rebind(&current, &pause, 1, &mixer, Timestamp::ZERO).is_err());
    assert_eq!(CONSTRUCTIONS.load(SeqCst), 1); // Unsupported restore after cold construction.
    assert_eq!(current.latest_pair(), pair_before);
    assert_eq!(format!("{pause:?}"), before);
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.pause_requested()
        ),
        (5, 2, true)
    );
}
