//! Executes the production callback with real Source ownership and bounded effects.
//! Portable software evidence does not establish HAL or acoustic acceptance.
use super::*;
use crate::audio::{
    channel_remix, count_heap_calls,
    source_history_retirement_mailbox::SourceHistoryRetirementRequest,
    source_output_retirement::{SourceRetirementPolicy, SourceRetirementStatus},
    DeviceFormat, SampleEncoding,
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use std::{
    cell::RefCell,
    sync::atomic::{AtomicU32, Ordering},
};

const PATTERN: [f32; 8] = [0.125, -0.25, 0.375, -0.5, 0.625, -0.75, 0.875, -1.];
const UNTOUCHED: f32 = 0.9375;

fn rig(
    channels: u16,
    practice: bool,
) -> (
    CommandProducer,
    NativeOutputState,
    Option<PracticeController>,
) {
    let source = AudioFormat::new(1000, 1).unwrap();
    let pcm: Vec<_> = PATTERN.into_iter().cycle().take(128).collect();
    let limits = PcmLimits::new(512, 512, 1).unwrap();
    let mut bank = SampleBank::new(source, limits).unwrap();
    bank.insert(SampleId(1), PcmSample::new(source, pcm, limits).unwrap())
        .unwrap();
    let program = practice.then(|| {
        PreparedPracticeProgram::new(
            &bank,
            vec![PracticeCue {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.,
            }],
            PracticeLimits::new(8, 1, 128, 8, 32).unwrap(),
        )
        .unwrap()
    });
    let (mut producer, consumer) = command_queue(16).unwrap();
    if !practice {
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.,
            })
            .unwrap();
    }
    let mut mixer = Mixer::new(
        MixerConfig::new(
            source,
            ClockDomainId(71),
            Timestamp::ZERO,
            AudioLimits::new(16, 4, 16, 32, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let controller = program.map(|program| {
        let (controller, endpoint) = practice_queue(&program).unwrap();
        mixer
            .install_practice(
                program,
                endpoint,
                PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(100_000_000), false)
                    .unwrap(),
            )
            .unwrap();
        controller
    });
    mixer.enable_source_playback_history(16).unwrap();
    let matrix = match channels {
        1 => None,
        2 => Some(ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()),
        3 => Some(ChannelMatrix::new(1, 3, &[1., -0.5, 0.25]).unwrap()),
        _ => unreachable!(),
    };
    let format = DeviceFormat::new(1000, channels, SampleEncoding::Float32, None).unwrap();
    let state = NativeOutputState::new(mixer, format, matrix, 8)
        .unwrap_or_else(|_| panic!("complete Source fixture must prepare"));
    (producer, state, controller)
}

#[derive(Debug, PartialEq)]
struct OwnerObservation {
    report: Option<RenderReport>,
    admitted: usize,
    samples: Vec<f32>,
    samples_address: usize,
    basis: OutputFrameBasis,
    counters: AudioCounters,
    physical: u64,
    playback: u64,
    history_address: usize,
    history_grid: (u32, u64, u64, u64, u64, u64, usize),
    history_spans: Vec<SourcePlaybackSpan>,
    quarantined: bool,
}

fn observe(state: &NativeOutputState) -> OwnerObservation {
    let history = state.mixer().source_playback_history().unwrap();
    OwnerObservation {
        report: state.pending_report(),
        admitted: state.admitted_frames(),
        samples: state.pending_samples().to_vec(),
        samples_address: state.pending_samples().as_ptr() as usize,
        basis: state.output_frame_basis(),
        counters: state.mixer().counters(),
        physical: state.mixer().frame_cursor(),
        playback: state.mixer().playback_frame_cursor(),
        history_address: history as *const SourcePlaybackHistory as usize,
        history_grid: (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.pulled_end_frame(),
            history.retained_start_frame(),
            history.retirement_revision(),
            history.capacity(),
        ),
        history_spans: (0..history.len())
            .map(|i| history.span(i).unwrap())
            .collect(),
        quarantined: state.admission_is_uncertain(),
    }
}

fn descriptor(samples: &mut [f32], channels: u32) -> CoreAudioBufferDescriptor {
    CoreAudioBufferDescriptor {
        channels,
        byte_size: u32::try_from(std::mem::size_of_val(samples)).unwrap(),
        address: samples.as_mut_ptr() as usize,
    }
}

#[test]
fn descriptor_validation_accepts_actual_variable_frames_and_disjoint_group_layouts() {
    for frames in 1..=8 {
        let mut mono = [UNTOUCHED; 8];
        let mut stereo = [UNTOUCHED; 16];
        let descriptors = [
            descriptor(&mut mono[..frames], 1),
            descriptor(&mut stereo[..frames * 2], 2),
        ];
        let geometry = validate_buffer_descriptors(&[1, 2], &descriptors, 3, 8).unwrap();
        assert_eq!((geometry.frames, geometry.channels), (frames, 3));
        assert_eq!(mono, [UNTOUCHED; 8]);
        assert_eq!(stereo, [UNTOUCHED; 16]);
    }
}

#[test]
fn descriptor_validation_rejects_malformed_native_metadata_without_writing() {
    let mut a = [UNTOUCHED; 16];
    let mut b = [UNTOUCHED; 16];
    let valid = [descriptor(&mut a[..4], 1), descriptor(&mut b[..8], 2)];
    let mut invalid = Vec::new();
    invalid.push((vec![], vec![], 0, 8));
    invalid.push((vec![1], vec![], 1, 8));
    invalid.push((vec![], valid.to_vec(), 3, 8));
    invalid.push((vec![1], valid.to_vec(), 3, 8));
    invalid.push((vec![1, 2], valid.to_vec(), 3, 0));
    invalid.push((vec![1, 2], valid.to_vec(), 3, 3));
    invalid.push((vec![1, 2], valid.to_vec(), 0, 8));
    invalid.push((vec![1, 2], valid.to_vec(), 33, 8));
    invalid.push((vec![1, 1], valid.to_vec(), 3, 8));
    for (index, channels, bytes, address) in [
        (0, 0, valid[0].byte_size, valid[0].address),
        (1, 33, valid[1].byte_size, valid[1].address),
        (1, 1, valid[1].byte_size, valid[1].address),
        (1, 2, 0, valid[1].address),
        (1, 2, 4, valid[1].address),
        (1, 2, 7, valid[1].address),
        (1, 2, 16, valid[1].address),
        (1, 2, valid[1].byte_size, 0),
        (1, 2, valid[1].byte_size, valid[1].address + 1),
        (1, 2, valid[1].byte_size, usize::MAX - 3),
        (1, 2, valid[1].byte_size, valid[0].address),
        (1, 2, valid[1].byte_size, valid[0].address + 4),
    ] {
        let mut descriptors = valid.to_vec();
        descriptors[index] = CoreAudioBufferDescriptor {
            channels,
            byte_size: bytes,
            address,
        };
        invalid.push((vec![1, 2], descriptors, 3, 8));
    }
    for (layout, descriptors, channels, maximum) in invalid {
        assert!(validate_buffer_descriptors(&layout, &descriptors, channels, maximum).is_err());
        assert_eq!(a, [UNTOUCHED; 16]);
        assert_eq!(b, [UNTOUCHED; 16]);
    }
}

const BEGIN_ERROR: i32 = -101;
const CONFIGURATION_ERROR: i32 = -102;
const BUFFERS_ERROR: i32 = -103;
const RENDER_ERROR: i32 = -104;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    None,
    Begin,
    Configuration,
    Buffers,
    History,
    Render(usize),
    Generated(bool),
    Copy(usize),
    Delivered(u64, u32, u64),
    End,
}

