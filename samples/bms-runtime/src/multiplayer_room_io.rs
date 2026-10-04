//! Bounded caller-driven room I/O with original elapsed stream observations.
//! The caller owns clock origin, waiting, deadlines and stream disposal.

use crate::multiplayer_protocol::OutboundFrame;
use crate::multiplayer_group::{GroupPrefix, MemberProgress};
use crate::multiplayer_room_play::RoomPlayClient;
use crate::multiplayer_room_wire::RoomFrameDecoder;
use crate::multiplayer_start::StartSchedule;
use crate::multiplayer_rooms::ParticipantId;
use std::{
    fmt,
    io::{self, Read, Write},
};

fn protocol_error(error: impl fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

/// One pending frame and one incremental decoder around the actual common
/// admission/clock/start/progress owner. Streams must be nonblocking or deadline-bounded;
/// this driver creates no timers, retries, transport tasks or acknowledgement.
pub struct RoomPlayIo<S: Read + Write> {
    session: RoomPlayClient,
    stream: S,
    pending: Option<OutboundFrame>,
    offset: usize,
    decoder: RoomFrameDecoder,
    scratch: [u8; 4096],
    last_observed: Option<i64>,
    failure: Option<(io::ErrorKind, String)>,
}

impl<S: Read + Write> RoomPlayIo<S> {
    /// Transfer a session with no externally outstanding OutboundFrame. A fresh
    /// client is the usual input; this driver cannot recover bytes already
    /// handed to a different transport owner by `poll_write`.
    pub fn new(session: RoomPlayClient, stream: S) -> Self {
        let mut owner = Self {
            session,
            stream,
            pending: None,
            offset: 0,
            decoder: RoomFrameDecoder::new(),
            scratch: [0; 4096],
            last_observed: None,
            failure: None,
        };
        if owner.session.leave_written() {
            owner.stop();
        }
        owner
    }

    pub fn session(&self) -> &RoomPlayClient {
        &self.session
    }

    /// Consume the driver without accessing the stream. Final close/drop and
    /// any transport-specific cleanup remain the caller's responsibility.
    pub fn into_stream(self) -> S {
        self.stream
    }

    fn ensure_live(&self) -> io::Result<()> {
        match &self.failure {
            Some((kind, message)) => Err(io::Error::new(*kind, message.clone())),
            None => Ok(()),
        }
    }

    /// A local phase or busy refusal does not stop a healthy transport owner.
    pub fn request_seal(&mut self) -> io::Result<()> {
        self.ensure_live()?;
        self.session
            .request_seal()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
    }

    pub fn request_ready(&mut self) -> io::Result<()> {
        self.ensure_live()?;
        self.session
            .request_ready()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
    }

    pub fn request_leave(&mut self) -> io::Result<()> {
        self.ensure_live()?;
        self.session
            .request_leave()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
    }

    /// Queue actual committed member progress; local admission refusals retain
    /// the healthy stream and do not manufacture a transport write receipt.
    pub fn publish_progress(
        &mut self,
        members: &[MemberProgress],
        final_prefix: bool,
    ) -> io::Result<()> {
        self.ensure_live()?;
        self.session
            .publish_progress(members, final_prefix)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
    }

    pub fn peer_progress(&self, participant: ParticipantId) -> Option<&GroupPrefix> {
        self.session.peer_progress(participant)
    }

    pub fn local_final_written(&self) -> bool {
        self.session.local_final_written()
    }
    pub fn local_final_acknowledged(&self) -> bool {
        self.session.local_final_acknowledged()
    }
    pub fn peer_final_ack_written(&self, participant: ParticipantId) -> bool {
        self.session.peer_final_ack_written(participant)
    }
    /// Local completion is not permission to close the entire room.
    pub fn progress_complete(&self) -> bool {
        self.session.progress_complete()
    }

    /// Return only the common owner's once-only committed software schedule.
    pub fn take_schedule(&mut self) -> io::Result<Option<StartSchedule>> {
        self.ensure_live()?;
        Ok(self.session.take_schedule())
    }

    /// Fence I/O and schedule delivery, retaining accepted session history and
    /// the first failure. This does not close a caller-owned stream.
    pub fn stop(&mut self) {
        self.failure.get_or_insert_with(|| {
            (
                io::ErrorKind::NotConnected,
                "room stream driver stopped".to_owned(),
            )
        });
        self.session.stop();
        self.pending = None;
        self.offset = 0;
    }

    fn observe<F: FnMut() -> io::Result<i64>>(&mut self, now: &mut F) -> io::Result<i64> {
        let value = now()?;
        if value < 0 || self.last_observed.is_some_and(|previous| value < previous) {
            return Err(protocol_error(
                "room stream clock is negative or regressing",
            ));
        }
        self.last_observed = Some(value);
        Ok(value)
    }

    /// Perform at most one write and one needed-prefix read (up to 4096 bytes).
    /// True means actual bytes moved. Every positive I/O result is timestamped
    /// immediately; complete-frame processing takes a separate observation.
    /// WouldBlock/Interrupted retain offsets without an internal retry. Any
    /// other clock, stream or protocol error permanently fences this owner.
    pub fn step<F: FnMut() -> io::Result<i64>>(&mut self, mut now: F) -> io::Result<bool> {
        self.ensure_live()?;
        let result = self.step_live(&mut now);
        if let Err(error) = &result {
            self.failure = Some((error.kind(), error.to_string()));
            self.stop();
        }
        result
    }

    fn step_live<F: FnMut() -> io::Result<i64>>(&mut self, now: &mut F) -> io::Result<bool> {
        let polled_ns = self.observe(now)?;
        let mut progressed = false;
        if self.pending.is_none() {
            self.pending = self.session.poll_write(polled_ns).map_err(protocol_error)?;
        }
        if let Some(frame) = &self.pending {
            let remaining = frame
                .bytes
                .get(self.offset..)
                .ok_or_else(|| protocol_error("room write offset exceeded frame"))?;
            let remaining_len = remaining.len();
            let frame_len = frame.bytes.len();
            let id = frame.id;
            match self.stream.write(remaining) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "room frame write made no progress",
                    ));
                }
                Ok(count) => {
                    let completed_ns = self.observe(now)?;
                    if count > remaining_len {
                        return Err(protocol_error("room writer exceeded its supplied slice"));
                    }
                    self.offset += count;
                    progressed = true;
                    if self.offset == frame_len {
                        let processing_ns = self.observe(now)?;
                        self.session
                            .written_at(id, completed_ns, processing_ns)
                            .map_err(protocol_error)?;
                        self.pending = None;
                        self.offset = 0;
                        if self.session.leave_written() {
                            self.stop();
                            return Ok(true);
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        let needed = self.decoder.needed().map_err(protocol_error)?;
        let limit = needed.min(self.scratch.len());
        match self.stream.read(&mut self.scratch[..limit]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "room stream ended",
                ));
            }
            Ok(count) => {
                let captured_ns = self.observe(now)?;
                if count > limit {
                    return Err(protocol_error("room reader exceeded its supplied slice"));
                }
                let admitted = self
                    .decoder
                    .push(&self.scratch[..count])
                    .map_err(protocol_error)?;
                if admitted != count {
                    return Err(protocol_error(
                        "room decoder did not admit the requested prefix",
                    ));
                }
                progressed = true;
                if let Some(message) = self.decoder.take().map_err(protocol_error)? {
                    let processing_ns = self.observe(now)?;
                    self.session
                        .receive_at(message, captured_ns, processing_ns)
                        .map_err(protocol_error)?;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
        Ok(progressed)
    }
}
