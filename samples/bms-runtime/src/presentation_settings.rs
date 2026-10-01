//! Portable display drafts and profile values, without GPU/window ownership.
use std::str::FromStr;

/// Explicit graphics backend vocabulary shared by profiles and native/Web views.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendChoice {
    Auto,
    Vulkan,
    Dx12,
    Metal,
    Gl,
}
impl BackendChoice {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Vulkan => "vulkan",
            Self::Dx12 => "dx12",
            Self::Metal => "metal",
            Self::Gl => "gl",
        }
    }
}
impl FromStr for BackendChoice {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "vulkan" => Ok(Self::Vulkan),
            "dx12" => Ok(Self::Dx12),
            "metal" => Ok(Self::Metal),
            "gl" => Ok(Self::Gl),
            _ => Err("graphics backend must be auto, vulkan, dx12, metal, or gl".into()),
        }
    }
}

/// Requested presentation policy; actual surface support remains GPU-owned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    Fifo,
    Immediate,
    Mailbox,
}
impl Presentation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fifo => "fifo",
            Self::Immediate => "immediate",
            Self::Mailbox => "mailbox",
        }
    }
}
impl FromStr for Presentation {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "fifo" => Ok(Self::Fifo),
            "immediate" => Ok(Self::Immediate),
            "mailbox" => Ok(Self::Mailbox),
            _ => Err("presentation must be fifo, immediate, or mailbox".into()),
        }
    }
}

/// Bounded presentation draft; validation never probes GPU or native resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentationSettings {
    pub backend: BackendChoice,
    pub presentation: Presentation,
    pub fps: u16,
    pub lookahead_ms: u32,
}
impl Default for PresentationSettings {
    fn default() -> Self {
        Self {
            backend: BackendChoice::Auto,
            presentation: Presentation::Fifo,
            fps: 120,
            lookahead_ms: 2000,
        }
    }
}
impl PresentationSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(30..=240).contains(&self.fps) {
            return Err("UI fps must be 30..240".into());
        }
        if !(100..=10000).contains(&self.lookahead_ms) {
            return Err("UI lookahead must be 100..10000 ms".into());
        }
        Ok(())
    }
    /// Four canonical ordered flag/value pairs, independent of native settings.
    pub fn args(&self) -> Vec<String> {
        vec![
            "--gpu-backend".into(),
            self.backend.as_str().into(),
            "--present".into(),
            self.presentation.as_str().into(),
            "--ui-fps".into(),
            self.fps.to_string(),
            "--ui-lookahead-ms".into(),
            self.lookahead_ms.to_string(),
        ]
    }
    /// Applies only supplied known pairs to a candidate; failure leaves self intact.
    pub fn apply_overrides(&self, args: &[String]) -> Result<Self, String> {
        if args.len() % 2 != 0 || args.len() > 8 {
            return Err("presentation overrides require at most four flag/value pairs".into());
        }
        let mut seen = [false; 4];
        let mut next = *self;
        for pair in args.chunks_exact(2) {
            let index = match pair[0].as_str() {
                "--gpu-backend" => 0,
                "--present" => 1,
                "--ui-fps" => 2,
                "--ui-lookahead-ms" => 3,
                _ => return Err(format!("unknown presentation setting {}", pair[0])),
            };
            if seen[index] {
                return Err(format!("duplicate presentation setting {}", pair[0]));
            }
            seen[index] = true;
            match index {
                0 => next.backend = pair[1].parse()?,
                1 => next.presentation = pair[1].parse()?,
                2 | 3 => {
                    if pair[1].is_empty() || !pair[1].bytes().all(|byte| byte.is_ascii_digit()) {
                        return Err(
                            "presentation numeric values require unsigned decimal digits".into(),
                        );
                    }
                    if index == 2 {
                        next.fps = pair[1].parse().map_err(|_| "UI fps exceeds u16")?;
                    } else {
                        next.lookahead_ms =
                            pair[1].parse().map_err(|_| "UI lookahead exceeds u32")?;
                    }
                }
                _ => unreachable!("known presentation setting index"),
            }
        }
        next.validate()?;
        Ok(next)
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }
    #[test]
    fn portable_vocab_and_defaults_roundtrip_without_gpu_features() {
        for backend in [
            BackendChoice::Auto,
            BackendChoice::Vulkan,
            BackendChoice::Dx12,
            BackendChoice::Metal,
            BackendChoice::Gl,
        ] {
            assert_eq!(backend.as_str().parse::<BackendChoice>().unwrap(), backend);
        }
        for policy in [
            Presentation::Fifo,
            Presentation::Immediate,
            Presentation::Mailbox,
        ] {
            assert_eq!(policy.as_str().parse::<Presentation>().unwrap(), policy);
        }
        let defaults = PresentationSettings::default();
        assert_eq!(
            defaults.args(),
            args(&[
                "--gpu-backend",
                "auto",
                "--present",
                "fifo",
                "--ui-fps",
                "120",
                "--ui-lookahead-ms",
                "2000"
            ])
        );
        assert_eq!(
            defaults.apply_overrides(&defaults.args()).unwrap(),
            defaults
        );
    }
    #[test]
    fn partial_overrides_preserve_unsupplied_values_and_checked_boundaries() {
        let original = PresentationSettings {
            backend: BackendChoice::Vulkan,
            presentation: Presentation::Mailbox,
            fps: 90,
            lookahead_ms: 5000,
        };
        let next = original
            .apply_overrides(&args(&["--ui-fps", "30", "--ui-lookahead-ms", "100"]))
            .unwrap();
        assert_eq!(next.backend, original.backend);
        assert_eq!(next.presentation, original.presentation);
        assert_eq!((next.fps, next.lookahead_ms), (30, 100));
        assert_eq!(
            next.apply_overrides(&args(&["--ui-fps", "240", "--ui-lookahead-ms", "10000"]))
                .unwrap()
                .fps,
            240
        );
        assert_eq!(original.fps, 90);
    }
    #[test]
    fn malformed_unknown_duplicate_and_out_of_range_overrides_are_atomic() {
        let original = PresentationSettings::default();
        for invalid in [
            args(&["--ui-fps"]),
            args(&["--unknown", "1"]),
            args(&["--ui-fps", "120", "--ui-fps", "90"]),
            args(&["--ui-fps", "29"]),
            args(&["--ui-fps", "241"]),
            args(&["--ui-fps", "65536"]),
            args(&["--ui-lookahead-ms", "99"]),
            args(&["--ui-lookahead-ms", "10001"]),
            args(&["--ui-lookahead-ms", "4294967296"]),
            args(&["--ui-fps", " 120"]),
            args(&["--ui-fps", "+120"]),
            args(&["--gpu-backend", "AUTO"]),
            args(&["--present", "automatic"]),
        ] {
            assert!(original.apply_overrides(&invalid).is_err());
            assert_eq!(original, PresentationSettings::default());
        }
    }
}
