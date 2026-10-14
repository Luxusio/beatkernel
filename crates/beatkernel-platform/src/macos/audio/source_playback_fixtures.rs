//! macOS-only owned callback-memory fixtures. No device is opened; execution
//! still requires macOS MachClock primitives and is not hardware/acoustic proof.
use super::*;
use crate::audio::count_heap_calls;
use beatkernel::audio::{
    command_queue, command_queue_with_start_gate, AudioCommand, AudioLimits, CommandProducer,
    InterpolatedTargetTime, MixerConfig, PcmLimits, PcmSample, SampleBank, SampleId,
    SourcePlaybackState, TargetTime, VoiceId,
};

fn mixer(gate: Option<u64>, end: Option<u64>, opted: bool) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, 2).unwrap();
    let limits = PcmLimits::new(64, 128, 2).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, -0.5], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = if gate.is_some() {
        command_queue_with_start_gate(8)
    } else {
        command_queue(8)
    }
    .unwrap();
    if let Some(frame) = gate {
        producer.schedule_start_at(frame).unwrap();
    }
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mut config = MixerConfig::new(
        format,
        ClockDomainId(7),
        Timestamp::ZERO,
        AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
    );
    if let Some(frame) = end {
        config = config.with_playback_end_frame(frame);
    }
    let mut mixer = Mixer::new(config, bank, consumer).unwrap();
    if opted {
        mixer.enable_source_playback_history(16).unwrap();
    }
    (producer, mixer)
}

fn context(mixer: Mixer) -> Context {
    let config = mixer.config();
    let telemetry = prepare_render_telemetry(&mixer).unwrap();
    Context {
        control: std::sync::Arc::new(SourceControl::new()),
        complete: false,
        render: UnsafeCell::new(RenderState {
            owner: RenderOwner::Legacy {
                mixer,
                remix: None,
                scratch: vec![0.; 16],
            },
            render_version: 0,
            publication_version: 0,
            completed_source_retirement: 0,
        }),
        render_telemetry: telemetry,
        cadence: Box::new(crate::audio::cadence::Capture::new()),
        clock: MachClock::new(ClockDomainId(8), ClockDomainId(9)).unwrap(),
        origin: config.origin(),
        output_domain: config.domain(),
        rate: 1000,
        channels: 2,
        layout: vec![1, 1],
        buffer_frames: 8,
        enabled: AtomicBool::new(true),
        rendering: AtomicBool::new(false),
        active: AtomicUsize::new(0),
        changed: AtomicBool::new(false),
        calls: AtomicU64::new(0),
        frames: AtomicU64::new(0),
        failures: AtomicU64::new(0),
        version: AtomicU64::new(0),
        ticks: AtomicU64::new(0),
        sample_bits: AtomicU64::new(0),
        flags: AtomicU32::new(0),
        first_frame: AtomicU64::new(0),
        block_frames: AtomicU32::new(0),
        delivered: AtomicBool::new(false),
        delivered_end: AtomicU64::new(0),
    }
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
    fn callback(&mut self, context: &Context, ticks: u64, sample: f64) {
        let time = ffi::AudioTimestamp {
            host_time: ticks,
            sample_time: sample,
            flags: 3,
            ..ffi::AudioTimestamp::default()
        };
        // SAFETY: owned C-layout flexible array has exactly two initialized
        // descriptors, disjoint live writable planes and a live timestamp.
        // There is no native registration or concurrent mutation of buffers.
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
}
fn query(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 1000).unwrap(), 0, 0, 1, 0, 1)
        .unwrap()
}
fn read(context: &Context, frame: u64) -> Option<CoreAudioSourcePlaybackSnapshot> {
    context.read_source_playback_at(&query(frame), 1000, 0, 0)
}
fn frame_cursor(context: &Context) -> u64 {
    // SAFETY: fixtures call only after callback return with no registered driver.
    let render = unsafe { &*context.render.get() };
    match &render.owner {
        RenderOwner::Legacy { mixer, .. } => mixer.frame_cursor(),
        RenderOwner::Complete(state) => state.mixer().frame_cursor(),
    }
}