struct Effects {
    values: [Effect; 64],
    len: usize,
}
impl Effects {
    fn push(&mut self, effect: Effect) {
        self.values[self.len] = effect;
        self.len += 1;
    }
    fn as_slice(&self) -> &[Effect] {
        &self.values[..self.len]
    }
}

#[derive(Clone, Copy)]
enum Corruption {
    None,
    Empty,
    LayoutChannels,
    ZeroChannels,
    ZeroBytes,
    PartialBytes,
    UnequalFrames,
    Null,
    Misaligned,
    Overflow,
    Overlap,
    SourceAlias(usize),
}

struct Ops {
    effects: RefCell<Effects>,
    frames: usize,
    layout: [u32; 3],
    count: usize,
    descriptors: [CoreAudioBufferDescriptor; 3],
    destination: [[f32; 32]; 3],
    corruption: Corruption,
    history: Option<SourceHistoryRetirementRequest>,
    begin_error: bool,
    configuration_error: bool,
    buffers_error: bool,
    render_error: bool,
    revision: u64,
    failures: usize,
    cadence: usize,
    delivery: Option<(RenderReport, u64, u32, u64)>,
}

impl Ops {
    fn new(frames: usize, layout: &[u32]) -> Self {
        let mut groups = [0; 3];
        groups[..layout.len()].copy_from_slice(layout);
        Self {
            effects: RefCell::new(Effects {
                values: [Effect::None; 64],
                len: 0,
            }),
            frames,
            layout: groups,
            count: layout.len(),
            descriptors: [CoreAudioBufferDescriptor {
                channels: 0,
                byte_size: 0,
                address: 0,
            }; 3],
            destination: [[UNTOUCHED; 32]; 3],
            corruption: Corruption::None,
            history: None,
            begin_error: false,
            configuration_error: false,
            buffers_error: false,
            render_error: false,
            revision: 40,
            failures: 7,
            cadence: 0,
            delivery: None,
        }
    }
    fn effect(&self, effect: Effect) {
        self.effects.borrow_mut().push(effect);
    }
    fn prepare_descriptors(&mut self) {
        for index in 0..self.count {
            self.descriptors[index] = CoreAudioBufferDescriptor {
                channels: self.layout[index],
                byte_size: (self.frames * self.layout[index] as usize * 4) as u32,
                address: self.destination[index].as_mut_ptr() as usize,
            };
        }
        if self.count == 0 {
            return;
        }
        let last = self.count - 1;
        match self.corruption {
            Corruption::None | Corruption::Empty => {}
            Corruption::LayoutChannels => self.descriptors[last].channels += 1,
            Corruption::ZeroChannels => self.descriptors[last].channels = 0,
            Corruption::ZeroBytes => self.descriptors[last].byte_size = 0,
            Corruption::PartialBytes => self.descriptors[last].byte_size -= 1,
            Corruption::UnequalFrames => self.descriptors[last].byte_size += self.layout[last] * 4,
            Corruption::Null => self.descriptors[last].address = 0,
            Corruption::Misaligned => self.descriptors[last].address += 1,
            Corruption::Overflow => self.descriptors[last].address = usize::MAX - 3,
            Corruption::Overlap => self.descriptors[last].address = self.descriptors[0].address + 4,
            Corruption::SourceAlias(address) => self.descriptors[last].address = address,
        }
    }
    fn untouched(&self) {
        assert_eq!(self.destination, [[UNTOUCHED; 32]; 3]);
    }
}

