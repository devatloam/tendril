//! The Settings window (ui/settings.slint): every setting, with search engines and quicklinks
//! editable too. Made when it's opened and dropped when it's closed. Changes are saved to
//! config.toml straight away and applied to the launcher through App, the same way the
//! launcher's Appearance view applies them.

use crate::app::with_app;
use crate::{Choice, ChoiceGroup, Entry, Look, SettingsWindow, Stored, Theme, appearance, config, hotkey, palettes, system, update};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set when a change from this window restarts Tendril, so the next one opens it again.
pub static REOPEN_AFTER_RESTART: AtomicBool = AtomicBool::new(false);

type Groups = &'static [(&'static str, &'static [&'static str])];

const GENERAL: Groups = &[
    ("Starting", &["login", "hide_on_blur"]),
    ("Clipboard and web", &["clipboard_history", "site_icons"]),
    ("Updates", &["updates", "previews"]),
];
const LOOK: Groups = &[
    ("Colours", &["theme"]),
    ("Style", &["style", "selection", "icons"]),
    ("See-through", &["blur", "blur_style", "opacity"]),
    ("Touches", &["glow", "gradient_divider"]),
];
const LAYOUT: Groups = &[("Size", &["size", "details"]), ("Before you type", &["favourites", "quick_keys", "headings"])];
const FILES: Groups = &[("Search", &["index", "hidden", "gitignore"]), ("Before you type", &["recent"])];

thread_local! {
    static WINDOW: RefCell<Option<SettingsWindow>> = const { RefCell::new(None) };
    /// The lists as they're being edited; written to config.toml on every change.
    static ENGINES: RefCell<Vec<config::Engine>> = const { RefCell::new(Vec::new()) };
    static LINKS: RefCell<Vec<config::Quicklink>> = const { RefCell::new(Vec::new()) };
    static SNIPPETS: RefCell<Vec<config::Snippet>> = const { RefCell::new(Vec::new()) };
    static ALIASES: RefCell<Vec<config::Alias>> = const { RefCell::new(Vec::new()) };
    /// File search's folders and patterns, as they're being edited.
    static FILE_LISTS: RefCell<FileLists> = const { RefCell::new(FileLists { folders: Vec::new(), include: Vec::new(), exclude: Vec::new(), exclude_regex: Vec::new() }) };
    /// A change waiting for "Turn On" in the GPU question.
    static PENDING: Cell<Option<(&'static str, i32)>> = const { Cell::new(None) };
    /// Something kept, waiting for "Delete" in the confirm dialog.
    static PENDING_DELETE: Cell<Option<&'static str>> = const { Cell::new(None) };
}

struct FileLists {
    folders: Vec<String>,
    include: Vec<String>,
    exclude: Vec<String>,
    exclude_regex: Vec<String>,
}

impl FileLists {
    fn get(&mut self, name: &str) -> Option<&mut Vec<String>> {
        match name {
            "folders" => Some(&mut self.folders),
            "include" => Some(&mut self.include),
            "exclude" => Some(&mut self.exclude),
            "exclude_regex" => Some(&mut self.exclude_regex),
            _ => None,
        }
    }
}

/// Where the window was and how big, when it was last closed: "x y width height", in pixels.
fn place_path() -> std::path::PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("tendril").join("settings-window")
}

fn saved_place() -> Option<(i32, i32, u32, u32)> {
    let text = std::fs::read_to_string(place_path()).ok()?;
    let mut n = text.split_whitespace();
    Some((n.next()?.parse().ok()?, n.next()?.parse().ok()?, n.next()?.parse().ok()?, n.next()?.parse().ok()?))
}

fn save_place(w: &SettingsWindow) {
    let (at, size) = (w.window().position(), w.window().size());
    if size.width > 0 && size.height > 0 {
        let _ = std::fs::write(place_path(), format!("{} {} {} {}", at.x, at.y, size.width, size.height));
    }
}

