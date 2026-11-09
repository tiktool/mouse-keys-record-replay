//! Every finished recording, kept as two files in the recordings folder:
//! `<id>.json` (what the list shows) and `<id>.steps.json` (what plays).

use crate::config;
use crate::engine::{self, Step};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const NAME_MAX: usize = 80;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// Unix seconds.
    pub created: u64,
    pub name: String,
    pub duration_ms: u64,
    /// Presses, clicks and wheel turns, as the status card counts them.
    pub actions: usize,
    #[serde(default)]
    pub starred: bool,
}

pub struct Store {
    dir: PathBuf,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 24 && id.bytes().all(|b| b.is_ascii_digit())
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Store {
    /// Fails when there is no app folder (no HOME on Unix); nothing is then
    /// listed or saved, rather than writing recordings somewhere shared.
    pub fn open() -> std::io::Result<Store> {
        Store::under(config::dir())
    }

    fn under(app_dir: std::io::Result<PathBuf>) -> std::io::Result<Store> {
        Ok(Store::at(app_dir?.join("recordings")))
    }

    pub fn at(dir: PathBuf) -> Store {
        Store { dir }
    }

    fn meta_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn steps_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.steps.json"))
    }

    /// Saves a recording made at `created` (unix seconds), named after its local time.
    pub fn add(&self, steps: &[Step], created: u64) -> std::io::Result<Entry> {
        let mut n = created.saturating_mul(1000);
        while self.meta_path(&n.to_string()).exists() || self.steps_path(&n.to_string()).exists() {
            n += 1;
        }
        let (actions, secs) = engine::summarize(steps);
        let e = Entry {
            id: n.to_string(),
            created,
            name: local_label(created),
            duration_ms: (secs as f64 * 1000.0).round() as u64,
            actions,
            starred: false,
        };
        // Steps first: a meta file only ever points at steps already on disk.
        config::write_atomic(&self.steps_path(&e.id), &serde_json::to_vec(steps)?)?;
        self.write_meta(&e)?;
        Ok(e)
    }

    fn write_meta(&self, e: &Entry) -> std::io::Result<()> {
        config::write_atomic(&self.meta_path(&e.id), &serde_json::to_vec_pretty(e)?)
    }

    /// Starred first, then newest first. Unreadable files are logged and skipped.
    pub fn list(&self) -> Vec<Entry> {
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<Entry> =
            rd.flatten()
                .filter_map(|f| {
                    let name = f.file_name().into_string().ok()?;
                    let id = name.strip_suffix(".json")?;
                    if !valid_id(id) {
                        return None;
                    }
                    match fs::read(f.path()).map_err(|e| e.to_string()).and_then(|b| {
                        serde_json::from_slice::<Entry>(&b).map_err(|e| e.to_string())
                    }) {
                        Ok(e) if e.id == id => Some(e),
                        Ok(_) => {
                            eprintln!("recordings: {name} names another id, skipped");
                            None
                        }
                        Err(err) => {
                            eprintln!("recordings: {name} unreadable, skipped: {err}");
                            None
                        }
                    }
                })
                .collect();
        out.sort_by(|a, b| {
            b.starred
                .cmp(&a.starred)
                .then(b.created.cmp(&a.created))
                .then(b.id.cmp(&a.id))
        });
        out
    }

    pub fn get(&self, id: &str) -> Option<Entry> {
        if !valid_id(id) {
            return None;
        }
        let b = fs::read(self.meta_path(id)).ok()?;
        serde_json::from_slice(&b).ok()
    }

    pub fn steps(&self, id: &str) -> Option<Vec<Step>> {
        if !valid_id(id) {
            return None;
        }
        let b = fs::read(self.steps_path(id)).ok()?;
        match serde_json::from_slice(&b) {
            Ok(s) => Some(s),
            Err(err) => {
                eprintln!("recordings: {id} steps unreadable: {err}");
                None
            }
        }
    }

    /// Blank names are refused; long ones are cut to NAME_MAX characters.
    pub fn rename(&self, id: &str, name: &str) -> bool {
        let name: String = name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(NAME_MAX)
            .collect();
        if name.is_empty() {
            return false;
        }
        self.edit(id, |e| e.name = name)
    }

    pub fn set_starred(&self, id: &str, on: bool) -> bool {
        self.edit(id, |e| e.starred = on)
    }

    fn edit(&self, id: &str, f: impl FnOnce(&mut Entry)) -> bool {
        let Some(mut e) = self.get(id) else {
            return false;
        };
        f(&mut e);
        self.write_meta(&e).is_ok()
    }

    pub fn delete(&self, id: &str) -> bool {
        if !valid_id(id) {
            return false;
        }
        // Meta first, so a half-finished delete never lists an entry without steps.
        let gone = fs::remove_file(self.meta_path(id)).is_ok();
        let _ = fs::remove_file(self.steps_path(id));
        gone
    }
}

