use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

use beatkernel::transport::Rate;
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}
struct Allocator;
fn count(kind: usize) {
    let _ = TRACK.try_with(|track| {
        if track.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut value = counts.get();
                value[kind] += 1;
                counts.set(value);
            });
        }
    });
}
// SAFETY: The allocator forwards unmodified pointer/layout arguments to System;
// bookkeeping only touches allocation-free thread-local scalar cells.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: GlobalAlloc caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: GlobalAlloc caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        // SAFETY: Caller supplies a live System allocation, matching layout and
        // nonzero new size under the GlobalAlloc contract.
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count(2);
        // SAFETY: Caller supplies the live original pointer and layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn track<T>(operation: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|counts| counts.set([0; 3]));
    TRACK.with(|track| track.set(true));
    let result = operation();
    TRACK.with(|track| track.set(false));
    (result, COUNTS.with(Cell::get))
}
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn rig(limits: AudioLimits) -> (CommandProducer, Mixer) {
    rig_end(limits, None)
}
fn rig_end(limits: AudioLimits, end: Option<u64>) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm_limits = PcmLimits::new(64, 64, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.0], pcm_limits).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(limits.queue_capacity()).unwrap();
    let mut config = MixerConfig::new(format, ClockDomainId(9), Timestamp::ZERO, limits);
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    (producer, Mixer::new(config, bank, consumer).unwrap())
}

#[test]
fn immutable_fence_prefix_silent_suffix_and_terminal_settlement_remain_rt_safe() {
    let (mut producer, mut mixer) = rig_end(AudioLimits::new(4, 1, 4, 8, 4).unwrap(), Some(3));
    producer.try_push(play(1, 0)).unwrap();
    producer.try_push(play(1, 3_000_000)).unwrap();
    let mut one = [99.0];
    render(&mut mixer, &mut one);
    assert_eq!(one, [0.25]);
    producer.request_pause(true);
    let mut silence = [99.0; 2];
    let manually_paused = render(&mut mixer, &mut silence);
    assert_eq!(manually_paused.playback_frames, 0);
    assert_eq!(manually_paused.playback_end_physical_frame, None);
    producer.request_pause(false);
    let mut crossing = [99.0; 4];
    let report = render(&mut mixer, &mut crossing);
    assert_eq!(crossing, [0.5, 0.75, 0.0, 0.0]);
    assert_eq!(report.playback_frames, 2);
    assert!(report.paused);
    assert_eq!(report.playback_end_physical_frame, Some(5));
    assert_eq!(report.active_voices, 1);
    assert_eq!(report.pending_commands, 1);
    producer.try_push(play(2, 3_000_000)).unwrap();
    producer.request_pause(false);
    let frozen = render(&mut mixer, &mut crossing);
    assert_eq!(crossing, [0.0; 4]);
    assert_eq!(frozen.counters.commands_consumed, 3);
    assert_eq!(frozen.counters.commands_applied, 1);
    assert_eq!(frozen.counters.commands_expired, 2);
    assert_eq!(frozen.pending_commands, 0);
    assert_eq!(frozen.active_voices, report.active_voices);
    assert_eq!(frozen.song_position, report.song_position);
    assert_eq!(frozen.playback_frames, 0);
    assert!(frozen.paused);
    assert_eq!(frozen.playback_end_physical_frame, Some(5));
    assert_eq!(render(&mut mixer, &mut []).playback_frames, 0);
    let mut invalid = [99.0; 9];
    let (result, counts) = track(|| mixer.render(&mut invalid));
    assert_eq!(counts, [0, 0, 0]);
    assert_eq!(result, Err(AudioError::RenderCapacity));
    assert_eq!(invalid, [99.0; 9]);
    assert_eq!(
        render(&mut mixer, &mut []).playback_end_physical_frame,
        Some(5)
    );
    let (mut producer, mut zero) = rig_end(AudioLimits::new(1, 1, 1, 8, 1).unwrap(), Some(0));
    producer.try_push(play(1, 0)).unwrap();
    let report = render(&mut zero, &mut one);
    assert_eq!(one, [0.0]);
    assert!(report.paused);
    assert_eq!(report.playback_end_physical_frame, Some(0));
    assert_eq!(report.counters.commands_consumed, 1);
    assert_eq!(report.counters.commands_applied, 0);
    assert_eq!(report.counters.commands_expired, 1);
    assert_eq!(report.pending_commands, 0);
}
fn play(voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(1),
        at: ts(at),
        gain: 1.0,
    }
}
fn render(mixer: &mut Mixer, output: &mut [f32]) -> RenderReport {
    let (result, counts) = track(|| mixer.render(output));
    // Assertions and formatting are outside the measured callback boundary.
    assert_eq!(counts, [0, 0, 0], "alloc/realloc/dealloc during render");
    result.unwrap()
}

