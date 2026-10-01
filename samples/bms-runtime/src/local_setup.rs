//! Pure local roster drafts; metadata discovery and native attachment are external.
use crate::{
    device_catalog::{DeviceCatalog, DeviceRequest},
    local_players::{InputPlan, LocalPlayer, LocalPlayers, MAX_LOCAL_PLAYERS, PlayerId},
    settings::{NativeSettings, SettingsHost},
};

/// A bounded stable-ID roster used by graphical setup and settings profiles.
#[derive(Clone, Debug)]
pub struct LocalSetup {
    host: SettingsHost,
    roster: LocalPlayers,
}
impl LocalSetup {
    /// Imports legacy ordered paths or stable ID:path assignments, never both.
    /// Empty local fields represent automatic solo; no metadata is queried.
    pub fn from_settings(settings: &NativeSettings, host: SettingsHost) -> Result<Self, String> {
        validate_settings_host(settings, host)?;
        let raw: Vec<_> = settings
            .fields()
            .iter()
            .filter(|field| field.flag == "--local-input" && !field.value.is_empty())
            .map(|field| field.value.as_str())
            .collect();
        let stable: Vec<_> = settings
            .fields()
            .iter()
            .filter(|field| field.flag == "--local-player" && !field.value.is_empty())
            .map(|field| field.value.as_str())
            .collect();
        if !raw.is_empty() && !stable.is_empty() {
            return Err("local-input and local-player forms cannot be mixed".into());
        }
        let count = raw.len() + stable.len();
        if count == 0 {
            return Ok(Self {
                host,
                roster: LocalPlayers::new(host, MAX_LOCAL_PLAYERS)?,
            });
        }
        if host != SettingsHost::Linux {
            return Err("multiple native local players currently require Linux".into());
        }
        if !(2..=MAX_LOCAL_PLAYERS).contains(&count) {
            return Err("local assignments require 2..64 players".into());
        }
        if settings
            .fields()
            .iter()
            .any(|field| field.flag == "--evdev" && !field.value.is_empty())
        {
            return Err("local assignments cannot be mixed with a solo evdev override".into());
        }
        let assignments = if !raw.is_empty() {
            raw.into_iter()
                .enumerate()
                .map(|(index, path)| (PlayerId(index as u32 + 1), path.to_owned()))
                .collect()
        } else {
            stable
                .into_iter()
                .map(parse_assignment)
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Self {
            host,
            roster: LocalPlayers::from_assignments(host, MAX_LOCAL_PLAYERS, assignments)?,
        })
    }

    /// Current stable members, including unassigned members in a draft.
    pub fn players(&self) -> &[LocalPlayer] {
        self.roster.players()
    }

    /// Retains surviving IDs; newly grown members never reuse retired IDs.
    /// Unsupported native hosts reject group growth before changing the roster.
    pub fn resize(&mut self, count: usize) -> Result<(), String> {
        if count > 1 && self.host != SettingsHost::Linux {
            return Err("multiple native local players currently require Linux".into());
        }
        self.roster.resize(count)
    }

    /// Clears only the requested member's assignment, leaving its ID unchanged.
    pub fn clear(&mut self, player: PlayerId) -> Result<(), String> {
        self.roster.clear(player)
    }

    /// Assigns an exact selectable keyboard identity from this host's catalog.
    /// Audio/foreign catalogs, unavailable rows and duplicate assignments fail.
    pub fn assign(
        &mut self,
        player: PlayerId,
        catalog: &DeviceCatalog,
        index: usize,
    ) -> Result<(), String> {
        if catalog.request() != keyboard_request(self.host) {
            return Err("local assignment requires a matching host keyboard catalog".into());
        }
        let choice = catalog
            .choices()
            .get(index)
            .ok_or("keyboard row unavailable")?;
        if !choice.selectable {
            return Err("keyboard device is not selectable".into());
        }
        self.roster.assign(player, self.host, &choice.id)
    }

