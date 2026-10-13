//! Static conversion between pure settings and exact native sample encodings.
use crate::settings::OutputEncoding;
use beatkernel_platform::audio::SampleEncoding;

pub fn native_encoding(choice: OutputEncoding) -> SampleEncoding {
    match choice {
        OutputEncoding::Float32 => SampleEncoding::Float32,
        OutputEncoding::Pcm16 => SampleEncoding::Pcm {
            container_bits: 16,
            valid_bits: 16,
        },
        OutputEncoding::Pcm24 => SampleEncoding::Pcm {
            container_bits: 24,
            valid_bits: 24,
        },
        OutputEncoding::Pcm32 => SampleEncoding::Pcm {
            container_bits: 32,
            valid_bits: 32,
        },
    }
}

pub fn encoding_choice(encoding: SampleEncoding) -> Result<OutputEncoding, String> {
    match encoding {
        SampleEncoding::Float32 => Ok(OutputEncoding::Float32),
        SampleEncoding::Pcm {
            container_bits: 16,
            valid_bits: 16,
        } => Ok(OutputEncoding::Pcm16),
        SampleEncoding::Pcm {
            container_bits: 24,
            valid_bits: 24,
        } => Ok(OutputEncoding::Pcm24),
        SampleEncoding::Pcm {
            container_bits: 32,
            valid_bits: 32,
        } => Ok(OutputEncoding::Pcm32),
        SampleEncoding::Pcm { .. } => {
            Err("native PCM encoding requires a full signed 16, 24 or 32-bit sample".into())
        }
    }
}