#[test]
fn allocator_calibration_observes_each_operation() {
    let (_, counts) = track(|| {
        let mut data = Vec::with_capacity(1);
        data.push(1_u8);
        data.reserve_exact(1024);
        std::hint::black_box(&data);
        drop(data);
    });
    assert!(counts.iter().all(|count| *count > 0));
}

#[test]
fn silence_active_natural_completion_stop_seek_invalid_and_disconnect_are_rt_safe() {
    let (mut producer, mut mixer) = rig(AudioLimits::new(16, 1, 16, 8, 16).unwrap());
    let mut one = [99.0];
    assert_eq!(render(&mut mixer, &mut one).active_voices, 0);
    assert_eq!(one, [0.0]);
    producer.try_push(play(1, 1_000_000)).unwrap();
    assert_eq!(render(&mut mixer, &mut one).active_voices, 1);
    assert_eq!(one, [0.25]);
    let mut completion = [99.0; 4];
    assert_eq!(render(&mut mixer, &mut completion).active_voices, 0);
    assert_eq!(completion, [0.5, 0.75, 1.0, 0.0]);
    producer.try_push(play(1, 6_000_000)).unwrap();
    producer
        .try_push(AudioCommand::Stop {
            voice: VoiceId(1),
            at: ts(7_000_000),
        })
        .unwrap();
    let mut two = [99.0; 2];
    assert_eq!(render(&mut mixer, &mut two).active_voices, 0);
    assert_eq!(two, [0.25, 0.0]);
    producer.try_push(play(1, 8_000_000)).unwrap();
    producer
        .try_push(AudioCommand::Seek {
            song_time: ts(-999),
            at: ts(9_000_000),
        })
        .unwrap();
    let report = render(&mut mixer, &mut two);
    assert_eq!(two, [0.25, 0.0]);
    assert_eq!(report.song_position, ts(-999));
    assert_eq!(report.active_voices, 0);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: ts(10_000_000),
            gain: f32::NAN,
        })
        .unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(99),
            at: ts(10_000_000),
            gain: 1.0,
        })
        .unwrap();
    producer.try_push(play(1, 10_000_000)).unwrap();
    producer.try_push(play(2, 10_000_000)).unwrap();
    let report = render(&mut mixer, &mut one);
    assert_eq!(one, [0.25]);
    assert_eq!(report.counters.invalid_gains, 1);
    assert_eq!(report.counters.unknown_samples, 1);
    assert_eq!(report.counters.voice_full, 1);
    let mut invalid = [99.0; 9];
    let previous = mixer.counters();
    let (result, counts) = track(|| mixer.render(&mut invalid));
    assert_eq!(counts, [0, 0, 0]);
    assert_eq!(result, Err(AudioError::RenderCapacity));
    assert_eq!(invalid, [99.0; 9]);
    assert_eq!(mixer.counters(), previous);
    drop(producer); // Endpoint and asset ownership changes happen off callback.
    let report = render(&mut mixer, &mut completion);
    assert!(report.producer_disconnected);
    assert_eq!(report.active_voices, 0);
    assert_eq!(completion, [0.5, 0.75, 1.0, 0.0]);
    assert_eq!(render(&mut mixer, &mut one).active_voices, 0);
    assert_eq!(one, [0.0]);
    drop(mixer);
}

