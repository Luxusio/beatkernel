use beatkernel::{
    input::{BackendId, DeviceId, EventMeta, NativeEventMeta},
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use beatkernel_platform::input::hid_report::{
    normalize_report, HidReportConversionError as Error, NativeReportLayout as Layout,
    MAX_NATIVE_REPORT_BYTES,
};

fn meta() -> EventMeta {
    let mut meta = EventMeta::new(
        DeviceId(44),
        ClockPoint {
            domain: ClockDomainId(3),
            timestamp: Timestamp::from_nanos(500),
        },
        123,
    );
    let origin = ClockPoint {
        domain: ClockDomainId(9),
        timestamp: Timestamp::from_nanos(900),
    };
    meta.original_clock_point = Some(origin);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(71),
        code: Some(7),
        timestamp: Some(origin),
    });
    meta
}

#[test]
fn declared_framing_disambiguates_payload_that_begins_with_id() {
    let native = [7, 7, 0xff, 0];
    let separate = normalize_report(meta(), 7, &native, Layout::SeparateId, 4).unwrap();
    let leading = normalize_report(meta(), 7, &native, Layout::LeadingId, 4).unwrap();
    assert_eq!(separate.data, [7, 7, 0xff, 0]);
    assert_eq!(leading.data, [7, 0xff, 0]);
    assert_eq!(separate.report_id, Some(7));
    assert_eq!(leading.report_id, Some(7));
    assert_eq!(separate.meta, meta());
    assert_eq!(leading.meta, meta());
    assert_eq!(native, [7, 7, 0xff, 0]);
}

#[test]
fn unnumbered_leading_zero_is_payload_and_empty_remains_raw_data() {
    for layout in [Layout::SeparateId, Layout::LeadingId] {
        let report = normalize_report(meta(), 0, &[0, 1, 0], layout, 3).unwrap();
        assert_eq!(report.report_id, None);
        assert_eq!(report.data, [0, 1, 0]);
        let empty = normalize_report(meta(), 0, &[], layout, 1).unwrap();
        assert_eq!(empty.report_id, None);
        assert!(empty.data.is_empty());
        assert_eq!(empty.meta, meta());
    }
}

#[test]
fn prefix_validation_and_native_capacity_include_the_id_byte() {
    assert_eq!(
        normalize_report(meta(), 7, &[], Layout::LeadingId, 1),
        Err(Error::MissingReportId)
    );
    assert_eq!(
        normalize_report(meta(), 7, &[8], Layout::LeadingId, 1),
        Err(Error::ReportIdMismatch)
    );
    assert_eq!(
        normalize_report(meta(), 7, &[7, 1], Layout::LeadingId, 1),
        Err(Error::ReportCapacity)
    );
    let report = normalize_report(meta(), 255, &[255], Layout::LeadingId, 1).unwrap();
    assert_eq!(report.report_id, Some(255));
    assert!(report.data.is_empty());
}

#[test]
fn integer_ids_and_limits_are_rejected_without_truncation() {
    for id in [256, 257, u32::MAX] {
        assert_eq!(
            normalize_report(meta(), id, &[1], Layout::SeparateId, 1),
            Err(Error::InvalidReportId)
        );
    }
    for limit in [0, MAX_NATIVE_REPORT_BYTES + 1, usize::MAX] {
        assert_eq!(
            normalize_report(meta(), 1, &[1], Layout::LeadingId, limit),
            Err(Error::InvalidLimit)
        );
    }
}
