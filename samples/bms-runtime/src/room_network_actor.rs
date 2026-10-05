//! Caller-driven room IO actor with portable streams, values and clocks.
use crate::{
    multiplayer_room_io::RoomPlayIo,
    multiplayer_group::GroupPrefix,
    multiplayer_group_rooms::GroupRoomMember,
    room_network_model::{
        RoomNetworkOptions, RoomCommand, RoomFailure, RoomReceipts, RoomRoster, RoomOutcome,
        RoomSnapshot,
    },
};
use std::{
    fmt,
    io::{self, Read, Write},
    sync::Arc,
    time::Duration,
};

pub trait RoomNetworkStream: Read + Write {
    fn idle(&mut self, duration: Duration) -> io::Result<()>;
    fn finish(&mut self, timeout: Duration) -> io::Result<()>;
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
fn allocation(error: impl fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
fn nanos(duration: Duration) -> io::Result<i64> {
    i64::try_from(duration.as_nanos()).map_err(|_| invalid("room clock exceeds signed nanoseconds"))
}
fn copy_slice<T: Clone>(slice: &[T]) -> io::Result<Vec<T>> {
    let mut result = Vec::new();
    result.try_reserve_exact(slice.len()).map_err(allocation)?;
    result.extend_from_slice(slice);
    Ok(result)
}

struct Drain {
    deadline_ns: i64,
    requested: bool,
}

/// The same bounded actor is used by the thread and in-memory fixture streams.
pub struct RoomNetworkActor<S: RoomNetworkStream> {
    io: RoomPlayIo<S>,
    options: RoomNetworkOptions,
    setup_deadline: crate::room_setup_wait::RoomDeadline,
    snapshot: RoomSnapshot,
    last_now: Option<i64>,
    drain: Option<Drain>,
    leaving: bool,
    changed: bool,
}
impl<S: RoomNetworkStream> RoomNetworkActor<S> {
    pub fn new(mut io: RoomPlayIo<S>, options: RoomNetworkOptions) -> io::Result<Self> {
        // The public owner validates options before acquiring a stream.
        options.validate()?;
        io.configure_frame_wait(options.frame_timeout.as_nanos() as u64)?;
        let setup_deadline =
            crate::room_setup_wait::RoomDeadline::new(0, options.setup_timeout.as_nanos() as u64)
                .map_err(|_| invalid("native room setup deadline overflow"))?;
        Ok(Self {
            setup_deadline,
            io,
            options,
            snapshot: RoomSnapshot::default(),
            last_now: None,
            drain: None,
            leaving: false,
            changed: false,
        })
    }

    pub fn snapshot(&self) -> &RoomSnapshot {
        &self.snapshot
    }

    /// Consume the dirty notification without copying retained evidence.
    pub fn take_changed_snapshot(&mut self) -> Option<&RoomSnapshot> {
        if !self.changed {
            return None;
        }
        self.changed = false;
        Some(&self.snapshot)
    }
    pub fn idle(&mut self, duration: Duration) -> io::Result<()> {
        self.io.stream_mut().idle(duration)
    }
    pub fn leave_written(&self) -> bool {
        self.io.session().leave_written()
    }

    fn check_now(&self, now: i64) -> io::Result<()> {
        if now < 0 || self.last_now.is_some_and(|previous| now < previous) {
            return Err(invalid("native room observation is negative or regressing"));
        }
        Ok(())
    }

    pub fn command(&mut self, command: RoomCommand, now: i64) -> io::Result<()> {
        self.check_now(now)?;
        if self.finished() || self.leaving {
            return Err(invalid("native room no longer accepts commands"));
        }
        match command {
            RoomCommand::Seal => self.io.request_seal()?,
            RoomCommand::Ready => self.io.request_ready()?,
            RoomCommand::Leave => {
                self.io.request_leave()?;
                self.leaving = true;
            }
            RoomCommand::Publish {
                members,
                final_prefix,
            } => {
                self.io.publish_progress(&members, final_prefix)?;
            }
            RoomCommand::Drain => {
                if self.snapshot.schedule.is_none() || self.drain.is_some() {
                    return Err(invalid(
                        "native room drain requires one committed live owner",
                    ));
                }
                let deadline_ns = now
                    .checked_add(nanos(self.options.drain_timeout)?)
                    .ok_or_else(|| invalid("native room drain deadline overflow"))?;
                let requested = self.io.progress_complete();
                if requested {
                    self.io.request_drain()?;
                }
                self.drain = Some(Drain {
                    deadline_ns,
                    requested,
                });
            }
        }
        self.last_now = Some(now);
        Ok(())
    }

    fn refresh(&mut self) -> io::Result<()> {
        let participant = self.io.session().participant();
        if self.snapshot.participant != participant {
            self.snapshot.participant = participant;
            self.changed = true;
        }
        if let Some(room) = self.io.session().room() {
            let changed = self.snapshot.room.as_ref().is_none_or(|old| {
                old.phase != room.phase
                    || old.deadline_ns != room.deadline_ns
                    || old.members.as_slice() != room.members
            });
            if changed {
                let revision = self
                    .snapshot
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
                self.snapshot.room = Some(Arc::new(RoomRoster {
                    members,
                    phase: room.phase,
                    deadline_ns: room.deadline_ns,
                }));
                self.snapshot.revision = revision;
                self.changed = true;
            }
            for member in room.members {
                if let Some(prefix) = self.io.peer_progress(member.id) {
                    let old = self
                        .snapshot
                        .peers
                        .iter()
                        .position(|(id, _)| *id == member.id);
                    if old.is_none_or(|index| {
                        self.snapshot.peers[index].1.sequence != prefix.sequence
                    }) {
                        let next = Arc::new(GroupPrefix {
                            sequence: prefix.sequence,
                            final_prefix: prefix.final_prefix,
                            members: copy_slice(&prefix.members)?,
                        });
                        if let Some(index) = old {
                            self.snapshot.peers[index].1 = next;
                        } else {
                            self.snapshot.peers.try_reserve(1).map_err(allocation)?;
                            self.snapshot.peers.push((member.id, next));
                            // Different peers can publish first in any order; presentation stays roster-ordered.
                            self.snapshot.peers.sort_by_key(|(id, _)| {
                                room.members.iter().position(|member| member.id == *id)
                            });
                        }
                        self.changed = true;
                    }
                }
            }
        }
        let receipts = RoomReceipts {
            local_final_written: self.snapshot.receipts.local_final_written
                || self.io.local_final_written(),
            local_final_acknowledged: self.snapshot.receipts.local_final_acknowledged
                || self.io.local_final_acknowledged(),
            progress_complete: self.snapshot.receipts.progress_complete
                || self.io.progress_complete(),
            drain_complete: self.snapshot.receipts.drain_complete || self.io.drain_complete(),
        };
        if self.snapshot.receipts != receipts {
            self.snapshot.receipts = receipts;
            self.changed = true;
        }
        if self.snapshot.schedule.is_none() && !self.io.session().leave_written() {
            if let Some(schedule) = self.io.take_schedule()? {
                self.snapshot.schedule = Some(schedule);
                self.changed = true;
            }
        }
        Ok(())
    }

    pub fn finished(&self) -> bool {
        self.io.session().leave_written() || self.io.drain_complete()
    }

    pub fn drive<F: FnMut() -> io::Result<i64>>(&mut self, mut now: F) -> io::Result<bool> {
        if self.finished() {
            return Ok(false);
        }
        let observed = now()?;
        self.check_now(observed)?;
        self.last_now = Some(observed);
        self.refresh()?;
        if self.snapshot.schedule.is_none() && self.setup_deadline.remaining_ns(observed).is_err() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "native room setup deadline expired",
            ));
        }
        if let Some(drain) = &mut self.drain {
            if observed >= drain.deadline_ns {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "native room drain deadline expired",
                ));
            }
            if !drain.requested && self.io.progress_complete() {
                self.io.request_drain()?;
                drain.requested = true;
            }
        }
        let setup_pending = self.snapshot.schedule.is_none();
        let last = &mut self.last_now;
        let result = self.io.step(|| {
            let value = now()?;
            if value < 0 || last.is_some_and(|previous| value < previous) {
                return Err(invalid("native room clock is negative or regressing"));
            }
            *last = Some(value);
            Ok(value)
        });
        // Preserve any accepted prefix even when a later I/O operation failed.
        let refreshed = self.refresh();
        match result {
            Err(error) => Err(error),
            Ok(progressed) => {
                refreshed?;
                // A bounded transport operation can complete across its fixed
                // deadline. Real late receipts remain history, never timely success.
                let completed = self.last_now.unwrap_or(observed);
                if setup_pending && self.setup_deadline.remaining_ns(completed).is_err() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "native room setup deadline expired during I/O",
                    ));
                }
                if self
                    .drain
                    .as_ref()
                    .is_some_and(|drain| completed >= drain.deadline_ns)
                {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "native room drain deadline expired during I/O",
                    ));
                }
                Ok(progressed)
            }
        }
    }

    pub fn finish(mut self, error: Option<RoomFailure>, cancelled: bool) -> RoomOutcome {
        let leave_written = self.io.session().leave_written();
        let refresh_error = self.refresh().err().map(RoomFailure::from);
        let receipts = self.snapshot.receipts;
        self.io.stop();
        let mut stream = self.io.into_stream();
        let cleanup_error = stream
            .finish(self.options.finish_timeout)
            .err()
            .map(RoomFailure::from);
        RoomOutcome {
            cancelled,
            error: error.or(refresh_error),
            cleanup_error,
            receipts,
            leave_written,
        }
    }
}

#[cfg(test)]
#[path = "room_network_actor_fixtures.rs"]
mod fixtures;
