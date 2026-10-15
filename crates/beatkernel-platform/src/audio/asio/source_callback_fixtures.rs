//! Executes production ASIO Source delivery with the actual Mixer and history.
//! Software fixtures establish neither SDK ABI nor native/acoustic acceptance.
use super::*;
use crate::audio::{
    asio::encode_asio_channel_validated,
    count_heap_calls,
    source_output_retirement::{
        SourceRetirementError, SourceRetirementPolicy, SourceRetirementStatus,
    },
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use std::cell::RefCell;

const PATTERN: [f32; 8] = [0.125, -0.25, 0.375, -0.5, 0.625, -0.75, 0.875, -1.];
const UNTOUCHED: u8 = 0xa7;
const ENCODINGS: [AsioPcmEncoding; 18] = [
    AsioPcmEncoding::Int16Msb,
    AsioPcmEncoding::Int24Msb,
    AsioPcmEncoding::Int32Msb,
    AsioPcmEncoding::Float32Msb,
    AsioPcmEncoding::Float64Msb,
    AsioPcmEncoding::Int32Msb16,
    AsioPcmEncoding::Int32Msb18,
    AsioPcmEncoding::Int32Msb20,
    AsioPcmEncoding::Int32Msb24,
    AsioPcmEncoding::Int16Lsb,
    AsioPcmEncoding::Int24Lsb,
    AsioPcmEncoding::Int32Lsb,
    AsioPcmEncoding::Float32Lsb,
    AsioPcmEncoding::Float64Lsb,
    AsioPcmEncoding::Int32Lsb16,
    AsioPcmEncoding::Int32Lsb18,
    AsioPcmEncoding::Int32Lsb20,
    AsioPcmEncoding::Int32Lsb24,
];

fn gain(channel: usize) -> f32 {
    [1., -0.5, 0.25][channel % 3]
}
fn layout(encodings: &[AsioPcmEncoding]) -> AsioPlanarLayout {
    let planes: Vec<_> = encodings
        .iter()
        .enumerate()
        .map(|(i, encoding)| (i as u32 * 3 + 2, *encoding))
        .collect();
    AsioPlanarLayout::new(
        AudioFormat::new(1000, encodings.len() as u16).unwrap(),
        &planes,
    )
    .unwrap()
}
fn configure(state: &mut NativeOutputState, encodings: &[AsioPcmEncoding], frames: usize) {
    let matrix = ChannelMatrix::new(
        1,
        encodings.len() as u16,
        &(0..encodings.len()).map(gain).collect::<Vec<_>>(),
    )
    .unwrap();
    state
        .reconfigure_asio(layout(encodings), Some(matrix), frames)
        .unwrap();
}
fn rig(
    encodings: &[AsioPcmEncoding],
    practice: bool,
) -> (
    CommandProducer,
    NativeOutputState,
    Option<PracticeController>,
) {
    let source = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(512, 512, 1).unwrap();
    let mut bank = SampleBank::new(source, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            source,
            PATTERN.into_iter().cycle().take(128).collect(),
            limits,
        )
        .unwrap(),
    )
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
    let mut state = NativeOutputState::from_mixer(mixer);
    configure(&mut state, encodings, 8);
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
    spans: Vec<SourcePlaybackSpan>,
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
        spans: (0..history.len())
            .map(|i| history.span(i).unwrap())
            .collect(),
        quarantined: state.admission_is_uncertain(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    None,
    Configuration,
    Buffers,
    Begin(u32),
    History,
    Render(usize),
    Generated(bool),
    Copy(usize),
    Delivered(u64, u32, u64),
    End,
}
struct Effects {
    values: [Effect; 96],
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
    Channel,
    Encoding,
    ZeroBytes,
    PartialBytes,
    UnequalFrames,
    Null,
    Overflow,
    Overlap,
    SourceAlias(usize),
}
const BEGIN_ERROR: i32 = -101;
const CONFIGURATION_ERROR: i32 = -102;
const BUFFERS_ERROR: i32 = -103;
const RENDER_ERROR: i32 = -104;
struct Ops {
    effects: RefCell<Effects>,
    frames: usize,
    layout: AsioPlanarLayout,
    descriptors: [AsioPlaneDescriptor; 32],
    destination: [[u8; 80]; 32],
    corruption: Corruption,
    history: Option<SourceHistoryRetirementRequest>,
    begin_error: bool,
    configuration_error: bool,
    buffers_error: bool,
    render_error: bool,
    cadence: usize,
    delivery: Option<(RenderReport, u64, u32, u64)>,
    begun: usize,
    ended: usize,
}
impl Ops {
    fn new(frames: usize, encodings: &[AsioPcmEncoding]) -> Self {
        Self {
            effects: RefCell::new(Effects {
                values: [Effect::None; 96],
                len: 0,
            }),
            frames,
            layout: layout(encodings),
            descriptors: [AsioPlaneDescriptor::default(); 32],
            destination: [[UNTOUCHED; 80]; 32],
            corruption: Corruption::None,
            history: None,
            begin_error: false,
            configuration_error: false,
            buffers_error: false,
            render_error: false,
            cadence: 0,
            delivery: None,
            begun: 0,
            ended: 0,
        }
    }
    fn effect(&self, effect: Effect) {
        self.effects.borrow_mut().push(effect);
    }
    fn prepare_descriptors(&mut self) {
        for (index, (channel, encoding)) in self.layout.channels().iter().copied().enumerate() {
            self.descriptors[index] = AsioPlaneDescriptor {
                channel,
                encoding,
                byte_size: self.frames * encoding.bytes_per_sample(),
                address: self.destination[index].as_mut_ptr() as usize,
            };
        }
        let last = self.layout.channels().len() - 1;
        match self.corruption {
            Corruption::None | Corruption::Empty => {}
            Corruption::Channel => self.descriptors[last].channel += 1,
            Corruption::Encoding => self.descriptors[last].encoding = AsioPcmEncoding::Float32Lsb,
            Corruption::ZeroBytes => self.descriptors[last].byte_size = 0,
            Corruption::PartialBytes => self.descriptors[last].byte_size -= 1,
            Corruption::UnequalFrames => {
                self.descriptors[last].byte_size +=
                    self.descriptors[last].encoding.bytes_per_sample()
            }
            Corruption::Null => self.descriptors[last].address = 0,
            Corruption::Overflow => self.descriptors[last].address = usize::MAX - 3,
            Corruption::Overlap => self.descriptors[last].address = self.descriptors[0].address + 1,
            Corruption::SourceAlias(address) => self.descriptors[last].address = address,
        }
    }
    fn untouched(&self) {
        assert_eq!(self.destination, [[UNTOUCHED; 80]; 32]);
    }
    fn no_copy_or_delivery(&self) {
        assert!(!self
            .effects
            .borrow()
            .as_slice()
            .iter()
            .any(|e| matches!(e, Effect::Copy(_) | Effect::Delivered(..))));
        assert_eq!(self.delivery, None);
        self.untouched();
    }
}
impl AsioSourceOps for Ops {
    fn check_configuration(&mut self) -> Result<(), i32> {
        self.effect(Effect::Configuration);
        if self.configuration_error {
            Err(CONFIGURATION_ERROR)
        } else {
            Ok(())
        }
    }
    fn buffers(&mut self) -> Result<&[AsioPlaneDescriptor], i32> {
        self.effect(Effect::Buffers);
        if self.buffers_error {
            return Err(BUFFERS_ERROR);
        }
        self.prepare_descriptors();
        let count = if matches!(self.corruption, Corruption::Empty) {
            0
        } else {
            self.layout.channels().len()
        };
        Ok(&self.descriptors[..count])
    }
    fn begin_publication(&mut self, frames: u32) -> Result<(), i32> {
        self.effect(Effect::Begin(frames));
        if self.begin_error {
            Err(BEGIN_ERROR)
        } else {
            self.begun += 1;
            Ok(())
        }
    }
    fn end_publication(&mut self) {
        self.effect(Effect::End);
        self.ended += 1;
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
            return Err(RENDER_ERROR);
        }
        let report = state.render_pending(frames).map_err(|_| RENDER_ERROR)?;
        self.cadence += 1;
        Ok(report)
    }
    fn publish_generated(&mut self, _: &NativeOutputState, report: Option<RenderReport>) {
        self.effect(Effect::Generated(report.is_some()));
    }
    fn copy_plane(
        &mut self,
        plane: AsioPlaneDescriptor,
        samples: &[f32],
        channels: usize,
        channel: usize,
    ) {
        let index = self
            .layout
            .channels()
            .iter()
            .position(|(native, _)| *native == plane.channel)
            .unwrap();
        assert_eq!(plane.address, self.destination[index].as_ptr() as usize);
        self.effect(Effect::Copy(index));
        encode_asio_channel_validated(
            plane.encoding,
            samples,
            channels,
            channel,
            &mut self.destination[index][..plane.byte_size],
        );
    }
    fn delivered(&mut self, report: RenderReport, first_frame: u64, frames: u32, end_frame: u64) {
        self.effect(Effect::Delivered(first_frame, frames, end_frame));
        self.delivery = Some((report, first_frame, frames, end_frame));
    }
}
fn permit(
    state: &NativeOutputState,
    slot: &SourceRetirementSlot,
    geometry: &AtomicU32,
) -> AsioSourcePermit {
    match source_callback_preflight(state, slot, geometry) {
        AsioSourcePreflight::Continue(permit) => permit,
        other => panic!("expected Continue, got {other:?}"),
    }
}
fn step(state: &mut NativeOutputState, ops: &mut Ops) -> AsioSourceStep {
    let permit = permit(state, &SourceRetirementSlot::new(), &AtomicU32::new(0));
    source_callback_fill(state, permit, ops, &mut 0)
}
fn failure(result: AsioSourceStep, phase: AsioSourcePhase) -> AsioSourceFailure {
    let AsioSourceStep::Failure(error) = result else {
        panic!("expected {phase:?}, got {result:?}")
    };
    assert_eq!(error.phase, phase);
    error
}
fn delivered(result: AsioSourceStep, first: u64, frames: u32, end: u64) -> RenderReport {
    let AsioSourceStep::Delivered {
        report,
        first_frame,
        frames: actual,
        end_frame,
    } = result
    else {
        panic!("expected delivered native prefix, got {result:?}")
    };
    assert_eq!((first_frame, actual, end_frame), (first, frames, end));
    report
}

