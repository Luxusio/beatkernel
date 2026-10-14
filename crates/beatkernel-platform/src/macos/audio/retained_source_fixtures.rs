//! Production IOProc and lifecycle software fixtures using owned memory.
//! No HAL device is registered. MachClock construction is not device evidence;
//! compiling these fixtures with macOS C stubs does not execute them.
use super::*;
use crate::audio::{count_heap_calls, DeviceFormat, NativeOutputState, SampleEncoding};
use beatkernel::audio::{
    command_queue, AudioCommand, AudioLimits, CommandProducer, MixerConfig, PcmLimits, PcmSample,
    SampleBank, SampleId, TargetTime, VoiceId,
};
use std::sync::Arc;

fn owner() -> (CommandProducer, NativeOutputState) {
    let format = AudioFormat::new(1000, 2).unwrap();
    let limits = PcmLimits::new(128, 256, 2).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    let samples = (0..32)
        .flat_map(|frame| {
            let value = (frame % 8 + 1) as f32 / 16.;
            [value, -value / 2.]
        })
        .collect();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, samples, limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(123_000),
            gain: 1.,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::from_nanos(123_000),
            AudioLimits::new(16, 2, 16, 16, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    // History identity deliberately predates both the next native basis and
    // the pulled Mixer frontier once a partially admitted report is retained.
    mixer.render(&mut [0.; 4]).unwrap();
    mixer.enable_source_playback_history(2).unwrap();
    let state = NativeOutputState::new(
        mixer,
        DeviceFormat::new(
            format.sample_rate(),
            format.channels(),
            SampleEncoding::Float32,
            None,
        )
        .unwrap(),
        None,
        16,
    )
    .unwrap_or_else(|_| panic!("prepared real Mixer owner"));
    (producer, state)
}

fn request(frames: u32) -> CoreAudioRequest {
    CoreAudioRequest {
        device: 1,
        format: AudioFormat::new(1000, 2).unwrap(),
        buffer_frames: frames,
    }
}

fn stream(state: NativeOutputState, nominal: u32, maximum: u32, variable: bool) -> CoreAudioStream {
    stream_with_matrix(state, nominal, maximum, variable, None)
}

fn stream_with_matrix(
    mut state: NativeOutputState,
    nominal: u32,
    maximum: u32,
    variable: bool,
    matrix: Option<ChannelMatrix>,
) -> CoreAudioStream {
    let mut request = request(nominal);
    request.format = state.format().unwrap().pcm();
    let layout = vec![1; usize::from(request.format.channels())];
    let basis = state.output_frame_basis();
    let config = state.mixer().config();
    let source_facts = SourceRetirementFacts {
        source: config.format(),
        applied: request.format,
        matrix: matrix.clone(),
        render_limit: config.limits().max_render_frames(),
    };
    let telemetry =
        prepare_complete_state(&request, &mut state, matrix, maximum, variable, 4).unwrap();
    let control = Arc::new(SourceControl::new());
    let context = Box::new(Context {
        control: Arc::clone(&control),
        complete: true,
        render: UnsafeCell::new(RenderState {
            owner: RenderOwner::Complete(state),
            render_version: 0,
            publication_version: 0,
            completed_source_retirement: 0,
        }),
        render_telemetry: telemetry,
        cadence: Box::new(crate::audio::cadence::Capture::new()),
        clock: MachClock::new(ClockDomainId(8), ClockDomainId(9)).unwrap(),
        origin: config.origin(),
        output_domain: config.domain(),
        rate: request.format.sample_rate(),
        channels: request.format.channels(),
        layout: layout.clone(),
        buffer_frames: maximum,
        enabled: AtomicBool::new(true),
        rendering: AtomicBool::new(false),
        active: AtomicUsize::new(0),
        changed: AtomicBool::new(false),
        calls: AtomicU64::new(0),
        frames: AtomicU64::new(0),
        failures: AtomicU64::new(0),
        version: AtomicU64::new(0),
        delivered: AtomicBool::new(false),
        delivered_end: AtomicU64::new(0),
        ticks: AtomicU64::new(0),
        sample_bits: AtomicU64::new(0),
        flags: AtomicU32::new(0),
        first_frame: AtomicU64::new(0),
        block_frames: AtomicU32::new(0),
    });
    CoreAudioStream {
        basis,
        applied: CoreAudioApplied {
            request,
            format: request.format,
            buffer_frames: nominal,
            buffer_channels: layout,
            output_domain: config.domain(),
            output_origin: config.origin(),
            native_clock: ClockDomainId(8),
        },
        context: Some(context),
        recovered_owner: None,
        control,
        source_facts: Some(source_facts),
        variable_buffers: variable,
        max_callback_frames: maximum,
        stop_required: false,
        failed_start: false,
        retired: false,
        proc_id: std::ptr::null_mut(),
        listeners: Vec::new(),
        started: false,
        owner: PhantomData,
        final_snapshot: CoreAudioSnapshot::default(),
        final_render_report: None,
        final_cadence: None,
    }
}

fn state(stream: &CoreAudioStream) -> &NativeOutputState {
    // SAFETY: these fixtures never register with HAL and inspect only between
    // synchronous production callback invocations.
    let render = unsafe { &*stream.context.as_ref().unwrap().render.get() };
    let RenderOwner::Complete(state) = &render.owner else {
        panic!("complete owner must retain its variant")
    };
    state
}

fn mutate_state(stream: &mut CoreAudioStream, f: impl FnOnce(&mut NativeOutputState)) {
    // SAFETY: same sole-owner, unregistered callback-memory fixture contract.
    let render = unsafe { &mut *stream.context.as_ref().unwrap().render.get() };
    let RenderOwner::Complete(state) = &mut render.owner else {
        panic!("complete owner must retain its variant")
    };
    f(state);
}

fn history(state: &NativeOutputState) -> (u32, u64, u64, u64, u64, u64, Vec<SourcePlaybackSpan>) {
    let history = state.mixer().source_playback_history().unwrap();
    (
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
        history.pulled_end_frame(),
        history.retained_start_frame(),
        history.retirement_revision(),
        (0..history.len())
            .map(|index| history.span(index).unwrap())
            .collect(),
    )
}

fn query(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 1000).unwrap(), 0, 0, 1, 0, 1)
        .unwrap()
}

#[repr(C)]
struct BufferList2 {
    count: u32,
    buffers: [ffi::AudioBuffer; 2],
}

struct Output {
    left: Vec<f32>,
    right: Vec<f32>,
    list: BufferList2,
}
impl Output {
    fn new(frames: usize) -> Self {
        let mut left = vec![0.75; frames];
        let mut right = vec![0.75; frames];
        let list = BufferList2 {
            count: 2,
            buffers: [
                ffi::AudioBuffer {
                    channels: 1,
                    byte_size: (frames * 4) as u32,
                    data: left.as_mut_ptr().cast(),
                },
                ffi::AudioBuffer {
                    channels: 1,
                    byte_size: (frames * 4) as u32,
                    data: right.as_mut_ptr().cast(),
                },
            ],
        };
        Self { left, right, list }
    }

    fn callback(&mut self, stream: &CoreAudioStream, ticks: u64, sample: f64) {
        let context = stream.context.as_deref().unwrap();
        self.callback_context(context, ticks, sample);
    }

    fn callback_context(&mut self, context: &Context, ticks: u64, sample: f64) {
        let time = ffi::AudioTimestamp {
            host_time: ticks,
            sample_time: sample,
            flags: 3,
            ..ffi::AudioTimestamp::default()
        };
        // SAFETY: both initialized descriptors and disjoint writable planes
        // remain alive. Malformed metadata is tested only before native copy;
        // every pointer remains representable and is never dereferenced then.
        unsafe {
            render_callback(
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                (&mut self.list as *mut BufferList2).cast(),
                &time,
                (context as *const Context).cast_mut().cast(),
            );
        }
    }

    fn untouched(&self) {
        assert!(self.left.iter().all(|sample| *sample == 0.75));
        assert!(self.right.iter().all(|sample| *sample == 0.75));
    }
}

