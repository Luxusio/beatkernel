//! Real production worker/control interleavings with injected native effects.
//! These are software ownership tests, never physical clock or acoustic proof.
use super::super::converted::{run_converted_worker, ConvertedAlsaStream};
use super::super::converted_fixtures::rig_boundaries;
use super::super::*;
use super::*;
use crate::audio::{count_heap_calls, ConvertedNativeOutputState};
use beatkernel::audio::*;
use std::{
    cell::Cell,
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

fn rig(
    held: bool,
) -> (
    CommandProducer,
    ConvertedNativeOutputState,
    AlsaAppliedConfig,
    converted_telemetry::ConvertedTelemetry,
    Shared,
) {
    let (producer, mut state, config) = rig_boundaries(8, 1000, 2000, None, None);
    state.enable_phase_history(16).unwrap();
    state.enable_source_playback_history(16).unwrap();
    let telemetry = converted_telemetry::ConvertedTelemetry::with_phase_history(
        &state,
        state.target_frame_basis(),
    )
    .unwrap();
    telemetry.set_held(held);
    let mut shared = Shared::with_converted_history(&telemetry);
    shared.converted_source = Some(ConvertedSourceFacts::from_state(&state));
    shared.status.store(1, Ordering::SeqCst);
    (producer, state, config, telemetry, shared)
}

struct Clock(Cell<usize>);
impl WorkerClock for Clock {
    fn now(&self) -> Result<ClockPoint, LinuxError> {
        let call = self.0.get() + 1;
        self.0.set(call);
        Ok(ClockPoint {
            domain: ClockDomainId(8),
            timestamp: Timestamp::from_nanos(call as i64),
        })
    }
}
#[derive(Default)]
struct Encoder {
    calls: usize,
    fail: bool,
}
impl PcmEncoder for Encoder {
    fn encode(
        &mut self,
        format: DeviceFormat,
        samples: &[f32],
        bytes: &mut [u8],
    ) -> Result<(), LinuxError> {
        self.calls += 1;
        if self.fail {
            Err(LinuxError::InvalidConfiguration(
                "conditional fixture encode",
            ))
        } else {
            encode_pcm(format, samples, bytes).map_err(LinuxError::Conversion)
        }
    }
}

struct Pcm<'a> {
    shared: &'a Shared,
    request: Option<ConvertedRetirementPolicy>,
    nonce: Option<u64>,
    cancel_before_worker: bool,
    cancel_on_drop: bool,
    cancel_result: Option<ConvertedRetirementStatus>,
    expected_second_status: Option<ConvertedRetirementStatus>,
    first_offer: [u8; 64],
    first_length: usize,
    admitted: [u8; 64],
    admitted_length: usize,
    writes: usize,
    stop_after: usize,
    fail_at_write: usize,
    drops: usize,
    drop_fail: bool,
}
impl<'a> Pcm<'a> {
    fn new(shared: &'a Shared, request: Option<ConvertedRetirementPolicy>) -> Self {
        Self {
            shared,
            request,
            nonce: None,
            cancel_before_worker: false,
            cancel_on_drop: false,
            cancel_result: None,
            expected_second_status: None,
            first_offer: [0; 64],
            first_length: 0,
            admitted: [0; 64],
            admitted_length: 0,
            writes: 0,
            stop_after: 2,
            fail_at_write: 0,
            drops: 0,
            drop_fail: false,
        }
    }
}
impl PcmOperations for Pcm<'_> {
    fn write(&mut self, bytes: &[u8], frames: usize) -> Result<Option<usize>, LinuxError> {
        self.writes += 1;
        if self.writes == 1 {
            self.first_offer[..bytes.len()].copy_from_slice(bytes);
            self.first_length = bytes.len();
            if let Some(policy) = self.request {
                let nonce = self.shared.conditional_retirement.submit(policy)?.unwrap();
                self.nonce = Some(nonce);
                if self.cancel_before_worker {
                    self.cancel_result = Some(self.shared.conditional_retirement.cancel(nonce)?);
                }
            }
        } else {
            // Refusal conserves bytes; ordinary native admission can still advance them.
            assert_eq!(bytes, &self.first_offer[8..self.first_length]);
            assert!(!self.shared.stop.load(Ordering::Acquire));
            if let Some(expected) = self.expected_second_status {
                assert_eq!(
                    self.shared
                        .conditional_retirement
                        .poll(self.nonce.unwrap())?,
                    expected
                );
            }
        }
        if self.writes == self.fail_at_write {
            return Err(LinuxError::Alsa {
                operation: "conditional fixture write",
                code: -32,
            });
        }
        let count = if self.writes == 1 { 2 } else { 1 };
        assert!(count <= frames);
        let end = self.admitted_length + count * 4;
        self.admitted[self.admitted_length..end].copy_from_slice(&bytes[..count * 4]);
        self.admitted_length = end;
        if self.writes == self.stop_after {
            self.shared.stop.store(true, Ordering::Release);
        }
        Ok(Some(count))
    }
    fn wait(&mut self) -> Result<bool, LinuxError> {
        panic!("script only positively admits or fails")
    }
    fn timing<C: WorkerClock>(
        &mut self,
        _clock: &C,
        _submitted: u64,
    ) -> Result<Option<AlsaTimingSnapshot>, LinuxError> {
        Ok(None)
    }
    fn drop_stream(&mut self) -> Result<(), LinuxError> {
        self.drops += 1;
        if self.cancel_on_drop {
            self.cancel_result = Some(
                self.shared
                    .conditional_retirement
                    .cancel(self.nonce.unwrap())?,
            );
        }
        if self.drop_fail {
            Err(LinuxError::Alsa {
                operation: "conditional fixture drop",
                code: -5,
            })
        } else {
            Ok(())
        }
    }
}
fn pump(
    pcm: &mut Pcm<'_>,
    state: &mut ConvertedNativeOutputState,
    config: &AlsaAppliedConfig,
    telemetry: &converted_telemetry::ConvertedTelemetry,
    clock: &Clock,
    encoder: &mut Encoder,
) -> Result<(), LinuxError> {
    let mut bytes = [0; 64];
    let shared = pcm.shared;
    run_converted_worker(
        pcm,
        state,
        config,
        &mut bytes[..config.period_frames as usize * 4],
        shared,
        clock,
        encoder,
        telemetry,
    )
}
fn encoded(samples: &[f32]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect()
}

