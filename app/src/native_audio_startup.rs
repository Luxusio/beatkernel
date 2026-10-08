//! Bounded original-observation startup shared by native launchers.
use crate::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    bgm::BgmFeeder,
    native_audio::feed_rendered,
    native_audio_presentation::NativeAudioPresentation,
    native_gameplay::NativeGameplayResult,
};
use beatkernel::{
    audio::{CommandProducer, OutputFrameBasis, RenderReport},
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockMapper, ClockPair, ClockPoint,
        Duration, ExtrapolationPolicy,
    },
};
use beatkernel_platform::audio::presentation::validation::NativePresentationValidator;

/// Original IO and retained input service; the boolean is continuation, not prefix closure.
pub trait NativeAudioSeedPort {
    fn service_input(&mut self) -> NativeGameplayResult<bool>;
    fn observe_audio(
        &mut self,
        presentation: &mut NativeAudioPresentation,
    ) -> NativeGameplayResult<()>;
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>>;
    fn host_now(&self) -> NativeGameplayResult<ClockPoint>;
    fn wait(&mut self, duration: std::time::Duration) -> NativeGameplayResult<()>;
}

/// Two original associations and the actual sample at readiness; no acoustic accuracy claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeededNativeAudio {
    pub observations: [ClockPair; 2],
    pub now: ClockPoint,
}
impl SeededNativeAudio {
    /// Projects a software acquisition origin with explicit finite backward permission.
    pub fn host_for_output(
        &self,
        raw: ClockPoint,
        before: Duration,
    ) -> NativeGameplayResult<ClockPoint> {
        let [first, second] = self.observations;
        if before < Duration::ZERO {
            return Err("native startup backward permission must be nonnegative".into());
        }
        let start = beatkernel::time::Timestamp::from_nanos(
            i64::try_from(
                i128::from(first.source.timestamp.as_nanos()) - i128::from(before.as_nanos()),
            )
            .map_err(|_| "native startup backward validity overflow")?,
        );
        let mapper = AffineClockMapper::from_pairs_unknown(
            first,
            second,
            ClockInterval {
                start,
                end: second.source.timestamp,
            },
            ExtrapolationPolicy::Bounded {
                before,
                after: Duration::ZERO,
            },
        )?;
        let timestamp = mapper
            .map(raw, second.target.domain)
            .ok_or("startup output lies outside its original bounded association")?;
        Ok(ClockPoint {
            domain: second.target.domain,
            timestamp,
        })
    }
}

/// Constructs matching cold owners on the real captured stream grid.
pub fn new_audio_presentation(
    epoch: u64,
    basis: OutputFrameBasis,
    host: ClockDomainId,
    logical_origin: ClockPoint,
    config: AudioAuthorityConfig,
) -> NativeGameplayResult<NativeAudioPresentation> {
    let stream_origin = basis.point_at_stream_frame(0)?;
    NativeAudioPresentation::new(
        AudioAuthority::new(
            config,
            AudioAuthorityEpoch {
                id: epoch,
                stream_origin,
                logical_origin,
                host_domain: host,
            },
        )?,
        NativePresentationValidator::new(epoch, stream_origin, host),
    )
}

/// Constructs target-duration owners without inventing a source-rate stream basis.
pub fn new_target_audio_presentation(
    epoch: u64,
    basis: beatkernel::audio::TargetFrameBasis,
    host: ClockDomainId,
    logical_origin: ClockPoint,
    config: AudioAuthorityConfig,
) -> NativeGameplayResult<NativeAudioPresentation> {
    let stream_origin = basis.point_at_stream_frame(0)?;
    NativeAudioPresentation::new(
        AudioAuthority::new(
            config,
            AudioAuthorityEpoch {
                id: epoch,
                stream_origin,
                logical_origin,
                host_domain: host,
            },
        )?,
        NativePresentationValidator::new(epoch, stream_origin, host),
    )
}

/// Primes a started stream without processing Runtime, closing input or fitting a rate.
pub fn prime_native_audio<P: NativeAudioSeedPort>(
    port: &mut P,
    presentation: &mut NativeAudioPresentation,
    bgm: &mut BgmFeeder,
    producer: &mut CommandProducer,
    timeout: Duration,
) -> NativeGameplayResult<Option<SeededNativeAudio>> {
    if timeout <= Duration::ZERO
        || presentation.authority().history_len() != 0
        || presentation.latest_record().is_some()
    {
        return Err(
            "native audio startup requires a positive bound and cold observation owners".into(),
        );
    }
    let start = port.host_now()?;
    if start.domain != presentation.authority().epoch().host_domain {
        return Err("native audio startup HOST domain differs".into());
    }
    let mut last = start;
    let mut first = None;
    let mut second = None;
    loop {
        if !port.service_input()? {
            return Ok(None);
        }
        // Retain two originals while waiting for assessed driver latency to pass.
        if second.is_none() {
            port.observe_audio(presentation)?;
            if let Some(pair) = presentation.authority().latest_observation() {
                if first.is_none() {
                    first = Some(pair);
                } else if first != Some(pair) {
                    second = Some(pair);
                }
            }
        }
        feed_rendered(bgm, port.render_report()?, |command| {
            producer.try_push(command)
        })?;
        let now = port.host_now()?;
        if now.domain != start.domain || now.timestamp < last.timestamp {
            return Err("native audio startup HOST clock changed or regressed".into());
        }
        last = now;
        let elapsed = i128::from(now.timestamp.as_nanos()) - i128::from(start.timestamp.as_nanos());
        if elapsed >= i128::from(timeout.as_nanos()) {
            return Err("native audio startup timed out awaiting two original associations".into());
        }
        if let (Some(first), Some(second)) = (first, second) {
            let age = i128::from(now.timestamp.as_nanos())
                - i128::from(second.target.timestamp.as_nanos());
            if age >= 0 {
                if age
                    > i128::from(
                        presentation
                            .authority()
                            .config()
                            .max_observation_age
                            .as_nanos(),
                    )
                {
                    return Err("native audio startup associations expired before readiness".into());
                }
                return Ok(Some(SeededNativeAudio {
                    observations: [first, second],
                    now,
                }));
            }
        }
        port.wait(std::time::Duration::from_millis(1))?;
    }
}

/// Target adapters use the same bounded input/BGM/original-association startup loop.
/// NativeAudioSeedPort::observe_audio must call admit_target with the creation basis;
/// render_report remains actual source evidence for BGM feeding, not target ACK.
pub fn prime_target_native_audio<P: NativeAudioSeedPort>(
    port: &mut P,
    presentation: &mut NativeAudioPresentation,
    bgm: &mut BgmFeeder,
    producer: &mut CommandProducer,
    timeout: Duration,
) -> NativeGameplayResult<Option<SeededNativeAudio>> {
    prime_native_audio(port, presentation, bgm, producer, timeout)
}