// Independent byte oracle: does not call the production encoder/quantizer.
fn expected_sample(encoding: AsioPcmEncoding, sample: f32) -> Vec<u8> {
    use AsioPcmEncoding::*;
    match encoding {
        Float32Msb => sample.to_be_bytes().to_vec(),
        Float32Lsb => sample.to_le_bytes().to_vec(),
        Float64Msb => f64::from(sample).to_be_bytes().to_vec(),
        Float64Lsb => f64::from(sample).to_le_bytes().to_vec(),
        _ => {
            let bits = match encoding {
                Int16Msb | Int16Lsb | Int32Msb16 | Int32Lsb16 => 16,
                Int32Msb18 | Int32Lsb18 => 18,
                Int32Msb20 | Int32Lsb20 => 20,
                Int24Msb | Int24Lsb | Int32Msb24 | Int32Lsb24 => 24,
                _ => 32,
            };
            let limit = 1i64 << (bits - 1);
            let signed =
                ((f64::from(sample) * limit as f64).round() as i64).clamp(-limit, limit - 1);
            let raw = (signed as u64 & ((1u64 << bits) - 1)) as u32;
            let width = encoding.bytes_per_sample();
            if matches!(
                encoding,
                Int16Lsb | Int24Lsb | Int32Lsb | Int32Lsb16 | Int32Lsb18 | Int32Lsb20 | Int32Lsb24
            ) {
                raw.to_le_bytes()[..width].to_vec()
            } else {
                raw.to_be_bytes()[4 - width..].to_vec()
            }
        }
    }
}
fn assert_pcm(ops: &Ops, first: usize, frames: usize) {
    for (channel, (_, encoding)) in ops.layout.channels().iter().copied().enumerate() {
        let expected: Vec<_> = (first..first + frames)
            .flat_map(|f| expected_sample(encoding, PATTERN[f % PATTERN.len()] * gain(channel)))
            .collect();
        assert_eq!(
            &ops.destination[channel][..expected.len()],
            expected.as_slice(),
            "channel {channel}: {encoding:?}"
        );
        assert!(ops.destination[channel][expected.len()..]
            .iter()
            .all(|b| *b == UNTOUCHED));
    }
}

