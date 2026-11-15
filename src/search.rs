//! Turns what you typed into the list: the calculator, matches best first, then the fallbacks.

use crate::calc;
use crate::catalog::{Icon, Item, Kind, Run};
use crate::config::{Engine, Quicklink};
use crate::files::Index;
use crate::usage::Usage;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};

pub enum Line {
    Section(String),
    Item(Item),
    /// Nothing matched: said over the shapes, above the web searches.
    Empty(String),
    /// What you use most, shown before you type, as tiles or a list.
    Favourites(Vec<Item>),
    /// A big welcome, with the shapes: the walkthrough's first and last steps.
    Hero(String, String),
}

const MAX_RESULTS: usize = 40;
const SUGGESTIONS: usize = 6;

pub fn lines(
    query: &str,
    catalog: &[Item],
    files: &Index,
    usage: &Usage,
    engines: &[Engine],
    links: &[Quicklink],
    matcher: &mut Matcher,
) -> Vec<Line> {
    if crate::paths::is_path(query) {
        let found = crate::paths::entries(query);
        if found.is_empty() {
            return vec![Line::Empty("Nothing there matches".into())];
        }
        return std::iter::once(Line::Section("Files".into())).chain(found.into_iter().map(Line::Item)).collect();
    }
    let query = query.trim();
    if query.is_empty() {
        return root(catalog, usage);
    }
    let mut lines = Vec::new();
    let mut keyword_used = false;

    // A quicklink's keyword and some text: that link, filled in, first.
    if let Some((word, rest)) = query.split_once(' ')
        && !rest.trim().is_empty()
        && let Some(link) = links.iter().find(|l| l.takes_text() && crate::catalog::keyword_of(l) == word.to_lowercase())
    {
        let rest = rest.trim();
        let filled = fill(&link.url, rest);
        keyword_used = true;
        lines.push(Line::Section("Quicklink".into()));
        lines.push(Line::Item(Item {
            id: format!("link:{}", link.name),
            title: format!("{}: {}", link.name, shorten(rest, 50)),
            subtitle: shorten(&filled, 60),
            keywords: String::new(),
            kind: Kind::Link,
            icon: Icon::Builtin("link"),
            tint: 7,
            run: crate::catalog::link_target(&filled),
            path: None,
        }));
    }

    if let Some(answer) = calc::evaluate(query) {
        lines.push(Line::Section("Calculator".into()));
        lines.push(Line::Item(Item {
            id: "calc".into(),
            title: query.into(),
            subtitle: answer.shown,
            keywords: "Answer".into(),
            kind: Kind::Calculator,
            icon: Icon::Builtin("calculator"),
            tint: 7,
            run: Run::Copy(answer.plain),
            path: None,
        }));
    }

    let matches = ranked(query, catalog, usage, matcher);
    let found = files.find(query);
    if !matches.is_empty() {
        lines.push(Line::Section("Results".into()));
        lines.extend(matches.into_iter().map(|i| Line::Item(i.clone())));
    } else if lines.is_empty() && found.is_empty() {
        lines.push(Line::Empty(format!("No apps or commands match “{}”", shorten(query, 40))));
    }
    if !found.is_empty() {
        lines.push(Line::Section("Files".into()));
        lines.extend(found.into_iter().map(Line::Item));
    }

    lines.push(Line::Section(format!("Use “{}” with…", shorten(query, 40))));
    if let Some(url) = as_url(query) {
        lines.push(Line::Item(fallback("open-url", "Open URL", query, "link", 7, url)));
    }
    for engine in engines {
        let url = engine.url.replace("{query}", &encode(query));
        let title = format!("Search {}", engine.name);
        lines.push(Line::Item(fallback(&engine.name, &title, query, "globe", 7, url)));
    }
    for link in links.iter().filter(|l| l.takes_text() && !keyword_used) {
        let target = crate::catalog::link_target(&fill(&link.url, query));
        let mut item = fallback(&link.name, &format!("Open {}", link.name), query, "link", 7, String::new());
        item.run = target;
        lines.push(Line::Item(item));
    }
    lines
}

