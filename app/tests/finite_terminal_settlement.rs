//! Public consumer evidence: expiry means retirement, never execution.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_bms_runtime::replay_audio::completed_render_cursor;

fn terminal() -> (CommandProducer, Mixer, RenderReport) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(32, 32, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 8], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(22),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        )
        .with_playback_end_frame(1),
        bank,
        consumer,
    )
    .unwrap();
    let marker = mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!(marker.counters.commands_consumed, 0);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    (producer, mixer, marker)
}

#[test]
fn actual_terminal_expiry_is_valid_render_cursor_without_execution_credit() {
    let (_producer, mut mixer, marker) = terminal();
    assert_eq!(completed_render_cursor(&marker).unwrap(), 2);
    let mut pcm = [1.0; 2];
    let report = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0; 2]);
    assert_eq!(report.counters.commands_consumed, 1);
    assert_eq!(report.counters.commands_applied, 0);
    assert_eq!(report.counters.commands_expired, 1);
    assert_eq!(completed_render_cursor(&report).unwrap(), 4);
}

#[test]
fn public_render_cursor_rejects_expiry_without_marker_or_checked_exact_counters() {
    let (_producer, mut mixer, _) = terminal();
    let actual = mixer.render(&mut [0.0; 2]).unwrap();
    for case in 0..6 {
        let mut forged = actual;
        match case {
            0 => forged.playback_end_physical_frame = None,
            1 => forged.paused = false,
            2 => forged.counters.commands_expired += 1,
            3 => forged.counters.commands_applied = 1,
            4 => forged.counters.commands_consumed = u64::MAX,
            5 => forged.counters.commands_expired = u64::MAX,
            _ => unreachable!(),
        }
        assert!(completed_render_cursor(&forged).is_err(), "case {case}");
    }
}