#[test]
fn mixed_eighteen_encodings_deliver_real_nonzero_pcm_in_exact_native_channel_order() {
    let (_, mut state, _) = rig(&ENCODINGS, false);
    let mut ops = Ops::new(8, &ENCODINGS);
    let (result, calls) = count_heap_calls(|| step(&mut state, &mut ops));
    let report = delivered(result, 0, 8, 8);
    assert_eq!(
        (report.start_frame, report.frames, report.playback_frames),
        (0, 8, 8)
    );
    assert_eq!(calls, 0);
    assert_pcm(&ops, 0, 8);
    assert_eq!((ops.cadence, ops.begun, ops.ended), (1, 1, 1));
    assert_eq!(state.pending_frames(), 0);
    assert_eq!(state.output_frame_basis().start_physical_frame(), 8);
}

#[test]
fn cold_smaller_native_buffer_delivers_retained_prefixes_without_rerender_or_report_rewrite() {
    let encodings = [AsioPcmEncoding::Float32Lsb, AsioPcmEncoding::Int24Msb];
    let (_, mut state, _) = rig(&encodings, false);
    let report = state.render_pending(8).unwrap();
    state.admit(2).unwrap();
    let before = observe(&state);
    configure(&mut state, &encodings, 3);
    assert_eq!(observe(&state), before);
    for first in [2, 5] {
        let mut ops = Ops::new(3, &encodings);
        let (result, calls) = count_heap_calls(|| step(&mut state, &mut ops));
        assert_eq!(delivered(result, first, 3, first + 3), report);
        assert_eq!(calls, 0);
        assert_pcm(&ops, first as usize, 3);
        assert_eq!(ops.cadence, 0);
        assert_eq!(
            (
                state.mixer().frame_cursor(),
                state.mixer().playback_frame_cursor()
            ),
            (8, 8)
        );
        assert_eq!(state.output_frame_basis().start_physical_frame(), first + 3);
        assert_eq!(
            state.mixer().source_playback_history().unwrap() as *const _ as usize,
            before.history_address
        );
        if first == 2 {
            assert_eq!(state.pending_report(), Some(report));
            assert_eq!(state.admitted_frames(), 5);
        }
    }
    assert_eq!(state.pending_report(), None);
    let mut next = Ops::new(3, &encodings);
    let next_report = delivered(step(&mut state, &mut next), 8, 3, 11);
    assert_ne!(next_report, report);
    assert_eq!(next.cadence, 1);
    assert_pcm(&next, 8, 3);
}

