//! Shared BKMP v6 framing and progress state, independent of transport and clocks.
//! Peer progress is self-reported display data, not authenticated scoring.
use crate::multiplayer_clock::{ClockFilter, ClockSample, OffsetEstimate};
use crate::multiplayer_start::{
    StartAgreement, StartMessage, StartPolicy, StartRole, StartSchedule,
};
use std::{collections::VecDeque, fmt};

pub(crate) const MAGIC: &[u8; 4] = b"BKMP";
pub(crate) const VERSION: u16 = 6;
pub(crate) const CLOCK_PROBES: u64 = 8;
pub(crate) const MAX_IDENTITY: usize = 65_536;
pub(crate) const MAX_BODY: usize = MAX_IDENTITY + 7;

/// A local cumulative summary; no implicit grade weighting or ranked-score claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub song_ns: i64,
    pub hits: u64,
    pub misses: u64,
    pub combo: u64,
    pub max_combo: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultiplayerError {
    InvalidOptions,
    Io(String),
    SetupTimeout,
    IoStalled,
    IncompatibleSetup,
    Protocol(String),
    QueueFull,
    Closed,
    WorkerPanicked,
}
impl fmt::Display for MultiplayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for MultiplayerError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultiplayerEvent {
    Connected,
    /// Compatible peers both completely sent and received preparation readiness.
    Ready,
    /// Minimum-delay software offset interval after eight validated probes.
    ClockEstimated(OffsetEstimate),
    /// A committed local session-clock target for the software start call.
    StartScheduled(StartSchedule),
    Progress(Progress),
    /// Peer terminal self-reported prefix, including aborted sessions.
    FinalProgress(Progress),
    /// Exact local final was acknowledged and any parsed peer ack was written.
    FinalAcknowledged,
    Disconnected(MultiplayerError),
}

/// Encode a bounded v6 frame without changing the tag or payload.
pub fn encode_frame(tag: u8, payload: &[u8]) -> Result<Vec<u8>, MultiplayerError> {
    if payload.len() > MAX_IDENTITY {
        return Err(MultiplayerError::Protocol(
            "frame payload exceeds limit".into(),
        ));
    }
    Ok(frame(tag, payload))
}

pub(crate) fn validate_progress(
    previous: Option<Progress>,
    next: Progress,
) -> Result<(), MultiplayerError> {
    if next.combo > next.max_combo
        || next.max_combo > next.hits
        || next.hits.checked_add(next.misses).is_none()
    {
        return Err(MultiplayerError::Protocol(
            "invalid cumulative counts".into(),
        ));
    }
    if let Some(previous) = previous {
        if next.song_ns < previous.song_ns
            || next.hits < previous.hits
            || next.misses < previous.misses
            || next.max_combo < previous.max_combo
        {
            return Err(MultiplayerError::Protocol(
                "progress regression; reconnect after section restart".into(),
            ));
        }
        let added_hits = next.hits - previous.hits;
        if next.combo > previous.combo.saturating_add(added_hits)
            || next.max_combo
                > previous
                    .max_combo
                    .max(previous.combo.saturating_add(added_hits))
            || (next.misses == previous.misses
                && (next.combo != previous.combo + added_hits
                    || next.max_combo != previous.max_combo.max(next.combo)))
        {
            return Err(MultiplayerError::Protocol(
                "inconsistent combo transition".into(),
            ));
        }
    }
    Ok(())
}
pub(crate) fn frame(tag: u8, payload: &[u8]) -> Vec<u8> {
    let len = 7 + payload.len();
    let mut bytes = Vec::with_capacity(4 + len);
    bytes.extend_from_slice(&(len as u32).to_le_bytes());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.push(tag);
    bytes.extend_from_slice(payload);
    bytes
}
pub(crate) fn progress_frame(sequence: u64, progress: Progress) -> Vec<u8> {
    prefix_frame(2, sequence, progress)
}
pub(crate) fn prefix_frame(tag: u8, sequence: u64, progress: Progress) -> Vec<u8> {
    let mut payload = Vec::with_capacity(48);
    payload.extend_from_slice(&sequence.to_le_bytes());
    payload.extend_from_slice(&progress.song_ns.to_le_bytes());
    for value in [
        progress.hits,
        progress.misses,
        progress.combo,
        progress.max_combo,
    ] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    frame(tag, &payload)
}
pub(crate) fn parse_progress(
    payload: &[u8],
    expected: u64,
    previous: Option<Progress>,
) -> Result<Progress, MultiplayerError> {
    if payload.len() != 48 {
        return Err(MultiplayerError::Protocol("invalid progress size".into()));
    }
    let word = |offset: usize| u64::from_le_bytes(payload[offset..offset + 8].try_into().unwrap());
    if word(0) != expected {
        return Err(MultiplayerError::Protocol("invalid sequence".into()));
    }
    let progress = Progress {
        song_ns: i64::from_le_bytes(payload[8..16].try_into().unwrap()),
        hits: word(16),
        misses: word(24),
        combo: word(32),
        max_combo: word(40),
    };
    validate_progress(previous, progress)?;
    Ok(progress)
}

