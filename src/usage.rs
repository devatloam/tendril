//! What you open, and what you picked for a query, so the right thing comes first next time.
//! Kept in usage.toml in the data folder. Nothing leaves your computer.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Default)]
pub struct Usage {
    #[serde(default)]
    items: HashMap<String, Use>,
    /// Query → the id you picked for it last.
    #[serde(default)]
    picks: HashMap<String, Pick>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct Use {
    count: u32,
    last: u64,
}

#[derive(Serialize, Deserialize, Clone)]
struct Pick {
    id: String,
    at: u64,
}

const MAX_PICKS: usize = 500;

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn path() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("flint").join("usage.toml")
}

impl Usage {
    pub fn load() -> Usage {
        std::fs::read_to_string(path()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let path = path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = toml::to_string(self) {
            let _ = std::fs::write(path, text);
        }
    }

    pub fn record(&mut self, id: &str, query: &str) {
        let at = now();
        let entry = self.items.entry(id.to_string()).or_insert(Use { count: 0, last: at });
        entry.count = entry.count.saturating_add(1);
        entry.last = at;
        let query = query.trim().to_lowercase();
        if !query.is_empty() {
            self.picks.insert(query, Pick { id: id.to_string(), at });
            if self.picks.len() > MAX_PICKS
                && let Some(oldest) = self.picks.iter().min_by_key(|(_, p)| p.at).map(|(q, _)| q.clone())
            {
                self.picks.remove(&oldest);
            }
        }
    }

    pub fn forget(&mut self, id: &str) {
        self.items.remove(id);
        self.picks.retain(|_, p| p.id != id);
    }

    /// How often and how lately: uses count less as they age.
    fn frecency(&self, id: &str) -> f64 {
        let Some(u) = self.items.get(id) else { return 0.0 };
        let days = now().saturating_sub(u.last) as f64 / 86_400.0;
        let weight = if days < 1.0 {
            1.0
        } else if days < 7.0 {
            0.7
        } else if days < 30.0 {
            0.4
        } else {
            0.2
        };
        u.count as f64 * weight
    }

    /// How many times, and when last (seconds since 1970), something was opened from Flint.
    pub fn stats(&self, id: &str) -> Option<(u32, u64)> {
        self.items.get(id).map(|u| (u.count, u.last))
    }

    /// Points added to a match's score.
    pub fn bonus(&self, id: &str, query: &str) -> u32 {
        let picked = self.picks.get(&query.trim().to_lowercase()).is_some_and(|p| p.id == id);
        let used = ((1.0 + self.frecency(id)).ln() * 20.0).min(70.0) as u32;
        used + if picked { 300 } else { 0 }
    }

    /// The most used ids, best first.
    pub fn top(&self, n: usize) -> Vec<&str> {
        let mut ids: Vec<(&str, f64)> = self.items.keys().map(|id| (id.as_str(), self.frecency(id))).collect();
        ids.sort_by(|a, b| b.1.total_cmp(&a.1));
        ids.into_iter().take(n).map(|(id, _)| id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Usage;

    #[test]
    fn picks_win_their_query() {
        let mut usage = Usage::default();
        usage.record("app:firefox", "f");
        assert!(usage.bonus("app:firefox", "F ") >= 300);
        assert!(usage.bonus("app:files", "f") < 300);
        assert_eq!(usage.top(1), vec!["app:firefox"]);
        usage.forget("app:firefox");
        assert_eq!(usage.bonus("app:firefox", "f"), 0);
    }
}
