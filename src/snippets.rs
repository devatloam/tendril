//! Snippets: text you keep in config.toml ([[snippet]]) and paste by finding it by name or
//! keyword. {date}, {time} and {clipboard} are filled in as it's pasted, and \n starts a line.

use crate::catalog::{Icon, Item, Kind, Run};
use crate::config::Snippet;

/// Your snippets as things to find.
pub fn items(snippets: &[Snippet]) -> Vec<Item> {
    snippets
        .iter()
        .filter(|s| !s.name.trim().is_empty())
        .map(|s| Item {
            id: format!("snippet:{}", s.name),
            title: s.name.clone(),
            subtitle: preview(&s.text),
            keywords: format!("{} snippet", s.keyword),
            kind: Kind::Snippet,
            icon: Icon::Builtin("copy"),
            tint: 7,
            run: Run::Snippet(s.text.clone()),
            path: None,
        })
        .collect()
}

/// The text on one line, shortened, for under the name.
fn preview(text: &str) -> String {
    let flat = text.replace("\\n", " ").replace('\n', " ");
    let flat = flat.trim();
    if flat.chars().count() > 70 { format!("{}…", flat.chars().take(69).collect::<String>()) } else { flat.to_string() }
}

/// The snippet as it's pasted.
pub fn expand(text: &str, clipboard: Option<&str>) -> String {
    let now = now();
    let mut out = text.replace("\\n", "\n").replace("{date}", &now.date()).replace("{time}", &now.time());
    if out.contains("{clipboard}") {
        out = out.replace("{clipboard}", clipboard.unwrap_or_default());
    }
    out
}

/// The local date and time.
struct Now {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
}

impl Now {
    fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    fn time(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

#[cfg(windows)]
fn now() -> Now {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    Now { year: t.wYear as i32, month: t.wMonth as u32, day: t.wDay as u32, hour: t.wHour as u32, minute: t.wMinute as u32 }
}

#[cfg(unix)]
fn now() -> Now {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as libc::time_t;
    unsafe { libc::localtime_r(&secs, &mut tm) };
    Now { year: tm.tm_year + 1900, month: tm.tm_mon as u32 + 1, day: tm.tm_mday as u32, hour: tm.tm_hour as u32, minute: tm.tm_min as u32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_in() {
        let out = expand("Hi {clipboard},\\nsent {date} at {time}", Some("Sam"));
        let mut lines = out.lines();
        assert_eq!(lines.next(), Some("Hi Sam,"));
        let second = lines.next().unwrap();
        // "sent 2026-11-16 at 09:05"
        assert_eq!(second.len(), "sent 2026-11-16 at 09:05".len(), "{second}");
        assert!(second.as_bytes()[9] == b'-' && second.as_bytes()[21] == b':', "{second}");
        assert_eq!(expand("{clipboard}", None), "");
        assert_eq!(preview("a\\nb"), "a b");
    }
}