#[test]
fn pending_full_rejection_and_future_execution_do_not_allocate_or_free() {
    let (mut producer, mut mixer) = rig(AudioLimits::new(4, 1, 1, 8, 4).unwrap());
    producer.try_push(play(1, 4_000_000)).unwrap();
    producer.try_push(play(2, 0)).unwrap();
    let mut silence = [99.0; 4];
    let report = render(&mut mixer, &mut silence);
    assert_eq!(silence, [0.0; 4]);
    assert_eq!(report.counters.pending_full, 1);
    assert_eq!(report.pending_commands, 1);
    drop(producer);
    let mut asset = [99.0; 4];
    let report = render(&mut mixer, &mut asset);
    assert_eq!(asset, [0.25, 0.5, 0.75, 1.0]);
    assert_eq!(report.active_voices, 0);
    assert!(report.producer_disconnected);
    drop(mixer);
}

#[test]
fn rate_changes_and_rational_overflow_rejection_do_not_allocate_or_free() {
    let (mut producer, mut mixer) = rig(AudioLimits::new(8, 2, 8, 8, 8).unwrap());
    let retained = Rate::new(1, u64::MAX).unwrap();
    producer
        .try_push(AudioCommand::SetRate {
            rate: retained,
            at: ts(0),
        })
        .unwrap();
    producer.try_push(play(1, 0)).unwrap();
    let mut one = [99.0];
    render(&mut mixer, &mut one);
    assert_eq!(one, [0.25]);
    producer
        .try_push(AudioCommand::SetRate {
            rate: Rate::new(1, u64::MAX - 2).unwrap(),
            at: ts(1_000_000),
        })
        .unwrap();
    let report = render(&mut mixer, &mut one);
    assert_eq!(report.counters.invalid_rates, 1);
    assert_eq!(mixer.rate(), retained);
    assert_eq!(one, [0.25]);
    producer
        .try_push(AudioCommand::SetRate {
            rate: Rate::ZERO,
            at: ts(2_000_000),
        })
        .unwrap();
    let mut paused = [99.0; 2];
    render(&mut mixer, &mut paused);
    assert_eq!(paused, [0.0; 2]);
    producer
        .try_push(AudioCommand::SetRate {
            rate: Rate::REVERSE,
            at: ts(4_000_000),
        })
        .unwrap();
    let mut reverse = [99.0; 2];
    let report = render(&mut mixer, &mut reverse);
    assert_eq!(reverse, [0.25, 0.0]);
    assert_eq!(report.active_voices, 0);
    drop(producer);
    drop(mixer);
}

#[test]
fn bounded_consume_budget_and_empty_render_do_not_allocate_or_free() {
    let (mut producer, mut mixer) = rig(AudioLimits::new(4, 2, 4, 8, 1).unwrap());
    for voice in 1..=3 {
        producer.try_push(play(voice, 0)).unwrap();
    }
    let report = render(&mut mixer, &mut []);
    assert_eq!(report.counters.commands_consumed, 0);
    let mut one = [99.0];
    for consumed in 1..=3 {
        let report = render(&mut mixer, &mut one);
        assert_eq!(report.counters.commands_consumed, consumed);
    }
    assert_eq!(mixer.counters().voice_full, 1);
    drop(producer);
    drop(mixer);
}