impl CoreAudioSourceOps for Ops {
    fn begin_publication(&mut self) -> Result<(), i32> {
        self.effect(Effect::Begin);
        if self.begin_error {
            self.failures += 1;
            Err(BEGIN_ERROR)
        } else {
            self.revision += 1;
            Ok(())
        }
    }
    fn end_publication(&mut self) {
        self.effect(Effect::End);
        self.revision += 1;
    }
    fn check_configuration(&mut self) -> Result<(), i32> {
        self.effect(Effect::Configuration);
        if self.configuration_error {
            Err(CONFIGURATION_ERROR)
        } else {
            Ok(())
        }
    }
    fn buffers(&mut self) -> Result<(&[u32], &[CoreAudioBufferDescriptor]), i32> {
        self.effect(Effect::Buffers);
        if self.buffers_error {
            return Err(BUFFERS_ERROR);
        }
        self.prepare_descriptors();
        let count = if matches!(self.corruption, Corruption::Empty) {
            0
        } else {
            self.count
        };
        Ok((&self.layout[..self.count], &self.descriptors[..count]))
    }
    fn history_request(&self) -> Option<SourceHistoryRetirementRequest> {
        self.effect(Effect::History);
        self.history
    }
    fn render_pending(
        &mut self,
        state: &mut NativeOutputState,
        frames: usize,
    ) -> Result<RenderReport, i32> {
        self.effect(Effect::Render(frames));
        if self.render_error {
            Err(RENDER_ERROR)
        } else {
            let report = state.render_pending(frames).map_err(|_| RENDER_ERROR)?;
            self.cadence += 1;
            Ok(report)
        }
    }
    fn publish_generated(&mut self, _: &NativeOutputState, report: Option<RenderReport>) {
        self.effect(Effect::Generated(report.is_some()));
    }
    fn copy_channel_group(
        &mut self,
        buffer: CoreAudioBufferDescriptor,
        samples: &[f32],
        channels: usize,
        first_channel: usize,
    ) {
        let index = (0..self.count)
            .find(|index| self.destination[*index].as_ptr() as usize == buffer.address)
            .expect("validated descriptor names a fixture destination");
        self.effect(Effect::Copy(index));
        let size = buffer.byte_size as usize / 4;
        channel_remix::copy_channel_group(
            samples,
            channels,
            first_channel,
            buffer.channels as usize,
            &mut self.destination[index][..size],
        )
        .expect("whole-layout validation makes every group copy infallible");
    }
    fn delivered(&mut self, report: RenderReport, first_frame: u64, frames: u32, end_frame: u64) {
        self.effect(Effect::Delivered(first_frame, frames, end_frame));
        self.delivery = Some((report, first_frame, frames, end_frame));
    }
}

fn step(state: &mut NativeOutputState, ops: &mut Ops) -> CoreAudioSourceStep {
    source_callback_step(
        state,
        &SourceRetirementSlot::new(),
        &AtomicU32::new(0),
        ops,
        &mut 0,
    )
}
fn failure(result: CoreAudioSourceStep, phase: CoreAudioSourcePhase) -> CoreAudioSourceFailure {
    let CoreAudioSourceStep::Failure(error) = result else {
        panic!("expected {phase:?} failure, got {result:?}")
    };
    assert_eq!(error.phase, phase);
    error
}
fn delivered(result: CoreAudioSourceStep, first: u64, frames: u32, end: u64) -> RenderReport {
    let CoreAudioSourceStep::Delivered {
        report,
        first_frame,
        frames: actual,
        end_frame,
    } = result
    else {
        panic!("expected native delivery, got {result:?}");
    };
    assert_eq!((first_frame, actual, end_frame), (first, frames, end));
    report
}