#[test]
fn all_owned_planes_copy_before_original_presentation_and_source_are_observable() {
    let (_, mixer) = mixer(None, None, true);
    let context = context(mixer);
    assert!(read(&context, 0).is_none());
    let mut output = Output::new(4);
    let ((), allocations) = count_heap_calls(|| output.callback(&context, 123, 456.25));
    assert_eq!(allocations, 0);
    assert_eq!(output.left, [0.25, 0., 0., 0.]);
    assert_eq!(output.right, [-0.5, 0., 0., 0.]);
    let (selected, allocations) = count_heap_calls(|| read(&context, 4));
    assert_eq!(allocations, 0);
    let selected = selected.unwrap();
    assert_eq!(
        (
            selected.pulled_source_end,
            selected.delivered_end_frame,
            selected.revision
        ),
        (4, 4, 2)
    );
    assert_eq!(selected.render.unwrap().start_frame, 0);
    assert_eq!(selected.playback.state(), SourcePlaybackState::Running);
    assert_eq!(
        (
            selected.snapshot.rendered_callbacks,
            selected.snapshot.rendered_frames
        ),
        (1, 4)
    );
    let presentation = selected.snapshot.presentation.unwrap();
    assert_eq!(
        (
            presentation.host_ticks,
            presentation.native_sample_frame,
            presentation.flags,
            presentation.first_frame,
            presentation.frames
        ),
        (123, Some(456.25), 3, 0, 4)
    );
    assert_eq!(presentation.output_grid.unwrap().timestamp, Timestamp::ZERO);
    assert_eq!(selected.snapshot, context.snapshot());
    let fraction_future = InterpolatedTargetTime::new(
        TargetTime::new(0, 8_000_001, 2_000_000_000).unwrap(),
        0,
        0,
        1,
        0,
        1,
    )
    .unwrap();
    assert!(context
        .read_source_playback_at(&fraction_future, 1000, 0, 0)
        .is_none());
}

#[test]
fn startup_three_actual_intervals_select_the_new_interval_at_exact_boundaries() {
    let (_, mixer) = mixer(Some(2), Some(3), true);
    let context = context(mixer);
    Output::new(8).callback(&context, 77, 0.);
    for (frame, start, playback, state, end) in [
        (0, 0, 0, SourcePlaybackState::Frozen, Some(2)),
        (2, 2, 0, SourcePlaybackState::Running, Some(5)),
        (4, 2, 0, SourcePlaybackState::Running, Some(5)),
        (5, 5, 3, SourcePlaybackState::Frozen, None),
        (8, 5, 3, SourcePlaybackState::Frozen, None),
    ] {
        let selected = read(&context, frame).unwrap();
        assert_eq!(
            (
                selected.playback.physical_start(),
                selected.playback.playback_start(),
                selected.playback.state(),
                selected.playback.physical_end()
            ),
            (start, playback, state, end)
        );
        assert_eq!(
            (selected.pulled_source_end, selected.delivered_end_frame),
            (8, 8)
        );
    }
    assert!(read(&context, 9).is_none());
}

#[test]
fn actual_pause_merge_and_resume_retain_historical_intervals_with_current_delivery() {
    let (mut producer, mixer) = mixer(None, None, true);
    let context = context(mixer);
    Output::new(4).callback(&context, 10, 0.);
    producer.request_pause(true);
    Output::new(3).callback(&context, 20, 4.);
    Output::new(2).callback(&context, 30, 7.);
    producer.request_pause(false);
    Output::new(4).callback(&context, 40, 9.);
    for (frame, start, playback, state) in [
        (3, 0, 0, SourcePlaybackState::Running),
        (4, 4, 4, SourcePlaybackState::Frozen),
        (8, 4, 4, SourcePlaybackState::Frozen),
        (9, 9, 4, SourcePlaybackState::Running),
        (13, 9, 4, SourcePlaybackState::Running),
    ] {
        let selected = read(&context, frame).unwrap();
        assert_eq!(
            (
                selected.playback.physical_start(),
                selected.playback.playback_start(),
                selected.playback.state()
            ),
            (start, playback, state)
        );
        assert_eq!((selected.delivered_end_frame, selected.revision), (13, 8));
        assert_eq!(selected.snapshot.presentation.unwrap().host_ticks, 40);
    }
}

#[test]
fn invalid_owned_layout_before_render_preserves_pcm_cursor_report_and_old_presentation() {
    let (_, mixer) = mixer(None, None, true);
    let context = context(mixer);
    Output::new(2).callback(&context, 10, 0.);
    let before = read(&context, 2).unwrap();
    for case in 0..5 {
        let mut output = Output::new(3);
        match case {
            0 => output.list.count = 1,
            1 => output.list.buffers[1].channels = 2,
            2 => output.list.buffers[1].byte_size = 10,
            3 => output.list.buffers[1].data = std::ptr::null_mut(),
            _ => output.list.buffers[1].data = output.list.buffers[0].data,
        }
        output.callback(&context, 99, 2.);
        assert_eq!(output.left, [0.75; 3]);
        assert_eq!(output.right, [0.75; 3]);
        assert_eq!(frame_cursor(&context), 2);
        let after = read(&context, 2).unwrap();
        assert_eq!(
            (
                after.pulled_source_end,
                after.delivered_end_frame,
                after.render
            ),
            (
                before.pulled_source_end,
                before.delivered_end_frame,
                before.render
            )
        );
        assert_eq!(after.snapshot.presentation, before.snapshot.presentation);
        assert_eq!(after.snapshot.callback_failures, case + 1);
        assert!(read(&context, 3).is_none());
    }
}

