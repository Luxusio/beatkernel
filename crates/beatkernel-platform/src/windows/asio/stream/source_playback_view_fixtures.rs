//! Actual memory-backed callback writes; no driver/control/SDK operation.
use super::*;
use crate::audio::asio::AsioPcmEncoding;
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

struct MemoryOwner {
    // Owner is dropped before its referenced planar regions.
    stream: AsioStream,
    planes: [Box<[u8]>; 2],
    producer: CommandProducer,
}
fn owner(capacity: usize, advance: bool) -> MemoryOwner {
    let format = AudioFormat::new(4, 1).unwrap();
    let limits = PcmLimits::new(128, 128, 1).unwrap();
    let bank = SampleBank::new(format, limits).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::from_nanos(-123),
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    if advance {
        mixer.render(&mut [0.; 2]).unwrap();
        producer.request_pause(true);
        mixer.render(&mut [0.; 2]).unwrap();
        producer.request_pause(false);
    }
    mixer.enable_source_playback_history(capacity).unwrap();
    let basis = mixer.output_frame_basis();
    let telemetry =
        Telemetry::with_source_history(mixer.source_playback_history().unwrap()).unwrap();
    let renderer = AsioBlockRenderer::new(mixer, 2, vec![AsioPcmEncoding::Int16Lsb]).unwrap();
    let mut planes = [
        vec![0xa5; 4].into_boxed_slice(),
        vec![0xa5; 4].into_boxed_slice(),
    ];
    let mut rows = [RawOutput::default(); 32];
    rows[0] = RawOutput {
        channel: 0,
        sample_type: 16,
        width: 2,
        buffer0: planes[0].as_mut_ptr().cast(),
        buffer1: planes[1].as_mut_ptr().cast(),
    };
    let context = Box::new(RenderContext {
        host_clock: None,
        cadence: crate::audio::cadence::Capture::new(),
        state: UnsafeCell::new(RenderState {
            renderer: RenderOwner::Legacy(renderer),
            frames: 2,
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
    });
    MemoryOwner {
        stream: AsioStream {
            basis,
            control: None,
            retired: true,
            context: ManuallyDrop::new(context),
            phase: AsioStreamPhase::Ready,
            native: AsioDiagnostics::default(),
            latencies: AsioLatencies {
                input_frames: 0,
                output_frames: 0,
            },
            sample_rate: 4,
            buffer_frames: 2,
            source_facts: None,
            native_release_unproved: false,
            latest_cleanup: None,
            failed_start: false,
        },
        planes,
        producer,
    }
}
fn query(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 4).unwrap(), 0, 0, 1, 0, 1).unwrap()
}
fn callback(index: i32, ordinal: u64) -> AsioCallbackEvent {
    AsioCallbackEvent {
        buffer_index: index,
        direct_process: -7,
        flags: 7,
        sample_position: 1000 + ordinal * 2,
        system_nanoseconds: 8000 + ordinal * 13,
        sample_rate: 4.0,
    }
}
impl MemoryOwner {
    fn fill(
        &mut self,
        index: i32,
        event: Option<AsioCallbackEvent>,
    ) -> Result<(), AsioRenderError> {
        // SAFETY: exclusive owner, stable live disjoint exactly sized planar Boxes.
        unsafe { self.stream.context.fill(index, event) }
    }
}

#[test]
fn immutable_view_preserves_nonzero_original_source_header_and_successful_planar_write() {
    let mut memory = owner(8, true);
    assert_eq!(memory.stream.basis.start_physical_frame(), 4);
    assert!(memory
        .stream
        .source_playback_view_at(&query(4), 4, 4, 2)
        .is_none());
    memory.fill(1, Some(callback(1, 0))).unwrap();
    assert_eq!(&*memory.planes[1], &[0, 0, 0, 0]);
    assert_eq!(&*memory.planes[0], &[0xa5; 4]);
    let view = memory
        .stream
        .source_playback_view_at(&query(5), 4, 4, 2)
        .unwrap();
    assert_eq!(view.revision, 2);
    assert_eq!(view.pulled_source_end, 6);
    assert_eq!(
        view.playback,
        SourcePlaybackSpan::new(4, None, 2, SourcePlaybackState::Running).unwrap()
    );
    assert_eq!(view.snapshot.prepared_frames, 2);
    assert_eq!(view.snapshot.buffer_fills, 1);
    assert_eq!(
        view.snapshot.buffer_observation.unwrap().event,
        callback(1, 0)
    );
    assert_eq!(
        view.snapshot.render,
        Some(view.snapshot.buffer_observation.unwrap().render)
    );
    for (hz, physical, playback) in [(5, 4, 2), (4, 0, 2), (4, 4, 0)] {
        assert!(memory
            .stream
            .source_playback_view_at(&query(5), hz, physical, playback)
            .is_none());
    }
}

