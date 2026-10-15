//! Bounded original-observation startup shared by native launchers.
use crate::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch, AudioAuthorityCoordinateBasis,
        AudioAuthorityObservation,
    },
    bgm::BgmFeeder,
    native_audio::feed_rendered,
    native_audio_presentation::NativeAudioPresentation,
    native_gameplay::NativeGameplayResult,
    native_coordinate_control::{CoordinateControlFacts, CoordinatePlaybackControlFacts},
    gameplay::output::ports::SourcePlaybackIdentity,
};
use beatkernel::{
    audio::{CommandProducer, OutputFrameBasis, RenderReport, InterpolatedTargetTime},
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockMapper, ClockPair, ClockPoint,
        Duration, ExtrapolationPolicy, Timestamp,
    },
};
use beatkernel_platform::audio::presentation::validation::NativePresentationValidator;

/// Slots for acquired input, retained practice receipts and native anchors.
pub const SOURCE_COORDINATE_HISTORY_HEADROOM: usize = 4096 + 256 + 2;

static NEXT_RUNTIME_PROGRAM_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

/// Identifies one immutable cold retained program, independently of its clocks.
/// Exhaustion refuses preparation rather than reusing zero or an earlier identity.
pub fn next_runtime_program_id() -> NativeGameplayResult<u64> {
    NEXT_RUNTIME_PROGRAM_ID
        .fetch_update(
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
            |next| next.checked_add(1),
        )
        .map_err(|_| "native retained runtime program identities exhausted".into())
}

/// Bounds Source history conservatively even when each callback contains one frame.
pub fn source_coordinate_history_capacity(buffer_frames: u32) -> NativeGameplayResult<usize> {
    if buffer_frames == 0 {
        return Err("Source coordinate history requires a nonzero buffer".into());
    }
    let capacity = u64::from(buffer_frames)
        .checked_add(u64::try_from(SOURCE_COORDINATE_HISTORY_HEADROOM)?)
        .ok_or("Source coordinate history capacity overflow")?;
    Ok(usize::try_from(capacity)?)
}

/// Captures the actual cold creation identity without enabling history or rendering.
pub fn cold_source_coordinate_identity(
    mixer: &beatkernel::audio::Mixer,
) -> NativeGameplayResult<SourcePlaybackIdentity> {
    let (hz, physical, playback) = match mixer.source_playback_history() {
        Some(history) => (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
        ),
        None => (
            mixer.config().format().sample_rate(),
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
        ),
    };
    Ok(SourcePlaybackIdentity::new(hz, physical, playback)?)
}

/// Enables history before native ownership and captures its actual cold cursor identity.
/// Repeated or zero-capacity enabling retains the mixer's existing refusal semantics.
pub fn prepare_source_coordinate_history(
    mixer: &mut beatkernel::audio::Mixer,
    capacity: usize,
) -> NativeGameplayResult<SourcePlaybackIdentity> {
    mixer.enable_source_playback_history(capacity)?;
    cold_source_coordinate_identity(mixer)
}

/// Exact original physical creation cut; this neither floors to nanoseconds nor reads HOST.
/// A mathematical query does not prove native presentation or startup admission.
pub fn initial_source_coordinate_query(
    basis: OutputFrameBasis,
) -> NativeGameplayResult<InterpolatedTargetTime> {
    Ok(InterpolatedTargetTime::new(
        beatkernel::audio::TargetTime::from_frames(
            basis.start_physical_frame(),
            basis.sample_rate(),
        )?,
        0,
        0,
        1,
        0,
        1,
    )?)
}

