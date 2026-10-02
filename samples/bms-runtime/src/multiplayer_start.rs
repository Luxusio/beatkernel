//! Checked software-start agreement; no I/O or physical audio timing guarantee.
use crate::multiplayer_clock::{ClockError, OffsetEstimate};
use std::fmt;

/// Role in the proposal/accept/commit exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartRole {
    Host,
    Join,
}
/// Caller-selected deadline and estimate admission bounds in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartPolicy {
    pub lead_ns: u64,
    pub min_remaining_ns: u64,
    pub max_age_ns: u64,
    pub max_uncertainty_ns: u64,
    /// Caller admission bound after a committed target; zero is permitted.
    pub max_release_lateness_ns: u64,
}
impl Default for StartPolicy {
    fn default() -> Self {
        Self {
            lead_ns: 2_000_000_000,
            min_remaining_ns: 100_000_000,
            max_age_ns: 5_000_000_000,
            max_uncertainty_ns: 100_000_000,
            max_release_lateness_ns: 25_000_000,
        }
    }
}
impl StartPolicy {
    /// Rejects zero minimum lead, insufficient lead and unrepresentable bounds.
    pub fn validate(self) -> Result<(), StartError> {
        if self.min_remaining_ns == 0
            || self.lead_ns <= self.min_remaining_ns
            || [
                self.lead_ns,
                self.min_remaining_ns,
                self.max_age_ns,
                self.max_uncertainty_ns,
                self.max_release_lateness_ns,
            ]
            .iter()
            .any(|bound| *bound > i64::MAX as u64)
        {
            return Err(StartError::InvalidPolicy);
        }
        Ok(())
    }
}
/// Exact message admitted for one complete frame write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartMessage {
    ClockReady,
    Propose(i64),
    Accept(i64),
    Commit(i64),
}
/// Committed target on this participant's elapsed clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartSchedule {
    pub target_ns: i64,
    pub uncertainty_ns: u64,
}
/// Atomic transition rejection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    InvalidPolicy,
    AlreadyPrepared,
    NotPrepared,
    NegativeNow,
    TimeRegression,
    UnexpectedMessage,
    WrongEcho,
    UncertainEstimate,
    Estimate(ClockError),
    Overflow,
    DeadlineTooClose,
}
impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for StartError {}
impl From<ClockError> for StartError {
    fn from(error: ClockError) -> Self {
        Self::Estimate(error)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Waiting,
    Proposing,
    Proposed,
    Accepting,
    Accepted,
    Committing,
    Committed,
}
/// Bounded scalar handshake with one in-flight frame and one consumable schedule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartAgreement {
    role: StartRole,
    policy: StartPolicy,
    estimate: Option<OffsetEstimate>,
    peer_ready: bool,
    local_ready: bool,
    in_flight: Option<StartMessage>,
    stage: Stage,
    proposal: Option<i64>,
    schedule: Option<StartSchedule>,
    last_now: Option<i64>,
}
impl StartAgreement {
    pub fn new(role: StartRole, policy: StartPolicy) -> Result<Self, StartError> {
        policy.validate()?;
        Ok(Self {
            role,
            policy,
            estimate: None,
            peer_ready: false,
            local_ready: false,
            in_flight: None,
            stage: Stage::Waiting,
            proposal: None,
            schedule: None,
            last_now: None,
        })
    }
    /// Installs one checked estimate; peer readiness may already have arrived.
    pub fn prepare(&mut self, estimate: OffsetEstimate) -> Result<(), StartError> {
        if self.estimate.is_some() {
            return Err(StartError::AlreadyPrepared);
        }
        if estimate.round_trip_ns() > self.policy.max_uncertainty_ns {
            return Err(StartError::UncertainEstimate);
        }
        self.estimate = Some(estimate);
        Ok(())
    }
    fn observe_now(&mut self, now: i64) -> Result<(), StartError> {
        if now < 0 {
            return Err(StartError::NegativeNow);
        }
        if self.last_now.is_some_and(|previous| now < previous) {
            return Err(StartError::TimeRegression);
        }
        self.last_now = Some(now);
        Ok(())
    }
    fn estimate_at(&self, now: i64) -> Result<OffsetEstimate, StartError> {
        let estimate = self.estimate.ok_or(StartError::NotPrepared)?;
        if now < estimate.observed_local_ns() {
            return Err(ClockError::FutureObservation.into());
        }
        if i128::from(now) - i128::from(estimate.observed_local_ns())
            > i128::from(self.policy.max_age_ns)
        {
            return Err(ClockError::StaleEstimate.into());
        }
        if estimate.round_trip_ns() > self.policy.max_uncertainty_ns {
            return Err(StartError::UncertainEstimate);
        }
        Ok(estimate)
    }
    fn schedule_at(&self, deadline: i64, now: i64) -> Result<StartSchedule, StartError> {
        let estimate = self.estimate_at(now)?;
        let (earliest, target) = match self.role {
            StartRole::Host => (deadline, deadline),
            StartRole::Join => {
                let window =
                    estimate.remote_deadline_to_local(deadline, now, self.policy.max_age_ns)?;
                (window.earliest_ns(), window.midpoint_ns())
            }
        };
        if i128::from(earliest) - i128::from(now) < i128::from(self.policy.min_remaining_ns) {
            return Err(StartError::DeadlineTooClose);
        }
        Ok(StartSchedule {
            target_ns: target,
            uncertainty_ns: estimate.round_trip_ns(),
        })
    }
    /// Admits at most one frame; returns None while a prior frame remains in flight.
    pub fn next(&mut self, now: i64) -> Result<Option<StartMessage>, StartError> {
        let mut candidate = *self;
        candidate.observe_now(now)?;
        let result = candidate.next_inner(now)?;
        *self = candidate;
        Ok(result)
    }
    fn next_inner(&mut self, now: i64) -> Result<Option<StartMessage>, StartError> {
        if self.in_flight.is_some() || self.stage == Stage::Committed {
            return Ok(None);
        }
        if !self.local_ready {
            if self.estimate.is_none() {
                return Ok(None);
            }
            self.estimate_at(now)?;
            self.in_flight = Some(StartMessage::ClockReady);
            return Ok(self.in_flight);
        }
        if !self.peer_ready {
            return Ok(None);
        }
        let message = match (self.role, self.stage) {
            (StartRole::Host, Stage::Waiting) => {
                self.estimate_at(now)?;
                let deadline = now
                    .checked_add(self.policy.lead_ns as i64)
                    .ok_or(StartError::Overflow)?;
                self.schedule_at(deadline, now)?;
                self.proposal = Some(deadline);
                self.stage = Stage::Proposing;
                StartMessage::Propose(deadline)
            }
            (StartRole::Join, Stage::Proposed) => {
                let deadline = self.proposal.ok_or(StartError::UnexpectedMessage)?;
                self.schedule_at(deadline, now)?;
                self.stage = Stage::Accepting;
                StartMessage::Accept(deadline)
            }
            (StartRole::Host, Stage::Accepted) => {
                let deadline = self.proposal.ok_or(StartError::UnexpectedMessage)?;
                self.schedule_at(deadline, now)?;
                self.stage = Stage::Committing;
                StartMessage::Commit(deadline)
            }
            _ => return Ok(None),
        };
        self.in_flight = Some(message);
        Ok(Some(message))
    }
    /// Completes exactly the admitted frame; partial writes must not call this.
    pub fn written(&mut self, message: StartMessage, now: i64) -> Result<(), StartError> {
        let mut candidate = *self;
        candidate.observe_now(now)?;
        if candidate.in_flight != Some(message) {
            return Err(StartError::UnexpectedMessage);
        }
        match message {
            StartMessage::ClockReady => {
                candidate.estimate_at(now)?;
                candidate.local_ready = true;
            }
            StartMessage::Propose(deadline) => {
                candidate.schedule_at(deadline, now)?;
                candidate.stage = Stage::Proposed;
            }
            StartMessage::Accept(deadline) => {
                candidate.schedule_at(deadline, now)?;
                candidate.stage = Stage::Accepted;
            }
            StartMessage::Commit(deadline) => {
                candidate.schedule = Some(candidate.schedule_at(deadline, now)?);
                candidate.stage = Stage::Committed;
            }
        }
        candidate.in_flight = None;
        *self = candidate;
        Ok(())
    }
    /// Receives an exact peer message, atomically rejecting invalid transitions.
    pub fn receive(&mut self, message: StartMessage, now: i64) -> Result<(), StartError> {
        let mut candidate = *self;
        candidate.observe_now(now)?;
        if message == StartMessage::ClockReady {
            if candidate.peer_ready {
                return Err(StartError::UnexpectedMessage);
            }
            candidate.peer_ready = true;
        } else {
            if !candidate.local_ready || !candidate.peer_ready || candidate.in_flight.is_some() {
                return Err(StartError::UnexpectedMessage);
            }
            candidate.estimate_at(now)?;
            match (candidate.role, candidate.stage, message) {
                (StartRole::Join, Stage::Waiting, StartMessage::Propose(deadline)) => {
                    candidate.schedule_at(deadline, now)?;
                    candidate.proposal = Some(deadline);
                    candidate.stage = Stage::Proposed;
                }
                (StartRole::Host, Stage::Proposed, StartMessage::Accept(deadline)) => {
                    if candidate.proposal != Some(deadline) {
                        return Err(StartError::WrongEcho);
                    }
                    candidate.schedule_at(deadline, now)?;
                    candidate.stage = Stage::Accepted;
                }
                (StartRole::Join, Stage::Accepted, StartMessage::Commit(deadline)) => {
                    if candidate.proposal != Some(deadline) {
                        return Err(StartError::WrongEcho);
                    }
                    candidate.schedule = Some(candidate.schedule_at(deadline, now)?);
                    candidate.stage = Stage::Committed;
                }
                _ => return Err(StartError::UnexpectedMessage),
            }
        }
        *self = candidate;
        Ok(())
    }
    /// Takes the schedule once, without changing the committed phase.
    pub fn take_schedule(&mut self) -> Option<StartSchedule> {
        self.schedule.take()
    }
    /// True only after full host Commit write or exact join Commit receipt.
    pub fn committed(&self) -> bool {
        self.stage == Stage::Committed
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::multiplayer_clock::{ClockFilter, ClockSample};
    fn estimate(times: [i64; 4]) -> OffsetEstimate {
        let mut filter = ClockFilter::new();
        filter
            .observe(ClockSample::new(times[0], times[1], times[2], times[3]).unwrap())
            .unwrap();
        filter.estimate().unwrap()
    }
    fn policy() -> StartPolicy {
        StartPolicy {
            lead_ns: 1000,
            min_remaining_ns: 100,
            max_age_ns: 10_000,
            max_uncertainty_ns: 100,
            max_release_lateness_ns: 25,
        }
    }
    fn prepared(role: StartRole) -> StartAgreement {
        let mut agreement = StartAgreement::new(role, policy()).unwrap();
        agreement.prepare(estimate([0, 0, 0, 0])).unwrap();
        agreement
    }
    fn ready(role: StartRole) -> StartAgreement {
        let mut agreement = prepared(role);
        agreement.receive(StartMessage::ClockReady, 0).unwrap();
        assert_eq!(agreement.next(0).unwrap(), Some(StartMessage::ClockReady));
        agreement.written(StartMessage::ClockReady, 0).unwrap();
        agreement
    }
    #[test]
    fn unprepared_polling_waits_and_retains_early_peer_readiness() {
        let mut agreement = StartAgreement::new(StartRole::Host, policy()).unwrap();
        assert_eq!(agreement.next(0).unwrap(), None);
        agreement.receive(StartMessage::ClockReady, 1).unwrap();
        assert_eq!(agreement.next(1).unwrap(), None);
        assert!(!agreement.committed());
        agreement.prepare(estimate([1, 1, 1, 1])).unwrap();
        assert_eq!(agreement.next(1).unwrap(), Some(StartMessage::ClockReady));
        agreement.written(StartMessage::ClockReady, 1).unwrap();
        assert_eq!(
            agreement.next(1).unwrap(),
            Some(StartMessage::Propose(1001))
        );
    }
    #[test]
    fn two_peer_literal_offsets_preserve_uncertainty_and_full_write_commit() {
        for (host_times, join_times, offset, width) in [
            ([100, 160, 180, 140], [100, 60, 80, 140], 50, 20),
            ([100, 60, 80, 140], [100, 160, 180, 140], -50, 20),
            ([0, 101, 111, 31], [101, 21, 31, 132], 90, 21),
        ] {
            let mut host = StartAgreement::new(StartRole::Host, policy()).unwrap();
            let mut join = StartAgreement::new(StartRole::Join, policy()).unwrap();
            // Peer readiness is legal before the participant prepares its own estimate.
            join.receive(StartMessage::ClockReady, 500 + offset)
                .unwrap();
            host.prepare(estimate(host_times)).unwrap();
            join.prepare(estimate(join_times)).unwrap();
            host.next(1000).unwrap();
            host.written(StartMessage::ClockReady, 1000).unwrap();
            join.next(1000 + offset).unwrap();
            join.written(StartMessage::ClockReady, 1000 + offset)
                .unwrap();
            host.receive(StartMessage::ClockReady, 1000).unwrap();
            let propose = host.next(1000).unwrap().unwrap();
            assert_eq!(propose, StartMessage::Propose(2000));
            assert_eq!(host.next(1001).unwrap(), None);
            host.written(propose, 1001).unwrap();
            join.receive(propose, 1001 + offset).unwrap();
            let accept = join.next(1002 + offset).unwrap().unwrap();
            assert_eq!(accept, StartMessage::Accept(2000));
            join.written(accept, 1003 + offset).unwrap();
            host.receive(accept, 1003).unwrap();
            let commit = host.next(1004).unwrap().unwrap();
            assert!(!host.committed() && host.take_schedule().is_none());
            host.written(commit, 1005).unwrap();
            assert_eq!(
                host.take_schedule(),
                Some(StartSchedule {
                    target_ns: 2000,
                    uncertainty_ns: width
                })
            );
            assert!(host.committed() && host.take_schedule().is_none());
            assert!(!join.committed());
            join.receive(commit, 1005 + offset).unwrap();
            assert_eq!(
                join.take_schedule(),
                Some(StartSchedule {
                    target_ns: 2000 + offset,
                    uncertainty_ns: width
                })
            );
            assert!(join.committed() && join.take_schedule().is_none());
        }
    }
    #[test]
    fn ordering_wrong_echo_and_failed_writes_preserve_pending_state() {
        let mut host = ready(StartRole::Host);
        let propose = host.next(10).unwrap().unwrap();
        let before = host;
        assert!(host.written(StartMessage::Propose(1011), 11).is_err());
        assert_eq!(host, before);
        assert!(host.receive(StartMessage::Accept(1010), 11).is_err());
        assert_eq!(host, before);
        host.written(propose, 11).unwrap();
        let before = host;
        assert_eq!(
            host.receive(StartMessage::Accept(1011), 12),
            Err(StartError::WrongEcho)
        );
        assert_eq!(host, before);
        assert!(host.receive(StartMessage::Propose(1010), 12).is_err());
        assert_eq!(host, before);
        host.receive(StartMessage::Accept(1010), 12).unwrap();
        let commit = host.next(13).unwrap().unwrap();
        let before = host;
        assert_eq!(host.written(commit, 911), Err(StartError::DeadlineTooClose));
        assert_eq!(host, before);
        host.written(commit, 14).unwrap();
        assert!(host.written(commit, 15).is_err());
        let mut join = ready(StartRole::Join);
        let before = join;
        assert!(join.receive(StartMessage::Commit(1000), 1).is_err());
        assert_eq!(join, before);
        join.receive(StartMessage::Propose(1000), 1).unwrap();
        let before = join;
        assert!(join.receive(StartMessage::Propose(1000), 2).is_err());
        assert_eq!(join, before);
        let accept = join.next(2).unwrap().unwrap();
        join.written(accept, 3).unwrap();
        let before = join;
        assert_eq!(
            join.receive(StartMessage::Commit(1001), 4),
            Err(StartError::WrongEcho)
        );
        assert_eq!(join, before);
        join.receive(StartMessage::Commit(1000), 4).unwrap();
        assert!(join.receive(StartMessage::Commit(1000), 5).is_err());
    }
    #[test]
    fn policy_prepare_chronology_age_and_lateness_boundaries() {
        for invalid in [
            StartPolicy {
                min_remaining_ns: 0,
                ..policy()
            },
            StartPolicy {
                lead_ns: 100,
                ..policy()
            },
            StartPolicy {
                lead_ns: u64::MAX,
                ..policy()
            },
            StartPolicy {
                max_release_lateness_ns: u64::MAX,
                ..policy()
            },
        ] {
            assert_eq!(invalid.validate(), Err(StartError::InvalidPolicy));
        }
        StartPolicy {
            max_uncertainty_ns: 0,
            max_release_lateness_ns: 0,
            ..policy()
        }
        .validate()
        .unwrap();
        let mut agreement = prepared(StartRole::Host);
        let before = agreement;
        assert_eq!(
            agreement.prepare(estimate([0, 0, 0, 0])),
            Err(StartError::AlreadyPrepared)
        );
        assert_eq!(agreement, before);
        assert_eq!(agreement.next(-1), Err(StartError::NegativeNow));
        assert_eq!(agreement, before);
        agreement.next(10).unwrap();
        let before = agreement;
        assert_eq!(
            agreement.written(StartMessage::ClockReady, 9),
            Err(StartError::TimeRegression)
        );
        assert_eq!(agreement, before);
        assert_eq!(
            agreement.written(StartMessage::ClockReady, 10_001),
            Err(StartError::Estimate(ClockError::StaleEstimate))
        );
        assert_eq!(agreement, before);
        agreement.written(StartMessage::ClockReady, 10).unwrap();
        agreement.receive(StartMessage::ClockReady, 10).unwrap();
        let before = agreement;
        assert!(agreement.receive(StartMessage::ClockReady, 11).is_err());
        assert_eq!(agreement, before);
        let mut uncertain = StartAgreement::new(
            StartRole::Join,
            StartPolicy {
                max_uncertainty_ns: 0,
                ..policy()
            },
        )
        .unwrap();
        assert_eq!(
            uncertain.prepare(estimate([0, 0, 0, 1])),
            Err(StartError::UncertainEstimate)
        );
        assert!(uncertain.estimate.is_none());
        let mut future = StartAgreement::new(StartRole::Host, policy()).unwrap();
        future.prepare(estimate([5, 5, 5, 5])).unwrap();
        assert_eq!(
            future.next(4),
            Err(StartError::Estimate(ClockError::FutureObservation))
        );
        let mut join = ready(StartRole::Join);
        let before = join;
        assert_eq!(
            join.receive(StartMessage::Propose(99), 0),
            Err(StartError::DeadlineTooClose)
        );
        assert_eq!(join, before);
        join.receive(StartMessage::Propose(100), 0).unwrap(); // exact minimum allowed
    }
    #[test]
    fn long_elapsed_timelines_and_overflow_are_checked() {
        for now in [
            20 * 60 * 60 * 1_000_000_000i64,
            7 * 24 * 60 * 60 * 1_000_000_000,
            i64::MAX - 1000,
        ] {
            let mut host = StartAgreement::new(StartRole::Host, policy()).unwrap();
            host.prepare(estimate([now, now, now, now])).unwrap();
            host.receive(StartMessage::ClockReady, now).unwrap();
            host.next(now).unwrap();
            host.written(StartMessage::ClockReady, now).unwrap();
            assert_eq!(
                host.next(now).unwrap(),
                Some(StartMessage::Propose(now + 1000))
            );
        }
        let now = i64::MAX - 999;
        let mut host = StartAgreement::new(StartRole::Host, policy()).unwrap();
        host.prepare(estimate([now, now, now, now])).unwrap();
        host.receive(StartMessage::ClockReady, now).unwrap();
        host.next(now).unwrap();
        host.written(StartMessage::ClockReady, now).unwrap();
        let before = host;
        assert_eq!(host.next(now), Err(StartError::Overflow));
        assert_eq!(host, before);
    }
}
