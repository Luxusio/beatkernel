//! Completion association and ordered save policy; effects are explicit callbacks.
use crate::{
    gauge::GaugeProfile,
    local_players::PlayerId,
    native_gameplay::NativeGameplayResult,
    play_result::{
        CompletedPlayResult, CompletedSoloPublicationError, CompletedLocalPublicationError,
    },
    replay_capture::LiveReplayCapture,
    result_archive::{ResultArchive, MAX_HEADER_BYTES},
};
use beatkernel::replay::ReplayHeader;

fn identity(
    player: PlayerId,
    capture: &LiveReplayCapture,
    profile: &GaugeProfile,
) -> NativeGameplayResult<(PlayerId, ReplayHeader, GaugeProfile)> {
    let header = capture.header();
    let length = header
        .chart_identity
        .len()
        .checked_add(header.rules_identity.len())
        .and_then(|n| n.checked_add(header.options.len()))
        .ok_or("archive header length overflow")?;
    if length > MAX_HEADER_BYTES {
        return Err("archive header exceeds bound".into());
    }
    let copy = |bytes: &[u8]| -> NativeGameplayResult<Vec<u8>> {
        let mut result = Vec::new();
        result.try_reserve_exact(bytes.len())?;
        result.extend_from_slice(bytes);
        Ok(result)
    };
    let mut grades = Vec::new();
    grades.try_reserve_exact(profile.grades().len())?;
    grades.extend_from_slice(profile.grades());
    let profile = GaugeProfile::new(
        profile.initial_units(),
        profile.clear_units(),
        profile.default_hit_delta(),
        profile.miss_delta(),
        profile.fail_on_empty(),
        grades,
    )?;
    Ok((
        player,
        ReplayHeader {
            version: header.version,
            chart_identity: copy(&header.chart_identity)?,
            rules_identity: copy(&header.rules_identity)?,
            options: copy(&header.options)?,
            seed: header.seed,
            normalized_clock: header.normalized_clock,
        },
        profile,
    ))
}
/// Only actual typed completion supplies results; a recorded prefix supplies identity only.
pub fn solo_archive(
    outcome: &NativeGameplayResult<Option<CompletedPlayResult>>,
    capture: Option<&LiveReplayCapture>,
    profile: &GaugeProfile,
) -> NativeGameplayResult<Option<ResultArchive>> {
    let Some(capture) = capture else {
        return Ok(None);
    };
    let result = match outcome {
        Ok(result) => result.as_ref(),
        Err(error) => error
            .downcast_ref::<CompletedSoloPublicationError>()
            .map(|error| &error.result),
    };
    let Some(result) = result else {
        return Ok(None);
    };
    Ok(Some(ResultArchive::from_completed(
        &[(PlayerId(1), *result)],
        &[identity(PlayerId(1), capture, profile)?],
    )?))
}
/// Borrowed business data; no concrete competition or device owner crosses this port.
pub struct ArchiveMember<'a> {
    pub player: PlayerId,
    pub capture: Option<&'a LiveReplayCapture>,
    pub profile: &'a GaugeProfile,
}
/// The whole original capture roster is matched by ID, never positional zip.
pub fn cohort_archive(
    outcome: &NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>>,
    states: &[ArchiveMember<'_>],
) -> NativeGameplayResult<Option<ResultArchive>> {
    if states.iter().all(|state| state.capture.is_none()) {
        return Ok(None);
    }
    let results = match outcome {
        Ok(results) => results.as_deref(),
        Err(error) => error
            .downcast_ref::<CompletedLocalPublicationError>()
            .map(|error| error.results.as_slice()),
    };
    let Some(results) = results else {
        return Ok(None);
    };
    if states.is_empty()
        || states.len() > crate::result_archive::MAX_PLAYERS
        || results.len() != states.len()
    {
        return Err("completed archive roster mismatch".into());
    }
    let mut identities = Vec::new();
    identities.try_reserve_exact(states.len())?;
    for state in states {
        let capture = state
            .capture
            .ok_or("completed archive member missing capture")?;
        identities.push(identity(state.player, capture, state.profile)?);
    }
    Ok(Some(ResultArchive::from_completed(results, &identities)?))
}
/// All replay effects precede the archive effect, even when an earlier stage failed.
/// Original boxed owner errors are returned unchanged after both save attempts.
pub fn finalize_completed_save<T>(
    outcome: NativeGameplayResult<T>,
    cleanup: NativeGameplayResult<()>,
    archive: NativeGameplayResult<Option<ResultArchive>>,
    save_replays: impl FnOnce() -> NativeGameplayResult<()>,
    save_archive: impl FnOnce(&ResultArchive) -> NativeGameplayResult<()>,
) -> NativeGameplayResult<()> {
    let replay = save_replays();
    let archived = match archive {
        Ok(Some(archive)) => save_archive(&archive),
        Ok(None) => Ok(()),
        Err(error) => Err(error),
    };
    outcome?;
    cleanup?;
    replay?;
    archived?;
    Ok(())
}