pub(crate) fn start_frame(message: StartMessage) -> Vec<u8> {
    match message {
        StartMessage::ClockReady(preroll) => frame(8, &preroll.to_le_bytes()),
        StartMessage::Propose(time) => frame(9, &time.to_le_bytes()),
        StartMessage::Accept(time) => frame(10, &time.to_le_bytes()),
        StartMessage::Commit(time) => frame(11, &time.to_le_bytes()),
    }
}
pub(crate) fn parse_start_frame(tag: u8, payload: &[u8]) -> Result<StartMessage, MultiplayerError> {
    if !(8..=11).contains(&tag) || payload.len() != 8 {
        return Err(MultiplayerError::Protocol("invalid start frame".into()));
    }
    let time = i64::from_le_bytes(payload.try_into().unwrap());
    Ok(match tag {
        8 => StartMessage::ClockReady(time),
        9 => StartMessage::Propose(time),
        10 => StartMessage::Accept(time),
        _ => StartMessage::Commit(time),
    })
}

/// Finite symmetric software probes over caller-supplied session timestamps.
#[derive(Default)]
pub(crate) struct ClockProbes {
    pub(crate) completed: u64,
    pub(crate) pending_ping: Option<(u64, i64)>,
    pub(crate) peer_sequence: u64,
    pub(crate) peer_send: Option<i64>,
    pub(crate) pending_pong: Option<(u64, i64, i64)>,
    pub(crate) last_receive: i64,
    pub(crate) filter: ClockFilter,
    pub(crate) emitted: bool,
}
impl ClockProbes {
    pub(crate) fn next_ping(&mut self, now: i64) -> Result<Option<Vec<u8>>, MultiplayerError> {
        if self.pending_ping.is_some() || self.completed == CLOCK_PROBES {
            return Ok(None);
        }
        if now < self.last_receive {
            return Err(MultiplayerError::Protocol("probe clock regressed".into()));
        }
        let mut payload = Vec::with_capacity(16);
        payload.extend_from_slice(&self.completed.to_le_bytes());
        payload.extend_from_slice(&now.to_le_bytes());
        self.pending_ping = Some((self.completed, now));
        Ok(Some(frame(6, &payload)))
    }
    pub(crate) fn receive_ping(
        &mut self,
        payload: &[u8],
        now: i64,
    ) -> Result<(), MultiplayerError> {
        if payload.len() != 16 {
            return Err(MultiplayerError::Protocol("invalid clock ping size".into()));
        }
        let sequence = u64::from_le_bytes(payload[..8].try_into().unwrap());
        let sent = i64::from_le_bytes(payload[8..].try_into().unwrap());
        if sequence != self.peer_sequence
            || sequence >= CLOCK_PROBES
            || sent < 0
            || now < 0
            || self.peer_send.is_some_and(|previous| sent < previous)
            || self.pending_pong.is_some()
        {
            return Err(MultiplayerError::Protocol(
                "invalid clock ping sequence/time".into(),
            ));
        }
        self.pending_pong = Some((sequence, sent, now));
        self.peer_sequence += 1;
        self.peer_send = Some(sent);
        Ok(())
    }
    pub(crate) fn next_pong(&mut self, now: i64) -> Result<Option<Vec<u8>>, MultiplayerError> {
        let Some((sequence, echo, received)) = self.pending_pong else {
            return Ok(None);
        };
        if now < received {
            return Err(MultiplayerError::Protocol(
                "clock pong send precedes receipt".into(),
            ));
        }
        let mut payload = Vec::with_capacity(32);
        payload.extend_from_slice(&sequence.to_le_bytes());
        for timestamp in [echo, received, now] {
            payload.extend_from_slice(&timestamp.to_le_bytes());
        }
        self.pending_pong = None;
        Ok(Some(frame(7, &payload)))
    }
    pub(crate) fn receive_pong(
        &mut self,
        payload: &[u8],
        now: i64,
    ) -> Result<(), MultiplayerError> {
        if payload.len() != 32 {
            return Err(MultiplayerError::Protocol("invalid clock pong size".into()));
        }
        let word =
            |offset: usize| i64::from_le_bytes(payload[offset..offset + 8].try_into().unwrap());
        let sequence = u64::from_le_bytes(payload[..8].try_into().unwrap());
        let Some((expected, sent)) = self.pending_ping else {
            return Err(MultiplayerError::Protocol("unsolicited clock pong".into()));
        };
        if sequence != expected || word(8) != sent {
            return Err(MultiplayerError::Protocol(
                "clock pong sequence/echo mismatch".into(),
            ));
        }
        let sample = ClockSample::new(sent, word(16), word(24), now)
            .map_err(|error| MultiplayerError::Protocol(error.to_string()))?;
        self.filter
            .observe(sample)
            .map_err(|error| MultiplayerError::Protocol(error.to_string()))?;
        self.completed += 1;
        self.last_receive = now;
        self.pending_ping = None;
        Ok(())
    }
    pub(crate) fn estimate_event(&mut self) -> Option<MultiplayerEvent> {
        if self.completed != CLOCK_PROBES || self.emitted {
            return None;
        }
        let estimate = self.filter.estimate()?;
        self.emitted = true;
        Some(MultiplayerEvent::ClockEstimated(estimate))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Outgoing {
    pub(crate) progress: Progress,
    pub(crate) final_prefix: bool,
}
#[derive(Default)]
pub(crate) struct Protocol {
    pub(crate) local_ready_sent: bool,
    pub(crate) local_ready_written: bool,
    pub(crate) remote_ready: bool,
    pub(crate) ready_emitted: bool,
    pub(crate) tx_sequence: u64,
    pub(crate) rx_sequence: u64,
    pub(crate) local: Option<Progress>,
    pub(crate) remote: Option<Progress>,
    pub(crate) local_final: Option<u64>,
    pub(crate) local_final_written: bool,
    pub(crate) remote_final: bool,
    pub(crate) pending_ack: Option<u64>,
    pub(crate) ack_in_flight: bool,
    pub(crate) local_ack_received: bool,
    pub(crate) acknowledgement_emitted: bool,
}
impl Protocol {
    pub(crate) fn ready(&self) -> bool {
        self.local_ready_written && self.remote_ready
    }
    pub(crate) fn next_ready(&mut self, requested: bool) -> Option<Vec<u8>> {
        if !requested || self.local_ready_sent {
            return None;
        }
        self.local_ready_sent = true;
        Some(frame(5, &[]))
    }
    pub(crate) fn readiness(&mut self) -> Option<MultiplayerEvent> {
        if self.ready() && !self.ready_emitted {
            self.ready_emitted = true;
            Some(MultiplayerEvent::Ready)
        } else {
            None
        }
    }
    pub(crate) fn outgoing(&mut self, message: Outgoing) -> Result<Vec<u8>, MultiplayerError> {
        if !self.ready() {
            return Err(MultiplayerError::Protocol(
                "bilateral readiness required".into(),
            ));
        }
        if self.local_final.is_some() {
            return Err(MultiplayerError::Protocol(
                "progress after local final".into(),
            ));
        }
        validate_progress(self.local, message.progress)?;
        let next = self
            .tx_sequence
            .checked_add(1)
            .ok_or_else(|| MultiplayerError::Protocol("sequence exhausted".into()))?;
        let bytes = if message.final_prefix {
            prefix_frame(3, self.tx_sequence, message.progress)
        } else {
            progress_frame(self.tx_sequence, message.progress)
        };
        if message.final_prefix {
            self.local_final = Some(self.tx_sequence);
        }
        self.tx_sequence = next;
        self.local = Some(message.progress);
        Ok(bytes)
    }
    pub(crate) fn receive(
        &mut self,
        tag: u8,
        payload: &[u8],
    ) -> Result<Option<MultiplayerEvent>, MultiplayerError> {
        if tag != 5 && !self.ready() {
            return Err(MultiplayerError::Protocol(
                "data before bilateral readiness".into(),
            ));
        }
        match tag {
            5 => {
                if !payload.is_empty() || self.remote_ready {
                    return Err(MultiplayerError::Protocol(
                        "invalid or duplicate readiness".into(),
                    ));
                }
                self.remote_ready = true;
                Ok(None)
            }
            2 | 3 => {
                if self.remote_final {
                    return Err(MultiplayerError::Protocol(
                        "progress after peer final".into(),
                    ));
                }
                let progress = parse_progress(payload, self.rx_sequence, self.remote)?;
                let next = self
                    .rx_sequence
                    .checked_add(1)
                    .ok_or_else(|| MultiplayerError::Protocol("sequence exhausted".into()))?;
                if tag == 3 {
                    self.pending_ack = Some(self.rx_sequence);
                    self.remote_final = true;
                }
                self.rx_sequence = next;
                self.remote = Some(progress);
                Ok(Some(if tag == 3 {
                    MultiplayerEvent::FinalProgress(progress)
                } else {
                    MultiplayerEvent::Progress(progress)
                }))
            }
            4 => {
                if payload.len() != 8 {
                    return Err(MultiplayerError::Protocol(
                        "invalid acknowledgement size".into(),
                    ));
                }
                let sequence = u64::from_le_bytes(payload.try_into().unwrap());
                if self.local_final != Some(sequence)
                    || !self.local_final_written
                    || self.local_ack_received
                {
                    return Err(MultiplayerError::Protocol(
                        "unsolicited, duplicate or wrong acknowledgement".into(),
                    ));
                }
                self.local_ack_received = true;
                Ok(None)
            }
            _ => Err(MultiplayerError::Protocol("unexpected message".into())),
        }
    }
    // Called only between complete frames: acknowledgements never interrupt payloads.
    pub(crate) fn next_ack(&mut self) -> Option<Vec<u8>> {
        let sequence = self.pending_ack.take()?;
        self.ack_in_flight = true;
        Some(frame(4, &sequence.to_le_bytes()))
    }
    pub(crate) fn written(&mut self, tag: u8) {
        if tag == 5 {
            self.local_ready_written = true;
        }
        if tag == 3 {
            self.local_final_written = true;
        }
        if tag == 4 {
            self.ack_in_flight = false;
        }
    }
    pub(crate) fn acknowledgement(&mut self) -> Option<MultiplayerEvent> {
        if self.local_ack_received
            && self.pending_ack.is_none()
            && !self.ack_in_flight
            && !self.acknowledgement_emitted
        {
            self.acknowledgement_emitted = true;
            Some(MultiplayerEvent::FinalAcknowledged)
        } else {
            None
        }
    }
}

/// Exact frame admitted for one complete local stream write. The adapter owns
/// these bytes until completion; partial writes never acknowledge `id`.
#[derive(Debug, PartialEq, Eq)]
pub struct OutboundFrame {
    pub id: u64,
    pub bytes: Vec<u8>,
}

/// Control traffic precedes application dequeue, without a second progress queue.
#[derive(Debug, PartialEq, Eq)]
pub enum WriteStep {
    Frame(OutboundFrame),
    ApplicationSlot,
    Waiting,
}

#[derive(Clone, Copy)]
struct InFlight {
    id: u64,
    tag: u8,
    start: Option<StartMessage>,
}

/// Transport-independent session orchestration. All times are supplied by the
/// caller on one nonnegative, monotonic session clock. No acoustic timing is
/// inferred from probes or complete local writes.
///
/// There is one in-flight frame and at most eight pending events. Drain events
/// after each operation, especially after `written` and before reading EOF.
/// Errors fence every mutating operation with the original failure; already
/// queued events remain available through `poll_event`.
pub struct Session {
    identity: Vec<u8>,
    identity_sent: bool,
    connected: bool,
    ready_requested: bool,
    protocol: Protocol,
    clocks: ClockProbes,
    start: StartAgreement,
    last_now: Option<i64>,
    last_frame_id: u64,
    in_flight: Option<InFlight>,
    application_slot: bool,
    events: VecDeque<MultiplayerEvent>,
    failure: Option<MultiplayerError>,
}

impl Session {
    pub fn new(
        identity: Vec<u8>,
        role: StartRole,
        policy: StartPolicy,
        preroll_ns: i64,
    ) -> Result<Self, MultiplayerError> {
        if identity.is_empty() || identity.len() > MAX_IDENTITY {
            return Err(MultiplayerError::InvalidOptions);
        }
        let start = StartAgreement::new_at(role, policy, preroll_ns)
            .map_err(|error| MultiplayerError::Protocol(error.to_string()))?;
        Ok(Self {
            identity,
            identity_sent: false,
            connected: false,
            ready_requested: false,
            protocol: Protocol::default(),
            clocks: ClockProbes::default(),
            start,
            last_now: None,
            last_frame_id: 0,
            in_flight: None,
            application_slot: false,
            events: VecDeque::with_capacity(8),
            failure: None,
        })
    }

    fn operate<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, MultiplayerError>,
    ) -> Result<T, MultiplayerError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = operation(self);
        if let Err(error) = &result {
            self.failure = Some(error.clone());
            self.application_slot = false;
        }
        result
    }