#[test]
fn retirement_and_sticky_authorization_skip_every_op_revision_failure_and_source_effect() {
    for policy in [
        SourceRetirementPolicy::PreservePausedSuffix,
        SourceRetirementPolicy::RequireEmptySuffix,
    ] {
        for geometry in [0, 1, 4, 8] {
            let (_, mut state, _) = rig(3, false);
            let before = observe(&state);
            let slot = SourceRetirementSlot::new();
            let payload = AtomicU32::new(99);
            let nonce = slot
                .submit_prepared(policy, || payload.store(geometry, Ordering::Relaxed))
                .unwrap()
                .unwrap();
            let mut ops = Ops::new(4, &[1, 2]);
            ops.begin_error = true;
            ops.configuration_error = true;
            ops.buffers_error = true;
            ops.history = Some(SourceHistoryRetirementRequest {
                revision: 1,
                source_pin: u64::MAX,
            });
            for _ in 0..2 {
                let (result, calls) = count_heap_calls(|| {
                    source_callback_step(&mut state, &slot, &payload, &mut ops, &mut 0)
                });
                assert_eq!(result, CoreAudioSourceStep::Retire);
                assert_eq!(calls, 0);
                assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Authorized));
                assert!(ops.effects.borrow().as_slice().is_empty());
                assert_eq!(
                    (ops.revision, ops.failures, ops.cadence, ops.delivery),
                    (40, 7, 0, None)
                );
                ops.untouched();
                assert_eq!(observe(&state), before);
            }
        }
    }
}

#[test]
fn quarantined_owner_refuses_before_every_op_without_replaying_retained_pcm() {
    let (_, mut state, _) = rig(2, false);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    state.mark_admission_uncertain();
    let before = observe(&state);
    let mut ops = Ops::new(2, &[2]);
    let (result, calls) = count_heap_calls(|| step(&mut state, &mut ops));
    failure(result, CoreAudioSourcePhase::Quarantined);
    assert_eq!(calls, 0);
    assert!(ops.effects.borrow().as_slice().is_empty());
    assert_eq!((ops.revision, ops.failures), (40, 7));
    ops.untouched();
    assert_eq!(observe(&state), before);
}

#[test]
fn paused_zero_suffix_uses_acquired_fixed_geometry_and_requires_whole_native_buffers() {
    for requested in [0, 1, 2, 3, 4, 6, 7, 8] {
        let (mut producer, mut state, _) = rig(2, false);
        producer.request_pause(true);
        let report = state.render_pending(8).unwrap();
        state.admit(2).unwrap();
        assert!(report.paused && state.paused_tail_admissible());
        let before = observe(&state);
        let slot = SourceRetirementSlot::new();
        let payload = AtomicU32::new(99);
        let nonce = slot
            .submit_prepared(SourceRetirementPolicy::PreservePausedSuffix, || {
                payload.store(requested, Ordering::Relaxed)
            })
            .unwrap()
            .unwrap();
        let mut ops = Ops::new(1, &[2]);
        let result = source_callback_step(&mut state, &slot, &payload, &mut ops, &mut 0);
        if requested != 0 && 6 % requested == 0 {
            assert_eq!(result, CoreAudioSourceStep::Retire);
            assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Authorized));
            assert!(ops.effects.borrow().as_slice().is_empty());
            assert_eq!(observe(&state), before);
        } else {
            delivered(result, 2, 1, 3);
            assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Refused));
            assert_eq!(state.pending_report(), Some(report));
            assert_eq!(state.admitted_frames(), 3);
            assert_eq!(state.mixer().frame_cursor(), 8);
            assert_eq!(ops.cadence, 0);
        }
    }
}

#[test]
fn pending_prefix_delivery_preserves_original_report_pcm_and_first_unsent_coordinate() {
    let (_, mut state, _) = rig(3, false);
    let report = state.render_pending(8).unwrap();
    state.admit(1).unwrap();
    let history_address = state.mixer().source_playback_history().unwrap() as *const _ as usize;
    let mut ops = Ops::new(3, &[1, 2]);
    assert_eq!(delivered(step(&mut state, &mut ops), 1, 3, 4), report);
    assert_eq!(&ops.destination[0][..3], &[-0.25, 0.375, -0.5]);
    assert_eq!(
        &ops.destination[1][..6],
        &[0.125, -0.0625, -0.1875, 0.09375, 0.25, -0.125]
    );
    assert_eq!(
        ops.effects.borrow().as_slice(),
        &[
            Effect::Begin,
            Effect::Configuration,
            Effect::Buffers,
            Effect::History,
            Effect::Generated(true),
            Effect::Copy(0),
            Effect::Copy(1),
            Effect::Delivered(1, 3, 4),
            Effect::End,
        ]
    );
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.admitted_frames(), 4);
    assert_eq!(state.output_frame_basis().start_physical_frame(), 4);
    assert_eq!(state.mixer().frame_cursor(), 8);
    assert_eq!(state.mixer().playback_frame_cursor(), 8);
    assert_eq!(
        state.mixer().source_playback_history().unwrap() as *const _ as usize,
        history_address
    );
    assert_eq!(ops.cadence, 0);
    for (frames, first, end) in [(2, 4, 6), (2, 6, 8)] {
        let mut next = Ops::new(frames, &[3]);
        assert_eq!(
            delivered(step(&mut state, &mut next), first, frames as u32, end),
            report
        );
        assert_eq!(next.cadence, 0);
        assert_eq!(state.mixer().frame_cursor(), 8);
    }
    assert_eq!(state.pending_frames(), 0);
    assert_eq!(state.output_frame_basis().start_physical_frame(), 8);
}

