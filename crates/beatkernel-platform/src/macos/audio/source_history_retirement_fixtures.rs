//! Owned callback memory only: no registered device or acoustic assertion.
use super::*;
use beatkernel::audio::{command_queue, AudioLimits, MixerConfig, PcmLimits, SampleBank};

fn fixture() -> (
    beatkernel::audio::CommandProducer,
    Context,
    OutputFrameBasis,
) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 64, 1).unwrap()).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(4).unwrap();
    let basis = mixer.output_frame_basis();
    let context = Context {
        control: std::sync::Arc::new(SourceControl::new()),
        complete: false,
        render_telemetry: prepare_render_telemetry(&mixer).unwrap(),
        render: UnsafeCell::new(RenderState {
            owner: RenderOwner::Legacy {
                mixer,
                remix: None,
                scratch: vec![0.; 8],
            },
            render_version: 0,
            publication_version: 0,
            completed_source_retirement: 0,
        }),
        cadence: Box::new(crate::audio::cadence::Capture::new()),
        clock: MachClock::new(ClockDomainId(8), ClockDomainId(9)).unwrap(),
        origin: Timestamp::ZERO,
        output_domain: ClockDomainId(7),
        rate: 1000,
        channels: 1,
        layout: vec![1],
        buffer_frames: 8,
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
    };
    (producer, context, basis)
}

fn callback(context: &Context, frames: usize, valid: bool) {
    let mut pcm = [1_f32; 8];
    let mut list = ffi::AudioBufferList {
        count: 1,
        buffers: [ffi::AudioBuffer {
            channels: if valid { 1 } else { 2 },
            byte_size: (frames * 4) as u32,
            data: pcm.as_mut_ptr().cast(),
        }],
    };
    let time = ffi::AudioTimestamp {
        host_time: 100,
        sample_time: 0.,
        flags: 3,
        ..ffi::AudioTimestamp::default()
    };
    // SAFETY: one owned initialized descriptor and writable PCM remain alive;
    // no driver, concurrent reader, or escaping reference exists.
    unsafe {
        render_callback(
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            &mut list,
            &time,
            (context as *const Context).cast_mut().cast(),
        );
    }
}

fn history_state(context: &Context) -> (u64, u64, u64) {
    // SAFETY: inspected only between synchronous owned-memory callbacks.
    let state = unsafe { &*context.render.get() };
    let RenderOwner::Legacy { mixer, .. } = &state.owner else {
        panic!("legacy fixture must retain its original render owner")
    };
    (
        mixer
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        mixer.frame_cursor(),
        state.completed_source_retirement,
    )
}

#[test]
fn intent_is_applied_only_by_valid_actual_callback_without_extra_delivery() {
    let (mut producer, context, basis) = fixture();
    callback(&context, 2, true);
    producer.request_pause(true);
    callback(&context, 2, true);
    producer.request_pause(false);
    callback(&context, 2, true);
    let before = context.snapshot();
    let (request, allocations) = crate::audio::count_heap_calls(|| {
        context.request_source_history_retirement(basis, (1000, 0, 0), 4)
    });
    let revision = request.unwrap().unwrap();
    assert_eq!(allocations, 0);
    assert_eq!(history_state(&context), (0, 6, 0));
    assert_eq!(context.snapshot(), before);
    let ((), allocations) = crate::audio::count_heap_calls(|| callback(&context, 2, true));
    assert_eq!(allocations, 0);
    assert_eq!(history_state(&context), (4, 8, revision));
    let after = context.snapshot();
    assert_eq!(after.rendered_callbacks, before.rendered_callbacks + 1);
    assert_eq!(after.rendered_frames, before.rendered_frames + 2);
    assert_eq!(after.callback_failures, 0);
    assert_eq!(context.delivered_end.load(Ordering::SeqCst), 8);
}

