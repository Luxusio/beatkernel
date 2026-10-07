//! Exact original-song practice start parsing and settings draft updates.
use crate::settings::NativeSettings;

const INPUT_BYTES: usize = 64;
const NANOS: u64 = 1_000_000_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeStart(i64);
impl PracticeStart {
    /// Preserves an observed original-song position exactly, without rounding.
    pub fn from_nanoseconds(nanoseconds: i64) -> Result<Self, String> {
        if nanoseconds < 0 {
            return Err("practice start requires nonnegative nanoseconds".into());
        }
        Ok(Self(nanoseconds))
    }
    /// Parses seconds, M:SS or H:MM:SS and an optional 1..9-digit fraction.
    /// Empty means zero. No signs, whitespace, exponent, rounding or float math.
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.len() > INPUT_BYTES {
            return Err("practice start exceeds 64 bytes".into());
        }
        if value.is_empty() {
            return Ok(Self(0));
        }
        let (whole, fractional) = value
            .split_once('.')
            .map_or((value, None), |(whole, fraction)| (whole, Some(fraction)));
        let mut parts = whole.split(':');
        let first = decimal(parts.next().unwrap_or(""))?;
        let seconds = match (parts.next(), parts.next(), parts.next()) {
            (None, None, None) => first,
            (Some(seconds), None, None) => {
                let seconds = decimal(seconds)?;
                if seconds >= 60 {
                    return Err("colon seconds must be below 60".into());
                }
                first
                    .checked_mul(60)
                    .and_then(|minutes| minutes.checked_add(seconds))
                    .ok_or("practice start overflow")?
            }
            (Some(minutes), Some(seconds), None) => {
                let minutes = decimal(minutes)?;
                let seconds = decimal(seconds)?;
                if minutes >= 60 || seconds >= 60 {
                    return Err("hour format minutes and seconds must be below 60".into());
                }
                first
                    .checked_mul(3600)
                    .and_then(|hours| hours.checked_add(minutes * 60 + seconds))
                    .ok_or("practice start overflow")?
            }
            _ => return Err("practice start requires seconds, M:SS or H:MM:SS".into()),
        };
        let fraction = match fractional {
            None => 0,
            Some(value) => {
                if !(1..=9).contains(&value.len()) {
                    return Err("fraction requires 1..9 decimal digits".into());
                }
                decimal(value)?
                    .checked_mul(10_u64.pow(9 - value.len() as u32))
                    .ok_or("practice fraction overflow")?
            }
        };
        let nanos = seconds
            .checked_mul(NANOS)
            .and_then(|whole| whole.checked_add(fraction))
            .and_then(|value| i64::try_from(value).ok())
            .ok_or("practice start exceeds nonnegative i64 nanoseconds")?;
        Ok(Self(nanos))
    }
    /// Reads only the existing original-song nanosecond field, not display text.
    pub fn from_settings(settings: &NativeSettings) -> Result<Self, String> {
        let value = settings
            .fields()
            .iter()
            .find(|field| field.flag == "--start-ns")
            .ok_or("practice start setting unavailable")?
            .value
            .as_str();
        if value.is_empty() {
            return Ok(Self(0));
        }
        let nanos = i64::try_from(decimal(value)?)
            .map_err(|_| "practice start exceeds nonnegative i64 nanoseconds")?;
        Ok(Self(nanos))
    }
    /// Reads the optional original-song end. Empty retains unlimited playback.
    pub fn section_end(settings: &NativeSettings) -> Result<Option<Self>, String> {
        let value = &settings
            .fields()
            .iter()
            .find(|field| field.flag == "--end-ns")
            .ok_or("practice end setting unavailable")?
            .value;
        if value.is_empty() {
            return Ok(None);
        }
        let nanos = i64::try_from(decimal(value)?)
            .map_err(|_| "practice end exceeds nonnegative i64 nanoseconds")?;
        Self::from_nanoseconds(nanos).map(Some)
    }
    /// Validates and commits both section fields together; no partial draft write.
    pub fn apply_section(
        self,
        end: Option<Self>,
        settings: &mut NativeSettings,
    ) -> Result<(), String> {
        if let Some(end) = end {
            crate::practice_loop::PracticeLoop::new(self, end)?;
        }
        let mut candidate = settings.clone();
        let start_index = candidate
            .fields()
            .iter()
            .position(|field| field.flag == "--start-ns")
            .ok_or("practice start setting unavailable")?;
        let index = candidate
            .fields()
            .iter()
            .position(|field| field.flag == "--end-ns")
            .ok_or("practice end setting unavailable")?;
        // Release both old values first so only the final section is subject to
        // the byte ceiling; replacing a full draft must not fail transiently.
        candidate.set_value(start_index, "")?;
        candidate.set_value(index, "")?;
        self.apply_to(&mut candidate)?;
        candidate.set_value(
            index,
            &end.map_or_else(String::new, |end| end.nanoseconds().to_string()),
        )?;
        *settings = candidate;
        Ok(())
    }
    pub const fn nanoseconds(self) -> i64 {
        self.0
    }
    /// Canonical M:SS/H:MM:SS with fractional trailing zeros removed.
    pub fn formatted(self) -> String {
        let seconds = self.0 / NANOS as i64;
        let mut result = if seconds >= 3600 {
            format!(
                "{}:{:02}:{:02}",
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60
            )
        } else {
            format!("{}:{:02}", seconds / 60, seconds % 60)
        };
        let fraction = self.0 % NANOS as i64;
        if fraction != 0 {
            result.push('.');
            result.push_str(format!("{fraction:09}").trim_end_matches('0'));
        }
        result
    }
    /// Changes one existing field atomically; every unrelated row remains exact.
    pub fn apply_to(self, settings: &mut NativeSettings) -> Result<(), String> {
        let index = settings
            .fields()
            .iter()
            .position(|field| field.flag == "--start-ns")
            .ok_or("practice start setting unavailable")?;
        settings.set_value(index, &self.0.to_string())
    }
}
fn decimal(value: &str) -> Result<u64, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("practice start requires unsigned decimal digits".into());
    }
    value
        .parse()
        .map_err(|_| "practice start decimal overflow".into())
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::settings::{MAX_VALUE_BYTES, SettingsHost};
    #[test]
    fn section_updates_both_endpoints_and_preserves_unrelated_settings_on_every_host() {
        for host in [
            SettingsHost::Linux,
            SettingsHost::Windows,
            SettingsHost::Macos,
        ] {
            let mut settings = NativeSettings::from_args(
                &[
                    "--bind".into(),
                    "11:04".into(),
                    "--bind".into(),
                    "12:05".into(),
                    "--preroll-ns".into(),
                    "1000000000".into(),
                ],
                host,
            )
            .unwrap();
            let unrelated = |settings: &NativeSettings| {
                settings
                    .native_args()
                    .chunks_exact(2)
                    .filter(|p| !matches!(p[0].as_str(), "--start-ns" | "--end-ns"))
                    .flat_map(|p| p.iter().cloned())
                    .collect::<Vec<_>>()
            };
            let original = unrelated(&settings);
            assert_eq!(PracticeStart::section_end(&settings).unwrap(), None);
            for (start, end) in [
                (1, 2),
                (72_000_000_000_000, 72_000_000_000_001),
                (604_800_000_000_000, 604_800_000_000_001),
                (i64::MAX - 1, i64::MAX),
            ] {
                let start = PracticeStart::from_nanoseconds(start).unwrap();
                let end = PracticeStart::from_nanoseconds(end).unwrap();
                start.apply_section(Some(end), &mut settings).unwrap();
                assert_eq!(PracticeStart::from_settings(&settings).unwrap(), start);
                assert_eq!(PracticeStart::section_end(&settings).unwrap(), Some(end));
                assert_eq!(unrelated(&settings), original);
            }
            PracticeStart::from_nanoseconds(7)
                .unwrap()
                .apply_section(None, &mut settings)
                .unwrap();
            assert_eq!(PracticeStart::section_end(&settings).unwrap(), None);
            assert_eq!(
                PracticeStart::from_settings(&settings)
                    .unwrap()
                    .nanoseconds(),
                7
            );
            assert_eq!(unrelated(&settings), original);
        }
    }
    #[test]
    fn section_replacement_reuses_old_endpoint_bytes_at_full_draft_capacity() {
        let mut settings = NativeSettings::from_args(
            &["--end-ns".into(), i64::MAX.to_string()],
            SettingsHost::Linux,
        )
        .unwrap();
        for i in 0..16 {
            let row = settings.add_binding().unwrap();
            settings
                .set_value(
                    row,
                    &"x".repeat(MAX_VALUE_BYTES - if i == 15 { 19 } else { 0 }),
                )
                .unwrap();
        }
        assert_eq!(
            settings
                .fields()
                .iter()
                .map(|field| field.value.len())
                .sum::<usize>(),
            crate::settings::MAX_TOTAL_BYTES
        );
        PracticeStart::from_nanoseconds(i64::MAX)
            .unwrap()
            .apply_section(None, &mut settings)
            .unwrap();
        assert_eq!(PracticeStart::section_end(&settings).unwrap(), None);
        assert_eq!(
            PracticeStart::from_settings(&settings)
                .unwrap()
                .nanoseconds(),
            i64::MAX
        );
        assert_eq!(
            settings
                .fields()
                .iter()
                .map(|field| field.value.len())
                .sum::<usize>(),
            crate::settings::MAX_TOTAL_BYTES
        );
    }
    #[test]
    fn section_rejection_and_second_field_byte_failure_are_atomic() {
        let mut settings = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
        let start = PracticeStart::from_nanoseconds(10).unwrap();
        let before = settings.native_args();
        for end in [0, 9, 10] {
            assert!(
                start
                    .apply_section(
                        Some(PracticeStart::from_nanoseconds(end).unwrap()),
                        &mut settings
                    )
                    .is_err()
            );
            assert_eq!(settings.native_args(), before);
        }
        let end_index = settings
            .fields()
            .iter()
            .position(|field| field.flag == "--end-ns")
            .unwrap();
        for value in ["-1", "+1", "1.0", "9223372036854775808"] {
            settings.set_value(end_index, value).unwrap();
            assert!(PracticeStart::section_end(&settings).is_err());
        }
        settings.set_value(end_index, "").unwrap();
        for index in 0..16 {
            let row = settings.add_binding().unwrap();
            settings
                .set_value(row, &"x".repeat(MAX_VALUE_BYTES - usize::from(index == 15)))
                .unwrap();
        }
        let before = settings.native_args();
        assert!(
            PracticeStart::from_nanoseconds(1)
                .unwrap()
                .apply_section(
                    Some(PracticeStart::from_nanoseconds(2).unwrap()),
                    &mut settings
                )
                .is_err()
        );
        assert_eq!(settings.native_args(), before); // First field would fit; second does not.
    }

    #[test]
    fn observed_integer_positions_remain_exact_through_display_and_settings() {
        let mut settings = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
        for nanos in [0, 1, 72_000_000_000_001, 604_800_000_000_001, i64::MAX] {
            let start = PracticeStart::from_nanoseconds(nanos).unwrap();
            assert_eq!(start.nanoseconds(), nanos);
            assert_eq!(PracticeStart::parse(&start.formatted()).unwrap(), start);
            start.apply_to(&mut settings).unwrap();
            assert_eq!(PracticeStart::from_settings(&settings).unwrap(), start);
        }
        assert!(PracticeStart::from_nanoseconds(-1).is_err());
        assert!(PracticeStart::from_nanoseconds(i64::MIN).is_err());
    }
    #[test]
    fn formats_precision_long_positions_and_maximum_roundtrip_exactly() {
        for (text, nanos) in [
            ("", 0),
            ("0.000000001", 1),
            ("1.123456789", 1_123_456_789),
            ("123:45.12", 7_425_120_000_000),
            ("20:00:00", 72_000_000_000_000),
            ("168:00:00", 604_800_000_000_000),
            ("9223372036.854775807", i64::MAX),
        ] {
            let parsed = PracticeStart::parse(text).unwrap();
            assert_eq!(parsed.nanoseconds(), nanos);
            assert_eq!(PracticeStart::parse(&parsed.formatted()).unwrap(), parsed);
        }
        assert_eq!(
            PracticeStart::parse("001.120000000").unwrap().formatted(),
            "0:01.12"
        );
        assert_eq!(PracticeStart::parse("3599").unwrap().formatted(), "59:59");
        assert_eq!(PracticeStart::parse("3600").unwrap().formatted(), "1:00:00");
    }
    #[test]
    fn malformed_fraction_components_and_arithmetic_overflow_reject_without_rounding() {
        for text in [
            "-1",
            "+1",
            " 1",
            "1 ",
            "1e3",
            "NaN",
            ".1",
            "1.",
            "1.0000000001",
            "1.2.3",
            ":01",
            "1:",
            "1::2",
            "1:60",
            "1:60:00",
            "1:00:60",
            "1:2:3:4",
            "١",
            "9223372036.854775808",
            "18446744073709551615",
            "18446744073709551615:59",
            "18446744073709551615:59:59",
        ] {
            assert!(PracticeStart::parse(text).is_err(), "{text}");
        }
        assert!(PracticeStart::parse(&"0".repeat(65)).is_err());
        assert_eq!(
            PracticeStart::parse(&"0".repeat(64)).unwrap().nanoseconds(),
            0
        );
    }
    #[test]
    fn settings_preserve_repeated_values_and_failed_write_is_atomic() {
        let args = [
            "--bind",
            "11:04",
            "--bind",
            "12:05",
            "--ghost-self",
            "a.bkr",
            "--ghost-other",
            "b.bkr",
            "--alsa",
            "hw:1",
            "--start-ns",
            "123",
        ];
        let mut settings = NativeSettings::from_args(
            &args.iter().map(|text| (*text).into()).collect::<Vec<_>>(),
            SettingsHost::Linux,
        )
        .unwrap();
        assert_eq!(
            PracticeStart::from_settings(&settings)
                .unwrap()
                .nanoseconds(),
            123
        );
        let other = |settings: &NativeSettings| {
            settings
                .fields()
                .iter()
                .filter(|field| field.flag != "--start-ns")
                .map(|field| (field.flag, field.value.clone()))
                .collect::<Vec<_>>()
        };
        let before = other(&settings);
        PracticeStart::parse("20:00:00.000000001")
            .unwrap()
            .apply_to(&mut settings)
            .unwrap();
        assert_eq!(other(&settings), before);
        assert_eq!(
            PracticeStart::from_settings(&settings)
                .unwrap()
                .nanoseconds(),
            72_000_000_000_001
        );
        let start_index = settings
            .fields()
            .iter()
            .position(|field| field.flag == "--start-ns")
            .unwrap();
        for invalid in ["-1", "+1", "1.0", "9223372036854775808"] {
            settings.set_value(start_index, invalid).unwrap();
            assert!(PracticeStart::from_settings(&settings).is_err());
        }
        let mut capped = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
        for _ in 0..16 {
            let index = capped.add_binding().unwrap();
            capped
                .set_value(index, &"x".repeat(MAX_VALUE_BYTES))
                .unwrap();
        }
        let before = capped.native_args();
        assert!(
            PracticeStart::parse("1")
                .unwrap()
                .apply_to(&mut capped)
                .is_err()
        );
        assert_eq!(capped.native_args(), before);
    }
}
