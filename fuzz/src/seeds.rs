//! Reproducible corpus inputs built with real production encoders and UTF-8 BMS.

use crate::codecs::{input_limits, replay_limits};
use beatkernel::{
    input::*,
    replay::{
        codec::{encode_replay, ReplayFile},
        ReplayHeader, ReplayOperation, ReplayRecord, REPLAY_VERSION,
    },
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use beatkernel_bms_runtime::{
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{GroupRoomMember, GroupRoomPhase},
    multiplayer_protocol::Progress,
    multiplayer_room_wire::{encode_message, RoomMessage},
    multiplayer_rooms::ParticipantId,
    multiplayer_start::StartMessage,
};

/// Returns target names and source-derived inputs. Invalid chart modes are
/// deliberate compiler witnesses; encoding errors in fixed fixtures are bugs.
pub fn seed_cases() -> Vec<(&'static str, Vec<u8>)> {
    let events = physical_inputs();
    let mut seeds: Vec<_> = events
        .iter()
        .map(|event| {
            (
                "input_codec",
                encode_event(event, input_limits()).expect("fixed physical seed encodes"),
            )
        })
        .collect();
    let mut records: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(index, physical)| ReplayRecord {
            ordinal: index as u64,
            song_time: Timestamp::from_nanos(index as i64),
            operation: ReplayOperation::Input(GameInputEvent {
                game_control: GameControlId(7),
                physical,
            }),
        })
        .collect();
    records.push(ReplayRecord {
        ordinal: records.len() as u64,
        song_time: Timestamp::from_nanos(records.len() as i64),
        operation: ReplayOperation::Advance,
    });
    let mut replay = ReplayFile::new(
        ReplayHeader {
            version: REPLAY_VERSION,
            chart_identity: b"seed-chart\0\xff".to_vec(),
            rules_identity: b"seed-rules/v1".to_vec(),
            options: vec![0, 255, 73],
            seed: 73,
            normalized_clock: ClockDomainId(3),
        },
        records,
    );
    replay.runtime_version = "beatkernel-fuzz-source/v1".into();
    replay.calibration_metadata = Some(vec![255, 0, 73]);
    seeds.push((
        "replay_codec",
        encode_replay(&replay, replay_limits()).expect("fixed replay seed encodes"),
    ));
    seeds.extend(room_messages().iter().map(|message| {
        (
            "room_codec",
            encode_message(message).expect("fixed room seed encodes"),
        )
    }));
    seeds.extend(
        BMS_TEXTS
            .iter()
            .map(|text| ("bms_parser", text.as_bytes().to_vec())),
    );
    for mode in 0..8 {
        seeds.push(("chart_compiler", chart_header(mode)));
    }
    // A header-only object exercises arbitrary's scalar zero-padding. Its
    // metadata length is also zero, so construction and compilation succeed.
    let mut padded = chart_header(0);
    padded[14] = 1;
    seeds.push(("chart_compiler", padded));
    seeds.push(("chart_compiler", scheduled_chart()));
    seeds
}

fn physical_inputs() -> Vec<PhysicalInputEvent> {
    let meta = EventMeta {
        source: DeviceId(42),
        timestamp: Timestamp::from_nanos(-17),
        clock_domain: ClockDomainId(3),
        sequence: 99,
        native: Some(NativeEventMeta {
            backend: BackendId(12),
            code: Some(255),
            timestamp: Some(ClockPoint {
                domain: ClockDomainId(5),
                timestamp: Timestamp::from_nanos(1234),
            }),
        }),
        original_clock_point: Some(ClockPoint {
            domain: ClockDomainId(9),
            timestamp: Timestamp::from_nanos(-987),
        }),
    };
    let control = PhysicalControlId::Vendor {
        namespace: VendorNamespaceId(17),
        code: 29,
    };
    let mut events = Vec::new();
    for state in [ButtonState::Down, ButtonState::Up, ButtonState::Repeat] {
        events.push(PhysicalInputEvent::Button(ButtonEvent {
            meta,
            control,
            state,
        }));
    }
    for bits in [0, 0x8000_0000, 0x7fc0_0073, 0xffa0_0042] {
        for mode in [AxisMode::Absolute, AxisMode::Relative] {
            events.push(PhysicalInputEvent::Axis(AxisEvent {
                meta,
                control,
                value: f32::from_bits(bits),
                mode,
            }));
        }
    }
    for phase in [
        TouchPhase::Down,
        TouchPhase::Move,
        TouchPhase::Up,
        TouchPhase::Cancel,
    ] {
        events.push(PhysicalInputEvent::Touch(TouchEvent {
            meta,
            control,
            contact: ContactId(23),
            phase,
            position: Position2 {
                x: -0.0,
                y: f32::from_bits(0x7fc0_0073),
            },
            pressure: if phase == TouchPhase::Up {
                None
            } else {
                Some(f32::from_bits(0xffa0_0042))
            },
        }));
    }
    for mode in [PointerMode::Absolute, PointerMode::Relative] {
        events.push(PhysicalInputEvent::Pointer(PointerEvent {
            meta,
            control,
            mode,
            position: Position2 { x: 0.0, y: -0.0 },
        }));
    }
    events.push(PhysicalInputEvent::Pose(PoseEvent {
        meta,
        control,
        position: Position3 {
            x: -0.0,
            y: f32::from_bits(1),
            z: f32::INFINITY,
        },
        orientation: Quaternion {
            x: f32::from_bits(0x7fc0_0073),
            y: f32::from_bits(0xffa0_0042),
            z: 0.0,
            w: -0.0,
        },
    }));
    for report_id in [None, Some(255)] {
        events.push(PhysicalInputEvent::RawHidReport(RawHidReportEvent {
            meta,
            report_id,
            data: vec![0, 255, 73],
        }));
    }
    events.push(PhysicalInputEvent::Custom(CustomInputEvent {
        meta,
        namespace: VendorNamespaceId(73),
        type_id: 42,
        payload: vec![255, 0, 128, 73],
    }));
    events
}

