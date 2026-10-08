//! Scripted native operations exercise the production pump, not a second pump.
use super::*;
use beatkernel::audio::{command_queue, AudioCommand, AudioFormat, AudioLimits,
    CommandProducer, MixerConfig, PcmLimits, PcmSample, SampleBank, SampleId,
    StoppedMixerSource, VoiceId};
use std::cell::Cell;

const PCM: [f32; 8] = [0.125, -0.25, 0.375, -0.5, 0.625, -0.75, 0.875, -1.0];

fn rig(period: usize) -> (CommandProducer, NativeOutputState, AlsaAppliedConfig) {
    let format = AudioFormat::new(8, 1).unwrap();
    let limits = PcmLimits::new(128, 512, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(SampleId(1), PcmSample::new(format, PCM.to_vec(), limits).unwrap()).unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer.try_push(AudioCommand::Play {
        voice: VoiceId(1), sample: SampleId(1), at: Timestamp::ZERO, gain: 1.0,
    }).unwrap();
    let mixer = Mixer::new(MixerConfig::new(format, ClockDomainId(7), Timestamp::ZERO,
        AudioLimits::new(16, 4, 16, 16, 16).unwrap()), bank, consumer).unwrap();
    let device = DeviceFormat::new(8, 1, SampleEncoding::Float32, None).unwrap();
    let request = AlsaRequest { device: "memory-never-native".into(), format: device,
        buffer_frames: 16, period_frames: period as u32, allow_size_rounding: false,
        monotonic_domain: ClockDomainId(8) };
    let config = AlsaAppliedConfig { requested: request, format: device, buffer_frames: 16,
        period_frames: period as u32, sizing_adjusted: false,
        output_domain: ClockDomainId(7), output_origin: Timestamp::ZERO };
    let state = NativeOutputState::new(mixer, device, None, period)
        .unwrap_or_else(|_| panic!("valid prepared state"));
    (producer, state, config)
}

#[derive(Clone, Copy)]
enum Write { Count(usize), Again, Interrupted, Fail, Excess }

struct Pcm<'a> {
    shared: &'a Shared,
    steps: &'a [Write],
    writes: usize,
    waits: usize,
    wait_ready: bool,
    wait_fail: bool,
    timing_fail: bool,
    drop_fail: bool,
    stop_after: usize,
    stop_on_wait: bool,
    output: [f32; 64],
    length: usize,
    timing_calls: usize,
    drops: usize,
}
impl<'a> Pcm<'a> {
    fn new(shared: &'a Shared, steps: &'a [Write], stop_after: usize) -> Self {
        Self { shared, steps, writes: 0, waits: 0, wait_ready: true, wait_fail: false,
            timing_fail: false, drop_fail: false, stop_after, stop_on_wait: false,
            output: [0.0; 64], length: 0, timing_calls: 0, drops: 0 }
    }
}
impl PcmOperations for Pcm<'_> {
    fn write(&mut self, bytes: &[u8], frames: usize) -> Result<Option<usize>, LinuxError> {
        let step = self.steps.get(self.writes).copied().unwrap_or(Write::Count(frames));
        self.writes += 1;
        if self.writes == self.stop_after { self.shared.stop.store(true, Ordering::Release); }
        match step {
            Write::Again | Write::Interrupted => Ok(None),
            Write::Fail => Err(LinuxError::Alsa { operation: "script write", code: -32 }),
            Write::Excess => Ok(Some(frames + 1)),
            Write::Count(count) => {
                assert!(count <= frames);
                for chunk in bytes[..count * 4].chunks_exact(4) {
                    self.output[self.length] = f32::from_le_bytes(chunk.try_into().unwrap());
                    self.length += 1;
                }
                Ok(Some(count))
            }
        }
    }
    fn wait(&mut self) -> Result<bool, LinuxError> {
        self.waits += 1;
        if self.stop_on_wait { self.shared.stop.store(true, Ordering::Release); }
        if self.wait_fail { Err(LinuxError::Alsa { operation: "script wait", code: -32 }) }
        else { Ok(self.wait_ready) }
    }
    fn timing<C: WorkerClock>(&mut self, _clock: &C, _submitted: u64)
        -> Result<Option<AlsaTimingSnapshot>, LinuxError> {
        self.timing_calls += 1;
        if self.timing_fail { Err(LinuxError::Alsa { operation: "script timing", code: -32 }) }
        else { Ok(None) }
    }
    fn drop_stream(&mut self) -> Result<(), LinuxError> {
        self.drops += 1;
        if self.drop_fail { Err(LinuxError::Alsa { operation: "script drop", code: -32 }) }
        else { Ok(()) }
    }
}
struct Clock { calls: Cell<usize>, fail_at: usize }
impl Clock { fn new(fail_at: usize) -> Self { Self { calls: Cell::new(0), fail_at } } }
impl WorkerClock for Clock {
    fn now(&self) -> Result<ClockPoint, LinuxError> {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        if call == self.fail_at { return Err(LinuxError::InvalidConfiguration("script clock")); }
        Ok(ClockPoint { domain: ClockDomainId(8), timestamp: Timestamp::from_nanos(call as i64) })
    }
}
struct Encoder { fail: bool }
impl PcmEncoder for Encoder {
    fn encode(&mut self, format: DeviceFormat, samples: &[f32], bytes: &mut [u8])
        -> Result<(), LinuxError> {
        if self.fail { Err(LinuxError::InvalidConfiguration("script encoding")) }
        else { encode_pcm(format, samples, bytes).map_err(LinuxError::Conversion) }
    }
}
fn pump(pcm: &mut Pcm<'_>, state: &mut NativeOutputState, config: &AlsaAppliedConfig,
    clock: &Clock, encoder: &mut Encoder) -> Result<(), LinuxError> {
    let mut bytes = [0u8; 64];
    let shared = pcm.shared;
    run_worker(pcm, state, config, &mut bytes[..config.period_frames as usize * 4],
        shared, clock, encoder)
}

