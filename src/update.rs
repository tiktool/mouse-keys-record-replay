//! Updates come from the latest release of the public repository. Every
//! download is checked against a signature made with the release key before
//! it replaces the app, so a tampered or truncated file is never installed.

use crate::config::{self, REPO, VERSION};
use ed25519_dalek::{Signature, VerifyingKey};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const PUBLIC_KEY: [u8; 32] = [
    0xb8, 0xd1, 0x41, 0x98, 0xe8, 0xeb, 0xc1, 0x90, 0xa0, 0x42, 0x24, 0x22, 0xfa, 0x47, 0xa6, 0x00,
    0xee, 0xe4, 0xc2, 0xeb, 0x60, 0x70, 0x7c, 0xa3, 0x90, 0xc7, 0x73, 0x4e, 0x08, 0x31, 0x77, 0x8b,
];

const RECHECK: Duration = Duration::from_secs(12 * 3600);
const MAX_BYTES: &str = "80000000";

#[cfg(windows)]
pub const ASSET: &str = "mouse-keys-record-replay-windows-x64.exe";
#[cfg(target_os = "macos")]
pub const ASSET: &str = "mouse-keys-record-replay-macos.zip";
#[cfg(target_os = "linux")]
pub const ASSET: &str = "mouse-keys-record-replay-linux-x64";

#[derive(Debug, Clone)]
pub enum Outcome {
    UpToDate,
    Installed(String),
    Failed(String),
    /// Background checks stay quiet unless something was installed.
    Silent,
}

/// Checks now, then again every few hours while the app keeps running.
pub fn schedule(done: impl Fn(Outcome) + Send + 'static) {
    std::thread::spawn(move || loop {
        let o = run_check(false);
        let installed = matches!(o, Outcome::Installed(_));
        done(o);
        if installed {
            return;
        }
        std::thread::sleep(RECHECK);
    });
}

pub fn check(manual: bool, done: impl FnOnce(Outcome) + Send + 'static) {
    std::thread::spawn(move || done(run_check(manual)));
}

fn run_check(manual: bool) -> Outcome {
    match check_and_install() {
        Ok(Some(v)) => Outcome::Installed(v),
        Ok(None) if manual => Outcome::UpToDate,
        Err(e) if manual => Outcome::Failed(e),
        _ => Outcome::Silent,
    }
}

fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches('v');
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

pub fn is_newer(remote: &str, local: &str) -> bool {
    match (parse_version(remote), parse_version(local)) {
        (Some(r), Some(l)) => r > l,
        _ => false,
    }
}

fn curl() -> Command {
    let path = if cfg!(windows) { r"C:\Windows\System32\curl.exe" } else { "/usr/bin/curl" };
    let mut c = Command::new(path);
    c.args(["-fsSL", "--proto", "=https", "--max-time", "120", "--max-filesize", MAX_BYTES]);
    c.args(["-H", concat!("User-Agent: mouse-keys-record-replay/", env!("CARGO_PKG_VERSION"))]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let out = curl()
        .args(["-H", "Accept: application/vnd.github+json", url])
        .output()
        .map_err(|_| "Could not reach GitHub to check for updates.".to_string())?;
    if !out.status.success() {
        return Err("Could not reach GitHub to check for updates. Check your internet connection.".into());
    }
    Ok(out.stdout)
}

fn download(url: &str, to: &Path) -> Result<(), String> {
    let ok = curl()
        .arg("-o")
        .arg(to)
        .arg(url)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok { Ok(()) } else { Err("The update could not be downloaded. It will be tried again later.".into()) }
}

pub fn verify(data: &[u8], sig: &[u8]) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(&PUBLIC_KEY) else { return false };
    let Ok(sig) = <[u8; 64]>::try_from(sig) else { return false };
    key.verify_strict(data, &Signature::from_bytes(&sig)).is_ok()
}

struct Release {
    version: String,
    asset: String,
    sig: String,
}

fn latest() -> Result<Release, String> {
    let body = fetch(&format!("https://api.github.com/repos/{REPO}/releases/latest"))?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| "GitHub sent an unexpected answer.".to_string())?;
    let version = v["tag_name"].as_str().ok_or("No release found.")?.to_string();
    let url_of = |name: &str| {
        v["assets"].as_array().and_then(|a| {
            a.iter()
                .find(|x| x["name"].as_str() == Some(name))
                .and_then(|x| x["browser_download_url"].as_str())
                .map(str::to_string)
        })
    };
    let asset = url_of(ASSET).ok_or("This release has no download for your system yet.")?;
    let sig = url_of(&format!("{ASSET}.sig")).ok_or("This release is not signed, so it was not installed.")?;
    Ok(Release { version, asset, sig })
}

