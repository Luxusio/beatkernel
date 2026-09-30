//! Fresh software output owners for a section; native presentation is host-owned.
use beatkernel::{audio::*, runtime::restart::*, time::*};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let limits = PcmLimits::new(128, 128, 1)?;
    let source = PcmSample::new(AudioFormat::new(3, 1)?, vec![0.1, 0.2, 0.3, 0.4], limits)?;
    let config = MixerConfig::new(
        source.format(),
        ClockDomainId(2),
        Timestamp::ZERO,
        AudioLimits::new(4, 4, 4, 8, 4)?,
    );
    // Synthetic paired observations demonstrate explicit mapping only.
    // A native host must supply actual presentation observations instead.
    let pair = |source, target| ClockPair {
        source: ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(source),
        },
        target: ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(target),
        },
    };
    let mapping = AffineClockMapper::from_pairs(
        pair(0, 1_000_000_000),
        pair(1_000_000_000, 2_000_000_000),
        ClockInterval {
            start: Timestamp::ZERO,
            end: Timestamp::from_nanos(1_000_000_000),
        },
        ExtrapolationPolicy::Forbid,
        CalibrationUncertainty {
            observation_error: Duration::from_nanos(100),
            residual_drift_error: None,
        },
    )?;
    for _ in 0..3 {
        let plan = RestartPlan::select(
            &source,
            Timestamp::ZERO,
            Timestamp::from_nanos(500_000_000),
            FrameRounding::Nearest,
        )?;
        let mut prepared = plan.prepare_audio(config, limits, SampleId(1), VoiceId(1))?;
        let (transport, quality) = prepared.mapped_transport(ClockDomainId(1), &mapping)?;
        let mut pcm = [0.0; 3];
        prepared.mixer.render(&mut pcm)?;
        println!(
            "source frame {}, applied {}ns, correction {}ns, synthetic host anchor {}ns, mapping {:?}, PCM {:?}",
            plan.source_frame(),
            plan.applied_song_time().as_nanos(),
            plan.correction_nanos(),
            transport.anchor().host_time.as_nanos(),
            quality,
            pcm
        );
    }
    Ok(())
}
