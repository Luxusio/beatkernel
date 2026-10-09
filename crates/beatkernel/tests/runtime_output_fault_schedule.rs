//! Actual Runtime-to-Mixer schedules; software retirement flags are not native fences.
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioCounters, AudioError, AudioFormat, AudioLimits, Mixer,
        MixerConfig, OutputOpenFailure, PcmLimits, PcmSample, QueuePushError, SampleBank, SampleId,
        StoppedMixerSource, VoiceId,
    },
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::InstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    runtime::{Runtime, RuntimeProcessingClock, SoundBinding},
    telemetry::RuntimeCounters,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use std::{cell::Cell, rc::Rc};

const TARGETS: [i64; 5] = [0, 1_000_000, 2_000_000, 3_000_000, 4_000_000];
fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
struct ExplicitClocks;
impl ClockMapper for ExplicitClocks {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

fn fixture() -> (Runtime, Mixer) {
    fixture_channels(1)
}

fn fixture_channels(channels: u16) -> (Runtime, Mixer) {
    let mut source = SourceChart::new(1000, Bpm::new(60, 1).unwrap()).unwrap();
    for id in 1..=5 {
        source.objects.push(SourceObject {
            id: ObjectId(id),
            start: Beat::new(id as i64 - 1).unwrap(),
            end: None,
            interaction: InteractionId(id as u32),
            visual: VisualId(id as u32),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let judge = JudgeEngine::new(
        source.compile().unwrap(),
        (1..=5)
            .map(|id| Rule {
                interaction: InteractionId(id),
                control: GameControlId(id),
                evaluator: Box::new(InstantEvaluator),
            })
            .collect(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let bindings = BindingMap::from_bindings((1_u16..=5).map(|id| Binding {
        device: DeviceSelector::Exact(DeviceId(99)),
        physical: PhysicalControlId::keyboard(id),
        game_control: GameControlId(u32::from(id)),
    }))
    .unwrap();
    let (producer, consumer) = command_queue(1).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        (1..=5)
            .map(|id| SoundBinding {
                object: ObjectId(id),
                stage: JudgeStage::Instant,
                sample: SampleId(1),
                voice: VoiceId(id),
                gain: 0.5,
            })
            .collect(),
        8,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let format = AudioFormat::new(1000, channels).unwrap();
    let limits = PcmLimits::new(64, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            format,
            if channels == 1 {
                vec![0.25, 0.5, 0.75, 1.0]
            } else {
                vec![0.25, -0.25, 0.5, -0.5, 0.75, -0.75, 1.0, -1.0]
            },
            limits,
        )
        .unwrap(),
    )
    .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(1, 4, 4, 4, 4).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (runtime, mixer)
}

fn play(note: u64, audio_ns: i64) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(note),
        sample: SampleId(1),
        at: Timestamp::from_nanos(audio_ns),
        gain: 0.5,
    }
}

fn hit(runtime: &mut Runtime, note: u32, audio_ns: i64, failure: Option<QueuePushError>) {
    let target = TARGETS[note as usize - 1];
    let report = runtime
        .process_input(
            PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(99), point(1, target), 100 + u64::from(note)),
                control: PhysicalControlId::keyboard(note as u16),
                state: ButtonState::Down,
            }),
            &ExplicitClocks,
            point(2, audio_ns),
        )
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(target));
    assert_eq!(report.judge_error, None);
    assert_eq!(report.bound_inputs.len(), 1);
    assert_eq!(report.bound_inputs[0].game_control, GameControlId(note));
    assert_eq!(report.judge_events.len(), 1);
    let event = report.judge_events[0];
    assert_eq!(event.object, ObjectId(u64::from(note)));
    assert_eq!(event.at, Timestamp::from_nanos(target));
    assert_eq!(event.stage, JudgeStage::Instant);
    assert_eq!(
        event.outcome,
        JudgeOutcome::Hit {
            grade: JudgeGrade(7),
            delta: Duration::ZERO
        }
    );
    assert_eq!(event.input.unwrap().source, DeviceId(99));
    assert_eq!(event.input.unwrap().sequence, 100 + u64::from(note));
    assert_eq!(
        runtime.judge().effective_song_time(),
        Some(Timestamp::from_nanos(target))
    );
    if let Some(reason) = failure {
        assert!(report.audio_commands.is_empty());
        assert_eq!(report.audio_failures.len(), 1);
        assert_eq!(report.audio_failures[0].reason, reason);
        assert_eq!(
            report.audio_failures[0].command,
            play(u64::from(note), audio_ns)
        );
    } else {
        assert!(report.audio_failures.is_empty());
        assert_eq!(report.audio_commands, [play(u64::from(note), audio_ns)]);
    }
}

