//! Deferred portable publication policy using scalar destinations and opaque errors.
use super::*;
use crate::{
    local_players::PlayerId,
    result_archive::{decode_archive, encode_archive},
    result_archive::member_fixtures::{whole, invalid_later},
};
use std::{cell::RefCell, sync::Arc};
#[derive(Debug, PartialEq, Eq)]
struct Destination(u32);
// Intentionally implements neither std::error::Error nor Display/Debug/Clone.
struct Opaque(Arc<usize>);
#[derive(Debug, PartialEq, Eq)]
enum Operation {
    Prepare(Option<PlayerId>),
    Write(Destination),
}
struct Port {
    operations: RefCell<Vec<Operation>>,
    writes: Vec<(Destination, Vec<u8>)>,
    aliases: Vec<(Option<PlayerId>, Destination)>,
    prepare_refusal: Option<Option<PlayerId>>,
    prepare_error: Arc<usize>,
    write_errors: Vec<Option<Arc<usize>>>,
}
impl Default for Port {
    fn default() -> Self {
        Self {
            operations: RefCell::new(Vec::new()),
            writes: Vec::new(),
            aliases: Vec::new(),
            prepare_refusal: None,
            prepare_error: Arc::new(77),
            write_errors: Vec::new(),
        }
    }
}
impl ResultArchivePublicationPort for Port {
    type Destination = Destination;
    type Error = Opaque;
    fn destination(&self, player: Option<PlayerId>) -> Result<Destination, Opaque> {
        self.operations
            .borrow_mut()
            .push(Operation::Prepare(player));
        if self.prepare_refusal == Some(player) {
            return Err(Opaque(self.prepare_error.clone()));
        }
        Ok(self
            .aliases
            .iter()
            .find(|(id, _)| *id == player)
            .map(|(_, token)| Destination(token.0))
            .unwrap_or(Destination(player.map_or(0, |player| player.0))))
    }
    fn create_new(&mut self, destination: &Destination, bytes: &[u8]) -> Result<(), Opaque> {
        self.operations
            .borrow_mut()
            .push(Operation::Write(Destination(destination.0)));
        let index = self.writes.len();
        self.writes
            .push((Destination(destination.0), bytes.to_vec()));
        match self.write_errors.get(index).and_then(Option::as_ref) {
            Some(error) => Err(Opaque(error.clone())),
            None => Ok(()),
        }
    }
}
#[test]
fn every_portable_roster_prepares_before_writes_and_publishes_exact_linear_member_payloads() {
    for count in 1..=64 {
        let archive = whole(count);
        let mut port = Port::default();
        assert!(publish_archive_set(&archive, &mut port).is_ok());
        let mut expected = vec![Operation::Prepare(None)];
        expected.extend(
            archive
                .entries()
                .iter()
                .map(|row| Operation::Prepare(Some(row.player))),
        );
        expected.push(Operation::Write(Destination(0)));
        expected.extend(
            archive
                .entries()
                .iter()
                .map(|row| Operation::Write(Destination(row.player.0))),
        );
        assert_eq!(*port.operations.borrow(), expected);
        assert_eq!(port.writes.len(), count as usize + 1);
        assert_eq!(port.writes[0].1, encode_archive(&archive).unwrap());
        for (index, row) in archive.entries().iter().enumerate() {
            assert_eq!(port.writes[index + 1].0, Destination(row.player.0));
            assert_eq!(
                decode_archive(&port.writes[index + 1].1).unwrap().entries(),
                std::slice::from_ref(row)
            );
        }
        assert_eq!(
            port.writes[1..]
                .iter()
                .map(|(_, bytes)| bytes.len())
                .sum::<usize>(),
            port.writes[0].1.len() + 16 * (count as usize - 1)
        );
    }
}
#[test]
fn invalid_whole_table_refuses_before_accessing_even_pure_adapter_preparation() {
    let mut port = Port::default();
    assert!(matches!(
        publish_archive_set(&invalid_later(), &mut port),
        Err(PublicationError::Archive(_))
    ));
    assert!(port.operations.borrow().is_empty());
    assert!(port.writes.is_empty());
}
#[test]
fn base_and_later_destination_refusals_preserve_opaque_error_and_have_zero_writes() {
    let archive = whole(3);
    for refusal in [None, Some(archive.entries()[2].player)] {
        let mut port = Port {
            prepare_refusal: Some(refusal),
            ..Default::default()
        };
        match publish_archive_set(&archive, &mut port) {
            Err(PublicationError::Destination(error)) => {
                assert!(Arc::ptr_eq(&error.0, &port.prepare_error))
            }
            _ => panic!("destination refusal changed error kind"),
        }
        assert!(port.writes.is_empty());
        assert!(
            port.operations
                .borrow()
                .iter()
                .all(|operation| matches!(operation, Operation::Prepare(_)))
        );
    }
}
#[test]
fn duplicate_whole_member_or_member_member_tokens_refuse_before_any_create() {
    let archive = whole(3);
    let first = archive.entries()[0].player;
    let second = archive.entries()[1].player;
    for aliases in [
        vec![(Some(first), Destination(0))],
        vec![(Some(second), Destination(first.0))],
    ] {
        let mut port = Port {
            aliases,
            ..Default::default()
        };
        assert!(matches!(
            publish_archive_set(&archive, &mut port),
            Err(PublicationError::DuplicateDestination)
        ));
        assert!(port.writes.is_empty());
        assert!(
            port.operations
                .borrow()
                .iter()
                .all(|operation| matches!(operation, Operation::Prepare(_)))
        );
    }
}
#[test]
fn every_storage_failure_subset_attempts_all_writes_and_returns_first_exact_opaque_token() {
    let archive = whole(3);
    let tokens: [Arc<usize>; 4] = std::array::from_fn(Arc::new);
    for mask in 1..16 {
        let errors = (0..4)
            .map(|index| {
                if mask & (1 << index) != 0 {
                    Some(tokens[index].clone())
                } else {
                    None
                }
            })
            .collect();
        let mut port = Port {
            write_errors: errors,
            ..Default::default()
        };
        match publish_archive_set(&archive, &mut port) {
            Err(PublicationError::Storage(error)) => {
                let first = (0..4).find(|index| mask & (1 << index) != 0).unwrap();
                assert!(Arc::ptr_eq(&error.0, &tokens[first]));
            }
            _ => panic!("storage refusal changed error kind"),
        }
        assert_eq!(port.writes.len(), 4);
        assert_eq!(
            port.writes
                .iter()
                .map(|(destination, _)| Destination(destination.0))
                .collect::<Vec<_>>(),
            [
                Destination(0),
                Destination(u32::MAX),
                Destination(u32::MAX - 17),
                Destination(u32::MAX - 34)
            ]
        );
    }
}