/// Opens the window at a tab (0 General … 8 About), or brings it forward if it's open.
pub fn open(tab: i32) {
    let mut fresh = false;
    let made = WINDOW.with_borrow_mut(|slot| {
        if slot.is_none() {
            match crate::window::plain(SettingsWindow::new) {
                Ok(w) => {
                    let (frameless, corner, inset) = crate::window::settings_frame();
                    w.set_frameless(frameless);
                    w.set_corner(corner);
                    w.set_top_inset(inset);
                    wire(&w);
                    *slot = Some(w);
                    fresh = true;
                }
                Err(e) => {
                    eprintln!("tendril: couldn't open settings: {e}");
                    return false;
                }
            }
        }
        true
    });
    if !made {
        return;
    }
    let place = if fresh { saved_place() } else { None };
    with_window(|w| {
        w.set_tab(tab);
        fill(w);
        // The size it was left at; the place is set once the system has made the window.
        if let Some((_, _, width, height)) = place {
            w.window().set_size(slint::PhysicalSize::new(width, height));
        }
        if let Err(e) = w.show() {
            eprintln!("tendril: couldn't show settings: {e}");
        }
        let weak = w.as_weak();
        let _ = slint::spawn_local(async move {
            if let Some(w) = weak.upgrade()
                && let Ok(winit) = w.window().winit_window().await
            {
                use slint::winit_030::winit::dpi::PhysicalPosition;
                // Where it was left, if that's still on a screen (one may have been unplugged).
                let on_screen = |x: i32, y: i32| {
                    winit.available_monitors().any(|m| {
                        let (at, size) = (m.position(), m.size());
                        x + 40 >= at.x && x + 40 < at.x + size.width as i32 && y >= at.y && y + 20 < at.y + size.height as i32
                    })
                };
                if let Some((x, y, _, _)) = place
                    && on_screen(x, y)
                {
                    winit.set_outer_position(PhysicalPosition::new(x, y));
                } else if fresh && let Some(monitor) = winit.current_monitor().or_else(|| winit.primary_monitor()) {
                    // In the middle of the screen, where the system might put it anywhere. One
                    // already open stays where it is.
                    let (at, screen, size) = (monitor.position(), monitor.size(), winit.outer_size());
                    let x = at.x + (screen.width as i32 - size.width as i32) / 2;
                    let y = at.y + (screen.height as i32 - size.height as i32) / 2;
                    winit.set_outer_position(PhysicalPosition::new(x, y));
                }
                crate::window::round(&winit);
                #[cfg(windows)]
                if w.global::<Look>().get_mica() {
                    let _ = window_vibrancy::apply_mica(&*winit, Some(w.global::<Theme>().get_dark()));
                }
                winit.focus_window();
            }
        });
    });
}

/// Shows the setting rows as they are now, if the window is open (file search's status changed).
/// The lists being edited are left alone.
pub fn refresh() {
    with_window(|w| choices(w, &config::load()));
}

/// Only file search's status line: a few times a second while indexing.
pub fn show_files_status() {
    with_window(|w| w.set_files_status(crate::files::status().into()));
}

fn with_window(f: impl FnOnce(&SettingsWindow)) {
    let w = WINDOW.with_borrow(|slot| slot.as_ref().map(ComponentHandle::clone_strong));
    if let Some(w) = w {
        f(&w);
    }
}

/// Closes the window and lets it go. Later, never from inside one of its own callbacks.
fn close() {
    with_window(|w| {
        if w.get_recording() {
            with_app(|a| a.pause_shortcut(false));
        }
        save_place(w);
        let _ = w.hide();
    });
    slint::Timer::single_shot(std::time::Duration::ZERO, || {
        WINDOW.with_borrow_mut(|slot| *slot = None);
    });
}

/// Everything the window shows, from config.toml as it is now.
fn fill(w: &SettingsWindow) {
    let config = config::load();
    choices(w, &config);

    let keys: Vec<SharedString> = hotkey::keycaps(&config.hotkey).into_iter().map(Into::into).collect();
    w.set_shortcut_keys(ModelRc::new(VecModel::from(keys)));
    let mut status = (String::new(), true);
    with_app(|a| status = a.shortcut_status());
    w.set_shortcut_note(status.0.into());
    w.set_shortcut_ok(status.1);

    ENGINES.with_borrow_mut(|e| *e = config.search.clone());
    LINKS.with_borrow_mut(|l| *l = config.quicklink.clone());
    // Shown on one line: a new line written in the file shows as \n.
    SNIPPETS.with_borrow_mut(|s| {
        *s = config.snippet.iter().map(|s| config::Snippet { text: s.text.replace('\n', "\\n"), ..s.clone() }).collect()
    });
    ALIASES.with_borrow_mut(|a| *a = config.alias.clone());
    let f = &config.files;
    FILE_LISTS.with_borrow_mut(|l| {
        *l = FileLists {
            folders: f.folders.clone(),
            include: f.include.clone(),
            exclude: f.exclude.clone(),
            exclude_regex: f.exclude_regex.clone(),
        }
    });
    show_lists(w);
    show_stored(w);
    w.set_version(update::VERSION.into());
    theme(w);
}

