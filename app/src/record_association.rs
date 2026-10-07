//! Exact historical identity association, without filesystem or live-proof inference.
use crate::{
    local_players::PlayerId,
    result_archive::{ResultArchive, ArchiveEntry},
};
use beatkernel::replay::ReplayHeader;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssociationError {
    InvalidPlayer,
    Missing,
    Ambiguous,
}
impl std::fmt::Display for AssociationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidPlayer => "historical player ID must be nonzero",
            Self::Missing => "historical archive has no matching recording/player",
            Self::Ambiguous => "historical archive requires an explicit player ID",
        })
    }
}
impl std::error::Error for AssociationError {}
pub fn associate<'a>(
    archive: &'a ResultArchive,
    header: &ReplayHeader,
    player: Option<PlayerId>,
) -> Result<&'a ArchiveEntry, AssociationError> {
    if player.is_some_and(|player| player.0 == 0) {
        return Err(AssociationError::InvalidPlayer);
    }
    let mut matches = archive.entries().iter().filter(|entry| {
        entry.header == *header && player.is_none_or(|player| entry.player == player)
    });
    let entry = matches.next().ok_or(AssociationError::Missing)?;
    if matches.next().is_some() {
        return Err(AssociationError::Ambiguous);
    }
    Ok(entry)
}