#[test]
fn actual_complete_wrapper_delivers_original_suffix_and_stopped_recovery_keeps_unique_owner() {
    let (_, mut owner) = owner();
    let report = owner.render_pending(8).unwrap();
    owner.admit(2).unwrap();
    let pending = owner.pending_samples().to_vec();
    let history_before = history(&owner);
    let counters = owner.mixer().counters();
    let mut stream = stream(owner, 2, 2, false);
    assert_eq!(stream.frame_basis().start_physical_frame(), 4);
    assert_eq!(stream.max_callback_frames(), 2);
    assert!(stream.source_playback_at(&query(4), 1000, 2, 2).is_none());
    let mut output = Output::new(2);
    let ((), allocations) = count_heap_calls(|| output.callback(&stream, 12345, 67.25));
    assert_eq!(allocations, 0);
    assert_eq!(output.left, [pending[0], pending[2]]);
    assert_eq!(output.right, [pending[1], pending[3]]);
    let selected = stream.source_playback_at(&query(4), 1000, 2, 2).unwrap();
    assert_eq!(
        (selected.pulled_source_end, selected.delivered_end_frame),
        (10, 6)
    );
    assert_eq!(selected.render, Some(report));
    assert_eq!(selected.revision, 2);
    let native = selected.snapshot.presentation.unwrap();
    assert_eq!(
        (native.host_ticks, native.native_sample_frame, native.flags),
        (12345, Some(67.25), 3)
    );
    assert_eq!((native.first_frame, native.frames), (4, 2));
    assert_eq!(
        native.output_grid.unwrap().timestamp,
        Timestamp::from_nanos(4_123_000)
    );
    assert_eq!(state(&stream).mixer().counters(), counters);
    assert_eq!(state(&stream).mixer().frame_cursor(), 10);
    assert_eq!(state(&stream).admitted_frames(), 4);
    assert_eq!(history(state(&stream)), history_before);
    assert_eq!(
        stream.take_stopped_output_state().err(),
        Some(CoreAudioError::RecoveryUnavailable)
    );
    stream.stop().unwrap(); // No native registration exists in this fixture.
    assert_eq!(
        beatkernel::audio::StoppedMixerSource::take_stopped_mixer(&mut stream).err(),
        Some(CoreAudioError::InvalidRequest)
    );
    let recovered = stream.take_stopped_output_state().unwrap().unwrap();
    assert_eq!(recovered.pending_report(), Some(report));
    assert_eq!(recovered.admitted_frames(), 4);
    assert_eq!(recovered.pending_samples(), &pending[4..]);
    assert_eq!(recovered.output_frame_basis().start_physical_frame(), 6);
    assert_eq!(history(&recovered), history_before);
    assert_eq!(recovered.mixer().counters(), counters);
    assert_eq!(stream.snapshot(), selected.snapshot);
    assert_eq!(stream.last_render_report(), Some(report));
    assert!(stream.take_stopped_output_state().unwrap().is_none());
}

#[test]
fn complete_wrapper_checks_every_descriptor_before_any_plane_copy_or_source_advance() {
    for case in 0..8 {
        let (_, owner) = owner();
        let stream = stream(owner, 4, 8, true);
        let before = history(state(&stream));
        let counters = state(&stream).mixer().counters();
        let mut output = Output::new(4);
        match case {
            0 => output.list.count = 1,
            1 => output.list.buffers[1].channels = 2,
            2 => output.list.buffers[1].byte_size = 15,
            3 => output.list.buffers[1].data = std::ptr::null_mut(),
            4 => output.list.buffers[1].data = output.list.buffers[0].data,
            5 => {
                output.list.buffers[1].data = output
                    .right
                    .as_mut_ptr()
                    .cast::<u8>()
                    .wrapping_add(1)
                    .cast()
            }
            6 => output.list.buffers[1].byte_size = 12,
            _ => output.list.count = 0,
        }
        let ((), allocations) = count_heap_calls(|| output.callback(&stream, 999, 0.));
        assert_eq!(allocations, 0);
        output.untouched();
        assert_eq!(history(state(&stream)), before);
        assert_eq!(state(&stream).mixer().counters(), counters);
        assert_eq!(state(&stream).mixer().frame_cursor(), 2);
        assert_eq!(state(&stream).pending_report(), None);
        let snapshot = stream.snapshot();
        assert_eq!(
            (
                snapshot.rendered_callbacks,
                snapshot.rendered_frames,
                snapshot.callback_failures
            ),
            (0, 0, 1)
        );
        assert!(snapshot.presentation.is_none());
    }
}

#[test]
fn complete_remix_variant_keeps_generated_nonzero_mono_tail_and_converter_on_recovery() {
    let (_, owner) = owner();
    let mixer = owner
        .into_mixer()
        .unwrap_or_else(|_| panic!("no pending owner yet"));
    let mono = AudioFormat::new(1000, 1).unwrap();
    let matrix = ChannelMatrix::new(2, 1, &[0.75, 0.25]).unwrap();
    let mut owner = NativeOutputState::new(
        mixer,
        DeviceFormat::new(
            mono.sample_rate(),
            mono.channels(),
            SampleEncoding::Float32,
            None,
        )
        .unwrap(),
        Some(matrix.clone()),
        8,
    )
    .unwrap_or_else(|_| panic!("prepared stereo to mono owner"));
    let report = owner.render_pending(8).unwrap();
    owner.admit(2).unwrap();
    let pending = owner.pending_samples().to_vec();
    let before = history(&owner);
    let mut stream = stream_with_matrix(owner, 2, 2, false, Some(matrix));
    let mut output = Output::new(2);
    output.list.count = 1;
    output.callback(&stream, 55, 4.);
    assert_eq!(output.left, pending[..2]);
    assert_eq!(output.left, [25. / 128., 30. / 128.]);
    assert_eq!(output.right, [0.75; 2]);
    assert_eq!(state(&stream).pending_report(), Some(report));
    assert_eq!(state(&stream).admitted_frames(), 4);
    stream.stop().unwrap();
    let mut recovered = stream.take_stopped_output_state().unwrap().unwrap();
    assert_eq!(recovered.pending_samples(), &pending[2..]);
    assert_eq!(recovered.format().unwrap().pcm(), mono);
    assert_eq!(history(&recovered), before);
    recovered.admit(4).unwrap();
    assert!(
        recovered.into_mixer().is_err(),
        "the retained remix converter must survive complete recovery"
    );
}

#[test]
fn actual_variable_callbacks_accept_one_through_maximum_and_refuse_outside_it() {
    for frames in 1..=8 {
        let (_, owner) = owner();
        let stream = stream(owner, 4, 8, true);
        let mut output = Output::new(frames);
        output.callback(&stream, frames as u64, 2.);
        for index in 0..frames {
            let value = ((index + 2) % 8 + 1) as f32 / 16.;
            assert_eq!(
                (output.left[index], output.right[index]),
                (value, -value / 2.)
            );
        }
        assert_eq!(state(&stream).mixer().frame_cursor(), 2 + frames as u64);
        assert_eq!(state(&stream).pending_frames(), 0);
        assert_eq!(
            stream.snapshot().presentation.unwrap().frames,
            frames as u32
        );
    }
    for frames in [0, 9] {
        let (_, owner) = owner();
        let stream = stream(owner, 4, 8, true);
        let before = history(state(&stream));
        let mut output = Output::new(frames);
        output.callback(&stream, 10, 2.);
        output.untouched();
        assert_eq!(history(state(&stream)), before);
        assert_eq!(state(&stream).mixer().frame_cursor(), 2);
        assert_eq!(stream.snapshot().callback_failures, 1);
    }
}

#[test]
fn insufficient_retained_tail_refuses_before_history_retirement_or_native_copy() {
    let (_, owner) = owner();
    let mut stream = stream(owner, 4, 8, true);
    mutate_state(&mut stream, |state| {
        state.render_pending(8).unwrap();
        state.admit(5).unwrap();
    });
    let pending = state(&stream).pending_samples().to_vec();
    let report = state(&stream).pending_report();
    let before = history(state(&stream));
    let mut output = Output::new(4);
    let ((), allocations) = count_heap_calls(|| output.callback(&stream, 10, 7.));
    assert_eq!(allocations, 0);
    output.untouched();
    assert_eq!(state(&stream).pending_samples(), pending);
    assert_eq!(state(&stream).pending_report(), report);
    assert_eq!(state(&stream).admitted_frames(), 5);
    assert_eq!(history(state(&stream)), before);
    assert_eq!(stream.snapshot().callback_failures, 1);
    assert!(stream.snapshot().presentation.is_none());
}

