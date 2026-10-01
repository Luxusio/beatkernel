//! Bounded portable device metadata; selection changes only an explicit settings draft.
use crate::settings::{NativeSettings, SettingsHost};

pub const MAX_DEVICES: usize = 1024;
pub const MAX_DEVICE_TEXT_BYTES: usize = 4096;
pub const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsioView {
    Native,
    Bits32,
    Bits64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceRequest {
    Wasapi,
    Asio { view: AsioView },
    Alsa,
    Coreaudio,
    WindowsKeyboard,
    LinuxKeyboard,
    MacosKeyboard,
}
impl DeviceRequest {
    pub fn from_settings(settings: &NativeSettings, host: SettingsHost) -> Result<Self, String> {
        let value = |flag| {
            settings
                .fields()
                .iter()
                .find(|f| f.flag == flag)
                .map(|f| f.value.as_str())
        };
        match host {
            SettingsHost::Linux if value("--alsa").is_some() => Ok(Self::Alsa),
            SettingsHost::Macos if value("--device").is_some() && value("--backend").is_none() => {
                Ok(Self::Coreaudio)
            }
            SettingsHost::Windows if value("--backend").is_some() => {
                match value("--backend").unwrap() {
                    "" | "wasapi" => Ok(Self::Wasapi),
                    "asio" => {
                        Ok(Self::Asio {
                            view: match value("--asio-view") {
                                Some("native") => AsioView::Native,
                                Some("32") => AsioView::Bits32,
                                Some("64") => AsioView::Bits64,
                                _ => return Err(
                                    "ASIO discovery requires explicit asio-view native, 32 or 64"
                                        .into(),
                                ),
                            },
                        })
                    }
                    _ => Err("unsupported audio backend for discovery".into()),
                }
            }
            _ => Err("settings do not match the discovery host".into()),
        }
    }
    pub fn keyboard(settings: &NativeSettings, host: SettingsHost) -> Result<Self, String> {
        let request = match host {
            SettingsHost::Windows => Self::WindowsKeyboard,
            SettingsHost::Linux => Self::LinuxKeyboard,
            SettingsHost::Macos => Self::MacosKeyboard,
        };
        if settings
            .fields()
            .iter()
            .any(|f| f.flag == request.field_flag())
        {
            Ok(request)
        } else {
            Err("keyboard settings do not match discovery host".into())
        }
    }
    pub fn is_keyboard(self) -> bool {
        matches!(
            self,
            Self::WindowsKeyboard | Self::LinuxKeyboard | Self::MacosKeyboard
        )
    }
    pub fn field_flag(self) -> &'static str {
        match self {
            Self::Alsa => "--alsa",
            Self::WindowsKeyboard => "--keyboard-path",
            Self::LinuxKeyboard => "--evdev",
            Self::MacosKeyboard => "--keyboard-registry",
            _ => "--device",
        }
    }
    fn host(self) -> SettingsHost {
        match self {
            Self::Wasapi | Self::Asio { .. } | Self::WindowsKeyboard => SettingsHost::Windows,
            Self::Alsa | Self::LinuxKeyboard => SettingsHost::Linux,
            Self::Coreaudio | Self::MacosKeyboard => SettingsHost::Macos,
        }
    }
}
#[derive(Debug)]
pub struct DeviceChoice {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub selectable: bool,
}
#[derive(Debug)]
pub struct DeviceCatalog {
    request: DeviceRequest,
    choices: Vec<DeviceChoice>,
}
impl DeviceCatalog {
    pub fn new(request: DeviceRequest, mut choices: Vec<DeviceChoice>) -> Result<Self, String> {
        if choices.len() > MAX_DEVICES {
            return Err("device count exceeds catalog limit".into());
        }
        let mut bytes = 0usize;
        for choice in &mut choices {
            if choice.id.is_empty()
                || choice
                    .id
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
            {
                return Err("device ID is empty or contains control characters".into());
            }
            for value in [&choice.id, &choice.label, &choice.detail] {
                if value.len() > MAX_DEVICE_TEXT_BYTES {
                    return Err("device text exceeds catalog limit".into());
                }
                bytes = bytes
                    .checked_add(value.len())
                    .ok_or("device catalog byte overflow")?;
                if bytes > MAX_CATALOG_BYTES {
                    return Err("device catalog exceeds aggregate byte limit".into());
                }
            }
            for value in [&mut choice.label, &mut choice.detail] {
                *value = value
                    .chars()
                    .map(|c| {
                        if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                            ' '
                        } else {
                            c
                        }
                    })
                    .collect();
            }
        }
        Ok(Self { request, choices })
    }
    pub fn request(&self) -> DeviceRequest {
        self.request
    }
    pub fn choices(&self) -> &[DeviceChoice] {
        &self.choices
    }
    pub fn apply(&self, index: usize, settings: &mut NativeSettings) -> Result<(), String> {
        let current = if self.request.is_keyboard() {
            DeviceRequest::keyboard(settings, self.request.host())?
        } else {
            DeviceRequest::from_settings(settings, self.request.host())?
        };
        if current != self.request {
            return Err("device catalog backend/view no longer matches settings".into());
        }
        let choice = self.choices.get(index).ok_or("device row unavailable")?;
        if !choice.selectable {
            return Err("device is not selectable".into());
        }
        let flag = self.request.field_flag();
        let index = settings
            .fields()
            .iter()
            .position(|f| f.flag == flag)
            .ok_or("device field unavailable")?;
        settings.set_value(index, &choice.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn choice(id: &str, selectable: bool) -> DeviceChoice {
        DeviceChoice {
            id: id.into(),
            label: "name\nsecond line".into(),
            detail: "metadata".into(),
            selectable,
        }
    }
    #[test]
    fn keyboard_selection_changes_only_host_input_identity() {
        for (host, request, id) in [
            (
                SettingsHost::Windows,
                DeviceRequest::WindowsKeyboard,
                "exact-interface-path",
            ),
            (
                SettingsHost::Linux,
                DeviceRequest::LinuxKeyboard,
                "/dev/input/event3",
            ),
            (SettingsHost::Macos, DeviceRequest::MacosKeyboard, "42"),
        ] {
            let mut draft =
                NativeSettings::from_args(&["--input-offset-ns".into(), "-123".into()], host)
                    .unwrap();
            assert_eq!(DeviceRequest::keyboard(&draft, host).unwrap(), request);
            let catalog = DeviceCatalog::new(request, vec![choice(id, true)]).unwrap();
            catalog.apply(0, &mut draft).unwrap();
            assert_eq!(
                draft
                    .fields()
                    .iter()
                    .find(|f| f.flag == request.field_flag())
                    .unwrap()
                    .value,
                id
            );
            assert_eq!(
                draft
                    .fields()
                    .iter()
                    .find(|f| f.flag == "--input-offset-ns")
                    .unwrap()
                    .value,
                "-123"
            );
            let other_host = if host == SettingsHost::Linux {
                SettingsHost::Windows
            } else {
                SettingsHost::Linux
            };
            let mut foreign = NativeSettings::from_args(&[], other_host).unwrap();
            assert!(catalog.apply(0, &mut foreign).is_err());
            assert!(foreign.native_args().is_empty());
        }
    }
    #[test]
    fn exact_selection_and_failed_selection_preserve_draft() {
        let mut draft = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
        let catalog = DeviceCatalog::new(
            DeviceRequest::Alsa,
            vec![choice("hw:2,1", true), choice("disabled", false)],
        )
        .unwrap();
        assert_eq!(catalog.choices()[0].label, "name second line");
        catalog.apply(0, &mut draft).unwrap();
        let accepted = draft.native_args();
        assert!(catalog.apply(1, &mut draft).is_err());
        assert!(catalog.apply(2, &mut draft).is_err());
        assert_eq!(draft.native_args(), accepted);
        assert_eq!(accepted, vec!["--alsa", "hw:2,1"]);
        assert!(DeviceCatalog::new(DeviceRequest::Alsa, vec![choice("bad\nID", true)]).is_err());
        assert!(
            DeviceCatalog::new(
                DeviceRequest::Alsa,
                vec![choice(&"x".repeat(MAX_DEVICE_TEXT_BYTES + 1), true)]
            )
            .is_err()
        );
    }
    #[test]
    fn asio_catalog_is_tied_to_explicit_registry_view() {
        let args = ["--backend", "asio", "--asio-view", "32"].map(String::from);
        let mut draft = NativeSettings::from_args(&args, SettingsHost::Windows).unwrap();
        let catalog = DeviceCatalog::new(
            DeviceRequest::Asio {
                view: AsioView::Bits64,
            },
            vec![choice("{CLSID}", true)],
        )
        .unwrap();
        let before = draft.native_args();
        assert!(catalog.apply(0, &mut draft).is_err());
        assert_eq!(draft.native_args(), before);
        let blank =
            NativeSettings::from_args(&["--backend".into(), "asio".into()], SettingsHost::Windows)
                .unwrap();
        assert!(DeviceRequest::from_settings(&blank, SettingsHost::Windows).is_err());
    }
}