#[test]
fn requested_pause_silence_and_resume_preserve_rt_storage_and_voice_phase() {
    let (mut producer, mut mixer) = rig(AudioLimits::new(2, 1, 2, 8, 2).unwrap());
    producer.try_push(play(1, 0)).unwrap();
    let mut one = [99.0];
    render(&mut mixer, &mut one);
    assert_eq!(one, [0.25]);
    // Keep both the ring and a live asset owned across the paused callback.
    producer.try_push(play(1, 10_000_000)).unwrap();
    producer.try_push(play(1, 11_000_000)).unwrap();
    let ((), counts) = track(|| producer.request_pause(true));
    assert_eq!(counts, [0, 0, 0]);
    let mut silence = [99.0; 3];
    let paused = render(&mut mixer, &mut silence);
    assert_eq!(silence, [0.0; 3]);
    assert!(paused.paused);
    assert_eq!(paused.playback_start_frame, 1);
    assert_eq!(paused.playback_frames, 0);
    assert_eq!(paused.counters.commands_consumed, 1);
    let ((), counts) = track(|| producer.request_pause(false));
    assert_eq!(counts, [0, 0, 0]);
    let resumed = render(&mut mixer, &mut one);
    assert_eq!(one, [0.5]);
    assert_eq!(resumed.start_frame, 4);
    assert_eq!(resumed.playback_start_frame, 1);
    assert_eq!(resumed.playback_frames, 1);
    assert!(!resumed.paused);
    assert_eq!(resumed.pending_commands, 2);
}

fn runtime_practice(
    cues: Vec<PracticeCue>,
    practice_limits: PracticeLimits,
) -> (Mixer, CommandProducer, PracticeController) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm_limits = PcmLimits::new(64, 64, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.0], pcm_limits).unwrap(),
    )
    .unwrap();
    let program = PreparedPracticeProgram::new_with_runtime_rates(
        &bank,
        cues,
        practice_limits,
        Rate::NORMAL,
        beatkernel::time::Duration::ZERO,
        false,
        0x14043,
    )
    .unwrap();
    let (controller, endpoint) = practice_queue(&program).unwrap();
    let (producer, consumer) = command_queue(16).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(9),
            ts(0),
            AudioLimits::new(16, 4, 16, 32, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer
        .install_practice(
            program,
            endpoint,
            PracticeRegion::new(ts(0), ts(20_000_000), false).unwrap(),
        )
        .unwrap();
    (mixer, producer, controller)
}
fn runtime_cue(voice: u64, at: i64) -> PracticeCue {
    PracticeCue {
        voice: VoiceId(voice),
        sample: SampleId(1),
        at: ts(at),
        gain: 1.0,
    }
}

#[test]
fn retained_runtime_rate_and_compound_restart_callbacks_use_no_heap() {
    let (mut mixer, _producer, mut controller) = runtime_practice(
        vec![runtime_cue(1, 0)],
        PracticeLimits::new(8, 1, 64, 8, 8).unwrap(),
    );
    render(&mut mixer, &mut [0.0]);
    assert!(matches!(
        controller.try_pop_event().unwrap(),
        PracticeEvent::Boundary(_)
    ));
    let prepared = controller.prepare_rate(Rate::new(2, 1).unwrap()).unwrap();
    let (admitted, counts) = track(|| controller.try_set_rate_next(1, 1, &prepared));
    assert_eq!(counts, [0, 0, 0]);
    admitted.unwrap();
    let mut changed = [99.0; 2];
    render(&mut mixer, &mut changed);
    assert_eq!(changed, [0.5, 1.0]);
    let (event, counts) = track(|| controller.try_pop_projected_event());
    assert_eq!(counts, [0, 0, 0]);
    assert!(
        matches!(event.unwrap().event, PracticeEvent::Rate(rate) if rate.outcome == PracticeRateOutcome::Applied)
    );
    let restore = controller.prepare_rate(Rate::NORMAL).unwrap();
    let original = PracticeRegion::new(ts(0), ts(20_000_000), false).unwrap();
    let (admitted, counts) = track(|| controller.try_restart_next(2, 1, original, &restore));
    assert_eq!(counts, [0, 0, 0]);
    admitted.unwrap();
    let mut restarted = [99.0; 3];
    render(&mut mixer, &mut restarted);
    assert_eq!(restarted, [0.25, 0.5, 0.75]);
    let (event, counts) = track(|| controller.try_pop_event());
    assert_eq!(counts, [0, 0, 0]);
    assert!(
        matches!(event.unwrap(), PracticeEvent::Restart(restart) if restart.outcome == PracticeRateOutcome::Applied)
    );
}