#[test]
fn already_open_variable_wrapper_splits_original_nonzero_pending_without_rerender() {
    let (_, owner) = owner();
    let mut stream = stream(owner, 4, 8, true);
    let mut original = None;
    mutate_state(&mut stream, |state| {
        original = Some(state.render_pending(8).unwrap());
        state.admit(2).unwrap();
    });
    let pending = state(&stream).pending_samples().to_vec();
    let history_before = history(state(&stream));
    let counters = state(&stream).mixer().counters();
    for (offset, first) in [(0, 4), (6, 7)] {
        let mut output = Output::new(3);
        let ((), allocations) =
            count_heap_calls(|| output.callback(&stream, 50 + first, first as f64));
        assert_eq!(allocations, 0);
        assert_eq!(
            output.left,
            [pending[offset], pending[offset + 2], pending[offset + 4]]
        );
        assert_eq!(
            output.right,
            [
                pending[offset + 1],
                pending[offset + 3],
                pending[offset + 5]
            ]
        );
        assert_eq!(state(&stream).mixer().counters(), counters);
        assert_eq!(history(state(&stream)), history_before);
        let selected = stream
            .source_playback_at(&query(first), 1000, 2, 2)
            .unwrap();
        assert_eq!(selected.render, original);
        assert_eq!(selected.pulled_source_end, 10);
        assert_eq!(selected.delivered_end_frame, first + 3);
        let presentation = selected.snapshot.presentation.unwrap();
        assert_eq!((presentation.first_frame, presentation.frames), (first, 3));
        assert_eq!(selected.revision % 2, 0);
    }
    assert_eq!(state(&stream).pending_frames(), 0);
    assert_eq!(state(&stream).admitted_frames(), 8);
    assert_eq!(state(&stream).mixer().frame_cursor(), 10);
    stream.stop().unwrap();
    assert_eq!(
        stream.render_cadence().unwrap().unwrap().successful_renders,
        0
    );
}

#[test]
fn authorized_retirement_precedes_changed_configuration_odd_publication_and_buffer_validation() {
    let (_, owner) = owner();
    let mut stream = stream(owner, 4, 8, true);
    let ticket = stream
        .submit_source_conditional_retirement(&request(4), None, 8)
        .unwrap()
        .unwrap();
    let context = stream.context.as_deref().unwrap();
    context.changed.store(true, Ordering::Release);
    context.failures.store(7, Ordering::Relaxed);
    context.version.store(1, Ordering::SeqCst);
    // SAFETY: only this fixture owns this unregistered callback context.
    unsafe {
        (&mut *context.render.get()).publication_version = 1;
    }
    let before = history(state(&stream));
    let mut output = Output::new(4);
    output.list.buffers[1].data = std::ptr::null_mut();
    for _ in 0..2 {
        let ((), allocations) = count_heap_calls(|| output.callback(&stream, 88, 2.));
        assert_eq!(allocations, 0);
        output.untouched();
        assert_eq!(context.version.load(Ordering::SeqCst), 1);
        assert_eq!(context.failures.load(Ordering::Relaxed), 7);
        assert_eq!(context.calls.load(Ordering::Relaxed), 0);
        assert_eq!(context.frames.load(Ordering::Relaxed), 0);
        assert_eq!(history(state(&stream)), before);
        assert_eq!(state(&stream).pending_report(), None);
        assert_eq!(context.render_telemetry.read().render, None);
    }
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket),
        Ok(SourceRetirementStatus::Authorized)
    );
    assert_eq!(
        stream.cancel_source_conditional_retirement(&ticket),
        Ok(SourceRetirementStatus::Authorized)
    );
}

#[test]
fn authorized_stream_fences_live_source_and_history_but_retains_diagnostics_and_ticket_after_drain()
{
    let (_, owner) = owner();
    let mut stream = registered_fixture(owner);
    let mut ops = FaultOps::new(&stream);
    stream.start_with(&mut ops).unwrap();
    Output::new(2).callback(&stream, 321, 2.25);
    assert!(stream.is_started());
    assert!(stream.source_playback_at(&query(2), 1000, 2, 2).is_some());
    let snapshot = stream.snapshot();
    let report = stream.last_render_report();
    let basis = stream.frame_basis();
    let ticket = stream
        .submit_source_conditional_retirement(&request(2), None, 2)
        .unwrap()
        .unwrap();
    let mut output = Output::new(2);
    output.callback(&stream, 999, 4.);
    output.untouched();
    assert!(
        !stream.is_started(),
        "sticky permission ends live admission before native drain"
    );
    assert!(stream.source_playback_at(&query(2), 1000, 2, 2).is_none());
    let context = stream.context.as_deref().unwrap();
    assert!(context
        .read_source_playback_at(&query(2), 1000, 2, 2)
        .is_none());
    assert!(context
        .render_telemetry
        .source_retirement_request()
        .is_none());
    assert_eq!(
        stream.request_source_history_retirement(basis, (1000, 2, 2), 4),
        Err(CoreAudioError::RecoveryUnavailable)
    );
    assert_eq!(
        context.request_source_history_retirement(basis, (1000, 2, 2), 4),
        Err(CoreAudioError::RecoveryUnavailable)
    );
    assert!(context
        .render_telemetry
        .source_retirement_request()
        .is_none());
    assert_eq!(stream.snapshot(), snapshot);
    assert_eq!(stream.last_render_report(), report);
    assert_eq!(
        stream.start_with(&mut ops),
        Err(CoreAudioError::RecoveryUnavailable)
    );
    assert_eq!(ops.events, [NativeEvent::Start(1, 0x10)]);
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket),
        Ok(SourceRetirementStatus::Authorized)
    );
    stream.stop_with(&mut ops).unwrap();
    assert!(stream.context.is_none());
    assert!(!stream.is_started());
    assert!(stream.source_playback_at(&query(2), 1000, 2, 2).is_none());
    assert_eq!(stream.snapshot(), snapshot);
    assert_eq!(stream.last_render_report(), report);
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket),
        Ok(SourceRetirementStatus::Authorized)
    );
    assert_eq!(
        stream.cancel_source_conditional_retirement(&ticket),
        Ok(SourceRetirementStatus::Authorized)
    );
    let recovered = stream.take_stopped_output_state().unwrap().unwrap();
    assert_eq!(recovered.output_frame_basis().start_physical_frame(), 4);
    assert!(!recovered.admission_is_uncertain());
}

#[test]
fn authorization_between_coherent_source_read_loads_refuses_cached_delivery_at_unchanged_revision()
{
    let (_, owner) = owner();
    let mut stream = registered_fixture(owner);
    let mut ops = FaultOps::new(&stream);
    stream.start_with(&mut ops).unwrap();
    Output::new(2).callback(&stream, 123, 2.);
    let snapshot = stream.snapshot();
    let ticket = stream
        .submit_source_conditional_retirement(&request(2), None, 2)
        .unwrap()
        .unwrap();
    let context = stream.context.as_deref().unwrap();
    let revision = context.version.load(Ordering::SeqCst);
    let mut attempts = 0;
    let mut output = Output::new(2);
    let selected = context.read_source_playback_with(&query(2), 1000, 2, 2, |attempt| {
        assert_eq!(attempt, 0);
        attempts += 1;
        // Run the actual callback exactly after the reader loads the old native
        // and Source facts. Retirement deliberately does not bump their revision.
        output.callback_context(context, 456, 4.);
    });
    assert!(selected.is_none());
    assert_eq!(attempts, 1);
    output.untouched();
    assert_eq!(context.version.load(Ordering::SeqCst), revision);
    assert_eq!(stream.snapshot(), snapshot);
    assert!(!stream.is_started());
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket),
        Ok(SourceRetirementStatus::Authorized)
    );
    stream.stop_with(&mut ops).unwrap();
}

