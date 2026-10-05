// Deferred common gameplay fixtures. This child reuses the actual parent
// Runtime/Mixer/capture fixture; it never supplies a room start or ACK receipt.
use super::*;
use crate::{
    competition::Competition,
    local_players::PlayerId,
    native_competition_network::NativeCompetitionNetwork,
    native_room_competition::NativeRoomCompetition,
    native_room_network::{
        NativeRoomNetwork, NativeRoomOptions, NativeRoomReceipts, NativeRoomStream,
    },
};
use std::io::{self, Read, Write};

struct NeverAcquired;
impl Read for NeverAcquired {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("no stream was acquired")
    }
}
impl Write for NeverAcquired {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        panic!("no stream was acquired")
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("no stream was acquired")
    }
}
impl NativeRoomStream for NeverAcquired {
    fn idle(&mut self, _: WallDuration) -> io::Result<()> {
        panic!("no stream was acquired")
    }
    fn finish(&mut self, _: WallDuration) -> io::Result<()> {
        panic!("no stream was acquired")
    }
}

fn attach_room(fixture: &mut Fixture) {
    let options = NativeRoomOptions::default();
    let mut network = NativeRoomNetwork::spawn_with::<NeverAcquired, _>(
        b"local completion fixture",
        &[PlayerId(1)],
        options,
        |_, _, _| {
            Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "controlled acquisition refusal",
            ))
        },
    )
    .unwrap();
    // Join the actual owner first: subsequent local judging is independent of
    // whether cancellation or the acquisition refusal won that thread race.
    let stopped = network.stop();
    assert_eq!(stopped.receipts, NativeRoomReceipts::default());
    assert!(!stopped.leave_written);
    assert!(stopped.cleanup_error.is_none());
    let room =
        NativeRoomCompetition::new(network, vec![PlayerId(1)], WallDuration::from_millis(10))
            .unwrap();
    let backend = NativeCompetitionNetwork::from_room(room, options.start_policy);
    assert!(backend.is_room());
    assert!(!backend.native_completed());
    assert!(backend.start_schedule().is_none());
    let competition =
        Competition::new(fixture.capture.as_ref().unwrap().header().clone(), 8).unwrap();
    fixture.competition = Some(
        LiveCompetition::from_prepared(
            PlayerId(1),
            competition,
            Some(backend),
            WallDuration::from_millis(10),
        )
        .unwrap(),
    );
}

// Forward every genuine clock, render and event observation. Only an explicit
// native-close boundary or temporary absence of render evidence is selected.
struct UntilClosed<'a> {
    device: &'a mut Device,
    close_at: u64,
    hide_render: bool,
}
impl NativeGameplayDevice for UntilClosed<'_> {
    fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
        self.device.observe(discipline)
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        let actual = self.device.render_report()?;
        Ok(if self.hide_render { None } else { actual })
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        self.device.host_now()
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        let mut batch = self.device.acquire(events)?;
        batch.closed = self.device.step >= self.close_at;
        Ok(batch)
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &PresentationDiscipline,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        self.device.observe_end(end, discipline, report)
    }
    fn seed_resume(
        &mut self,
        discipline: &mut PresentationDiscipline,
        reference: ClockPair,
    ) -> NativeGameplayResult<()> {
        self.device.seed_resume(discipline, reference)
    }
    fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint> {
        self.device.fallback_schedule(rate)
    }
}
fn run_until(
    fixture: &mut Fixture,
    finite: bool,
    close_at: u64,
    seconds: Option<u64>,
    hide_render: bool,
) -> NativeGameplayResult<()> {
    run_gameplay(
        &mut UntilClosed {
            device: &mut fixture.device,
            close_at,
            hide_render,
        },
        NativeGameplaySession {
            runtime: &mut fixture.runtime,
            gauge: &mut fixture.gauge,
            bgm: &mut fixture.bgm,
            discipline: &mut fixture.discipline,
            pause: &mut fixture.pause,
            end: &mut fixture.end,
            completion: &mut fixture.completion,
            capture: &mut fixture.capture,
            competition: &mut fixture.competition,
            delivery: &mut fixture.delivery,
            pre_origin_inputs: &mut fixture.pre,
        },
        NativeGameplayConfig {
            origin: point(1, 0),
            stream_origin: point(2, 0),
            playback_origin: point(2, 0),
            song_origin: Timestamp::ZERO,
            sample_rate: 1000,
            end_song: finite.then_some(Timestamp::from_nanos(10_000_000)),
            advance_lag: Duration::from_nanos(10_000_000),
            seconds,
            pause_supported: false,
            logical_schedule: true,
        },
    )
}
fn completed(fixture: &Fixture) -> bool {
    fixture.competition.as_ref().unwrap().native_completed()
}