/// Privacy & Data: what's kept, where, and how big.
fn show_stored(w: &SettingsWindow) {
    let stored: Vec<Stored> = crate::data::kept()
        .into_iter()
        .map(|k| Stored {
            key: k.key.into(),
            title: k.title.into(),
            about: k.about.into(),
            path: k.path.to_string_lossy().as_ref().into(),
            size: crate::data::human(crate::data::size(&k.path)).into(),
            action: k.action.unwrap_or_default().into(),
        })
        .collect();
    w.set_stored(ModelRc::new(VecModel::from(stored)));
    let ranking = crate::usage::path();
    w.set_ranking_path(ranking.to_string_lossy().as_ref().into());
    w.set_ranking_size(crate::data::human(crate::data::size(&ranking)).into());
}

/// Where a kept thing is, for Show.
fn stored_path(key: &str) -> Option<std::path::PathBuf> {
    if key == "ranking" {
        return Some(crate::usage::path());
    }
    crate::data::kept().into_iter().find(|k| k.key == key).map(|k| k.path)
}

/// Asks before deleting something kept; `answer` does it.
fn ask_delete(key: &str) {
    let (key, title, message, label): (&'static str, &str, &str, &str) = match key {
        "ranking" => (
            "ranking",
            "Reset ranking?",
            "Tendril forgets what you've opened and what you picked for each search, so results go back to being ordered by name. Your favourites stay pinned.",
            "Reset",
        ),
        "clipboard" => {
            ("clipboard", "Clear clipboard history?", "Everything you've copied is deleted from Tendril. This can't be undone.", "Clear")
        }
        "files" => (
            "files",
            "Delete the file index?",
            "File search finds nothing until Tendril looks through your folders again, the next time it opens.",
            "Delete",
        ),
        "apps" => {
            ("apps", "Delete the app list?", "Your apps are found again on the next start. Until then the list stays as it is.", "Delete")
        }
        "icons" => ("icons", "Delete website icons?", "Quicklinks show a plain icon until theirs are fetched again.", "Delete"),
        "all" => (
            "all",
            "Delete everything but settings?",
            "Ranking, clipboard history, the file index, the app list and website icons are deleted. Your settings and favourites stay.",
            "Delete",
        ),
        _ => return,
    };
    with_window(|w| {
        w.set_confirm_title(title.into());
        w.set_confirm_message(message.into());
        w.set_confirm_label(label.into());
        w.set_confirm_danger(true);
        let mut icons = crate::icons::Icons::default();
        w.set_confirm_icon(icons.builtin_solid("eraser"));
        w.set_confirm_open(true);
    });
    PENDING_DELETE.set(Some(key));
}

/// The setting rows on the General, Look and Layout tabs.
fn choices(w: &SettingsWindow, config: &config::Config) {
    let all = appearance::settings(config);
    let groups = |groups: Groups| -> ModelRc<ChoiceGroup> {
        let made: Vec<ChoiceGroup> = groups
            .iter()
            .filter_map(|(title, keys)| {
                let choices: Vec<Choice> = keys
                    .iter()
                    .filter_map(|k| all.iter().find(|s| s.key == *k))
                    .map(|s| {
                        let labels: Vec<SharedString> = s.choices.iter().map(|(l, _)| l.as_str().into()).collect();
                        let toggle = labels.len() == 2 && labels[0] == "On" && labels[1] == "Off";
                        Choice {
                            key: s.key.into(),
                            title: s.title.into(),
                            about: s.about.as_str().into(),
                            labels: ModelRc::new(VecModel::from(labels)),
                            current: s.current as i32,
                            toggle,
                        }
                    })
                    .collect();
                (!choices.is_empty()).then(|| ChoiceGroup { title: (*title).into(), choices: ModelRc::new(VecModel::from(choices)) })
            })
            .collect();
        ModelRc::new(VecModel::from(made))
    };
    w.set_general(groups(GENERAL));
    w.set_look(groups(LOOK));
    w.set_layout(groups(LAYOUT));
    w.set_files(groups(FILES));
    w.set_files_status(crate::files::status().into());
}