#[test]
fn history_intent_queued_before_authorization_keeps_its_exact_revision_after_the_effect() {
    let (_, owner) = owner();
    let mut stream = registered_fixture(owner);
    let mut ops = FaultOps::new(&stream);
    stream.start_with(&mut ops).unwrap();
    Output::new(2).callback(&stream, 123, 2.);
    let snapshot = stream.snapshot();
    let basis = stream.frame_basis();
    let ticket = stream
        .submit_source_conditional_retirement(&request(2), None, 2)
        .unwrap()
        .unwrap();
    let context = stream.context.as_deref().unwrap();
    let outer = context.version.load(Ordering::SeqCst);
    let before = history(state(&stream));
    let mut output = Output::new(2);
    let revision = context
        .request_source_history_retirement_with(basis, (1000, 2, 2), 4, || {
            output.callback_context(context, 456, 4.);
        })
        .unwrap()
        .expect("authorization cannot retract the sole already-completed queue effect");
    let queued = context
        .render_telemetry
        .source_retirement_request()
        .unwrap();
    assert_eq!((queued.revision, queued.source_pin), (revision, 4));
    output.untouched();
    assert_eq!(context.version.load(Ordering::SeqCst), outer);
    // SAFETY: no HAL registration exists; the synchronous callback returned.
    assert_eq!(
        unsafe { (&*context.render.get()).completed_source_retirement },
        0
    );
    assert_eq!(history(state(&stream)), before);
    assert_eq!(
        context.request_source_history_retirement(basis, (1000, 2, 2), 4),
        Err(CoreAudioError::RecoveryUnavailable)
    );
    assert_eq!(
        context.render_telemetry.source_retirement_request(),
        Some(queued)
    );
    assert_eq!(stream.snapshot(), snapshot);
    assert_eq!(
        stream.poll_source_conditional_retirement(&ticket),
        Ok(SourceRetirementStatus::Authorized)
    );
    stream.stop_with(&mut ops).unwrap();
}

#[test]
fn pending_cancelled_and_refused_tickets_preserve_live_source_and_history_qualification() {
    for cancel in [false, true] {
        let (_, owner) = owner();
        let mut stream = registered_fixture(owner);
        let mut ops = FaultOps::new(&stream);
        stream.start_with(&mut ops).unwrap();
        Output::new(2).callback(&stream, 123, 2.);
        if !cancel {
            mutate_state(&mut stream, |state| {
                state.render_pending(2).unwrap();
            });
        }
        let ticket = stream
            .submit_source_conditional_retirement(&request(2), None, 2)
            .unwrap()
            .unwrap();
        assert!(stream.is_started());
        assert!(stream.source_playback_at(&query(2), 1000, 2, 2).is_some());
        if cancel {
            assert_eq!(
                stream.cancel_source_conditional_retirement(&ticket),
                Ok(SourceRetirementStatus::Cancelled)
            );
        }
        Output::new(2).callback(&stream, 456, 4.);
        assert_eq!(
            stream.poll_source_conditional_retirement(&ticket),
            Ok(if cancel {
                SourceRetirementStatus::Cancelled
            } else {
                SourceRetirementStatus::Refused
            })
        );
        assert!(stream.is_started());
        assert!(stream.source_playback_at(&query(6), 1000, 2, 2).is_some());
        assert!(stream
            .request_source_history_retirement(stream.frame_basis(), (1000, 2, 2), 6)
            .unwrap()
            .is_some());
        stream.stop_with(&mut ops).unwrap();
    }
}

#[test]
fn history_mailbox_binds_first_unsent_basis_and_preserves_original_pending_report_pin() {
    let (_, mut owner) = owner();
    let report = owner.render_pending(8).unwrap();
    owner.admit(2).unwrap();
    let mixer_basis = owner.mixer().output_frame_basis();
    let stream = stream(owner, 2, 2, false);
    let basis = stream.frame_basis();
    assert_ne!(basis, mixer_basis);
    Output::new(2).callback(&stream, 10, 4.);
    assert_eq!(
        stream.request_source_history_retirement(mixer_basis, (1000, 2, 2), 6),
        Err(CoreAudioError::InvalidRequest)
    );
    let revision = stream
        .request_source_history_retirement(basis, (1000, 2, 2), 6)
        .unwrap()
        .unwrap();
    Output::new(2).callback(&stream, 20, 6.);
    assert_eq!(state(&stream).pending_report(), Some(report));
    assert_eq!(state(&stream).admitted_frames(), 6);
    let context = stream.context.as_deref().unwrap();
    // SAFETY: synchronous callback has returned; no native registration exists.
    assert_eq!(
        unsafe { (&*context.render.get()).completed_source_retirement },
        0
    );
    let history = state(&stream).mixer().source_playback_history().unwrap();
    assert_eq!(
        (
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.retained_start_frame()
        ),
        (2, 2, 2)
    );
    assert!(revision > 0);
    let selected = stream.source_playback_at(&query(6), 1000, 2, 2).unwrap();
    assert_eq!((selected.revision, selected.delivered_end_frame), (4, 8));
    assert_eq!(selected.snapshot.presentation.unwrap().first_frame, 6);
}

#[test]
fn cold_variable_geometry_preserves_nominal_and_checks_the_real_maximum() {
    for (nominal, advertised, expected) in [
        (4, 0, (4, false)),
        (4, 4, (4, true)),
        (4, 8, (8, true)),
        (1, u32::MAX, (u32::MAX, true)),
    ] {
        assert_eq!(
            validate_callback_geometry(nominal, advertised),
            Ok(expected)
        );
    }
    for (nominal, advertised) in [(0, 0), (0, 8), (4, 3), (8, 1)] {
        assert!(validate_callback_geometry(nominal, advertised).is_err());
    }
    let (_, mut owner) = owner();
    let before = history(&owner);
    assert!(prepare_complete_state(&request(4), &mut owner, None, 17, true, 4).is_err());
    assert_eq!(history(&owner), before);
    assert_eq!(owner.max_frames(), 16);
}

#[test]
fn cold_complete_growth_precedes_telemetry_and_keeps_pcm_history_identity_and_first_unsent_basis() {
    let (mut producer, mut owner) = owner();
    let report = owner.render_pending(8).unwrap();
    owner.admit(4).unwrap();
    let pending = owner.pending_samples().to_vec();
    let before = history(&owner);
    let basis = owner.output_frame_basis();
    let counters = owner.mixer().counters();
    let telemetry = prepare_complete_state(&request(4), &mut owner, None, 4, false, 4).unwrap();
    assert_eq!(
        owner.mixer().source_playback_history().unwrap().capacity(),
        8
    );
    assert_eq!(owner.pending_report(), Some(report));
    assert_eq!(owner.admitted_frames(), 4);
    assert_eq!(owner.pending_samples(), pending);
    assert_eq!(owner.output_frame_basis(), basis);
    assert_eq!(owner.mixer().counters(), counters);
    assert_eq!(history(&owner), before);
    assert!(telemetry
        .request_source_history_retirement(basis, (1000, 2, 2), 6)
        .unwrap()
        .is_some());
    assert!(telemetry
        .request_source_history_retirement(owner.mixer().output_frame_basis(), (1000, 2, 2), 6)
        .is_err());
    // A subsequent history transition fits the grown owner's publication. A
    // telemetry allocation made before growth would retain a two-slot payload.
    drop(telemetry);
    let mut stream = stream(owner, 4, 4, false);
    Output::new(4).callback(&stream, 10, 6.);
    mutate_state(&mut stream, |state| assert_eq!(state.pending_frames(), 0));
    assert!(stream.source_playback_at(&query(10), 1000, 2, 2).is_some());
    for (turn, paused) in [true, false, true, false].into_iter().enumerate() {
        producer.request_pause(paused);
        Output::new(4).callback(&stream, 20 + turn as u64, 10. + turn as f64 * 4.);
    }
    assert_eq!(
        state(&stream)
            .mixer()
            .source_playback_history()
            .unwrap()
            .len(),
        5
    );
    assert!(stream.source_playback_at(&query(26), 1000, 2, 2).is_some());
    assert_eq!(stream.snapshot().callback_failures, 0);
}

#[test]
fn history_and_later_telemetry_allocation_refusals_keep_the_complete_original_state() {
    for permitted_allocations in [0, 1] {
        let (_, mut owner) = owner();
        let report = owner.render_pending(8).unwrap();
        owner.admit(4).unwrap();
        let pending = owner.pending_samples().to_vec();
        let before = history(&owner);
        let basis = owner.output_frame_basis();
        let counters = owner.mixer().counters();
        let result = crate::audio::fail_allocation(permitted_allocations, || {
            prepare_complete_state(&request(4), &mut owner, None, 4, false, 4)
        });
        assert!(matches!(result, Err(CoreAudioError::Capacity)));
        assert_eq!(owner.pending_report(), Some(report));
        assert_eq!(owner.admitted_frames(), 4);
        assert_eq!(owner.pending_samples(), pending);
        assert_eq!(owner.output_frame_basis(), basis);
        assert_eq!(owner.mixer().counters(), counters);
        assert_eq!(history(&owner), before);
        let capacity = owner.mixer().source_playback_history().unwrap().capacity();
        assert_eq!(capacity, if permitted_allocations == 0 { 2 } else { 8 });
    }
}