#[test]
fn empty_snapshot_then_actual_partial_held_generation_refuses_require_empty_and_stays_live() {
    let (_producer, mut state, config, telemetry, shared) = rig(true);
    assert_eq!(state.pending_frames(), 0);
    let source = state.converter_owner().source_position();
    let mut pcm = Pcm::new(&shared, Some(ConvertedRetirementPolicy::RequireEmptySuffix));
    pcm.expected_second_status = Some(ConvertedRetirementStatus::Refused);
    let mut encoder = Encoder::default();
    let (result, heap) = count_heap_calls(|| {
        pump(
            &mut pcm,
            &mut state,
            &config,
            &telemetry,
            &Clock(Cell::new(0)),
            &mut encoder,
        )
    });
    result.unwrap();
    assert_eq!(heap, 0);
    assert_eq!((pcm.writes, pcm.drops, encoder.calls), (2, 1, 2));
    assert_eq!(shared.renders.load(Ordering::Relaxed), 1);
    assert_eq!(shared.submitted.load(Ordering::SeqCst), 3);
    assert_eq!(state.pending_frames(), 5);
    assert!(state.pending_is_held());
    assert_eq!(
        encoded(state.pending_samples()),
        pcm.first_offer[12..pcm.first_length]
    );
    assert_eq!(state.converter_owner().source_position(), source);
    assert_eq!(state.format(), config.format);
    assert_eq!(
        state.pending_report().unwrap().target_end_time,
        TargetTime::from_frames(8, 2000).unwrap()
    );
}

#[test]
fn compatible_partial_held_suffix_authorizes_before_any_later_render_encode_or_write() {
    let (_producer, mut state, config, telemetry, shared) = rig(true);
    let source = state.converter_owner().source_position();
    let mut pcm = Pcm::new(&shared, Some(ConvertedRetirementPolicy::PreserveHeldSuffix));
    let mut encoder = Encoder::default();
    let (result, heap) = count_heap_calls(|| {
        pump(
            &mut pcm,
            &mut state,
            &config,
            &telemetry,
            &Clock(Cell::new(0)),
            &mut encoder,
        )
    });
    result.unwrap();
    assert_eq!(heap, 0);
    assert_eq!((pcm.writes, pcm.drops, encoder.calls), (1, 1, 1));
    assert_eq!(shared.renders.load(Ordering::Relaxed), 1);
    assert_eq!(shared.submitted.load(Ordering::SeqCst), 2);
    assert!(shared.stop.load(Ordering::Acquire));
    assert_eq!(
        shared
            .conditional_retirement
            .poll(pcm.nonce.unwrap())
            .unwrap(),
        ConvertedRetirementStatus::Authorized
    );
    assert_eq!(state.pending_frames(), 6);
    assert_eq!(
        encoded(state.pending_samples()),
        pcm.first_offer[8..pcm.first_length]
    );
    assert_eq!(state.converter_owner().source_position(), source);
    assert_eq!(
        state.admitted_target_time(),
        Some(TargetTime::from_frames(2, 2000).unwrap())
    );
}

#[test]
fn active_partial_suffix_refuses_preserve_even_with_an_identical_format() {
    let (_producer, mut state, config, telemetry, shared) = rig(false);
    let mut pcm = Pcm::new(&shared, Some(ConvertedRetirementPolicy::PreserveHeldSuffix));
    pcm.expected_second_status = Some(ConvertedRetirementStatus::Refused);
    let mut encoder = Encoder::default();
    let (result, heap) = count_heap_calls(|| {
        pump(
            &mut pcm,
            &mut state,
            &config,
            &telemetry,
            &Clock(Cell::new(0)),
            &mut encoder,
        )
    });
    result.unwrap();
    assert_eq!(heap, 0);
    assert_eq!(pcm.writes, 2);
    assert!(!state.pending_is_held());
    assert_eq!(state.pending_frames(), 5);
    assert_eq!(
        encoded(state.pending_samples()),
        pcm.first_offer[12..pcm.first_length]
    );
    assert_eq!(
        state.pending_report().unwrap().state,
        ConvertedOutputState::Active
    );
    assert_eq!(shared.renders.load(Ordering::Relaxed), 1);
    assert_eq!(shared.submitted.load(Ordering::SeqCst), 3);
}

