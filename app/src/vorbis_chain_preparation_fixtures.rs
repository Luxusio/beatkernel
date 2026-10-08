//! Real default preparation and scheduled Mixer output from authored link trims.
use crate::{
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    prepare_from_source,
    vorbis_fixture::{link, packed_audio, pages, reseal_page, silence},
    ChannelPolicy, DefaultAssetDecoder, PreparedBms,
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits,
        SampleId,
    },
    time::{ClockDomainId, Timestamp},
};
use std::error::Error;

const CHART: &str = "#BPM 24000\n#WAV01 keys.WAV\n#WAV02 marker.wav\n#00001:0102\n#00011:01\n";

fn marker() -> Vec<u8> {
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&44u32.to_le_bytes()); // 36-byte RIFF body + four i16 frames.
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&24_000u32.to_le_bytes());
    bytes.extend_from_slice(&48_000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&8u32.to_le_bytes());
    for value in [16_384i16, -16_384, 8_192, -8_192] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn chain() -> Vec<u8> {
    [
        link(silence(1, 1), 101, 24_000),
        link(packed_audio(1, 48), 202, 24_000),
        link(silence(1, 48), 303, 24_000),
    ]
    .concat()
}

fn prepare(
    encoded: Vec<u8>,
    channels: u16,
    limits: PcmLimits,
) -> Result<PreparedBms, Box<dyn Error>> {
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("pack/chart.bms", CHART.as_bytes().to_vec())
        .unwrap();
    files.insert("pack/keys.OGG", encoded).unwrap();
    files.insert("pack/marker.wav", marker()).unwrap();
    let source = files.scope("pack/chart.bms").unwrap();
    prepare_from_source(
        CHART.as_bytes(),
        &source,
        AudioFormat::new(24_000, channels).unwrap(),
        limits,
        if channels == 1 {
            ChannelPolicy::Exact
        } else {
            ChannelPolicy::MonoToStereo
        },
        &DefaultAssetDecoder,
        AssetPathPolicy::AudioVariants,
        0,
        None,
    )
}

#[test]
fn default_preparation_preserves_chart_sounds_and_mixer_runs_the_entire_chain_tail() {
    // 1 + 48 + 48 = 97 source frames. At BPM24000 one measure is10ms,
    // so the second of two BGM slots begins at5ms, exactly output frame120.
    for channels in [1u16, 2] {
        let prepared = prepare(chain(), channels, PcmLimits::new(1024, 4096, 4).unwrap()).unwrap();
        assert_eq!(prepared.source.samples.get(&1).unwrap(), "keys.WAV");
        assert_eq!(prepared.bank.len(), 2);
        let chain_pcm = prepared.bank.get(SampleId(1)).unwrap();
        assert_eq!(chain_pcm.frames(), 97);
        assert_eq!(
            chain_pcm.format(),
            AudioFormat::new(24_000, channels).unwrap()
        );
        assert_eq!(
            chain_pcm.samples(),
            vec![0.0f32; 97 * usize::from(channels)]
        );
        assert_eq!(prepared.sounds.len(), 1);
        assert_eq!(prepared.sounds[0].sample, SampleId(1));
        assert_eq!(prepared.bgm_commands.len(), 2);
        for (command, sample, nanos) in [
            (prepared.bgm_commands[0], SampleId(1), 0),
            (prepared.bgm_commands[1], SampleId(2), 5_000_000),
        ] {
            let AudioCommand::Play {
                sample: actual,
                at,
                gain,
                ..
            } = command
            else {
                panic!("expected original BGM play")
            };
            assert_eq!(actual, sample);
            assert_eq!(at, Timestamp::from_nanos(nanos));
            assert_eq!(gain, 1.0);
        }
        let (mut producer, consumer) = command_queue(8).unwrap();
        for command in prepared.bgm_commands {
            producer.try_push(command).unwrap();
        }
        let mut mixer = Mixer::new(
            MixerConfig::new(
                prepared.bank.format(),
                ClockDomainId(17),
                Timestamp::ZERO,
                AudioLimits::new(8, 4, 8, 128, 8).unwrap(),
            ),
            prepared.bank,
            consumer,
        )
        .unwrap();
        let mut prefix = vec![1.0; 96 * usize::from(channels)];
        let before_tail = mixer.render(&mut prefix).unwrap();
        assert!(prefix.iter().all(|&sample| sample == 0.0));
        assert_eq!(
            before_tail.active_voices, 1,
            "third link still owns the last frame"
        );
        assert_eq!(before_tail.counters.commands_applied, 1);
        assert_eq!(before_tail.pending_commands, 1);
        let mut tail = vec![1.0; usize::from(channels)];
        let tail_end = mixer.render(&mut tail).unwrap();
        assert_eq!(tail, vec![0.0; usize::from(channels)]);
        assert_eq!(tail_end.start_frame, 96);
        assert_eq!(
            tail_end.active_voices, 0,
            "independent trims end exactly at97"
        );
        let mut remainder = vec![1.0; 28 * usize::from(channels)];
        let end = mixer.render(&mut remainder).unwrap();
        let marker_pcm = [0.5f32, -0.5, 0.25, -0.25];
        let expected: Vec<_> = (97..125)
            .flat_map(|frame| {
                let value = if (120..124).contains(&frame) {
                    marker_pcm[frame - 120]
                } else {
                    0.0
                };
                std::iter::repeat_n(value, usize::from(channels))
            })
            .collect();
        assert_eq!(
            remainder, expected,
            "actual scheduled nonzero PCM anchors the source timeline"
        );
        assert_eq!(end.start_frame, 97);
        assert_eq!(end.frames, 28);
        assert_eq!(end.active_voices, 0);
        assert_eq!(end.pending_commands, 0);
        assert_eq!(end.counters.commands_applied, 2);
        assert_eq!(end.counters.late_commands, 0);
    }
}

#[test]
fn default_preparation_rejects_aggregate_pcm_caps_and_malformed_later_link() {
    // Each link independently fits192 bytes; the combined mono asset needs388.
    assert!(prepare(chain(), 1, PcmLimits::new(192, 4096, 4).unwrap()).is_err());
    // Combined assets need388 + 16 =404 bytes, not only the Ogg asset's cap.
    assert!(prepare(chain(), 1, PcmLimits::new(388, 403, 4).unwrap()).is_err());
    assert!(prepare(chain(), 1, PcmLimits::new(388, 404, 4).unwrap()).is_ok());
    let mut damaged = chain();
    let later_bos = pages(&damaged)
        .into_iter()
        .find(|&(offset, _)| {
            u32::from_le_bytes(damaged[offset + 14..offset + 18].try_into().unwrap()) == 303
        })
        .unwrap()
        .0;
    damaged[later_bos + 28] = 0;
    reseal_page(&mut damaged, later_bos);
    assert!(prepare(damaged, 1, PcmLimits::new(1024, 4096, 4).unwrap()).is_err());
}
