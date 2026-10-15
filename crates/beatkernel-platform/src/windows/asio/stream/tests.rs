//! Pure Rust memory-backed publication fixtures; no SDK/registry/foreign calls.
use super::*;
use crate::audio::asio::AsioPcmEncoding;
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

struct MemoryRig {
    // Context is dropped before the owned regions its rows reference.
    context: RenderContext,
    planes: [Box<[u8]>; 4],
    producer: CommandProducer,
}
fn rig(samples: &[f32], frames: u32) -> MemoryRig {
    rig_with_history(samples, frames, None)
}
fn rig_with_history(samples: &[f32], frames: u32, history_capacity: Option<usize>) -> MemoryRig {
    let format = AudioFormat::new(4, 2).unwrap();
    let limits = PcmLimits::new(256, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, samples.to_vec(), limits).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(4).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(4, 2, 4, 16, 4).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    if let Some(capacity) = history_capacity {
        mixer.enable_source_playback_history(capacity).unwrap();
    }
    let telemetry = match mixer.source_playback_history() {
        Some(history) => Telemetry::with_source_history(history).unwrap(),
        None => Telemetry::new(),
    };
    let encodings = vec![AsioPcmEncoding::Int16Lsb, AsioPcmEncoding::Float32Msb];
    let renderer = AsioBlockRenderer::new(mixer, frames, encodings).unwrap();
    let mut planes: [Box<[u8]>; 4] = std::array::from_fn(|index| {
        vec![0xa5; frames as usize * if index < 2 { 2 } else { 4 }].into_boxed_slice()
    });
    let mut rows = [RawOutput::default(); 32];
    rows[0] = RawOutput {
        channel: 0,
        sample_type: 16,
        width: 2,
        buffer0: planes[0].as_mut_ptr().cast(),
        buffer1: planes[1].as_mut_ptr().cast(),
    };
    rows[1] = RawOutput {
        channel: 1,
        sample_type: 3,
        width: 4,
        buffer0: planes[2].as_mut_ptr().cast(),
        buffer1: planes[3].as_mut_ptr().cast(),
    };
    MemoryRig {
        context: RenderContext {
            host_clock: None,
            cadence: crate::audio::cadence::Capture::new(),
            state: UnsafeCell::new(RenderState {
                renderer: RenderOwner::Legacy(renderer),
                frames,
                permit: None,
                last_report: None,
                rows,
                prepared_frames: 0,
                buffer_fills: 0,
                version: 0,
                source_retirement_completed: 0,
            }),
            telemetry,
            publication: AtomicU64::new(0),
            event_values: std::array::from_fn(|_| AtomicU64::new(0)),
            source_control: Arc::new(SourceControl::new()),
        },
        planes,
        producer,
    }
}
impl MemoryRig {
    fn legacy_mut(&mut self) -> &mut AsioBlockRenderer {
        match &mut self.context.state.get_mut().renderer {
            RenderOwner::Legacy(renderer) => renderer,
            RenderOwner::Complete(_) => panic!("legacy fixture keeps its actual renderer"),
        }
    }
    fn fill(
        &mut self,
        index: i32,
        event: Option<AsioCallbackEvent>,
    ) -> Result<(), AsioRenderError> {
        // SAFETY: exclusive rig access excludes all other fills/plane accesses.
        // All four Box regions are live, exactly sized, aligned for bytes,
        // representable and disjoint; they remain allocated through this call.
        unsafe { self.context.fill(index, event) }
    }
    fn play(&mut self, at: i64, gain: f32) {
        self.producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::from_nanos(at),
                gain,
            })
            .unwrap();
    }
}
fn event(index: i32, ordinal: u64) -> AsioCallbackEvent {
    AsioCallbackEvent {
        buffer_index: index,
        direct_process: -7,
        flags: 7,
        sample_position: 1000 + ordinal * 2,
        system_nanoseconds: 8000 + ordinal * 13,
        sample_rate: 48_000.25,
    }
}