#[test]
fn odd_and_checked_exhausted_outer_version_refuse_before_pcm_or_mixer_mutation() {
    for version in [1, u64::MAX - 1, u64::MAX] {
        let (_, mixer) = mixer(None, None, true);
        let context = context(mixer);
        context.version.store(version, Ordering::SeqCst);
        // SAFETY: no native registration; sole fixture owns all callback state.
        unsafe {
            (&mut *context.render.get()).publication_version = version;
        }
        let mut output = Output::new(2);
        output.callback(&context, 100, 0.);
        assert_eq!(frame_cursor(&context), 0);
        assert_eq!(output.left, [0.75; 2]);
        assert_eq!(output.right, [0.75; 2]);
        assert!(read(&context, 0).is_none());
        assert!(!context.delivered.load(Ordering::SeqCst));
        if version >= u64::MAX - 1 {
            assert_eq!(context.version.load(Ordering::SeqCst), u64::MAX);
        }
    }
}

#[test]
fn disabled_legacy_still_delivers_and_opted_query_requires_original_history_identity() {
    let (_, legacy_mixer) = mixer(None, None, false);
    let legacy = context(legacy_mixer);
    Output::new(2).callback(&legacy, 10, 0.);
    assert_eq!(legacy.snapshot().rendered_frames, 2);
    assert!(read(&legacy, 0).is_none());
    let (_, opted_mixer) = mixer(None, None, true);
    let opted = context(opted_mixer);
    Output::new(3).callback(&opted, 11, 0.);
    for (hz, physical, playback) in [(999, 0, 0), (1000, 1, 0), (1000, 0, 1)] {
        assert!(opted
            .read_source_playback_at(&query(1), hz, physical, playback)
            .is_none());
    }
    assert!(read(&opted, 1).is_some());
}

#[test]
fn cold_advanced_owner_keeps_history_identity_before_new_delivery_basis() {
    let (_, mut mixer) = mixer(None, None, false);
    mixer.render(&mut [0.; 4]).unwrap();
    mixer.enable_source_playback_history(4).unwrap();
    mixer.render(&mut [0.; 4]).unwrap();
    let context = context(mixer);
    assert!(context
        .read_source_playback_at(&query(2), 1000, 2, 2)
        .is_none());
    Output::new(2).callback(&context, 22, 4.);
    let selected = context
        .read_source_playback_at(&query(2), 1000, 2, 2)
        .unwrap();
    assert_eq!(selected.playback.physical_start(), 2);
    assert_eq!(selected.snapshot.presentation.unwrap().first_frame, 4);
    assert_eq!(
        (selected.pulled_source_end, selected.delivered_end_frame),
        (6, 6)
    );
    assert!(context
        .read_source_playback_at(&query(2), 1000, 4, 4)
        .is_none());
}

#[test]
fn corrupt_delivery_cannot_extend_lookup_past_actual_pulled_source() {
    let (_, mixer) = mixer(None, None, true);
    let context = context(mixer);
    Output::new(4).callback(&context, 11, 0.);
    context.delivered.store(false, Ordering::SeqCst);
    assert!(read(&context, 0).is_none());
    context.delivered.store(true, Ordering::SeqCst);
    context.delivered_end.store(3, Ordering::SeqCst);
    assert!(read(&context, 3).is_some());
    assert!(read(&context, 4).is_none());
    context.delivered_end.store(5, Ordering::SeqCst);
    assert!(read(&context, 0).is_none());
}