#[test]
fn require_empty_checks_actual_empty_state_before_the_first_callback() {
    let (_producer, mut state, config, telemetry, shared) = rig(true);
    let nonce = shared
        .conditional_retirement
        .submit(ConvertedRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    let source = state.converter_owner().source_position();
    let basis = state.target_frame_basis();
    let mut pcm = Pcm::new(&shared, None);
    let mut encoder = Encoder::default();
    let clock = Clock(Cell::new(0));
    let (result, heap) = count_heap_calls(|| {
        pump(
            &mut pcm,
            &mut state,
            &config,
            &telemetry,
            &clock,
            &mut encoder,
        )
    });
    result.unwrap();
    assert_eq!(heap, 0);
    assert_eq!(
        (pcm.writes, pcm.drops, encoder.calls, clock.0.get()),
        (0, 1, 0, 0)
    );
    assert_eq!(state.pending_frames(), 0);
    assert_eq!(state.target_frame_basis(), basis);
    assert_eq!(state.converter_owner().source_position(), source);
    assert_eq!(
        shared.conditional_retirement.poll(nonce).unwrap(),
        ConvertedRetirementStatus::Authorized
    );
    assert!(shared
        .conditional_retirement
        .submit(ConvertedRetirementPolicy::PreserveHeldSuffix)
        .unwrap()
        .is_none());
}

#[test]
fn timeout_cancel_winning_before_worker_keeps_the_same_pcm_stream() {
    let (_producer, mut state, config, telemetry, shared) = rig(true);
    let mut pcm = Pcm::new(&shared, Some(ConvertedRetirementPolicy::PreserveHeldSuffix));
    pcm.cancel_before_worker = true;
    pcm.expected_second_status = Some(ConvertedRetirementStatus::Cancelled);
    let mut encoder = Encoder::default();
    let (result, heap) = count_heap_calls(|| {
        pump(
            &mut pcm,
            &mut state,
            &config,
            &telemetry,
            &Clock(Cell::new(0)),
            &mut encoder,
        )
    });
    result.unwrap();
    assert_eq!(heap, 0);
    assert_eq!(
        pcm.cancel_result,
        Some(ConvertedRetirementStatus::Cancelled)
    );
    assert_eq!((pcm.writes, pcm.drops), (2, 1));
    assert_eq!(state.pending_frames(), 5);
    assert_eq!(shared.submitted.load(Ordering::SeqCst), 3);
}

#[test]
fn authorization_winning_before_cancel_cannot_restore_live_output_even_when_drop_fails() {
    let (_producer, mut state, config, telemetry, shared) = rig(true);
    let mut pcm = Pcm::new(&shared, Some(ConvertedRetirementPolicy::PreserveHeldSuffix));
    pcm.cancel_on_drop = true;
    pcm.drop_fail = true;
    let result = pump(
        &mut pcm,
        &mut state,
        &config,
        &telemetry,
        &Clock(Cell::new(0)),
        &mut Encoder::default(),
    );
    assert!(matches!(
        result,
        Err(LinuxError::Alsa {
            operation: "conditional fixture drop",
            code: -5
        })
    ));
    assert_eq!(
        pcm.cancel_result,
        Some(ConvertedRetirementStatus::Authorized)
    );
    assert_eq!((pcm.writes, pcm.drops), (1, 1));
    assert!(shared.stop.load(Ordering::Acquire));
    assert_eq!(state.pending_frames(), 6);
    assert_eq!(
        encoded(state.pending_samples()),
        pcm.first_offer[8..pcm.first_length]
    );
    assert_eq!(
        shared
            .conditional_retirement
            .poll(pcm.nonce.unwrap())
            .unwrap(),
        ConvertedRetirementStatus::Authorized
    );
}

#[test]
fn unconsumed_refusal_busy_nonce_and_stale_cancel_preserve_the_matching_slot() {
    let (_producer, mut state, config, telemetry, shared) = rig(true);
    let first = shared
        .conditional_retirement
        .submit(ConvertedRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    // A complete actual held block arose after an earlier empty snapshot.
    state.render_held_pending(8).unwrap();
    state.admit(2).unwrap();
    assert!(shared
        .conditional_retirement
        .submit(ConvertedRetirementPolicy::PreserveHeldSuffix)
        .unwrap()
        .is_none());
    let mut pcm = Pcm::new(&shared, None);
    pcm.stop_after = 1;
    pump(
        &mut pcm,
        &mut state,
        &config,
        &telemetry,
        &Clock(Cell::new(0)),
        &mut Encoder::default(),
    )
    .unwrap();
    assert_eq!(
        shared.conditional_retirement.status(first).unwrap(),
        ConvertedRetirementStatus::Refused
    );
    assert!(shared
        .conditional_retirement
        .submit(ConvertedRetirementPolicy::PreserveHeldSuffix)
        .unwrap()
        .is_none());
    assert_eq!(
        shared.conditional_retirement.cancel(first).unwrap(),
        ConvertedRetirementStatus::Refused
    );
    assert_eq!(
        shared.conditional_retirement.poll(first).unwrap(),
        ConvertedRetirementStatus::Refused
    );
    let second = shared
        .conditional_retirement
        .submit(ConvertedRetirementPolicy::PreserveHeldSuffix)
        .unwrap()
        .unwrap();
    assert_ne!(first, second);
    assert!(matches!(
        shared.conditional_retirement.poll(first),
        Err(LinuxError::InvalidLifecycle)
    ));
    assert!(matches!(
        shared.conditional_retirement.cancel(first),
        Err(LinuxError::InvalidLifecycle)
    ));
    assert_eq!(
        shared.conditional_retirement.status(second).unwrap(),
        ConvertedRetirementStatus::Pending
    );
    assert_eq!(
        shared.conditional_retirement.cancel(second).unwrap(),
        ConvertedRetirementStatus::Cancelled
    );
    assert!(shared
        .conditional_retirement
        .submit(ConvertedRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .is_none());
    assert_eq!(
        shared.conditional_retirement.poll(second).unwrap(),
        ConvertedRetirementStatus::Cancelled
    );
    assert!(matches!(
        shared.conditional_retirement.poll(second),
        Err(LinuxError::InvalidLifecycle)
    ));
}

#[test]
fn checked_nonce_exhaustion_never_wraps_or_overwrites_a_terminal_response() {
    let slot = ConditionalRetirement::new();
    slot.word
        .store((MAX_NONCE - 1) << NONCE_SHIFT, Ordering::SeqCst);
    let last = slot
        .submit(ConvertedRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    assert_eq!(last, MAX_NONCE);
    assert!(slot
        .submit(ConvertedRetirementPolicy::PreserveHeldSuffix)
        .unwrap()
        .is_none());
    assert_eq!(
        slot.cancel(last).unwrap(),
        ConvertedRetirementStatus::Cancelled
    );
    assert!(slot
        .submit(ConvertedRetirementPolicy::PreserveHeldSuffix)
        .unwrap()
        .is_none());
    assert_eq!(
        slot.poll(last).unwrap(),
        ConvertedRetirementStatus::Cancelled
    );
    let word = slot.word.load(Ordering::SeqCst);
    for _ in 0..3 {
        assert!(matches!(
            slot.submit(ConvertedRetirementPolicy::PreserveHeldSuffix),
            Err(LinuxError::Overflow)
        ));
        assert_eq!(slot.word.load(Ordering::SeqCst), word);
    }
    assert_eq!(word, MAX_NONCE << NONCE_SHIFT);
}

#[test]
fn policy_uses_exact_encoding_rate_channels_and_matrix_equality() {
    let (_, state, config, _, _) = rig(true);
    let matrix = state.converter_owner().converter().matrix();
    let same = ChannelMatrix::new(1, 1, &[1.]).unwrap();
    assert_eq!(
        ConvertedRetirementPolicy::for_interpretation(config.format, matrix, config.format, &same),
        ConvertedRetirementPolicy::PreserveHeldSuffix
    );
    let other = ChannelMatrix::new(1, 1, &[0.5]).unwrap();
    for format in [
        DeviceFormat::new(1000, 1, SampleEncoding::Float32, None).unwrap(),
        DeviceFormat::new(
            2000,
            1,
            SampleEncoding::Pcm {
                container_bits: 16,
                valid_bits: 16,
            },
            None,
        )
        .unwrap(),
    ] {
        assert_eq!(
            ConvertedRetirementPolicy::for_interpretation(config.format, matrix, format, &same),
            ConvertedRetirementPolicy::RequireEmptySuffix
        );
    }
    assert_eq!(
        ConvertedRetirementPolicy::for_interpretation(config.format, matrix, config.format, &other),
        ConvertedRetirementPolicy::RequireEmptySuffix
    );
    let stereo = DeviceFormat::new(2000, 2, SampleEncoding::Float32, None).unwrap();
    let stereo_matrix = ChannelMatrix::new(1, 2, &[1., 1.]).unwrap();
    assert_eq!(
        ConvertedRetirementPolicy::for_interpretation(
            config.format,
            matrix,
            stereo,
            &stereo_matrix
        ),
        ConvertedRetirementPolicy::RequireEmptySuffix
    );
}

#[test]
fn cold_source_facts_validate_request_bounds_matrix_domain_and_actual_capacity() {
    let (_, state, config, _, shared) = rig(true);
    let facts = ConvertedSourceFacts::from_state(&state);
    let matrix = state.converter_owner().converter().matrix();
    facts.validate(&config.requested, matrix).unwrap();
    let before = shared.conditional_retirement.word.load(Ordering::SeqCst);
    for case in 0..7 {
        let mut request = config.requested.clone();
        match case {
            0 => request.device.clear(),
            1 => request.device = "bad\0device".into(),
            2 => request.period_frames = 0,
            3 => request.buffer_frames = request.period_frames,
            4 => request.monotonic_domain = state.mixer().config().domain(),
            5 => {
                request.format =
                    DeviceFormat::new(2000, 1, SampleEncoding::Float32, Some(1)).unwrap()
            }
            6 => {
                request.format = DeviceFormat::new(
                    2000,
                    1,
                    SampleEncoding::Pcm {
                        container_bits: 24,
                        valid_bits: 20,
                    },
                    None,
                )
                .unwrap()
            }
            _ => unreachable!(),
        }
        assert!(facts.validate(&request, matrix).is_err());
        assert_eq!(
            shared.conditional_retirement.word.load(Ordering::SeqCst),
            before
        );
    }
    let wrong_source = ChannelMatrix::new(2, 1, &[0.5, 0.5]).unwrap();
    let wrong_target = ChannelMatrix::new(1, 2, &[1., 1.]).unwrap();
    assert!(facts.validate(&config.requested, &wrong_source).is_err());
    assert!(facts.validate(&config.requested, &wrong_target).is_err());
    let mut oversized = config.requested.clone();
    oversized.period_frames = 1024;
    oversized.buffer_frames = 2048;
    assert!(matches!(
        facts.validate(&oversized, matrix),
        Err(LinuxError::Mixer(AudioError::RenderCapacity))
    ));
    // Encoding, rate and dimension changes are valid cold requests when capacity permits.
    let mut valid = config.requested.clone();
    valid.format = DeviceFormat::new(
        1500,
        2,
        SampleEncoding::Pcm {
            container_bits: 24,
            valid_bits: 24,
        },
        None,
    )
    .unwrap();
    facts.validate(&valid, &wrong_target).unwrap();
    assert_eq!(state.pending_frames(), 0);
    assert_eq!(state.mixer().frame_cursor(), 0);
    assert_eq!(
        shared.conditional_retirement.word.load(Ordering::SeqCst),
        before
    );
}

#[test]
fn original_source_worker_keeps_its_default_continue_without_a_converted_decision() {
    let (_, mut state, config) = super::super::continuity_fixtures::rig(4);
    let shared = Shared::new();
    let nonce = shared
        .conditional_retirement
        .submit(ConvertedRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    let mut pcm = Pcm::new(&shared, None);
    pcm.stop_after = 1;
    let mut encoder = Encoder::default();
    let mut bytes = [0; 16];
    let (result, heap) = count_heap_calls(|| {
        run_worker(
            &mut pcm,
            &mut state,
            &config,
            &mut bytes,
            &shared,
            &Clock(Cell::new(0)),
            &mut encoder,
        )
    });
    result.unwrap();
    assert_eq!(heap, 0);
    assert_eq!((pcm.writes, pcm.drops, encoder.calls), (1, 1, 1));
    assert_eq!(shared.submitted.load(Ordering::Relaxed), 2);
    assert_eq!(
        shared.conditional_retirement.status(nonce).unwrap(),
        ConvertedRetirementStatus::Pending
    );
    assert_eq!(state.pending_frames(), 2);
    assert_eq!(
        pcm.admitted[..8],
        encoded(&super::super::continuity_fixtures::PCM[..2])
    );
}

// Only native-effect fixtures block on channels. Production retirement remains
// the real run_converted_worker packed-atomic decision; no fixture state machine.
enum Event {
    Write(usize),
    Dropped,
}
enum Command {
    Admit(usize),
    Fail,
    Stop,
}
struct GatedPcm<'a> {
    shared: &'a Shared,
    events: Sender<Event>,
    commands: Receiver<Command>,
    drop_fail: bool,
}
impl PcmOperations for GatedPcm<'_> {
    fn write(&mut self, _bytes: &[u8], frames: usize) -> Result<Option<usize>, LinuxError> {
        self.events
            .send(Event::Write(frames))
            .map_err(|_| LinuxError::InvalidLifecycle)?;
        match self
            .commands
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| LinuxError::InvalidConfiguration("fixture gate timeout"))?
        {
            Command::Admit(n) => {
                assert!(n <= frames);
                Ok(Some(n))
            }
            Command::Fail => Err(LinuxError::Alsa {
                operation: "conditional gated write",
                code: -32,
            }),
            Command::Stop => {
                self.shared.stop.store(true, Ordering::Release);
                Ok(Some(1))
            }
        }
    }
    fn wait(&mut self) -> Result<bool, LinuxError> {
        panic!("gated PCM positively admits")
    }
    fn timing<C: WorkerClock>(
        &mut self,
        _clock: &C,
        _submitted: u64,
    ) -> Result<Option<AlsaTimingSnapshot>, LinuxError> {
        Ok(None)
    }
    fn drop_stream(&mut self) -> Result<(), LinuxError> {
        let _ = self.events.send(Event::Dropped);
        if self.drop_fail {
            Err(LinuxError::Alsa {
                operation: "conditional gated drop",
                code: -5,
            })
        } else {
            Ok(())
        }
    }
}
struct GatedStream {
    stream: ConvertedAlsaStream,
    events: Receiver<Event>,
    commands: Sender<Command>,
}
impl GatedStream {
    fn new(drop_fail: bool) -> Self {
        let (_producer, mut state, configuration, telemetry, shared) = rig(true);
        let basis = state.target_frame_basis();
        shared.start.store(true, Ordering::Release);
        let shared = Arc::new(shared);
        let telemetry = Arc::new(telemetry);
        let worker_shared = Arc::clone(&shared);
        let worker_telemetry = Arc::clone(&telemetry);
        let config = configuration.clone();
        let (event_send, events) = mpsc::channel();
        let (commands, command_receive) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut pcm = GatedPcm {
                shared: &worker_shared,
                events: event_send,
                commands: command_receive,
                drop_fail,
            };
            let mut bytes = [0; 64];
            let result = run_converted_worker(
                &mut pcm,
                &mut state,
                &config,
                &mut bytes[..32],
                &worker_shared,
                &Clock(Cell::new(0)),
                &mut Encoder::default(),
                &worker_telemetry,
            );
            worker_shared
                .status
                .store(if result.is_ok() { 2 } else { 3 }, Ordering::Release);
            (result, state)
        });
        let stream = ConvertedAlsaStream {
            configuration,
            basis,
            shared,
            telemetry,
            worker: Some(worker),
            recovered_output: None,
            retired: false,
        };
        let fixture = Self {
            stream,
            events,
            commands,
        };
        assert!(matches!(fixture.event(), Event::Write(8)));
        fixture
    }
    fn event(&self) -> Event {
        self.events
            .recv_timeout(Duration::from_secs(5))
            .expect("actual worker event")
    }
}
impl Drop for GatedStream {
    fn drop(&mut self) {
        self.stream.shared.stop.store(true, Ordering::Release);
        let _ = self.commands.send(Command::Stop);
        let _ = self.stream.stop();
    }
}