#[test]
fn retained_runtime_capacity_refusal_continues_pcm_without_heap_or_fake_credits() {
    let (mut mixer, _producer, mut controller) = runtime_practice(
        vec![
            runtime_cue(1, 0),
            runtime_cue(2, 4_000_000),
            runtime_cue(3, 8_000_000),
        ],
        PracticeLimits::new(8, 1, 64, 8, 8).unwrap(),
    );
    render(&mut mixer, &mut [0.0]);
    controller.try_pop_event().unwrap();
    let prepared = controller.prepare_rate(Rate::new(8, 1).unwrap()).unwrap();
    controller.try_set_rate_next(1, 1, &prepared).unwrap();
    let mut old_pcm = [99.0; 4];
    let report = render(&mut mixer, &mut old_pcm);
    assert_eq!(old_pcm, [0.5, 0.75, 1.0, 0.25]);
    let (event, counts) = track(|| controller.try_pop_event());
    assert_eq!(counts, [0, 0, 0]);
    assert!(
        matches!(event.unwrap(), PracticeEvent::Rate(rate) if rate.outcome == PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity))
    );
    assert_eq!(controller.metadata().unwrap().revision, 0);
    assert_eq!(report.counters.commands_applied, 0);
    assert_eq!(report.counters.voice_full, 0);
}

#[test]
fn retained_runtime_preflight_budget_error_is_allocation_free_and_transactional() {
    let (mut mixer, _producer, mut controller) = runtime_practice(
        vec![runtime_cue(1, 1_000_000), runtime_cue(2, 3_000_000)],
        PracticeLimits::new(8, 2, 2, 8, 8).unwrap(),
    );
    render(&mut mixer, &mut [0.0]);
    controller.try_pop_event().unwrap();
    let prepared = controller.prepare_rate(Rate::new(2, 1).unwrap()).unwrap();
    controller.try_set_rate_next(1, 1, &prepared).unwrap();
    let before = mixer.render(&mut []).unwrap();
    let metadata = controller.metadata().unwrap();
    let mut untouched = [99.0; 2];
    let (result, counts) = track(|| mixer.render(&mut untouched));
    assert_eq!(counts, [0, 0, 0]);
    assert_eq!(result, Err(AudioError::PracticeEventBudget));
    assert_eq!(untouched, [99.0; 2]);
    assert_eq!(controller.metadata().unwrap(), metadata);
    assert_eq!(mixer.render(&mut []).unwrap(), before);
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    // Control1+cue1 fits the minimum frame, while the failed two-frame
    // callback also contains the next cue and exceeds the unchanged budget2.
    let mut one = [99.0];
    render(&mut mixer, &mut one);
    assert_eq!(one, [0.25]);
    let (event, counts) = track(|| controller.try_pop_event());
    assert_eq!(counts, [0, 0, 0]);
    match event.unwrap() {
        PracticeEvent::Rate(event) => {
            assert_eq!(event.outcome, PracticeRateOutcome::Applied);
            assert_eq!((event.request_id, event.generation), (1, 1));
            assert_eq!((event.request_revision, event.applied_revision), (0, 1));
            assert_eq!((event.physical_frame, event.playback_frame), (1, 1));
        }
        event => panic!("expected preserved rate after a smaller retry, got {event:?}"),
    }
    let mut suffix = [99.0; 2];
    let report = render(&mut mixer, &mut suffix);
    assert_eq!(suffix, [1.0, 0.75]);
    assert_eq!(report.counters.voice_full, 0);
    assert_eq!(report.counters.commands_applied, 0);
    assert_eq!(controller.metadata().unwrap().revision, 1);
    assert_eq!(controller.history_retention_pins(), (None, None));
}

