//! Pure numeric attachment selection; native enumeration stays in the adapter.
use crate::local_players::{PlayerId, MAX_LOCAL_PLAYERS};
use beatkernel::input::DeviceId;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentError {
    InvalidAssignment,
    Ambiguous,
    Unusable,
    Changed,
}
impl std::fmt::Display for AttachmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidAssignment => "invalid or duplicate local input assignment",
            Self::Ambiguous => "ambiguous local input attachment",
            Self::Unusable => "incapable or aliased local input attachment",
            Self::Changed => "assigned input attachment retired or reconnected",
        })
    }
}
impl std::error::Error for AttachmentError {}

/// Cold setup resolution. Missing attachments may still arrive; invalid or
/// ambiguous metadata refuses. Device order follows requested player order.
pub fn resolve(
    requested: &[(PlayerId, u64)],
    attached: &[(Option<u64>, DeviceId, bool)],
) -> Result<Option<Vec<DeviceId>>, AttachmentError> {
    if requested.len() > MAX_LOCAL_PLAYERS {
        return Err(AttachmentError::InvalidAssignment);
    }
    let mut players = HashSet::new();
    let mut keys = HashSet::new();
    let mut ids = HashSet::new();
    let mut selected = Vec::with_capacity(requested.len());
    let mut missing = false;
    for (player, key) in requested {
        if player.0 == 0 || *key == 0 || !players.insert(*player) || !keys.insert(*key) {
            return Err(AttachmentError::InvalidAssignment);
        }
        let mut matches = attached.iter().filter(|(id, _, _)| *id == Some(*key));
        let Some((_, id, usable)) = matches.next() else {
            missing = true;
            continue;
        };
        if matches.next().is_some() {
            return Err(AttachmentError::Ambiguous);
        }
        if !usable || id.0 == 0 || !ids.insert(*id) {
            return Err(AttachmentError::Unusable);
        }
        selected.push(*id);
    }
    Ok(if missing { None } else { Some(selected) })
}