#[test]
fn clock_free_memory_rendering_never_fabricates_cadence_from_native_time() {
    let mut memory = rig(&[0.5, -0.5, 0.25, 0.75], 2);
    memory.play(0, 1.0);
    memory.fill(1, None).unwrap();
    memory.fill(0, Some(event(0, 1))).unwrap();
    assert!(memory.context.host_clock.is_none());
    let summary = memory.context.cadence.summary(4).unwrap();
    assert_eq!(summary.successful_renders, 0);
    assert_eq!(summary.retained_points, 0);
    assert!(summary.intervals.is_none());
}

#[test]
fn prestart_buffer_b_has_actual_pcm_and_software_report_but_no_callback_observation() {
    let mut memory = rig(&[0.5, -0.5, 0.25, 0.75], 2);
    memory.play(0, 1.0);
    assert!(!memory.context.read().0.telemetry_available);
    memory.fill(1, None).unwrap();
    assert_eq!(&*memory.planes[1], &[0, 0x40, 0, 0x20]);
    assert_eq!(&*memory.planes[3], &[0xbf, 0, 0, 0, 0x3f, 0x40, 0, 0]);
    assert!(memory.planes[0].iter().all(|&value| value == 0xa5));
    assert!(memory.planes[2].iter().all(|&value| value == 0xa5));
    let (software, observation) = memory.context.read();
    assert!(software.telemetry_available);
    assert_eq!(
        (
            software.counters.submitted_frames,
            software.counters.buffer_fills
        ),
        (2, 1)
    );
    assert!(software.clock.is_none());
    assert!(observation.is_none());
    let report = software.render.unwrap();
    assert_eq!(
        (
            report.start_frame,
            report.frames,
            report.counters.commands_applied
        ),
        (0, 2, 1)
    );
}

#[test]
fn copied_callback_event_and_actual_report_belong_to_same_completed_buffer_write() {
    let mut memory = rig(&[0.5, -0.5, 0.25, 0.75], 2);
    memory.play(0, 1.0);
    let mut supplied = event(0, 9);
    let original = supplied;
    memory.fill(0, Some(supplied)).unwrap();
    supplied.sample_position = 999_999; // Caller storage may change after the copy.
    let (software, observation) = memory.context.read();
    let observation = observation.unwrap();
    assert_eq!(observation.event, original);
    assert_ne!(observation.event.sample_position, supplied.sample_position);
    assert_eq!(Some(observation.render), software.render);
    assert_eq!(
        (
            software.counters.submitted_frames,
            software.counters.buffer_fills
        ),
        (2, 1)
    );
    assert_eq!(&*memory.planes[0], &[0, 0x40, 0, 0x20]);
    assert_eq!(&*memory.planes[2], &[0xbf, 0, 0, 0, 0x3f, 0x40, 0, 0]);
    memory.fill(1, Some(event(1, 10))).unwrap();
    let (next, observation) = memory.context.read();
    let observation = observation.unwrap();
    assert_eq!(observation.event, event(1, 10));
    assert_eq!(
        (observation.render.start_frame, observation.render.frames),
        (2, 2)
    );
    assert_eq!(
        (next.counters.submitted_frames, next.counters.buffer_fills),
        (4, 2)
    );
}

#[test]
fn large_finite_sample_and_gain_are_clipped_before_successful_native_encoding() {
    let mut memory = rig(&[0.25, f32::MAX], 1);
    memory.play(0, 0.0);
    memory.fill(0, Some(event(0, 1))).unwrap();
    assert!(memory.context.read().1.is_some());
    memory.play(250_000_000, 2.0);
    memory.fill(1, Some(event(1, 2))).unwrap();
    assert_eq!(&*memory.planes[1], &[0, 0x40]);
    assert_eq!(&*memory.planes[3], &[0x3f, 0x80, 0, 0]);
    let (software, observation) = memory.context.read();
    assert!(software.telemetry_available);
    assert_eq!(observation.unwrap().event, event(1, 2));
    assert_eq!(software.status, AudioStreamStatus::Ready);
    assert_eq!(
        (
            software.counters.submitted_frames,
            software.counters.buffer_fills
        ),
        (2, 2)
    );
    let actual = software.render.unwrap();
    assert_eq!(
        (
            actual.start_frame,
            actual.frames,
            actual.counters.commands_consumed,
            actual.counters.commands_applied
        ),
        (1, 1, 2, 2)
    );
    assert_eq!(actual.counters.invalid_gains, 0);
}

