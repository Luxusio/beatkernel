//! Portable staging and publication policy with opaque destinations and errors.
use crate::{
    local_players::PlayerId,
    result_archive::{ResultArchive, ArchiveError, encode_archive},
};
use std::collections::TryReserveError;

pub trait ResultArchivePublicationPort {
    type Destination: Eq;
    type Error;
    /// Effect-free preparation. None identifies the whole set; Some preserves the original member ID.
    fn destination(&self, player: Option<PlayerId>) -> Result<Self::Destination, Self::Error>;
    /// Exclusively create the prepared destination without overwriting an existing value.
    fn create_new(
        &mut self,
        destination: &Self::Destination,
        bytes: &[u8],
    ) -> Result<(), Self::Error>;
}
#[derive(Debug)]
pub enum PublicationError<E> {
    Archive(ArchiveError),
    Allocation(TryReserveError),
    Destination(E),
    DuplicateDestination,
    Storage(E),
}
/// Validate and stage everything before writes, then attempt all writes in original roster order.
/// Associated errors require no trait-object, formatting, cloning or OS representation.
pub fn publish_archive_set<P: ResultArchivePublicationPort>(
    archive: &ResultArchive,
    port: &mut P,
) -> Result<(), PublicationError<P::Error>> {
    let whole = encode_archive(archive).map_err(PublicationError::Archive)?;
    let mut publications = Vec::new();
    publications
        .try_reserve_exact(archive.entries().len() + 1)
        .map_err(PublicationError::Allocation)?;
    let destination = port
        .destination(None)
        .map_err(PublicationError::Destination)?;
    publications.push((destination, whole));
    for entry in archive.entries() {
        let destination = port
            .destination(Some(entry.player))
            .map_err(PublicationError::Destination)?;
        if publications.iter().any(|(prior, _)| prior == &destination) {
            return Err(PublicationError::DuplicateDestination);
        }
        let member = archive
            .for_player(entry.player)
            .map_err(PublicationError::Archive)?;
        let bytes = encode_archive(&member).map_err(PublicationError::Archive)?;
        publications.push((destination, bytes));
    }
    let mut first_error = None;
    for (destination, bytes) in publications {
        if let Err(error) = port.create_new(&destination, &bytes) {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(PublicationError::Storage(error)),
        None => Ok(()),
    }
}
#[cfg(test)]
#[path = "result_archive_publication_fixtures.rs"]
mod fixtures;
