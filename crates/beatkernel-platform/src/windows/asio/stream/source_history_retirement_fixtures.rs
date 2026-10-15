//! SDK-gated types, memory-backed real callback writes; no physical driver proof.
use super::*;
use crate::audio::asio::AsioPcmEncoding;
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

struct Owner {
    stream: AsioStream,
    _planes: [Box<[u8]>; 2],
    producer: CommandProducer,
}
fn owner() -> Owner {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(256, 256, 1).unwrap();
    let bank = SampleBank::new(format, limits).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(1),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 32, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(4).unwrap();
    let basis = mixer.output_frame_basis();
    let telemetry = Telemetry::with_source_history_and_retirement(
        mixer.source_playback_history().unwrap(),
        basis,
    )
    .unwrap();
    let renderer = AsioBlockRenderer::new(mixer, 2, vec![AsioPcmEncoding::Float32Lsb]).unwrap();
    let mut planes = [
        vec![0xa5; 8].into_boxed_slice(),
        vec![0xa5; 8].into_boxed_slice(),
    ];
    let mut rows = [RawOutput::default(); 32];
    rows[0] = RawOutput {
        channel: 0,
        sample_type: 19,
        width: 4,
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
    Owner {
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
            sample_rate: 1000,
            buffer_frames: 2,
            source_facts: None,
            native_release_unproved: false,
            latest_cleanup: None,
            failed_start: false,
        },
        _planes: planes,
        producer,
    }
}
impl Owner {
    fn fill(&mut self) -> Result<(), AsioRenderError> {
        // SAFETY: exclusively borrowed owner; both disjoint planes remain alive
        // and exact throughout this synchronous memory-backed invocation.
        unsafe { self.stream.context.fill(0, None) }
    }
}
fn query(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 1000).unwrap(), 0, 0, 1, 0, 1)
        .unwrap()
}

#[test]
fn request_changes_no_render_then_actual_callback_compacts_original_history() {
    let mut owner = owner();
    owner.fill().unwrap();
    owner.producer.request_pause(true);
    owner.fill().unwrap();
    owner.producer.request_pause(false);
    owner.fill().unwrap();
    let basis = owner.stream.frame_basis();
    let before = owner
        .stream
        .source_playback_view_at(&query(0), 1000, 0, 0)
        .unwrap();
    assert_eq!(
        owner
            .stream
            .request_source_history_retirement(basis, (1000, 0, 0), 4)
            .unwrap(),
        Some(2)
    );
    let after_request = owner
        .stream
        .source_playback_view_at(&query(0), 1000, 0, 0)
        .unwrap();
    assert_eq!(before.revision, after_request.revision);
    assert_eq!(before.snapshot.render, after_request.snapshot.render);
    owner.fill().unwrap();
    assert!(owner
        .stream
        .source_playback_view_at(&query(0), 1000, 0, 0)
        .is_none());
    assert!(owner
        .stream
        .source_playback_view_at(&query(4), 1000, 0, 0)
        .is_some());
    assert_eq!(owner.stream.frame_basis(), basis);
    // SAFETY: exclusively held memory owner, no driver callback exists.
    let state = unsafe { &*owner.stream.context.state.get() };
    assert_eq!(state.source_retirement_completed, 2);
    assert_eq!(state.prepared_frames, 8);
}

#[test]
fn wrong_identity_fault_and_invalid_destination_do_not_apply_queued_intent() {
    let mut owner = owner();
    owner.fill().unwrap();
    let basis = owner.stream.frame_basis();
    assert!(owner
        .stream
        .request_source_history_retirement(basis, (999, 0, 0), 1)
        .is_err());
    assert!(owner
        .stream
        .request_source_history_retirement(basis, (1000, 1, 0), 1)
        .is_err());
    assert!(owner
        .stream
        .request_source_history_retirement(basis, (1000, 0, 0), 3)
        .is_err());
    owner.stream.native.render_error = 2;
    assert!(owner
        .stream
        .request_source_history_retirement(basis, (1000, 0, 0), 1)
        .is_err());
    owner.stream.native.render_error = 0;
    owner
        .stream
        .request_source_history_retirement(basis, (1000, 0, 0), 1)
        .unwrap()
        .unwrap();
    // Invalid index is rejected before touching any borrowed native region.
    assert_eq!(
        unsafe { owner.stream.context.fill(2, None) },
        Err(AsioRenderError::InvalidBuffers)
    );
    // SAFETY: exclusive memory owner without callbacks.
    let state = unsafe { &*owner.stream.context.state.get() };
    assert_eq!(state.source_retirement_completed, 0);
    assert_eq!(state.prepared_frames, 2);
}