/// Moves the single recording older versions kept into the store and returns
/// its id. The old file is renamed, never deleted; an unreadable one is left as is.
pub fn migrate(store: &Store, old: &Path) -> Option<String> {
    let b = fs::read(old).ok()?;
    let steps: Vec<Step> = match serde_json::from_slice(&b) {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "recordings: {} unreadable, left in place: {err}",
                old.display()
            );
            return None;
        }
    };
    if steps.len() <= 1 {
        return None;
    }
    let created = fs::metadata(old)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or_else(now_secs);
    let e = store.add(&steps, created).ok()?;
    let _ = fs::rename(old, old.with_extension("old.json"));
    Some(e.id)
}

/// "2026-11-09 14:05" in local time.
pub fn local_label(secs: u64) -> String {
    let (y, mo, d, h, mi) = local_parts(secs as i64);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}")
}

/// Days since 1970-01-01 to (year, month, day). Howard Hinnant's civil_from_days.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn utc_parts(secs: i64) -> (i64, u32, u32, u32, u32) {
    let (y, mo, d) = civil(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    (y, mo, d, (s / 3600) as u32, (s % 3600 / 60) as u32)
}

#[cfg(unix)]
fn local_parts(secs: i64) -> (i64, u32, u32, u32, u32) {
    let t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return utc_parts(secs);
    }
    (
        tm.tm_year as i64 + 1900,
        tm.tm_mon as u32 + 1,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
    )
}

