//! Deferred real model/ready ownership and atomic timing publication.
use crate::{
    gameplay_presentation::{
        GameplayOutputContext, GameplayPauseControl, prepare_output_timing_rebind,
    },
    output_replacement::{ReadyOutput, publish_ready_output},
    native_end::NativeEnd,
    native_gameplay::NativeGameplayConfig,
    local_runtime::SoloRuntime,
    playback_pause::NativePause,
};
use crate::gameplay_presentation_port_fixtures::{point, judge, source, bindings};
use beatkernel::{
    audio::*,
    time::{
        ClockDomainId, ClockPair, Duration, Timestamp,
        presentation::{DisciplineConfig, PresentationEstimator},
    },
    transport::{Rate, Transport},
    runtime::RuntimeProcessingClock,
};
struct Output {
    mixer: Mixer,
    id: Box<u64>,
}
fn pair(frame: u64, shift: i64) -> ClockPair {
    ClockPair {
        source: point(2, frame as i64 * 1_000_000),
        target: point(1, shift + frame as i64 * 1_000_000),
    }
}
struct Fixture {
    ready: Option<ReadyOutput<PresentationEstimator, Output>>,
    output: Option<Output>,
    presentation: PresentationEstimator,
    pause: NativePause,
    config: NativeGameplayConfig,
    end: Option<NativeEnd>,
    runtime: SoloRuntime,
}
impl Fixture {
    fn new(finite: bool, frozen: usize, start: Option<u64>, shift: i64) -> Self {
        let format = AudioFormat::new(1000, 1).unwrap();
        let pcm = PcmLimits::new(64, 128, 1).unwrap();
        let mut bank = SampleBank::new(format, pcm).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.], pcm).unwrap(),
        )
        .unwrap();
        let (mut producer, consumer) = if start.is_some() {
            command_queue_with_start_gate(8)
        } else {
            command_queue(8)
        }
        .unwrap();
        if let Some(start) = start {
            producer.schedule_start_at(start).unwrap();
        }
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(u64::MAX),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.,
            })
            .unwrap();
        let config = MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        );
        let mut mixer = Mixer::new(
            if finite {
                config.with_playback_end_frame(8)
            } else {
                config
            },
            bank,
            consumer,
        )
        .unwrap();
        let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 1000).unwrap();
        if finite {
            pause = pause.with_playback_end_frame(8).unwrap();
        }
        if let Some(start) = start {
            pause = pause.with_start_frame(start).unwrap();
        }
        mixer
            .render(&mut vec![0.; frozen + start.unwrap_or(0) as usize])
            .unwrap();
        pause.request(true, pair(mixer.frame_cursor(), 0)).unwrap();
        producer.request_pause(true);
        let report = mixer.render(&mut [0.; 1]).unwrap();
        let boundary = pause
            .observe(Some(report), pair(mixer.frame_cursor(), 0))
            .unwrap()
            .unwrap();
        mixer.render(&mut [0.; 2]).unwrap();
        let settings = DisciplineConfig {
            capacity: 8,
            min_span: Duration::from_nanos(500_000_000),
            ..Default::default()
        };
        let mut presentation =
            PresentationEstimator::new(settings, point(2, 0), ClockDomainId(1), Timestamp::ZERO)
                .unwrap();
        presentation
            .observe_clock_pair(pair(mixer.frame_cursor() - 2, 0))
            .unwrap();
        let hold = producer.hold_pause().unwrap();
        let mut timing =
            prepare_output_timing_rebind(&presentation, &pause, 1, &mixer, Timestamp::ZERO)
                .unwrap();
        let report = mixer.render(&mut [0.; 1]).unwrap();
        let accepted = pair(mixer.frame_cursor(), shift);
        timing
            .presentation
            .observe_clock_pair_in_epoch(1, accepted)
            .unwrap();
        timing
            .pause
            .observe_in_epoch(1, Some(report), accepted)
            .unwrap();
        let ready = ReadyOutput {
            output: Output {
                mixer,
                id: Box::new(71),
            },
            timing,
            hold,
        };
        let mut transport = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
        transport.pause(boundary.host.timestamp).unwrap();
        transport
            .seek(
                boundary.host.timestamp,
                Timestamp::from_nanos(frozen as i64 * 1_000_000),
            )
            .unwrap();
        let mut runtime = SoloRuntime::new(
            ClockDomainId(1),
            ClockDomainId(2),
            transport,
            bindings(None),
            judge(&source()),
            producer,
            vec![],
            8,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        let mut end =
            finite.then(|| NativeEnd::new(point(2, 0), ClockDomainId(1), 1000, 8).unwrap());
        if let Some(end) = end.as_mut() {
            if let Some(start) = start {
                *end = end.clone().with_start_frame(start).unwrap();
            }
            end.observe(None, pair(0, 0)).unwrap();
        }
        Self {
            ready: Some(ready),
            output: None,
            presentation,
            pause,
            config: NativeGameplayConfig {
                origin: point(1, 0),
                stream_origin: point(2, 0),
                playback_origin: point(2, start.unwrap_or(0) as i64 * 1_000_000),
                song_origin: Timestamp::ZERO,
                sample_rate: 1000,
                end_song: finite.then_some(Timestamp::from_nanos(8_000_000)),
                advance_lag: Duration::from_nanos(10_000_000),
                seconds: None,
                pause_supported: true,
                logical_schedule: true,
            },
            end,
            runtime,
        }
    }
    fn publish(
        &mut self,
    ) -> Result<(), crate::output_replacement::ReadyPublicationFailure<PresentationEstimator, Output>>
    {
        publish_ready_output(
            self.ready.take().unwrap(),
            &mut self.output,
            GameplayOutputContext {
                presentation: &mut self.presentation,
                pause: &mut self.pause,
                config: &mut self.config,
                end: &mut self.end,
                control: GameplayPauseControl::solo(&mut self.runtime),
            },
        )
    }
    fn snapshot(&mut self) -> (String, String, String, String, Transport) {
        (
            format!("{:?}", self.presentation),
            format!("{:?}", self.pause),
            format!("{:?}", self.config),
            format!("{:?}", self.end),
            self.runtime.transport_mut().clone(),
        )
    }
}
#[test]
fn refused_occupied_publication_returns_same_ready_output_and_hold_without_changing_live_clocks_or_history()
 {
    let mut f = Fixture::new(true, 2, None, 0);
    let mut occupied = Fixture::new(false, 2, None, 0);
    f.output = Some(occupied.ready.take().unwrap().output);
    let before = f.snapshot();
    let id = f.ready.as_ref().unwrap().output.id.as_ref() as *const u64;
    let failure = match f.publish() {
        Err(failure) => failure,
        Ok(()) => panic!("occupied owner must refuse"),
    };
    assert_eq!(failure.ready.output.id.as_ref() as *const u64, id);
    assert_eq!(f.snapshot(), before);
    assert!(f.runtime.hold_audio_pause().is_err());
    f.runtime.request_audio_pause(false);
    let mut ready = failure.ready;
    let report = ready.output.mixer.render(&mut [0.; 1]).unwrap();
    assert!(report.paused);
    assert_eq!(ready.output.mixer.playback_frame_cursor(), 2);
}
#[test]
fn malformed_ready_epoch_grid_host_start_end_or_frozen_identity_is_rejected_atomically_with_genuine_reports()
 {
    for case in 0..8 {
        let mut f = Fixture::new(true, 2, None, 0);
        let before = f.snapshot();
        let id = f.ready.as_ref().unwrap().output.id.as_ref() as *const u64;
        match case {
            0 => f.ready.as_mut().unwrap().timing.playback_origin = point(9, 5_000_000),
            1 => {
                f.ready.as_mut().unwrap().timing.basis =
                    OutputFrameBasis::new(point(2, 0), 999, 5).unwrap()
            }
            2 => f
                .ready
                .as_mut()
                .unwrap()
                .timing
                .presentation
                .rebind_output(2, point(2, 5_000_000), point(2, 5_000_000), Timestamp::ZERO)
                .unwrap(),
            3 => {
                let mut other = Fixture::new(true, 1, None, 0);
                f.ready.as_mut().unwrap().timing.pause = other.ready.take().unwrap().timing.pause;
            }
            4 => {
                let mut other = Fixture::new(true, 2, Some(4), 0);
                f.ready.as_mut().unwrap().timing.pause = other.ready.take().unwrap().timing.pause;
            }
            5 => {
                let mut other = Fixture::new(false, 2, None, 0);
                f.ready.as_mut().unwrap().timing.pause = other.ready.take().unwrap().timing.pause;
            }
            6 => {
                let ready = f.ready.as_mut().unwrap();
                let origin = ready.timing.playback_origin;
                let accepted = ready.timing.presentation.latest_pair().unwrap();
                let mut foreign = PresentationEstimator::new_with_playback_origin(
                    f.presentation.config(),
                    origin,
                    origin,
                    ClockDomainId(9),
                    Timestamp::from_nanos(5_000_000),
                )
                .unwrap();
                foreign
                    .rebind_output(1, origin, origin, Timestamp::from_nanos(5_000_000))
                    .unwrap();
                foreign
                    .observe_clock_pair_in_epoch(
                        1,
                        ClockPair {
                            source: accepted.source,
                            target: point(9, accepted.target.timestamp.as_nanos()),
                        },
                    )
                    .unwrap();
                ready.timing.presentation = foreign;
            }
            _ => {
                f.ready.as_mut().unwrap().timing.basis =
                    OutputFrameBasis::new(point(2, 1), 1000, 5).unwrap()
            }
        }
        let failure = match f.publish() {
            Err(failure) => failure,
            Ok(()) => panic!("identity fault must refuse"),
        };
        assert_eq!(failure.ready.output.id.as_ref() as *const u64, id);
        assert_eq!(f.snapshot(), before);
        assert!(f.output.is_none());
        assert!(f.runtime.hold_audio_pause().is_err());
    }
}
#[test]
fn successful_publication_updates_only_output_origins_and_epoch_leaves_history_song_and_audio_paused_until_explicit_resume()
 {
    let mut f = Fixture::new(false, 2, None, 0);
    let before = f.runtime.transport_mut().clone();
    let original_song = f.config.song_origin;
    let original_host = f.config.origin;
    match f.publish() {
        Ok(()) => {}
        Err(_) => panic!("genuine ready must publish"),
    }
    assert_eq!(f.presentation.epoch(), 1);
    assert_eq!(f.pause.epoch(), 1);
    assert_eq!(f.config.stream_origin, point(2, 5_000_000));
    assert_eq!(f.config.playback_origin, point(2, 5_000_000));
    assert_eq!(f.config.song_origin, original_song);
    assert_eq!(f.config.origin, original_host);
    assert_eq!(f.runtime.transport_mut(), &before);
    let hold = f.runtime.hold_audio_pause().unwrap();
    drop(hold);
    let output = f.output.as_mut().unwrap();
    assert_eq!(*output.id, 71);
    let mut pcm = [0.; 2];
    output.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.; 2]);
    assert_eq!(output.mixer.playback_frame_cursor(), 2);
    f.runtime.request_audio_pause(false);
    output.mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.75, 1.]);
}
#[test]
fn finite_end_uses_only_new_output_bracket_and_retains_original_grid_and_endpoint_after_publication()
 {
    let mut f = Fixture::new(true, 2, None, 1_000_000_000);
    match f.publish() {
        Ok(()) => {}
        Err(_) => panic!("finite ready must publish"),
    }
    let output = f.output.as_mut().unwrap();
    let reference = pair(output.mixer.frame_cursor(), 1_000_000_000);
    f.pause.request_in_epoch(1, false, reference).unwrap();
    f.runtime.request_audio_pause(false);
    let resumed = output.mixer.render(&mut [0.; 1]).unwrap();
    f.pause
        .observe_in_epoch(
            1,
            Some(resumed),
            pair(output.mixer.frame_cursor(), 1_000_000_000),
        )
        .unwrap()
        .unwrap();
    let report = output.mixer.render(&mut [0.; 6]).unwrap();
    assert_eq!(report.playback_end_physical_frame, Some(12));
    assert_eq!(output.mixer.frame_cursor(), 13);
    let end = f.end.as_mut().unwrap();
    let boundary = end
        .observe(Some(report), pair(13, 1_000_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(boundary.playback_frame, 8);
    assert_eq!(boundary.physical_frame, 12);
    assert_eq!(boundary.output, point(2, 12_000_000));
    assert_eq!(boundary.host, point(1, 1_012_000_000));
    assert_eq!(
        end.observe(Some(report), pair(13, 1_000_000_000)).unwrap(),
        None
    );
}
