//! Shared accepted-room metadata projection, without transport or start consumption.
use crate::{
    multiplayer_group::GroupPrefix,
    multiplayer_group_rooms::GroupRoomMember,
    multiplayer_room_play::RoomPlayClient,
    room_network_model::{RoomSnapshot, RoomRoster, RoomReceipts},
};
use std::{fmt, io, sync::Arc};
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
fn allocation(error: impl fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
fn copy_slice<T: Clone>(slice: &[T]) -> io::Result<Vec<T>> {
    let mut result = Vec::new();
    result.try_reserve_exact(slice.len()).map_err(allocation)?;
    result.extend_from_slice(slice);
    Ok(result)
}
pub(crate) fn refresh(snapshot: &mut RoomSnapshot, session: &RoomPlayClient) -> io::Result<bool> {
    let mut changed = false;
    refresh_with_changed(snapshot, session, &mut changed)?;
    Ok(changed)
}
pub(crate) fn refresh_with_changed(
    snapshot: &mut RoomSnapshot,
    session: &RoomPlayClient,
    changed: &mut bool,
) -> io::Result<()> {
    let participant = session.participant();
    if snapshot.participant != participant {
        snapshot.participant = participant;
        *changed = true;
    }
    if let Some(room) = session.room() {
        let room_changed = snapshot.room.as_ref().is_none_or(|old| {
            old.phase != room.phase
                || old.deadline_ns != room.deadline_ns
                || old.members.as_slice() != room.members
        });
        if room_changed {
            let revision = snapshot
                .revision
                .checked_add(1)
                .ok_or_else(|| invalid("native room revision exhausted"))?;
            let mut members = Vec::new();
            members
                .try_reserve_exact(room.members.len())
                .map_err(allocation)?;
            for member in room.members {
                members.push(GroupRoomMember {
                    id: member.id,
                    players: copy_slice(&member.players)?,
                    prepared: member.prepared,
                });
            }
            snapshot.room = Some(Arc::new(RoomRoster {
                members,
                phase: room.phase,
                deadline_ns: room.deadline_ns,
            }));
            snapshot.revision = revision;
            *changed = true;
        }
        for member in room.members {
            if let Some(prefix) = session.peer_progress(member.id) {
                let old = snapshot.peers.iter().position(|(id, _)| *id == member.id);
                if old.is_none_or(|index| snapshot.peers[index].1.sequence != prefix.sequence) {
                    let next = Arc::new(GroupPrefix {
                        sequence: prefix.sequence,
                        final_prefix: prefix.final_prefix,
                        members: copy_slice(&prefix.members)?,
                    });
                    if let Some(index) = old {
                        snapshot.peers[index].1 = next;
                    } else {
                        snapshot.peers.try_reserve(1).map_err(allocation)?;
                        snapshot.peers.push((member.id, next));
                        // Different peers can publish first in any order; presentation stays roster-ordered.
                        snapshot.peers.sort_by_key(|(id, _)| {
                            room.members.iter().position(|member| member.id == *id)
                        });
                    }
                    *changed = true;
                }
            }
        }
    }
    let receipts = RoomReceipts {
        local_final_written: snapshot.receipts.local_final_written || session.local_final_written(),
        local_final_acknowledged: snapshot.receipts.local_final_acknowledged
            || session.local_final_acknowledged(),
        progress_complete: snapshot.receipts.progress_complete || session.progress_complete(),
        drain_complete: snapshot.receipts.drain_complete || session.drain_complete(),
    };
    if snapshot.receipts != receipts {
        snapshot.receipts = receipts;
        *changed = true;
    }
    Ok(())
}
#[cfg(test)]
#[path = "room_snapshot_projection_fixtures.rs"]
mod room_snapshot_projection_fixtures;