#[test]
fn retained_runtime_minimum_budget_refusal_keeps_pcm_and_control_ownership_without_heap() {
    let (mut mixer, _producer, mut controller) = runtime_practice(
        vec![runtime_cue(1, 1_000_000)],
        PracticeLimits::new(8, 1, 1, 8, 8).unwrap(),
    );
    render(&mut mixer, &mut [0.0]);
    controller.try_pop_event().unwrap();
    let metadata = controller.metadata().unwrap();
    let prepared = controller.prepare_rate(Rate::new(2, 1).unwrap()).unwrap();
    let (result, counts) = track(|| controller.try_set_rate_next(1, 1, &prepared));
    assert_eq!(counts, [0, 0, 0]);
    assert_eq!(result, Err(PracticeError::EventBudget));
    let original = PracticeRegion::new(ts(0), ts(20_000_000), false).unwrap();
    let (result, counts) = track(|| controller.try_restart_next(1, 1, original, &prepared));
    assert_eq!(counts, [0, 0, 0]);
    assert_eq!(result, Err(PracticeError::EventBudget));
    assert_eq!(controller.metadata().unwrap(), metadata);
    assert_eq!(controller.history_retention_pins(), (None, None));
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
    let mut one = [99.0];
    let report = render(&mut mixer, &mut one);
    assert_eq!(one, [0.25]);
    assert_eq!(report.counters.commands_applied, 0);
    assert_eq!(report.counters.voice_full, 0);
    assert_eq!(controller.metadata().unwrap(), metadata);
    let empty = PracticeRegion::new(ts(8_000_000), ts(20_000_000), false).unwrap();
    let (result, counts) = track(|| controller.try_request_next(1, 1, empty));
    assert_eq!(counts, [0, 0, 0]);
    result.unwrap();
    render(&mut mixer, &mut one);
    assert_eq!(one, [0.0]);
    let (event, counts) = track(|| controller.try_pop_event());
    assert_eq!(counts, [0, 0, 0]);
    match event.unwrap() {
        PracticeEvent::Boundary(boundary) => {
            assert_eq!(boundary.kind, PracticeBoundaryKind::Requested);
            assert_eq!((boundary.request_id, boundary.generation), (1, 2));
            assert_eq!((boundary.physical_frame, boundary.playback_frame), (2, 2));
            assert_eq!(boundary.applied_song_time, ts(8_000_000));
        }
        event => panic!("expected usable control after cold refusal, got {event:?}"),
    }
    assert_eq!(controller.metadata().unwrap().revision, 1);
    assert_eq!(controller.history_retention_pins(), (None, None));
}

#[test]
fn live_portable_rate_binding_and_token_drop_use_no_heap() {
    let (mut mixer, _producer, mut controller) = runtime_practice(
        vec![runtime_cue(1, 0)],
        PracticeLimits::new(8, 1, 64, 8, 8).unwrap(),
    );
    render(&mut mixer, &mut [0.0]);
    controller.try_pop_event().unwrap();
    let geometry = [PracticeRateCueGeometry {
        at: ts(0),
        source_frames: 4,
        source_hz: 1000,
    }];
    // Geometry preparation and wire decoding are producer-side cold work.
    // Only the active Worklet's bounded bind and temporary token lifetime
    // belong to the measured message boundary below.
    let cold = prepare_practice_rate(0x14043, 1000, &geometry, Rate::new(3, 2).unwrap()).unwrap();
    let decoded = PracticeRatePreparation::from_words(cold.to_words()).unwrap();
    let metadata = controller.metadata().unwrap();
    assert!(metadata.started && metadata.active);
    let (bound, counts) = track(|| -> Result<(), PracticeError> {
        let token =
            controller.bind_rate_preparation(metadata.generation, metadata.revision, decoded)?;
        std::hint::black_box(&token);
        drop(token);
        Ok(())
    });
    assert_eq!(
        counts,
        [0, 0, 0],
        "live bounded bind/drop alloc/realloc/dealloc"
    );
    bound.unwrap();
    assert_eq!(controller.metadata().unwrap(), metadata);
    // Both original owners remain alive throughout the measured drop.
    let mut continuing = [99.0];
    render(&mut mixer, &mut continuing);
    assert_eq!(continuing, [0.5]);
    assert_eq!(mixer.rate(), Rate::NORMAL);
}
