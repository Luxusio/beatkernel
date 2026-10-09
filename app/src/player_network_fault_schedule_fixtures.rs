//! Real accepted gameplay prefixes through portable publication and wire owners.
//! Scripts inject only I/O admission, capture times and full-write receipts.
use crate::{
    competition_progress::{CompetitionProgressPort, ProgressNotice},
    competition_progress_cadence::{
        publish_progress_with_clock, CadenceError, CompetitionProgressClock, ProgressCadence,
    },
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_protocol::Progress,
    multiplayer_room_progress::{RoomProgressError, RoomProgressRelay},
    multiplayer_room_progress_client::{RoomProgressClient, RoomProgressClientError},
    multiplayer_room_wire::{decode_message, encode_message, RoomMessage},
    multiplayer_rooms::ParticipantId,
};
use beatkernel::{
    audio::{command_queue, CommandConsumer},
    chart::*,
    input::*,
    interaction::InstantEvaluator,
    judge::*,
    runtime::{Runtime, RuntimeReport},
    time::*,
    transport::{Rate, Transport},
};
use std::collections::VecDeque;

const TARGETS: [i64; 3] = [10_000_000, 20_000_000, 30_000_000];
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(1),
        timestamp: ts(n),
    }
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
struct Gameplay {
    runtime: Runtime,
    _consumer: CommandConsumer,
    chart: CompiledChart,
    reports: Vec<RuntimeReport>,
    hash: Option<u64>,
}
impl Gameplay {
    fn new() -> Self {
        let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
        source.objects = TARGETS
            .into_iter()
            .enumerate()
            .map(|(index, at)| SourceObject {
                id: ObjectId(index as u64 + 101),
                start: Beat::new(at).unwrap(),
                end: None,
                interaction: InteractionId(1),
                visual: VisualId(1),
                audio: None,
                metadata: ObjectMetadata::default(),
            })
            .collect();
        let chart = source.compile().unwrap();
        let judge = JudgeEngine::new(
            chart.clone(),
            vec![Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(InstantEvaluator),
            }],
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(7),
                    early: Duration::ZERO,
                    late: Duration::ZERO,
                }],
                Duration::ZERO,
            )
            .unwrap(),
        )
        .unwrap();
        let bindings = BindingMap::from_bindings((1..=3).map(|key| Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(key),
            game_control: GameControlId(1),
        }))
        .unwrap();
        let (producer, consumer) = command_queue(4).unwrap();
        let runtime = Runtime::new(
            ClockDomainId(1),
            ClockDomainId(1),
            Transport::new(ts(0), ts(0), Rate::NORMAL),
            bindings,
            judge,
            producer,
            vec![],
            4,
        )
        .unwrap();
        Self {
            runtime,
            _consumer: consumer,
            chart,
            reports: vec![],
            hash: None,
        }
    }
    fn accept(&mut self) -> [MemberProgress; 1] {
        let index = self.reports.len();
        let key = index as u16 + 1;
        let at = TARGETS[index];
        let input = PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(41), point(at), index as u64 + 901),
            control: PhysicalControlId::keyboard(key),
            state: ButtonState::Down,
        });
        let report = self
            .runtime
            .process_input(input.clone(), &Identity, point(at))
            .unwrap();
        assert_eq!(report.input, Some(input));
        assert_eq!(report.song_time, ts(at));
        assert_eq!(report.audio_at, point(at));
        assert_eq!(report.judge_error, None);
        assert_eq!(report.bound_inputs.len(), 1);
        assert_eq!(
            report.judge_events,
            vec![JudgeEvent {
                object: ObjectId(index as u64 + 101),
                stage: JudgeStage::Instant,
                outcome: JudgeOutcome::Hit {
                    grade: JudgeGrade(7),
                    delta: Duration::ZERO
                },
                at: ts(at),
                input: Some(EventMeta::new(DeviceId(41), point(at), index as u64 + 901)),
            }]
        );
        self.reports.push(report);
        self.hash = Some(self.runtime.judge().stable_hash().unwrap());
        self.unchanged();
        let progress = Progress {
            song_ns: at,
            hits: self
                .reports
                .iter()
                .flat_map(|r| &r.judge_events)
                .filter(|e| matches!(e.outcome, JudgeOutcome::Hit { .. }))
                .count() as u64,
            misses: 0,
            combo: index as u64 + 1,
            max_combo: index as u64 + 1,
        };
        let expected = match index {
            0 => Progress {
                song_ns: 10_000_000,
                hits: 1,
                misses: 0,
                combo: 1,
                max_combo: 1,
            },
            1 => Progress {
                song_ns: 20_000_000,
                hits: 2,
                misses: 0,
                combo: 2,
                max_combo: 2,
            },
            2 => Progress {
                song_ns: 30_000_000,
                hits: 3,
                misses: 0,
                combo: 3,
                max_combo: 3,
            },
            _ => unreachable!(),
        };
        assert_eq!(progress, expected);
        [MemberProgress {
            player: PlayerId(7),
            progress,
        }]
    }
    fn unchanged(&self) {
        assert_eq!(
            self.chart
                .objects()
                .iter()
                .map(|o| (o.id.0, o.time.start.as_nanos()))
                .collect::<Vec<_>>(),
            vec![(101, 10_000_000), (102, 20_000_000), (103, 30_000_000)]
        );
        if let Some(hash) = self.hash {
            assert_eq!(self.runtime.judge().stable_hash().unwrap(), hash);
        }
        for (index, report) in self.reports.iter().enumerate() {
            assert_eq!(report.song_time.as_nanos(), TARGETS[index]);
            assert_eq!(report.judge_events[0].object, ObjectId(index as u64 + 101));
            assert_eq!(report.judge_events[0].stage, JudgeStage::Instant);
            assert_eq!(report.judge_events[0].at.as_nanos(), TARGETS[index]);
            assert_eq!(
                report.judge_events[0].outcome,
                JudgeOutcome::Hit {
                    grade: JudgeGrade(7),
                    delta: Duration::ZERO
                }
            );
            assert_eq!(
                report.judge_events[0].input,
                Some(EventMeta::new(
                    DeviceId(41),
                    point(TARGETS[index]),
                    index as u64 + 901
                ))
            );
        }
        assert_eq!(
            self.runtime.judge().effective_song_time(),
            self.reports.last().map(|r| r.song_time)
        );
    }
}
#[derive(Debug, PartialEq, Eq)]
enum AdmissionError {
    Refused,
    Client(RoomProgressClientError),
}
struct Network {
    own: ParticipantId,
    peer: ParticipantId,
    client: RoomProgressClient,
    remote: RoomProgressClient,
    relay: RoomProgressRelay,
    refuse: bool,
    admitted: Vec<Vec<MemberProgress>>,
    stops: usize,
}
impl Network {
    fn new() -> Self {
        let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 8, 100).unwrap());
        let own = registry
            .join("fault", b"accepted runtime", &[PlayerId(7)], 0)
            .unwrap()
            .id;
        let peer = registry
            .join("fault", b"accepted runtime", &[PlayerId(11)], 0)
            .unwrap()
            .id;
        registry.seal(own, 1).unwrap();
        registry.ready(own, 2).unwrap();
        registry.ready(peer, 2).unwrap();
        let snapshot = registry.room("fault").unwrap();
        let mut client = RoomProgressClient::new(snapshot, own).unwrap();
        let mut remote = RoomProgressClient::new(snapshot, peer).unwrap();
        let mut relay = RoomProgressRelay::new(snapshot).unwrap();
        client.activate().unwrap();
        remote.activate().unwrap();
        relay.activate().unwrap();
        Self {
            own,
            peer,
            client,
            remote,
            relay,
            refuse: false,
            admitted: vec![],
            stops: 0,
        }
    }
}
impl CompetitionProgressPort for Network {
    type Error = AdmissionError;
    type Notices = std::iter::Empty<ProgressNotice<AdmissionError>>;
    fn notices(&mut self) -> Self::Notices {
        std::iter::empty()
    }
    fn ready(&self) -> bool {
        self.client.active()
    }
    fn started(&self) -> bool {
        self.client.active()
    }
    fn publish(&mut self, members: &[MemberProgress]) -> Result<(), Self::Error> {
        if std::mem::take(&mut self.refuse) {
            return Err(AdmissionError::Refused);
        }
        self.client
            .publish(members, false)
            .map_err(AdmissionError::Client)?;
        self.admitted.push(members.to_vec());
        Ok(())
    }
    fn observe_room(&mut self, _: &[MemberProgress]) -> Result<(), Self::Error> {
        panic!("different room-controller port")
    }
    fn request_stop(&mut self) {
        self.stops += 1;
        self.client.stop();
    }
}
struct Clock(VecDeque<Result<u64, &'static str>>);
impl CompetitionProgressClock for Clock {
    type Error = &'static str;
    fn now_ns(&mut self) -> Result<u64, Self::Error> {
        self.0
            .pop_front()
            .expect("unexpected publication clock sample")
    }
}
fn publish(
    network: &mut Network,
    cadence: &mut ProgressCadence,
    rows: &[MemberProgress],
    samples: &[Result<u64, &'static str>],
) -> Result<bool, CadenceError<AdmissionError, &'static str>> {
    let mut clock = Clock(samples.iter().copied().collect());
    let result = publish_progress_with_clock(network, &mut clock, cadence, rows, true, true);
    assert!(clock.0.is_empty());
    result
}
fn wire(bytes: &[u8]) -> RoomMessage {
    let message = decode_message(bytes).unwrap();
    assert_eq!(encode_message(&message).unwrap(), bytes);
    message
}
fn delivered(client: &mut RoomProgressClient, bytes: &[u8], captured: i64, observed: i64) {
    assert!(observed >= captured);
    client.receive(&wire(bytes), captured).unwrap();
}
fn prefix(rows: &[MemberProgress], sequence: u64, final_prefix: bool) -> GroupPrefix {
    GroupPrefix {
        sequence,
        final_prefix,
        members: rows.to_vec(),
    }
}

#[test]
fn accepted_runtime_prefixes_obey_cadence_refusal_coalescing_and_capture_atomicity() {
    for _repeat in 0..2 {
        let mut game = Gameplay::new();
        let mut network = Network::new();
        let mut cadence = ProgressCadence::new();
        let first = game.accept();
        assert!(matches!(
            publish(&mut network, &mut cadence, &first, &[Ok(0), Ok(0)]),
            Ok(true)
        ));
        assert_eq!(cadence.last_published(), Some(0));
        let upload = network.client.poll_write(10).unwrap().unwrap();
        assert_eq!(upload.id, 1);
        assert_eq!(
            wire(&upload.bytes),
            RoomMessage::Progress(prefix(&first, 1, false))
        );
        let second = game.accept();
        assert!(matches!(
            publish(&mut network, &mut cadence, &second, &[Ok(49_999_999)]),
            Ok(false)
        ));
        let before = network.client.clone();
        network.refuse = true;
        assert!(matches!(
            publish(&mut network, &mut cadence, &second, &[Ok(50_000_000)]),
            Err(CadenceError::Publication(AdmissionError::Refused))
        ));
        assert_eq!(network.client, before);
        assert_eq!(cadence.last_published(), Some(0));
        assert!(matches!(
            publish(
                &mut network,
                &mut cadence,
                &second,
                &[Ok(50_000_000), Ok(50_000_000)]
            ),
            Ok(true)
        ));
        let third = game.accept();
        assert!(matches!(
            publish(
                &mut network,
                &mut cadence,
                &third,
                &[Ok(100_000_000), Ok(100_000_001)]
            ),
            Ok(true)
        ));
        assert_eq!(cadence.last_published(), Some(100_000_001));
        assert_eq!(
            network.admitted,
            vec![first.to_vec(), second.to_vec(), third.to_vec()]
        );
        assert!(network.client.poll_write(11).unwrap().is_none());
        let before = network.client.clone();
        assert_eq!(
            network.client.written(2),
            Err(RoomProgressClientError::UnknownWrite)
        );
        assert_eq!(network.client, before);
        game.unchanged();
        // Upload was captured at 20, processed much later; its original clock is retained.
        network
            .relay
            .receive_at(network.own, &wire(&upload.bytes), 20)
            .unwrap();
        for captured in [20, 19] {
            let before = network.relay.clone();
            assert!(network
                .relay
                .receive_at(network.own, &wire(&upload.bytes), captured)
                .is_err());
            assert_eq!(network.relay, before);
        }
        network.client.written(upload.id).unwrap();
        let peer_frame = network
            .relay
            .poll_write_at(network.peer, 30)
            .unwrap()
            .unwrap();
        assert_eq!(
            wire(&peer_frame.bytes),
            RoomMessage::PeerProgress {
                participant: network.own,
                prefix: prefix(&first, 1, false)
            }
        );
        delivered(&mut network.remote, &peer_frame.bytes, 40, 10_000);
        for captured in [40, 39] {
            let before = network.remote.clone();
            assert!(network
                .remote
                .receive(&wire(&peer_frame.bytes), captured)
                .is_err());
            assert_eq!(network.remote, before);
        }
        let before = network.relay.clone();
        assert_eq!(
            network.relay.written(network.peer, peer_frame.id + 1),
            Err(RoomProgressError::UnknownWrite)
        );
        assert_eq!(network.relay, before);
        network.relay.written(network.peer, peer_frame.id).unwrap();
        let coalesced = network.client.poll_write(50).unwrap().unwrap();
        assert_eq!(coalesced.id, 2);
        assert_eq!(
            wire(&coalesced.bytes),
            RoomMessage::Progress(prefix(&third, 2, false))
        );
        network
            .relay
            .receive_at(network.own, &wire(&coalesced.bytes), 60)
            .unwrap();
        network.client.written(coalesced.id).unwrap();
        let latest = network
            .relay
            .poll_write_at(network.peer, 70)
            .unwrap()
            .unwrap();
        assert_eq!(
            wire(&latest.bytes),
            RoomMessage::PeerProgress {
                participant: network.own,
                prefix: prefix(&third, 2, false)
            }
        );
        delivered(&mut network.remote, &latest.bytes, 80, 20_000);
        network.relay.written(network.peer, latest.id).unwrap();
        assert_eq!(
            network.remote.peer_progress(network.own),
            Some(&prefix(&third, 2, false))
        );
        assert!(
            !network.client.local_complete()
                && !network.remote.local_complete()
                && !network.relay.complete()
        );
        game.unchanged();
    }
}

#[test]
fn actual_publication_effect_survives_failed_post_clock_and_caller_disables_before_retry() {
    for _repeat in 0..2 {
        for regression in [false, true] {
            let mut game = Gameplay::new();
            let rows = game.accept();
            let mut network = Network::new();
            let mut cadence = ProgressCadence::new();
            let before = network.client.clone();
            assert!(matches!(
                publish(
                    &mut network,
                    &mut cadence,
                    &rows,
                    &[Err("pre-clock refused")]
                ),
                Err(CadenceError::Clock("pre-clock refused"))
            ));
            assert_eq!(network.client, before);
            assert!(network.admitted.is_empty());
            let post = if regression {
                Ok(9)
            } else {
                Err("post-clock refused")
            };
            let result = publish(&mut network, &mut cadence, &rows, &[Ok(10), post]);
            if regression {
                assert!(matches!(result, Err(CadenceError::ClockRegressed)));
            } else {
                assert!(matches!(
                    result,
                    Err(CadenceError::Clock("post-clock refused"))
                ));
            }
            assert_eq!(cadence.last_observed(), Some(10));
            assert_eq!(cadence.last_published(), None);
            assert_eq!(network.admitted, vec![rows.to_vec()]);
            let performed = network.client.poll_write(20).unwrap().unwrap();
            assert_eq!(
                wire(&performed.bytes),
                RoomMessage::Progress(prefix(&rows, 1, false))
            );
            network
                .relay
                .receive_at(network.own, &wire(&performed.bytes), 30)
                .unwrap();
            network.client.written(performed.id).unwrap();
            let delivery = network
                .relay
                .poll_write_at(network.peer, 40)
                .unwrap()
                .unwrap();
            delivered(&mut network.remote, &delivery.bytes, 50, 10_000);
            network.relay.written(network.peer, delivery.id).unwrap();
            assert_eq!(
                network.remote.peer_progress(network.own),
                Some(&prefix(&rows, 1, false))
            );
            // This is the existing caller disposal contract, not effect rollback.
            network.request_stop();
            let mut no_clock = Clock(VecDeque::new());
            assert!(matches!(
                publish_progress_with_clock(
                    &mut network,
                    &mut no_clock,
                    &mut cadence,
                    &rows,
                    false,
                    true
                ),
                Ok(false)
            ));
            assert_eq!(network.stops, 1);
            assert_eq!(network.admitted.len(), 1);
            assert_eq!(cadence.last_published(), None);
            assert!(!network.client.active() && !network.client.local_complete());
            game.unchanged();
        }
    }
}

#[test]
fn final_runtime_prefix_requires_distinct_upload_peer_ack_aggregate_and_drain_receipts() {
    for _repeat in 0..2 {
        let mut game = Gameplay::new();
        game.accept();
        game.accept();
        let final_rows = game.accept();
        let mut network = Network::new();
        let remote_rows = [MemberProgress {
            player: PlayerId(11),
            progress: final_rows[0].progress,
        }];
        network.client.publish(&final_rows, true).unwrap();
        network.remote.publish(&remote_rows, true).unwrap();
        let own_ack = RoomMessage::FinalAck {
            participant: network.own,
            sequence: 1,
        };
        let before = network.client.clone();
        assert_eq!(
            network.client.receive(&own_ack, 10),
            Err(RoomProgressClientError::InvalidAck)
        );
        assert_eq!(network.client, before);
        assert!(
            !network.client.local_final_written() && !network.client.local_final_acknowledged()
        );
        let upload = network.client.poll_write(10).unwrap().unwrap();
        let remote_upload = network.remote.poll_write(10).unwrap().unwrap();
        assert_eq!((upload.id, remote_upload.id), (1, 1));
        assert_eq!(
            wire(&upload.bytes),
            RoomMessage::Progress(prefix(&final_rows, 1, true))
        );
        assert_eq!(
            wire(&remote_upload.bytes),
            RoomMessage::Progress(prefix(&remote_rows, 1, true))
        );
        network
            .relay
            .receive_at(network.own, &wire(&upload.bytes), 20)
            .unwrap();
        network
            .relay
            .receive_at(network.peer, &wire(&remote_upload.bytes), 20)
            .unwrap();
        network.remote.written(remote_upload.id).unwrap();
        // Own final remains in flight while peer really consumes its captured bytes.
        let to_remote = network
            .relay
            .poll_write_at(network.peer, 30)
            .unwrap()
            .unwrap();
        let to_own = network
            .relay
            .poll_write_at(network.own, 30)
            .unwrap()
            .unwrap();
        assert_eq!(
            wire(&to_remote.bytes),
            RoomMessage::PeerProgress {
                participant: network.own,
                prefix: prefix(&final_rows, 1, true)
            }
        );
        assert_eq!(
            wire(&to_own.bytes),
            RoomMessage::PeerProgress {
                participant: network.peer,
                prefix: prefix(&remote_rows, 1, true)
            }
        );
        delivered(&mut network.remote, &to_remote.bytes, 40, 10_000);
        delivered(&mut network.client, &to_own.bytes, 40, 10_000);
        network.relay.written(network.peer, to_remote.id).unwrap();
        network.relay.written(network.own, to_own.id).unwrap();
        let remote_ack = network.remote.poll_write(50).unwrap().unwrap();
        assert_eq!(remote_ack.id, 2);
        assert_eq!(wire(&remote_ack.bytes), own_ack);
        assert!(network.client.poll_write(50).unwrap().is_none());
        assert!(network
            .relay
            .poll_write_at(network.own, 50)
            .unwrap()
            .is_none());
        assert!(!network.relay.final_acknowledged(network.own).unwrap());
        assert!(!network.client.local_complete() && !network.relay.complete());
        let forged = RoomMessage::FinalAck {
            participant: network.own,
            sequence: 2,
        };
        let before = network.relay.clone();
        assert_eq!(
            network.relay.receive_at(network.peer, &forged, 60),
            Err(RoomProgressError::InvalidAck)
        );
        assert_eq!(network.relay, before);
        network
            .relay
            .receive_at(network.peer, &wire(&remote_ack.bytes), 60)
            .unwrap();
        let own_aggregate = network
            .relay
            .poll_write_at(network.own, 70)
            .unwrap()
            .unwrap();
        assert_eq!(wire(&own_aggregate.bytes), own_ack);
        for (message, capture) in [(forged, 80), (own_ack.clone(), 9)] {
            let before = network.client.clone();
            assert!(network.client.receive(&message, capture).is_err());
            assert_eq!(network.client, before);
        }
        delivered(&mut network.client, &own_aggregate.bytes, 80, 20_000);
        assert!(
            !network.client.local_final_written() && !network.client.local_final_acknowledged()
        );
        assert!(!network.client.local_complete() && !network.relay.complete());
        let before = network.client.clone();
        assert_eq!(
            network.client.receive(&own_ack, 81),
            Err(RoomProgressClientError::InvalidAck)
        );
        assert_eq!(network.client, before);
        assert!(network.client.request_drain().is_err());
        assert_eq!(network.client, before);
        network.client.written(upload.id).unwrap();
        assert!(network.client.local_final_written() && network.client.local_final_acknowledged());
        assert!(!network.client.local_complete());
        let ack = network.client.poll_write(90).unwrap().unwrap();
        assert_eq!(ack.id, 2);
        assert_eq!(
            wire(&ack.bytes),
            RoomMessage::FinalAck {
                participant: network.peer,
                sequence: 1
            }
        );
        network
            .relay
            .receive_at(network.own, &wire(&ack.bytes), 100)
            .unwrap();
        let remote_aggregate = network
            .relay
            .poll_write_at(network.peer, 110)
            .unwrap()
            .unwrap();
        assert_eq!(
            wire(&remote_aggregate.bytes),
            RoomMessage::FinalAck {
                participant: network.peer,
                sequence: 1
            }
        );
        delivered(&mut network.remote, &remote_aggregate.bytes, 120, 30_000);
        assert!(!network.client.local_complete() && !network.remote.local_complete());
        network.client.written(ack.id).unwrap();
        assert!(network.client.local_complete());
        assert!(!network.remote.local_complete() && !network.relay.complete());
        network.remote.written(remote_ack.id).unwrap();
        assert!(network.remote.local_complete());
        assert!(!network.relay.complete());
        network
            .relay
            .written(network.own, own_aggregate.id)
            .unwrap();
        assert!(!network.relay.complete());
        network
            .relay
            .written(network.peer, remote_aggregate.id)
            .unwrap();
        assert!(network.relay.complete() && !network.relay.drained());
        game.unchanged();
        network.client.request_drain().unwrap();
        network.remote.request_drain().unwrap();
        let ready = network.client.poll_write(130).unwrap().unwrap();
        let remote_ready = network.remote.poll_write(130).unwrap().unwrap();
        assert_eq!((ready.id, remote_ready.id), (3, 3));
        assert_eq!(
            wire(&ready.bytes),
            RoomMessage::DrainReady {
                participant: network.own,
                sequence: 1
            }
        );
        assert_eq!(
            wire(&remote_ready.bytes),
            RoomMessage::DrainReady {
                participant: network.peer,
                sequence: 1
            }
        );
        network
            .relay
            .receive_at(network.own, &wire(&ready.bytes), 140)
            .unwrap();
        assert!(network
            .relay
            .poll_write_at(network.own, 145)
            .unwrap()
            .is_none());
        network
            .relay
            .receive_at(network.peer, &wire(&remote_ready.bytes), 140)
            .unwrap();
        let notice = network
            .relay
            .poll_write_at(network.own, 150)
            .unwrap()
            .unwrap();
        let remote_notice = network
            .relay
            .poll_write_at(network.peer, 150)
            .unwrap()
            .unwrap();
        assert_eq!(
            wire(&notice.bytes),
            RoomMessage::DrainComplete {
                participant: network.own,
                sequence: 1
            }
        );
        assert_eq!(
            wire(&remote_notice.bytes),
            RoomMessage::DrainComplete {
                participant: network.peer,
                sequence: 1
            }
        );
        delivered(&mut network.client, &notice.bytes, 160, 40_000);
        delivered(&mut network.remote, &remote_notice.bytes, 160, 40_000);
        assert!(
            !network.client.drain_complete()
                && !network.remote.drain_complete()
                && !network.relay.drained()
        );
        let before = network.client.clone();
        assert_eq!(
            network.client.written(ready.id + 1),
            Err(RoomProgressClientError::UnknownWrite)
        );
        assert_eq!(network.client, before);
        network.client.written(ready.id).unwrap();
        assert!(
            network.client.drain_complete()
                && !network.remote.drain_complete()
                && !network.relay.drained()
        );
        network.remote.written(remote_ready.id).unwrap();
        assert!(network.remote.drain_complete() && !network.relay.drained());
        let before = network.relay.clone();
        assert_eq!(
            network.relay.written(network.own, notice.id + 1),
            Err(RoomProgressError::UnknownWrite)
        );
        assert_eq!(network.relay, before);
        network.relay.written(network.own, notice.id).unwrap();
        assert!(!network.relay.drained());
        network
            .relay
            .written(network.peer, remote_notice.id)
            .unwrap();
        assert!(network.relay.drained());
        game.unchanged();
    }
}