fn room_messages() -> Vec<RoomMessage> {
    let participant = ParticipantId(7);
    let prefix = GroupPrefix {
        sequence: 73,
        final_prefix: true,
        members: vec![MemberProgress {
            player: PlayerId(2),
            progress: Progress {
                song_ns: 1234,
                hits: 10,
                misses: 2,
                combo: 3,
                max_combo: 5,
            },
        }],
    };
    let mut messages = vec![
        RoomMessage::Join {
            identity: vec![73; 65_536],
            players: (1..=64).map(PlayerId).collect(),
        },
        RoomMessage::Admitted { participant },
        RoomMessage::Snapshot {
            members: vec![GroupRoomMember {
                id: participant,
                players: vec![PlayerId(2)],
                prepared: false,
            }],
            phase: GroupRoomPhase::Collecting,
            deadline_ns: Some(1234),
        },
        RoomMessage::Seal,
        RoomMessage::Ready,
        RoomMessage::Leave,
        RoomMessage::ClockPing {
            sequence: 73,
            sent_ns: 1234,
        },
        RoomMessage::ClockPong {
            sequence: 73,
            sent_ns: 1234,
            received_ns: 1235,
            replied_ns: 1236,
        },
        RoomMessage::Progress(prefix.clone()),
        RoomMessage::PeerProgress {
            participant,
            prefix,
        },
        RoomMessage::FinalAck {
            participant,
            sequence: 73,
        },
        RoomMessage::DrainReady {
            participant,
            sequence: 73,
        },
        RoomMessage::DrainComplete {
            participant,
            sequence: 73,
        },
    ];
    for start in [
        StartMessage::ClockReady(1234),
        StartMessage::Propose(1235),
        StartMessage::Accept(1235),
        StartMessage::Commit(1235),
    ] {
        messages.push(RoomMessage::Start(start));
    }
    messages
}

const BMS_TEXTS: &[&str] = &[
    "#TITLE 時間と音\n#BPM 90\n#BPM01 180\n#STOP01 48\n#WAV01 音.wav\n#WAV02 tail.wav\n#BMP01 image.png\n#00008:00010000\n#00009:00010000\n#00011:01010000\n#00052:00010002\n#00001:00010002\n#00004:00010000\n#00006:00010000\n#00007:00010000\n#0000A:00010000\n#0000B:00800000\n#0000C:00400000\n#0000D:00FF0000\n#0000E:00010000\n#00031:00010000\n#000D2:0001ZZ00\n",
    "#RANDOM 6\n#IF 6\n#TITLE branch-six\n#WAV01 six.wav\n#00011:01\n#ELSEIF 5\n#TITLE branch-five\n#WAV02 five.wav\n#00012:02\n#ELSE\n#BPM malformed\n#ENDIF\n#ENDRANDOM\n",
    "#BASE 62\n#BPM 120\n#RANK +0004\n#DEFEXRANK 87.5\n#WAV0A upper.wav\n#WAV0a lower.wav\n#00011:0A0a\n",
];

fn chart_header(mode: u8) -> Vec<u8> {
    let mut data = vec![mode, 0];
    for value in [480u32, 120, 1] {
        data.extend(value.to_le_bytes());
    }
    data.extend([0; 4]);
    data
}

fn scheduled_chart() -> Vec<u8> {
    let mut data = chart_header(0);
    data[14..18].copy_from_slice(&[1, 1, 1, 1]);
    data.extend(73u64.to_le_bytes()); // object ID
    data.extend(480u32.to_le_bytes()); // start, domain 0
    data.push(1); // hold
    data.extend(960u32.to_le_bytes()); // duration ticks
    data.extend(7u32.to_le_bytes()); // interaction
    data.extend(11u32.to_le_bytes()); // visual
    data.push(1); // audio present
    data.extend(13u32.to_le_bytes());
    data.push(3); // metadata length
    data.extend([0, 255, 73]);
    for value in [960u32, 180, 1] {
        data.extend(value.to_le_bytes());
    } // BPM
    for value in [960u32, 100_000_000] {
        data.extend(value.to_le_bytes());
    } // STOP
    data.extend(480u32.to_le_bytes()); // scroll beat
    data.extend((-1i64).to_le_bytes());
    data.extend(2u32.to_le_bytes());
    data
}