fn show_lists(w: &SettingsWindow) {
    let engines: Vec<Entry> = ENGINES.with_borrow(|e| {
        e.iter().map(|e| Entry { name: e.name.as_str().into(), keyword: SharedString::new(), url: e.url.as_str().into() }).collect()
    });
    let links: Vec<Entry> = LINKS.with_borrow(|l| {
        l.iter().map(|l| Entry { name: l.name.as_str().into(), keyword: l.keyword.as_str().into(), url: l.url.as_str().into() }).collect()
    });
    w.set_engines(ModelRc::new(VecModel::from(engines)));
    w.set_links(ModelRc::new(VecModel::from(links)));
    let snippets: Vec<Entry> = SNIPPETS.with_borrow(|s| {
        s.iter().map(|s| Entry { name: s.name.as_str().into(), keyword: s.keyword.as_str().into(), url: s.text.as_str().into() }).collect()
    });
    w.set_snippets(ModelRc::new(VecModel::from(snippets)));
    let aliases: Vec<Entry> = ALIASES.with_borrow(|a| {
        a.iter()
            .map(|a| Entry {
                name: if a.name.is_empty() { a.id.as_str().into() } else { a.name.as_str().into() },
                keyword: a.word.as_str().into(),
                url: a.id.as_str().into(),
            })
            .collect()
    });
    w.set_aliases(ModelRc::new(VecModel::from(aliases)));
    let model = |list: &[String]| -> ModelRc<SharedString> {
        ModelRc::new(VecModel::from(list.iter().map(|s| s.as_str().into()).collect::<Vec<SharedString>>()))
    };
    FILE_LISTS.with_borrow(|l| {
        w.set_folders(model(&l.folders));
        w.set_include(model(&l.include));
        w.set_exclude(model(&l.exclude));
        w.set_exclude_regex(model(&l.exclude_regex));
    });
}

/// Saves one of file search's lists, leaving out entries not filled in yet. The launcher looks
/// through the folders again the next time it opens, or now with Index Now.
fn save_file_list(name: &'static str) {
    let list = FILE_LISTS
        .with_borrow_mut(|l| l.get(name).map(|v| v.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>()));
    let Some(list) = list else { return };
    if let Err(e) = config::set(Some("files"), name, config::Value::List(&list)) {
        with_window(|w| w.set_files_status(format!("Couldn't save: {e}").into()));
    }
    with_app(|a| a.settings_changed());
}

fn list_name(name: &str) -> Option<&'static str> {
    ["folders", "include", "exclude", "exclude_regex"].into_iter().find(|n| *n == name)
}

/// The same colours as the launcher.
fn theme(w: &SettingsWindow) {
    let mut palette = &palettes::PALETTES[0];
    let mut flags = (false, false, false);
    with_app(|a| {
        palette = a.palette();
        let c = a.config.borrow();
        flags = (c.appearance.selection == "blue", c.appearance.icons == "bold", false);
    });
    let t = w.global::<Theme>();
    palettes::apply(&t, palette);
    t.set_blue_selection(flags.0);
    t.set_bold_icons(flags.1);
    if crate::fluent::on() {
        let (on_dark, on_light) = crate::fluent::apply(&t);
        let look = w.global::<Look>();
        look.set_fluent(true);
        // Mica needs a see-through window, which only the GPU renderer draws on Windows.
        look.set_mica(cfg!(windows) && crate::window::is_gpu());
        look.set_accent_on_dark(on_dark);
        look.set_accent_on_light(on_light);
    }
}