#[test]
fn short_pending_suffix_refuses_before_history_render_copy_or_admission() {
    let (_, mut state, _) = rig(2, false);
    state.render_pending(4).unwrap();
    state.admit(2).unwrap();
    let before = observe(&state);
    let mut ops = Ops::new(3, &[2]);
    ops.history = Some(SourceHistoryRetirementRequest {
        revision: 2,
        source_pin: u64::MAX,
    });
    let (result, calls) = count_heap_calls(|| step(&mut state, &mut ops));
    failure(result, CoreAudioSourcePhase::Pending);
    assert_eq!(calls, 0);
    assert_eq!(
        ops.effects.borrow().as_slice(),
        &[
            Effect::Begin,
            Effect::Configuration,
            Effect::Buffers,
            Effect::End
        ]
    );
    assert_eq!(observe(&state), before);
    ops.untouched();
}

#[test]
fn empty_owner_renders_the_actual_variable_size_and_publishes_only_positive_native_extent() {
    for frames in 1..=8 {
        let (_, mut state, _) = rig(2, false);
        let mut ops = Ops::new(frames, &[2]);
        let (result, calls) = count_heap_calls(|| step(&mut state, &mut ops));
        let report = delivered(result, 0, frames as u32, frames as u64);
        assert_eq!(calls, 0);
        assert_eq!(
            (report.start_frame, report.frames, report.playback_frames),
            (0, frames, frames)
        );
        assert_eq!(state.mixer().frame_cursor(), frames as u64);
        assert_eq!(state.pending_frames(), 0);
        assert_eq!(ops.cadence, 1);
        assert_eq!(ops.revision, 42);
        assert_eq!(
            ops.delivery,
            Some((report, 0, frames as u32, frames as u64))
        );
        assert_eq!(
            ops.effects.borrow().as_slice(),
            &[
                Effect::Begin,
                Effect::Configuration,
                Effect::Buffers,
                Effect::History,
                Effect::Render(frames),
                Effect::Generated(true),
                Effect::Copy(0),
                Effect::Delivered(0, frames as u32, frames as u64),
                Effect::End,
            ]
        );
        for frame in 0..frames {
            assert_eq!(ops.destination[0][frame * 2], PATTERN[frame]);
            assert_eq!(ops.destination[0][frame * 2 + 1], PATTERN[frame] * -0.5);
        }
        assert_eq!(
            &ops.destination[0][frames * 2..],
            &[UNTOUCHED; 32][frames * 2..]
        );
    }
}

#[test]
fn mono_interleaved_and_multiple_groups_match_independent_nonzero_pcm_oracle() {
    for (channels, layout) in [
        (1, &[1][..]),
        (2, &[2][..]),
        (3, &[3][..]),
        (3, &[1, 2][..]),
        (3, &[1, 1, 1][..]),
    ] {
        let (_, mut state, _) = rig(channels, false);
        let mut ops = Ops::new(4, layout);
        delivered(step(&mut state, &mut ops), 0, 4, 4);
        let gains = [1., -0.5, 0.25];
        let mut first = 0;
        for (group, width) in layout.iter().copied().enumerate() {
            for frame in 0..4 {
                for channel in 0..width as usize {
                    assert_eq!(
                        ops.destination[group][frame * width as usize + channel],
                        PATTERN[frame] * gains[first + channel]
                    );
                }
            }
            assert_eq!(
                &ops.destination[group][4 * width as usize..],
                &[UNTOUCHED; 32][4 * width as usize..]
            );
            first += width as usize;
        }
        assert_eq!(first, channels as usize);
    }
}

#[test]
fn invalid_final_group_or_actual_size_preserves_all_destinations_and_source_state() {
    for corruption in [
        Corruption::Empty,
        Corruption::LayoutChannels,
        Corruption::ZeroChannels,
        Corruption::ZeroBytes,
        Corruption::PartialBytes,
        Corruption::UnequalFrames,
        Corruption::Null,
        Corruption::Misaligned,
        Corruption::Overflow,
        Corruption::Overlap,
    ] {
        let (_, mut state, _) = rig(3, false);
        let before = observe(&state);
        let mut ops = Ops::new(4, &[1, 2]);
        ops.corruption = corruption;
        failure(step(&mut state, &mut ops), CoreAudioSourcePhase::Layout);
        assert_eq!(
            ops.effects.borrow().as_slice(),
            &[
                Effect::Begin,
                Effect::Configuration,
                Effect::Buffers,
                Effect::End
            ]
        );
        assert_eq!(observe(&state), before);
        ops.untouched();
    }
    for frames in [0, 9] {
        let (_, mut state, _) = rig(3, false);
        let before = observe(&state);
        let mut ops = Ops::new(frames, &[1, 2]);
        failure(step(&mut state, &mut ops), CoreAudioSourcePhase::Layout);
        assert_eq!(observe(&state), before);
        ops.untouched();
    }
}