#[test]
fn short_and_nonwhole_retained_suffixes_refuse_before_history_render_or_any_copy() {
    let encodings = [AsioPcmEncoding::Float32Lsb, AsioPcmEncoding::Float64Msb];
    for frames in [4, 7, 8] {
        let (_, mut state, _) = rig(&encodings, false);
        state.render_pending(8).unwrap();
        state.admit(2).unwrap();
        configure(&mut state, &encodings, frames);
        let before = observe(&state);
        let mut ops = Ops::new(frames, &encodings);
        ops.history = Some(SourceHistoryRetirementRequest {
            revision: 1,
            source_pin: u64::MAX,
        });
        let (result, calls) = count_heap_calls(|| step(&mut state, &mut ops));
        failure(result, AsioSourcePhase::Pending);
        assert_eq!(calls, 0);
        assert_eq!(observe(&state), before);
        ops.no_copy_or_delivery();
        assert!(!ops
            .effects
            .borrow()
            .as_slice()
            .iter()
            .any(|e| matches!(e, Effect::History | Effect::Render(_))));
        assert_eq!(ops.begun, ops.ended);
    }
}

#[test]
fn malformed_final_plane_preserves_every_earlier_destination_and_source_owner() {
    let encodings = [AsioPcmEncoding::Int16Lsb, AsioPcmEncoding::Float64Lsb];
    for corruption in [
        Corruption::Empty,
        Corruption::Channel,
        Corruption::Encoding,
        Corruption::ZeroBytes,
        Corruption::PartialBytes,
        Corruption::UnequalFrames,
        Corruption::Null,
        Corruption::Overflow,
        Corruption::Overlap,
    ] {
        let (_, mut state, _) = rig(&encodings, false);
        configure(&mut state, &encodings, 4);
        let before = observe(&state);
        let mut ops = Ops::new(4, &encodings);
        ops.corruption = corruption;
        let (result, calls) = count_heap_calls(|| step(&mut state, &mut ops));
        failure(result, AsioSourcePhase::Layout);
        assert_eq!(calls, 0);
        assert_eq!(observe(&state), before);
        assert_eq!(ops.cadence, 0);
        ops.no_copy_or_delivery();
        assert_eq!(ops.begun, ops.ended);
    }
    for frames in [0, 3, 5, 9] {
        let (_, mut state, _) = rig(&encodings, false);
        configure(&mut state, &encodings, 4);
        let before = observe(&state);
        let mut ops = Ops::new(frames, &encodings);
        failure(step(&mut state, &mut ops), AsioSourcePhase::Layout);
        assert_eq!(observe(&state), before);
        ops.no_copy_or_delivery();
    }
}

#[test]
fn heterogeneous_descriptors_require_exact_fixed_frame_count_and_checked_disjoint_ranges() {
    let encodings = [AsioPcmEncoding::Int24Msb, AsioPcmEncoding::Float64Lsb];
    for frames in 1..=8 {
        let mut ops = Ops::new(frames, &encodings);
        ops.prepare_descriptors();
        let geometry =
            validate_plane_descriptors(&ops.layout, &ops.descriptors[..2], frames).unwrap();
        assert_eq!((geometry.frames, geometry.channels), (frames, 2));
        for fixed in [0, frames + 1, usize::MAX] {
            assert!(validate_plane_descriptors(&ops.layout, &ops.descriptors[..2], fixed).is_err());
        }
        assert!(validate_plane_descriptors(&ops.layout, &ops.descriptors[..1], frames).is_err());
        let mut malformed = ops.descriptors;
        malformed[1].byte_size = usize::MAX;
        assert!(validate_plane_descriptors(&ops.layout, &malformed[..2], frames).is_err());
        malformed[1] = malformed[0];
        assert!(validate_plane_descriptors(&ops.layout, &malformed[..2], frames).is_err());
        ops.untouched();
    }
}