fn wire(w: &SettingsWindow) {
    w.on_chosen(|key, index| choose(&key, index));
    w.on_dismiss(close);
    // Its own title bar, where the window has no frame from the system.
    w.on_drag(|| {
        with_window(|w| {
            let _ = w.window().with_winit_window(|winit| winit.drag_window());
        })
    });
    w.on_minimize(|| {
        with_window(|w| {
            let _ = w.window().with_winit_window(|winit| winit.set_minimized(true));
        })
    });
    w.window().on_close_requested(|| {
        close();
        slint::CloseRequestResponse::KeepWindowShown
    });
    w.on_start_recording(|| {
        with_app(|a| a.pause_shortcut(true));
        with_window(|w| w.set_recording(true));
    });
    w.on_stop_recording(|| {
        with_app(|a| a.pause_shortcut(false));
        with_window(|w| w.set_recording(false));
    });
    w.on_record(|text, control, alt, shift, meta| {
        let Some(combo) = hotkey::shortcut(&text, control, alt, shift, meta) else { return false };
        let mut taken = Err(String::new());
        with_app(|a| taken = a.take_shortcut(&combo));
        with_window(|w| match taken {
            Ok(()) => {
                with_app(|a| a.pause_shortcut(false));
                w.set_recording(false);
                fill(w);
                w.set_shortcut_note("Saved".into());
            }
            Err(e) => {
                w.set_shortcut_note(format!("Not those: {e}. Try others.").into());
                w.set_shortcut_ok(false);
            }
        });
        true
    });
    w.on_engine_edited(|i, field, text| {
        ENGINES.with_borrow_mut(|engines| {
            if let Some(e) = engines.get_mut(i as usize) {
                if field == "name" { e.name = text.to_string() } else { e.url = text.to_string() }
            }
        });
        save_engines();
    });
    w.on_engine_add(|| {
        ENGINES.with_borrow_mut(|e| e.push(config::Engine { name: String::new(), url: String::new() }));
        with_window(show_lists);
    });
    w.on_engine_remove(|i| {
        ENGINES.with_borrow_mut(|e| {
            if (i as usize) < e.len() {
                e.remove(i as usize);
            }
        });
        save_engines();
        with_window(show_lists);
    });
    w.on_link_edited(|i, field, text| {
        LINKS.with_borrow_mut(|links| {
            if let Some(l) = links.get_mut(i as usize) {
                match field.as_str() {
                    "name" => l.name = text.to_string(),
                    "keyword" => l.keyword = text.to_string(),
                    _ => l.url = text.to_string(),
                }
            }
        });
        save_links();
    });
    w.on_link_add(|| {
        LINKS.with_borrow_mut(|l| l.push(config::Quicklink { name: String::new(), keyword: String::new(), url: String::new() }));
        with_window(show_lists);
    });
    w.on_link_remove(|i| {
        LINKS.with_borrow_mut(|l| {
            if (i as usize) < l.len() {
                l.remove(i as usize);
            }
        });
        save_links();
        with_window(show_lists);
    });
    w.on_snippet_edited(|i, field, text| {
        SNIPPETS.with_borrow_mut(|snippets| {
            if let Some(s) = snippets.get_mut(i as usize) {
                match field.as_str() {
                    "name" => s.name = text.to_string(),
                    "keyword" => s.keyword = text.to_string(),
                    _ => s.text = text.to_string(),
                }
            }
        });
        save_snippets();
    });
    w.on_snippet_add(|| {
        SNIPPETS.with_borrow_mut(|s| s.push(config::Snippet { name: String::new(), keyword: String::new(), text: String::new() }));
        with_window(show_lists);
    });
    w.on_snippet_remove(|i| {
        SNIPPETS.with_borrow_mut(|s| {
            if (i as usize) < s.len() {
                s.remove(i as usize);
            }
        });
        save_snippets();
        with_window(show_lists);
    });
    w.on_alias_edited(|i, word| {
        ALIASES.with_borrow_mut(|a| {
            if let Some(a) = a.get_mut(i as usize) {
                a.word = word.trim().to_lowercase();
            }
        });
        save_aliases();
    });
    w.on_alias_remove(|i| {
        ALIASES.with_borrow_mut(|a| {
            if (i as usize) < a.len() {
                a.remove(i as usize);
            }
        });
        save_aliases();
        with_window(show_lists);
    });
    w.on_list_edited(|name, i, text| {
        let Some(name) = list_name(&name) else { return };
        FILE_LISTS.with_borrow_mut(|l| {
            if let Some(entry) = l.get(name).and_then(|v| v.get_mut(i as usize)) {
                *entry = text.to_string();
            }
        });
        save_file_list(name);
    });
    w.on_list_add(|name| {
        let Some(name) = list_name(&name) else { return };
        FILE_LISTS.with_borrow_mut(|l| {
            if let Some(v) = l.get(name) {
                v.push(String::new());
            }
        });
        with_window(show_lists);
    });
    w.on_list_remove(|name, i| {
        let Some(name) = list_name(&name) else { return };
        FILE_LISTS.with_borrow_mut(|l| {
            if let Some(v) = l.get(name)
                && (i as usize) < v.len()
            {
                v.remove(i as usize);
            }
        });
        save_file_list(name);
        with_window(show_lists);
    });
    w.on_pick_folder(|| {
        let owner = handle();
        let _ = std::thread::Builder::new().name("folder-picker".into()).spawn(move || {
            let Some(path) = system::pick_folder(owner) else { return };
            let _ = slint::invoke_from_event_loop(move || {
                let shown = crate::paths::shorten_home(&path);
                let added = FILE_LISTS.with_borrow_mut(|l| {
                    let folders = &mut l.folders;
                    folders.retain(|f| !f.trim().is_empty());
                    let new = !folders.iter().any(|f| crate::paths::expand(f) == path);
                    if new {
                        folders.push(shown);
                    }
                    new
                });
                if added {
                    save_file_list("folders");
                }
                with_window(show_lists);
            });
        });
    });
    w.on_index_now(|| {
        with_app(|a| a.index_files(true));
        show_files_status();
    });
    w.on_check_updates(check_updates);
    w.on_open_file(|| {
        if let Err(e) = system::open(&config::path().to_string_lossy()) {
            with_window(|w| w.set_update_note(format!("Couldn't open it: {e}").into()));
        }
    });
    w.on_answer(|yes| {
        with_window(|w| w.set_confirm_open(false));
        if let Some((key, delta)) = PENDING.take()
            && yes
        {
            apply(key, delta);
        }
        if let Some(key) = PENDING_DELETE.take()
            && yes
        {
            let mut result = Err(String::from("Tendril isn't running"));
            with_app(|a| result = a.delete_data(key));
            with_window(|w| {
                w.set_data_note(result.unwrap_or_else(|e| format!("Couldn't delete it: {e}")).into());
                show_stored(w);
                w.set_files_status(crate::files::status().into());
            });
        }
    });
    w.on_data_show(|key| {
        let Some(path) = stored_path(&key) else { return };
        // A file is shown selected in its folder; a folder (or a file not made yet) is opened.
        let shown = if path.is_file() {
            system::reveal(&path)
        } else {
            let folder = if path.is_dir() { path } else { path.parent().map(|p| p.to_path_buf()).unwrap_or(path) };
            let _ = std::fs::create_dir_all(&folder);
            system::open(&folder.to_string_lossy())
        };
        if let Err(e) = shown {
            with_window(|w| w.set_data_note(format!("Couldn't show it: {e}").into()));
        }
    });
    w.on_data_delete(|key| ask_delete(&key));
    w.on_resize(|dir| {
        use slint::winit_030::winit::window::ResizeDirection as R;
        let direction = match dir.as_str() {
            "n" => R::North,
            "s" => R::South,
            "e" => R::East,
            "w" => R::West,
            "ne" => R::NorthEast,
            "nw" => R::NorthWest,
            "se" => R::SouthEast,
            _ => R::SouthWest,
        };
        with_window(|w| {
            let _ = w.window().with_winit_window(|winit| winit.drag_resize_window(direction));
        });
    });
}