#[test]
fn invalid_native_buffers_leave_requested_retirement_and_mixer_untouched() {
    let (mut producer, context, basis) = fixture();
    callback(&context, 2, true);
    producer.request_pause(true);
    callback(&context, 2, true);
    context
        .request_source_history_retirement(basis, (1000, 0, 0), 2)
        .unwrap()
        .unwrap();
    let before = history_state(&context);
    callback(&context, 2, false);
    assert_eq!(history_state(&context), before);
    assert_eq!(context.calls.load(Ordering::Relaxed), 2);
    assert_eq!(context.delivered_end.load(Ordering::SeqCst), 4);
    assert_eq!(context.failures.load(Ordering::Relaxed), 1);
    assert_eq!(
        context.request_source_history_retirement(basis, (1000, 0, 0), 2),
        Err(CoreAudioError::InvalidRequest)
    );
}

#[test]
fn unpublished_or_odd_callback_cannot_admit_retirement_and_invalid_identity_refuses() {
    let (_, context, basis) = fixture();
    assert_eq!(
        context
            .request_source_history_retirement(basis, (1000, 0, 0), 0)
            .unwrap(),
        None
    );
    callback(&context, 2, true);
    assert!(context
        .request_source_history_retirement(basis, (999, 0, 0), 1)
        .is_err());
    assert!(context
        .request_source_history_retirement(basis, (1000, 0, 0), 3)
        .is_err());
    context.version.store(3, Ordering::SeqCst);
    assert_eq!(
        context
            .request_source_history_retirement(basis, (1000, 0, 0), 1)
            .unwrap(),
        None
    );
    context.version.store(2, Ordering::SeqCst);
    context.changed.store(true, Ordering::Release);
    assert_eq!(
        context.request_source_history_retirement(basis, (1000, 0, 0), 1),
        Err(CoreAudioError::ConfigurationChanged)
    );
    assert_eq!(history_state(&context), (0, 2, 0));
}

#[test]
fn repeated_retirement_allows_more_real_pause_transitions_than_cold_capacity() {
    let (mut producer, context, basis) = fixture();
    callback(&context, 2, true);
    for turn in 1_u64..65 {
        let pin = turn * 2;
        let revision = context
            .request_source_history_retirement(basis, (1000, 0, 0), pin)
            .unwrap()
            .unwrap();
        producer.request_pause(turn % 2 == 1);
        let ((), allocations) = crate::audio::count_heap_calls(|| callback(&context, 2, true));
        assert_eq!(allocations, 0);
        let (retained, frame, completed) = history_state(&context);
        assert!(retained <= pin);
        assert_eq!(frame, pin + 2);
        assert_eq!(completed, revision);
        // SAFETY: synchronous callback has returned and no device owns state.
        assert!(
            unsafe {
                let state = &*context.render.get();
                let RenderOwner::Legacy { mixer, .. } = &state.owner else {
                    panic!("legacy fixture must retain its original render owner")
                };
                mixer.source_playback_history().unwrap().len()
            } <= 4
        );
    }
    assert_eq!(context.calls.load(Ordering::Relaxed), 65);
    assert_eq!(context.frames.load(Ordering::Relaxed), 130);
    assert_eq!(context.failures.load(Ordering::Relaxed), 0);
}

#[test]
fn queued_revision_survives_callback_application_and_later_native_failure() {
    let (mut producer, context, basis) = fixture();
    callback(&context, 2, true);
    producer.request_pause(true);
    callback(&context, 2, true);
    producer.request_pause(false);
    callback(&context, 2, true);
    assert_eq!(history_state(&context), (0, 6, 0));
    let outer_before = context.version.load(Ordering::SeqCst);
    let (queued, allocations) = crate::audio::count_heap_calls(|| {
        context.request_source_history_retirement_with(basis, (1000, 0, 0), 4, || {
            callback(&context, 2, true);
            callback(&context, 2, false);
        })
    });
    assert_eq!(allocations, 0);
    let revision = queued
        .unwrap()
        .expect("the sole queue effect remains reported");
    assert_eq!(history_state(&context), (4, 8, revision));
    assert_eq!(context.version.load(Ordering::SeqCst), outer_before + 4);
    assert_eq!(context.calls.load(Ordering::Relaxed), 4);
    assert_eq!(context.frames.load(Ordering::Relaxed), 8);
    assert_eq!(context.failures.load(Ordering::Relaxed), 1);
    assert_eq!(context.delivered_end.load(Ordering::SeqCst), 8);
    assert_eq!(
        context.request_source_history_retirement(basis, (1000, 0, 0), 4),
        Err(CoreAudioError::InvalidRequest),
    );
}
