//! Deferred original-open and cleanup diagnostics have separate ownership.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use std::{cell::Cell, rc::Rc};
struct Opaque(Box<u64>);
struct Owner {
    mixer: Option<Mixer>,
    id: Box<u64>,
    drops: Rc<Cell<usize>>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}
fn mixer() -> Mixer {
    let format = AudioFormat::new(4, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 128, 1).unwrap()).unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(u32::MAX),
            Timestamp::from_nanos(-123),
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    mixer
}
#[test]
fn attaching_cleanup_keeps_pending_owner_and_original_error_then_retry_replaces_only_diagnostic() {
    let drops = Rc::new(Cell::new(0));
    let mixer = mixer();
    let basis = mixer.output_frame_basis();
    let owner = Owner {
        mixer: Some(mixer),
        id: Box::new(71),
        drops: drops.clone(),
    };
    let owner_pointer = owner.id.as_ref() as *const u64;
    let original = Opaque(Box::new(1));
    let original_pointer = original.0.as_ref() as *const u64;
    let cleanup = Opaque(Box::new(2));
    let cleanup_pointer = cleanup.0.as_ref() as *const u64;
    let mut failure = OutputOpenFailure::pending(original, owner).with_cleanup_error(cleanup);
    assert_eq!(failure.error().0.as_ref() as *const u64, original_pointer);
    assert_eq!(
        failure.cleanup_error().unwrap().0.as_ref() as *const u64,
        cleanup_pointer
    );
    assert_eq!(
        failure.pending_owner().unwrap().id.as_ref() as *const u64,
        owner_pointer
    );
    assert!(failure.mixer().is_none());
    assert_eq!(drops.get(), 0);
    let later = Opaque(Box::new(3));
    let later_pointer = later.0.as_ref() as *const u64;
    assert!(
        failure
            .retry_retirement(|owner| {
                assert_eq!(owner.mixer.as_ref().unwrap().output_frame_basis(), basis);
                Err(later)
            })
            .is_err()
    );
    assert_eq!(
        failure.cleanup_error().unwrap().0.as_ref() as *const u64,
        later_pointer
    );
    assert_eq!(failure.error().0.as_ref() as *const u64, original_pointer);
    assert_eq!(drops.get(), 0);
    match failure.retry_retirement(|owner| Ok(owner.mixer.take())) {
        Ok(true) => {}
        _ => panic!("successful retirement required"),
    }
    assert_eq!(drops.get(), 1);
    assert!(failure.cleanup_error().is_none());
    assert!(failure.pending_owner().is_none());
    assert_eq!(failure.mixer().unwrap().output_frame_basis(), basis);
}
#[test]
fn attaching_cleanup_to_recovered_mixer_survives_noop_retry_and_moves_both_original_payloads() {
    let mixer = mixer();
    let basis = mixer.output_frame_basis();
    let original = Opaque(Box::new(5));
    let original_pointer = original.0.as_ref() as *const u64;
    let cleanup = Opaque(Box::new(6));
    let cleanup_pointer = cleanup.0.as_ref() as *const u64;
    let mut failure: OutputOpenFailure<Opaque, Owner> =
        OutputOpenFailure::recovered(original, Some(mixer)).with_cleanup_error(cleanup);
    match failure.retry_retirement(|_| panic!("no owner must not invoke cleanup")) {
        Ok(false) => {}
        _ => panic!("no-op required"),
    }
    assert_eq!(failure.mixer().unwrap().output_frame_basis(), basis);
    assert_eq!(
        failure.cleanup_error().unwrap().0.as_ref() as *const u64,
        cleanup_pointer
    );
    let (original, mixer, owner, cleanup) = failure.into_parts();
    assert_eq!(original.0.as_ref() as *const u64, original_pointer);
    assert_eq!(cleanup.unwrap().0.as_ref() as *const u64, cleanup_pointer);
    assert!(owner.is_none());
    assert_eq!(mixer.unwrap().output_frame_basis(), basis);
}
#[test]
fn attaching_cleanup_without_model_or_owner_never_invents_recovery_and_can_replace_diagnostic() {
    let original = Opaque(Box::new(9));
    let original_pointer = original.0.as_ref() as *const u64;
    let replacement = Opaque(Box::new(u64::MAX));
    let replacement_pointer = replacement.0.as_ref() as *const u64;
    let mut failure: OutputOpenFailure<Opaque, Owner> =
        OutputOpenFailure::recovered(original, None)
            .with_cleanup_error(Opaque(Box::new(10)))
            .with_cleanup_error(replacement);
    assert!(failure.mixer().is_none());
    assert!(failure.pending_owner().is_none());
    match failure.retry_retirement(|_| panic!("absent owner must stay absent")) {
        Ok(false) => {}
        _ => panic!("no-op required"),
    }
    let (error, mixer, owner, cleanup) = failure.into_parts();
    assert_eq!(error.0.as_ref() as *const u64, original_pointer);
    assert!(mixer.is_none());
    assert!(owner.is_none());
    assert_eq!(
        cleanup.unwrap().0.as_ref() as *const u64,
        replacement_pointer
    );
}
