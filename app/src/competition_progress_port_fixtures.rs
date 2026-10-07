//! Deferred generic progress policy with owned notices and opaque associated errors.
use super::*;
use crate::{local_players::PlayerId, multiplayer::Progress};
use std::{cell::Cell, rc::Rc, sync::Arc};
// No Error/Display/Debug/Clone requirement crosses the progress policy port.
struct Opaque(Arc<usize>);
struct Notices {
    inner: std::vec::IntoIter<ProgressNotice<Opaque>>,
    consumed: Rc<Cell<usize>>,
}
impl Iterator for Notices {
    type Item = ProgressNotice<Opaque>;
    fn next(&mut self) -> Option<Self::Item> {
        let notice = self.inner.next()?;
        self.consumed.set(self.consumed.get() + 1);
        Some(notice)
    }
}
#[derive(Default)]
struct Port {
    batch: Vec<ProgressNotice<Opaque>>,
    consumed: Rc<Cell<usize>>,
    ready: bool,
    started: bool,
    readiness_reads: Cell<usize>,
    start_reads: Cell<usize>,
    published: Vec<Vec<MemberProgress>>,
    rooms: Vec<Vec<MemberProgress>>,
    borrowed: Vec<(usize, usize)>,
    publication_error: Option<Arc<usize>>,
    room_error: Option<Arc<usize>>,
    stops: usize,
}
impl CompetitionProgressPort for Port {
    type Error = Opaque;
    type Notices = Notices;
    fn notices(&mut self) -> Notices {
        Notices {
            inner: std::mem::take(&mut self.batch).into_iter(),
            consumed: self.consumed.clone(),
        }
    }
    fn ready(&self) -> bool {
        self.readiness_reads.set(self.readiness_reads.get() + 1);
        self.ready
    }
    fn started(&self) -> bool {
        self.start_reads.set(self.start_reads.get() + 1);
        self.started
    }
    fn publish(&mut self, members: &[MemberProgress]) -> Result<(), Opaque> {
        self.borrowed
            .push((members.as_ptr() as usize, members.len()));
        self.published.push(members.to_vec());
        match &self.publication_error {
            Some(error) => Err(Opaque(error.clone())),
            None => Ok(()),
        }
    }
    fn observe_room(&mut self, members: &[MemberProgress]) -> Result<(), Opaque> {
        self.borrowed
            .push((members.as_ptr() as usize, members.len()));
        self.rooms.push(members.to_vec());
        match &self.room_error {
            Some(error) => Err(Opaque(error.clone())),
            None => Ok(()),
        }
    }
    fn request_stop(&mut self) {
        self.stops += 1;
    }
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
#[test]
fn every_notice_order_drains_the_batch_and_disconnect_dominates_readiness() {
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let error = Arc::new(91);
        let mut port = Port::default();
        port.batch = order
            .into_iter()
            .map(|event| match event {
                0 => ProgressNotice::Connected,
                1 => ProgressNotice::Ready,
                _ => ProgressNotice::Disconnected(Opaque(error.clone())),
            })
            .collect();
        let mut status = NetworkStatus::Waiting;
        match poll_progress(&mut port, &mut status, true) {
            Err(returned) => assert!(Arc::ptr_eq(&returned.0, &error)),
            Ok(()) => panic!("disconnect was ignored"),
        }
        assert_eq!(status, NetworkStatus::Disconnected);
        assert_eq!(port.consumed.get(), 3);
        assert!(port.published.is_empty());
        assert!(port.rooms.is_empty());
        assert_eq!(port.stops, 0);
    }
}
#[test]
fn first_of_multiple_disconnect_errors_is_retained_without_short_circuiting_remaining_notices() {
    let first = Arc::new(31);
    let second = Arc::new(32);
    let mut port = Port::default();
    port.batch = vec![
        ProgressNotice::Disconnected(Opaque(first.clone())),
        ProgressNotice::Ready,
        ProgressNotice::Disconnected(Opaque(second)),
        ProgressNotice::Connected,
    ];
    let mut status = NetworkStatus::Connected;
    match poll_progress(&mut port, &mut status, true) {
        Err(error) => assert!(Arc::ptr_eq(&error.0, &first)),
        Ok(()) => panic!("first error lost"),
    }
    assert_eq!(port.consumed.get(), 4);
    assert_eq!(status, NetworkStatus::Disconnected);
    assert_eq!(port.stops, 0);
}
#[test]
fn active_and_terminal_status_gates_are_monotonic_without_local_publication() {
    for initial in [
        NetworkStatus::Waiting,
        NetworkStatus::Connected,
        NetworkStatus::Disconnected,
        NetworkStatus::Stopped,
    ] {
        for active in [false, true] {
            let mut port = Port::default();
            port.batch = vec![ProgressNotice::Ready, ProgressNotice::Connected];
            let mut status = initial;
            assert!(poll_progress(&mut port, &mut status, active).is_ok());
            let expected =
                if active && matches!(initial, NetworkStatus::Waiting | NetworkStatus::Connected) {
                    NetworkStatus::Connected
                } else {
                    initial
                };
            assert_eq!(status, expected);
            assert_eq!(port.consumed.get(), 2);
            assert_eq!(port.readiness_reads.get(), 0);
            assert_eq!(port.start_reads.get(), 0);
            assert!(port.published.is_empty());
            assert!(port.rooms.is_empty());
            assert_eq!(port.stops, 0);
        }
    }
    let mut port = Port::default();
    let error = Arc::new(81);
    port.batch = vec![
        ProgressNotice::Disconnected(Opaque(error)),
        ProgressNotice::Ready,
    ];
    let mut status = NetworkStatus::Stopped;
    assert!(poll_progress(&mut port, &mut status, true).is_err());
    assert_eq!(status, NetworkStatus::Stopped);
    assert_eq!(port.consumed.get(), 2);
}
#[test]
fn every_allowed_due_readiness_and_required_start_combination_admits_only_actual_publication() {
    let rows = members();
    for allowed in [false, true] {
        for due in [false, true] {
            for ready in [false, true] {
                for require_start in [false, true] {
                    for started in [false, true] {
                        let mut port = Port {
                            ready,
                            started,
                            ..Default::default()
                        };
                        let expected = allowed && due && ready && (!require_start || started);
                        match publish_progress(&mut port, &rows, allowed, require_start, due) {
                            Ok(admitted) => assert_eq!(admitted, expected),
                            Err(_) => panic!("healthy port refused"),
                        }
                        assert_eq!(port.published.len(), usize::from(expected));
                        assert!(port.rooms.is_empty());
                        assert_eq!(port.readiness_reads.get(), usize::from(allowed && due));
                        assert_eq!(
                            port.start_reads.get(),
                            usize::from(allowed && due && ready && require_start)
                        );
                        if expected {
                            assert_eq!(port.published[0], rows);
                            assert_eq!(port.borrowed, [(rows.as_ptr() as usize, 2)]);
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn refusal_returns_original_opaque_error_and_does_not_report_admitted_or_route_as_room() {
    let rows = members();
    let error = Arc::new(42);
    let mut port = Port {
        ready: true,
        started: true,
        publication_error: Some(error.clone()),
        ..Default::default()
    };
    match publish_progress(&mut port, &rows, true, true, true) {
        Err(returned) => assert!(Arc::ptr_eq(&returned.0, &error)),
        Ok(_) => panic!("refused write marked admitted"),
    }
    assert_eq!(port.published, [rows.to_vec()]);
    assert!(port.rooms.is_empty());
    assert_eq!(port.borrowed, [(rows.as_ptr() as usize, 2)]);
    assert_eq!(port.stops, 0);
}
#[test]
fn room_observation_preserves_exact_borrowed_members_without_bilateral_gates_or_completion() {
    let rows = members();
    let mut port = Port::default();
    assert!(observe_room_progress(&mut port, &rows).is_ok());
    assert_eq!(port.rooms, [rows.to_vec()]);
    assert!(port.published.is_empty());
    assert_eq!(port.borrowed, [(rows.as_ptr() as usize, 2)]);
    assert_eq!(port.readiness_reads.get(), 0);
    assert_eq!(port.start_reads.get(), 0);
    assert_eq!(port.stops, 0);
    let error = Arc::new(92);
    port.room_error = Some(error.clone());
    match observe_room_progress(&mut port, &rows) {
        Err(returned) => assert!(Arc::ptr_eq(&returned.0, &error)),
        Ok(()) => panic!("room refusal ignored"),
    }
    assert_eq!(port.rooms.len(), 2);
    assert!(port.published.is_empty());
}