#[test]
fn priming_is_software_history_without_callback_or_native_permission_and_read_is_immutable() {
    let mut memory = owner(8, false);
    memory.fill(1, None).unwrap();
    // Deliberate stale diagnostics demonstrate preservation, not fresh SDK observation.
    memory.stream.native = AsioDiagnostics {
        faults: AsioFaults(8),
        render_error: 9,
        event: Some(callback(0, 99)),
    };
    let before = memory.stream.native;
    let view = memory
        .stream
        .source_playback_view_at(&query(1), 4, 0, 0)
        .unwrap();
    assert_eq!(view.snapshot.native, before);
    assert_eq!(view.snapshot.phase, AsioStreamPhase::Failed);
    assert_eq!(memory.stream.phase, AsioStreamPhase::Ready);
    assert!(view.snapshot.buffer_observation.is_none());
    assert!(memory.stream.control.is_none());
    assert_eq!(memory.stream.native, before);
    assert_eq!(memory.stream.context.publication.load(Ordering::SeqCst), 2);
}

#[test]
fn exact_fraction_past_prepared_prefix_refuses_even_when_display_nanoseconds_are_equal() {
    let mut memory = owner(8, false);
    memory.fill(0, Some(callback(0, 0))).unwrap();
    assert!(memory
        .stream
        .source_playback_view_at(&query(2), 4, 0, 0)
        .is_some());
    let future = InterpolatedTargetTime::new(
        TargetTime::from_frames(2, 4).unwrap(),
        0,
        1,
        u64::MAX,
        1,
        u64::MAX - 1,
    )
    .unwrap();
    assert!(memory
        .stream
        .source_playback_view_at(&future, 4, 0, 0)
        .is_none());
}

#[test]
fn failed_history_delivery_preserves_old_prefix_and_clears_callback_event() {
    let mut memory = owner(1, false);
    memory.fill(0, Some(callback(0, 0))).unwrap();
    let first = memory
        .stream
        .source_playback_view_at(&query(2), 4, 0, 0)
        .unwrap();
    memory.producer.request_pause(true);
    assert_eq!(
        memory.fill(1, Some(callback(1, 1))),
        Err(AsioRenderError::Core(AudioError::RenderCapacity))
    );
    let failed = memory
        .stream
        .source_playback_view_at(&query(2), 4, 0, 0)
        .unwrap();
    assert_eq!(failed.revision, 4);
    assert_eq!(failed.pulled_source_end, first.pulled_source_end);
    assert_eq!(failed.playback, first.playback);
    assert_eq!(failed.snapshot.prepared_frames, 2);
    assert_eq!(failed.snapshot.render, first.snapshot.render);
    assert!(failed.snapshot.buffer_observation.is_none());
    assert_eq!(&*memory.planes[1], &[0xa5; 4]);
}

#[test]
fn odd_zero_and_changed_publications_never_produce_a_compound_view() {
    let mut memory = owner(8, false);
    memory.fill(0, Some(callback(0, 0))).unwrap();
    for revision in [0, 3, u64::MAX] {
        memory
            .stream
            .context
            .publication
            .store(revision, Ordering::SeqCst);
        assert!(memory
            .stream
            .source_playback_view_at(&query(0), 4, 0, 0)
            .is_none());
    }
    memory.stream.context.publication.store(2, Ordering::SeqCst);
    let mut reads = 0;
    assert!(memory
        .stream
        .context
        .read_source_playback_with(&query(0), 4, 0, 0, |_| {
            reads += 1;
            memory
                .stream
                .context
                .publication
                .fetch_add(2, Ordering::SeqCst);
        })
        .is_none());
    assert_eq!(reads, 3);
}

#[test]
fn immutable_reader_and_sole_memory_callback_writer_keep_source_report_and_event_coherent() {
    const FILLS: u64 = 64;
    let memory = owner(8, false);
    let context = &memory.stream.context;
    std::thread::scope(|scope| {
        let writer = scope.spawn(|| {
            for ordinal in 0..FILLS {
                let index = (ordinal & 1) as i32;
                // SAFETY: sole writer; regions are live through join; reader touches only atomics.
                unsafe {
                    context.fill(index, Some(callback(index, ordinal))).unwrap();
                }
            }
        });
        for _ in 0..1024 {
            if let Some((source, Some(event), revision)) =
                context.read_source_playback_at(&query(0), 4, 0, 0)
            {
                let ordinal = event.render.start_frame / 2;
                assert_eq!(revision, 2 * (ordinal + 1));
                assert_eq!(event.event, callback((ordinal & 1) as i32, ordinal));
                assert_eq!(source.snapshot.render, Some(event.render));
                assert_eq!(source.pulled_source_end, event.render.start_frame + 2);
            }
        }
        writer.join().unwrap();
    });
    let final_view = memory
        .stream
        .source_playback_view_at(&query(0), 4, 0, 0)
        .unwrap();
    assert_eq!(final_view.revision, 2 * FILLS);
    assert_eq!(final_view.pulled_source_end, 2 * FILLS);
    assert_eq!(
        final_view.snapshot.buffer_observation.unwrap().event,
        callback(1, FILLS - 1)
    );
}