fn refuse_large_render(mixer: &mut Mixer) {
    let before = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.output_frame_basis(),
        mixer.rate(),
        mixer.counters(),
        mixer.is_paused(),
    );
    let mut output = [91.0; 5];
    assert_eq!(mixer.render(&mut output), Err(AudioError::RenderCapacity));
    assert_eq!(output, [91.0; 5]);
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.output_frame_basis(),
            mixer.rate(),
            mixer.counters(),
            mixer.is_paused()
        ),
        before
    );
}

fn render_parts(mixer: &mut Mixer, parts: &[usize]) -> Vec<f32> {
    let mut pcm = Vec::new();
    for &frames in parts {
        let start = mixer.frame_cursor();
        let mut output = vec![91.0; frames * usize::from(mixer.config().format().channels())];
        let report = mixer.render(&mut output).unwrap();
        assert_eq!(report.start_frame, start);
        assert_eq!(report.frames, frames);
        assert!(!report.producer_disconnected);
        pcm.extend(output);
    }
    pcm
}

#[test]
fn queue_full_and_invalid_render_preserve_judgments_and_literal_pcm_across_partitions() {
    for (prefix, suffix) in [(&[2][..], &[2, 4][..]), (&[1, 1][..], &[1, 1, 2, 2][..])] {
        for _ in 0..3 {
            let (mut runtime, mut mixer) = fixture();
            hit(&mut runtime, 1, 0, None);
            hit(&mut runtime, 2, 1_000_000, Some(QueuePushError::Full));
            let hash = runtime.judge().stable_hash().unwrap();
            refuse_large_render(&mut mixer);
            assert_eq!(runtime.judge().stable_hash().unwrap(), hash);
            let mut pcm = render_parts(&mut mixer, prefix);
            assert_eq!(pcm, [0.125, 0.25]);
            hit(&mut runtime, 3, 2_000_000, None);
            // The active first voice and queued third sound both survive refusal.
            refuse_large_render(&mut mixer);
            pcm.extend(render_parts(&mut mixer, suffix));
            assert_eq!(pcm, [0.125, 0.25, 0.5, 0.75, 0.375, 0.5, 0.0, 0.0]);
            assert_eq!(mixer.frame_cursor(), 8);
            assert_eq!(mixer.playback_frame_cursor(), 8);
            assert_eq!(mixer.rate(), Rate::NORMAL);
            assert_eq!(
                mixer.counters(),
                AudioCounters {
                    rendered_frames: 8,
                    commands_consumed: 2,
                    commands_applied: 2,
                    ..AudioCounters::default()
                }
            );
            assert_eq!(
                runtime.telemetry().counters(),
                RuntimeCounters {
                    inputs: 3,
                    judge_results: 3,
                    audio_commands: 2,
                    queue_full: 1,
                    ..RuntimeCounters::default()
                }
            );
            assert_eq!(runtime.telemetry().processing(), None);
        }
    }
}

