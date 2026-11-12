//! Start at login: the Run key on Windows, a LaunchAgent on macOS and an XDG
//! autostart entry on Linux. Each entry points at the current executable and
//! is rewritten on launch so it follows the app when it moves or updates.

use std::path::PathBuf;

fn exe() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::Registry::*;

    const KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE: &str = "MouseKeysRecordReplay";

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn set(on: bool) -> Result<(), String> {
        let key = w(KEY);
        let value = w(VALUE);
        let rc = unsafe {
            if on {
                let exe = super::exe().ok_or("Could not find the app location.")?;
                let data = w(&format!("\"{}\"", exe.display()));
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    value.as_ptr(),
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            } else {
                let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr());
                if r == 2 {
                    0
                } else {
                    r
                }
            }
        };
        if rc == 0 {
            Ok(())
        } else {
            Err("Windows did not allow changing the startup setting.".into())
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::PathBuf;

    const LABEL: &str = "tools.tik.mousekeysrecordreplay";

    fn plist() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(format!("Library/LaunchAgents/{LABEL}.plist")))
    }

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    pub fn set(on: bool) -> Result<(), String> {
        let path = plist().ok_or("Could not find your home folder.")?;
        if !on {
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        let exe = super::exe().ok_or("Could not find the app location.")?;
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key><array><string>{}</string></array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#,
            xml_escape(&exe.display().to_string())
        );
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, body).map_err(|_| "Could not save the login item.".to_string())
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;

    fn entry() -> Option<PathBuf> {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join("autostart/mouse-keys-record-replay.desktop"))
    }

    pub fn set(on: bool) -> Result<(), String> {
        let path = entry().ok_or("Could not find your home folder.")?;
        if !on {
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        let exe = super::exe().ok_or("Could not find the app location.")?;
        let quoted = exe
            .display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let body = format!(
            "[Desktop Entry]\nType=Application\nName=Mouse Keys Record Replay\nExec=\"{quoted}\"\nIcon=mouse-keys-record-replay\nX-GNOME-Autostart-enabled=true\nNoDisplay=false\n"
        );
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, body).map_err(|_| "Could not save the startup entry.".to_string())
    }
}

pub use imp::set;
