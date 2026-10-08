//! Platform-neutral real converter rigs and controlled original port evidence.
//! Controlled pairs exercise application policy; they do not prove a device clock.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPair, ClockPoint, Timestamp},
};
use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};

pub(crate) fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
pub(crate) fn rig(
    source_rate: u32,
    target_rate: u32,
    gate: Option<u64>,
    end: Option<u64>,
    origin: i64,
) -> (CommandProducer, ConvertedNativeOutputState) {
    rig_gate(source_rate, target_rate, gate.map(Some), end, origin)
}
pub(crate) fn rig_gate(
    source_rate: u32,
    target_rate: u32,
    gate: Option<Option<u64>>,
    end: Option<u64>,
    origin: i64,
) -> (CommandProducer, ConvertedNativeOutputState) {
    let format = AudioFormat::new(source_rate, 1).unwrap();
    let limits = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 512], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = if gate.is_some() {
        command_queue_with_start_gate(16).unwrap()
    } else {
        command_queue(16).unwrap()
    };
    if let Some(Some(gate)) = gate {
        producer.schedule_start_at(gate).unwrap();
    }
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(origin),
            gain: 1.0,
        })
        .unwrap();
    let mut config = MixerConfig::new(
        format,
        ClockDomainId(2),
        Timestamp::from_nanos(origin),
        AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    let mixer = Mixer::new(config, bank, consumer).unwrap();
    let device = DeviceFormat::new(target_rate, 1, SampleEncoding::Float32, None).unwrap();
    let owner = ConvertedNativeOutputState::new(
        mixer,
        device,
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        128,
    )
    .unwrap_or_else(|_| panic!("valid exact converted fixture owner"));
    (producer, owner)
}
/// Supplies a controlled native-port pair at an exact physical target coordinate.
/// Linux association fixtures separately validate original ALSA snapshots.
pub(crate) fn controlled_pair(basis: TargetFrameBasis, played: u64, host_ns: i64) -> ClockPair {
    ClockPair {
        source: basis.point_at_stream_frame(played).unwrap(),
        target: point(1, host_ns),
    }
}
