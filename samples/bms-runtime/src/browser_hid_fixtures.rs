//! Deferred WebHID boundary: actual core codec and platform SeparateId normalization.
use crate::browser_input::decode_input;
use beatkernel::{
    input::{
        BackendId, CodecLimits, DeviceId, EventMeta, NativeEventMeta, PhysicalInputEvent,
        RawHidReportEvent, encode_event,
    },
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use beatkernel_platform::input::hid_report::{
    HidReportConversionError, NativeReportLayout, normalize_report,
};

const HOST: ClockDomainId = ClockDomainId(0x57494e);
// Independent literal shared with physical-input.test.mjs, not computed by a fixture codec.
const NUMBERED: [u8; 73] = [
    66, 75, 80, 73, 1, 0, 5, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 8, 7, 6, 5, 4, 3, 2,
    1, 0x4e, 0x49, 0x57, 0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 1, 0x44, 0x49, 0x48,
    0x57, 1, 0x7f, 0, 0, 0, 1, 0x4e, 0x49, 0x57, 0, 8, 7, 6, 5, 4, 3, 2, 1, 0, 1, 0x7f, 4, 0, 0, 0,
    0, 0, 0, 0, 0x7f, 0, 0xff, 0x80,
];
fn metadata(report_id: u32) -> EventMeta {
    let host = ClockPoint {
        domain: HOST,
        timestamp: Timestamp::from_nanos(0x0102030405060708),
    };
    EventMeta {
        source: DeviceId(0xfedc_ba98_7654_3210),
        timestamp: host.timestamp,
        clock_domain: host.domain,
        sequence: 0x8877_6655_4433_2211,
        native: Some(NativeEventMeta {
            backend: BackendId(0x57484944),
            code: Some(report_id),
            timestamp: Some(host),
        }),
        original_clock_point: None,
    }
}

#[test]
fn literal_webhid_packet_round_trips_through_actual_core_with_full_provenance_and_no_id_stripping()
{
    let expected = PhysicalInputEvent::RawHidReport(RawHidReportEvent {
        meta: metadata(0x7f),
        report_id: Some(0x7f),
        data: vec![0x7f, 0, 0xff, 0x80],
    });
    let limits = CodecLimits::new(4096, 1024).unwrap();
    assert_eq!(decode_input(&NUMBERED, limits, HOST).unwrap(), expected);
    assert_eq!(encode_event(&expected, limits).unwrap(), NUMBERED);
    let normalized = normalize_report(
        metadata(0x7f),
        0x7f,
        &[0x7f, 0, 0xff, 0x80],
        NativeReportLayout::SeparateId,
        1024,
    )
    .unwrap();
    assert_eq!(PhysicalInputEvent::RawHidReport(normalized), expected);
    for length in [6, 35, 58, 59, 60, 64, 68, 72] {
        assert!(decode_input(&NUMBERED[..length], limits, HOST).is_err());
    }
    let mut trailing = NUMBERED.to_vec();
    trailing.push(0);
    assert!(decode_input(&trailing, limits, HOST).is_err());
    let mut invalid_tag = NUMBERED;
    invalid_tag[59] = 2;
    assert!(decode_input(&invalid_tag, limits, HOST).is_err());
    assert!(decode_input(&NUMBERED, limits, ClockDomainId(HOST.0 + 1)).is_err());
    assert!(decode_input(&NUMBERED, CodecLimits::new(72, 4).unwrap(), HOST).is_err());
    assert!(decode_input(&NUMBERED, CodecLimits::new(73, 3).unwrap(), HOST).is_err());
}

#[test]
fn actual_separate_id_normalization_preserves_zero_empty_and_bounded_payloads_before_canonical_decode()
 {
    for id in [0, 1, 255] {
        for payload in [vec![], vec![id as u8, 0, 255], vec![id as u8; 1024]] {
            let mut meta = metadata(id);
            meta.source = DeviceId(u64::MAX);
            meta.sequence = u64::MAX;
            meta.timestamp = Timestamp::from_nanos(i64::MAX);
            let point = ClockPoint {
                domain: HOST,
                timestamp: meta.timestamp,
            };
            meta.native.as_mut().unwrap().timestamp = Some(point);
            let report =
                normalize_report(meta, id, &payload, NativeReportLayout::SeparateId, 1024).unwrap();
            assert_eq!(report.meta, meta);
            assert_eq!(report.report_id, (id != 0).then_some(id as u8));
            assert_eq!(report.data, payload);
            let event = PhysicalInputEvent::RawHidReport(report);
            let bytes = encode_event(&event, CodecLimits::new(4096, 1024).unwrap()).unwrap();
            assert_eq!(bytes.len(), (if id == 0 { 68 } else { 69 }) + payload.len());
            let exact = CodecLimits::new(bytes.len(), payload.len()).unwrap();
            assert_eq!(decode_input(&bytes, exact, HOST).unwrap(), event);
            if id == 0 {
                assert_eq!(
                    &bytes[59..68],
                    &[
                        0,
                        payload.len() as u8,
                        (payload.len() >> 8) as u8,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0
                    ]
                );
            }
        }
    }
    assert_eq!(
        normalize_report(metadata(1), 256, &[1], NativeReportLayout::SeparateId, 1024).unwrap_err(),
        HidReportConversionError::InvalidReportId
    );
    assert_eq!(
        normalize_report(
            metadata(1),
            1,
            &[1; 1025],
            NativeReportLayout::SeparateId,
            1024
        )
        .unwrap_err(),
        HidReportConversionError::ReportCapacity
    );
    let mut negative = metadata(0);
    negative.timestamp = Timestamp::from_nanos(-1);
    let event = PhysicalInputEvent::RawHidReport(
        normalize_report(negative, 0, &[], NativeReportLayout::SeparateId, 1024).unwrap(),
    );
    let limits = CodecLimits::new(4096, 1024).unwrap();
    assert!(decode_input(&encode_event(&event, limits).unwrap(), limits, HOST).is_err());
}
