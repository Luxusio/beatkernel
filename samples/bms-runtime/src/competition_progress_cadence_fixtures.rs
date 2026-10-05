use super::*;
use crate::{competition_progress::ProgressNotice, local_players::PlayerId, multiplayer::Progress};
use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::Arc};

// Deliberately neither Debug, Clone, Display nor std::error::Error.
struct PublicationToken(Arc<u8>);
struct ClockToken(Arc<u8>);
#[derive(Debug, PartialEq, Eq)]
enum Operation {
    Ready,
    Started,
    Clock,
    Publish,
    Room,
    Stop,
    Notices,
}
type Trace = Rc<RefCell<Vec<Operation>>>;
struct Port {
    trace: Trace,
    ready: bool,
    started: bool,
    refusal: Option<Arc<u8>>,
    rows: Vec<Vec<MemberProgress>>,
    pointers: Vec<usize>,
}
impl CompetitionProgressPort for Port {
    type Error = PublicationToken;
    type Notices = std::iter::Empty<ProgressNotice<PublicationToken>>;
    fn notices(&mut self) -> Self::Notices {
        self.trace.borrow_mut().push(Operation::Notices);
        std::iter::empty()
    }
    fn ready(&self) -> bool {
        self.trace.borrow_mut().push(Operation::Ready);
        self.ready
    }
    fn started(&self) -> bool {
        self.trace.borrow_mut().push(Operation::Started);
        self.started
    }
    fn publish(&mut self, rows: &[MemberProgress]) -> Result<(), PublicationToken> {
        self.trace.borrow_mut().push(Operation::Publish);
        self.pointers.push(rows.as_ptr() as usize);
        self.rows.push(rows.to_vec());
        match self.refusal.take() {
            Some(token) => Err(PublicationToken(token)),
            None => Ok(()),
        }
    }
    fn observe_room(&mut self, _: &[MemberProgress]) -> Result<(), PublicationToken> {
        self.trace.borrow_mut().push(Operation::Room);
        Ok(())
    }
    fn request_stop(&mut self) {
        self.trace.borrow_mut().push(Operation::Stop);
    }
}
struct Clock {
    trace: Trace,
    samples: VecDeque<Result<u64, ClockToken>>,
}
impl CompetitionProgressClock for Clock {
    type Error = ClockToken;
    fn now_ns(&mut self) -> Result<u64, ClockToken> {
        self.trace.borrow_mut().push(Operation::Clock);
        self.samples.pop_front().expect("unexpected clock access")
    }
}
fn setup(
    samples: impl IntoIterator<Item = Result<u64, ClockToken>>,
) -> (Port, Clock, ProgressCadence) {
    let trace = Rc::new(RefCell::new(Vec::new()));
    (
        Port {
            trace: trace.clone(),
            ready: true,
            started: true,
            refusal: None,
            rows: Vec::new(),
            pointers: Vec::new(),
        },
        Clock {
            trace,
            samples: samples.into_iter().collect(),
        },
        ProgressCadence::new(),
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
fn send(
    port: &mut Port,
    clock: &mut Clock,
    cadence: &mut ProgressCadence,
    rows: &[MemberProgress],
) -> Result<bool, CadenceError<PublicationToken, ClockToken>> {
    publish_progress_with_clock(port, clock, cadence, rows, true, true)
}
fn accepted(result: Result<bool, CadenceError<PublicationToken, ClockToken>>) {
    assert!(matches!(result, Ok(true)));
}

#[test]
fn every_gate_combination_suppresses_clock_and_effect_until_open() {
    for allowed in [false, true] {
        for ready in [false, true] {
            for require_start in [false, true] {
                for started in [false, true] {
                    let (mut port, mut clock, mut cadence) = setup([Ok(11), Ok(13)]);
                    port.ready = ready;
                    port.started = started;
                    let open = allowed && ready && (!require_start || started);
                    let answer = publish_progress_with_clock(
                        &mut port,
                        &mut clock,
                        &mut cadence,
                        &members(),
                        allowed,
                        require_start,
                    );
                    assert!(matches!(answer, Ok(value) if value == open));
                    let mut expected = Vec::new();
                    if allowed {
                        expected.push(Operation::Ready);
                    }
                    if allowed && ready && require_start {
                        expected.push(Operation::Started);
                    }
                    if open {
                        expected.extend([Operation::Clock, Operation::Publish, Operation::Clock]);
                    }
                    assert_eq!(*port.trace.borrow(), expected);
                    assert_eq!(cadence.last_observed(), if open { Some(13) } else { None });
                    assert_eq!(cadence.last_published(), if open { Some(13) } else { None });
                }
            }
        }
    }
    assert_eq!(ProgressCadence::default().last_observed(), None);
    assert_eq!(ProgressCadence::default().last_published(), None);
}

#[test]
fn initial_and_exact_interval_boundaries_preserve_original_borrowed_progress() {
    for delta in [49_999_999, 50_000_000, 50_000_001] {
        let (mut port, mut clock, mut cadence) =
            setup([Ok(10), Ok(20), Ok(20 + delta), Ok(21 + delta)]);
        let rows = members();
        accepted(send(&mut port, &mut clock, &mut cadence, &rows));
        let answer = send(&mut port, &mut clock, &mut cadence, &rows);
        assert!(matches!(answer, Ok(value) if value == (delta >= 50_000_000)));
        assert_eq!(port.rows.len(), if delta < 50_000_000 { 1 } else { 2 });
        assert!(
            port.rows
                .iter()
                .all(|sent| sent.as_slice() == rows.as_slice())
        );
        assert!(
            port.pointers
                .iter()
                .all(|pointer| *pointer == rows.as_ptr() as usize)
        );
        assert_eq!(
            cadence.last_observed(),
            Some(if delta < 50_000_000 {
                20 + delta
            } else {
                21 + delta
            })
        );
        assert_eq!(
            cadence.last_published(),
            Some(if delta < 50_000_000 { 20 } else { 21 + delta })
        );
        assert!(!port.trace.borrow().contains(&Operation::Room));
        assert!(!port.trace.borrow().contains(&Operation::Stop));
    }
}

#[test]
fn integer_precision_week_extents_and_maximum_clock_never_need_deadline_addition() {
    for origin in [
        604_800_000_000_001,
        9_007_199_254_740_993,
        u64::MAX - 50_000_000,
    ] {
        let (mut port, mut clock, mut cadence) = setup([
            Ok(origin),
            Ok(origin),
            Ok(origin + 49_999_999),
            Ok(origin + 50_000_000),
            Ok(origin + 50_000_000),
        ]);
        accepted(send(&mut port, &mut clock, &mut cadence, &members()));
        assert!(matches!(
            send(&mut port, &mut clock, &mut cadence, &members()),
            Ok(false)
        ));
        accepted(send(&mut port, &mut clock, &mut cadence, &members()));
        assert_eq!(cadence.last_published(), Some(origin + 50_000_000));
        assert_eq!(port.rows.len(), 2);
    }
    let (mut port, mut clock, mut cadence) = setup([Ok(u64::MAX), Ok(u64::MAX), Ok(u64::MAX)]);
    accepted(send(&mut port, &mut clock, &mut cadence, &members()));
    assert!(matches!(
        send(&mut port, &mut clock, &mut cadence, &members()),
        Ok(false)
    ));
    assert_eq!(cadence.last_published(), Some(u64::MAX));
}

#[test]
fn slow_success_starts_interval_after_effect_completion() {
    let (mut port, mut clock, mut cadence) = setup([
        Ok(100),
        Ok(200_000_000),
        Ok(249_999_999),
        Ok(250_000_000),
        Ok(260_000_000),
    ]);
    accepted(send(&mut port, &mut clock, &mut cadence, &members()));
    assert_eq!(cadence.last_published(), Some(200_000_000));
    assert!(matches!(
        send(&mut port, &mut clock, &mut cadence, &members()),
        Ok(false)
    ));
    assert_eq!(cadence.last_observed(), Some(249_999_999));
    accepted(send(&mut port, &mut clock, &mut cadence, &members()));
    assert_eq!(cadence.last_published(), Some(260_000_000));
    assert_eq!(
        *port.trace.borrow(),
        vec![
            Operation::Ready,
            Operation::Started,
            Operation::Clock,
            Operation::Publish,
            Operation::Clock,
            Operation::Ready,
            Operation::Started,
            Operation::Clock,
            Operation::Ready,
            Operation::Started,
            Operation::Clock,
            Operation::Publish,
            Operation::Clock
        ]
    );
}

#[test]
fn refusal_keeps_original_publication_token_without_post_sample_or_marker_commit() {
    let token = Arc::new(42);
    let (mut port, mut clock, mut cadence) = setup([Ok(100), Ok(100), Ok(50_000_100)]);
    accepted(send(&mut port, &mut clock, &mut cadence, &members()));
    port.trace.borrow_mut().clear();
    port.refusal = Some(token.clone());
    match send(&mut port, &mut clock, &mut cadence, &members()) {
        Err(CadenceError::Publication(PublicationToken(actual))) => {
            assert!(Arc::ptr_eq(&token, &actual))
        }
        _ => panic!("publication refusal must retain its original opaque token"),
    }
    assert_eq!(cadence.last_observed(), Some(50_000_100));
    assert_eq!(cadence.last_published(), Some(100));
    assert_eq!(
        *port.trace.borrow(),
        vec![
            Operation::Ready,
            Operation::Started,
            Operation::Clock,
            Operation::Publish
        ]
    );
}

#[test]
fn pre_and_post_clock_refusals_keep_identity_and_distinguish_irreversible_effect() {
    for post in [false, true] {
        let token = Arc::new(61);
        let mut script = vec![Ok(100), Ok(100)];
        if post {
            script.push(Ok(50_000_100));
        }
        script.push(Err(ClockToken(token.clone())));
        let (mut port, mut clock, mut cadence) = setup(script);
        accepted(send(&mut port, &mut clock, &mut cadence, &members()));
        port.trace.borrow_mut().clear();
        match send(&mut port, &mut clock, &mut cadence, &members()) {
            Err(CadenceError::Clock(ClockToken(actual))) => assert!(Arc::ptr_eq(&token, &actual)),
            _ => panic!("clock refusal must retain its original opaque token"),
        }
        assert_eq!(port.rows.len(), if post { 2 } else { 1 });
        assert_eq!(
            cadence.last_observed(),
            Some(if post { 50_000_100 } else { 100 })
        );
        assert_eq!(cadence.last_published(), Some(100));
        let expected = if post {
            vec![
                Operation::Ready,
                Operation::Started,
                Operation::Clock,
                Operation::Publish,
                Operation::Clock,
            ]
        } else {
            vec![Operation::Ready, Operation::Started, Operation::Clock]
        };
        assert_eq!(*port.trace.borrow(), expected);
    }
}

#[test]
fn regression_is_against_every_observed_sample_and_never_commits_backward_time() {
    let (mut port, mut clock, mut cadence) = setup([Ok(100), Ok(100), Ok(120), Ok(110)]);
    accepted(send(&mut port, &mut clock, &mut cadence, &members()));
    assert!(matches!(
        send(&mut port, &mut clock, &mut cadence, &members()),
        Ok(false)
    ));
    port.trace.borrow_mut().clear();
    assert!(matches!(
        send(&mut port, &mut clock, &mut cadence, &members()),
        Err(CadenceError::ClockRegressed)
    ));
    assert_eq!(cadence.last_observed(), Some(120));
    assert_eq!(cadence.last_published(), Some(100));
    assert_eq!(
        *port.trace.borrow(),
        vec![Operation::Ready, Operation::Started, Operation::Clock]
    );
    assert_eq!(port.rows.len(), 1);

    for first in [false, true] {
        let script = if first {
            vec![Ok(100), Ok(99)]
        } else {
            vec![Ok(100), Ok(100), Ok(50_000_100), Ok(50_000_099)]
        };
        let (mut port, mut clock, mut cadence) = setup(script);
        if !first {
            accepted(send(&mut port, &mut clock, &mut cadence, &members()));
        }
        port.trace.borrow_mut().clear();
        assert!(matches!(
            send(&mut port, &mut clock, &mut cadence, &members()),
            Err(CadenceError::ClockRegressed)
        ));
        assert_eq!(
            cadence.last_observed(),
            Some(if first { 100 } else { 50_000_100 })
        );
        assert_eq!(
            cadence.last_published(),
            if first { None } else { Some(100) }
        );
        assert_eq!(port.rows.len(), if first { 1 } else { 2 });
        assert_eq!(
            *port.trace.borrow(),
            vec![
                Operation::Ready,
                Operation::Started,
                Operation::Clock,
                Operation::Publish,
                Operation::Clock
            ]
        );
    }
}