#[test]
fn exhausted_publication_generation_fails_before_queue_or_mixer_mutation() {
    let mut memory = rig(&[0.5, -0.5], 1);
    memory.play(0, 1.0);
    memory.context.state.get_mut().version = u64::MAX - 1;
    assert!(matches!(
        memory.fill(0, Some(event(0, 1))),
        Err(AsioRenderError::Capacity)
    ));
    assert_eq!(memory.legacy_mut().last_render_report(), None);
    assert!(memory
        .planes
        .iter()
        .all(|plane| plane.iter().all(|&value| value == 0xa5)));
    assert_eq!(memory.context.publication.load(Ordering::SeqCst), u64::MAX);
    // Exhaustion is permanent even if a fixture resets private writer state.
    memory.context.state.get_mut().version = 0;
    assert_eq!(
        memory.fill(0, Some(event(0, 1))),
        Err(AsioRenderError::Capacity)
    );
    assert!(!memory.context.read().0.telemetry_available);
    // Actual cold owner recovery proves no queued command was consumed.
    let mut mixer = memory.legacy_mut().take_mixer().unwrap();
    let mut output = [0.0; 2];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(
        (
            report.start_frame,
            report.frames,
            report.counters.commands_consumed
        ),
        (0, 1, 1)
    );
    assert_eq!(output, [0.5, -0.5]);
}

#[test]
fn sole_rust_publisher_and_bounded_concurrent_reader_never_mix_event_and_report_generations() {
    const FILLS: u64 = 256;
    let memory = rig(&[], 2);
    let context = &memory.context;
    let mut coherent_reads = 0usize;
    std::thread::scope(|scope| {
        let publisher = scope.spawn(|| {
            for ordinal in 0..FILLS {
                let index = (ordinal & 1) as i32;
                // SAFETY: this thread is the only fill caller. Main reads only
                // atomic publication and never state or planes until join. Owned
                // disjoint Boxes outlive the scope; borrowed context is not moved.
                unsafe {
                    context.fill(index, Some(event(index, ordinal))).unwrap();
                }
            }
        });
        for _ in 0..4096 {
            let (software, observation) = context.read();
            if let Some(observation) = observation {
                coherent_reads += 1;
                let ordinal = observation.render.start_frame / 2;
                assert_eq!(observation.event, event((ordinal & 1) as i32, ordinal));
                assert_eq!(Some(observation.render), software.render);
                assert_eq!(
                    software.counters.submitted_frames,
                    observation.render.start_frame + 2
                );
                assert_eq!(software.counters.buffer_fills, ordinal + 1);
            }
        }
        publisher.join().unwrap();
    });
    // Independent final assertion prevents a scheduler that produces no
    // concurrent coherent reads from making the publication fixture vacuous.
    let (software, observation) = context.read();
    let final_observation = observation.expect("completed sole publisher must be observable");
    assert!(software.telemetry_available);
    assert_eq!(final_observation.event, event(1, FILLS - 1));
    assert_eq!(
        (
            final_observation.render.start_frame,
            final_observation.render.frames
        ),
        (510, 2)
    );
    assert_eq!(
        (
            software.counters.submitted_frames,
            software.counters.buffer_fills
        ),
        (512, 256)
    );
    assert_eq!(Some(final_observation.render), software.render);
    std::hint::black_box(coherent_reads);
}

fn source_query(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(
        TargetTime::new(frame / 4, frame % 4, 4).unwrap(),
        0,
        0,
        1,
        0,
        1,
    )
    .unwrap()
}