#[test]
fn native_preflight_refusal_leaves_the_actual_running_worker_and_request_slot_untouched() {
    let mut fixture = GatedStream::new(false);
    let matrix = ChannelMatrix::new(1, 1, &[1.]).unwrap();
    let original = fixture.stream.configuration().requested.clone();
    let before = fixture
        .stream
        .shared
        .conditional_retirement
        .word
        .load(Ordering::SeqCst);
    let mut unsupported = original.clone();
    unsupported.format = DeviceFormat::new(
        2000,
        1,
        SampleEncoding::Pcm {
            container_bits: 24,
            valid_bits: 20,
        },
        None,
    )
    .unwrap();
    assert!(fixture
        .stream
        .validate_conditional_replacement(&unsupported, &matrix)
        .is_err());
    let wrong = ChannelMatrix::new(2, 1, &[0.5, 0.5]).unwrap();
    assert!(fixture
        .stream
        .validate_conditional_replacement(&original, &wrong)
        .is_err());
    assert_eq!(
        fixture
            .stream
            .shared
            .conditional_retirement
            .word
            .load(Ordering::SeqCst),
        before
    );
    assert!(!fixture.stream.shared.stop.load(Ordering::Acquire));
    fixture
        .stream
        .validate_conditional_replacement(&original, &matrix)
        .unwrap();
    let policy = ConvertedRetirementPolicy::for_interpretation(
        fixture.stream.configuration().format,
        &matrix,
        original.format,
        &matrix,
    );
    let ticket = fixture
        .stream
        .submit_conditional_retirement(policy)
        .unwrap()
        .unwrap();
    fixture.commands.send(Command::Admit(2)).unwrap();
    assert!(matches!(fixture.event(), Event::Dropped));
    assert_eq!(
        fixture.stream.poll_conditional_retirement(&ticket).unwrap(),
        ConvertedRetirementStatus::Authorized
    );
    fixture.stream.stop().unwrap();
    assert_eq!(
        fixture
            .stream
            .take_stopped_output()
            .unwrap()
            .unwrap()
            .pending_frames(),
        6
    );
}

