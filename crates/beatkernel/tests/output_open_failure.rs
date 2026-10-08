//! Deferred pending-owner retirement policy; probe flags are not native fences.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use std::{cell::Cell, rc::Rc};
struct Opaque(Box<u64>); // No Clone, Debug, Display or Error bound.
struct Owner {
    mixer: Option<Mixer>,
    id: Box<u64>,
    retired: bool,
    drops: Rc<Cell<usize>>,
    unretired_drops: Rc<Cell<usize>>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        if !self.retired {
            self.unretired_drops.set(self.unretired_drops.get() + 1);
        }
    }
}
fn rig() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(4, 1).unwrap();
    let limits = PcmLimits::new(64, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(u64::MAX),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(u32::MAX),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.; 3]).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(7),
            sample: SampleId(1),
            at: Timestamp::from_nanos(1_500_000_000),
            gain: 1.,
        })
        .unwrap();
    (producer, mixer)
}
fn owner(mixer: Option<Mixer>) -> (Owner, Rc<Cell<usize>>, Rc<Cell<usize>>) {
    let drops = Rc::new(Cell::new(0));
    let unretired = Rc::new(Cell::new(0));
    (
        Owner {
            mixer,
            id: Box::new(71),
            retired: false,
            drops: drops.clone(),
            unretired_drops: unretired.clone(),
        },
        drops,
        unretired,
    )
}
fn pcm(mut mixer: Mixer, producer: &mut CommandProducer) {
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.is_paused()
        ),
        (5, 2, true)
    );
    producer.request_pause(false);
    let mut samples = [0.; 6];
    mixer.render(&mut samples).unwrap();
    assert_eq!(samples, [0.75, 1., 0., 0., 0.25, 0.5]);
}
#[test]
fn repeated_refusals_keep_same_pending_owner_and_original_error_then_retirement_moves_original_pcm_once(
) {
    let (mut producer, mixer) = rig();
    let basis = mixer.output_frame_basis();
    let (owner, drops, unretired) = owner(Some(mixer));
    let owner_id = owner.id.as_ref() as *const u64;
    let original = Opaque(Box::new(1));
    let original_id = original.0.as_ref() as *const u64;
    let mut failure = OutputOpenFailure::pending(original, owner);
    assert!(failure.mixer().is_none());
    assert!(failure.cleanup_error().is_none());
    for value in [2, 3] {
        let cleanup = Opaque(Box::new(value));
        let cleanup_id = cleanup.0.as_ref() as *const u64;
        let result = failure.retry_retirement(|pending| {
            assert_eq!(pending.id.as_ref() as *const u64, owner_id);
            assert_eq!(pending.mixer.as_ref().unwrap().output_frame_basis(), basis);
            Err(cleanup)
        });
        match result {
            Err(error) => assert_eq!(error.0.as_ref() as *const u64, cleanup_id),
            Ok(_) => panic!("cleanup refusal must remain visible"),
        }
        assert_eq!(failure.error().0.as_ref() as *const u64, original_id);
        assert_eq!(
            failure.cleanup_error().unwrap().0.as_ref() as *const u64,
            cleanup_id
        );
        assert_eq!(
            failure.pending_owner().unwrap().id.as_ref() as *const u64,
            owner_id
        );
        assert!(failure.mixer().is_none());
        assert_eq!(drops.get(), 0);
    }
    match failure.retry_retirement(|pending| {
        pending.retired = true;
        Ok(pending.mixer.take())
    }) {
        Ok(true) => {}
        _ => panic!("successful retirement must commit"),
    }
    assert_eq!(drops.get(), 1);
    assert_eq!(unretired.get(), 0);
    assert!(failure.pending_owner().is_none());
    assert!(failure.cleanup_error().is_none());
    assert_eq!(failure.mixer().unwrap().output_frame_basis(), basis);
    match failure.retry_retirement(|_| panic!("already retired must not invoke callback")) {
        Ok(false) => {}
        _ => panic!("retired retry must be noop"),
    }
    let (error, mixer, pending, cleanup) = failure.into_parts();
    assert_eq!(error.0.as_ref() as *const u64, original_id);
    assert!(pending.is_none());
    assert!(cleanup.is_none());
    pcm(mixer.unwrap(), &mut producer);
}
#[test]
fn into_parts_transfers_unretired_owner_and_latest_cleanup_without_premature_drop_or_mixer_publication(
) {
    let (mut producer, mixer) = rig();
    let (owner, drops, unretired) = owner(Some(mixer));
    let owner_id = owner.id.as_ref() as *const u64;
    let original = Opaque(Box::new(5));
    let original_id = original.0.as_ref() as *const u64;
    let cleanup = Opaque(Box::new(6));
    let cleanup_id = cleanup.0.as_ref() as *const u64;
    let mut failure = OutputOpenFailure::pending(original, owner);
    assert!(failure.retry_retirement(|_| Err(cleanup)).is_err());
    assert_eq!(drops.get(), 0);
    let (error, mixer, pending, cleanup) = failure.into_parts();
    assert!(mixer.is_none());
    assert_eq!(error.0.as_ref() as *const u64, original_id);
    assert_eq!(cleanup.unwrap().0.as_ref() as *const u64, cleanup_id);
    let mut pending = pending.unwrap();
    assert_eq!(pending.id.as_ref() as *const u64, owner_id);
    assert!(!pending.retired);
    assert_eq!(drops.get(), 0);
    // The recipient owns the unresolved retirement obligation, not a Mixer.
    pending.retired = true;
    let mixer = pending.mixer.take().unwrap();
    drop(pending);
    assert_eq!(drops.get(), 1);
    assert_eq!(unretired.get(), 0);
    pcm(mixer, &mut producer);
}
#[test]
fn recovered_some_or_none_has_no_pending_retry_and_never_fabricates_a_mixer() {
    for available in [false, true] {
        let (mut producer, mixer) = rig();
        let original = Opaque(Box::new(9));
        let pointer = original.0.as_ref() as *const u64;
        let mut failure: OutputOpenFailure<Opaque, Owner> =
            OutputOpenFailure::recovered(original, available.then_some(mixer));
        assert_eq!(failure.mixer().is_some(), available);
        assert!(failure.pending_owner().is_none());
        assert!(failure.cleanup_error().is_none());
        match failure.retry_retirement(|_| panic!("recovered owner must not invoke callback")) {
            Ok(false) => {}
            _ => panic!("recovered retry must be noop"),
        }
        let (error, mixer, pending, cleanup) = failure.into_parts();
        assert_eq!(error.0.as_ref() as *const u64, pointer);
        assert_eq!(mixer.is_some(), available);
        assert!(pending.is_none());
        assert!(cleanup.is_none());
        if let Some(mixer) = mixer {
            pcm(mixer, &mut producer);
        }
    }
}
#[test]
fn successful_retirement_without_recoverable_mixer_drops_owner_only_after_proof_and_clears_cleanup()
{
    let (owner, drops, unretired) = owner(None);
    let mut failure = OutputOpenFailure::pending(Opaque(Box::new(11)), owner);
    assert!(failure
        .retry_retirement(|_| Err(Opaque(Box::new(12))))
        .is_err());
    assert_eq!(drops.get(), 0);
    match failure.retry_retirement(|pending| {
        pending.retired = true;
        Ok(None)
    }) {
        Ok(true) => {}
        _ => panic!("retirement is successful even when ownership unavailable"),
    }
    assert_eq!(drops.get(), 1);
    assert_eq!(unretired.get(), 0);
    assert!(failure.mixer().is_none());
    assert!(failure.pending_owner().is_none());
    assert!(failure.cleanup_error().is_none());
    match failure.retry_retirement(|_| panic!("retirement must be one shot")) {
        Ok(false) => {}
        _ => panic!("no pending owner expected"),
    }
}

