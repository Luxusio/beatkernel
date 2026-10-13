//! Real converted owner and native presentation with deterministic effect ports.
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    native_audio_presentation::{NativeAudioPresentation, TargetNativeAudioSnapshot},
    native_audio_startup::new_coordinate_audio_presentation,
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
    gameplay::output::ports::{
        OriginalCoordinateTargetNativeOutputBackend, OriginalTargetNativeOutputBackend,
    },
};
use beatkernel::{
    audio::{
        command_queue, AudioFormat, AudioLimits, ChannelMatrix, CommandProducer, Mixer,
        MixerConfig, PcmLimits, ResampleQuality, SampleBank, StoppedMixerSource, TargetFrameBasis,
        TargetTime,
    },
    time::{ClockDomainId, ClockPair, ClockPoint, Timestamp},
};
use beatkernel_platform::audio::{
    ConvertedBoundaryFacts, ConvertedNativeOutputState, DeviceFormat, SampleEncoding,
    presentation::validation::OriginalNativePresentationEvidence,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
#[derive(Default)]
struct Io {
    snapshots:
        RefCell<VecDeque<Result<Option<CoordinateTargetNativeAudioSnapshot>, std::io::Error>>>,
    calls: Cell<usize>,
    legacy: Cell<usize>,
    report_fail: Cell<bool>,
    mutate_epoch: Cell<bool>,
    mutate_basis: Cell<bool>,
}
struct Output {
    owner: Option<ConvertedNativeOutputState>,
    epoch: u64,
    basis: TargetFrameBasis,
    retired: bool,
}
impl StoppedMixerSource<ConvertedNativeOutputState> for Output {
    type Error = std::io::Error;
    fn take_stopped_mixer(&mut self) -> Result<Option<ConvertedNativeOutputState>, Self::Error> {
        if self.retired {
            Ok(self.owner.take())
        } else {
            Err(std::io::Error::other("not retired"))
        }
    }
}
struct Backend {
    io: Rc<Io>,
}
impl OutputReplacementBackend<ConvertedNativeOutputState, TargetFrameBasis> for Backend {
    type Presentation = beatkernel::time::presentation::PresentationEstimator;
    type Output = Output;
    type Request = ();
    type Error = std::io::Error;
    fn open(
        &mut self,
        _: (),
        owner: ConvertedNativeOutputState,
        epoch: u64,
    ) -> Result<Output, OutputOpenFailure<Self::Error, Output, ConvertedNativeOutputState>> {
        let basis = owner.target_frame_basis();
        Ok(Output {
            owner: Some(owner),
            epoch,
            basis,
            retired: false,
        })
    }
    fn retire(&mut self, o: &mut Output) -> Result<(), Self::Error> {
        o.retired = true;
        Ok(())
    }
    fn start(&mut self, _: &mut Output) -> Result<(), Self::Error> {
        Ok(())
    }
    fn epoch(&self, o: &Output) -> u64 {
        o.epoch
    }
    fn basis(&self, o: &Output) -> TargetFrameBasis {
        o.basis
    }
    fn observe(&mut self, _: &mut Output, _: &mut Self::Presentation) -> Result<(), Self::Error> {
        panic!("coordinate owner cannot call scalar presentation")
    }
    fn render_report(&self, _: &Output) -> Result<Option<RenderReport>, Self::Error> {
        if self.io.report_fail.get() {
            Err(std::io::Error::other("report fault"))
        } else {
            Ok(None)
        }
    }
}
impl OriginalTargetNativeOutputBackend<ConvertedNativeOutputState> for Backend {
    fn planned_target_basis(
        &self,
        _: &(),
        owner: &ConvertedNativeOutputState,
    ) -> Result<TargetFrameBasis, Self::Error> {
        Ok(owner.target_frame_basis())
    }
    fn observe_native_target(
        &mut self,
        _: &mut Output,
    ) -> Result<Option<TargetNativeAudioSnapshot>, Self::Error> {
        self.io.legacy.set(self.io.legacy.get() + 1);
        panic!("coordinate owner must use original target coordinate port")
    }
    fn converted_report(
        &self,
        _: &Output,
    ) -> Result<Option<beatkernel::audio::ConvertedRenderReport>, Self::Error> {
        Ok(None)
    }
    fn boundary_facts(&self, _: &Output) -> Result<ConvertedBoundaryFacts, Self::Error> {
        Ok(ConvertedBoundaryFacts::default())
    }
    fn set_held(&mut self, _: &mut Output, _: bool) -> Result<(), Self::Error> {
        Ok(())
    }
}
impl OriginalCoordinateTargetNativeOutputBackend<ConvertedNativeOutputState> for Backend {
    fn observe_native_target_coordinate(
        &mut self,
        o: &mut Output,
    ) -> Result<Option<CoordinateTargetNativeAudioSnapshot>, Self::Error> {
        self.io.calls.set(self.io.calls.get() + 1);
        let result = self
            .io
            .snapshots
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(None));
        if self.io.mutate_epoch.get() {
            o.epoch += 1;
        }
        if self.io.mutate_basis.get() {
            o.basis = TargetFrameBasis::new(o.basis.origin(), o.basis.start_time(), 1000).unwrap();
        }
        result
    }
}
type Owner = GameplayOutputOwner<Backend, ConvertedNativeOutputState, TargetFrameBasis>;
fn rig() -> (Owner, NativeAudioPresentation, Rc<Io>, CommandProducer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 64, 1).unwrap()).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::from_nanos(100),
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let converted = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(1_000_000_001, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        8,
    )
    .unwrap_or_else(|_| panic!("valid actual converted owner"));
    let basis = converted.target_frame_basis();
    let io = Rc::new(Io::default());
    let p = new_coordinate_audio_presentation(
        7,
        AudioAuthorityCoordinateBasis::TargetFrame(basis),
        ClockDomainId(1),
        point(3, 500),
        AudioAuthorityConfig::default(),
    )
    .unwrap();
    (
        GameplayOutputOwner::new(
            Backend { io: io.clone() },
            Output {
                owner: Some(converted),
                epoch: 7,
                basis,
                retired: false,
            },
        ),
        p,
        io,
        producer,
    )
}
fn snapshot(basis: TargetFrameBasis, frame: u64, at: i64) -> CoordinateTargetNativeAudioSnapshot {
    CoordinateTargetNativeAudioSnapshot::new(
        TargetNativeAudioSnapshot {
            epoch: 7,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: point(1, at),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}
fn push(io: &Io, s: CoordinateTargetNativeAudioSnapshot) {
    io.snapshots.borrow_mut().push_back(Ok(Some(s)));
}

#[test]
fn actual_target_owner_routes_original_frame_records_to_same_authority() {
    let (mut owner, mut p, io, _producer) = rig();
    let basis = owner.current().unwrap().basis;
    for (frame, at) in [(0, 100), (1, 101)] {
        push(&io, snapshot(basis, frame, at));
        owner.observe_target_native_coordinate(&mut p).unwrap();
    }
    assert_eq!(io.calls.get(), 2);
    assert_eq!(io.legacy.get(), 0);
    assert_eq!(p.target_basis(), Some(basis));
    assert_eq!(p.authority().history_len(), 2);
    assert_eq!(
        p.authority()
            .latest_coordinate_observation()
            .unwrap()
            .coordinate(),
        Some(NativeOutputCoordinate::StreamFrame { frame: 1 })
    );
    assert_eq!(p.latest_record().unwrap().pair().source, point(2, 100));
    assert!(owner.current().unwrap().owner.is_some());
}
#[test]
fn unavailable_or_suspended_owner_has_no_admission_or_legacy_io() {
    let (mut owner, mut p, io, _producer) = rig();
    let before = format!("{p:?}");
    owner.observe_target_native_coordinate(&mut p).unwrap();
    assert_eq!(format!("{p:?}"), before);
    let output = owner.current.take().unwrap();
    let calls = io.calls.get();
    owner.observe_target_native_coordinate(&mut p).unwrap();
    assert_eq!(io.calls.get(), calls);
    assert_eq!(format!("{p:?}"), before);
    owner.current = Some(output);
}
#[test]
fn backend_error_preserves_original_software_owner_and_presentation() {
    let (mut owner, mut p, io, _producer) = rig();
    let before = format!("{p:?}");
    let pointer =
        owner.current().unwrap().owner.as_ref().unwrap() as *const ConvertedNativeOutputState;
    io.snapshots
        .borrow_mut()
        .push_back(Err(std::io::Error::other("native IO fault")));
    assert!(owner.observe_target_native_coordinate(&mut p).is_err());
    assert_eq!(format!("{p:?}"), before);
    assert_eq!(
        owner.current().unwrap().owner.as_ref().unwrap() as *const ConvertedNativeOutputState,
        pointer
    );
    assert_eq!(owner.render_report(), None);
}

#[test]
fn separate_bgm_report_read_error_does_not_commit_candidate_native_time() {
    let (mut owner, mut p, io, _producer) = rig();
    let b = owner.current().unwrap().basis;
    push(&io, snapshot(b, 0, 100));
    io.report_fail.set(true);
    let before = format!("{p:?}");
    assert!(owner.observe_target_native_coordinate(&mut p).is_err());
    assert_eq!(format!("{p:?}"), before);
    assert_eq!(p.target_basis(), None);
    assert_eq!(owner.render_report(), None);
    assert!(owner.current().unwrap().owner.is_some());
}
#[test]
fn wrong_owner_epoch_and_descriptor_refuse_before_native_io() {
    for mismatch in [false, true] {
        let (mut owner, mut p, io, _producer) = rig();
        if mismatch {
            owner.current_mut().unwrap().epoch = 8;
        } else {
            let b = owner.current().unwrap().basis;
            let descriptor = AudioAuthorityCoordinateBasis::SourceFrame(
                beatkernel::audio::OutputFrameBasis::new(b.origin(), b.sample_rate(), 0).unwrap(),
            );
            p = new_coordinate_audio_presentation(
                7,
                descriptor,
                ClockDomainId(1),
                point(3, 500),
                AudioAuthorityConfig::default(),
            )
            .unwrap();
        }
        let before = format!("{p:?}");
        assert!(owner.observe_target_native_coordinate(&mut p).is_err());
        assert_eq!(io.calls.get(), 0);
        assert_eq!(format!("{p:?}"), before);
    }
}
#[test]
fn wrong_returned_epoch_or_basis_refuses_without_pin() {
    for changed in [false, true] {
        let (mut owner, mut p, io, _producer) = rig();
        let b = owner.current().unwrap().basis;
        let wrong = if changed {
            TargetFrameBasis::new(b.origin(), TargetTime::from_frames(0, 1).unwrap(), 1000).unwrap()
        } else {
            b
        };
        let s = snapshot(wrong, 0, 100).snapshot();
        let s = CoordinateTargetNativeAudioSnapshot::new(
            TargetNativeAudioSnapshot {
                epoch: if changed { 7 } else { 8 },
                ..s
            },
            NativeOutputCoordinate::StreamFrame { frame: 0 },
        )
        .unwrap();
        push(&io, s);
        let before = format!("{p:?}");
        assert!(owner.observe_target_native_coordinate(&mut p).is_err());
        assert_eq!(format!("{p:?}"), before);
        assert_eq!(p.target_basis(), None);
    }
}
#[test]
fn owner_identity_changed_during_native_read_refuses_before_admission() {
    for epoch in [false, true] {
        let (mut owner, mut p, io, _producer) = rig();
        let b = owner.current().unwrap().basis;
        push(&io, snapshot(b, 0, 100));
        if epoch {
            io.mutate_epoch.set(true);
        } else {
            io.mutate_basis.set(true);
        }
        let before = format!("{p:?}");
        assert!(owner.observe_target_native_coordinate(&mut p).is_err());
        assert_eq!(format!("{p:?}"), before);
        assert!(owner.current().unwrap().owner.is_some());
    }
}
#[test]
#[cfg(target_os = "linux")]
fn existing_converted_alsa_backend_exposes_original_target_coordinate_port() {
    fn assert_port<B: OriginalCoordinateTargetNativeOutputBackend<ConvertedNativeOutputState>>() {}
    assert_port::<crate::gameplay::output::adapters::alsa::ConvertedAlsaReplacementBackend>();
}