/// Builds the first attempt on the actual Source playback cut and selected song setup.
/// The supplied logical chronology is independent of Source history creation metadata.
pub fn initial_source_coordinate_transport(
    epoch: u64,
    identity: SourcePlaybackIdentity,
    logical_origin: ClockPoint,
    setup: crate::live_speed::LiveSpeedSetup,
) -> NativeGameplayResult<beatkernel::transport::Transport> {
    let basis = beatkernel::transport::OutputPhaseBasis::new(
        logical_origin.domain,
        epoch,
        1,
        identity.initial_playback(),
        identity.hz(),
    )?;
    Ok(beatkernel::transport::Transport::new_with_output_phase(
        basis,
        logical_origin,
        setup.start(),
        setup.preroll(),
        setup.speed().rate(),
        setup.end(),
    )?)
}

/// Builds the same actual creation phase with retained runtime-rate history.
/// This opt-in does not establish presentation or input-cutoff authority.
pub fn initial_source_coordinate_runtime_transport(
    epoch: u64,
    identity: SourcePlaybackIdentity,
    logical_origin: ClockPoint,
    setup: crate::live_speed::LiveSpeedSetup,
) -> NativeGameplayResult<beatkernel::transport::Transport> {
    let basis = beatkernel::transport::OutputPhaseBasis::new(
        logical_origin.domain,
        epoch,
        1,
        identity.initial_playback(),
        identity.hz(),
    )?;
    Ok(beatkernel::transport::Transport::new_with_runtime_output_phase(
        basis,
        logical_origin,
        setup.start(),
        setup.preroll(),
        setup.speed().rate(),
        setup.end(),
    )?)
}

