//! Bounded drafts of existing native options, with no device or file ownership.
use std::collections::BTreeSet;

pub const MAX_FIELDS: usize = 128;
pub const MAX_VALUE_BYTES: usize = 4096;
pub const MAX_TOTAL_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsHost {
    Windows,
    Linux,
    Macos,
}

/// Explicit override flags replace the entire corresponding profile group.
/// Values remain individual tokens; chart selection is not persisted here.
pub fn overlay_native_args(
    base: &[String],
    overrides: &[String],
    host: SettingsHost,
) -> Result<Vec<String>, String> {
    if base.len() % 2 != 0 || overrides.len() % 2 != 0 {
        return Err("settings overrides require flag/value pairs".into());
    }
    // Validate each source before merging, including unknown and duplicate flags.
    NativeSettings::from_args(base, host)?;
    NativeSettings::from_args(overrides, host)?;
    let replaced: BTreeSet<_> = overrides
        .chunks_exact(2)
        .map(|pair| pair[0].as_str())
        .collect();
    let merged: Vec<_> = base
        .chunks_exact(2)
        .filter(|pair| !replaced.contains(pair[0].as_str()))
        .chain(overrides.chunks_exact(2))
        .flat_map(|pair| pair.iter().cloned())
        .collect();
    Ok(NativeSettings::from_args(&merged, host)?.native_args())
}