#[test]
fn invalid_stereo_length_preserves_active_voice_and_queued_sound_then_renders_literal_pcm() {
    let (mut runtime, mut mixer) = fixture_channels(2);
    hit(&mut runtime, 1, 0, None);
    hit(&mut runtime, 2, 1_000_000, Some(QueuePushError::Full));
    let mut pcm = render_parts(&mut mixer, &[2]);
    assert_eq!(pcm, [0.125, -0.125, 0.25, -0.25]);
    hit(&mut runtime, 3, 2_000_000, None);
    let before = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.output_frame_basis(),
        mixer.rate(),
        mixer.counters(),
        mixer.is_paused(),
    );
    let judge_hash = runtime.judge().stable_hash().unwrap();
    let mut incomplete_frame = [91.0; 3];
    assert_eq!(
        mixer.render(&mut incomplete_frame),
        Err(AudioError::InvalidBuffer)
    );
    assert_eq!(incomplete_frame, [91.0; 3]);
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.output_frame_basis(),
            mixer.rate(),
            mixer.counters(),
            mixer.is_paused()
        ),
        before
    );
    assert_eq!(runtime.judge().stable_hash().unwrap(), judge_hash);
    pcm.extend(render_parts(&mut mixer, &[2, 4]));
    assert_eq!(
        pcm,
        [
            0.125, -0.125, 0.25, -0.25, 0.5, -0.5, 0.75, -0.75, 0.375, -0.375, 0.5, -0.5, 0.0, 0.0,
            0.0, 0.0
        ]
    );
    assert_eq!(mixer.frame_cursor(), 8);
    assert_eq!(mixer.playback_frame_cursor(), 8);
    assert_eq!(
        mixer.counters(),
        AudioCounters {
            rendered_frames: 8,
            commands_consumed: 2,
            commands_applied: 2,
            ..AudioCounters::default()
        }
    );
    assert_eq!(
        runtime.telemetry().counters(),
        RuntimeCounters {
            inputs: 3,
            judge_results: 3,
            audio_commands: 2,
            queue_full: 1,
            ..RuntimeCounters::default()
        }
    );
}

#[derive(Debug)]
struct Fault(Box<u32>);
struct SoftwareOwner {
    mixer: Option<Mixer>,
    identity: Box<u32>,
    retired: bool,
    drops: Rc<Cell<usize>>,
    unretired_drops: Rc<Cell<usize>>,
}
impl Drop for SoftwareOwner {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        if !self.retired {
            self.unretired_drops.set(self.unretired_drops.get() + 1);
        }
    }
}
impl StoppedMixerSource for SoftwareOwner {
    type Error = Fault;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Fault> {
        if !self.retired {
            return Err(Fault(Box::new(2)));
        }
        Ok(self.mixer.take())
    }
}

fn active_with_future() -> (Runtime, Mixer) {
    let (mut runtime, mut mixer) = fixture();
    hit(&mut runtime, 1, 0, None);
    hit(&mut runtime, 2, 1_000_000, Some(QueuePushError::Full));
    assert_eq!(render_parts(&mut mixer, &[2]), [0.125, 0.25]);
    hit(&mut runtime, 3, 4_000_000, None);
    let mut output = [91.0];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.375]);
    assert_eq!(report.active_voices, 1);
    assert_eq!(report.pending_commands, 1);
    assert_eq!(report.counters.commands_consumed, 2);
    (runtime, mixer)
}

