use super::*;
use crate::{local_players::PlayerId, multiplayer::Progress};
use std::sync::Arc;

// Associated errors intentionally implement no error, formatting or clone traits.
struct Token(Arc<u8>);
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    Deliver,
    Cleanup,
    Drain,
}
struct Port {
    effects: Vec<Effect>,
    failures: [Option<Token>; 3],
    pointers: Vec<usize>,
    delivered: Vec<Vec<MemberProgress>>,
}
impl CompetitionTerminalPort for Port {
    type Error = Token;
    fn deliver(&mut self, members: &[MemberProgress]) -> Result<(), Token> {
        self.effects.push(Effect::Deliver);
        self.pointers.push(members.as_ptr() as usize);
        self.delivered.push(members.to_vec());
        match self.failures[0].take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn cleanup(&mut self) -> Result<(), Token> {
        self.effects.push(Effect::Cleanup);
        match self.failures[1].take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn drain(&mut self) -> Result<(), Token> {
        self.effects.push(Effect::Drain);
        match self.failures[2].take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
fn port(mask: u8) -> (Port, [Arc<u8>; 3]) {
    let identities = [Arc::new(31), Arc::new(47), Arc::new(83)];
    let failures = std::array::from_fn(|index| {
        if mask & (1 << index) != 0 {
            Some(Token(identities[index].clone()))
        } else {
            None
        }
    });
    (
        Port {
            effects: Vec::new(),
            failures,
            pointers: Vec::new(),
            delivered: Vec::new(),
        },
        identities,
    )
}
fn members() -> [MemberProgress; 2] {
    [
        MemberProgress {
            player: PlayerId(u32::MAX),
            progress: Progress {
                song_ns: 9_007_199_254_740_993,
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
            },
        },
        MemberProgress {
            player: PlayerId(7),
            progress: Progress {
                song_ns: 604_800_000_000_001,
                hits: 17,
                misses: 3,
                combo: 9,
                max_combo: 15,
            },
        },
    ]
}
fn original<T>(result: &Result<T, Token>, expected: Option<&Arc<u8>>) {
    match (result, expected) {
        (Ok(_), None) => {}
        (Err(Token(actual)), Some(expected)) => assert!(Arc::ptr_eq(actual, expected)),
        _ => panic!("terminal outcome must preserve each original refusal independently"),
    }
}
fn first(mask: u8, identities: &[Arc<u8>; 3]) -> Option<&Arc<u8>> {
    (0..3)
        .find(|index| mask & (1 << index) != 0)
        .map(|index| &identities[index])
}

#[test]
fn every_send_failure_mask_attempts_all_effects_and_preserves_each_original_error() {
    for mask in 0..8 {
        let (mut port, identities) = port(mask);
        let rows = members();
        let outcome = finalize_terminal(&mut port, DeliveryIntent::Send(&rows));
        assert_eq!(
            port.effects,
            [Effect::Deliver, Effect::Cleanup, Effect::Drain]
        );
        assert_eq!(port.pointers, [rows.as_ptr() as usize]);
        assert_eq!(port.delivered.len(), 1);
        assert_eq!(port.delivered[0].as_slice(), rows.as_slice());
        original(
            &outcome.delivery,
            if mask & 1 != 0 {
                Some(&identities[0])
            } else {
                None
            },
        );
        if mask & 1 == 0 {
            assert!(matches!(&outcome.delivery, Ok(DeliveryStatus::Accepted)));
        }
        original(
            &outcome.cleanup,
            if mask & 2 != 0 {
                Some(&identities[1])
            } else {
                None
            },
        );
        original(
            &outcome.drain,
            if mask & 4 != 0 {
                Some(&identities[2])
            } else {
                None
            },
        );
        assert_eq!(outcome.has_failed(), mask != 0);
        original(&outcome.into_result(), first(mask, &identities));
    }
}

#[test]
fn unobserved_skip_never_delivers_but_attempts_cleanup_and_drain_for_all_masks() {
    for mask in [0, 2, 4, 6] {
        let (mut port, identities) = port(mask);
        let outcome = finalize_terminal(&mut port, DeliveryIntent::Skip);
        assert_eq!(port.effects, [Effect::Cleanup, Effect::Drain]);
        assert!(port.pointers.is_empty());
        assert!(port.delivered.is_empty());
        original(&outcome.delivery, None);
        assert!(matches!(&outcome.delivery, Ok(DeliveryStatus::Skipped)));
        original(
            &outcome.cleanup,
            if mask & 2 != 0 {
                Some(&identities[1])
            } else {
                None
            },
        );
        original(
            &outcome.drain,
            if mask & 4 != 0 {
                Some(&identities[2])
            } else {
                None
            },
        );
        assert_eq!(outcome.has_failed(), mask != 0);
        original(&outcome.into_result(), first(mask, &identities));
    }
}

#[test]
fn preparation_refusal_dominates_all_cleanup_masks_without_sending_any_rows() {
    for mask in [0, 2, 4, 6] {
        let (mut port, identities) = port(mask);
        let preparation = Arc::new(101);
        let outcome = finalize_terminal(
            &mut port,
            DeliveryIntent::Refuse(Token(preparation.clone())),
        );
        assert_eq!(port.effects, [Effect::Cleanup, Effect::Drain]);
        assert!(port.pointers.is_empty());
        assert!(port.delivered.is_empty());
        original(&outcome.delivery, Some(&preparation));
        original(
            &outcome.cleanup,
            if mask & 2 != 0 {
                Some(&identities[1])
            } else {
                None
            },
        );
        original(
            &outcome.drain,
            if mask & 4 != 0 {
                Some(&identities[2])
            } else {
                None
            },
        );
        assert!(outcome.has_failed());
        original(&outcome.into_result(), Some(&preparation));
    }
}

#[test]
fn empty_explicit_room_cancellation_is_a_delivery_not_an_unobserved_skip() {
    let (mut send, _) = port(0);
    let empty: [MemberProgress; 0] = [];
    let sent = finalize_terminal(&mut send, DeliveryIntent::Send(&empty));
    assert_eq!(
        send.effects,
        [Effect::Deliver, Effect::Cleanup, Effect::Drain]
    );
    assert_eq!(send.delivered.len(), 1);
    assert!(send.delivered[0].is_empty());
    assert_eq!(send.pointers, [empty.as_ptr() as usize]);
    assert!(matches!(&sent.delivery, Ok(DeliveryStatus::Accepted)));
    assert!(!sent.has_failed());
    assert!(sent.into_result().is_ok());

    let (mut skipped, _) = port(0);
    let skipped_outcome = finalize_terminal(&mut skipped, DeliveryIntent::Skip);
    assert_eq!(skipped.effects, [Effect::Cleanup, Effect::Drain]);
    assert!(skipped.delivered.is_empty());
    assert!(matches!(
        &skipped_outcome.delivery,
        Ok(DeliveryStatus::Skipped)
    ));
    assert!(skipped_outcome.into_result().is_ok());
}

#[test]
fn cleanup_failure_cannot_erase_successful_delivery_or_prevent_later_drain() {
    let (mut port, identities) = port(6);
    let rows = members();
    let outcome = finalize_terminal(&mut port, DeliveryIntent::Send(&rows));
    assert_eq!(
        port.effects,
        [Effect::Deliver, Effect::Cleanup, Effect::Drain]
    );
    original(&outcome.delivery, None);
    assert!(matches!(&outcome.delivery, Ok(DeliveryStatus::Accepted)));
    original(&outcome.cleanup, Some(&identities[1]));
    original(&outcome.drain, Some(&identities[2]));
    assert_eq!(port.delivered[0].as_slice(), rows.as_slice());
    original(&outcome.into_result(), Some(&identities[1]));
}