/// Windows: the current UTC offset applied to the timestamp.
#[cfg(windows)]
fn local_parts(secs: i64) -> (i64, u32, u32, u32, u32) {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
    fn to_secs(t: &SYSTEMTIME) -> i64 {
        // days_from_civil, the inverse of civil().
        let (y, m, d) = (t.wYear as i64, t.wMonth as i64, t.wDay as i64);
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        days * 86_400 + t.wHour as i64 * 3600 + t.wMinute as i64 * 60 + t.wSecond as i64
    }
    let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let mut utc: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe {
        GetLocalTime(&mut local);
        GetSystemTime(&mut utc);
    }
    let offset = ((to_secs(&local) - to_secs(&utc)) as f64 / 60.0).round() as i64 * 60;
    utc_parts(secs + offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Ev;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mkrr-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn rec(n: u64) -> Vec<Step> {
        let mut v = vec![Step {
            t: 0,
            ev: Ev::Move { x: 1, y: 2 },
        }];
        for i in 1..=n {
            v.push(Step {
                t: i * 500_000,
                ev: Ev::Button {
                    b: 1,
                    down: true,
                    x: 0,
                    y: 0,
                    n: 1,
                },
            });
            v.push(Step {
                t: i * 500_000 + 1,
                ev: Ev::Button {
                    b: 1,
                    down: false,
                    x: 0,
                    y: 0,
                    n: 1,
                },
            });
        }
        v
    }

    #[test]
    fn save_keeps_steps_and_summary() {
        let s = Store::at(tmp("save"));
        let steps = rec(3);
        let e = s.add(&steps, 1_700_000_000).unwrap();
        assert_eq!(e.actions, 3);
        assert_eq!(e.duration_ms, 1500);
        assert!(!e.starred);
        assert_eq!(e.name, local_label(1_700_000_000));
        assert_eq!(s.steps(&e.id).unwrap(), steps);
        assert_eq!(s.get(&e.id).unwrap(), e);
    }

    #[test]
    fn same_second_gets_distinct_ids() {
        let s = Store::at(tmp("ids"));
        let a = s.add(&rec(1), 100).unwrap();
        let b = s.add(&rec(2), 100).unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(s.list().len(), 2);
    }

    #[test]
    fn list_puts_starred_first_then_newest() {
        let s = Store::at(tmp("order"));
        let old = s.add(&rec(1), 100).unwrap();
        let mid = s.add(&rec(1), 200).unwrap();
        let new = s.add(&rec(1), 300).unwrap();
        let ids = |s: &Store| s.list().into_iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(ids(&s), [new.id.clone(), mid.id.clone(), old.id.clone()]);
        assert!(s.set_starred(&old.id, true));
        assert_eq!(ids(&s), [old.id.clone(), new.id.clone(), mid.id.clone()]);
        assert!(s.set_starred(&old.id, false));
        assert_eq!(ids(&s), [new.id, mid.id, old.id]);
    }

    #[test]
    fn rename_trims_caps_and_refuses_blank() {
        let s = Store::at(tmp("rename"));
        let e = s.add(&rec(1), 100).unwrap();
        assert!(s.rename(&e.id, "  Login flow  "));
        assert_eq!(s.get(&e.id).unwrap().name, "Login flow");
        assert!(!s.rename(&e.id, "   "));
        assert_eq!(s.get(&e.id).unwrap().name, "Login flow");
        assert!(s.rename(&e.id, &"x".repeat(200)));
        assert_eq!(s.get(&e.id).unwrap().name.chars().count(), NAME_MAX);
        assert!(!s.rename("999", "nobody"));
        assert!(!s.rename("../settings", "nobody"));
    }

    #[test]
    fn star_persists() {
        let s = Store::at(tmp("star"));
        let e = s.add(&rec(1), 100).unwrap();
        assert!(s.set_starred(&e.id, true));
        assert!(Store::at(s.dir.clone()).get(&e.id).unwrap().starred);
    }

    #[test]
    fn delete_removes_both_files() {
        let s = Store::at(tmp("delete"));
        let a = s.add(&rec(1), 100).unwrap();
        let b = s.add(&rec(1), 200).unwrap();
        assert!(s.delete(&a.id));
        assert!(!s.meta_path(&a.id).exists());
        assert!(!s.steps_path(&a.id).exists());
        assert!(s.steps(&a.id).is_none());
        assert_eq!(s.list(), vec![b]);
        assert!(!s.delete(&a.id));
    }

    #[test]
    fn migration_imports_old_file_and_keeps_it() {
        let d = tmp("migrate");
        let old = d.join("recording.json");
        let steps = rec(4);
        fs::write(&old, serde_json::to_vec(&steps).unwrap()).unwrap();
        let s = Store::at(d.join("recordings"));
        let id = migrate(&s, &old).unwrap();
        assert_eq!(s.steps(&id).unwrap(), steps);
        assert_eq!(s.list().len(), 1);
        assert!(!old.exists());
        assert!(d.join("recording.old.json").exists());
        // A second start finds nothing left to import.
        assert!(migrate(&s, &old).is_none());
        assert_eq!(s.list().len(), 1);
    }

    #[test]
    fn migration_leaves_unreadable_or_empty_old_file() {
        let d = tmp("migrate-bad");
        let old = d.join("recording.json");
        let s = Store::at(d.join("recordings"));
        fs::write(&old, b"{not json").unwrap();
        assert!(migrate(&s, &old).is_none());
        assert_eq!(fs::read(&old).unwrap(), b"{not json");
        fs::write(&old, b"[]").unwrap();
        assert!(migrate(&s, &old).is_none());
        assert!(old.exists());
        assert!(s.list().is_empty());
        assert!(migrate(&s, &d.join("missing.json")).is_none());
    }

    #[test]
    fn corrupt_files_are_skipped() {
        let s = Store::at(tmp("corrupt"));
        let good = s.add(&rec(1), 100).unwrap();
        fs::write(s.dir.join("123.json"), b"\x00garbage").unwrap();
        fs::write(
            s.dir.join("456.json"),
            br#"{"id":"789","created":1,"name":"x","duration_ms":1,"actions":1}"#,
        )
        .unwrap();
        fs::write(s.dir.join("notes.txt"), b"hi").unwrap();
        fs::write(
            s.steps_path(&good.id).with_file_name("777.steps.json"),
            b"[",
        )
        .unwrap();
        assert_eq!(s.list(), vec![good]);
        assert!(s.steps("777").is_none());
        assert!(s.get("123").is_none());
    }

    #[test]
    fn writes_are_atomic() {
        let s = Store::at(tmp("atomic"));
        let e = s.add(&rec(1), 100).unwrap();
        // A temp file left by an interrupted write is never listed.
        fs::write(s.dir.join(format!("{}.tmp", e.id)), b"half").unwrap();
        assert!(s.rename(&e.id, "Renamed"));
        assert_eq!(s.list().len(), 1);
        assert_eq!(s.get(&e.id).unwrap().name, "Renamed");
        let left: Vec<_> = fs::read_dir(&s.dir)
            .unwrap()
            .flatten()
            .map(|f| f.file_name().into_string().unwrap())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(left.is_empty(), "temp files left: {left:?}");
    }

    #[test]
    fn no_app_folder_means_no_store() {
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "no home");
        let e = Store::under(Err(missing)).err().unwrap();
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
        let s = Store::under(Ok(PathBuf::from("app"))).unwrap();
        assert_eq!(s.dir, PathBuf::from("app").join("recordings"));
    }

    #[test]
    fn missing_folder_lists_nothing() {
        let s = Store::at(std::env::temp_dir().join("mkrr-history-does-not-exist-xyz"));
        assert!(s.list().is_empty());
    }

    #[test]
    fn civil_dates() {
        assert_eq!(utc_parts(0), (1970, 1, 1, 0, 0));
        assert_eq!(utc_parts(1_700_000_000), (2023, 11, 14, 22, 13));
        assert_eq!(utc_parts(951_782_400), (2000, 2, 29, 0, 0));
        assert_eq!(local_label(1_700_000_000).len(), 16);
    }
}