#[test]
fn destination_aliasing_prefix_or_later_unsent_tail_refuses_before_any_copy() {
    let encodings = [AsioPcmEncoding::Float32Lsb, AsioPcmEncoding::Float32Msb];
    for offset in [0, 8, 40] {
        let (_, mut state, _) = rig(&encodings, false);
        state.render_pending(8).unwrap();
        configure(&mut state, &encodings, 2);
        let before = observe(&state);
        let mut ops = Ops::new(2, &encodings);
        ops.corruption =
            Corruption::SourceAlias(state.pending_samples().as_ptr() as usize + offset);
        failure(step(&mut state, &mut ops), AsioSourcePhase::Extent);
        assert_eq!(observe(&state), before);
        ops.no_copy_or_delivery();
    }
}

#[test]
fn original_delivery_coordinates_reject_zero_outside_prefix_and_both_overflows() {
    let (_, mut state, _) = rig(&ENCODINGS[..1], false);
    let mut report = state.render_pending(8).unwrap();
    report.start_frame = 10;
    assert_eq!(checked_native_interval(report, 2, 3), Ok((12, 15)));
    assert_eq!(checked_native_interval(report, 7, 1), Ok((17, 18)));
    for (admitted, frames) in [(0, 0), (8, 1), (9, 1), (usize::MAX, 1), (7, 2), (0, 9)] {
        assert!(checked_native_interval(report, admitted, frames).is_err());
    }
    report.start_frame = u64::MAX;
    assert!(checked_native_interval(report, 1, 1).is_err());
    assert!(checked_native_interval(report, 0, 1).is_err());
    report.start_frame = u64::MAX - 2;
    assert_eq!(
        checked_native_interval(report, 1, 1),
        Ok((u64::MAX - 1, u64::MAX))
    );
    assert!(checked_native_interval(report, 1, 2).is_err());
}

#[test]
fn retired_preflight_remains_sticky_and_changes_no_source_or_native_state() {
    for policy in [
        SourceRetirementPolicy::PreservePausedSuffix,
        SourceRetirementPolicy::RequireEmptySuffix,
    ] {
        let (_, state, _) = rig(&ENCODINGS[..2], false);
        let before = observe(&state);
        let slot = SourceRetirementSlot::new();
        let geometry = AtomicU32::new(4);
        let nonce = slot.submit(policy).unwrap().unwrap();
        for _ in 0..3 {
            let (result, calls) =
                count_heap_calls(|| source_callback_preflight(&state, &slot, &geometry));
            assert!(matches!(result, AsioSourcePreflight::Retire));
            assert_eq!(calls, 0);
            assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Authorized));
            assert_eq!(observe(&state), before);
        }
    }
}

#[test]
fn paused_zero_suffix_retirement_requires_whole_requested_native_buffers() {
    for requested in [0, 1, 2, 3, 4, 6, 7, 8] {
        let (mut producer, mut state, _) = rig(&ENCODINGS[..2], false);
        producer.request_pause(true);
        let report = state.render_pending(8).unwrap();
        state.admit(2).unwrap();
        configure(&mut state, &ENCODINGS[..2], 2);
        assert!(report.paused && state.paused_tail_admissible());
        let slot = SourceRetirementSlot::new();
        let geometry = AtomicU32::new(99);
        let nonce = slot
            .submit_prepared(SourceRetirementPolicy::PreservePausedSuffix, || {
                geometry.store(requested, Ordering::Relaxed)
            })
            .unwrap()
            .unwrap();
        let before = observe(&state);
        let result = source_callback_preflight(&state, &slot, &geometry);
        if requested != 0 && 6 % requested == 0 {
            assert!(matches!(result, AsioSourcePreflight::Retire));
            assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Authorized));
            assert_eq!(observe(&state), before);
        } else {
            let AsioSourcePreflight::Continue(granted) = result else {
                panic!("invalid geometry must refuse retirement")
            };
            assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Refused));
            let mut ops = Ops::new(2, &ENCODINGS[..2]);
            assert_eq!(
                delivered(
                    source_callback_fill(&mut state, granted, &mut ops, &mut 0),
                    2,
                    2,
                    4
                ),
                report
            );
            for (i, (_, encoding)) in ops.layout.channels().iter().enumerate() {
                assert!(ops.destination[i][..2 * encoding.bytes_per_sample()]
                    .iter()
                    .all(|b| *b == 0));
            }
            assert_eq!(ops.cadence, 0);
        }
    }
}

