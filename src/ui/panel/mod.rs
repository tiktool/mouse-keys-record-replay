//! The recordings panel: a card at the top center of the display under the
//! cursor that lists saved recordings. It takes focus and closes when it
//! loses it. Calls run on the main thread, and user actions go back through
//! the recording functions in `app`.

/// One saved recording as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub starred: bool,
    pub active: bool,
    /// Size on disk.
    pub bytes: u64,
}

/// The header line: how many recordings are saved and the disk space they take.
pub fn count_text(rows: &[Row]) -> String {
    let b = rows.iter().map(|r| r.bytes).sum::<u64>() as f64;
    let size = if b < 1e6 {
        format!("{:.0} KB", (b / 1e3).ceil())
    } else if b < 1e9 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.1} GB", b / 1e9)
    };
    format!("{} saved, {size}", rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_shows_count_and_total_size() {
        let row = |bytes| Row {
            id: String::new(),
            name: String::new(),
            detail: String::new(),
            starred: false,
            active: false,
            bytes,
        };
        assert_eq!(count_text(&[row(400)]), "1 saved, 1 KB");
        assert_eq!(
            count_text(&[row(1_000_000), row(500_000)]),
            "2 saved, 1.5 MB"
        );
        assert_eq!(count_text(&[row(2_300_000_000)]), "1 saved, 2.3 GB");
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::{close, is_open, show};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::{close, is_open, show};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::{close, is_open, show};