    fn observe_now(&mut self, now: i64) -> Result<(), MultiplayerError> {
        if now < 0 || self.last_now.is_some_and(|previous| now < previous) {
            return Err(MultiplayerError::Protocol(
                "session clock is negative or regressed".into(),
            ));
        }
        self.last_now = Some(now);
        Ok(())
    }

    fn event(&mut self, event: MultiplayerEvent) -> Result<(), MultiplayerError> {
        if self.events.len() == 8 {
            return Err(MultiplayerError::QueueFull);
        }
        self.events.push_back(event);
        Ok(())
    }

    fn collect_events(&mut self) -> Result<(), MultiplayerError> {
        if let Some(event) = self.protocol.readiness() {
            self.event(event)?;
        }
        if let Some(event) = self.protocol.acknowledgement() {
            self.event(event)?;
        }
        if let Some(event) = self.clocks.estimate_event() {
            if let MultiplayerEvent::ClockEstimated(estimate) = &event {
                self.start
                    .prepare(*estimate)
                    .map_err(|error| MultiplayerError::Protocol(error.to_string()))?;
            }
            self.event(event)?;
        }
        if let Some(schedule) = self.start.take_schedule() {
            self.event(MultiplayerEvent::StartScheduled(schedule))?;
        }
        Ok(())
    }

