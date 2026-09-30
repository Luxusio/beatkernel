//! Offline audio setup and fixed-size real-time scheduling data.
//!
//! Scheduling timestamps belong to the caller-selected output clock domain,
//! not automatically to song time or a host clock. Native output belongs to
//! `beatkernel-platform`. Scalar commands contain no owning audio assets.

mod model;
mod pcm;

pub use model::{
    AudioCommand, AudioCounters, AudioError, AudioFormat, AudioLimits, MixerConfig, PcmLimits,
    RenderReport, SampleId, VoiceId,
};

pub use pcm::{PcmSample, SampleBank, WavError};