#[test]
fn public_tickets_bind_the_original_arc_and_isolate_same_nonce_other_streams_and_stale_tickets() {
    let mut a = GatedStream::new(false);
    let mut b = GatedStream::new(false);
    let first = a
        .stream
        .submit_conditional_retirement(ConvertedRetirementPolicy::PreserveHeldSuffix)
        .unwrap()
        .unwrap();
    let foreign = b
        .stream
        .submit_conditional_retirement(ConvertedRetirementPolicy::PreserveHeldSuffix)
        .unwrap()
        .unwrap();
    assert!(matches!(
        b.stream.poll_conditional_retirement(&first),
        Err(LinuxError::InvalidLifecycle)
    ));
    assert!(matches!(
        b.stream.cancel_conditional_retirement(&first),
        Err(LinuxError::InvalidLifecycle)
    ));
    assert!(matches!(
        a.stream.poll_conditional_retirement(&foreign),
        Err(LinuxError::InvalidLifecycle)
    ));
    assert!(matches!(
        a.stream.cancel_conditional_retirement(&foreign),
        Err(LinuxError::InvalidLifecycle)
    ));
    assert_eq!(
        a.stream.poll_conditional_retirement(&first).unwrap(),
        ConvertedRetirementStatus::Pending
    );
    assert_eq!(
        b.stream.poll_conditional_retirement(&foreign).unwrap(),
        ConvertedRetirementStatus::Pending
    );
    assert!(a
        .stream
        .submit_conditional_retirement(ConvertedRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .is_none());
    assert_eq!(
        a.stream.cancel_conditional_retirement(&first).unwrap(),
        ConvertedRetirementStatus::Cancelled
    );
    assert!(a
        .stream
        .submit_conditional_retirement(ConvertedRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .is_none());
    assert_eq!(
        a.stream.poll_conditional_retirement(&first).unwrap(),
        ConvertedRetirementStatus::Cancelled
    );
    let second = a
        .stream
        .submit_conditional_retirement(ConvertedRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    assert!(matches!(
        a.stream.cancel_conditional_retirement(&first),
        Err(LinuxError::InvalidLifecycle)
    ));
    assert!(matches!(
        a.stream.poll_conditional_retirement(&first),
        Err(LinuxError::InvalidLifecycle)
    ));
    a.commands.send(Command::Admit(2)).unwrap();
    assert!(matches!(a.event(), Event::Write(6)));
    assert!(!a.stream.shared.stop.load(Ordering::Acquire));
    assert_eq!(
        a.stream.poll_conditional_retirement(&second).unwrap(),
        ConvertedRetirementStatus::Refused
    );
    assert_eq!(
        b.stream.poll_conditional_retirement(&foreign).unwrap(),
        ConvertedRetirementStatus::Pending
    );
    drop(a);
    // The stale Weak keeps its old allocation identity alive after owner destruction.
    let mut replacement = GatedStream::new(false);
    assert!(matches!(
        replacement.stream.poll_conditional_retirement(&first),
        Err(LinuxError::InvalidLifecycle)
    ));
    assert!(matches!(
        replacement.stream.cancel_conditional_retirement(&first),
        Err(LinuxError::InvalidLifecycle)
    ));
    assert!(replacement
        .stream
        .submit_conditional_retirement(ConvertedRetirementPolicy::PreserveHeldSuffix)
        .unwrap()
        .is_some());
}

#[test]
fn public_authorization_survives_drop_error_and_recovers_the_unique_complete_owner_once() {
    let mut fixture = GatedStream::new(true);
    let ticket = fixture
        .stream
        .submit_conditional_retirement(ConvertedRetirementPolicy::PreserveHeldSuffix)
        .unwrap()
        .unwrap();
    fixture.commands.send(Command::Admit(2)).unwrap();
    assert!(matches!(fixture.event(), Event::Dropped));
    assert_eq!(
        fixture
            .stream
            .cancel_conditional_retirement(&ticket)
            .unwrap(),
        ConvertedRetirementStatus::Authorized
    );
    assert_eq!(
        fixture.stream.poll_conditional_retirement(&ticket).unwrap(),
        ConvertedRetirementStatus::Authorized
    );
    assert!(matches!(
        fixture.stream.stop(),
        Err(LinuxError::Alsa {
            operation: "conditional gated drop",
            code: -5
        })
    ));
    assert_eq!(
        fixture.stream.poll_conditional_retirement(&ticket).unwrap(),
        ConvertedRetirementStatus::Authorized
    );
    let state = fixture.stream.take_stopped_output().unwrap().unwrap();
    assert_eq!(state.pending_frames(), 6);
    assert!(state.pending_is_held());
    assert_eq!(state.pending_samples(), &[0.; 6]);
    assert_eq!(
        state.admitted_target_time(),
        Some(TargetTime::from_frames(2, 2000).unwrap())
    );
    assert_eq!(state.mixer().frame_cursor(), 0);
    assert_eq!(state.mixer().playback_frame_cursor(), 0);
    assert!(state.phase_history().is_some());
    assert!(state.converter_owner().source_playback_history().is_some());
    assert!(fixture.stream.take_stopped_output().unwrap().is_none());
    assert!(fixture
        .stream
        .submit_conditional_retirement(ConvertedRetirementPolicy::PreserveHeldSuffix)
        .is_err());
}

#[test]
fn worker_failure_is_not_live_refusal_and_stop_preserves_the_unsubmitted_owner() {
    let mut fixture = GatedStream::new(false);
    let ticket = fixture
        .stream
        .submit_conditional_retirement(ConvertedRetirementPolicy::PreserveHeldSuffix)
        .unwrap()
        .unwrap();
    fixture.commands.send(Command::Fail).unwrap();
    assert!(matches!(
        fixture.stream.stop(),
        Err(LinuxError::Alsa {
            operation: "conditional gated write",
            code: -32
        })
    ));
    assert!(fixture.stream.poll_conditional_retirement(&ticket).is_err());
    assert!(fixture
        .stream
        .cancel_conditional_retirement(&ticket)
        .is_err());
    let state = fixture.stream.take_stopped_output().unwrap().unwrap();
    assert_eq!(state.pending_frames(), 8);
    assert_eq!(state.admitted_frames(), 0);
    assert_eq!(state.admitted_target_time(), None);
    assert_eq!(
        state.pending_report().unwrap().state,
        ConvertedOutputState::Held
    );
    assert_eq!(state.mixer().frame_cursor(), 0);
    assert!(state.phase_history().is_some());
}

#[test]
fn actual_worker_and_caller_cancel_race_never_rescue_an_authorized_output() {
    for _ in 0..16 {
        let mut fixture = GatedStream::new(false);
        let ticket = fixture
            .stream
            .submit_conditional_retirement(ConvertedRetirementPolicy::PreserveHeldSuffix)
            .unwrap()
            .unwrap();
        fixture.commands.send(Command::Admit(2)).unwrap();
        match fixture
            .stream
            .cancel_conditional_retirement(&ticket)
            .unwrap()
        {
            ConvertedRetirementStatus::Cancelled => {
                assert!(matches!(fixture.event(), Event::Write(6)));
                assert!(!fixture.stream.shared.stop.load(Ordering::Acquire));
                assert_eq!(
                    fixture.stream.poll_conditional_retirement(&ticket).unwrap(),
                    ConvertedRetirementStatus::Cancelled
                );
                assert_eq!(fixture.stream.shared.submitted.load(Ordering::SeqCst), 2);
            }
            ConvertedRetirementStatus::Authorized => {
                assert!(matches!(fixture.event(), Event::Dropped));
                assert!(fixture.stream.shared.stop.load(Ordering::Acquire));
                assert_eq!(
                    fixture.stream.poll_conditional_retirement(&ticket).unwrap(),
                    ConvertedRetirementStatus::Authorized
                );
                fixture.stream.stop().unwrap();
                let state = fixture.stream.take_stopped_output().unwrap().unwrap();
                assert_eq!(state.pending_frames(), 6);
                assert_eq!(
                    state.admitted_target_time(),
                    Some(TargetTime::from_frames(2, 2000).unwrap())
                );
            }
            other => panic!("held suffix cancel/authorize decision returned {other:?}"),
        }
    }
}

/// Opted real ALSA null verifies the production factory/worker lifecycle only.
/// Null supplies no physical playback, eligible audio clock or acoustic evidence.
#[test]
#[ignore = "requires explicit BEATKERNEL_TEST_ALSA_DEVICE=null; native software lifecycle only"]
fn opted_native_null_factory_conditional_retirement_recovers_the_complete_owner() {
    let device = std::env::var("BEATKERNEL_TEST_ALSA_DEVICE")
        .expect("set BEATKERNEL_TEST_ALSA_DEVICE=null explicitly");
    assert_eq!(
        device, "null",
        "this diagnostic only permits the ALSA null PCM"
    );
    let (_producer, state, config, _telemetry, _shared) = rig(true);
    let source_config = state.mixer().config();
    let source_identity = (
        source_config.format(),
        source_config.domain(),
        source_config.origin(),
        state.mixer().frame_cursor(),
        state.mixer().playback_frame_cursor(),
    );
    let original_basis = state.target_frame_basis();
    let original_quality = state.converter_owner().converter().quality();
    let matrix = state.converter_owner().converter().matrix().clone();
    let mut request = config.requested;
    request.device = device;
    let mut stream = ConvertedAlsaStream::open_recoverable(request, state, matrix.clone())
        .unwrap_or_else(|failure| panic!("actual null factory failed: {:?}", failure.error()));
    // Stream owns its JoinHandle throughout. Its existing Drop stops/unparks and
    // joins the worker on every assertion/deadline failure, including before start.
    assert!(
        stream.shared.converted_source.is_some(),
        "production factory must cache source facts"
    );
    let applied = stream.configuration().clone();
    stream
        .validate_conditional_replacement(&applied.requested, &matrix)
        .unwrap();
    stream.set_held(true).unwrap();
    stream.start().unwrap();
    let running_deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let status = stream.shared.status.load(Ordering::Acquire);
        assert!(
            status <= 1,
            "actual null worker terminated before request: status={status}"
        );
        if status == 1 && stream.shared.submitted.load(Ordering::SeqCst) != 0 {
            break;
        }
        assert!(
            std::time::Instant::now() < running_deadline,
            "actual null worker did not admit held PCM before deadline"
        );
        thread::yield_now();
    }
    let mut unsupported = applied.requested.clone();
    unsupported.format = DeviceFormat::new(
        applied.format.sample_rate(),
        applied.format.channels(),
        SampleEncoding::Pcm {
            container_bits: 24,
            valid_bits: 20,
        },
        None,
    )
    .unwrap();
    assert!(stream
        .validate_conditional_replacement(&unsupported, &matrix)
        .is_err());
    assert_eq!(stream.shared.status.load(Ordering::Acquire), 1);
    assert!(!stream.shared.stop.load(Ordering::Acquire));
    assert_eq!(
        stream
            .shared
            .conditional_retirement
            .word
            .load(Ordering::SeqCst),
        0
    );
    let policy = ConvertedRetirementPolicy::for_interpretation(
        applied.format,
        &matrix,
        applied.format,
        &matrix,
    );
    assert_eq!(policy, ConvertedRetirementPolicy::PreserveHeldSuffix);
    let ticket = stream
        .submit_conditional_retirement(policy)
        .unwrap()
        .unwrap();
    let response_deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        match stream.poll_conditional_retirement(&ticket).unwrap() {
            ConvertedRetirementStatus::Pending => {
                assert!(
                    std::time::Instant::now() < response_deadline,
                    "actual null conditional retirement deadline"
                );
                thread::yield_now();
            }
            ConvertedRetirementStatus::Authorized => break,
            other => panic!("actual held null worker unexpectedly returned {other:?}"),
        }
    }
    stream.stop().unwrap();
    assert!(stream.worker.is_none());
    assert!(stream.retired);
    assert_eq!(
        stream.poll_conditional_retirement(&ticket).unwrap(),
        ConvertedRetirementStatus::Authorized
    );
    let recovered = stream.take_stopped_output().unwrap().unwrap();
    assert!(stream.take_stopped_output().unwrap().is_none());
    assert_eq!(
        (
            recovered.mixer().config().format(),
            recovered.mixer().config().domain(),
            recovered.mixer().config().origin(),
            recovered.mixer().frame_cursor(),
            recovered.mixer().playback_frame_cursor()
        ),
        source_identity
    );
    assert_eq!(recovered.format(), applied.format);
    assert_eq!(
        recovered.converter_owner().converter().quality(),
        original_quality
    );
    assert_eq!(recovered.converter_owner().converter().matrix(), &matrix);
    let rendered = stream.shared.rendered.load(Ordering::Relaxed);
    let submitted = stream.shared.submitted.load(Ordering::SeqCst);
    assert!(submitted > 0 && submitted <= rendered);
    assert_eq!(recovered.converter_owner().target_frame_cursor(), rendered);
    assert_eq!(
        recovered.converter_owner().target_time(),
        original_basis
            .start_time()
            .checked_add_frames(rendered, applied.format.sample_rate())
            .unwrap()
    );
    assert_eq!(
        recovered.target_frame_basis().start_time(),
        original_basis
            .start_time()
            .checked_add_frames(submitted, applied.format.sample_rate())
            .unwrap()
    );
    assert_eq!(recovered.pending_frames() as u64, rendered - submitted);
    assert!(recovered.pending_frames() == 0 || recovered.pending_is_held());
    let source = recovered
        .converter_owner()
        .source_playback_history()
        .unwrap();
    assert_eq!(
        (
            source.source_rate(),
            source.initial_physical_frame(),
            source.initial_playback_frame()
        ),
        (
            source_identity.0.sample_rate(),
            source_identity.3,
            source_identity.4
        )
    );
    assert_eq!(
        recovered.phase_history().unwrap().end_time(),
        recovered.converter_owner().target_time()
    );
}