#[test]
fn native_destination_aliasing_pending_source_refuses_before_any_copy() {
    let (_, mut state, _) = rig(2, false);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let before = observe(&state);
    let mut ops = Ops::new(2, &[2]);
    ops.corruption = Corruption::SourceAlias(state.pending_samples().as_ptr() as usize);
    failure(step(&mut state, &mut ops), CoreAudioSourcePhase::Extent);
    assert_eq!(observe(&state), before);
    assert!(!ops
        .effects
        .borrow()
        .as_slice()
        .iter()
        .any(|effect| matches!(effect, Effect::Copy(_) | Effect::Delivered(..))));
    ops.untouched();
}

#[test]
fn destination_overlapping_only_the_future_unsent_tail_cannot_corrupt_retained_pcm() {
    let (_, mut state, _) = rig(2, false);
    state.render_pending(8).unwrap();
    let before = observe(&state);
    let mut ops = Ops::new(1, &[2]);
    ops.corruption = Corruption::SourceAlias(state.pending_samples().as_ptr() as usize + 4 * 2 * 4);
    failure(step(&mut state, &mut ops), CoreAudioSourcePhase::Extent);
    assert_eq!(observe(&state), before);
    assert!(!ops
        .effects
        .borrow()
        .as_slice()
        .iter()
        .any(|effect| matches!(effect, Effect::Copy(_) | Effect::Delivered(..))));
    ops.untouched();
}

#[test]
fn checked_native_interval_rejects_zero_outside_prefix_and_both_overflow_boundaries() {
    let (_, mut state, _) = rig(1, false);
    let mut report = state.render_pending(8).unwrap();
    report.start_frame = 10;
    assert_eq!(checked_native_interval(report, 2, 3), Ok((12, 15)));
    assert_eq!(checked_native_interval(report, 7, 1), Ok((17, 18)));
    for (admitted, frames) in [(0, 0), (8, 1), (9, 1), (usize::MAX, 1), (7, 2), (0, 9)] {
        assert!(checked_native_interval(report, admitted, frames).is_err());
    }
    report.start_frame = u64::MAX;
    assert!(
        checked_native_interval(report, 1, 1).is_err(),
        "first unsent frame must be checked"
    );
    assert!(
        checked_native_interval(report, 0, 1).is_err(),
        "positive native end must be checked"
    );
    report.start_frame = u64::MAX - 2;
    assert_eq!(
        checked_native_interval(report, 1, 1),
        Ok((u64::MAX - 1, u64::MAX))
    );
    assert!(checked_native_interval(report, 1, 2).is_err());
}

#[test]
fn pending_history_clamps_to_original_block_start_then_completes_after_full_admission() {
    let (mut producer, mut state, _) = rig(2, false);
    let report = state.render_pending(8).unwrap();
    state.admit(3).unwrap();
    let request = SourceHistoryRetirementRequest {
        revision: 2,
        source_pin: 8,
    };
    let slot = SourceRetirementSlot::new();
    let geometry = AtomicU32::new(0);
    let mut completed = 0;
    for (frames, first, end) in [(2, 3, 5), (3, 5, 8)] {
        let mut ops = Ops::new(frames, &[2]);
        ops.history = Some(request);
        assert_eq!(
            delivered(
                source_callback_step(&mut state, &slot, &geometry, &mut ops, &mut completed),
                first,
                frames as u32,
                end
            ),
            report
        );
        assert_eq!(
            completed, 0,
            "later admission cannot complete the earlier clamped history request"
        );
        let history = state.mixer().source_playback_history().unwrap();
        assert_eq!(history.retained_start_frame(), 0);
        assert_eq!(
            (
                history.source_rate(),
                history.initial_physical_frame(),
                history.initial_playback_frame()
            ),
            (1000, 0, 0)
        );
    }
    let mut ops = Ops::new(1, &[2]);
    ops.history = Some(request);
    delivered(
        source_callback_step(&mut state, &slot, &geometry, &mut ops, &mut completed),
        8,
        1,
        9,
    );
    assert_eq!(completed, 2);
    assert_eq!(&ops.destination[0][..2], &[0.125, -0.0625]);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(history.retained_start_frame(), 0);
    assert_eq!(history.pulled_end_frame(), 9);
    assert_eq!(history.len(), 1);
    assert_eq!(history.retirement_revision(), 0);
    assert_eq!(
        history.span(0),
        Some(SourcePlaybackSpan::new(0, None, 0, SourcePlaybackState::Running).unwrap()),
        "completion preserves the original descriptor covering the requested pin"
    );

    producer.request_pause(true);
    let mut pause = Ops::new(1, &[2]);
    let paused = delivered(
        source_callback_step(&mut state, &slot, &geometry, &mut pause, &mut completed),
        9,
        1,
        10,
    );
    assert!(paused.paused);
    assert_eq!(&pause.destination[0][..2], &[0., 0.]);
    assert_eq!(completed, 2);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(history.retained_start_frame(), 0);
    assert_eq!(history.len(), 2);
    assert_eq!(
        history.span(0),
        Some(SourcePlaybackSpan::new(0, Some(9), 0, SourcePlaybackState::Running).unwrap())
    );
    assert_eq!(
        history.span(1),
        Some(SourcePlaybackSpan::new(9, None, 9, SourcePlaybackState::Frozen).unwrap())
    );

    let mut compact = Ops::new(1, &[2]);
    compact.history = Some(SourceHistoryRetirementRequest {
        revision: 4,
        source_pin: 9,
    });
    delivered(
        source_callback_step(&mut state, &slot, &geometry, &mut compact, &mut completed),
        10,
        1,
        11,
    );
    assert_eq!(completed, 4);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(history.retained_start_frame(), 9);
    assert_eq!(history.len(), 1);
    assert_eq!(history.retirement_revision(), 1);
    assert_eq!(
        history.span(0),
        Some(SourcePlaybackSpan::new(9, None, 9, SourcePlaybackState::Frozen).unwrap())
    );
}

