//! Pure Rust memory-backed publication fixtures; no SDK/registry/foreign calls.
use super::*;
use crate::audio::asio::{AsioPcmEncoding, AsioPcmError};
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
    let format = AudioFormat::new(4, 2).unwrap();
    let limits = PcmLimits::new(256, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, samples.to_vec(), limits).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(4).unwrap();
    let mixer = Mixer::new(
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
            state: UnsafeCell::new(RenderState {
                renderer,
                rows,
                prepared_frames: 0,
                buffer_fills: 0,
                version: 0,
            }),
            telemetry: Telemetry::new(),
            publication: AtomicU64::new(0),
            event_values: std::array::from_fn(|_| AtomicU64::new(0)),
        },
        planes,
        producer,
    }
}
impl MemoryRig {
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
fn nonfinite_delivery_clears_old_observation_retains_actual_mixer_report_and_prepared_counts() {
    let mut memory = rig(&[0.25, f32::MAX], 1);
    memory.play(0, 0.0);
    memory.fill(0, Some(event(0, 1))).unwrap();
    assert!(memory.context.read().1.is_some());
    memory.play(250_000_000, 2.0);
    assert!(matches!(
        memory.fill(1, Some(event(1, 2))),
        Err(AsioRenderError::Pcm(AsioPcmError::NonFiniteSample))
    ));
    assert!(memory.planes[1].iter().all(|&value| value == 0xa5));
    assert!(memory.planes[3].iter().all(|&value| value == 0xa5));
    let (software, observation) = memory.context.read();
    assert!(software.telemetry_available);
    assert!(observation.is_none());
    assert!(matches!(software.status, AudioStreamStatus::Failed { .. }));
    assert_eq!(
        (
            software.counters.submitted_frames,
            software.counters.buffer_fills
        ),
        (1, 1)
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
    assert_eq!(
        memory.context.state.get_mut().renderer.last_render_report(),
        None
    );
    assert!(memory
        .planes
        .iter()
        .all(|plane| plane.iter().all(|&value| value == 0xa5)));
    assert_eq!(memory.context.publication.load(Ordering::SeqCst), 0);
    // Test-only replacement of unpublished counter state proves the failed call
    // never consumed the already queued command or advanced the owned Mixer.
    memory.context.state.get_mut().version = 0;
    memory.fill(0, Some(event(0, 1))).unwrap();
    let report = memory.context.read().0.render.unwrap();
    assert_eq!(
        (
            report.start_frame,
            report.frames,
            report.counters.commands_consumed
        ),
        (0, 1, 1)
    );
    assert_eq!(&*memory.planes[0], &[0, 0x40]);
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