#[test]
fn cold_fixed_reopen_keeps_divisible_suffix_and_refuses_nonmultiple_intact() {
    for (admitted, accepted) in [(0, true), (4, true), (5, false)] {
        let (mut producer, mut owner) = owner();
        producer.request_pause(true);
        let report = owner.render_pending(8).unwrap();
        if admitted != 0 {
            owner.admit(admitted).unwrap();
        }
        let pending = owner.pending_samples().to_vec();
        let before = history(&owner);
        let basis = owner.output_frame_basis();
        let result = prepare_complete_state(&request(4), &mut owner, None, 4, false, 4);
        assert_eq!(result.is_ok(), accepted);
        assert_eq!(owner.pending_report(), Some(report));
        assert_eq!(owner.admitted_frames(), admitted);
        assert_eq!(owner.pending_samples(), pending);
        assert_eq!(owner.output_frame_basis(), basis);
        assert_eq!(history(&owner), before);
    }
}

#[test]
fn cold_variable_destination_refuses_incoming_pending_intact_even_when_paused_zero() {
    for paused in [false, true] {
        let (mut producer, mut owner) = owner();
        producer.request_pause(paused);
        let report = owner.render_pending(8).unwrap();
        owner.admit(4).unwrap();
        let pending = owner.pending_samples().to_vec();
        let basis = owner.output_frame_basis();
        let before = history(&owner);
        let capacity = owner.mixer().source_playback_history().unwrap().capacity();
        assert!(prepare_complete_state(&request(4), &mut owner, None, 8, true, 4).is_err());
        assert_eq!(owner.pending_report(), Some(report));
        assert_eq!(owner.admitted_frames(), 4);
        assert_eq!(owner.pending_samples(), pending);
        assert_eq!(owner.output_frame_basis(), basis);
        assert_eq!(history(&owner), before);
        assert_eq!(
            owner.mixer().source_playback_history().unwrap().capacity(),
            capacity
        );
    }
}

#[test]
fn native_weak_tickets_reject_wrong_control_zero_stale_and_busy_without_changing_geometry() {
    let (_, owner) = owner();
    let mut first = stream(owner, 4, 4, false);
    let (_, owner) = self::owner();
    let mut second = stream(owner, 4, 4, false);
    let strong = Arc::strong_count(&first.control);
    let ticket = first
        .submit_source_conditional_retirement(&request(4), None, 4)
        .unwrap()
        .unwrap();
    assert_eq!(Arc::strong_count(&first.control), strong);
    assert_eq!(first.control.geometry.load(Ordering::Relaxed), 4);
    assert!(first
        .submit_source_conditional_retirement(&request(2), None, 2)
        .unwrap()
        .is_none());
    assert_eq!(first.control.geometry.load(Ordering::Relaxed), 4);
    assert_eq!(
        second.poll_source_conditional_retirement(&ticket),
        Err(CoreAudioError::InvalidRequest)
    );
    assert_eq!(
        second.cancel_source_conditional_retirement(&ticket),
        Err(CoreAudioError::InvalidRequest)
    );
    let zero = SourceRetirementTicket {
        control: Arc::downgrade(&first.control),
        nonce: 0,
    };
    assert_eq!(
        first.poll_source_conditional_retirement(&zero),
        Err(CoreAudioError::InvalidRequest)
    );
    assert_eq!(
        first.cancel_source_conditional_retirement(&zero),
        Err(CoreAudioError::InvalidRequest)
    );
    assert_eq!(
        first.cancel_source_conditional_retirement(&ticket),
        Ok(SourceRetirementStatus::Cancelled)
    );
    assert!(first
        .submit_source_conditional_retirement(&request(2), None, 2)
        .unwrap()
        .is_none());
    assert_eq!(
        first.poll_source_conditional_retirement(&ticket),
        Ok(SourceRetirementStatus::Cancelled)
    );
    let next = first
        .submit_source_conditional_retirement(&request(2), None, 2)
        .unwrap()
        .unwrap();
    assert_eq!(next.nonce, ticket.nonce + 1);
    assert_eq!(first.control.geometry.load(Ordering::Relaxed), 2);
    assert_eq!(
        first.poll_source_conditional_retirement(&ticket),
        Err(CoreAudioError::InvalidRequest)
    );
    assert_eq!(
        first.cancel_source_conditional_retirement(&next),
        Ok(SourceRetirementStatus::Cancelled)
    );
    let weak = next.control.clone();
    first.stop().unwrap();
    drop(first);
    assert!(
        weak.upgrade().is_none(),
        "a ticket must never keep the stream control alive"
    );
    assert_eq!(
        second.poll_source_conditional_retirement(&next),
        Err(CoreAudioError::InvalidRequest)
    );
}

#[test]
fn native_geometry_payload_authorizes_paused_eight_to_four_but_refuses_three_to_four() {
    for (remaining, authorized) in [(8, true), (3, false)] {
        let (mut producer, mut owner) = owner();
        producer.request_pause(true);
        let report = owner.render_pending(8).unwrap();
        if remaining == 3 {
            owner.admit(5).unwrap();
        }
        let mut stream = stream(owner, 1, 1, false);
        let ticket = stream
            .submit_source_conditional_retirement(&request(4), None, 4)
            .unwrap()
            .unwrap();
        assert_eq!(stream.control.geometry.load(Ordering::Relaxed), 4);
        let pending = state(&stream).pending_samples().to_vec();
        let before = history(state(&stream));
        let mut output = Output::new(1);
        output.callback(&stream, 10, 2.);
        assert_eq!(state(&stream).pending_report(), Some(report));
        assert_eq!(
            state(&stream).pending_samples(),
            if authorized {
                &pending[..]
            } else {
                &pending[2..]
            }
        );
        assert_eq!(history(state(&stream)), before);
        assert_eq!(
            stream.poll_source_conditional_retirement(&ticket),
            Ok(if authorized {
                SourceRetirementStatus::Authorized
            } else {
                SourceRetirementStatus::Refused
            })
        );
        if authorized {
            output.untouched();
            assert_eq!(stream.snapshot().callback_failures, 0);
            assert_eq!(
                stream.cancel_source_conditional_retirement(&ticket),
                Ok(SourceRetirementStatus::Authorized)
            );
            assert!(stream
                .submit_source_conditional_retirement(&request(2), None, 2)
                .unwrap()
                .is_none());
        } else {
            assert_eq!(
                (output.left.as_slice(), output.right.as_slice()),
                (&[0.][..], &[0.][..])
            );
            assert_eq!(stream.snapshot().callback_failures, 0);
            assert_eq!(state(&stream).pending_frames(), 2);
        }
    }
}