#[test]
fn running_pending_refuses_retirement_and_cancelled_or_foreign_nonce_never_authorizes() {
    let (_, mut state, _) = rig(&ENCODINGS[..2], false);
    state.render_pending(8).unwrap();
    configure(&mut state, &ENCODINGS[..2], 2);
    let slot = SourceRetirementSlot::new();
    let geometry = AtomicU32::new(2);
    let nonce = slot
        .submit(SourceRetirementPolicy::PreservePausedSuffix)
        .unwrap()
        .unwrap();
    let granted = permit(&state, &slot, &geometry);
    assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Refused));
    assert_eq!(slot.poll(nonce), Ok(SourceRetirementStatus::Refused));
    let next = slot
        .submit(SourceRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    assert_ne!(nonce, next);
    assert_eq!(
        slot.status(nonce),
        Err(SourceRetirementError::InvalidTicket)
    );
    assert_eq!(
        slot.cancel(nonce),
        Err(SourceRetirementError::InvalidTicket)
    );
    assert_eq!(slot.cancel(next), Ok(SourceRetirementStatus::Cancelled));
    delivered(
        source_callback_fill(
            &mut state,
            granted,
            &mut Ops::new(2, &ENCODINGS[..2]),
            &mut 0,
        ),
        0,
        2,
        2,
    );
    assert!(matches!(
        source_callback_preflight(&state, &slot, &geometry),
        AsioSourcePreflight::Continue(_)
    ));
    assert_eq!(slot.status(next), Ok(SourceRetirementStatus::Cancelled));
}

#[test]
fn request_published_after_preflight_waits_for_next_callback_without_a_second_decision() {
    let (_, mut state, _) = rig(&ENCODINGS[..2], false);
    configure(&mut state, &ENCODINGS[..2], 2);
    let slot = SourceRetirementSlot::new();
    let geometry = AtomicU32::new(2);
    let granted = permit(&state, &slot, &geometry);
    let nonce = slot
        .submit(SourceRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    let mut ops = Ops::new(2, &ENCODINGS[..2]);
    delivered(
        source_callback_fill(&mut state, granted, &mut ops, &mut 0),
        0,
        2,
        2,
    );
    assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Pending));
    assert_pcm(&ops, 0, 2);
    let before = observe(&state);
    assert!(matches!(
        source_callback_preflight(&state, &slot, &geometry),
        AsioSourcePreflight::Retire
    ));
    assert_eq!(slot.status(nonce), Ok(SourceRetirementStatus::Authorized));
    assert_eq!(observe(&state), before);
}

#[test]
fn wrong_owner_permit_refuses_before_every_native_effect() {
    let (_, first, _) = rig(&ENCODINGS[..2], false);
    let (_, mut second, _) = rig(&ENCODINGS[..2], false);
    let granted = permit(&first, &SourceRetirementSlot::new(), &AtomicU32::new(0));
    let before = observe(&second);
    let mut ops = Ops::new(8, &ENCODINGS[..2]);
    failure(
        source_callback_fill(&mut second, granted, &mut ops, &mut 0),
        AsioSourcePhase::Permit,
    );
    assert!(ops.effects.borrow().as_slice().is_empty());
    assert_eq!(observe(&second), before);
    ops.untouched();
}

#[test]
fn quarantined_owner_refuses_both_preflight_and_an_already_issued_permit() {
    let (_, mut state, _) = rig(&ENCODINGS[..2], false);
    state.render_pending(8).unwrap();
    let granted = permit(&state, &SourceRetirementSlot::new(), &AtomicU32::new(0));
    state.mark_admission_uncertain();
    let before = observe(&state);
    let (result, calls) = count_heap_calls(|| {
        source_callback_preflight(&state, &SourceRetirementSlot::new(), &AtomicU32::new(0))
    });
    let AsioSourcePreflight::Failure(error) = result else {
        panic!("quarantine must refuse")
    };
    assert_eq!(error.phase, AsioSourcePhase::Quarantined);
    assert_eq!(calls, 0);
    let mut ops = Ops::new(8, &ENCODINGS[..2]);
    failure(
        source_callback_fill(&mut state, granted, &mut ops, &mut 0),
        AsioSourcePhase::Quarantined,
    );
    assert!(ops.effects.borrow().as_slice().is_empty());
    assert_eq!(observe(&state), before);
    ops.untouched();
}

#[test]
fn counter_publication_configuration_buffers_and_render_failures_copy_nothing() {
    for phase in [
        AsioSourcePhase::Publication,
        AsioSourcePhase::Configuration,
        AsioSourcePhase::Layout,
        AsioSourcePhase::Render,
    ] {
        let (_, mut state, _) = rig(&ENCODINGS[..2], false);
        configure(&mut state, &ENCODINGS[..2], 4);
        let before = observe(&state);
        let mut ops = Ops::new(4, &ENCODINGS[..2]);
        let code = match phase {
            AsioSourcePhase::Publication => {
                ops.begin_error = true;
                BEGIN_ERROR
            }
            AsioSourcePhase::Configuration => {
                ops.configuration_error = true;
                CONFIGURATION_ERROR
            }
            AsioSourcePhase::Layout => {
                ops.buffers_error = true;
                BUFFERS_ERROR
            }
            AsioSourcePhase::Render => {
                ops.render_error = true;
                RENDER_ERROR
            }
            _ => unreachable!(),
        };
        let (result, calls) = count_heap_calls(|| step(&mut state, &mut ops));
        assert_eq!(failure(result, phase).code, code);
        assert_eq!(calls, 0);
        assert_eq!(observe(&state), before);
        assert_eq!(ops.begun, ops.ended);
        assert_eq!(ops.cadence, 0);
        ops.no_copy_or_delivery();
    }
}

