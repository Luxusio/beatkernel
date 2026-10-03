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
    let input_family = |flag: &str| match host {
        SettingsHost::Linux => matches!(flag, "--evdev" | "--local-input" | "--local-player"),
        SettingsHost::Windows => matches!(flag, "--keyboard-path" | "--local-player"),
        SettingsHost::Macos => matches!(flag, "--keyboard-registry" | "--local-player"),
    };
    let replaces_input = overrides.chunks_exact(2).any(|pair| input_family(&pair[0]));
    let network_family = |flag: &str| {
        matches!(
            flag,
            "--mp-host"
                | "--mp-join"
                | "--mp-webtransport"
                | "--mp-role"
                | "--mp-origin"
                | "--mp-cert"
                | "--mp-key"
                | "--mp-ca"
                | "--mp-server-name"
        )
    };
    let replaces_network = overrides.chunks_exact(2).any(|pair| {
        matches!(
            pair[0].as_str(),
            "--mp-host" | "--mp-join" | "--mp-webtransport"
        )
    });
    let merged: Vec<_> = base
        .chunks_exact(2)
        .filter(|pair| {
            !replaced.contains(pair[0].as_str())
                && !(replaces_input && input_family(&pair[0]))
                && !(replaces_network && network_family(&pair[0]))
        })
        .chain(overrides.chunks_exact(2))
        .flat_map(|pair| pair.iter().cloned())
        .collect();
    Ok(NativeSettings::from_args(&merged, host)?.native_args())
}

