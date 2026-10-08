//! Shared exact logical pause anchoring and conservative interval input cutoffs.
use crate::{
    native_start::HostStartWindow,
    playback_pause::{NativePause, PauseError, PauseIntervalObservation},
};
use beatkernel::{
    audio::RenderReport,
    time::{ClockPair, ClockPoint, Timestamp},
    transport::Transport,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LivePauseObservation {
    Point(ClockPair),
    Target {
        epoch: u64,
        basis: beatkernel::audio::TargetFrameBasis,
        facts: beatkernel_platform::audio::ConvertedBoundaryFacts,
        source: Option<RenderReport>,
        pair: ClockPair,
    },
    Interval {
        observation: Option<PauseIntervalObservation>,
        now: ClockPoint,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LivePauseBoundary {
    pub paused: bool,
    pub window: HostStartWindow,
    /// Software input cutoff, not an assertion of exact physical presentation.
    pub at: ClockPoint,
    pub playback_frame: u64,
    pub song: Timestamp,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LivePauseUpdate {
    pub requested: Option<bool>,
    pub boundary: Option<LivePauseBoundary>,
    pub observed: bool,
}

/// Existing HOST pause boundary plus its actual epoch-bound physical output cutoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioLivePauseBoundary {
    pub original: LivePauseBoundary,
    pub epoch: u64,
    pub raw_output: ClockPoint,
}
/// A staged native pause update; waiting observations never invent a transition marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioLivePauseUpdate {
    pub requested: Option<bool>,
    pub boundary: Option<AudioLivePauseBoundary>,
    pub observed: bool,
}

pub fn update_live_pause(
    pause: &mut NativePause,
    evidence: LivePauseObservation,
    rendered: Option<RenderReport>,
    desired: Option<bool>,
    song_origin: Timestamp,
    sample_rate: u32,
) -> Result<LivePauseUpdate, PauseError> {
    let mut next = pause.clone();
    let update = update_live_pause_inner(
        &mut next,
        evidence,
        rendered,
        desired,
        song_origin,
        sample_rate,
    )?;
    *pause = next;
    Ok(update)
}

/// Retain the conservative original HOST cutoff and the real physical frame independently.
/// The entire update commits only after all existing validation and raw conversion succeeds.
pub fn update_live_audio_pause(
    pause: &mut NativePause,
    evidence: LivePauseObservation,
    rendered: Option<RenderReport>,
    desired: Option<bool>,
    song_origin: Timestamp,
    sample_rate: u32,
) -> Result<AudioLivePauseUpdate, PauseError> {
    let mut next = pause.clone();
    let update = update_live_pause_inner(
        &mut next,
        evidence,
        rendered,
        desired,
        song_origin,
        sample_rate,
    )?;
    let boundary = update
        .boundary
        .map(|original| -> Result<AudioLivePauseBoundary, PauseError> {
            let raw_output = next.last_transition_output()?.ok_or(PauseError(
                "committed native pause transition lacks its physical frame",
            ))?;
            Ok(AudioLivePauseBoundary {
                original,
                epoch: next.epoch(),
                raw_output,
            })
        })
        .transpose()?;
    *pause = next;
    Ok(AudioLivePauseUpdate {
        requested: update.requested,
        boundary,
        observed: update.observed,
    })
}

fn update_live_pause_inner(
    pause: &mut NativePause,
    evidence: LivePauseObservation,
    rendered: Option<RenderReport>,
    desired: Option<bool>,
    song_origin: Timestamp,
    sample_rate: u32,
) -> Result<LivePauseUpdate, PauseError> {
    if sample_rate == 0 || sample_rate > 1_000_000_000 {
        return Err(PauseError(
            "live pause requires a representable nonzero frame grid",
        ));
    }
    let mut requested = None;
    let (boundary, observed) = match evidence {
        LivePauseObservation::Target {
            epoch,
            basis,
            facts,
            source,
            pair,
        } => {
            if facts.source_rate != sample_rate || pause.target_basis() != Some(basis) {
                return Err(PauseError("target live pause source rate or basis differs"));
            }
            if let Some(desired) = desired {
                if pause.request_in_epoch(epoch, desired, pair)? {
                    requested = Some(desired);
                }
            }
            let boundary = pause
                .observe_target(epoch, basis, facts, source, pair)?
                .map(|boundary| {
                    HostStartWindow::new(boundary.host, boundary.host)
                        .map(|window| (boundary.paused, window, boundary.playback_frame))
                        .map_err(|_| PauseError("target live pause boundary is invalid"))
                })
                .transpose()?;
            (boundary, true)
        }
        LivePauseObservation::Point(pair) => {
            if let Some(desired) = desired {
                if pause.request(desired, pair)? {
                    requested = Some(desired);
                }
            }
            let boundary = pause
                .observe(rendered, pair)?
                .map(|boundary| {
                    HostStartWindow::new(boundary.host, boundary.host)
                        .map(|window| (boundary.paused, window, boundary.playback_frame))
                        .map_err(|_| PauseError("live pause point boundary is invalid"))
                })
                .transpose()?;
            (boundary, true)
        }
        LivePauseObservation::Interval { observation, now } => {
            if observation.is_some_and(|value| value.sample_rate != sample_rate) {
                return Err(PauseError(
                    "live pause interval rate differs from its song grid",
                ));
            }
            if let (Some(desired), Some(reference)) = (desired, observation) {
                if pause.request_interval(desired, reference)? {
                    requested = Some(desired);
                }
            }
            let boundary = pause
                .observe_interval(observation, now)?
                .map(|boundary| (boundary.paused, boundary.host, boundary.playback_frame));
            (boundary, observation.is_some())
        }
    };
    let boundary = boundary
        .map(
            |(paused, window, playback_frame)| -> Result<LivePauseBoundary, PauseError> {
                let elapsed = i128::from(playback_frame) * 1_000_000_000 / i128::from(sample_rate);
                let song = i128::from(song_origin.as_nanos())
                    .checked_add(elapsed)
                    .and_then(|value| i64::try_from(value).ok())
                    .ok_or(PauseError("live pause boundary song time overflow"))?;
                Ok(LivePauseBoundary {
                    paused,
                    window,
                    at: if paused {
                        window.earliest()
                    } else {
                        window.latest()
                    },
                    playback_frame,
                    song: Timestamp::from_nanos(song),
                })
            },
        )
        .transpose()?;
    Ok(LivePauseUpdate {
        requested,
        boundary,
        observed,
    })
}

fn validate_boundary(boundary: LivePauseBoundary) -> Result<(), PauseError> {
    let expected = if boundary.paused {
        boundary.window.earliest()
    } else {
        boundary.window.latest()
    };
    if boundary.at != expected {
        return Err(PauseError(
            "live pause cutoff differs from its selected window endpoint",
        ));
    }
    Ok(())
}

/// Stages a transport replacement without changing any historical pre-cutoff mapping.
pub fn prepare_live_transport(
    transport: &Transport,
    boundary: LivePauseBoundary,
    last_song: Timestamp,
) -> Result<Transport, PauseError> {
    validate_boundary(boundary)?;
    prepare_transport_at(transport, boundary, boundary.at.timestamp, last_song)
}

/// Stage pause or resume on the logical point of the original native transition.
/// The caller still validates the authority token and commits the actual Runtime effect.
pub fn prepare_live_audio_transport(
    transport: &Transport,
    boundary: AudioLivePauseBoundary,
    control: &crate::audio_authority::PreparedControlCutoff,
    last_song: Timestamp,
) -> Result<Transport, PauseError> {
    validate_boundary(boundary.original)?;
    if boundary.epoch != control.epoch()
        || boundary.raw_output != control.raw_output()
        || boundary.original.at != control.host()
        || boundary.original.paused == control.is_resume()
    {
        return Err(PauseError(
            "audio pause boundary differs from prepared control",
        ));
    }
    prepare_transport_at(
        transport,
        boundary.original,
        control.output().timestamp,
        last_song,
    )
}

fn prepare_transport_at(
    transport: &Transport,
    boundary: LivePauseBoundary,
    at: Timestamp,
    last_song: Timestamp,
) -> Result<Transport, PauseError> {
    let mut next = transport.clone();
    if boundary.paused {
        if last_song > boundary.song {
            return Err(PauseError(
                "committed judge prefix exceeds the exact pause song",
            ));
        }
        if transport.is_paused() {
            return Err(PauseError("live pause requires a running transport"));
        }
        next.pause(at).map_err(|_| {
            PauseError("live pause transport cutoff violates chronology or mapping")
        })?;
        next.seek(at, boundary.song)
            .map_err(|_| PauseError("live pause exact song anchor violates chronology"))?;
    } else {
        if !transport.is_paused() || last_song != boundary.song {
            return Err(PauseError(
                "live resume requires the exact frozen judge prefix",
            ));
        }
        if transport
            .position_at(at)
            .map_err(|_| PauseError("live resume transport mapping is invalid"))?
            != boundary.song
        {
            return Err(PauseError(
                "live resume transport differs from the frozen song",
            ));
        }
        next.resume(at)
            .map_err(|_| PauseError("live resume cutoff violates transport chronology"))?;
    }
    Ok(next)
}

/// Guards original inputs admitted strictly before the pause software cutoff.
pub fn validate_pre_pause_input(
    transport: &Transport,
    boundary: LivePauseBoundary,
    host: ClockPoint,
) -> Result<(), PauseError> {
    validate_boundary(boundary)?;
    if !boundary.paused
        || host.domain != boundary.at.domain
        || host.timestamp >= boundary.at.timestamp
    {
        return Err(PauseError(
            "live pre-pause input is outside its original cutoff/domain",
        ));
    }
    if transport
        .position_at(host.timestamp)
        .map_err(|_| PauseError("live pre-pause input has an invalid transport mapping"))?
        > boundary.song
    {
        return Err(PauseError(
            "queued input exceeds the exact pause song prefix",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "live_pause_fixtures.rs"]
mod fixtures;

#[cfg(test)]
#[path = "audio_authority_lifecycle_fixtures.rs"]
mod audio_authority_lifecycle_fixtures;