/// Original IO and retained input service; the boolean is continuation, not prefix closure.
pub trait NativeAudioSeedPort {
    fn service_input(&mut self) -> NativeGameplayResult<bool>;
    fn observe_audio(
        &mut self,
        presentation: &mut NativeAudioPresentation,
    ) -> NativeGameplayResult<()>;
    /// Original compound source publication; unsupported adapters refuse explicitly.
    fn observe_coordinate_audio(
        &mut self,
        _: &mut NativeAudioPresentation,
        _: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinateControlFacts>> {
        Err("original compound source startup is unsupported".into())
    }
    /// Original compound playback publication; existing Source adapters retain their path.
    fn observe_coordinate_playback_audio(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinatePlaybackControlFacts>> {
        Ok(self
            .observe_coordinate_audio(presentation, identity)?
            .map(CoordinatePlaybackControlFacts::Source))
    }
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

/// Two coupled original native observations captured on one immutable descriptor.
/// Exact inverse projection is mathematical correspondence, not a native ACK.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeededCoordinateNativeAudio {
    basis: AudioAuthorityCoordinateBasis,
    epoch: AudioAuthorityEpoch,
    observations: [AudioAuthorityObservation; 2],
    now: ClockPoint,
}
impl SeededCoordinateNativeAudio {
    pub const fn basis(&self) -> AudioAuthorityCoordinateBasis {
        self.basis
    }
    pub const fn epoch(&self) -> AudioAuthorityEpoch {
        self.epoch
    }
    pub const fn observations(&self) -> [AudioAuthorityObservation; 2] {
        self.observations
    }
    pub const fn now(&self) -> ClockPoint {
        self.now
    }

    /// Minimum integer original HOST at which the exact physical query is reached.
    /// Only the explicit finite backward HOST allowance extends the observed range.
    pub fn host_for_query(
        &self,
        query: &InterpolatedTargetTime,
        before_host: Duration,
    ) -> NativeGameplayResult<Option<ClockPoint>> {
        if before_host < Duration::ZERO {
            return Err("native startup backward HOST permission must be nonnegative".into());
        }
        let [left, right] = self.observations;
        let start = i128::from(left.pair().target.timestamp.as_nanos())
            - i128::from(before_host.as_nanos());
        let start =
            i64::try_from(start).map_err(|_| "native startup backward HOST range overflow")?;
        let end = right.pair().target.timestamp.as_nanos();
        let at = |ns| {
            self.basis.interpolate(
                left,
                right,
                ClockPoint {
                    domain: self.epoch.host_domain,
                    timestamp: Timestamp::from_nanos(ns),
                },
            )
        };
        if query.compare_interpolated_time(at(start)?).is_lt()
            || query.compare_interpolated_time(at(end)?).is_gt()
        {
            return Ok(None);
        }
        // Inclusive lower-bound search over all i64 HOST points takes at most 64 steps.
        let mut low = start;
        let mut high = end;
        for _ in 0..64 {
            if low == high {
                break;
            }
            let middle = i64::try_from(i128::from(low) + (i128::from(high) - i128::from(low)) / 2)
                .map_err(|_| "native startup HOST midpoint overflow")?;
            if at(middle)?.compare_interpolated_time(*query).is_lt() {
                low = middle
                    .checked_add(1)
                    .ok_or("native startup HOST ceil overflow")?;
            } else {
                high = middle;
            }
        }
        Ok(Some(ClockPoint {
            domain: self.epoch.host_domain,
            timestamp: Timestamp::from_nanos(low),
        }))
    }
}

/// Cold original-coordinate owners; the descriptor remains unchanged on admission.
pub fn new_coordinate_audio_presentation(
    epoch: u64,
    basis: AudioAuthorityCoordinateBasis,
    host: ClockDomainId,
    logical_origin: ClockPoint,
    config: AudioAuthorityConfig,
) -> NativeGameplayResult<NativeAudioPresentation> {
    let stream_origin = basis.origin()?;
    NativeAudioPresentation::new(
        AudioAuthority::new_with_coordinates(
            config,
            AudioAuthorityEpoch {
                id: epoch,
                stream_origin,
                logical_origin,
                host_domain: host,
            },
            basis,
        )?,
        NativePresentationValidator::new(epoch, stream_origin, host),
    )
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
    Ok(prime_observations(
        port,
        presentation,
        bgm,
        producer,
        timeout,
        None,
        acquire_scalar_audio::<P, _, _>,
        |presentation| Ok(presentation.authority().latest_observation()),
        |pair: ClockPair| pair.target,
    )?
    .map(|(observations, now)| SeededNativeAudio { observations, now }))
}

/// Primes explicit original-coordinate admission without processing gameplay.
/// The port must admit its actual original native coordinate, not scalar pairs.
pub fn prime_coordinate_native_audio<P: NativeAudioSeedPort>(
    port: &mut P,
    presentation: &mut NativeAudioPresentation,
    bgm: &mut BgmFeeder,
    producer: &mut CommandProducer,
    timeout: Duration,
) -> NativeGameplayResult<Option<SeededCoordinateNativeAudio>> {
    presentation.validate_coordinate_seed()?;
    let epoch = presentation.authority().epoch();
    let basis = presentation
        .authority()
        .coordinate_basis()
        .ok_or("coordinate startup lacks original descriptor")?;
    Ok(prime_observations(
        port,
        presentation,
        bgm,
        producer,
        timeout,
        None,
        acquire_scalar_audio::<P, _, _>,
        |presentation| {
            if presentation.authority().epoch() != epoch
                || presentation.authority().coordinate_basis() != Some(basis)
            {
                return Err("coordinate startup creation descriptor or epoch changed".into());
            }
            presentation.validate_coordinate_seed()?;
            Ok(presentation.authority().latest_coordinate_observation())
        },
        |observation: AudioAuthorityObservation| observation.pair().target,
    )?
    .map(|(observations, now)| SeededCoordinateNativeAudio {
        basis,
        epoch,
        observations,
        now,
    }))
}

/// Primes original source publications without scalar reads or endpoint acknowledgement.
pub fn prime_source_coordinate_native_audio<P: NativeAudioSeedPort>(
    port: &mut P,
    presentation: &mut NativeAudioPresentation,
    identity: SourcePlaybackIdentity,
    bgm: &mut BgmFeeder,
    producer: &mut CommandProducer,
    timeout: Duration,
) -> NativeGameplayResult<Option<SeededCoordinateNativeAudio>> {
    prime_source_coordinate_native_audio_with_deadline(
        port,
        presentation,
        identity,
        bgm,
        producer,
        timeout,
        None,
    )
}

fn prime_source_coordinate_native_audio_with_deadline<P: NativeAudioSeedPort>(
    port: &mut P,
    presentation: &mut NativeAudioPresentation,
    identity: SourcePlaybackIdentity,
    bgm: &mut BgmFeeder,
    producer: &mut CommandProducer,
    timeout: Duration,
    deadline: Option<std::time::Instant>,
) -> NativeGameplayResult<Option<SeededCoordinateNativeAudio>> {
    presentation.validate_coordinate_seed()?;
    let epoch = presentation.authority().epoch();
    let basis = presentation
        .authority()
        .coordinate_basis()
        .ok_or("source coordinate startup lacks original descriptor")?;
    let source_basis = match basis {
        AudioAuthorityCoordinateBasis::SourceFrame(basis) => basis,
        AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency } if frequency != 0 => {
            basis
        }
        _ => return Err("source coordinate startup requires a source descriptor".into()),
    };
    if identity.hz() != source_basis.sample_rate()
        || identity.initial_physical() > source_basis.start_physical_frame()
    {
        return Err(
            "source coordinate startup history identity differs from creation basis".into(),
        );
    }
    let read = |presentation: &NativeAudioPresentation| {
        if presentation.authority().epoch() != epoch
            || presentation.authority().coordinate_basis() != Some(basis)
        {
            return Err("coordinate startup creation descriptor or epoch changed".into());
        }
        presentation.validate_coordinate_seed()?;
        Ok(presentation.authority().latest_coordinate_observation())
    };
    Ok(prime_observations(
        port,
        presentation,
        bgm,
        producer,
        timeout,
        deadline,
        |port: &mut P, presentation: &mut NativeAudioPresentation, collect: bool, _: &_| {
            read(presentation)?;
            let facts = port.observe_coordinate_audio(presentation, identity)?;
            let observation = read(presentation)?;
            match facts {
                Some(facts) => {
                    facts.validate(presentation, identity)?;
                    Ok((
                        if collect { observation } else { None },
                        facts.source().render,
                    ))
                }
                None => Ok((None, None)),
            }
        },
        read,
        |observation: AudioAuthorityObservation| observation.pair().target,
    )?
    .map(|(observations, now)| SeededCoordinateNativeAudio {
        basis,
        epoch,
        observations,
        now,
    }))
}

/// A local initial gate confirmed by original Source observations on both sides.
/// This is software/native correspondence, not acoustic acknowledgement.
#[derive(Clone, Copy, Debug)]
pub struct StartedSourceCoordinateNativeAudio {
    pub source_frame: u64,
    pub query: InterpolatedTargetTime,
    pub host_origin: ClockPoint,
    pub seeded: SeededCoordinateNativeAudio,
}

/// Arms one real local Source gate and waits for its exact original clock bracket.
/// Input remains serviced by the caller's original collector throughout startup.
pub fn start_local_source_coordinate_native_audio<P: NativeAudioSeedPort>(
    port: &mut P,
    presentation: &mut NativeAudioPresentation,
    identity: SourcePlaybackIdentity,
    bgm: &mut BgmFeeder,
    producer: &mut CommandProducer,
    pause: &mut crate::playback_pause::NativePause,
    end: &mut Option<crate::native_end::NativeEnd>,
    buffer_frames: u32,
    timeout: Duration,
) -> NativeGameplayResult<Option<StartedSourceCoordinateNativeAudio>> {
    if buffer_frames == 0
        || timeout <= Duration::ZERO
        || producer.start_gate_frame() != Some(None)
        || producer.applied_start_frame().is_some()
        || producer.is_disconnected()
        || identity.initial_playback() != 0
    {
        return Err("local Source startup requires a fresh closed gate and valid bounds".into());
    }
    presentation.validate_coordinate_seed()?;
    let epoch = presentation.authority().epoch();
    let basis = presentation
        .authority()
        .coordinate_basis()
        .ok_or("local Source startup lacks its creation descriptor")?;
    let source_basis = match basis {
        AudioAuthorityCoordinateBasis::SourceFrame(basis) => basis,
        AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency } if frequency != 0 => {
            basis
        }
        _ => return Err("local Source startup requires an original Source descriptor".into()),
    };
    if identity.hz() != source_basis.sample_rate()
        || identity.initial_physical() > source_basis.start_physical_frame()
    {
        return Err("local Source startup identity differs from creation descriptor".into());
    }
    // Resource watchdog only: original audio/HOST observations still own time mapping.
    let deadline = std::time::Instant::now()
        .checked_add(std::time::Duration::from_nanos(u64::try_from(
            timeout.as_nanos(),
        )?))
        .ok_or("local Source startup watchdog deadline overflow")?;
    let started_at = port.host_now()?;
    if started_at.domain != epoch.host_domain {
        return Err("local Source startup HOST domain differs".into());
    }
    let Some(initial) = prime_source_coordinate_native_audio_with_deadline(
        port,
        presentation,
        identity,
        bgm,
        producer,
        timeout,
        Some(deadline),
    )?
    else {
        return Ok(None);
    };
    let mut last_now = started_at;
    let mut observations = initial.observations();
    let mut selected: Option<(u64, InterpolatedTargetTime)> = None;
    let creation_query = initial_source_coordinate_query(source_basis)?;
    loop {
        if std::time::Instant::now() >= deadline {
            return Err("local Source startup watchdog expired awaiting gate presentation".into());
        }
        if !port.service_input()? {
            return Ok(None);
        }
        if presentation.authority().epoch() != epoch
            || presentation.authority().coordinate_basis() != Some(basis)
        {
            return Err("local Source startup creation descriptor or epoch changed".into());
        }
        presentation.validate_coordinate_seed()?;
        let facts = port.observe_coordinate_audio(presentation, identity)?;
        if presentation.authority().epoch() != epoch
            || presentation.authority().coordinate_basis() != Some(basis)
        {
            return Err("local Source startup creation descriptor or epoch changed".into());
        }
        presentation.validate_coordinate_seed()?;
        let now = port.host_now()?;
        if now.domain != epoch.host_domain || now.timestamp < last_now.timestamp {
            return Err("local Source startup HOST clock changed or regressed".into());
        }
        last_now = now;
        if i128::from(now.timestamp.as_nanos()) - i128::from(started_at.timestamp.as_nanos())
            >= i128::from(timeout.as_nanos())
        {
            return Err("local Source startup timed out awaiting exact gate presentation".into());
        }
        if let Some((source_frame, _)) = selected {
            if producer.start_gate_frame() != Some(Some(source_frame))
                || producer
                    .applied_start_frame()
                    .is_some_and(|frame| frame != source_frame)
            {
                return Err("local Source startup gate or applied frame changed".into());
            }
        }
        if let Some(facts) = facts {
            facts.validate(presentation, identity)?;
            let current = presentation
                .authority()
                .latest_coordinate_observation()
                .ok_or("local Source startup lost its original observation")?;
            let age = i128::from(now.timestamp.as_nanos())
                - i128::from(current.pair().target.timestamp.as_nanos());
            if age
                > i128::from(
                    presentation
                        .authority()
                        .config()
                        .max_observation_age
                        .as_nanos(),
                )
            {
                return Err("local Source startup observation expired".into());
            }
            let report = facts.source().render;
            feed_rendered(bgm, report, |command| producer.try_push(command))?;
            if selected.is_none() {
                if producer.start_gate_frame() != Some(None)
                    || producer.applied_start_frame().is_some()
                {
                    return Err("local Source startup gate changed before arming".into());
                }
                if let Some(report) = report.filter(|_| {
                    facts
                        .query()
                        .compare_interpolated_time(creation_query)
                        .is_gt()
                }) {
                    if report.frames == 0
                        || report.playback_start_frame != 0
                        || report.playback_frames != 0
                        || report.playback_end_physical_frame.is_some()
                        || report.producer_disconnected
                        || report.start_frame < source_basis.start_physical_frame()
                    {
                        return Err(
                            "local Source startup requires a frozen original render prefix".into(),
                        );
                    }
                    let rendered_end = report
                        .start_frame
                        .checked_add(u64::try_from(report.frames)?)
                        .ok_or("local Source startup render extent overflow")?;
                    let source_frame = rendered_end
                        .checked_add(u64::from(buffer_frames) * 2)
                        .ok_or("local Source startup gate frame overflow")?;
                    let query = InterpolatedTargetTime::new(
                        beatkernel::audio::TargetTime::from_frames(source_frame, identity.hz())?,
                        0,
                        0,
                        1,
                        0,
                        1,
                    )?;
                    if !facts.query().compare_interpolated_time(query).is_lt() {
                        return Err(
                            "local Source startup selected cut lacks an original left bracket"
                                .into(),
                        );
                    }
                    let staged_pause = pause.clone().with_start_frame(source_frame)?;
                    let staged_end = end
                        .clone()
                        .map(|observer| observer.with_start_frame(source_frame))
                        .transpose()?;
                    producer.schedule_start_at(source_frame)?;
                    *pause = staged_pause;
                    *end = staged_end;
                    observations[0] = current;
                    observations[1] = current;
                    selected = Some((source_frame, query));
                }
            } else if observations[1] != current {
                let (_, query) = selected.unwrap();
                if facts.query().compare_interpolated_time(query).is_lt() {
                    observations[0] = current;
                }
                observations[1] = current;
            }
            if let Some((source_frame, query)) = selected {
                if producer.start_gate_frame() != Some(Some(source_frame)) {
                    return Err("local Source startup armed gate changed".into());
                }
                let applied = producer.applied_start_frame();
                if applied.is_some_and(|frame| frame != source_frame) {
                    return Err(
                        "local Source startup applied frame differs from selected gate".into(),
                    );
                }
                if applied == Some(source_frame) && observations[0] != observations[1] && age >= 0 {
                    let seeded = SeededCoordinateNativeAudio {
                        basis,
                        epoch,
                        observations,
                        now,
                    };
                    let left = basis.interpolate(
                        observations[0],
                        observations[1],
                        observations[0].pair().target,
                    )?;
                    let right = basis.interpolate(
                        observations[0],
                        observations[1],
                        observations[1].pair().target,
                    )?;
                    if left.compare_interpolated_time(query).is_lt()
                        && !right.compare_interpolated_time(query).is_lt()
                    {
                        if let Some(host_origin) = seeded.host_for_query(&query, Duration::ZERO)? {
                            return Ok(Some(StartedSourceCoordinateNativeAudio {
                                source_frame,
                                query,
                                host_origin,
                                seeded,
                            }));
                        }
                    }
                }
            }
        }
        port.wait(std::time::Duration::from_millis(1))?;
    }
}

