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

mod model;
mod pcm;
mod queue;

pub use model::{
    AudioCommand, AudioCounters, AudioError, AudioFormat, AudioLimits, MixerConfig, PcmLimits,
    RenderReport, SampleId, VoiceId,
};

pub use pcm::{PcmSample, SampleBank, WavError};
pub use queue::{
    command_queue, CommandConsumer, CommandProducer, CommandPushError, QueueCounters,
    QueuePopError, QueuePushError,
};