#[test]
fn actual_priming_history_has_software_facts_but_no_copied_native_observation() {
    let mut memory = rig_with_history(&[0.5, -0.5, 0.25, 0.75], 2, Some(8));
    memory.play(0, 1.0);
    assert!(memory
        .context
        .read_source_playback_at(&source_query(0), 4, 0, 0)
        .is_none());
    memory.fill(1, None).unwrap();
    let (source, observation, revision) = memory
        .context
        .read_source_playback_at(&source_query(2), 4, 0, 0)
        .unwrap();
    assert_eq!(revision, memory.context.publication.load(Ordering::SeqCst));
    assert_eq!(revision & 1, 0);
    assert_eq!(source.pulled_source_end, 2);
    assert_eq!(
        source.playback,
        SourcePlaybackSpan::new(0, None, 0, SourcePlaybackState::Running).unwrap()
    );
    assert_eq!(source.snapshot.counters.submitted_frames, 2); // Internal software count only.
    assert_eq!(source.snapshot.render.unwrap().start_frame, 0);
    assert!(observation.is_none());
    assert!(source.snapshot.clock.is_none());
    let future = InterpolatedTargetTime::new(
        TargetTime::new(0, 1, 2).unwrap(),
        0,
        1,
        u64::MAX,
        1,
        u64::MAX - 1,
    )
    .unwrap();
    assert!(memory
        .context
        .read_source_playback_at(&future, 4, 0, 0)
        .is_none());
    for identity in [(5, 0, 0), (4, 1, 0), (4, 0, 1)] {
        assert!(memory
            .context
            .read_source_playback_at(&source_query(0), identity.0, identity.1, identity.2)
            .is_none());
    }
}

#[test]
fn actual_callback_pause_resume_history_report_and_event_share_one_outer_revision() {
    let mut memory = rig_with_history(&[], 2, Some(8));
    memory.fill(0, Some(event(0, 0))).unwrap();
    memory.producer.request_pause(true);
    memory.fill(1, Some(event(1, 1))).unwrap();
    memory.producer.request_pause(false);
    memory.fill(0, Some(event(0, 2))).unwrap();
    for (frame, physical, playback, state) in [
        (0, 0, 0, SourcePlaybackState::Running),
        (2, 2, 2, SourcePlaybackState::Frozen),
        (4, 4, 2, SourcePlaybackState::Running),
        (6, 4, 2, SourcePlaybackState::Running),
    ] {
        let (source, observation, revision) = memory
            .context
            .read_source_playback_at(&source_query(frame), 4, 0, 0)
            .unwrap();
        assert_eq!(revision, 6);
        assert_eq!(source.pulled_source_end, 6);
        assert_eq!(
            (
                source.playback.physical_start(),
                source.playback.playback_start(),
                source.playback.state()
            ),
            (physical, playback, state)
        );
        let observation = observation.unwrap();
        assert_eq!(observation.event, event(0, 2));
        assert_eq!(Some(observation.render), source.snapshot.render);
        assert_eq!(
            (
                observation.render.start_frame,
                source.snapshot.counters.submitted_frames,
                source.snapshot.counters.buffer_fills
            ),
            (4, 6, 3)
        );
    }
}

#[test]
fn actual_history_capacity_error_clears_event_and_keeps_prepared_prefix_and_source_facts() {
    let mut memory = rig_with_history(&[], 2, Some(1));
    memory.fill(0, Some(event(0, 0))).unwrap();
    let previous = memory
        .context
        .read_source_playback_at(&source_query(2), 4, 0, 0)
        .unwrap()
        .0;
    memory.producer.request_pause(true);
    assert_eq!(
        memory.fill(1, Some(event(1, 1))),
        Err(AsioRenderError::Core(AudioError::RenderCapacity))
    );
    let (source, observation, revision) = memory
        .context
        .read_source_playback_at(&source_query(2), 4, 0, 0)
        .unwrap();
    assert_eq!(revision, 4);
    assert!(observation.is_none());
    assert_eq!(source.playback, previous.playback);
    assert_eq!(source.pulled_source_end, previous.pulled_source_end);
    assert_eq!(source.snapshot.render, previous.snapshot.render);
    assert_eq!(
        (
            source.snapshot.counters.submitted_frames,
            source.snapshot.counters.buffer_fills
        ),
        (2, 1)
    );
    assert!(matches!(
        source.snapshot.status,
        AudioStreamStatus::Failed { .. }
    ));
    assert!(memory.planes[1].iter().all(|&byte| byte == 0xa5));
    assert!(memory.planes[3].iter().all(|&byte| byte == 0xa5));
}