    fn admit(
        &mut self,
        bytes: Vec<u8>,
        start: Option<StartMessage>,
    ) -> Result<OutboundFrame, MultiplayerError> {
        let id = self.last_frame_id.checked_add(1).ok_or_else(|| {
            MultiplayerError::Protocol("outbound frame identity exhausted".into())
        })?;
        self.last_frame_id = id;
        self.in_flight = Some(InFlight {
            id,
            tag: bytes[10],
            start,
        });
        self.application_slot = false;
        Ok(OutboundFrame { id, bytes })
    }

    /// One-shot local preparation request; it may precede peer identity receipt.
    pub fn request_ready(&mut self) -> Result<(), MultiplayerError> {
        self.operate(|session| {
            if session.ready_requested {
                return Err(MultiplayerError::Protocol(
                    "local readiness already requested".into(),
                ));
            }
            session.ready_requested = true;
            session.application_slot = false;
            Ok(())
        })
    }

    /// Accept one complete decoded frame. Transport framing and deadlines remain
    /// the caller's responsibility; every phase/order check lives here.
    pub fn receive(&mut self, tag: u8, payload: &[u8], now: i64) -> Result<(), MultiplayerError> {
        self.operate(|session| {
            session.observe_now(now)?;
            session.application_slot = false;
            if payload.len() > MAX_IDENTITY {
                return Err(MultiplayerError::Protocol(
                    "frame payload exceeds limit".into(),
                ));
            }
            if !session.connected {
                if tag != 1 {
                    return Err(MultiplayerError::Protocol("expected setup".into()));
                }
                if payload != session.identity.as_slice() {
                    return Err(MultiplayerError::IncompatibleSetup);
                }
                session.connected = true;
                session.event(MultiplayerEvent::Connected)?;
            } else if tag == 6 || tag == 7 {
                if !session.protocol.ready() {
                    return Err(MultiplayerError::Protocol(
                        "clock probe before readiness".into(),
                    ));
                }
                if tag == 6 {
                    session.clocks.receive_ping(payload, now)?;
                } else {
                    session.clocks.receive_pong(payload, now)?;
                }
            } else if (8..=11).contains(&tag) {
                if !session.protocol.ready() {
                    return Err(MultiplayerError::Protocol(
                        "start before preparation readiness".into(),
                    ));
                }
                session
                    .start
                    .receive(parse_start_frame(tag, payload)?, now)
                    .map_err(|error| MultiplayerError::Protocol(error.to_string()))?;
            } else {
                if matches!(tag, 2 | 3) && !session.start.committed() {
                    return Err(MultiplayerError::Protocol(
                        "progress before committed start".into(),
                    ));
                }
                if let Some(event) = session.protocol.receive(tag, payload)? {
                    session.event(event)?;
                }
            }
            session.collect_events()
        })
    }