    /// Seals assignments and returns a new validated settings draft.
    /// Groups remove solo evdev and export stable ID:path pairs in roster order;
    /// solo removes local groups and preserves any advanced solo override.
    /// Unrelated options remain exact. Failure cannot change the supplied base.
    pub fn settings(&self, base: &NativeSettings) -> Result<NativeSettings, String> {
        validate_settings_host(base, self.host)?;
        let plan = self.roster.seal()?;
        let group = matches!(&plan, InputPlan::Assigned(_));
        let base_args = base.native_args();
        let mut args: Vec<_> = base_args
            .chunks_exact(2)
            .filter(|pair| {
                !matches!(pair[0].as_str(), "--local-input" | "--local-player")
                    && !(group && pair[0] == "--evdev")
            })
            .flat_map(|pair| pair.iter().cloned())
            .collect();
        if let InputPlan::Assigned(assignments) = plan {
            for (player, path) in assignments {
                args.push("--local-player".into());
                args.push(format!("{}:{path}", player.0));
            }
        }
        NativeSettings::from_args(&args, self.host)
    }
}

fn validate_settings_host(settings: &NativeSettings, host: SettingsHost) -> Result<(), String> {
    // NativeSettings retains empty schema fields; checking the keyboard schema
    // detects a foreign empty draft as well as foreign configured native flags.
    if DeviceRequest::keyboard(settings, host)? != keyboard_request(host) {
        return Err("local setup settings host mismatch".into());
    }
    NativeSettings::from_args(&settings.native_args(), host)?;
    Ok(())
}
fn keyboard_request(host: SettingsHost) -> DeviceRequest {
    match host {
        SettingsHost::Linux => DeviceRequest::LinuxKeyboard,
        SettingsHost::Windows => DeviceRequest::WindowsKeyboard,
        SettingsHost::Macos => DeviceRequest::MacosKeyboard,
    }
}
fn parse_assignment(value: &str) -> Result<(PlayerId, String), String> {
    let (id, path) = value
        .split_once(':')
        .ok_or("local-player requires ID:PATH")?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("local player ID must be a positive u32 decimal".into());
    }
    let id = id
        .parse::<u32>()
        .map_err(|_| "local player ID exceeds u32")?;
    if id == 0 || path.is_empty() {
        return Err("local-player requires positive ID and nonempty path".into());
    }
    Ok((PlayerId(id), path.into()))
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::device_catalog::DeviceChoice;
    fn settings(values: &[&str]) -> NativeSettings {
        NativeSettings::from_args(
            &values
                .iter()
                .map(|value| (*value).into())
                .collect::<Vec<_>>(),
            SettingsHost::Linux,
        )
        .unwrap()
    }
    fn catalog(request: DeviceRequest, devices: &[(&str, bool)]) -> DeviceCatalog {
        DeviceCatalog::new(
            request,
            devices
                .iter()
                .map(|(id, selectable)| DeviceChoice {
                    id: (*id).into(),
                    label: "keyboard".into(),
                    detail: String::new(),
                    selectable: *selectable,
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn four_and_sixty_four_members_preserve_stable_ids_and_import_export_exact_paths() {
        let base = settings(&[
            "--alsa",
            "hw:2,1",
            "--bind",
            "11:04",
            "--bind",
            "12:05",
            "--input-offset-ns",
            "-123",
        ]);
        let mut setup = LocalSetup::from_settings(&base, SettingsHost::Linux).unwrap();
        setup.resize(4).unwrap();
        let rows = catalog(
            DeviceRequest::LinuxKeyboard,
            &[
                ("/dev/input/event0", true),
                ("/dev/input/event1", true),
                ("/dev/input/event2", true),
                ("/dev/input/event3", true),
            ],
        );
        for index in 0..4 {
            setup
                .assign(setup.players()[index].id, &rows, index)
                .unwrap();
        }
        let accepted = setup.settings(&base).unwrap();
        let imported = LocalSetup::from_settings(&accepted, SettingsHost::Linux).unwrap();
        assert_eq!(imported.players(), setup.players());
        assert_eq!(
            accepted
                .fields()
                .iter()
                .filter(|field| field.flag == "--bind")
                .map(|field| field.value.as_str())
                .collect::<Vec<_>>(),
            vec!["11:04", "12:05"]
        );
        assert_eq!(
            accepted
                .fields()
                .iter()
                .find(|field| field.flag == "--alsa")
                .unwrap()
                .value,
            "hw:2,1"
        );
        let retired = setup.players()[3].id;
        setup.resize(2).unwrap();
        setup.resize(4).unwrap();
        assert!(setup.players()[2].id > retired);
        setup.resize(64).unwrap();
        let paths = (0..64)
            .map(|index| (format!("/dev/input/event{index}"), true))
            .collect::<Vec<_>>();
        let row_refs = paths
            .iter()
            .map(|(path, enabled)| (path.as_str(), *enabled))
            .collect::<Vec<_>>();
        let all = catalog(DeviceRequest::LinuxKeyboard, &row_refs);
        for index in 0..64 {
            setup
                .assign(setup.players()[index].id, &all, index)
                .unwrap();
        }
        assert_eq!(
            LocalSetup::from_settings(&setup.settings(&base).unwrap(), SettingsHost::Linux)
                .unwrap()
                .players(),
            setup.players()
        );
        let before = setup.players().to_vec();
        assert!(setup.resize(65).is_err());
        assert_eq!(setup.players(), before);
    }

    #[test]
    fn raw_and_profile_style_colon_paths_import_without_losing_identity() {
        let raw = settings(&[
            "--local-input",
            "/dev/input/event0",
            "--local-input",
            "/dev/input/event1",
        ]);
        let model = LocalSetup::from_settings(&raw, SettingsHost::Linux).unwrap();
        assert_eq!(
            model
                .players()
                .iter()
                .map(|player| player.id)
                .collect::<Vec<_>>(),
            vec![PlayerId(1), PlayerId(2)]
        );
        let stable = settings(&[
            "--local-player",
            "9:/input/path:with:colons",
            "--local-player",
            "3:/other/path",
        ]);
        let setup = LocalSetup::from_settings(&stable, SettingsHost::Linux).unwrap();
        assert_eq!(setup.players()[0].id, PlayerId(9));
        assert_eq!(setup.players()[0].input(), Some("/input/path:with:colons"));
        assert_eq!(
            setup.settings(&stable).unwrap().native_args(),
            stable.native_args()
        );
        let maximum = settings(&[
            "--local-player",
            "4294967295:/last",
            "--local-player",
            "7:/other",
        ]);
        let mut exhausted = LocalSetup::from_settings(&maximum, SettingsHost::Linux).unwrap();
        assert_eq!(exhausted.players()[0].id, PlayerId(u32::MAX));
        assert_eq!(
            exhausted.settings(&maximum).unwrap().native_args(),
            maximum.native_args()
        );
        let before = exhausted.players().to_vec();
        assert!(exhausted.resize(3).is_err());
        assert_eq!(exhausted.players(), before);
        exhausted.resize(1).unwrap();

        for value in [
            "0:/path",
            "-1:/path",
            "+1:/path",
            "1:",
            "4294967296:/path",
            "without-colon",
        ] {
            assert!(parse_assignment(value).is_err());
        }
        for values in [
            vec!["--local-input", "/a"],
            vec!["--local-player", "1:/a", "--local-player", "1:/b"],
            vec!["--local-player", "1:/a", "--local-player", "2:/a"],
            vec![
                "--local-input",
                "/a",
                "--local-input",
                "/b",
                "--local-player",
                "3:/c",
            ],
            vec![
                "--local-input",
                "/a",
                "--local-input",
                "/b",
                "--evdev",
                "/solo",
            ],
        ] {
            assert!(LocalSetup::from_settings(&settings(&values), SettingsHost::Linux).is_err());
        }
    }

    #[test]
    fn absent_disabled_duplicate_foreign_and_incomplete_assignments_are_atomic() {
        let base = settings(&[]);
        let mut setup = LocalSetup::from_settings(&base, SettingsHost::Linux).unwrap();
        setup.resize(4).unwrap();
        let rows = catalog(DeviceRequest::LinuxKeyboard, &[("/a", true), ("/b", false)]);
        let first = setup.players()[0].id;
        setup.assign(first, &rows, 0).unwrap();
        let before = setup.players().to_vec();
        assert!(setup.assign(setup.players()[1].id, &rows, 0).is_err());
        assert!(setup.assign(first, &rows, 1).is_err());
        assert!(setup.assign(first, &rows, 2).is_err());
        assert!(setup.assign(PlayerId(999), &rows, 0).is_err());
        assert!(
            setup
                .assign(first, &catalog(DeviceRequest::Alsa, &[("audio", true)]), 0)
                .is_err()
        );
        assert!(
            setup
                .assign(
                    first,
                    &catalog(DeviceRequest::WindowsKeyboard, &[("foreign", true)]),
                    0
                )
                .is_err()
        );
        assert_eq!(setup.players(), before);
        let base_before = base.native_args();
        assert!(setup.settings(&base).is_err());
        assert_eq!(base.native_args(), base_before);
        setup.clear(first).unwrap();
        assert_eq!(setup.players()[0].input(), None);
        assert!(setup.clear(PlayerId(999)).is_err());
        let huge = "x".repeat(crate::settings::MAX_VALUE_BYTES + 1);
        assert!(
            LocalPlayers::from_assignments(
                SettingsHost::Linux,
                64,
                vec![(PlayerId(1), huge), (PlayerId(2), "b".into())]
            )
            .is_err()
        );
    }

    #[test]
    fn exported_id_prefix_and_aggregate_settings_caps_fail_without_mutating_base() {
        let base = settings(&["--alsa", "hw:1"]);
        let mut setup = LocalSetup::from_settings(&base, SettingsHost::Linux).unwrap();
        setup.resize(2).unwrap();
        let long_path = "x".repeat(crate::settings::MAX_VALUE_BYTES);
        let rows = catalog(
            DeviceRequest::LinuxKeyboard,
            &[(long_path.as_str(), true), ("/short", true)],
        );
        setup.assign(setup.players()[0].id, &rows, 0).unwrap();
        setup.assign(setup.players()[1].id, &rows, 1).unwrap();
        let before = base.native_args();
        assert!(setup.settings(&base).is_err()); // ID: makes the first value too long.
        assert_eq!(base.native_args(), before);
        setup.resize(64).unwrap();
        let paths = (0..64)
            .map(|index| (format!("{}:{index}", "x".repeat(1024)), true))
            .collect::<Vec<_>>();
        let row_refs = paths
            .iter()
            .map(|(path, enabled)| (path.as_str(), *enabled))
            .collect::<Vec<_>>();
        let rows = catalog(DeviceRequest::LinuxKeyboard, &row_refs);
        for index in 0..64 {
            setup
                .assign(setup.players()[index].id, &rows, index)
                .unwrap();
        }
        assert!(setup.settings(&base).is_err()); // Whole settings values exceed 64 KiB.
        assert_eq!(base.native_args(), before);
        assert!(
            setup
                .players()
                .iter()
                .all(|player| player.input().is_some())
        );
    }

    #[test]
    fn solo_preserves_advanced_input_and_unsupported_hosts_reject_group_growth() {
        let base = settings(&["--evdev", "/advanced", "--alsa", "hw:1"]);
        let mut setup = LocalSetup::from_settings(&base, SettingsHost::Linux).unwrap();
        assert_eq!(
            setup.settings(&base).unwrap().native_args(),
            base.native_args()
        );
        setup.resize(2).unwrap();
        let rows = catalog(DeviceRequest::LinuxKeyboard, &[("/a", true), ("/b", true)]);
        for index in 0..2 {
            setup
                .assign(setup.players()[index].id, &rows, index)
                .unwrap();
        }
        let group = setup.settings(&base).unwrap();
        assert!(!group.native_args().iter().any(|arg| arg == "--evdev"));
        setup.resize(1).unwrap();
        assert_eq!(
            setup.settings(&base).unwrap().native_args(),
            base.native_args()
        );
        assert!(
            setup
                .settings(&NativeSettings::from_args(&[], SettingsHost::Windows).unwrap())
                .is_err()
        );
        for host in [SettingsHost::Windows, SettingsHost::Macos] {
            let base = NativeSettings::from_args(&[], host).unwrap();
            let mut setup = LocalSetup::from_settings(&base, host).unwrap();
            let before = setup.players().to_vec();
            assert!(setup.resize(2).is_err());
            assert_eq!(setup.players(), before);
            assert!(setup.settings(&base).is_ok());
        }
    }
}