/// Primes original Target publications without scalar reads or endpoint acknowledgement.
/// The Source history identity remains independent of the captured Target stream grid.
pub fn prime_target_coordinate_native_audio<P: NativeAudioSeedPort>(
    port: &mut P,
    presentation: &mut NativeAudioPresentation,
    identity: SourcePlaybackIdentity,
    bgm: &mut BgmFeeder,
    producer: &mut CommandProducer,
    timeout: Duration,
) -> NativeGameplayResult<Option<SeededCoordinateNativeAudio>> {
    presentation.validate_coordinate_seed()?;
    let epoch = presentation.authority().epoch();
    let basis = presentation
        .authority()
        .coordinate_basis()
        .ok_or("target coordinate startup lacks original descriptor")?;
    if !matches!(basis, AudioAuthorityCoordinateBasis::TargetFrame(_)) {
        return Err("target coordinate startup requires a target descriptor".into());
    }
    let read = |presentation: &NativeAudioPresentation| {
        if presentation.authority().epoch() != epoch
            || presentation.authority().coordinate_basis() != Some(basis)
        {
            return Err("coordinate startup creation descriptor or epoch changed".into());
        }
        presentation.validate_coordinate_seed()?;
        Ok(presentation.authority().latest_coordinate_observation())
    };
    Ok(prime_observations(
        port,
        presentation,
        bgm,
        producer,
        timeout,
        None,
        |port: &mut P, presentation: &mut NativeAudioPresentation, collect: bool, _: &_| {
            read(presentation)?;
            let facts = port.observe_coordinate_playback_audio(presentation, identity)?;
            let observation = read(presentation)?;
            match facts {
                Some(facts) => {
                    if !matches!(facts, CoordinatePlaybackControlFacts::Target(_)) {
                        return Err(
                            "target coordinate startup requires target playback facts".into()
                        );
                    }
                    facts.validate(presentation, identity)?;
                    Ok((if collect { observation } else { None }, facts.report()))
                }
                None => Ok((None, None)),
            }
        },
        read,
        |observation: AudioAuthorityObservation| observation.pair().target,
    )?
    .map(|(observations, now)| SeededCoordinateNativeAudio {
        basis,
        epoch,
        observations,
        now,
    }))
}