    /// Admit one control frame or offer one application dequeue. While a frame
    /// remains in flight, this returns Waiting without producing another copy.
    pub fn poll_write(&mut self, now: i64) -> Result<WriteStep, MultiplayerError> {
        self.operate(|session| {
            session.observe_now(now)?;
            session.application_slot = false;
            if session.in_flight.is_some() {
                return Ok(WriteStep::Waiting);
            }
            if !session.identity_sent {
                let bytes = frame(1, &session.identity);
                let admitted = session.admit(bytes, None)?;
                session.identity_sent = true;
                return Ok(WriteStep::Frame(admitted));
            }
            if !session.connected {
                return Ok(WriteStep::Waiting);
            }
            let (bytes, start) =
                if let Some(bytes) = session.protocol.next_ready(session.ready_requested) {
                    (bytes, None)
                } else if let Some(bytes) = session.protocol.next_ack() {
                    (bytes, None)
                } else if session.protocol.ready() && session.clocks.pending_pong.is_some() {
                    let bytes = session.clocks.next_pong(now)?.ok_or_else(|| {
                        MultiplayerError::Protocol("missing pending clock pong".into())
                    })?;
                    (bytes, None)
                } else if session.protocol.ready()
                    && session.clocks.pending_ping.is_none()
                    && session.clocks.completed < CLOCK_PROBES
                {
                    let bytes = session.clocks.next_ping(now)?.ok_or_else(|| {
                        MultiplayerError::Protocol("missing next clock ping".into())
                    })?;
                    (bytes, None)
                } else if let Some(message) = session
                    .start
                    .next(now)
                    .map_err(|error| MultiplayerError::Protocol(error.to_string()))?
                {
                    (start_frame(message), Some(message))
                } else {
                    if session.start.committed() && session.protocol.local_final.is_none() {
                        session.application_slot = true;
                        return Ok(WriteStep::ApplicationSlot);
                    }
                    return Ok(WriteStep::Waiting);
                };
            Ok(WriteStep::Frame(session.admit(bytes, start)?))
        })
    }