#[test]
fn history_validates_original_revision_and_range_before_pending_clamp_without_copy() {
    for (revision, source_pin) in [(0, 0), (1, 0), (2, 5)] {
        let (_, mut state, _) = rig(2, false);
        state.render_pending(4).unwrap();
        state.admit(1).unwrap();
        let before = observe(&state);
        let mut ops = Ops::new(1, &[2]);
        ops.history = Some(SourceHistoryRetirementRequest {
            revision,
            source_pin,
        });
        let mut completed = 0;
        failure(
            source_callback_step(
                &mut state,
                &SourceRetirementSlot::new(),
                &AtomicU32::new(0),
                &mut ops,
                &mut completed,
            ),
            CoreAudioSourcePhase::History,
        );
        assert_eq!(completed, 0);
        assert_eq!(observe(&state), before);
        assert_eq!(
            ops.effects.borrow().as_slice(),
            &[
                Effect::Begin,
                Effect::Configuration,
                Effect::Buffers,
                Effect::History,
                Effect::Generated(false),
                Effect::End,
            ]
        );
        ops.untouched();
    }
}

#[test]
fn stale_completed_history_request_does_not_revalidate_an_obsolete_pin() {
    let (_, mut state, _) = rig(1, false);
    state.render_pending(4).unwrap();
    let mut completed = 2;
    let mut ops = Ops::new(1, &[1]);
    ops.history = Some(SourceHistoryRetirementRequest {
        revision: 2,
        source_pin: u64::MAX,
    });
    delivered(
        source_callback_step(
            &mut state,
            &SourceRetirementSlot::new(),
            &AtomicU32::new(0),
            &mut ops,
            &mut completed,
        ),
        0,
        1,
        1,
    );
    assert_eq!(completed, 2);
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        0
    );
    assert_eq!(state.pending_frames(), 3);
}

#[test]
fn practice_receipt_and_pending_report_both_pin_original_history_until_consumed() {
    let (mut producer, mut state, controller) = rig(2, true);
    let mut controller = controller.unwrap();
    for paused in [false, true] {
        producer.request_pause(paused);
        let mut ops = Ops::new(2, &[2]);
        let first = state.output_frame_basis().start_physical_frame();
        delivered(step(&mut state, &mut ops), first, 2, first + 2);
    }
    producer.request_pause(false);
    let report = state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    assert_eq!(report.start_frame, 4);
    assert_eq!(state.mixer().practice_history_retention_pins().0, Some(0));
    let request = SourceHistoryRetirementRequest {
        revision: 2,
        source_pin: 8,
    };
    let slot = SourceRetirementSlot::new();
    let geometry = AtomicU32::new(0);
    let mut completed = 0;
    let mut ops = Ops::new(1, &[2]);
    ops.history = Some(request);
    delivered(
        source_callback_step(&mut state, &slot, &geometry, &mut ops, &mut completed),
        5,
        1,
        6,
    );
    assert_eq!(completed, 0);
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        0
    );
    assert_eq!(
        controller
            .try_pop_projected_receipt()
            .unwrap()
            .receipt
            .physical_frame,
        0
    );
    assert_eq!(state.mixer().practice_history_retention_pins().0, None);
    for first in [6, 7] {
        let mut ops = Ops::new(1, &[2]);
        ops.history = Some(request);
        delivered(
            source_callback_step(&mut state, &slot, &geometry, &mut ops, &mut completed),
            first,
            1,
            first + 1,
        );
        assert_eq!(completed, 0);
        assert_eq!(
            state
                .mixer()
                .source_playback_history()
                .unwrap()
                .retained_start_frame(),
            4
        );
    }
    let mut ops = Ops::new(1, &[2]);
    ops.history = Some(request);
    delivered(
        source_callback_step(&mut state, &slot, &geometry, &mut ops, &mut completed),
        8,
        1,
        9,
    );
    assert_eq!(completed, 2);
    assert_eq!(&ops.destination[0][..2], &[0.875, -0.4375]);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(history.retained_start_frame(), 4);
    assert_eq!(history.pulled_end_frame(), 9);
    assert_eq!(history.len(), 1);
    assert_eq!(history.retirement_revision(), 1);
    assert_eq!(
        history.span(0),
        Some(SourcePlaybackSpan::new(4, None, 2, SourcePlaybackState::Running).unwrap()),
        "released consumer pins still preserve the original covering descriptor"
    );

    producer.request_pause(true);
    let mut pause = Ops::new(1, &[2]);
    let paused = delivered(
        source_callback_step(&mut state, &slot, &geometry, &mut pause, &mut completed),
        9,
        1,
        10,
    );
    assert!(paused.paused);
    assert_eq!(&pause.destination[0][..2], &[0., 0.]);
    assert_eq!(completed, 2);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(history.retained_start_frame(), 4);
    assert_eq!(history.len(), 2);
    assert_eq!(
        history.span(0),
        Some(SourcePlaybackSpan::new(4, Some(9), 2, SourcePlaybackState::Running).unwrap())
    );
    assert_eq!(
        history.span(1),
        Some(SourcePlaybackSpan::new(9, None, 7, SourcePlaybackState::Frozen).unwrap())
    );

    let mut compact = Ops::new(1, &[2]);
    compact.history = Some(SourceHistoryRetirementRequest {
        revision: 4,
        source_pin: 9,
    });
    delivered(
        source_callback_step(&mut state, &slot, &geometry, &mut compact, &mut completed),
        10,
        1,
        11,
    );
    assert_eq!(completed, 4);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(history.retained_start_frame(), 9);
    assert_eq!(history.len(), 1);
    assert_eq!(history.retirement_revision(), 2);
    assert_eq!(
        history.span(0),
        Some(SourcePlaybackSpan::new(9, None, 7, SourcePlaybackState::Frozen).unwrap())
    );
}