/// Parses an explicit native chart seed without signs or implicit truncation.
pub fn parse_chart_seed(value: &str) -> Result<u64, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("chart seed must be unsigned decimal u64".into());
    }
    value.parse().map_err(|_| "chart seed exceeds u64".into())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsField {
    pub flag: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub value: String,
}
type Spec = (&'static str, &'static str, &'static str);

const COMMON: &[Spec] = &[
    (
        "--chart-seed",
        "CHART BRANCH SEED",
        "Empty uses 0. Unsigned decimal u64; retries and recordings preserve the same BMS RANDOM branches.",
    ),
    (
        "--start-ns",
        "PRACTICE START (NS)",
        "Empty starts the full song. Nonnegative original song position; earlier note heads are excluded.",
    ),
    (
        "--end-ns",
        "PRACTICE END (NS)",
        "Empty plays without an endpoint. Unsigned original song position after start; native presentation and input drain finish this prefix.",
    ),
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
        "--mp-webtransport",
        "WEBTRANSPORT ROOM URL",
        "Optional HTTPS /rooms/KEY relay URL; choose this or raw QUIC host/join. Requires the webtransport build feature.",
    ),
    (
        "--mp-role",
        "WEBTRANSPORT START ROLE",
        "host proposes the start; join accepts it. Both connect to the relay as clients.",
    ),
    (
        "--mp-origin",
        "WEBTRANSPORT ORIGIN",
        "Exact canonical HTTPS or HTTP loopback origin allowed by the relay.",
    ),
    (
        "--mp-timeout-ms",
        "NETWORK TIMEOUT (MS)",
        "100..120000 milliseconds; requires a multiplayer transport.",
    ),
    (
        "--mp-cert",
        "QUIC HOST CERTIFICATE",
        "Host certificate chain path (PEM or DER); requires --mp-key.",
    ),
    (
        "--mp-key",
        "QUIC HOST PRIVATE KEY",
        "Host private key path (PEM or DER); requires --mp-cert.",
    ),
    (
        "--mp-ca",
        "NETWORK TRUST ANCHOR",
        "Explicit trust anchor path (PEM or DER); raw QUIC join also requires --mp-server-name.",
    ),
    (
        "--mp-server-name",
        "QUIC SERVER NAME",
        "Certificate server identity for join; requires --mp-ca.",
    ),
];
const WINDOWS: &[Spec] = &[
    (
        "--local-player",
        "LOCAL PLAYER ID:KEYBOARD PATH",
        "Repeat stable positive ID:exact interface path for 2..64 players; omit keyboard-path.",
    ),
    (
        "--advance-lag-ns",
        "LOCAL ADVANCE LAG (NS)",
        "0..1000000000ns; default 2000000ns, shared local input frontier margin.",
    ),
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
        "--local-player",
        "LOCAL PLAYER ID:KEYBOARD PATH",
        "Repeat stable positive player ID:path for 2..64 local players; exclusive with --local-input/--evdev.",
    ),
    (
        "--local-input",
        "LOCAL PLAYER KEYBOARD PATH",
        "Repeat for 2..64 local players in order; mutually exclusive with --evdev.",
    ),
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
        "--local-player",
        "LOCAL PLAYER REGISTRY",
        "Repeat ID:REGISTRY for2..64 distinct keyboards. Solo remains automatic.",
    ),
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
    /// Empty draft means the native default; incomplete text fails at apply/launch.
    pub fn chart_seed(&self) -> Result<u64, String> {
        let value = self
            .fields
            .iter()
            .find(|row| row.flag == "--chart-seed")
            .map_or("", |row| row.value.as_str());
        if value.is_empty() {
            Ok(0)
        } else {
            parse_chart_seed(value)
        }
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
    /// Adds a saved opponent to this draft, preserving all other options.
    /// Reuses an empty row of the same kind; all rejection is atomic.
    pub fn add_opponent(
        &mut self,
        kind: crate::competition::OpponentKind,
        path: &str,
    ) -> Result<(), String> {
        valid_value(path)?;
        if path.is_empty() {
            return Err("replay opponent requires a nonempty path".into());
        }
        let opponents = self
            .fields
            .iter()
            .filter(|row| {
                matches!(row.flag, "--ghost-self" | "--ghost-other") && !row.value.is_empty()
            })
            .count();
        if opponents >= 8 {
            return Err("at most eight replay opponents are supported".into());
        }
        let flag = opponent_flag(kind);
        if let Some(index) = self
            .fields
            .iter()
            .position(|row| row.flag == flag && row.value.is_empty())
        {
            return self.set_value(index, path);
        }
        if self.fields.len() >= MAX_FIELDS {
            return Err("native settings exceed 128 fields".into());
        }
        if self.fields.iter().map(|row| row.value.len()).sum::<usize>() + path.len()
            > MAX_TOTAL_BYTES
        {
            return Err("native settings exceed 64 KiB of values".into());
        }
        let spec = *COMMON
            .iter()
            .find(|spec| spec.0 == flag)
            .expect("known opponent flag");
        self.fields
            .try_reserve(1)
            .map_err(|_| "opponent row allocation failed")?;
        self.fields.push(field(spec, path.to_owned()));
        Ok(())
    }
    /// Counts exact kind/path occurrences without resolving filesystem identity.
    /// Empty queries never count unused editable rows.
    pub fn opponent_count(&self, kind: crate::competition::OpponentKind, path: &str) -> usize {
        if path.is_empty() {
            return 0;
        }
        let flag = opponent_flag(kind);
        self.fields
            .iter()
            .filter(|row| row.flag == flag && row.value == path)
            .count()
    }
    /// Clears the first exact kind/path occurrence, preserving its editable row
    /// and every other value. Missing paths return false; rejection is atomic.
    pub fn remove_opponent(
        &mut self,
        kind: crate::competition::OpponentKind,
        path: &str,
    ) -> Result<bool, String> {
        valid_value(path)?;
        if path.is_empty() {
            return Err("replay opponent requires a nonempty path".into());
        }
        let flag = opponent_flag(kind);
        if let Some(row) = self
            .fields
            .iter_mut()
            .find(|row| row.flag == flag && row.value == path)
        {
            row.value.clear();
            return Ok(true);
        }
        Ok(false)
    }
    /// Clears saved opponents while retaining editable rows and unrelated options.
    pub fn clear_opponents(&mut self) {
        for row in &mut self.fields {
            if matches!(row.flag, "--ghost-self" | "--ghost-other") {
                row.value.clear();
            }
        }
    }
    pub fn add_binding(&mut self) -> Result<usize, String> {
        if self.fields.len() == MAX_FIELDS {
            return Err("native settings exceed 128 fields".into());
        }
        let index = self.fields.len();
        let spec = *COMMON
            .iter()
            .find(|spec| spec.0 == "--bind")
            .expect("binding schema");
        self.fields.push(field(spec, String::new()));
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
fn opponent_flag(kind: crate::competition::OpponentKind) -> &'static str {
    match kind {
        crate::competition::OpponentKind::Own => "--ghost-self",
        crate::competition::OpponentKind::Other => "--ghost-other",
    }
}
fn repeatable(flag: &str) -> bool {
    matches!(
        flag,
        "--bind" | "--ghost-self" | "--ghost-other" | "--local-input" | "--local-player"
    )
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
    #[test]
    fn windows_input_mode_overrides_replace_the_assignment_family() {
        let group = args(&[
            "--device",
            "output",
            "--local-player",
            "3:path:A",
            "--local-player",
            "9:path:B",
        ]);
        let solo = overlay_native_args(
            &group,
            &args(&["--keyboard-path", "path:C"]),
            SettingsHost::Windows,
        )
        .unwrap();
        assert!(!solo.iter().any(|arg| arg == "--local-player"));
        assert!(
            solo.chunks_exact(2)
                .any(|pair| pair == ["--device", "output"])
        );
        let restored = overlay_native_args(&solo, &group[2..], SettingsHost::Windows).unwrap();
        assert!(!restored.iter().any(|arg| arg == "--keyboard-path"));
        assert_eq!(restored, group);
        assert!(
            NativeSettings::from_args(&args(&["--local-input", "path:A"]), SettingsHost::Windows)
                .is_err()
        );
    }
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }
    #[test]
    fn input_mode_overrides_replace_the_entire_linux_assignment_family() {
        let raw = args(&[
            "--alsa",
            "default",
            "--local-input",
            "/dev/input/event1",
            "--local-input",
            "/dev/input/event2",
        ]);
        let stable = args(&[
            "--local-player",
            "7:/dev/input/event3",
            "--local-player",
            "99:/dev/input/event4",
        ]);
        let replaced = overlay_native_args(&raw, &stable, SettingsHost::Linux).unwrap();
        assert!(!replaced.iter().any(|arg| arg == "--local-input"));
        assert!(replaced.chunks_exact(2).any(|p| p == ["--alsa", "default"]));
        let solo = overlay_native_args(
            &replaced,
            &args(&["--evdev", "/dev/input/event5"]),
            SettingsHost::Linux,
        )
        .unwrap();
        assert!(!solo.iter().any(|arg| arg == "--local-player"));
        let again = overlay_native_args(&solo, &stable, SettingsHost::Linux).unwrap();
        assert!(!again.iter().any(|arg| arg == "--evdev"));
        assert_eq!(again, replaced);
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
    fn end_profile_overlay_and_binding_rows_preserve_exact_endpoints_on_every_host() {
        use crate::{
            presentation_settings::PresentationSettings,
            settings_profile::{
                PlayerProfile, decode_player_profile, decode_profile, encode_player_profile,
                encode_profile,
            },
        };
        for host in [
            SettingsHost::Linux,
            SettingsHost::Windows,
            SettingsHost::Macos,
        ] {
            let supplied = args(&[
                "--start-ns",
                "604800000000001",
                "--end-ns",
                "9223372036854775807",
                "--bind",
                "11:04",
            ]);
            let mut model = NativeSettings::from_args(&supplied, host).unwrap();
            let row = model.add_binding().unwrap();
            assert_eq!(model.fields()[row].flag, "--bind");
            model.set_value(row, "12:05").unwrap();
            let mut expected = supplied.clone();
            expected.extend(args(&["--bind", "12:05"]));
            assert_eq!(model.native_args(), expected);
            let encoded = encode_profile(&model, host).unwrap();
            let restored = decode_profile(&encoded, host).unwrap();
            assert_eq!(restored.native_args(), expected);
            let combined = encode_player_profile(
                &PlayerProfile {
                    native: restored.clone(),
                    presentation: PresentationSettings::default(),
                },
                host,
            )
            .unwrap();
            let restored = decode_player_profile(&combined, host).unwrap().native;
            assert_eq!(restored.native_args(), expected);
            let overlaid = overlay_native_args(
                &restored.native_args(),
                &args(&["--end-ns", "9223372036854775806"]),
                host,
            )
            .unwrap();
            assert_eq!(
                overlaid
                    .chunks_exact(2)
                    .filter(|pair| pair[0] == "--end-ns")
                    .count(),
                1
            );
            assert_eq!(
                overlaid
                    .chunks_exact(2)
                    .find(|pair| pair[0] == "--end-ns")
                    .unwrap()[1],
                "9223372036854775806"
            );
            let unlimited = overlay_native_args(&overlaid, &args(&["--end-ns", ""]), host).unwrap();
            assert!(!unlimited.chunks_exact(2).any(|pair| pair[0] == "--end-ns"));
            assert_eq!(
                unlimited
                    .chunks_exact(2)
                    .filter(|pair| pair[0] == "--bind")
                    .count(),
                2
            );
            let blank = NativeSettings::from_args(&[], host).unwrap();
            assert!(blank.native_args().is_empty());
            assert_eq!(
                blank
                    .fields()
                    .iter()
                    .find(|row| row.flag == "--end-ns")
                    .unwrap()
                    .value,
                ""
            );
            // Editable draft values do not duplicate native parser authority.
            assert!(NativeSettings::from_args(&args(&["--end-ns", "invalid draft"]), host).is_ok());
        }
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
    #[test]
    fn macos_registry_and_group_overrides_replace_the_input_family() {
        let solo = args(&["--keyboard-registry", "900", "--device", "4"]);
        let group = args(&["--local-player", "1:100", "--local-player", "9:200"]);
        let merged = overlay_native_args(&solo, &group, SettingsHost::Macos).unwrap();
        assert!(!merged.iter().any(|arg| arg == "--keyboard-registry"));
        let restored = overlay_native_args(
            &merged,
            &args(&["--keyboard-registry", "300"]),
            SettingsHost::Macos,
        )
        .unwrap();
        assert!(!restored.iter().any(|arg| arg == "--local-player"));
        assert_eq!(
            restored,
            args(&["--device", "4", "--keyboard-registry", "300"])
        );
    }
}

#[cfg(test)]
mod opponent_fixtures {
    use super::*;
    use crate::competition::OpponentKind;
    #[test]
    fn exact_removal_clears_only_first_kind_path_row_and_preserves_schema_order() {
        let path = "C:records:一.bkr";
        let args = [
            "--ghost-self",
            path,
            "--bind",
            "30:1",
            "--ghost-other",
            path,
            "--ghost-self",
            "other.bkr",
            "--ghost-self",
            path,
            "--start-ns",
            "123",
            "--end-ns",
            "456",
        ]
        .map(String::from);
        let mut draft = NativeSettings::from_args(&args, SettingsHost::Windows).unwrap();
        assert_eq!(draft.opponent_count(OpponentKind::Own, path), 2);
        assert_eq!(draft.opponent_count(OpponentKind::Other, path), 1);
        assert_eq!(draft.opponent_count(OpponentKind::Own, ""), 0);
        assert_eq!(
            draft.opponent_count(OpponentKind::Own, "./C:records:一.bkr"),
            0
        );
        let mut expected = draft.fields.clone();
        expected[0].value.clear();
        assert_eq!(draft.remove_opponent(OpponentKind::Own, path), Ok(true));
        assert_eq!(draft.fields, expected);
        assert_eq!(draft.opponent_count(OpponentKind::Own, path), 1);
        assert_eq!(draft.opponent_count(OpponentKind::Other, path), 1);
        expected[4].value.clear();
        assert_eq!(draft.remove_opponent(OpponentKind::Own, path), Ok(true));
        assert_eq!(draft.fields, expected);
        assert_eq!(draft.remove_opponent(OpponentKind::Own, path), Ok(false));
        assert_eq!(draft.fields, expected);
        expected[2].value.clear();
        assert_eq!(draft.remove_opponent(OpponentKind::Other, path), Ok(true));
        assert_eq!(draft.fields, expected);
        assert_eq!(draft.opponent_count(OpponentKind::Own, "other.bkr"), 1);
    }
    #[test]
    fn invalid_and_missing_removal_preserve_even_full_draft_and_free_row_reuses_capacity() {
        let mut draft = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
        for i in 0..8 {
            draft
                .add_opponent(OpponentKind::Own, &format!("record{i}.bkr"))
                .unwrap();
        }
        while draft.fields.len() < MAX_FIELDS {
            draft.add_binding().unwrap();
        }
        let before = draft.fields.clone();
        for path in [
            String::new(),
            "bad\n.bkr".into(),
            "bad\0.bkr".into(),
            "bad\u{2028}.bkr".into(),
            "x".repeat(MAX_VALUE_BYTES + 1),
        ] {
            assert!(draft.remove_opponent(OpponentKind::Own, &path).is_err());
            assert_eq!(draft.fields, before);
        }
        assert_eq!(
            draft.remove_opponent(OpponentKind::Other, "record0.bkr"),
            Ok(false)
        );
        assert_eq!(
            draft.remove_opponent(OpponentKind::Own, &"x".repeat(MAX_VALUE_BYTES)),
            Ok(false)
        );
        assert_eq!(draft.fields, before);
        assert!(draft.add_opponent(OpponentKind::Own, "ninth.bkr").is_err());
        let cleared_index = draft
            .fields
            .iter()
            .position(|row| row.flag == "--ghost-self" && row.value == "record3.bkr")
            .unwrap();
        assert_eq!(
            draft.remove_opponent(OpponentKind::Own, "record3.bkr"),
            Ok(true)
        );
        draft
            .add_opponent(OpponentKind::Own, "replacement.bkr")
            .unwrap();
        let mut expected = before;
        expected[cleared_index].value = "replacement.bkr".into();
        assert_eq!(draft.fields, expected);
        assert_eq!(draft.fields.len(), MAX_FIELDS);
        assert_eq!(draft.opponent_count(OpponentKind::Own, "record3.bkr"), 0);
        assert_eq!(
            draft.opponent_count(OpponentKind::Own, "replacement.bkr"),
            1
        );
    }
    #[test]
    fn opponent_rows_fill_append_count_both_kinds_and_clear_without_changing_other_options() {
        let mut draft =
            NativeSettings::from_args(&["--alsa".into(), "hw:1".into()], SettingsHost::Linux)
                .unwrap();
        let original_fields = draft.fields.len();
        draft.add_opponent(OpponentKind::Own, "own:一.bkr").unwrap();
        draft
            .add_opponent(OpponentKind::Other, "other.bkr")
            .unwrap();
        assert_eq!(draft.fields.len(), original_fields);
        for index in 0..6 {
            draft
                .add_opponent(OpponentKind::Own, &format!("record{index}.bkr"))
                .unwrap();
        }
        let before = draft.native_args();
        assert!(
            draft
                .add_opponent(OpponentKind::Other, "ninth.bkr")
                .is_err()
        );
        assert_eq!(draft.native_args(), before);
        draft.clear_opponents();
        assert_eq!(
            draft.native_args(),
            vec!["--alsa".to_owned(), "hw:1".to_owned()]
        );
        let rows = draft.fields.len();
        draft.add_opponent(OpponentKind::Own, "again.bkr").unwrap();
        assert_eq!(draft.fields.len(), rows);
        for path in [
            String::new(),
            "bad\n.bkr".into(),
            "x".repeat(MAX_VALUE_BYTES + 1),
        ] {
            let before = draft.native_args();
            assert!(draft.add_opponent(OpponentKind::Other, &path).is_err());
            assert_eq!(draft.native_args(), before);
        }
    }
    #[test]
    fn empty_repeated_kind_rows_fill_first_and_field_total_caps_are_atomic() {
        let mut draft = NativeSettings::from_args(
            &[
                "--ghost-other".into(),
                String::new(),
                "--ghost-other".into(),
                String::new(),
            ],
            SettingsHost::Windows,
        )
        .unwrap();
        draft
            .add_opponent(OpponentKind::Other, "first.bkr")
            .unwrap();
        let ghosts: Vec<_> = draft
            .fields()
            .iter()
            .filter(|row| row.flag == "--ghost-other")
            .map(|row| row.value.as_str())
            .collect();
        assert_eq!(ghosts, vec!["first.bkr", ""]);
        // Build valid boundary drafts through the existing bounded model.
        let mut full = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
        full.add_opponent(OpponentKind::Own, "filled.bkr").unwrap();
        while full.fields.len() < MAX_FIELDS {
            full.add_binding().unwrap();
        }
        let before = full.native_args();
        assert!(full.add_opponent(OpponentKind::Own, "extra.bkr").is_err());
        assert_eq!(full.native_args(), before);
        let mut bytes = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
        for _ in 0..16 {
            let index = bytes.add_binding().unwrap();
            bytes
                .set_value(index, &"x".repeat(MAX_VALUE_BYTES))
                .unwrap();
        }
        let before = bytes.native_args();
        assert!(
            bytes
                .add_opponent(OpponentKind::Other, "overflow.bkr")
                .is_err()
        );
        assert_eq!(bytes.native_args(), before);
    }
}

#[cfg(test)]
mod chart_seed_fixtures {
    use super::*;
    #[test]
    fn full_u64_seed_default_draft_and_overlay_are_explicit() {
        for host in [
            SettingsHost::Windows,
            SettingsHost::Linux,
            SettingsHost::Macos,
        ] {
            let mut draft = NativeSettings::from_args(&[], host).unwrap();
            assert_eq!(draft.chart_seed(), Ok(0));
            let row = draft
                .fields()
                .iter()
                .position(|row| row.flag == "--chart-seed")
                .unwrap();
            for text in ["0", "3", "0003", "18446744073709551615"] {
                draft.set_value(row, text).unwrap();
                assert_eq!(draft.chart_seed().unwrap(), text.parse::<u64>().unwrap());
                assert!(
                    draft
                        .native_args()
                        .chunks_exact(2)
                        .any(|pair| pair == ["--chart-seed", text])
                );
            }
            draft.set_value(row, "incomplete").unwrap();
            assert!(draft.chart_seed().is_err()); // Editing stays reversible; apply validates.
            draft.set_value(row, "").unwrap();
            assert_eq!(draft.chart_seed(), Ok(0));
            let base = ["--chart-seed", "18446744073709551615"].map(String::from);
            let overrides = ["--chart-seed", "3"].map(String::from);
            let merged = overlay_native_args(&base, &overrides, host).unwrap();
            assert_eq!(
                NativeSettings::from_args(&merged, host)
                    .unwrap()
                    .chart_seed(),
                Ok(3)
            );
            assert_eq!(merged, overrides);
            assert!(
                NativeSettings::from_args(
                    &["--chart-seed", "0", "--chart-seed", "3"].map(String::from),
                    host
                )
                .is_err()
            );
        }
        for text in [
            "",
            "-1",
            "+1",
            " 1",
            "1 ",
            "1.0",
            "١",
            "18446744073709551616",
        ] {
            assert!(parse_chart_seed(text).is_err(), "{text}");
        }
    }
}