#[test]
fn actual_pump_full_write_and_short_prefix_deliver_distinct_pcm_once() {
    for steps in [&[Write::Count(4)][..], &[Write::Count(1), Write::Count(3)][..]] {
        let (_producer, mut state, config) = rig(4);
        let shared = Shared::new();
        let mut pcm = Pcm::new(&shared, steps, steps.len());
        pump(&mut pcm, &mut state, &config, &Clock::new(0), &mut Encoder { fail: false }).unwrap();
        assert_eq!(&pcm.output[..pcm.length], &PCM[..4]);
        assert_eq!(state.pending_frames(), 0);
        assert_eq!(state.mixer().frame_cursor(), 4);
        assert_eq!(shared.submitted.load(Ordering::Relaxed), 4);
        assert_eq!(shared.rendered.load(Ordering::Relaxed), 4);
        assert_eq!(shared.renders.load(Ordering::Relaxed), 1);
        assert_eq!(pcm.drops, 1);
    }
}

#[test]
fn no_progress_wait_variants_retain_one_block_without_another_source_pull() {
    for step in [Write::Count(0), Write::Again, Write::Interrupted] {
        for ready in [false, true] {
            let (_producer, mut state, config) = rig(4);
            let shared = Shared::new();
            let steps = [step];
            let mut pcm = Pcm::new(&shared, &steps, usize::MAX);
            pcm.wait_ready = ready;
            pcm.stop_on_wait = true;
            pump(&mut pcm, &mut state, &config, &Clock::new(0), &mut Encoder { fail: false }).unwrap();
            assert_eq!(pcm.waits, 1);
            assert_eq!(pcm.length, 0);
            assert_eq!(state.pending_samples(), &PCM[..4]);
            assert_eq!(state.mixer().frame_cursor(), 4);
            assert_eq!(shared.submitted.load(Ordering::Relaxed), 0);
            assert_eq!(shared.renders.load(Ordering::Relaxed), 1);
        }
    }
}