    /// Consumes a fresh ApplicationSlot. Receive, readiness, or write-completion
    /// operations invalidate that grant; poll again before dequeuing progress.
    pub fn send_progress(
        &mut self,
        progress: Progress,
        final_prefix: bool,
        now: i64,
    ) -> Result<OutboundFrame, MultiplayerError> {
        self.operate(|session| {
            session.observe_now(now)?;
            if !session.application_slot
                || session.in_flight.is_some()
                || !session.start.committed()
            {
                return Err(MultiplayerError::Protocol(
                    "application write slot required".into(),
                ));
            }
            session.application_slot = false;
            let bytes = session.protocol.outgoing(Outgoing {
                progress,
                final_prefix,
            })?;
            session.admit(bytes, None)
        })
    }

    /// Credits exactly the in-flight frame once. This is a complete local write,
    /// never a claim of peer receipt or final application acknowledgement.
    pub fn written(&mut self, frame_id: u64, now: i64) -> Result<(), MultiplayerError> {
        self.operate(|session| {
            session.observe_now(now)?;
            session.application_slot = false;
            let pending = session
                .in_flight
                .filter(|frame| frame.id == frame_id)
                .ok_or_else(|| {
                    MultiplayerError::Protocol("stale or unexpected complete write".into())
                })?;
            session.protocol.written(pending.tag);
            if let Some(message) = pending.start {
                session
                    .start
                    .written(message, now)
                    .map_err(|error| MultiplayerError::Protocol(error.to_string()))?;
            }
            session.in_flight = None;
            session.collect_events()
        })
    }