#[derive(Clone, Debug)]
pub struct SettingsField {
    pub flag: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub value: String,
}
type Spec = (&'static str, &'static str, &'static str);

const COMMON: &[Spec] = &[
    (
        "--bind",
        "KEY BINDING",
        "Lane hex:HID usage hex, e.g. 11:04. Add one row per used lane.",
    ),
    (
        "--early-ns",
        "EARLY WINDOW (NS)",
        "Nonnegative nanoseconds; empty uses the native default.",
    ),
    (
        "--late-ns",
        "LATE WINDOW (NS)",
        "Nonnegative nanoseconds; empty uses the native default.",
    ),
    (
        "--input-offset-ns",
        "INPUT OFFSET (NS)",
        "Signed nanoseconds, applied once to judging.",
    ),
    (
        "--preroll-ns",
        "PREROLL (NS)",
        "0..10000000000 nanoseconds before song zero.",
    ),
    (
        "--bgm-lookahead-ns",
        "BGM LOOKAHEAD (NS)",
        "Positive nanoseconds of queued background audio.",
    ),
    (
        "--voices",
        "MAX AUDIO VOICES",
        "1..4096 simultaneous voices; empty uses the native default.",
    ),
    (
        "--channel-policy",
        "CHANNEL TREATMENT",
        "exact or mono-stereo; no inferred speaker layout.",
    ),
    (
        "--seconds",
        "SESSION CUTOFF (S)",
        "Empty plays the full song. Optional diagnostic limit: 1..3600 seconds.",
    ),
    (
        "--record-replay",
        "REPLAY SAVE PATH",
        "Optional new file path; existing files are not overwritten.",
    ),
    (
        "--replay-max-records",
        "REPLAY RECORD LIMIT",
        "Positive maximum recorded operations.",
    ),
    (
        "--replay-max-bytes",
        "REPLAY BYTE LIMIT",
        "Positive maximum encoded recording bytes.",
    ),
    (
        "--ghost-self",
        "MY SAVED OPPONENT",
        "Optional replay path for competition against your own record.",
    ),
    (
        "--ghost-other",
        "OTHER SAVED OPPONENT",
        "Optional replay path for competition against another record.",
    ),
    (
        "--mp-host",
        "HOST ADDRESS",
        "Optional numeric IP:port; choose host or join.",
    ),
    (
        "--mp-join",
        "JOIN ADDRESS",
        "Optional numeric IP:port; choose host or join.",
    ),
    (
        "--mp-timeout-ms",
        "NETWORK TIMEOUT (MS)",
        "100..120000 milliseconds; requires host or join.",
    ),
];
const WINDOWS: &[Spec] = &[
    (
        "--keyboard-path",
        "KEYBOARD INTERFACE PATH",
        "Optional exact Raw Input keyboard path; empty accepts any keyboard.",
    ),
    (
        "--device",
        "AUDIO DEVICE ID",
        "Empty uses the OS default WASAPI output. Advanced override: endpoint ID or ASIO CLSID.",
    ),
    (
        "--backend",
        "AUDIO BACKEND",
        "wasapi or asio; ASIO requires the optional SDK build.",
    ),
    (
        "--mode",
        "WASAPI MODE",
        "shared or exclusive; required for WASAPI, omit for ASIO.",
    ),
    (
        "--buffer",
        "AUDIO BUFFER",
        "default, frames:N or ns:N. ASIO accepts default or frames:N.",
    ),
    (
        "--period",
        "WASAPI PERIOD",
        "default, frames:N or ns:N; omit for ASIO.",
    ),
    (
        "--shared-policy",
        "WASAPI SHARED POLICY",
        "engine or legacy; omit for ASIO.",
    ),
    (
        "--asio-view",
        "ASIO REGISTRY VIEW",
        "native, 32 or 64; required for ASIO.",
    ),
    (
        "--output-channels",
        "ASIO OUTPUT CHANNELS",
        "Distinct zero-based channel indices, e.g. 0,1.",
    ),
    (
        "--asio-system-clock",
        "ASIO SYSTEM CLOCK",
        "multimedia; explicit caller assessment required.",
    ),
    (
        "--asio-timer-error-ns",
        "ASIO TIMER ERROR (NS)",
        "Nonnegative caller estimate; required for ASIO.",
    ),
    (
        "--asio-drift-error-ns",
        "ASIO DRIFT ERROR (NS)",
        "Nonnegative caller estimate; required for ASIO.",
    ),
    (
        "--asio-latency-error-ns",
        "ASIO LATENCY ERROR (NS)",
        "Nonnegative caller estimate; required for ASIO.",
    ),
    (
        "--asio-anchor-age-ns",
        "ASIO ANCHOR AGE (NS)",
        "Positive freshness limit in nanoseconds.",
    ),
];
const LINUX: &[Spec] = &[
    (
        "--evdev",
        "KEYBOARD DEVICE PATH",
        "Empty chooses a readable keyboard automatically. Advanced override: exact evdev node.",
    ),
    (
        "--alsa",
        "ALSA ENDPOINT",
        "Empty uses ALSA default. Advanced override: exact endpoint.",
    ),
    (
        "--rate",
        "SAMPLE RATE (HZ)",
        "Empty uses app/device defaults. Advanced override: positive output sample rate.",
    ),
    (
        "--channels",
        "OUTPUT CHANNEL COUNT",
        "Explicit output channel count, 1..32.",
    ),
    (
        "--period-frames",
        "ALSA PERIOD (FRAMES)",
        "Empty uses 256 frames. Advanced override: positive period below buffer.",
    ),
    (
        "--buffer-frames",
        "ALSA BUFFER (FRAMES)",
        "Empty uses 1024 frames. Advanced override: buffer greater than period.",
    ),
    (
        "--advance-lag-ns",
        "INPUT ADVANCE LAG (NS)",
        "0..1000000000 nanoseconds; preserves native acquisition ordering.",
    ),
];
const MACOS: &[Spec] = &[
    (
        "--device",
        "AUDIO DEVICE ID",
        "Empty uses the OS default output. Advanced override: CoreAudio device ID.",
    ),
    (
        "--keyboard-registry",
        "KEYBOARD REGISTRY ID",
        "Empty discovers a keyboard automatically. Advanced override: positive IORegistry ID.",
    ),
    (
        "--rate",
        "SAMPLE RATE (HZ)",
        "Empty uses app/device defaults. Advanced override: positive output sample rate.",
    ),
    (
        "--channels",
        "OUTPUT CHANNEL COUNT",
        "Explicit output channel count, 1..32.",
    ),
    (
        "--buffer-frames",
        "AUDIO BUFFER (FRAMES)",
        "Empty uses the current device buffer. Advanced override: 1..1048576 frames.",
    ),
    (
        "--advance-lag-ns",
        "INPUT ADVANCE LAG (NS)",
        "0..1000000000 nanoseconds; preserves native acquisition ordering.",
    ),
];

#[derive(Clone, Debug)]
pub struct NativeSettings {
    fields: Vec<SettingsField>,
}
impl NativeSettings {
    /// Retains configured pair order/repeated bindings and opponents. Missing
    /// known options become empty fields; no device, key or numeric default is guessed.
    pub fn from_args(args: &[String], host: SettingsHost) -> Result<Self, String> {
        if args.len() % 2 != 0 || args.len() / 2 > MAX_FIELDS {
            return Err("settings require at most 128 flag/value pairs".into());
        }
        let platform = match host {
            SettingsHost::Windows => WINDOWS,
            SettingsHost::Linux => LINUX,
            SettingsHost::Macos => MACOS,
        };
        let specs: Vec<_> = platform.iter().chain(COMMON).copied().collect();
        let mut fields = Vec::new();
        let mut seen = BTreeSet::new();
        for pair in args.chunks_exact(2) {
            let flag = pair[0].as_str();
            if !repeatable(flag) && !seen.insert(flag) {
                return Err(format!("duplicate native setting {flag}"));
            }
            if flag == "--chart" {
                continue;
            }
            let spec = specs
                .iter()
                .find(|spec| spec.0 == flag)
                .ok_or_else(|| format!("unknown native setting {flag}"))?;
            valid_value(&pair[1])?;
            fields.push(field(*spec, pair[1].clone()));
        }
        for spec in specs {
            if !fields.iter().any(|field| field.flag == spec.0) {
                fields.push(field(spec, String::new()));
            }
        }
        if fields.len() > MAX_FIELDS
            || fields.iter().map(|field| field.value.len()).sum::<usize>() > MAX_TOTAL_BYTES
        {
            return Err("native settings exceed field or total byte limits".into());
        }
        Ok(Self { fields })
    }
    pub fn fields(&self) -> &[SettingsField] {
        &self.fields
    }
    /// Rejection leaves the previous draft intact.
    pub fn set_value(&mut self, index: usize, value: &str) -> Result<(), String> {
        valid_value(value)?;
        let previous = self.fields.get(index).ok_or("unknown settings field")?;
        let total = self
            .fields
            .iter()
            .map(|field| field.value.len())
            .sum::<usize>()
            - previous.value.len()
            + value.len();
        if total > MAX_TOTAL_BYTES {
            return Err("native settings exceed 64 KiB of values".into());
        }
        self.fields[index].value = value.to_owned();
        Ok(())
    }
    pub fn add_binding(&mut self) -> Result<usize, String> {
        if self.fields.len() == MAX_FIELDS {
            return Err("native settings exceed 128 fields".into());
        }
        let index = self.fields.len();
        self.fields.push(field(COMMON[0], String::new()));
        Ok(index)
    }
    /// Empty fields omit the option. Selection supplies --chart separately;
    /// syntax/cross-option validation must use the actual native parser before Apply.
    pub fn native_args(&self) -> Vec<String> {
        self.fields
            .iter()
            .filter(|field| !field.value.is_empty())
            .flat_map(|field| [field.flag.to_owned(), field.value.clone()])
            .collect()
    }
}
fn field(spec: Spec, value: String) -> SettingsField {
    SettingsField {
        flag: spec.0,
        label: spec.1,
        hint: spec.2,
        value,
    }
}
fn repeatable(flag: &str) -> bool {
    matches!(flag, "--bind" | "--ghost-self" | "--ghost-other")
}
fn valid_value(value: &str) -> Result<(), String> {
    if value.len() > MAX_VALUE_BYTES
        || value
            .chars()
            .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
    {
        return Err("settings values allow at most 4096 UTF-8 bytes without controls".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }
    #[test]
    fn cli_overrides_replace_whole_repeat_groups_and_can_clear_optional_values() {
        let base = args(&[
            "--bind",
            "11:04",
            "--bind",
            "12:05",
            "--alsa",
            "hw:0",
            "--seconds",
            "10",
        ]);
        let overrides = args(&[
            "--bind",
            "11:06",
            "--alsa",
            "device with spaces",
            "--seconds",
            "",
        ]);
        assert_eq!(
            overlay_native_args(&base, &overrides, SettingsHost::Linux).unwrap(),
            args(&["--bind", "11:06", "--alsa", "device with spaces"])
        );
        assert!(
            overlay_native_args(&base, &args(&["--unsupported", "x"]), SettingsHost::Linux)
                .is_err()
        );
    }
    #[test]
    fn exact_values_and_repeat_order_survive_without_chart_or_defaults() {
        let supplied = args(&[
            "--chart",
            "곡.bms",
            "--bind",
            "11:04",
            "--ghost-other",
            "path with spaces.bkr",
            "--bind",
            "12:05",
            "--ghost-self",
            "내 기록.bkr",
        ]);
        let mut settings = NativeSettings::from_args(&supplied, SettingsHost::Linux).unwrap();
        assert_eq!(settings.native_args(), supplied[2..]);
        let row = settings.add_binding().unwrap();
        settings.set_value(row, "16:2C").unwrap();
        let output = settings.native_args();
        assert_eq!(&output[output.len() - 2..], ["--bind", "16:2C"]);
        settings.set_value(row, "").unwrap();
        assert_eq!(settings.native_args(), supplied[2..]);
    }
    #[test]
    fn host_options_and_failed_edits_preserve_draft_limits() {
        assert!(
            NativeSettings::from_args(&args(&["--device", "17"]), SettingsHost::Linux).is_err()
        );
        assert!(
            NativeSettings::from_args(
                &args(&["--device", "17", "--device", "18"]),
                SettingsHost::Macos
            )
            .is_err()
        );
        let mut settings = NativeSettings::from_args(&[], SettingsHost::Windows).unwrap();
        settings.set_value(0, "device with spaces").unwrap();
        let before = settings.native_args();
        assert!(settings.set_value(0, "bad\nvalue").is_err());
        assert!(settings.set_value(0, "bad\u{2028}value").is_err());
        assert!(
            settings
                .set_value(0, &"x".repeat(MAX_VALUE_BYTES + 1))
                .is_err()
        );
        assert_eq!(settings.native_args(), before);
        while settings.fields().len() < MAX_FIELDS {
            settings.add_binding().unwrap();
        }
        assert!(settings.add_binding().is_err());
    }
    #[test]
    fn aggregate_budget_rejects_edit_without_losing_previous_values() {
        let mut settings = NativeSettings::from_args(&[], SettingsHost::Windows).unwrap();
        let value = "x".repeat(MAX_VALUE_BYTES);
        for index in 0..MAX_TOTAL_BYTES / MAX_VALUE_BYTES {
            settings.set_value(index, &value).unwrap();
        }
        let next = MAX_TOTAL_BYTES / MAX_VALUE_BYTES;
        assert!(settings.set_value(next, "x").is_err());
        assert_eq!(settings.fields()[next].value, "");
        assert_eq!(
            settings
                .fields()
                .iter()
                .map(|field| field.value.len())
                .sum::<usize>(),
            MAX_TOTAL_BYTES
        );
    }
}