#[test]
fn only_actual_finite_completion_marks_room_proof_not_ok_cancel_seconds_or_input_failure() {
    let mut finite = Fixture::new(true, false);
    attach_room(&mut finite);
    finite.run(true, true).unwrap();
    assert!(completed(&finite));
    assert_eq!(finite.device.step, 3);
    assert!(
        finite
            .device
            .report
            .unwrap()
            .playback_end_physical_frame
            .is_some()
    );
    assert_eq!(finite.delivery.observed_events(), 1);
    assert_eq!(finite.runtime.telemetry().counters().inputs, 0);
    assert_eq!(
        finite
            .capture
            .as_ref()
            .unwrap()
            .records()
            .last()
            .unwrap()
            .song_time,
        Timestamp::from_nanos(10_000_000)
    );
    assert_eq!(
        finite.runtime.judge().state(beatkernel::chart::ObjectId(1)),
        Some(beatkernel::interaction::InteractionState::Pending)
    );
    finite.competition.as_mut().unwrap().finish();
    assert!(completed(&finite));

    let mut native_close = Fixture::new(true, false);
    attach_room(&mut native_close);
    run_until(&mut native_close, true, 1, None, false).unwrap();
    assert!(!completed(&native_close));
    assert!(native_close.capture.as_ref().unwrap().records().is_empty());
    native_close.competition.as_mut().unwrap().finish();

    let mut seconds = Fixture::new(true, false);
    attach_room(&mut seconds);
    run_until(&mut seconds, true, 8, Some(0), false).unwrap();
    assert!(!completed(&seconds));
    assert_eq!(seconds.device.step, 0);
    seconds.competition.as_mut().unwrap().finish();

    let mut cancelled = Fixture::new(true, false);
    attach_room(&mut cancelled);
    let (publisher, viewer) = player::channel();
    viewer.cancel();
    player::with_publisher(publisher, || {
        cancelled.run(true, true).map_err(|error| error.to_string())
    })
    .unwrap();
    assert!(!completed(&cancelled));
    assert_eq!(cancelled.device.step, 0);
    cancelled.competition.as_mut().unwrap().finish();

    let mut invalid = Fixture::new(false, false);
    attach_room(&mut invalid);
    invalid.device.invalid = true;
    assert!(invalid.run(false, true).is_err());
    assert!(!completed(&invalid));
    assert_eq!(invalid.capture.as_ref().unwrap().records().len(), 1);
    invalid.competition.as_mut().unwrap().finish();
}

#[test]
fn whole_song_marks_only_after_actual_judge_pcm_and_later_presented_drain() {
    for hide_render in [false, true] {
        let mut fixture = Fixture::new(false, false);
        let source = fixture.source.clone();
        let compiled = source.compile().unwrap();
        let format = AudioFormat::new(1000, 1).unwrap();
        let pcm_limits = PcmLimits::new(64, 256, 1).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25, 0.5], pcm_limits).unwrap(),
        )
        .unwrap();
        let sound = SoundBinding {
            object: compiled.chart.objects()[0].id,
            stage: beatkernel::judge::JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(1),
            gain: 1.0,
        };
        let prepared = crate::PreparedBms {
            source,
            compiled,
            bank,
            sounds: vec![sound],
            bgm_commands: Vec::new(),
        };
        fixture.completion =
            Some(SongCompletion::prepare(&prepared, 0, 0, 0, ClockDomainId(2)).unwrap());
        attach_room(&mut fixture);
        run_until(&mut fixture, false, 8, None, hide_render).unwrap();
        assert_eq!(completed(&fixture), !hide_render);
        assert_eq!(
            fixture
                .runtime
                .judge()
                .state(beatkernel::chart::ObjectId(1)),
            Some(beatkernel::interaction::InteractionState::Completed)
        );
        assert_eq!(&fixture.device.pcm[20..23], &[0.25, 0.5, 0.0]);
        if hide_render {
            assert_eq!(fixture.device.step, 8);
        } else {
            assert!(fixture.device.step >= 5 && fixture.device.step < 8);
            let rendered = fixture.device.report.unwrap();
            assert_eq!(rendered.active_voices, 0);
            assert_eq!(rendered.pending_commands, 0);
            assert!(
                fixture
                    .discipline
                    .latest_pair()
                    .unwrap()
                    .source
                    .timestamp
                    .as_nanos()
                    >= (rendered.start_frame + rendered.frames as u64) as i64 * 1_000_000
            );
        }
        let capture = fixture.capture.as_ref().unwrap();
        let replay = beatkernel::replay::codec::ReplayFile::new(
            capture.header().clone(),
            capture.records().to_vec(),
        );
        crate::replay_playback::reconstruct(&fixture.source, replay, limits()).unwrap();
        fixture.competition.as_mut().unwrap().finish();
        assert_eq!(completed(&fixture), !hide_render);
    }
}