/// A quicklink's address with the text put in: {query} made safe for a URL, {raw} as typed.
fn fill(url: &str, text: &str) -> String {
    url.replace("{query}", &encode(text)).replace("{raw}", text)
}

/// The list before you type: what you use most, then every command.
fn root(catalog: &[Item], usage: &Usage) -> Vec<Line> {
    let mut lines = Vec::new();
    // What you pinned, in your order, then what you use most.
    let pinned = usage.pinned().iter().filter_map(|id| catalog.iter().find(|i| &i.id == id));
    let used = usage.top(SUGGESTIONS).into_iter().filter_map(|id| catalog.iter().find(|i| i.id == id));
    let mut top: Vec<&Item> = Vec::new();
    for item in pinned.chain(used) {
        if top.len() < SUGGESTIONS.max(usage.pinned().len()) && !top.iter().any(|t| t.id == item.id) {
            top.push(item);
        }
    }
    if !top.is_empty() {
        lines.push(Line::Favourites(top.iter().map(|i| (*i).clone()).collect()));
    }
    lines.push(Line::Section("Commands".into()));
    lines.extend(
        catalog
            .iter()
            .filter(|i| {
                i.kind == Kind::Command && !matches!(i.run, Run::Theme(_) | Run::Setting(_)) && !i.id.starts_with("cmd:ms-settings")
            })
            .filter(|i| !top.iter().any(|t| t.id == i.id))
            .map(|i| Line::Item(i.clone())),
    );
    lines
}

fn ranked<'a>(query: &str, catalog: &'a [Item], usage: &Usage, matcher: &mut Matcher) -> Vec<&'a Item> {
    let pattern = Pattern::new(query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
    let lower = query.to_lowercase();
    let mut buf = Vec::new();
    let words: Vec<String> = lower.split_whitespace().map(str::to_string).collect();
    let mut scored: Vec<(u32, &Item)> = catalog
        .iter()
        .filter_map(|item| {
            let name = item.title.to_lowercase();
            // The name is matched loosely ("vsc" finds Visual Studio Code), but only when the
            // letters hang together; descriptions and keywords only by the start of a word,
            // so "shut" doesn't find an app whose blurb has an s, h, u and t somewhere in it.
            let title = pattern
                .score(Utf32Str::new(&item.title, &mut buf), matcher)
                .filter(|&s| s >= 12 * lower.chars().filter(|c| !c.is_whitespace()).count() as u32 || name.contains(&lower));
            let other = format!("{} {}", item.subtitle, item.keywords).to_lowercase();
            let other_words: Vec<&str> = other.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
            let described = words.iter().all(|q| other_words.iter().any(|w| w.starts_with(q.as_str())) || name.contains(q.as_str()));
            let other = described.then(|| 10 * lower.len() as u32);
            let base = title.max(other)?;
            let starts = if name.starts_with(&lower) {
                40
            } else if name.split([' ', '-', '_', '.']).any(|w| w.starts_with(&lower)) {
                20
            } else {
                0
            };
            Some((base + starts + usage.bonus(&item.id, query), item))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.title.len().cmp(&b.1.title.len())).then(a.1.title.cmp(&b.1.title)));
    scored.into_iter().take(MAX_RESULTS).map(|(_, i)| i).collect()
}

fn fallback(id: &str, title: &str, query: &str, icon: &'static str, tint: i32, url: String) -> Item {
    Item {
        id: format!("fallback:{}", id.to_lowercase()),
        title: title.into(),
        subtitle: format!("“{}”", shorten(query, 60)),
        keywords: String::new(),
        kind: Kind::Fallback,
        icon: Icon::Builtin(icon),
        tint,
        run: Run::Url(url),
        path: None,
    }
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max - 1).collect();
    format!("{cut}…")
}

/// `example.com/path` or `https://…` as something a browser opens; anything else isn't a URL.
pub fn as_url(s: &str) -> Option<String> {
    if s.contains(char::is_whitespace) {
        return None;
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        return Some(s.to_string());
    }
    if s.starts_with("localhost:") {
        return Some(format!("http://{s}"));
    }
    let host = s.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once(':').map_or(host, |(h, port)| if port.chars().all(|c| c.is_ascii_digit()) { h } else { host });
    let labels: Vec<&str> = host.split('.').collect();
    let tld = labels.last()?;
    let ok = labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic());
    ok.then(|| format!("https://{s}"))
}