/// A setting set to its `index`th choice, asking first when that means drawing with the GPU.
fn choose(key: &str, index: i32) {
    let config = config::load();
    let Some(setting) = appearance::settings(&config).into_iter().find(|s| s.key == key) else { return };
    let delta = index - setting.current as i32;
    if delta == 0 {
        return;
    }
    let key = setting.key;
    let mut question = None;
    with_app(|a| question = a.gpu_question(key, delta));
    if let Some((message, icon)) = question {
        with_window(|w| {
            w.set_confirm_title("Draw with the GPU?".into());
            w.set_confirm_message(message.into());
            w.set_confirm_label("Turn On".into());
            w.set_confirm_danger(false);
            let mut icons = crate::icons::Icons::default();
            w.set_confirm_icon(icons.builtin_solid(icon));
            w.set_confirm_open(true);
        });
        PENDING.set(Some((key, delta)));
        return;
    }
    apply(key, delta);
}

fn apply(key: &'static str, delta: i32) {
    REOPEN_AFTER_RESTART.store(true, Ordering::SeqCst);
    with_app(|a| a.apply_setting(key, delta));
    // Not restarting after all: nothing to reopen.
    if !crate::app::RESTART.load(Ordering::SeqCst) {
        REOPEN_AFTER_RESTART.store(false, Ordering::SeqCst);
    }
    with_window(fill);
}