#[test]
fn history_only_helper_simulation_preserves_prior_delivery_and_presentation() {
    // This exercises reader/helper qualification after actual Mixer advancement.
    // It is NOT a reproduced native copy fault or reachable post-render failure.
    let (_, mixer) = mixer(None, None, true);
    let context = context(mixer);
    Output::new(2).callback(&context, 11, 0.);
    let old = read(&context, 2).unwrap();
    context.version.store(3, Ordering::SeqCst);
    // SAFETY: single fixture owner; no callbacks registered or running.
    let state = unsafe { &mut *context.render.get() };
    let RenderOwner::Legacy { mixer, .. } = &mut state.owner else {
        panic!("legacy fixture must retain its original render owner")
    };
    mixer.render(&mut [0.; 4]).unwrap();
    context.render_telemetry.publish_with_source_history(
        None,
        mixer.source_playback_history(),
        &mut state.render_version,
    );
    state.publication_version = 4;
    context.version.store(4, Ordering::SeqCst);
    let selected = read(&context, 2).unwrap();
    assert_eq!(
        (selected.pulled_source_end, selected.delivered_end_frame),
        (4, 2)
    );
    assert_eq!(selected.render, old.render);
    assert_eq!(selected.snapshot.presentation, old.snapshot.presentation);
    assert_eq!(selected.revision, 4);
    assert!(read(&context, 3).is_none());
}

// The handle exposes only atomic read_source_playback_at. It never exposes the
// callback UnsafeCell. The scoped owner guarantees Context remains alive; one
// writer alone invokes callbacks and their existing exclusive callback guard.
struct AtomicReader<'a>(&'a Context);
// SAFETY: read_source_playback_at reads only shared atomic payloads and immutable
// clock/format configuration. No UnsafeCell render state is read by this handle.
unsafe impl Send for AtomicReader<'_> {}
impl AtomicReader<'_> {
    fn read(&self) -> Option<CoreAudioSourcePlaybackSnapshot> {
        read(self.0, 0)
    }
}

#[test]
fn concurrent_atomic_reader_never_crosses_source_report_allcopy_and_native_presentation() {
    let (_, mixer) = mixer(None, None, true);
    let context = context(mixer);
    let mut output = Output::new(1);
    let barrier = std::sync::Barrier::new(2);
    let done = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let reader = AtomicReader(&context);
        let barrier = &barrier;
        let done = &done;
        let reading = scope.spawn(move || {
            barrier.wait();
            while !done.load(Ordering::SeqCst) {
                if let Some(selected) = reader.read() {
                    let report = selected.render.unwrap();
                    let presentation = selected.snapshot.presentation.unwrap();
                    assert_eq!(selected.pulled_source_end, selected.delivered_end_frame);
                    assert_eq!(
                        selected.pulled_source_end,
                        report.start_frame + report.frames as u64
                    );
                    assert_eq!(
                        selected.pulled_source_end,
                        selected.snapshot.rendered_frames
                    );
                    assert_eq!(
                        selected.pulled_source_end,
                        presentation.first_frame + presentation.frames as u64
                    );
                    assert_eq!(selected.pulled_source_end, presentation.host_ticks);
                    assert_eq!(selected.revision, 2 * selected.snapshot.rendered_callbacks);
                    assert_eq!(
                        presentation.native_sample_frame,
                        Some(report.start_frame as f64)
                    );
                }
                std::thread::yield_now();
            }
        });
        barrier.wait();
        for frame in 0..128 {
            output.callback(&context, frame + 1, frame as f64);
            std::thread::yield_now();
        }
        done.store(true, Ordering::SeqCst);
        reading.join().unwrap();
    });
    let selected = read(&context, 128).unwrap();
    assert_eq!(
        (
            selected.pulled_source_end,
            selected.delivered_end_frame,
            selected.revision
        ),
        (128, 128, 256)
    );
}

#[test]
fn retired_owner_keeps_final_legacy_facts_and_complete_mixer_but_no_new_compound_query() {
    let (_, mixer) = mixer(None, None, true);
    let context = context(mixer);
    Output::new(3).callback(&context, 12, 0.);
    let final_snapshot = context.snapshot();
    let final_render_report = context.render_telemetry.read().render;
    // No IOProc/listener was registered. Moving the sole-owned cell after the
    // callback returned models already-confirmed retirement, not OS teardown.
    let RenderOwner::Legacy { mixer, .. } = context.render.into_inner().owner else {
        panic!("legacy fixture must retain its original render owner")
    };
    let basis = mixer.output_frame_basis();
    let mut retired = CoreAudioStream {
        basis,
        applied: CoreAudioApplied {
            request: CoreAudioRequest {
                device: 1,
                format: AudioFormat::new(1000, 2).unwrap(),
                buffer_frames: 8,
            },
            format: AudioFormat::new(1000, 2).unwrap(),
            buffer_frames: 8,
            buffer_channels: vec![1, 1],
            output_domain: ClockDomainId(7),
            output_origin: Timestamp::ZERO,
            native_clock: ClockDomainId(8),
        },
        context: None,
        recovered_owner: Some(RenderOwner::Legacy {
            mixer,
            remix: None,
            scratch: Vec::new(),
        }),
        control: std::sync::Arc::new(SourceControl::new()),
        source_facts: None,
        variable_buffers: false,
        max_callback_frames: 8,
        stop_required: false,
        failed_start: false,
        retired: true,
        proc_id: std::ptr::null_mut(),
        listeners: Vec::new(),
        started: false,
        owner: PhantomData,
        final_snapshot,
        final_render_report,
        final_cadence: None,
    };
    assert!(retired.source_playback_at(&query(1), 1000, 0, 0).is_none());
    assert_eq!(retired.snapshot(), final_snapshot);
    assert_eq!(retired.last_render_report(), final_render_report);
    let recovered = beatkernel::audio::StoppedMixerSource::take_stopped_mixer(&mut retired)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.output_frame_basis(), basis);
    assert_eq!(
        recovered
            .source_playback_history()
            .unwrap()
            .pulled_end_frame(),
        3
    );
    assert!(retired.source_playback_at(&query(1), 1000, 0, 0).is_none());
}