/// Percent-encodes a query for a search URL.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::commands;

    fn titles(query: &str, usage: &Usage) -> Vec<String> {
        let catalog = commands();
        let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
        lines(query, &catalog, &Index::default(), usage, &[], &[], &mut matcher)
            .into_iter()
            .filter_map(|l| match l {
                Line::Item(i) if i.kind == Kind::Command => Some(i.title),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn finds_by_title_and_keyword() {
        let usage = Usage::default();
        assert_eq!(titles("lock", &usage).first().map(String::as_str), Some("Lock Screen"));
        assert_eq!(titles("reboot", &usage).first().map(String::as_str), Some("Restart"));
        assert_eq!(titles("shut", &usage).first().map(String::as_str), Some("Shut Down"));
    }

    #[test]
    fn your_pick_comes_first() {
        let mut usage = Usage::default();
        let before = titles("s", &usage);
        let last = before.last().unwrap().clone();
        assert_ne!(before.first(), Some(&last));
        let id = commands().into_iter().find(|i| i.title == last).unwrap().id;
        usage.record(&id, "s");
        assert_eq!(titles("s", &usage).first(), Some(&last));
    }

    #[test]
    fn descriptions_match_by_word_start() {
        let usage = Usage::default();
        assert!(titles("shut", &usage).iter().all(|t| t == "Shut Down"), "{:?}", titles("shut", &usage));
        assert!(titles("calc", &usage).is_empty(), "{:?}", titles("calc", &usage));
        // Every palette, by its description; on Windows the Settings app's Colors pages come too.
        let colours = titles("colo", &usage);
        for p in crate::palettes::PALETTES {
            assert!(colours.iter().any(|t| t.contains(p.name)), "{} missing from {colours:?}", p.name);
        }
    }

    #[test]
    fn says_when_nothing_matches() {
        let catalog = commands();
        let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
        let lines = lines("zzqx", &catalog, &Index::default(), &Usage::default(), &[], &[], &mut matcher);
        assert!(matches!(lines.first(), Some(Line::Empty(m)) if m.contains("zzqx")));
        let lines = lines_for_sum(&catalog, &mut matcher);
        assert!(!lines.iter().any(|l| matches!(l, Line::Empty(_))), "a sum isn't 'nothing found'");
    }

    fn lines_for_sum(catalog: &[Item], matcher: &mut Matcher) -> Vec<Line> {
        lines("2+2", catalog, &Index::default(), &Usage::default(), &[], &[], matcher)
    }

    #[test]
    fn quicklinks_take_text() {
        let links = vec![crate::config::Quicklink { name: "GitHub".into(), keyword: "gh".into(), url: "https://github.com/{raw}".into() }];
        let catalog = commands();
        let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
        let found = lines("gh devatloam/tendril", &catalog, &Index::default(), &Usage::default(), &[], &links, &mut matcher);
        match &found[1] {
            Line::Item(item) => assert!(matches!(&item.run, Run::Url(u) if u == "https://github.com/devatloam/tendril")),
            _ => panic!("the quicklink comes first"),
        }
        assert_eq!(fill("https://x.test/?q={query}", "a b"), "https://x.test/?q=a+b");
    }

    #[test]
    fn urls() {
        assert_eq!(as_url("example.com").as_deref(), Some("https://example.com"));
        assert_eq!(as_url("github.com/a/b?q=1").as_deref(), Some("https://github.com/a/b?q=1"));
        assert_eq!(as_url("localhost:3000").as_deref(), Some("http://localhost:3000"));
        assert!(as_url("1.5").is_none());
        assert!(as_url("hello world.com").is_none());
        assert!(as_url("firefox").is_none());
    }

    #[test]
    fn encodes() {
        assert_eq!(encode("a b&c"), "a+b%26c");
        assert_eq!(encode("é"), "%C3%A9");
    }
}
