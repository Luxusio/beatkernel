//! Bounded browser setup for the same HID profiles used by native adapters.
//! Numeric rows preserve physical identity; they never create logical bindings.

use std::sync::Arc;

use beatkernel::input::{
    AxisMode, Binding, DeviceId, DeviceSelector, EventMeta, PhysicalInputSink, RawHidReportEvent,
};
use beatkernel_platform::input::{
    hid_profile::{
        HidBitOrder, HidFieldKind, HidFieldSpec, HidProfile, HidProfileAdapter, HidProfileMatch,
        HidReportSpec,
    },
    validate_report_order,
};

use crate::browser_input::decode_physical_control;

const MAX_SOURCES: usize = 16;
const MAX_REPORTS: usize = 256;
const MAX_FIELDS: usize = 256;
const MAX_ROWS: usize = MAX_SOURCES * 512;

struct DeviceDraft {
    source: DeviceId,
    matcher: HidProfileMatch,
    reports: Vec<HidReportSpec>,
    fields: usize,
}

#[derive(Debug)]
struct Source {
    id: DeviceId,
    adapter: HidProfileAdapter,
    last_meta: Option<EventMeta>,
}

/// Complete device/profile configuration with independent retained source state.
#[derive(Debug)]
pub struct BrowserHidSetup {
    sources: Vec<Source>,
}