#[test]
fn changed_outer_revision_after_inner_none_retries_actual_new_delivery() {
    let (_, mixer) = mixer(None, None, true);
    let context = context(mixer);
    let mut output = Output::new(2);
    output.callback(&context, 10, 0.);
    assert!(context
        .render_telemetry
        .read_source_playback_at(&query(4), 1000, 0, 0)
        .is_none());
    let mut attempts = 0;
    let selected = context
        .read_source_playback_with(&query(4), 1000, 0, 0, |attempt| {
            assert_eq!(attempt, attempts);
            attempts += 1;
            if attempt == 0 {
                // A real successful callback changes both the previously unavailable
                // inner source query and its outer all-buffer delivery revision.
                output.callback(&context, 20, 2.);
            }
        })
        .unwrap();
    assert_eq!(attempts, 2);
    assert_eq!(
        (
            selected.pulled_source_end,
            selected.delivered_end_frame,
            selected.revision
        ),
        (4, 4, 4)
    );
    assert_eq!(selected.snapshot.presentation.unwrap().host_ticks, 20);
    assert_eq!(selected.render.unwrap().start_frame, 2);
}

#[test]
fn precisely_three_changed_outer_attempts_exhaust_without_returning_crossed_facts() {
    let (_, mixer) = mixer(None, None, true);
    let context = context(mixer);
    let mut output = Output::new(1);
    output.callback(&context, 1, 0.);
    let mut attempts = 0;
    let selected = context.read_source_playback_with(&query(0), 1000, 0, 0, |attempt| {
        assert_eq!(attempt, attempts);
        attempts += 1;
        output.callback(&context, (attempt + 2) as u64, (attempt + 1) as f64);
    });
    assert!(selected.is_none());
    assert_eq!(attempts, 3);
    assert_eq!(frame_cursor(&context), 4);
    let selected = read(&context, 4).unwrap();
    assert_eq!(
        (
            selected.pulled_source_end,
            selected.delivered_end_frame,
            selected.revision
        ),
        (4, 4, 8)
    );
    assert_eq!(selected.snapshot.presentation.unwrap().host_ticks, 4);
}

#[test]
fn actual_history_capacity_refusal_publishes_no_fake_render_or_delivery() {
    let (mut producer, mut mixer) = mixer(None, None, false);
    mixer.enable_source_playback_history(1).unwrap();
    let context = context(mixer);
    Output::new(2).callback(&context, 10, 0.);
    let old = read(&context, 2).unwrap();
    producer.request_pause(true);
    let mut output = Output::new(2);
    output.callback(&context, 20, 2.);
    assert_eq!(output.left, [0.75; 2]);
    assert_eq!(output.right, [0.75; 2]);
    assert_eq!(frame_cursor(&context), 2);
    let selected = read(&context, 2).unwrap();
    assert_eq!(
        (
            selected.pulled_source_end,
            selected.delivered_end_frame,
            selected.revision
        ),
        (2, 2, 4)
    );
    assert_eq!(selected.render, old.render);
    assert_eq!(selected.snapshot.presentation, old.snapshot.presentation);
    assert_eq!(
        (
            selected.snapshot.rendered_callbacks,
            selected.snapshot.rendered_frames,
            selected.snapshot.callback_failures
        ),
        (1, 2, 1)
    );
    assert_eq!(selected.playback.state(), SourcePlaybackState::Running);
    assert!(read(&context, 3).is_none());
}