/// Saves the engines that have both a name and an address; half-typed ones wait until they do.
fn save_engines() {
    let engines = ENGINES.with_borrow(|e| e.clone());
    let entries: Vec<Vec<(&str, config::Value)>> = engines
        .iter()
        .filter(|e| !e.name.trim().is_empty() && !e.url.trim().is_empty())
        .map(|e| vec![("name", config::Value::Text(e.name.trim())), ("url", config::Value::Text(e.url.trim()))])
        .collect();
    if let Err(e) = config::set_list("search", &entries) {
        with_window(|w| w.set_update_note(format!("Couldn't save: {e}").into()));
    }
    with_app(|a| a.settings_changed());
}

fn save_links() {
    let links = LINKS.with_borrow(|l| l.clone());
    let entries: Vec<Vec<(&str, config::Value)>> = links
        .iter()
        .filter(|l| !l.name.trim().is_empty() && !l.url.trim().is_empty())
        .map(|l| {
            let mut entry = vec![("name", config::Value::Text(l.name.trim()))];
            if !l.keyword.trim().is_empty() {
                entry.push(("keyword", config::Value::Text(l.keyword.trim())));
            }
            entry.push(("url", config::Value::Text(l.url.trim())));
            entry
        })
        .collect();
    if let Err(e) = config::set_list("quicklink", &entries) {
        with_window(|w| w.set_update_note(format!("Couldn't save: {e}").into()));
    }
    with_app(|a| a.settings_changed());
}

/// The window's handle, so the folder picker opens in front of it (Windows); 0 elsewhere.
#[cfg(windows)]
fn handle() -> isize {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let mut owner = 0;
    with_window(|w| {
        let _ = w.window().with_winit_window(|winit| {
            if let Ok(handle) = winit.window_handle()
                && let RawWindowHandle::Win32(h) = handle.as_raw()
            {
                owner = h.hwnd.get();
            }
        });
    });
    owner
}

#[cfg(not(windows))]
fn handle() -> isize {
    0
}

/// Saves the aliases that have a word, one word each; one being retyped waits until it has one.
fn save_aliases() {
    let aliases = ALIASES.with_borrow(|a| a.clone());
    let entries: Vec<Vec<(&str, config::Value)>> = aliases
        .iter()
        .filter(|a| !a.word.is_empty() && !a.word.contains(char::is_whitespace))
        .map(|a| vec![("word", config::Value::Text(&a.word)), ("id", config::Value::Text(&a.id)), ("name", config::Value::Text(&a.name))])
        .collect();
    if let Err(e) = config::set_list("alias", &entries) {
        with_window(|w| w.set_update_note(format!("Couldn't save: {e}").into()));
    }
    with_app(|a| a.settings_changed());
}

/// Saves the snippets that have a name and some text; half-typed ones wait until they do.
fn save_snippets() {
    let snippets = SNIPPETS.with_borrow(|s| s.clone());
    let entries: Vec<Vec<(&str, config::Value)>> = snippets
        .iter()
        .filter(|s| !s.name.trim().is_empty() && !s.text.is_empty())
        .map(|s| {
            let mut entry = vec![("name", config::Value::Text(s.name.trim()))];
            if !s.keyword.trim().is_empty() {
                entry.push(("keyword", config::Value::Text(s.keyword.trim())));
            }
            entry.push(("text", config::Value::Text(&s.text)));
            entry
        })
        .collect();
    if let Err(e) = config::set_list("snippet", &entries) {
        with_window(|w| w.set_update_note(format!("Couldn't save: {e}").into()));
    }
    with_app(|a| a.settings_changed());
}

/// "Check for Updates" in About: asks GitHub and says what came of it here; the launcher then
/// installs or offers it as its Updates setting says.
fn check_updates() {
    with_window(|w| w.set_update_note("Checking…".into()));
    let _ = std::thread::Builder::new().name("settings-update".into()).spawn(|| {
        let result = update::check();
        let _ = slint::invoke_from_event_loop(move || {
            let note = match &result {
                Ok(found) => match &found.release {
                    Some(r) if update::newer(&r.version, update::VERSION) => {
                        format!("Tendril {} is out. It's on its way, as your Updates setting says.", r.version)
                    }
                    _ => format!("You're on the latest version ({}).", update::VERSION),
                },
                Err(e) => format!("Couldn't check: {e}"),
            };
            with_window(|w| w.set_update_note(note.into()));
            with_app(|a| a.update_checked(result, false));
        });
    });
}
