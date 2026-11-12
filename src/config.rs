use crate::shortcuts;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
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
    let s = SITE_URL
        .strip_prefix("https://")
        .or_else(|| SITE_URL.strip_prefix("http://"))
        .unwrap_or(SITE_URL);
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
    /// Moving the mouse or pressing a key stops playback. Off: only the
    /// shortcuts stop it.
    pub stop_on_input: bool,
    /// Recordings keep clicks, scrolls and key presses, not pointer movement.
    pub skip_moves: bool,
    /// Index into speed::LABELS; the last step plays at `custom_speed`.
    pub speed_step: usize,
    pub custom_speed: f64,
    pub history: String,
    /// Id of the saved recording the play shortcuts use.
    pub active: String,
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
            stop_on_input: true,
            skip_moves: false,
            speed_step: crate::speed::DEFAULT_STEP,
            custom_speed: crate::speed::DEFAULT_CUSTOM,
            history: shortcuts::DEFAULT_HISTORY.into(),
            active: String::new(),
        }
    }
}

/// The app's own folder for settings and recordings. On Unix there is no shared
/// fallback: without HOME (or XDG_CONFIG_HOME on Linux) this is an error, since
/// recordings can hold typed text and must not land in a world-readable place.
pub fn dir() -> io::Result<PathBuf> {
    dir_from(|k| std::env::var_os(k))
}

fn dir_from(var: impl Fn(&str) -> Option<OsString>) -> io::Result<PathBuf> {
    let base = if cfg!(windows) {
        Some(
            var("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir),
        )
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    let name = if cfg!(target_os = "linux") {
        "mouse-keys-record-replay"
    } else {
        APP_NAME
    };
    base.map(|b| b.join(name)).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "HOME is not set, so there is no folder for settings",
        )
    })
}

/// Creates the app folder and, on Unix, makes it and the files already in it
/// private to the user. Folders and files from older versions were 0755/0644.
#[cfg(unix)]
pub fn private_dir() -> io::Result<PathBuf> {
    let d = dir()?;
    create_private_dir(&d)?;
    tighten(&d)?;
    Ok(d)
}

/// Creates a folder (and any missing parents) private to the user on Unix.
/// Everything under the app folder is created through this.
#[cfg(unix)]
pub fn create_private_dir(d: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(d)?;
    fs::set_permissions(d, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
pub fn create_private_dir(d: &Path) -> io::Result<()> {
    fs::create_dir_all(d)
}

// One level deep on purpose: the only subfolder is update/, which is removed
// at every start.
#[cfg(unix)]
fn tighten(d: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    for e in fs::read_dir(d)?.flatten() {
        let Ok(m) = fs::symlink_metadata(e.path()) else {
            continue;
        };
        let mode = if m.is_dir() {
            0o700
        } else if m.is_file() {
            0o600
        } else {
            continue;
        };
        let _ = fs::set_permissions(e.path(), fs::Permissions::from_mode(mode));
    }
    Ok(())
}

#[cfg(unix)]
fn open_private(p: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(p)?;
    // mode() only applies when the file is created; a leftover temp file keeps its old mode.
    f.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(f)
}

#[cfg(not(unix))]
fn open_private(p: &Path) -> io::Result<fs::File> {
    fs::File::create(p)
}

pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(p) = path.parent() {
        create_private_dir(p)?;
    }
    let tmp = path.with_extension("tmp");
    open_private(&tmp)?.write_all(data)?;
    fs::rename(&tmp, path)
}

fn read(name: &str) -> Option<Vec<u8>> {
    fs::read(dir().ok()?.join(name)).ok()
}

fn write(name: &str, data: &[u8]) {
    if let Ok(d) = dir() {
        let _ = write_atomic(&d.join(name), data);
    }
}

pub fn load_settings() -> Settings {
    read("settings.json")
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_settings(s: &Settings) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        write("settings.json", &b);
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
        assert!(s.stop_on_input && !s.skip_moves);
    }

    #[test]
    fn playback_options_round_trip() {
        let s = Settings {
            stop_on_input: false,
            skip_moves: true,
            ..Settings::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert!(!back.stop_on_input && back.skip_moves);
    }

    #[test]
    fn speed_round_trips() {
        let s = Settings {
            speed_step: 5,
            custom_speed: 2.5,
            ..Settings::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!((back.speed_step, back.custom_speed), (5, 2.5));
    }

    #[test]
    fn site_label_is_the_url_host() {
        assert_eq!(site_label(), "mouserec.com");
        assert!(SITE_URL.ends_with(site_label()));
    }

    #[cfg(unix)]
    #[test]
    fn missing_home_is_an_error_not_a_temp_folder() {
        let e = dir_from(|_| None).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        let d = dir_from(|k| (k == "HOME").then(|| OsString::from("/home/u"))).unwrap();
        assert!(d.starts_with("/home/u"));
    }

    #[cfg(unix)]
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mkrr-perms-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[cfg(unix)]
    fn mode(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_makes_folder_0700_and_file_0600() {
        let d = scratch("write").join("app");
        let f = d.join("recording.json");
        write_atomic(&f, b"[]").unwrap();
        assert_eq!(mode(&d), 0o700);
        assert_eq!(mode(&f), 0o600);
        assert_eq!(fs::read(&f).unwrap(), b"[]");
        fs::remove_dir_all(d.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn leftover_temp_file_is_reopened_0600() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("tmp");
        fs::create_dir_all(&d).unwrap();
        let tmp = d.join("recording.tmp");
        fs::write(&tmp, b"old").unwrap();
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644)).unwrap();
        drop(open_private(&tmp).unwrap());
        assert_eq!(mode(&tmp), 0o600);
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn existing_folder_and_files_are_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("tighten");
        let sub = d.join("update");
        fs::create_dir_all(&sub).unwrap();
        let f = d.join("settings.json");
        fs::write(&f, b"{}").unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        create_private_dir(&d).unwrap();
        tighten(&d).unwrap();
        assert_eq!(mode(&d), 0o700);
        assert_eq!(mode(&sub), 0o700);
        assert_eq!(mode(&f), 0o600);
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn update_folder_and_missing_app_folder_are_0700() {
        let d = scratch("update");
        let app = d.join("app");
        let update = app.join("update");
        create_private_dir(&update).unwrap();
        assert_eq!(mode(&app), 0o700);
        assert_eq!(mode(&update), 0o700);
        fs::remove_dir_all(&d).unwrap();
    }
}