#[test]
fn invalid_original_history_revision_or_range_refuses_before_pending_clamp_or_copy() {
    for (revision, source_pin) in [(0, 0), (1, 0), (2, 9)] {
        let (_, mut state, _) = rig(&ENCODINGS[..2], false);
        state.render_pending(8).unwrap();
        state.admit(2).unwrap();
        configure(&mut state, &ENCODINGS[..2], 2);
        let before = observe(&state);
        let mut ops = Ops::new(2, &ENCODINGS[..2]);
        ops.history = Some(SourceHistoryRetirementRequest {
            revision,
            source_pin,
        });
        let mut completed = 0;
        let granted = permit(&state, &SourceRetirementSlot::new(), &AtomicU32::new(0));
        failure(
            source_callback_fill(&mut state, granted, &mut ops, &mut completed),
            AsioSourcePhase::History,
        );
        assert_eq!(completed, 0);
        assert_eq!(observe(&state), before);
        assert_eq!(ops.begun, ops.ended);
        ops.no_copy_or_delivery();
    }
}

#[test]
fn original_pending_report_pins_real_history_until_the_callback_after_final_admission() {
    let encodings = [AsioPcmEncoding::Float32Lsb, AsioPcmEncoding::Float32Msb];
    let (mut producer, mut state, _) = rig(&encodings, false);
    let report = state.render_pending(8).unwrap();
    state.admit(2).unwrap();
    configure(&mut state, &encodings, 2);
    let request = SourceHistoryRetirementRequest {
        revision: 2,
        source_pin: 8,
    };
    let mut completed = 0;
    for first in [2, 4, 6] {
        let mut ops = Ops::new(2, &encodings);
        ops.history = Some(request);
        let granted = permit(&state, &SourceRetirementSlot::new(), &AtomicU32::new(0));
        assert_eq!(
            delivered(
                source_callback_fill(&mut state, granted, &mut ops, &mut completed),
                first,
                2,
                first + 2
            ),
            report
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
    }
    let mut ops = Ops::new(2, &encodings);
    ops.history = Some(request);
    let granted = permit(&state, &SourceRetirementSlot::new(), &AtomicU32::new(0));
    delivered(
        source_callback_fill(&mut state, granted, &mut ops, &mut completed),
        8,
        2,
        10,
    );
    assert_eq!(completed, 2);
    producer.request_pause(true);
    let mut pause = Ops::new(2, &encodings);
    assert!(delivered(step(&mut state, &mut pause), 10, 2, 12).paused);
    let mut compact = Ops::new(2, &encodings);
    compact.history = Some(SourceHistoryRetirementRequest {
        revision: 4,
        source_pin: 10,
    });
    let granted = permit(&state, &SourceRetirementSlot::new(), &AtomicU32::new(0));
    delivered(
        source_callback_fill(&mut state, granted, &mut compact, &mut completed),
        12,
        2,
        14,
    );
    assert_eq!(completed, 4);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(history.retained_start_frame(), 10);
    assert_eq!(
        history.span(0),
        Some(SourcePlaybackSpan::new(10, None, 10, SourcePlaybackState::Frozen).unwrap())
    );
}

#[test]
fn practice_receipt_and_pending_pcm_both_pin_original_source_history() {
    let encodings = [AsioPcmEncoding::Float32Lsb];
    let (mut producer, mut state, controller) = rig(&encodings, true);
    let mut controller = controller.unwrap();
    configure(&mut state, &encodings, 2);
    for paused in [false, true] {
        producer.request_pause(paused);
        let first = state.output_frame_basis().start_physical_frame();
        delivered(
            step(&mut state, &mut Ops::new(2, &encodings)),
            first,
            2,
            first + 2,
        );
    }
    producer.request_pause(false);
    configure(&mut state, &encodings, 4);
    let report = state.render_pending(4).unwrap();
    assert_eq!(report.start_frame, 4);
    configure(&mut state, &encodings, 1);
    assert_eq!(state.mixer().practice_history_retention_pins().0, Some(0));
    let mut completed = 0;
    for first in [4, 5, 6, 7] {
        if first == 5 {
            assert_eq!(
                controller
                    .try_pop_projected_receipt()
                    .unwrap()
                    .receipt
                    .physical_frame,
                0
            );
            assert_eq!(state.mixer().practice_history_retention_pins().0, None);
        }
        let mut ops = Ops::new(1, &encodings);
        ops.history = Some(SourceHistoryRetirementRequest {
            revision: 2,
            source_pin: 8,
        });
        let granted = permit(&state, &SourceRetirementSlot::new(), &AtomicU32::new(0));
        assert_eq!(
            delivered(
                source_callback_fill(&mut state, granted, &mut ops, &mut completed),
                first,
                1,
                first + 1
            ),
            report
        );
        assert_eq!(completed, 0);
        assert_eq!(
            state
                .mixer()
                .source_playback_history()
                .unwrap()
                .retained_start_frame(),
            if first == 4 { 0 } else { 4 }
        );
    }
    let mut ops = Ops::new(1, &encodings);
    ops.history = Some(SourceHistoryRetirementRequest {
        revision: 2,
        source_pin: 8,
    });
    let granted = permit(&state, &SourceRetirementSlot::new(), &AtomicU32::new(0));
    delivered(
        source_callback_fill(&mut state, granted, &mut ops, &mut completed),
        8,
        1,
        9,
    );
    assert_eq!(completed, 2);
}

#[test]
fn actual_whole_prefix_guard_rejects_nonfinite_final_channel_before_encoder_dispatch() {
    // Unit coverage of the actual guard used by fill. The public Mixer itself
    // guarantees finite PCM, so no invalid shared-reference mutation is used.
    assert_eq!(validate_prefix_samples(&[0.125, -0.25, 0.5, -1.]), Ok(()));
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for index in 0..6 {
            let mut prefix = [0.25; 6];
            prefix[index] = invalid;
            let (result, calls) = count_heap_calls(|| validate_prefix_samples(&prefix));
            assert_eq!(result, Err(AsioSourcePhase::Pcm));
            assert_eq!(calls, 0);
        }
    }
}