fn check_and_install() -> Result<Option<String>, String> {
    let r = latest()?;
    if !is_newer(&r.version, VERSION) {
        return Ok(None);
    }
    let dir = config::dir().join("update");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|_| "Could not prepare the update folder.".to_string())?;
    let file = dir.join(ASSET);
    let sig_file = dir.join(format!("{ASSET}.sig"));
    download(&r.asset, &file)?;
    download(&r.sig, &sig_file)?;
    let data = std::fs::read(&file).map_err(|e| e.to_string())?;
    let sig = std::fs::read(&sig_file).map_err(|e| e.to_string())?;
    if !verify(&data, &sig) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err("The downloaded update failed its security check and was discarded.".into());
    }
    install(&file, &dir)?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(Some(r.version.trim_start_matches('v').to_string()))
}

fn exe() -> Result<PathBuf, String> {
    std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|_| "Could not find the app location.".to_string())
}

fn old_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

/// The .app bundle that holds the running binary, if there is one.
#[cfg(target_os = "macos")]
fn bundle() -> Option<PathBuf> {
    let exe = exe().ok()?;
    let b = exe.parent()?.parent()?.parent()?;
    (b.extension()? == "app").then(|| b.to_path_buf())
}

#[cfg(windows)]
fn install(file: &Path, _dir: &Path) -> Result<(), String> {
    let exe = exe()?;
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|_| "The update needs write access to the app folder. Download it from GitHub instead.".to_string())?;
    if std::fs::copy(file, &exe).is_err() {
        let _ = std::fs::rename(&old, &exe);
        return Err("The update could not be installed. Download it from GitHub instead.".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn install(file: &Path, dir: &Path) -> Result<(), String> {
    let Some(app) = bundle() else { return replace_binary(file) };
    let out = dir.join("unpacked");
    let ok = Command::new("/usr/bin/ditto").arg("-x").arg("-k").arg(file).arg(&out).status().map(|s| s.success()).unwrap_or(false);
    let new_app = std::fs::read_dir(&out)
        .ok()
        .and_then(|mut d| d.find_map(|e| e.ok().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "app"))));
    let (true, Some(new_app)) = (ok, new_app) else {
        return Err("The update could not be unpacked.".into());
    };
    let old = old_path(&app);
    let _ = std::fs::remove_dir_all(&old);
    std::fs::rename(&app, &old).map_err(|_| "The update needs write access to the Applications folder.".to_string())?;
    if std::fs::rename(&new_app, &app).is_err() {
        let _ = std::fs::rename(&old, &app);
        return Err("The update could not be installed.".into());
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

#[cfg(unix)]
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn replace_binary(file: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let exe = exe()?;
    let staged = old_path(&exe).with_extension("new");
    std::fs::copy(file, &staged).map_err(|_| "The update needs write access to the app folder.".to_string())?;
    let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    std::fs::rename(&staged, &exe).map_err(|_| "The update could not be installed.".to_string())
}

#[cfg(target_os = "linux")]
fn install(file: &Path, _dir: &Path) -> Result<(), String> {
    replace_binary(file)
}

/// Removes what the previous update left behind.
pub fn cleanup() {
    if let Ok(exe) = exe() {
        let _ = std::fs::remove_file(old_path(&exe));
    }
    let _ = std::fs::remove_dir_all(config::dir().join("update"));
}

pub const RELAUNCH_ARG: &str = "--relaunch";

/// Starts the freshly installed copy and ends this one. The new copy waits
/// for this process to exit before it takes over.
pub fn relaunch() -> ! {
    #[cfg(target_os = "macos")]
    if let Some(app) = bundle() {
        let _ = Command::new("/usr/bin/open").arg("-n").arg(&app).args(["--args", RELAUNCH_ARG]).spawn();
        std::process::exit(0);
    }
    if let Ok(exe) = exe() {
        let _ = Command::new(exe).arg(RELAUNCH_ARG).spawn();
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("v0.1.10", "0.1.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn rejects_bad_signatures() {
        assert!(!verify(b"data", &[0u8; 64]));
        assert!(!verify(b"data", &[0u8; 10]));
    }
}
