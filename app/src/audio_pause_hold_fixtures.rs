//! Deferred actual static Runtime/Group/Solo producer hold delegation.
use crate::gameplay_presentation_port_fixtures::*;
use crate::{
    local_players::PlayerId,
    local_runtime::{MemberConfig, RuntimeGroup, SoloRuntime},
};
fn play() -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(u64::MAX),
        sample: SampleId(1),
        at: Timestamp::ZERO,
        gain: 1.,
    }
}
fn held_then_resume(mixer: &mut Mixer, hold: PauseHold, resume: impl FnOnce()) {
    let mut pcm = [0.; 4];
    let report = mixer.render(&mut pcm).unwrap();
    assert!(report.paused);
    assert_eq!(pcm, [0.; 4]);
    assert_eq!(mixer.playback_frame_cursor(), 0);
    drop(hold);
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.; 4]);
    assert_eq!(mixer.playback_frame_cursor(), 0);
    resume();
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.5, 0., 0.]);
    assert_eq!(mixer.playback_frame_cursor(), 4);
}
#[test]
fn core_runtime_and_solo_wrapper_hold_the_same_original_producer_while_resume_requests_are_ignored()
{
    let source = source();
    for solo in [false, true] {
        let (mut output, producer) = device(false, vec![DeviceId(u64::MAX)]);
        if solo {
            let mut runtime = SoloRuntime::new(
                ClockDomainId(1),
                ClockDomainId(2),
                Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
                bindings(None),
                judge(&source),
                producer,
                vec![],
                8,
            )
            .unwrap();
            runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
            runtime.enqueue_audio(play()).unwrap();
            let hold = match runtime.hold_audio_pause() {
                Ok(hold) => hold,
                Err(_) => panic!("solo hold required"),
            };
            assert!(runtime.hold_audio_pause().is_err());
            runtime.request_audio_pause(false);
            held_then_resume(&mut output.mixer, hold, || {
                runtime.request_audio_pause(false)
            });
        } else {
            let mut runtime = beatkernel::runtime::Runtime::new(
                ClockDomainId(1),
                ClockDomainId(2),
                Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
                bindings(None),
                judge(&source),
                producer,
                vec![],
                8,
            )
            .unwrap();
            runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
            runtime.enqueue_audio(play()).unwrap();
            let hold = match runtime.hold_audio_pause() {
                Ok(hold) => hold,
                Err(_) => panic!("core runtime hold required"),
            };
            runtime.request_audio_pause(false);
            held_then_resume(&mut output.mixer, hold, || {
                runtime.request_audio_pause(false)
            });
        }
    }
}
#[test]
fn actual_local_cohort_hold_pins_shared_mixer_for_one_two_and_sixty_four_original_players() {
    let source = source();
    for count in [1, 2, 64] {
        let (mut output, producer) = device(false, vec![]);
        let ids = (0..count)
            .map(|i| PlayerId(u32::MAX - i * 7))
            .collect::<Vec<_>>();
        let members = ids
            .iter()
            .map(|&player| {
                let device = DeviceId(u64::MAX - u64::from(player.0));
                MemberConfig {
                    player,
                    device: Some(device),
                    bindings: bindings(Some(device)),
                    judge: judge(&source),
                    sounds: vec![],
                }
            })
            .collect();
        let mut group = RuntimeGroup::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            producer,
            members,
            8,
            &[],
        )
        .unwrap();
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        group.enqueue_audio(play()).unwrap();
        let hold = match group.hold_audio_pause() {
            Ok(hold) => hold,
            Err(_) => panic!("group hold required"),
        };
        assert!(group.hold_audio_pause().is_err());
        group.request_audio_pause(false);
        held_then_resume(&mut output.mixer, hold, || group.request_audio_pause(false));
    }
}