impl BrowserHidSetup {
    /// Validates six-word device rows, thirteen-word field rows and two f32
    /// parameters per field row before returning any usable configuration.
    /// Every real field must already have an Any or matching Exact binding.
    /// All three empty slices configure no devices without creating a profile.
    pub fn new(
        device_words: &[u32],
        field_words: &[u32],
        axis_params: &[f32],
        bindings: &[Binding],
    ) -> Result<Self, String> {
        if device_words.len() > MAX_SOURCES * 6 || device_words.len() % 6 != 0 {
            return Err("HID devices require at most sixteen complete six-word rows".into());
        }
        if field_words.len() > MAX_ROWS * 13
            || field_words.len() % 13 != 0
            || axis_params.len() != (field_words.len() / 13) * 2
        {
            return Err(
                "HID fields require bounded thirteen-word rows and exactly two parameters per row"
                    .into(),
            );
        }
        if bindings.len() > 256 {
            return Err("browser HID setup exceeds the constructor binding capacity".into());
        }
        let count = device_words.len() / 6;
        let mut drafts: Vec<DeviceDraft> = Vec::new();
        drafts
            .try_reserve_exact(count)
            .map_err(|_| "HID device setup allocation failed")?;
        for row in device_words.chunks_exact(6) {
            let source = DeviceId(u64::from(row[0]) | (u64::from(row[1]) << 32));
            if source.0 < 3 || drafts.iter().any(|entry| entry.source == source) {
                return Err("HID sources must be distinct identities at least three".into());
            }
            drafts.push(DeviceDraft {
                source,
                matcher: HidProfileMatch {
                    vendor_id: hardware_id(row[2], row[3])?,
                    product_id: hardware_id(row[4], row[5])?,
                },
                reports: Vec::new(),
                fields: 0,
            });
        }
        for (row, params) in field_words
            .chunks_exact(13)
            .zip(axis_params.chunks_exact(2))
        {
            let index =
                usize::try_from(row[0]).map_err(|_| "HID device index is unrepresentable")?;
            let draft = drafts
                .get_mut(index)
                .ok_or("HID field references an unknown device index")?;
            let report_id = match (row[1], row[2]) {
                (0, 0) => None,
                (1, value @ 1..=255) => Some(value as u8),
                _ => return Err("HID report tag and ID are not canonical".into()),
            };
            if row[3] > 1024 {
                return Err("HID report payload exceeds 1024 bytes".into());
            }
            let payload_bytes = row[3] as usize;
            let existing = draft
                .reports
                .iter()
                .position(|report| report.report_id == report_id);
            let report_index = match existing {
                Some(index) => index,
                None => {
                    if draft.reports.len() == MAX_REPORTS {
                        return Err("HID device exceeds 256 report definitions".into());
                    }
                    draft
                        .reports
                        .try_reserve(1)
                        .map_err(|_| "HID report setup allocation failed")?;
                    draft.reports.push(HidReportSpec {
                        report_id,
                        payload_bytes,
                        fields: Vec::new(),
                    });
                    draft.reports.len() - 1
                }
            };
            let report = &mut draft.reports[report_index];
            if report.payload_bytes != payload_bytes {
                return Err("HID report rows disagree on the exact payload extent".into());
            }
            if existing.is_some() && (report.fields.is_empty() || row[10] == 2) {
                return Err("HID empty-report rows must be exclusive to their report".into());
            }
            if row[10] == 2 {
                if row[4..10].iter().any(|value| *value != 0)
                    || row[11] != 0
                    || row[12] != 0
                    || params.iter().any(|value| value.to_bits() != 0)
                {
                    return Err(
                        "HID empty-report rows require zero unused words and parameters".into(),
                    );
                }
                continue;
            }
            if draft.fields == MAX_FIELDS {
                return Err("HID device exceeds 256 physical fields".into());
            }
            let control = decode_physical_control(row[4], row[5], row[6])?;
            if !bindings.iter().any(|binding| {
                binding.physical == control
                    && (binding.device == DeviceSelector::Any
                        || binding.device == DeviceSelector::Exact(draft.source))
            }) {
                return Err("HID physical control has no Any or matching source binding".into());
            }
            let width_bits = u8::try_from(row[8]).map_err(|_| "HID field width exceeds u8")?;
            let order = match row[9] {
                0 => HidBitOrder::LeastSignificantFirst,
                1 => HidBitOrder::MostSignificantFirst,
                _ => return Err("HID field has an invalid bit order".into()),
            };
            let flag = match row[11] {
                0 => false,
                1 => true,
                _ => return Err("HID field flag must be zero or one".into()),
            };
            let kind = match row[10] {
                0 => {
                    if row[12] != 0 || params.iter().any(|value| value.to_bits() != 0) {
                        return Err("HID button rows require zero axis mode and parameters".into());
                    }
                    HidFieldKind::Button { invert: flag }
                }
                1 => HidFieldKind::Axis {
                    signed: flag,
                    mode: match row[12] {
                        0 => AxisMode::Absolute,
                        1 => AxisMode::Relative,
                        _ => return Err("HID axis mode must be absolute or relative".into()),
                    },
                    scale: params[0],
                    offset: params[1],
                },
                _ => return Err("HID field kind must be button, axis or empty report".into()),
            };
            report
                .fields
                .try_reserve(1)
                .map_err(|_| "HID field setup allocation failed")?;
            report.fields.push(HidFieldSpec {
                control,
                offset_bits: row[7],
                width_bits,
                order,
                kind,
            });
            draft.fields += 1;
        }
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(count)
            .map_err(|_| "HID adapter setup allocation failed")?;
        for draft in drafts {
            let profile =
                HidProfile::new(draft.matcher, draft.reports).map_err(|error| error.to_string())?;
            sources.push(Source {
                id: draft.source,
                adapter: HidProfileAdapter::new(Arc::new(profile)),
                last_meta: None,
            });
        }
        Ok(Self { sources })
    }

    /// Returns the exact number of configured source owners, including no devices.
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// Validates acquisition order and decodes through the common profile adapter.
    /// Refusal leaves prior levels/order unchanged; successful zero-event reports
    /// still adopt metadata. Caller-owned sink publication follows common adapter
    /// semantics and cannot be rolled back after a sink panic.
    pub fn decode_report(
        &mut self,
        report: &RawHidReportEvent,
        out: &mut dyn PhysicalInputSink,
    ) -> Result<usize, String> {
        let source = self
            .sources
            .iter_mut()
            .find(|source| source.id == report.meta.source)
            .ok_or("HID report source is not configured")?;
        validate_report_order(source.last_meta, report.meta).map_err(|error| error.to_string())?;
        let emitted = source
            .adapter
            .decode_report(report, out)
            .map_err(|error| error.to_string())?;
        source.last_meta = Some(report.meta);
        Ok(emitted)
    }
}

fn hardware_id(tag: u32, value: u32) -> Result<Option<u16>, String> {
    match (tag, value) {
        (0, 0) => Ok(None),
        (1, value) => u16::try_from(value)
            .map(Some)
            .map_err(|_| "HID hardware ID exceeds u16".into()),
        _ => Err("HID hardware IDs require canonical zero/one tags and absent zero values".into()),
    }
}
