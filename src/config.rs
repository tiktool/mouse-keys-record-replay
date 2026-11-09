use crate::engine::Step;
use crate::shortcuts;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "Mouse Keys Record Replay";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPO: &str = "tiktool/mouse-keys-record-replay";
pub const REPO_URL: &str = "https://github.com/tiktool/mouse-keys-record-replay";
#[cfg_attr(not(windows), allow(dead_code))]
pub const SITE_URL: &str = "https://mouserec.com";

/// The site link text: SITE_URL without its scheme, so the two cannot drift.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn site_label() -> &'static str {
    let s = SITE_URL.strip_prefix("https://").or_else(|| SITE_URL.strip_prefix("http://")).unwrap_or(SITE_URL);
    s.trim_end_matches('/')
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub record: String,
    pub play: String,
    #[serde(rename = "loop")]
    pub repeat: String,
    pub autostart: bool,
    pub welcomed: bool,
    pub auto_update: bool,
    pub random_timing: bool,
    /// Index into speed::LABELS; the last step plays at `custom_speed`.
    pub speed_step: usize,
    pub custom_speed: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            record: shortcuts::DEFAULT_RECORD.into(),
            play: shortcuts::DEFAULT_PLAY.into(),
            repeat: shortcuts::DEFAULT_LOOP.into(),
            autostart: true,
            welcomed: false,
            auto_update: true,
            random_timing: false,
            speed_step: crate::speed::DEFAULT_STEP,
            custom_speed: crate::speed::DEFAULT_CUSTOM,
        }
    }
}

pub fn dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    let name = if cfg!(target_os = "linux") { "mouse-keys-record-replay" } else { APP_NAME };
    base.unwrap_or_else(std::env::temp_dir).join(name)
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

pub fn load_settings() -> Settings {
    fs::read(dir().join("settings.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_settings(s: &Settings) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let _ = write_atomic(&dir().join("settings.json"), &b);
    }
}

pub fn load_recording() -> Vec<Step> {
    fs::read(dir().join("recording.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_recording(steps: &[Step]) {
    if let Ok(b) = serde_json::to_vec(steps) {
        let _ = write_atomic(&dir().join("recording.json"), &b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_without_speed_load_with_defaults() {
        let old = r#"{"record":"ctrl+alt+shift+F1","play":"ctrl+alt+shift+F2","loop":"ctrl+alt+shift+F3","autostart":false,"welcomed":true,"auto_update":true,"random_timing":true}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert!(!s.autostart && s.welcomed && s.random_timing);
        assert_eq!(s.repeat, "ctrl+alt+shift+F3");
        assert_eq!(s.speed_step, crate::speed::DEFAULT_STEP);
        assert_eq!(s.custom_speed, crate::speed::DEFAULT_CUSTOM);
    }

    #[test]
    fn speed_round_trips() {
        let s = Settings { speed_step: 5, custom_speed: 2.5, ..Settings::default() };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!((back.speed_step, back.custom_speed), (5, 2.5));
    }

    #[test]
    fn site_label_is_the_url_host() {
        assert_eq!(super::site_label(), "mouserec.com");
        assert!(super::SITE_URL.ends_with(super::site_label()));
    }
}
