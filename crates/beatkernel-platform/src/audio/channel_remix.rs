//! Shared same-rate native channel policy; no native calls or handle ownership.
use beatkernel::audio::{
    AudioError, AudioFormat, ChannelMatrix, FormatConverter, Mixer, MixerConfig, RenderReport,
    ResampleQuality,
};

pub(crate) fn validate(
    source: AudioFormat,
    target: AudioFormat,
    matrix: &ChannelMatrix,
) -> Result<(), AudioError> {
    if source.sample_rate() != target.sample_rate()
        || matrix.source_channels() != source.channels()
        || matrix.target_channels() != target.channels()
    {
        return Err(AudioError::InvalidFormat);
    }
    Ok(())
}

pub(crate) fn prepare(
    config: MixerConfig,
    target: AudioFormat,
    matrix: ChannelMatrix,
    max_frames: usize,
) -> Result<FormatConverter, AudioError> {
    validate(config.format(), target, &matrix)?;
    FormatConverter::for_mixer(config, target, matrix, ResampleQuality::Linear, max_frames)
}

// Native setup owns the immutable source Mixer config and prepared converter.
pub(crate) fn render(
    mixer: &mut Mixer,
    remix: &mut Option<FormatConverter>,
    output: &mut [f32],
) -> Result<RenderReport, AudioError> {
    match remix {
        Some(converter) => converter.render(output, |source| mixer.render(source)),
        None => mixer.render(output),
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::audio::{DeviceFormat, SampleEncoding, encode_pcm};
    use beatkernel::{
        audio::{
            AudioCommand, AudioLimits, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId,
            command_queue,
        },
        time::{ClockDomainId, Timestamp},
    };

    #[test]
    fn common_native_path_preserves_report_and_encodes_exact_target_channel_bytes() {
        let source = AudioFormat::new(48_000, 1).unwrap();
        let target = DeviceFormat::new(
            48_000,
            2,
            SampleEncoding::Pcm {
                container_bits: 16,
                valid_bits: 16,
            },
            None,
        )
        .unwrap();
        let limits = PcmLimits::new(64, 128, 1).unwrap();
        let mut bank = SampleBank::new(source, limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(source, vec![0.25, -0.5], limits).unwrap(),
        )
        .unwrap();
        let (mut producer, consumer) = command_queue(4).unwrap();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.,
            })
            .unwrap();
        let config = MixerConfig::new(
            source,
            ClockDomainId(1),
            Timestamp::ZERO,
            AudioLimits::new(4, 1, 4, 2, 4).unwrap(),
        );
        let mut mixer = Mixer::new(config, bank, consumer).unwrap();
        let mut remix = Some(
            prepare(
                config,
                target.pcm(),
                ChannelMatrix::new(1, 2, &[1., 0.5]).unwrap(),
                2,
            )
            .unwrap(),
        );
        let mut output = [0.; 4];
        let report = render(&mut mixer, &mut remix, &mut output).unwrap();
        assert_eq!(
            (report.start_frame, report.frames, report.playback_frames),
            (0, 2, 2)
        );
        assert_eq!(output, [0.25, 0.125, -0.5, -0.25]);
        let mut encoded = [0; 8];
        encode_pcm(target, &output, &mut encoded).unwrap();
        assert_eq!(encoded, [0, 32, 0, 16, 0, 192, 0, 224]);
        producer.request_pause(true);
        let paused = render(&mut mixer, &mut remix, &mut output).unwrap();
        assert!(paused.paused);
        assert_eq!((paused.start_frame, paused.playback_frames), (2, 0));
        assert_eq!(output, [0.; 4]);
    }

    #[test]
    fn common_preflight_rejects_rate_dimensions_and_applied_capacity() {
        let source = AudioFormat::new(48_000, 1).unwrap();
        let matrix = ChannelMatrix::default_mix(1, 2).unwrap();
        for target in [
            AudioFormat::new(44_100, 2).unwrap(),
            AudioFormat::new(48_000, 1).unwrap(),
        ] {
            assert_eq!(
                validate(source, target, &matrix),
                Err(AudioError::InvalidFormat)
            );
        }
        let config = MixerConfig::new(
            source,
            ClockDomainId(1),
            Timestamp::ZERO,
            AudioLimits::new(4, 1, 4, 2, 4).unwrap(),
        );
        assert_eq!(
            prepare(config, AudioFormat::new(48_000, 2).unwrap(), matrix, 3).unwrap_err(),
            AudioError::RenderCapacity
        );
    }
}