#[test]
fn begin_configuration_buffers_and_render_failures_have_exact_cleanup_and_keep_pcm_safe() {
    for phase in [
        CoreAudioSourcePhase::Publication,
        CoreAudioSourcePhase::Configuration,
        CoreAudioSourcePhase::Layout,
        CoreAudioSourcePhase::Render,
    ] {
        let (_, mut state, _) = rig(2, false);
        let before = observe(&state);
        let mut ops = Ops::new(4, &[2]);
        let code = match phase {
            CoreAudioSourcePhase::Publication => {
                ops.begin_error = true;
                BEGIN_ERROR
            }
            CoreAudioSourcePhase::Configuration => {
                ops.configuration_error = true;
                CONFIGURATION_ERROR
            }
            CoreAudioSourcePhase::Layout => {
                ops.buffers_error = true;
                BUFFERS_ERROR
            }
            CoreAudioSourcePhase::Render => {
                ops.render_error = true;
                RENDER_ERROR
            }
            _ => unreachable!(),
        };
        assert_eq!(failure(step(&mut state, &mut ops), phase).code, code);
        assert_eq!(observe(&state), before);
        let effects = ops.effects.borrow();
        if phase == CoreAudioSourcePhase::Publication {
            assert_eq!(effects.as_slice(), &[Effect::Begin]);
            assert_eq!(ops.revision, 40);
        } else {
            assert_eq!(effects.as_slice().last(), Some(&Effect::End));
            assert_eq!(
                effects
                    .as_slice()
                    .iter()
                    .filter(|effect| **effect == Effect::End)
                    .count(),
                1
            );
            assert_eq!(ops.revision, 42);
        }
        assert!(!effects
            .as_slice()
            .iter()
            .any(|effect| matches!(effect, Effect::Copy(_) | Effect::Delivered(..))));
        assert_eq!(ops.cadence, 0);
        ops.untouched();
    }
}

#[test]
fn calibrated_allocator_observes_known_heap_work_and_zero_callback_heap_work() {
    let (value, calibration) = count_heap_calls(|| {
        let value = Box::new(19u64);
        std::hint::black_box(&value);
        *value
    });
    assert_eq!(value, 19);
    assert!(
        calibration >= 2,
        "allocator probe must observe a known allocation and destruction"
    );
    let (_, mut fresh, _) = rig(2, false);
    let mut render = Ops::new(4, &[2]);
    let (result, calls) = count_heap_calls(|| step(&mut fresh, &mut render));
    delivered(result, 0, 4, 4);
    assert_eq!(calls, 0);
    let (_, mut pending, _) = rig(2, false);
    pending.render_pending(8).unwrap();
    let mut delivery = Ops::new(2, &[2]);
    let (result, calls) = count_heap_calls(|| step(&mut pending, &mut delivery));
    delivered(result, 0, 2, 2);
    assert_eq!(calls, 0);
    let mut refusal = Ops::new(7, &[2]);
    let (result, calls) = count_heap_calls(|| step(&mut pending, &mut refusal));
    failure(result, CoreAudioSourcePhase::Pending);
    assert_eq!(calls, 0);
    let (_, mut empty, _) = rig(2, false);
    let slot = SourceRetirementSlot::new();
    slot.submit(SourceRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    let mut retire = Ops::new(4, &[2]);
    let (result, calls) = count_heap_calls(|| {
        source_callback_step(&mut empty, &slot, &AtomicU32::new(0), &mut retire, &mut 0)
    });
    assert_eq!(result, CoreAudioSourceStep::Retire);
    assert_eq!(calls, 0);
}
