//! Offline audio setup and fixed-size real-time scheduling data.
//!
//! Scheduling timestamps belong to the caller-selected output clock domain,
//! not automatically to song time or a host clock. Native output belongs to
//! `beatkernel-platform`. Scalar commands contain no owning audio assets.
//!
//! ```
//! use beatkernel::{audio::{command_queue, AudioCommand, VoiceId}, time::Timestamp};
//! let (mut producer, mut consumer) = command_queue(2)?;
//! let command = AudioCommand::Stop { voice: VoiceId(1), at: Timestamp::ZERO };
//! producer.try_push(command).unwrap();
//! assert_eq!(consumer.try_pop().unwrap(), command);
//! # Ok::<(), beatkernel::audio::AudioError>(())
//! ```
//!
//! Load samples and allocate mixer storage before entering a callback:
//!
//! ```
//! use beatkernel::{
//!     audio::{command_queue, AudioCommand, AudioFormat, AudioLimits, Mixer,
//!         MixerConfig, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId},
//!     time::{ClockDomainId, Timestamp},
//! };
//! let format = AudioFormat::new(48_000, 1)?;
//! let pcm_limits = PcmLimits::new(1024, 4096, 4)?;
//! let mut bank = SampleBank::new(format, pcm_limits)?;
//! bank.insert(SampleId(1), PcmSample::new(format, vec![0.25, 0.5], pcm_limits)?)?;
//! let limits = AudioLimits::new(4, 4, 4, 128, 4)?;
//! let config = MixerConfig::new(format, ClockDomainId(1), Timestamp::ZERO, limits);
//! let (mut producer, consumer) = command_queue(4)?;
//! let mut mixer = Mixer::new(config, bank, consumer)?;
//! producer.try_push(AudioCommand::Play {
//!     voice: VoiceId(1), sample: SampleId(1), at: Timestamp::ZERO, gain: 1.0,
//! }).unwrap();
//! let mut output = [0.0; 3];
//! let report = mixer.render(&mut output)?;
//! assert_eq!(output, [0.25, 0.5, 0.0]);
//! assert_eq!(report.frames, 3);
//! # Ok::<(), beatkernel::audio::AudioError>(())
//! ```

mod frame_basis;
mod handoff;
mod mixer;
mod model;
mod pcm;
mod queue;

pub use frame_basis::{OutputFrameBasis, OutputFrameBasisError};
pub use handoff::{MixerOpenFailure, StoppedMixerSource};
pub use mixer::Mixer;
pub use model::{
    AudioCommand, AudioCounters, AudioError, AudioFormat, AudioLimits, MixerConfig, PcmLimits,
    RenderReport, SampleId, VoiceId,
};

pub use pcm::{PcmSample, SampleBank, WavError};
pub use queue::{
    CommandConsumer, CommandProducer, CommandPushError, QueueCounters, QueuePopError,
    QueuePushError, command_queue, command_queue_with_start_gate,
};