#[test]
fn write_wait_and_excess_count_refusals_preserve_exact_unadmitted_block() {
    for step in [Write::Fail, Write::Again, Write::Excess] {
        let (_producer, mut state, config) = rig(4);
        let shared = Shared::new();
        let steps = [step];
        let mut pcm = Pcm::new(&shared, &steps, usize::MAX);
        pcm.wait_fail = true;
        assert!(pump(&mut pcm, &mut state, &config, &Clock::new(0), &mut Encoder { fail: false }).is_err());
        assert_eq!(state.pending_samples(), &PCM[..4]);
        assert_eq!(state.admitted_frames(), 0);
        assert_eq!(state.mixer().frame_cursor(), 4);
        assert_eq!(shared.submitted.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn encoding_refusal_after_render_preserves_pcm_and_retry_does_not_render_again() {
    let (_producer, mut state, config) = rig(4);
    let shared = Shared::new();
    let mut pcm = Pcm::new(&shared, &[], 1);
    assert!(pump(&mut pcm, &mut state, &config, &Clock::new(0), &mut Encoder { fail: true }).is_err());
    assert_eq!(state.pending_samples(), &PCM[..4]);
    assert_eq!(state.mixer().frame_cursor(), 4);
    assert_eq!(pcm.writes, 0);
    let retry = Shared::new();
    let mut pcm = Pcm::new(&retry, &[], 1);
    pump(&mut pcm, &mut state, &config, &Clock::new(0), &mut Encoder { fail: false }).unwrap();
    assert_eq!(&pcm.output[..pcm.length], &PCM[..4]);
    assert_eq!(state.mixer().frame_cursor(), 4);
    assert_eq!(retry.renders.load(Ordering::Relaxed), 0);
    assert_eq!(retry.render_telemetry.read().render, None);
}

#[test]
fn positive_write_commits_before_timing_clock_or_submitted_counter_refusal() {
    for failure in 0..3 {
        let (_producer, mut state, config) = rig(4);
        let shared = Shared::new();
        if failure == 2 { shared.submitted.store(u64::MAX, Ordering::Relaxed); }
        let mut pcm = Pcm::new(&shared, &[Write::Count(1)], usize::MAX);
        pcm.timing_fail = failure == 0;
        assert!(pump(&mut pcm, &mut state, &config,
            &Clock::new(if failure == 1 { 2 } else { 0 }), &mut Encoder { fail: false }).is_err());
        assert_eq!(&pcm.output[..pcm.length], &PCM[..1]);
        assert_eq!(state.admitted_frames(), 1);
        assert_eq!(state.pending_samples(), &PCM[1..4]);
        assert_eq!(state.output_frame_basis().start_physical_frame(), 1);
        assert_eq!(state.mixer().frame_cursor(), 4);
    }
}

#[test]
fn render_start_clock_and_rendered_counter_refusals_never_discard_generated_pcm() {
    for clock_failure in [true, false] {
        let (_producer, mut state, config) = rig(4);
        let shared = Shared::new();
        if !clock_failure { shared.rendered.store(u64::MAX, Ordering::Relaxed); }
        let mut pcm = Pcm::new(&shared, &[], 1);
        assert!(pump(&mut pcm, &mut state, &config,
            &Clock::new(if clock_failure { 1 } else { 0 }), &mut Encoder { fail: false }).is_err());
        assert_eq!(pcm.writes, 0);
        if clock_failure { assert_eq!(state.mixer().frame_cursor(), 0); assert_eq!(state.pending_frames(), 0); }
        else { assert_eq!(state.pending_samples(), &PCM[..4]); assert_eq!(state.mixer().frame_cursor(), 4); }
    }
}

#[test]
fn stop_mid_tail_and_drop_refusal_preserve_same_suffix_and_first_frame_basis() {
    for drop_fail in [false, true] {
        let (_producer, mut state, config) = rig(4);
        let shared = Shared::new();
        let mut pcm = Pcm::new(&shared, &[Write::Count(1)], 1);
        pcm.drop_fail = drop_fail;
        let result = pump(&mut pcm, &mut state, &config, &Clock::new(0), &mut Encoder { fail: false });
        assert_eq!(result.is_err(), drop_fail);
        assert_eq!(pcm.drops, 1);
        assert_eq!(state.pending_samples(), &PCM[1..4]);
        assert_eq!(state.output_frame_basis().start_physical_frame(), 1);
        assert_eq!(state.mixer().frame_cursor(), 4);
    }
}

#[test]
fn stop_before_pump_never_renders_or_consumes_existing_pending_output() {
    let (_producer, mut state, config) = rig(4);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let shared = Shared::new();
    shared.stop.store(true, Ordering::Release);
    let mut pcm = Pcm::new(&shared, &[], 1);
    pump(&mut pcm, &mut state, &config, &Clock::new(0), &mut Encoder { fail: false }).unwrap();
    assert_eq!(pcm.writes, 0);
    assert_eq!(pcm.drops, 1);
    assert_eq!(state.pending_samples(), &PCM[1..4]);
    assert_eq!(shared.render_telemetry.read().render, None);
}

#[test]
fn reopen_smaller_or_larger_period_admits_suffix_before_fresh_distinct_pcm() {
    for period in [2, 5] {
        let (_producer, mut state, mut config) = rig(4);
        let shared = Shared::new();
        let mut first = Pcm::new(&shared, &[Write::Count(1)], 1);
        pump(&mut first, &mut state, &config, &Clock::new(0), &mut Encoder { fail: false }).unwrap();
        assert_eq!(&first.output[..first.length], &PCM[..1]);
        state.reconfigure(config.format, None, period).unwrap();
        config.period_frames = period as u32;
        config.requested.period_frames = period as u32;
        assert_eq!(state.output_frame_basis().start_physical_frame(), 1);
        let retry = Shared::new();
        let tail_writes = 3usize.div_ceil(period);
        let mut reopened = Pcm::new(&retry, &[], tail_writes + 1);
        pump(&mut reopened, &mut state, &config, &Clock::new(0), &mut Encoder { fail: false }).unwrap();
        let expected: Vec<f32> = PCM[1..4].iter().copied()
            .chain(PCM[4..].iter().copied()).chain(std::iter::repeat(0.0)).take(3 + period).collect();
        assert_eq!(&reopened.output[..reopened.length], expected);
        assert_eq!(state.mixer().frame_cursor(), (4 + period) as u64);
        assert_eq!(retry.submitted.load(Ordering::Relaxed), (3 + period) as u64);
        assert_eq!(retry.renders.load(Ordering::Relaxed), 1);
        assert_eq!(retry.render_telemetry.read().render.unwrap().start_frame, 4);
    }
}

#[test]
fn actual_render_and_pending_replay_pump_paths_perform_no_heap_operations() {
    for retained in [false, true] {
        let (_producer, mut state, config) = rig(4);
        if retained { state.render_pending(4).unwrap(); state.admit(1).unwrap(); }
        let shared = Shared::new();
        let mut pcm = Pcm::new(&shared, &[], 1);
        let clock = Clock::new(0);
        let mut encoder = Encoder { fail: false };
        let (result, calls) = crate::audio::count_heap_calls(||
            pump(&mut pcm, &mut state, &config, &clock, &mut encoder));
        result.unwrap();
        assert_eq!(calls, 0);
        assert_eq!(&pcm.output[..pcm.length], if retained { &PCM[1..4] } else { &PCM[..4] });
    }
}

#[test]
fn actual_worker_join_legacy_refusal_and_whole_take_keep_pending_tail_once() {
    let (_producer, state, config) = rig(4);
    let basis = state.output_frame_basis();
    let shared = Arc::new(Shared::new());
    let owned_shared = shared.clone();
    let worker_config = config.clone();
    let worker = launch_worker(NativeWorkerSpawner, state, move |mut state| {
        let mut pcm = Pcm::new(&owned_shared, &[Write::Count(1)], 1);
        pcm.drop_fail = true;
        let result = pump(&mut pcm, &mut state, &worker_config,
            &Clock::new(0), &mut Encoder { fail: false });
        assert_eq!(&pcm.output[..pcm.length], &PCM[..1]);
        (result, state)
    }).unwrap_or_else(|_| panic!("memory worker launch"));
    let mut stream = AlsaStream { configuration: config, basis, shared, worker: Some(worker),
        recovered_output: None, retired: false };
    assert!(stream.take_stopped_output().is_err());
    // Join before stop so the scripted shortwrite occurs independently of caller stop.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !stream.worker.as_ref().unwrap().is_finished() {
        assert!(std::time::Instant::now() < deadline, "scripted worker completion");
        thread::yield_now();
    }
    assert!(stream.stop().is_err());
    assert!(stream.take_stopped_mixer().is_err());
    assert_eq!(stream.recovered_output.as_ref().unwrap().pending_samples(), &PCM[1..4]);
    let state = stream.take_stopped_output().unwrap().unwrap();
    assert_eq!(state.pending_samples(), &PCM[1..4]);
    assert_eq!(state.output_frame_basis().start_physical_frame(), 1);
    assert!(stream.take_stopped_output().unwrap().is_none());
    assert!(stream.take_stopped_mixer().unwrap().is_none());
}

#[test]
fn incompatible_full_owner_open_preserves_tail_and_permits_exact_same_format_retry() {
    for changed_matrix in [false, true] {
        let (_producer, mut state, config) = rig(4);
        state.render_pending(4).unwrap();
        state.admit(1).unwrap();
        let mut request = config.requested.clone();
        let failure = if changed_matrix {
            AlsaStream::open_state_remixed_recoverable(request, state,
                ChannelMatrix::new(1, 1, &[0.5]).unwrap()).err().unwrap()
        } else {
            request.format = DeviceFormat::new(8, 1,
                SampleEncoding::Pcm { container_bits: 16, valid_bits: 16 }, None).unwrap();
            AlsaStream::open_state_recoverable(request, state).err().unwrap()
        };
        let (_, state) = failure.into_parts();
        let mut state = state.unwrap();
        assert_eq!(state.format(), Some(config.format));
        assert_eq!(state.pending_samples(), &PCM[1..4]);
        assert_eq!(state.output_frame_basis().start_physical_frame(), 1);
        assert_eq!(state.mixer().frame_cursor(), 4);
        state.reconfigure(config.format, None, 2).unwrap();
        let mut config = config;
        config.period_frames = 2;
        let shared = Shared::new();
        let mut pcm = Pcm::new(&shared, &[], 2);
        pump(&mut pcm, &mut state, &config, &Clock::new(0), &mut Encoder { fail: false }).unwrap();
        assert_eq!(&pcm.output[..pcm.length], &PCM[1..4]);
        assert_eq!(state.mixer().frame_cursor(), 4);
    }
}