#[test]
fn actual_admission_failure_guard_irreversibly_quarantines_real_retained_owner() {
    // Unit coverage of the exact post-copy guard called by fill, without an
    // overridable production admission operation or unsafe aliasing injection.
    let (_, mut state, _) = rig(&ENCODINGS[..2], false);
    let report = state.render_pending(8).unwrap();
    state.admit(2).unwrap();
    let pcm = state.pending_samples().to_vec();
    let basis = state.output_frame_basis();
    let (result, calls) = count_heap_calls(|| admit_delivery(&mut state, 7));
    assert_eq!(result.unwrap_err().phase, AsioSourcePhase::Admission);
    assert_eq!(calls, 0);
    assert!(state.admission_is_uncertain());
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.pending_samples(), pcm);
    assert_eq!(state.output_frame_basis(), basis);
    assert!(state.admit(1).is_err());
    assert!(state.render_pending(1).is_err());
    assert!(state
        .validate_reconfigure_asio(&layout(&ENCODINGS[..2]), None, 2)
        .is_err());
    assert!(matches!(
        source_callback_preflight(&state, &SourceRetirementSlot::new(), &AtomicU32::new(0)),
        AsioSourcePreflight::Failure(AsioSourceFailure {
            phase: AsioSourcePhase::Quarantined,
            ..
        })
    ));
}

#[test]
fn calibrated_allocator_observes_heap_work_but_no_fresh_retained_refused_or_retired_heap_work() {
    let (value, calibration) = count_heap_calls(|| {
        let boxed = Box::new(19u64);
        std::hint::black_box(&boxed);
        *boxed
    });
    assert_eq!(value, 19);
    assert!(
        calibration >= 2,
        "probe must observe allocation and destruction"
    );
    for frames in 1..=8 {
        let (_, mut state, _) = rig(&ENCODINGS[..2], false);
        configure(&mut state, &ENCODINGS[..2], frames);
        let mut ops = Ops::new(frames, &ENCODINGS[..2]);
        let (result, calls) = count_heap_calls(|| step(&mut state, &mut ops));
        delivered(result, 0, frames as u32, frames as u64);
        assert_eq!(calls, 0);
    }
    let (_, mut state, _) = rig(&ENCODINGS[..2], false);
    state.render_pending(8).unwrap();
    configure(&mut state, &ENCODINGS[..2], 2);
    let mut ops = Ops::new(2, &ENCODINGS[..2]);
    let (result, calls) = count_heap_calls(|| step(&mut state, &mut ops));
    delivered(result, 0, 2, 2);
    assert_eq!(calls, 0);
    configure(&mut state, &ENCODINGS[..2], 4);
    let mut refusal = Ops::new(4, &ENCODINGS[..2]);
    let (result, calls) = count_heap_calls(|| step(&mut state, &mut refusal));
    failure(result, AsioSourcePhase::Pending);
    assert_eq!(calls, 0);
    let (_, empty, _) = rig(&ENCODINGS[..2], false);
    let slot = SourceRetirementSlot::new();
    slot.submit(SourceRetirementPolicy::RequireEmptySuffix)
        .unwrap()
        .unwrap();
    let (result, calls) =
        count_heap_calls(|| source_callback_preflight(&empty, &slot, &AtomicU32::new(0)));
    assert!(matches!(result, AsioSourcePreflight::Retire));
    assert_eq!(calls, 0);
}