/// Hot roster verification without allocating. Each adapter supplies a stable
/// lookup returning at most two copied runtime ID/usability candidates per key.
/// Missing/changed identities are deferred until every assignment is checked,
/// preserving cold-resolution refusal precedence for malformed metadata.
pub fn verify(
    requested: &[(PlayerId, u64)],
    selected: &[DeviceId],
    mut lookup: impl FnMut(u64) -> [Option<(DeviceId, bool)>; 2],
) -> Result<(), AttachmentError> {
    if requested.len() > MAX_LOCAL_PLAYERS {
        return Err(AttachmentError::InvalidAssignment);
    }
    let mut actual = [DeviceId(0); MAX_LOCAL_PLAYERS];
    let mut changed = selected.len() != requested.len();
    for (index, (player, key)) in requested.iter().enumerate() {
        if player.0 == 0
            || *key == 0
            || requested[..index]
                .iter()
                .any(|(p, k)| p == player || k == key)
        {
            return Err(AttachmentError::InvalidAssignment);
        }
        let [first, second] = lookup(*key);
        let Some((id, usable)) = first else {
            changed = true;
            continue;
        };
        if second.is_some() {
            return Err(AttachmentError::Ambiguous);
        }
        if !usable || id.0 == 0 || actual[..index].contains(&id) {
            return Err(AttachmentError::Unusable);
        }
        actual[index] = id;
        changed |= selected.get(index) != Some(&id);
    }
    if changed {
        Err(AttachmentError::Changed)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lookup(
        attached: &[(Option<u64>, DeviceId, bool)],
        key: u64,
    ) -> [Option<(DeviceId, bool)>; 2] {
        let mut matches = attached
            .iter()
            .filter(|candidate| candidate.0 == Some(key))
            .map(|candidate| (candidate.1, candidate.2));
        [matches.next(), matches.next()]
    }
    #[test]
    fn borrowed_roster_preserves_request_order_and_full_width_identities() {
        let requested = [(PlayerId(u32::MAX), u64::MAX), (PlayerId(3), 4)];
        let attached = [
            (None, DeviceId(7), true),
            (Some(4), DeviceId(9), true),
            (Some(u64::MAX), DeviceId(u64::MAX), true),
        ];
        let selected = resolve(&requested, &attached).unwrap().unwrap();
        assert_eq!(selected, [DeviceId(u64::MAX), DeviceId(9)]);
        assert_eq!(
            verify(&requested, &selected, |key| lookup(&attached, key)),
            Ok(())
        );
        assert_eq!(
            verify(&requested, &[selected[1], selected[0]], |key| lookup(
                &attached, key
            )),
            Err(AttachmentError::Changed)
        );
        assert_eq!(
            verify(&requested, &selected[..1], |key| lookup(&attached, key)),
            Err(AttachmentError::Changed)
        );
    }
    #[test]
    fn hot_refusals_keep_cold_error_precedence_after_missing_or_changed_members() {
        let requested = [(PlayerId(1), 11), (PlayerId(2), 22)];
        let selected = [DeviceId(1), DeviceId(2)];
        for attached in [
            vec![(Some(22), DeviceId(2), false)],
            vec![
                (Some(11), DeviceId(99), true),
                (Some(22), DeviceId(99), true),
            ],
            vec![(Some(22), DeviceId(2), true), (Some(22), DeviceId(3), true)],
        ] {
            let cold = resolve(&requested, &attached).unwrap_err();
            assert_eq!(
                verify(&requested, &selected, |key| lookup(&attached, key)),
                Err(cold)
            );
        }
        assert_eq!(
            verify(&requested, &selected, |_| [None, None]),
            Err(AttachmentError::Changed)
        );
        let invalid = [(PlayerId(1), 11), (PlayerId(1), 22)];
        assert_eq!(
            verify(&invalid, &selected, |_| [None, None]),
            Err(AttachmentError::InvalidAssignment)
        );
    }
    #[test]
    fn hot_verification_matches_cold_resolution_over_generated_small_rosters() {
        let mut seed = 0x79f8_c25a_174e_984du64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            seed >> 32
        };
        for _ in 0..20_000 {
            let requested: Vec<_> = (0..next() % 6)
                .map(|_| (PlayerId((next() % 7) as u32), next() % 7))
                .collect();
            let attached: Vec<_> = (0..next() % 9)
                .map(|_| {
                    (
                        if next() % 4 == 0 {
                            None
                        } else {
                            Some(next() % 7)
                        },
                        DeviceId(next() % 7),
                        next() % 3 != 0,
                    )
                })
                .collect();
            let selected: Vec<_> = (0..next() % 7).map(|_| DeviceId(next() % 7)).collect();
            let expected = match resolve(&requested, &attached) {
                Ok(Some(ids)) if ids == selected => Ok(()),
                Ok(_) => Err(AttachmentError::Changed),
                Err(error) => Err(error),
            };
            assert_eq!(
                verify(&requested, &selected, |key| lookup(&attached, key)),
                expected
            );
        }
    }

    #[test]
    fn supported_roster_capacity_is_checked_before_lookup_or_stack_indexing() {
        let requested: Vec<_> = (1..=MAX_LOCAL_PLAYERS as u32 + 1)
            .map(|id| (PlayerId(id), u64::from(id)))
            .collect();
        assert_eq!(
            resolve(&requested, &[]),
            Err(AttachmentError::InvalidAssignment)
        );
        assert_eq!(
            verify(&requested, &[], |_| panic!(
                "oversized roster must not query devices"
            )),
            Err(AttachmentError::InvalidAssignment)
        );
        let supported = &requested[..MAX_LOCAL_PLAYERS];
        let selected: Vec<_> = supported.iter().map(|(_, key)| DeviceId(*key)).collect();
        assert_eq!(
            verify(supported, &selected, |key| [
                Some((DeviceId(key), true)),
                None
            ]),
            Ok(())
        );
    }
}