#[test]
fn variable_destination_and_changed_interpretation_cannot_retire_nonempty_paused_suffix() {
    for variable in [false, true] {
        let (mut producer, owner) = owner();
        producer.request_pause(true);
        let mut stream = stream(owner, 4, if variable { 8 } else { 4 }, variable);
        mutate_state(&mut stream, |state| {
            state.render_pending(if variable { 8 } else { 4 }).unwrap();
        });
        let matrix = ChannelMatrix::new(2, 2, &[0., 1., 1., 0.]).unwrap();
        let ticket = stream
            .submit_source_conditional_retirement(
                &request(4),
                if variable { None } else { Some(&matrix) },
                8,
            )
            .unwrap()
            .unwrap();
        let mut output = Output::new(4);
        output.callback(&stream, 10, 2.);
        assert_eq!(
            stream.poll_source_conditional_retirement(&ticket),
            Ok(SourceRetirementStatus::Refused)
        );
        assert_eq!(output.left, [0.; 4]);
        assert_eq!(output.right, [0.; 4]);
        assert_eq!(
            state(&stream).pending_frames(),
            if variable { 4 } else { 0 }
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeEvent {
    Create(u32),
    Add(u32, u32, u32, u32),
    Start(u32, usize),
    Stop(u32, usize),
    Remove(u32, u32),
    Destroy(u32, usize),
}

// Only primitive native effects are replaced. All start ACK, stop-required,
// unregister, drain, quarantine and extraction sequencing is production code.
struct FaultOps {
    events: Vec<NativeEvent>,
    create_code: i32,
    create_id: usize,
    add_code: i32,
    fail_add: Option<usize>,
    add_calls: usize,
    start_code: i32,
    stop_code: i32,
    remove_code: i32,
    destroy_code: i32,
    callback: Option<(*const Context, Output)>,
    context: *const Context,
}
impl FaultOps {
    fn new(stream: &CoreAudioStream) -> Self {
        let mut ops = Self::before_install();
        ops.context = stream.context.as_deref().unwrap();
        ops
    }

    fn before_install() -> Self {
        Self {
            events: Vec::new(),
            create_code: 0,
            create_id: 0x10,
            add_code: 0,
            fail_add: None,
            add_calls: 0,
            start_code: 0,
            stop_code: 0,
            remove_code: 0,
            destroy_code: 0,
            callback: None,
            context: std::ptr::null(),
        }
    }
}
impl NativeLifecycleOps for FaultOps {
    fn create(&mut self, device: u32, context: *mut c_void, id: &mut ffi::IoProcId) -> i32 {
        assert!(
            self.context.is_null(),
            "a fixture installs one stable context"
        );
        assert!(!context.is_null());
        self.context = context.cast_const().cast();
        self.events.push(NativeEvent::Create(device));
        *id = self.create_id as ffi::IoProcId;
        self.create_code
    }
    fn add_listener(
        &mut self,
        object: u32,
        address: &ffi::PropertyAddress,
        context: *mut c_void,
    ) -> i32 {
        assert_eq!(context.cast_const().cast::<Context>(), self.context);
        self.events.push(NativeEvent::Add(
            object,
            address.selector,
            address.scope,
            address.element,
        ));
        self.add_calls += 1;
        if self.fail_add == Some(self.add_calls) {
            self.add_code
        } else {
            0
        }
    }
    fn start(&mut self, device: u32, id: ffi::IoProcId) -> i32 {
        self.events.push(NativeEvent::Start(device, id as usize));
        if let Some((context, output)) = &mut self.callback {
            // SAFETY: start_with retains this exact stable callback box until
            // stop/unregister/drain. Invocation is synchronous inside Start.
            output.callback_context(unsafe { &**context }, 987, 2.5);
        }
        self.start_code
    }
    fn stop(&mut self, device: u32, id: ffi::IoProcId) -> i32 {
        self.events.push(NativeEvent::Stop(device, id as usize));
        self.stop_code
    }
    fn remove_listener(
        &mut self,
        object: u32,
        address: &ffi::PropertyAddress,
        context: *mut c_void,
    ) -> i32 {
        assert_eq!(context.cast_const().cast::<Context>(), self.context);
        self.events
            .push(NativeEvent::Remove(object, address.selector));
        self.remove_code
    }
    fn destroy(&mut self, device: u32, id: ffi::IoProcId) -> i32 {
        self.events.push(NativeEvent::Destroy(device, id as usize));
        self.destroy_code
    }
}

fn install_fixture(
    mut owner: NativeOutputState,
    variable_property: bool,
    variable: bool,
    ops: &mut FaultOps,
) -> Result<CoreAudioStream, (CoreAudioError, CoreAudioStream)> {
    let request = request(2);
    let basis = owner.output_frame_basis();
    let config = owner.mixer().config();
    let maximum = if variable { 4 } else { 2 };
    let telemetry =
        prepare_complete_state(&request, &mut owner, None, maximum, variable, 4).unwrap();
    let facts = SourceRetirementFacts {
        source: config.format(),
        applied: request.format,
        matrix: None,
        render_limit: config.limits().max_render_frames(),
    };
    let setup = NativeSetup {
        applied: CoreAudioApplied {
            request,
            format: request.format,
            buffer_frames: 2,
            buffer_channels: vec![1, 1],
            output_domain: config.domain(),
            output_origin: config.origin(),
            native_clock: ClockDomainId(8),
        },
        layout: vec![1, 1],
        streams: vec![81, 82],
        max_frames: maximum,
        variable,
        variable_property,
    };
    CoreAudioStream::install_with(
        request,
        MachClock::new(ClockDomainId(8), ClockDomainId(9)).unwrap(),
        basis,
        RenderOwner::Complete(owner),
        telemetry,
        setup,
        Some(facts),
        ops,
    )
}

fn expected_properties(variable_property: bool) -> Vec<(u32, u32, u32, u32)> {
    let mut properties = vec![
        (1, RATE, GLOBAL, 0),
        (1, BUFFER, GLOBAL, 0),
        (1, STREAM_CONFIG, OUTPUT, 0),
        (1, STREAMS, OUTPUT, 0),
    ];
    if variable_property {
        properties.push((1, VARIABLE_BUFFER, GLOBAL, 0));
    }
    properties.extend([(81, FORMAT, GLOBAL, 0), (82, FORMAT, GLOBAL, 0)]);
    properties
}

fn tracked_properties(stream: &CoreAudioStream) -> Vec<(u32, u32, u32, u32)> {
    stream
        .listeners
        .iter()
        .map(|(object, address)| (*object, address.selector, address.scope, address.element))
        .collect()
}

#[test]
fn actual_install_create_status_and_null_id_refusals_retain_the_complete_owner() {
    for (code, id) in [(-701, 0), (-702, 0x10), (0, 0)] {
        let (_, mut owner) = owner();
        let report = owner.render_pending(8).unwrap();
        owner.admit(2).unwrap();
        let pending = owner.pending_samples().to_vec();
        let before = history(&owner);
        let counters = owner.mixer().counters();
        let mut ops = FaultOps::before_install();
        ops.create_code = code;
        ops.create_id = id;
        let (error, mut stream) = install_fixture(owner, false, false, &mut ops)
            .err()
            .expect("creation refusal retains its real partial owner");
        assert_eq!(
            error,
            if code == 0 {
                CoreAudioError::DeviceUnavailable
            } else {
                CoreAudioError::Native {
                    operation: "AudioDeviceCreateIOProcID",
                    code,
                }
            }
        );
        assert_eq!(ops.events, [NativeEvent::Create(1)]);
        assert_eq!(
            stream.context.as_deref().unwrap() as *const Context,
            ops.context
        );
        assert_eq!(stream.proc_id as usize, id);
        assert!(stream.listeners.is_empty());
        assert!(!stream.stop_required);
        assert!(!stream.is_started());
        assert_eq!(state(&stream).pending_report(), Some(report));
        assert_eq!(state(&stream).pending_samples(), pending);
        assert_eq!(state(&stream).admitted_frames(), 2);
        assert_eq!(history(state(&stream)), before);
        assert_eq!(state(&stream).mixer().counters(), counters);
        assert_eq!(
            stream.take_stopped_output_state().err(),
            Some(CoreAudioError::RecoveryUnavailable)
        );
        stream.stop_with(&mut ops).unwrap();
        let recovered = stream.take_stopped_output_state().unwrap().unwrap();
        assert_eq!(recovered.pending_report(), Some(report));
        assert_eq!(recovered.pending_samples(), pending);
        assert_eq!(history(&recovered), before);
        assert_eq!(recovered.mixer().counters(), counters);
        assert_eq!(
            ops.events,
            if id == 0 {
                vec![NativeEvent::Create(1)]
            } else {
                vec![NativeEvent::Create(1), NativeEvent::Destroy(1, id)]
            }
        );
    }
}

#[test]
fn actual_install_tracks_optional_variable_property_presence_and_every_native_stream() {
    for (present, variable) in [(false, false), (true, false), (true, true)] {
        let (_, owner) = owner();
        let before = history(&owner);
        let mut ops = FaultOps::before_install();
        let mut stream = install_fixture(owner, present, variable, &mut ops)
            .unwrap_or_else(|_| panic!("successful primitive registration"));
        let properties = expected_properties(present);
        assert_eq!(tracked_properties(&stream), properties);
        assert_eq!(
            stream.context.as_deref().unwrap() as *const Context,
            ops.context
        );
        assert_eq!(stream.proc_id as usize, 0x10);
        assert_eq!(stream.variable_buffers, variable);
        assert_eq!(stream.max_callback_frames(), if variable { 4 } else { 2 });
        assert!(!stream.is_started());
        assert!(!stream.stop_required);
        assert_eq!(stream.snapshot(), CoreAudioSnapshot::default());
        assert_eq!(history(state(&stream)), before);
        let mut events = vec![NativeEvent::Create(1)];
        events.extend(
            properties
                .iter()
                .map(|&(object, selector, scope, element)| {
                    NativeEvent::Add(object, selector, scope, element)
                }),
        );
        assert_eq!(ops.events, events);
        stream.stop_with(&mut ops).unwrap();
        events.extend(
            properties
                .iter()
                .rev()
                .map(|&(object, selector, _, _)| NativeEvent::Remove(object, selector)),
        );
        events.push(NativeEvent::Destroy(1, 0x10));
        assert_eq!(ops.events, events);
        assert!(stream.context.is_none());
        assert!(stream.listeners.is_empty());
        assert!(stream.proc_id.is_null());
        let recovered = stream.take_stopped_output_state().unwrap().unwrap();
        assert_eq!(history(&recovered), before);
        assert!(!recovered.admission_is_uncertain());
    }
}

#[test]
fn actual_install_failure_at_each_listener_keeps_only_successful_registrations_and_exact_owner() {
    for present in [false, true] {
        let properties = expected_properties(present);
        for failed in 1..=properties.len() {
            let (_, mut owner) = owner();
            let report = owner.render_pending(8).unwrap();
            owner.admit(2).unwrap();
            let pending = owner.pending_samples().to_vec();
            let before = history(&owner);
            let basis = owner.output_frame_basis();
            let mut ops = FaultOps::before_install();
            ops.fail_add = Some(failed);
            ops.add_code = -710 - failed as i32;
            let (error, mut stream) = install_fixture(owner, present, false, &mut ops)
                .err()
                .expect("one exact listener refuses");
            assert_eq!(
                error,
                CoreAudioError::Native {
                    operation: "AudioObjectAddPropertyListener",
                    code: ops.add_code,
                }
            );
            assert_eq!(tracked_properties(&stream), properties[..failed - 1]);
            assert_eq!(
                stream.context.as_deref().unwrap() as *const Context,
                ops.context
            );
            assert_eq!(stream.proc_id as usize, 0x10);
            assert!(!stream.stop_required);
            assert!(!stream.is_started());
            assert_eq!(state(&stream).pending_report(), Some(report));
            assert_eq!(state(&stream).pending_samples(), pending);
            assert_eq!(state(&stream).admitted_frames(), 2);
            assert_eq!(state(&stream).output_frame_basis(), basis);
            assert_eq!(history(state(&stream)), before);
            let mut events = vec![NativeEvent::Create(1)];
            events.extend(properties[..failed].iter().map(
                |&(object, selector, scope, element)| {
                    NativeEvent::Add(object, selector, scope, element)
                },
            ));
            assert_eq!(
                ops.events, events,
                "no listener after the first failure is attempted"
            );
            stream.stop_with(&mut ops).unwrap();
            events.extend(
                properties[..failed - 1]
                    .iter()
                    .rev()
                    .map(|&(object, selector, _, _)| NativeEvent::Remove(object, selector)),
            );
            events.push(NativeEvent::Destroy(1, 0x10));
            assert_eq!(ops.events, events);
            let recovered = stream.take_stopped_output_state().unwrap().unwrap();
            assert_eq!(recovered.pending_report(), Some(report));
            assert_eq!(recovered.pending_samples(), pending);
            assert_eq!(recovered.admitted_frames(), 2);
            assert_eq!(recovered.output_frame_basis(), basis);
            assert_eq!(history(&recovered), before);
        }
    }
}

#[test]
fn actual_install_failure_cleanup_retries_preserve_original_and_latest_error_on_the_same_owner() {
    let (_, mut owner) = owner();
    let report = owner.render_pending(8).unwrap();
    owner.admit(2).unwrap();
    let pending = owner.pending_samples().to_vec();
    let before = history(&owner);
    let mut ops = FaultOps::before_install();
    ops.fail_add = Some(6); // Present vfsz succeeds; the first stream listener fails.
    ops.add_code = -801;
    let (original, stream) = install_fixture(owner, true, false, &mut ops).err().unwrap();
    assert_eq!(
        original,
        CoreAudioError::Native {
            operation: "AudioObjectAddPropertyListener",
            code: -801
        }
    );
    let pointer = stream.context.as_deref().unwrap() as *const Context;
    let mut failure = CoreAudioStateOpenFailure::pending_state(original.clone(), stream);
    ops.remove_code = -802;
    let remove = CoreAudioError::Native {
        operation: "AudioObjectRemovePropertyListener",
        code: -802,
    };
    assert_eq!(retry_with(&mut failure, &mut ops), Err(remove.clone()));
    assert_eq!(failure.error(), &original);
    assert_eq!(failure.cleanup_error(), Some(&remove));
    assert!(failure.mixer().is_none());
    let live = failure.pending_owner().unwrap();
    assert_eq!(live.context.as_deref().unwrap() as *const Context, pointer);
    assert_eq!(tracked_properties(live), expected_properties(true)[..5]);
    assert_eq!(state(live).pending_samples(), pending);
    ops.remove_code = 0;
    ops.destroy_code = -803;
    let destroy = CoreAudioError::Native {
        operation: "AudioDeviceDestroyIOProcID",
        code: -803,
    };
    assert_eq!(retry_with(&mut failure, &mut ops), Err(destroy.clone()));
    assert_eq!(failure.error(), &original);
    assert_eq!(failure.cleanup_error(), Some(&destroy));
    let live = failure.pending_owner().unwrap();
    assert_eq!(live.context.as_deref().unwrap() as *const Context, pointer);
    assert!(live.listeners.is_empty());
    assert_eq!(live.proc_id as usize, 0x10);
    assert_eq!(state(live).pending_report(), Some(report));
    assert_eq!(state(live).admitted_frames(), 2);
    assert_eq!(history(state(live)), before);
    ops.destroy_code = 0;
    assert_eq!(retry_with(&mut failure, &mut ops), Ok(true));
    assert_eq!(failure.error(), &original);
    assert!(failure.cleanup_error().is_none());
    assert!(failure.pending_owner().is_none());
    let recovered = failure.mixer().unwrap();
    assert_eq!(recovered.pending_report(), Some(report));
    assert_eq!(recovered.pending_samples(), pending);
    assert_eq!(recovered.admitted_frames(), 2);
    assert_eq!(history(recovered), before);
    assert!(!recovered.admission_is_uncertain());
    assert!(!ops
        .events
        .iter()
        .any(|event| matches!(event, NativeEvent::Start(..) | NativeEvent::Stop(..))));
    assert_eq!(
        &ops.events[ops.events.len() - 2..],
        [NativeEvent::Destroy(1, 0x10), NativeEvent::Destroy(1, 0x10)]
    );
    assert_eq!(
        CoreAudioStream::retry_state_open_cleanup(&mut failure),
        Ok(false)
    );
}

fn registered_fixture(owner: NativeOutputState) -> CoreAudioStream {
    let stream = stream(owner, 2, 2, false);
    // These tracked, never-dereferenced identities go only to FaultOps. No OS
    // registration is claimed; they exercise production partial cleanup order.
    let mut stream = stream;
    stream.proc_id = 0x10usize as ffi::IoProcId;
    stream.listeners = vec![(91, address(RATE, GLOBAL)), (92, address(BUFFER, GLOBAL))];
    stream
        .context
        .as_ref()
        .unwrap()
        .enabled
        .store(false, Ordering::Release);
    stream
}

fn retry_with(
    failure: &mut CoreAudioStateOpenFailure,
    ops: &mut FaultOps,
) -> Result<bool, CoreAudioError> {
    failure
        .retry_retirement(|stream| {
            stream.stop_with(ops)?;
            stream.take_stopped_output_state()
        })
        .map_err(Clone::clone)
}

#[test]
fn failed_start_without_callback_requires_stop_and_every_cleanup_retry_preserves_exact_errors_and_owner(
) {
    let (_, mut owner) = owner();
    let report = owner.render_pending(8).unwrap();
    owner.admit(2).unwrap();
    let pending = owner.pending_samples().to_vec();
    let before = history(&owner);
    let mut stream = registered_fixture(owner);
    let pointer = stream.context.as_deref().unwrap() as *const Context;
    let mut ops = FaultOps::new(&stream);
    ops.start_code = -501;
    let original = CoreAudioError::Native {
        operation: "AudioDeviceStart",
        code: -501,
    };
    assert_eq!(stream.start_with(&mut ops), Err(original.clone()));
    assert!(!stream.is_started(), "failed native Start never grants ACK");
    assert!(
        stream.stop_required,
        "attempted Start still requires actual Stop"
    );
    assert!(!stream.started);
    assert!(stream.failed_start);
    assert!(!state(&stream).admission_is_uncertain());
    assert_eq!(
        stream.take_stopped_output_state().err(),
        Some(CoreAudioError::RecoveryUnavailable)
    );
    let mut failure = CoreAudioStateOpenFailure::pending_state(original.clone(), stream);

    ops.stop_code = -502;
    let stopped = CoreAudioError::Native {
        operation: "AudioDeviceStop",
        code: -502,
    };
    assert_eq!(retry_with(&mut failure, &mut ops), Err(stopped.clone()));
    assert_eq!(failure.error(), &original);
    assert_eq!(failure.cleanup_error(), Some(&stopped));
    assert!(failure.mixer().is_none());
    let live = failure.pending_owner().unwrap();
    assert_eq!(live.context.as_deref().unwrap() as *const Context, pointer);
    assert_eq!(live.listeners.len(), 2);
    assert_eq!(state(live).pending_samples(), pending);
    assert_eq!(history(state(live)), before);
    assert!(live.stop_required);

    ops.stop_code = 0;
    ops.remove_code = -503;
    let listener = CoreAudioError::Native {
        operation: "AudioObjectRemovePropertyListener",
        code: -503,
    };
    assert_eq!(retry_with(&mut failure, &mut ops), Err(listener.clone()));
    assert_eq!(failure.error(), &original);
    assert_eq!(failure.cleanup_error(), Some(&listener));
    let live = failure.pending_owner().unwrap();
    assert!(!live.stop_required);
    assert_eq!(
        live.listeners.len(),
        2,
        "a failed unregister remains tracked"
    );
    assert_eq!(state(live).pending_report(), Some(report));
    assert!(!state(live).admission_is_uncertain());

    ops.remove_code = 0;
    ops.destroy_code = -504;
    let destroy = CoreAudioError::Native {
        operation: "AudioDeviceDestroyIOProcID",
        code: -504,
    };
    assert_eq!(retry_with(&mut failure, &mut ops), Err(destroy.clone()));
    assert_eq!(failure.error(), &original);
    assert_eq!(failure.cleanup_error(), Some(&destroy));
    let live = failure.pending_owner().unwrap();
    assert!(live.listeners.is_empty());
    assert_eq!(live.proc_id as usize, 0x10);
    assert_eq!(live.context.as_deref().unwrap() as *const Context, pointer);
    assert_eq!(state(live).pending_samples(), pending);
    assert_eq!(state(live).admitted_frames(), 2);

    ops.destroy_code = 0;
    assert_eq!(retry_with(&mut failure, &mut ops), Ok(true));
    assert_eq!(failure.error(), &original);
    assert!(failure.cleanup_error().is_none());
    assert!(failure.pending_owner().is_none());
    let recovered = failure.mixer().unwrap();
    assert_eq!(recovered.pending_report(), Some(report));
    assert_eq!(recovered.pending_samples(), pending);
    assert_eq!(recovered.admitted_frames(), 2);
    assert_eq!(history(recovered), before);
    assert!(
        !recovered.admission_is_uncertain(),
        "Start failure without copying is replayable"
    );
    assert_eq!(
        CoreAudioStream::retry_state_open_cleanup(&mut failure),
        Ok(false)
    );
    assert_eq!(
        ops.events,
        [
            NativeEvent::Start(1, 0x10),
            NativeEvent::Stop(1, 0x10),
            NativeEvent::Stop(1, 0x10),
            NativeEvent::Remove(92, BUFFER),
            NativeEvent::Remove(92, BUFFER),
            NativeEvent::Remove(91, RATE),
            NativeEvent::Destroy(1, 0x10),
            NativeEvent::Destroy(1, 0x10),
        ]
    );
}

#[test]
fn callback_during_failed_start_quarantines_only_actual_copy_after_confirmed_cleanup() {
    for callback in [0, 1, 2] {
        let (_, owner) = owner();
        let mut stream = registered_fixture(owner);
        let mut ops = FaultOps::new(&stream);
        ops.start_code = -601;
        if callback != 0 {
            let mut output = Output::new(2);
            if callback == 1 {
                output.list.buffers[1].channels = 2;
            }
            ops.callback = Some((ops.context, output));
        }
        assert_eq!(
            stream.start_with(&mut ops),
            Err(CoreAudioError::Native {
                operation: "AudioDeviceStart",
                code: -601
            })
        );
        assert!(!stream.is_started());
        assert!(stream.source_playback_at(&query(2), 1000, 2, 2).is_none());
        assert_eq!(
            stream.request_source_history_retirement(stream.frame_basis(), (1000, 2, 2), 2),
            Err(CoreAudioError::RecoveryUnavailable)
        );
        assert!(stream
            .context
            .as_ref()
            .unwrap()
            .render_telemetry
            .source_retirement_request()
            .is_none());
        assert!(stream.stop_required);
        assert!(
            !state(&stream).admission_is_uncertain(),
            "callback must finish before control cleanup quarantines"
        );
        assert_eq!(
            stream
                .context
                .as_ref()
                .unwrap()
                .active
                .load(Ordering::Acquire),
            0
        );
        assert_eq!(
            stream.take_stopped_output_state().err(),
            Some(CoreAudioError::RecoveryUnavailable)
        );
        if callback == 2 {
            let output = &ops.callback.as_ref().unwrap().1;
            assert_eq!(output.left, [3. / 16., 4. / 16.]);
            assert_eq!(output.right, [-3. / 32., -4. / 32.]);
            let observation = stream.snapshot().presentation.unwrap();
            assert_eq!(
                (
                    observation.host_ticks,
                    observation.native_sample_frame,
                    observation.first_frame,
                    observation.frames
                ),
                (987, Some(2.5), 2, 2)
            );
        } else if callback == 1 {
            ops.callback.as_ref().unwrap().1.untouched();
        }
        stream.stop_with(&mut ops).unwrap();
        assert!(stream.context.is_none());
        let recovered = stream.take_stopped_output_state().unwrap().unwrap();
        assert_eq!(recovered.admission_is_uncertain(), callback == 2);
        assert_eq!(
            recovered.mixer().frame_cursor(),
            if callback == 2 { 4 } else { 2 }
        );
        assert_eq!(
            recovered
                .mixer()
                .source_playback_history()
                .unwrap()
                .initial_physical_frame(),
            2
        );
        assert_eq!(
            ops.events,
            [
                NativeEvent::Start(1, 0x10),
                NativeEvent::Stop(1, 0x10),
                NativeEvent::Remove(92, BUFFER),
                NativeEvent::Remove(91, RATE),
                NativeEvent::Destroy(1, 0x10),
            ]
        );
    }
}

#[test]
fn native_start_ack_is_separate_from_delivery_and_successful_stop_recovers_exact_complete_owner() {
    let (_, owner) = owner();
    let mut stream = registered_fixture(owner);
    let mut ops = FaultOps::new(&stream);
    assert!(!stream.is_started());
    stream.start_with(&mut ops).unwrap();
    assert!(stream.is_started());
    assert!(stream.stop_required);
    assert!(!stream.failed_start);
    assert!(
        stream.snapshot().presentation.is_none(),
        "ACK alone supplies no Source progress"
    );
    stream.start_with(&mut ops).unwrap();
    assert_eq!(ops.events, [NativeEvent::Start(1, 0x10)]);
    Output::new(2).callback(&stream, 123, 2.);
    let before = history(state(&stream));
    let basis = state(&stream).output_frame_basis();
    stream.stop_with(&mut ops).unwrap();
    assert!(!stream.is_started());
    assert!(!stream.stop_required);
    assert!(stream.retired);
    assert!(stream.context.is_none());
    let recovered = stream.take_stopped_output_state().unwrap().unwrap();
    assert_eq!(history(&recovered), before);
    assert_eq!(recovered.output_frame_basis(), basis);
    assert!(!recovered.admission_is_uncertain());
    stream.stop_with(&mut ops).unwrap();
    assert_eq!(
        ops.events,
        [
            NativeEvent::Start(1, 0x10),
            NativeEvent::Stop(1, 0x10),
            NativeEvent::Remove(92, BUFFER),
            NativeEvent::Remove(91, RATE),
            NativeEvent::Destroy(1, 0x10),
        ]
    );
}
