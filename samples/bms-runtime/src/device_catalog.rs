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
    fn host(self) -> SettingsHost {
        match self {
            Self::Wasapi | Self::Asio { .. } => SettingsHost::Windows,
            Self::Alsa => SettingsHost::Linux,
            Self::Coreaudio => SettingsHost::Macos,
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
            return Err("audio device count exceeds catalog limit".into());
        }
        let mut bytes = 0usize;
        for choice in &mut choices {
            if choice.id.is_empty()
                || choice
                    .id
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
            {
                return Err("audio device ID is empty or contains control characters".into());
            }
            for value in [&choice.id, &choice.label, &choice.detail] {
                if value.len() > MAX_DEVICE_TEXT_BYTES {
                    return Err("audio device text exceeds catalog limit".into());
                }
                bytes = bytes
                    .checked_add(value.len())
                    .ok_or("audio catalog byte overflow")?;
                if bytes > MAX_CATALOG_BYTES {
                    return Err("audio catalog exceeds aggregate byte limit".into());
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
        if DeviceRequest::from_settings(settings, self.request.host())? != self.request {
            return Err("device catalog backend/view no longer matches settings".into());
        }
        let choice = self
            .choices
            .get(index)
            .ok_or("audio device row unavailable")?;
        if !choice.selectable {
            return Err("audio device is not selectable".into());
        }
        let flag = if self.request == DeviceRequest::Alsa {
            "--alsa"
        } else {
            "--device"
        };
        let index = settings
            .fields()
            .iter()
            .position(|f| f.flag == flag)
            .ok_or("audio device field unavailable")?;
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
