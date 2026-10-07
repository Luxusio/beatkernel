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

pub fn update_live_pause(
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
    let mut next = pause.clone();
    let mut requested = None;
    let (boundary, observed) = match evidence {
        LivePauseObservation::Point(pair) => {
            if let Some(desired) = desired {
                if next.request(desired, pair)? {
                    requested = Some(desired);
                }
            }
            let boundary = next
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
                if next.request_interval(desired, reference)? {
                    requested = Some(desired);
                }
            }
            let boundary = next
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
    *pause = next;
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
        next.pause(boundary.at.timestamp).map_err(|_| {
            PauseError("live pause transport cutoff violates chronology or mapping")
        })?;
        next.seek(boundary.at.timestamp, boundary.song)
            .map_err(|_| PauseError("live pause exact song anchor violates chronology"))?;
    } else {
        if !transport.is_paused() || last_song != boundary.song {
            return Err(PauseError(
                "live resume requires the exact frozen judge prefix",
            ));
        }
        if transport
            .position_at(boundary.at.timestamp)
            .map_err(|_| PauseError("live resume transport mapping is invalid"))?
            != boundary.song
        {
            return Err(PauseError(
                "live resume transport differs from the frozen song",
            ));
        }
        next.resume(boundary.at.timestamp)
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