    pub fn poll_event(&mut self) -> Option<MultiplayerEvent> {
        self.events.pop_front()
    }

    /// True after the peer supplied exactly the expected identity.
    pub fn setup_complete(&self) -> bool {
        self.connected
    }

    /// Used by adapter setup deadlines; identity alone is insufficient.
    pub fn preparation_pending(&self) -> bool {
        !self.connected
            || !self.protocol.ready()
            || self.clocks.completed < CLOCK_PROBES
            || !self.start.committed()
    }

    pub fn start_committed(&self) -> bool {
        self.start.committed()
    }
}

/// Incremental decoder preserves partial headers/bodies and bounds allocation before reading.
pub struct FrameDecoder {
    pub(crate) bytes: Vec<u8>,
}
impl FrameDecoder {
    pub fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(MAX_BODY + 4),
        }
    }
    pub fn needed(&self) -> Result<usize, MultiplayerError> {
        if self.bytes.len() < 4 {
            return Ok(4 - self.bytes.len());
        }
        let length = u32::from_le_bytes(self.bytes[..4].try_into().unwrap()) as usize;
        if !(7..=MAX_BODY).contains(&length) {
            return Err(MultiplayerError::Protocol("invalid frame length".into()));
        }
        (length + 4).checked_sub(self.bytes.len()).ok_or_else(|| {
            MultiplayerError::Protocol("frame extent exceeds declared length".into())
        })
    }
    /// Copy only the prefix currently needed. Take a completed frame before
    /// admitting the remainder of a coalesced transport chunk.
    pub fn push(&mut self, chunk: &[u8]) -> Result<usize, MultiplayerError> {
        let count = self.needed()?.min(chunk.len());
        self.bytes.extend_from_slice(&chunk[..count]);
        Ok(count)
    }
    pub fn take(&mut self) -> Result<Option<(u8, Vec<u8>)>, MultiplayerError> {
        if self.needed()? != 0 {
            return Ok(None);
        }
        if &self.bytes[4..8] != MAGIC
            || u16::from_le_bytes(self.bytes[8..10].try_into().unwrap()) != VERSION
        {
            return Err(MultiplayerError::Protocol(
                "unknown protocol/version".into(),
            ));
        }
        let tag = self.bytes[10];
        let payload = self.bytes[11..].to_vec();
        self.bytes.clear();
        Ok(Some((tag, payload)))
    }
}