#[test]
fn complete_recovered_owner_keeps_pending_pcm_and_offset_through_cleanup_refusal() {
    struct Complete {
        mixer: Mixer,
        converter: FormatConverter,
        pending: Box<[f32]>,
        admitted: usize,
    }
    struct Pending {
        complete: Option<Complete>,
        retired: bool,
    }
    let (mut producer, mixer) = rig();
    let converter = FormatConverter::for_mixer(
        mixer.config(),
        AudioFormat::new(4, 2).unwrap(),
        ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap(),
        ResampleQuality::Linear,
        8,
    )
    .unwrap();
    let pending = vec![0.75, -0.375, 1., -0.5].into_boxed_slice();
    let pcm_pointer = pending.as_ptr();
    let original = Opaque(Box::new(37));
    let error_pointer = original.0.as_ref() as *const u64;
    let mut failure: OutputOpenFailure<Opaque, Pending, Complete> =
        OutputOpenFailure::pending_state(
            original,
            Pending {
                complete: Some(Complete {
                    mixer,
                    converter,
                    pending,
                    admitted: 1,
                }),
                retired: false,
            },
        );
    assert!(failure
        .retry_retirement(|owner| {
            assert!(!owner.retired);
            assert_eq!(
                owner.complete.as_ref().unwrap().pending.as_ptr(),
                pcm_pointer
            );
            Err(Opaque(Box::new(41)))
        })
        .is_err());
    assert!(failure.mixer().is_none());
    assert_eq!(failure.cleanup_error().unwrap().0.as_ref(), &41);
    assert_eq!(failure.error().0.as_ref() as *const u64, error_pointer);
    assert!(matches!(
        failure.retry_retirement(|owner| {
            owner.retired = true;
            Ok(owner.complete.take())
        }),
        Ok(true)
    ));
    assert!(failure.pending_owner().is_none());
    assert!(failure.cleanup_error().is_none());
    assert!(matches!(
        failure.retry_retirement(|_| panic!("complete state must move once")),
        Ok(false)
    ));
    let (error, complete, pending, cleanup) = failure.into_parts();
    assert_eq!(error.0.as_ref() as *const u64, error_pointer);
    assert!(pending.is_none());
    assert!(cleanup.is_none());
    let mut complete = complete.unwrap();
    assert_eq!(complete.pending.as_ptr(), pcm_pointer);
    assert_eq!(&complete.pending[complete.admitted * 2..], [1., -0.5]);
    assert_eq!(complete.converter.output_frame_cursor(), 0);
    producer.request_pause(false);
    let mut output = [0.; 12];
    complete
        .converter
        .render(&mut output, |source| complete.mixer.render(source))
        .unwrap();
    assert_eq!(
        output,
        [0.75, -0.375, 1., -0.5, 0., 0., 0., 0., 0.25, -0.125, 0.5, -0.25]
    );
}