#[test]
fn pending_output_refusals_recover_original_pcm_once_and_mixer_drop_disconnects_runtime() {
    for parts in [&[4, 4][..], &[1, 2, 1, 2, 2][..]] {
        for _ in 0..3 {
            let (mut runtime, mixer) = active_with_future();
            let (mut reference_runtime, mut reference) = active_with_future();
            let hash = runtime.judge().stable_hash().unwrap();
            let counters = runtime.telemetry().counters();
            let basis = mixer.output_frame_basis();
            let mixer_counters = mixer.counters();
            let drops = Rc::new(Cell::new(0));
            let unretired_drops = Rc::new(Cell::new(0));
            let owner = SoftwareOwner {
                mixer: Some(mixer),
                identity: Box::new(71),
                retired: false,
                drops: drops.clone(),
                unretired_drops: unretired_drops.clone(),
            };
            let owner_identity = owner.identity.as_ref() as *const u32;
            let error = Fault(Box::new(17));
            let error_identity = error.0.as_ref() as *const u32;
            let mut failure = OutputOpenFailure::pending(error, owner);
            for _ in 0..2 {
                assert!(failure
                    .retry_retirement(StoppedMixerSource::take_stopped_mixer)
                    .is_err());
                assert_eq!(failure.error().0.as_ref() as *const u32, error_identity);
                assert_eq!(failure.cleanup_error().unwrap().0.as_ref(), &2);
                assert!(failure.mixer().is_none());
                let pending = failure.pending_owner().unwrap();
                assert_eq!(pending.identity.as_ref() as *const u32, owner_identity);
                assert_eq!(pending.mixer.as_ref().unwrap().output_frame_basis(), basis);
                assert_eq!(pending.mixer.as_ref().unwrap().counters(), mixer_counters);
                assert_eq!(drops.get(), 0);
                assert_eq!(unretired_drops.get(), 0);
                assert_eq!(runtime.judge().stable_hash().unwrap(), hash);
                assert_eq!(runtime.telemetry().counters(), counters);
            }
            // The existing producer remains usable while its original consumer
            // and pending voice state await software retirement.
            hit(&mut runtime, 4, 6_000_000, None);
            hit(&mut reference_runtime, 4, 6_000_000, None);
            let accepted_hash = runtime.judge().stable_hash().unwrap();
            assert_eq!(
                accepted_hash,
                reference_runtime.judge().stable_hash().unwrap()
            );
            assert!(failure
                .retry_retirement(|pending| {
                    pending.retired = true;
                    pending.take_stopped_mixer()
                })
                .unwrap());
            assert_eq!(drops.get(), 1);
            assert_eq!(unretired_drops.get(), 0);
            assert!(failure.pending_owner().is_none());
            assert!(failure.cleanup_error().is_none());
            assert_eq!(failure.mixer().unwrap().output_frame_basis(), basis);
            assert!(!failure
                .retry_retirement(|_| panic!("retirement must move once"))
                .unwrap());
            let (error, recovered, pending, cleanup) = failure.into_parts();
            assert_eq!(error.0.as_ref() as *const u32, error_identity);
            assert!(pending.is_none());
            assert!(cleanup.is_none());
            let mut recovered = recovered.unwrap();
            refuse_large_render(&mut recovered);
            let actual = render_parts(&mut recovered, parts);
            let uninterrupted = render_parts(&mut reference, &[2, 4, 2]);
            assert_eq!(actual, [0.5, 0.125, 0.25, 0.5, 0.75, 0.375, 0.5, 0.0]);
            assert_eq!(actual, uninterrupted);
            assert_eq!(
                recovered.counters(),
                AudioCounters {
                    rendered_frames: 11,
                    commands_consumed: 3,
                    commands_applied: 3,
                    ..AudioCounters::default()
                }
            );
            assert_eq!(recovered.counters(), reference.counters());
            assert_eq!(recovered.frame_cursor(), 11);
            assert_eq!(recovered.playback_frame_cursor(), 11);
            assert_eq!(recovered.rate(), Rate::NORMAL);
            assert_eq!(runtime.judge().stable_hash().unwrap(), accepted_hash);
            assert_eq!(
                runtime.telemetry().counters(),
                RuntimeCounters {
                    inputs: 4,
                    judge_results: 4,
                    audio_commands: 3,
                    queue_full: 1,
                    ..RuntimeCounters::default()
                }
            );
            drop(recovered); // Actual consumer destruction, with Runtime retained.
            hit(
                &mut runtime,
                5,
                11_000_000,
                Some(QueuePushError::Disconnected),
            );
            assert_eq!(
                runtime.telemetry().counters(),
                RuntimeCounters {
                    inputs: 5,
                    judge_results: 5,
                    audio_commands: 3,
                    queue_full: 1,
                    queue_disconnected: 1,
                    ..RuntimeCounters::default()
                }
            );
            assert_ne!(runtime.judge().stable_hash().unwrap(), accepted_hash);
            assert_eq!(drops.get(), 1);
            assert_eq!(unretired_drops.get(), 0);
        }
    }
}