#[test]
fn default_source_disabled_and_odd_or_exhausted_outer_revision_refuse_compound_proof() {
    let mut legacy = rig(&[], 2);
    legacy.fill(0, Some(event(0, 0))).unwrap();
    assert!(legacy.context.read().1.is_some());
    assert!(legacy
        .context
        .read_source_playback_at(&source_query(0), 4, 0, 0)
        .is_none());
    let mut memory = rig_with_history(&[], 2, Some(8));
    memory.fill(0, Some(event(0, 0))).unwrap();
    memory.context.publication.store(3, Ordering::SeqCst);
    assert!(memory
        .context
        .read_source_playback_at(&source_query(0), 4, 0, 0)
        .is_none());
    memory.context.publication.store(2, Ordering::SeqCst);
    memory.context.state.get_mut().version = u64::MAX - 1;
    assert_eq!(
        memory.fill(1, Some(event(1, 1))),
        Err(AsioRenderError::Capacity)
    );
    assert_eq!(memory.context.publication.load(Ordering::SeqCst), u64::MAX);
    assert!(memory
        .context
        .read_source_playback_at(&source_query(0), 4, 0, 0)
        .is_none());
    assert_eq!(
        memory
            .legacy_mut()
            .source_playback_history()
            .unwrap()
            .pulled_end_frame(),
        2
    );
    assert!(memory.planes[1].iter().all(|&byte| byte == 0xa5));
    assert!(memory.planes[3].iter().all(|&byte| byte == 0xa5));
}

#[test]
fn source_candidate_is_discarded_after_exactly_three_changed_outer_generations() {
    let mut memory = rig_with_history(&[], 2, Some(8));
    memory.fill(0, Some(event(0, 0))).unwrap();
    let mut attempts = 0;
    let result = memory
        .context
        .read_source_playback_with(&source_query(0), 4, 0, 0, |attempt| {
            assert_eq!(attempt, attempts);
            attempts += 1;
            memory.context.publication.fetch_add(2, Ordering::SeqCst);
        });
    assert!(result.is_none());
    assert_eq!(attempts, 3);
    // Only the fixture changes the version; the actual completed payload remains.
    let (source, observation, revision) = memory
        .context
        .read_source_playback_at(&source_query(0), 4, 0, 0)
        .unwrap();
    assert_eq!(revision, 8);
    assert_eq!(source.pulled_source_end, 2);
    assert_eq!(observation.unwrap().event, event(0, 0));
}

#[test]
fn actual_sole_callback_writer_never_mixes_source_frontier_report_or_copied_event() {
    const FILLS: u64 = 128;
    let memory = rig_with_history(&[], 2, Some(8));
    let context = &memory.context;
    std::thread::scope(|scope| {
        let publisher = scope.spawn(|| {
            for ordinal in 0..FILLS {
                let index = (ordinal & 1) as i32;
                // SAFETY: Sole callback writer; stable disjoint planes outlive scope.
                // Reader accesses only atomic publications, never renderer or planes.
                unsafe {
                    context.fill(index, Some(event(index, ordinal))).unwrap();
                }
            }
        });
        for _ in 0..2048 {
            if let Some((source, Some(observation), revision)) =
                context.read_source_playback_at(&source_query(0), 4, 0, 0)
            {
                let ordinal = observation.render.start_frame / 2;
                assert_eq!(revision, 2 * (ordinal + 1));
                assert_eq!(observation.event, event((ordinal & 1) as i32, ordinal));
                assert_eq!(Some(observation.render), source.snapshot.render);
                assert_eq!(source.pulled_source_end, observation.render.start_frame + 2);
                assert_eq!(
                    source.snapshot.counters.submitted_frames,
                    source.pulled_source_end
                );
                assert_eq!(source.snapshot.counters.buffer_fills, ordinal + 1);
            }
        }
        publisher.join().unwrap();
    });
    // Final nonvacuous assertion regardless of concurrent scheduling.
    let (source, observation, revision) = context
        .read_source_playback_at(&source_query(0), 4, 0, 0)
        .unwrap();
    assert_eq!(revision, 2 * FILLS);
    assert_eq!(source.pulled_source_end, 2 * FILLS);
    assert_eq!(source.snapshot.render.unwrap().start_frame, 2 * (FILLS - 1));
    assert_eq!(observation.unwrap().event, event(1, FILLS - 1));
}