// Preserve the original scalar/coordinate/target acquisition and render-read cadence.
fn acquire_scalar_audio<P, O, Read>(
    port: &mut P,
    presentation: &mut NativeAudioPresentation,
    collect: bool,
    read: &Read,
) -> NativeGameplayResult<(Option<O>, Option<RenderReport>)>
where
    P: NativeAudioSeedPort,
    Read: Fn(&NativeAudioPresentation) -> NativeGameplayResult<Option<O>>,
{
    let observation = if collect {
        port.observe_audio(presentation)?;
        read(presentation)?
    } else {
        None
    };
    Ok((observation, port.render_report()?))
}

fn prime_observations<P, O, Acquire, Read, Host>(
    port: &mut P,
    presentation: &mut NativeAudioPresentation,
    bgm: &mut BgmFeeder,
    producer: &mut CommandProducer,
    timeout: Duration,
    deadline: Option<std::time::Instant>,
    mut acquire: Acquire,
    read: Read,
    observation_host: Host,
) -> NativeGameplayResult<Option<([O; 2], ClockPoint)>>
where
    P: NativeAudioSeedPort,
    O: Copy + PartialEq,
    Acquire: FnMut(
        &mut P,
        &mut NativeAudioPresentation,
        bool,
        &Read,
    ) -> NativeGameplayResult<(Option<O>, Option<RenderReport>)>,
    Read: Fn(&NativeAudioPresentation) -> NativeGameplayResult<Option<O>>,
    Host: Fn(O) -> ClockPoint,
{
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
        if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
            return Err(
                "local Source startup watchdog expired awaiting original associations".into(),
            );
        }
        if !port.service_input()? {
            return Ok(None);
        }
        // Retain two originals while waiting for assessed driver latency to pass.
        let (observation, render) = acquire(port, presentation, second.is_none(), &read)?;
        if second.is_none() {
            if let Some(pair) = observation {
                if first.is_none() {
                    first = Some(pair);
                } else if first != Some(pair) {
                    second = Some(pair);
                }
            }
        }
        feed_rendered(bgm, render, |command| producer.try_push(command))?;
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
                - i128::from(observation_host(second).timestamp.as_nanos());
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
                read(presentation)?;
                return Ok(Some(([first, second], now)));
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

#[cfg(test)]
#[path = "native_audio_startup_coordinate_fixtures.rs"]
mod coordinate_fixtures;

#[cfg(test)]
#[path = "native_audio_startup_source_coordinate_fixtures.rs"]
mod source_coordinate_fixtures;

#[cfg(test)]
#[path = "native_audio_startup_target_coordinate_fixtures.rs"]
mod target_coordinate_fixtures;

#[cfg(test)]
#[path = "native_audio_startup_local_source_gate_fixtures.rs"]
mod local_source_gate_fixtures;
