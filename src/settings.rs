//! The Settings window (ui/settings.slint): every setting, with search engines and quicklinks
//! editable too. Made when it's opened and dropped when it's closed. Changes are saved to
//! config.toml straight away and applied to the launcher through App, the same way the
//! launcher's Appearance view applies them.

use crate::app::with_app;
use crate::{Choice, ChoiceGroup, Entry, SettingsWindow, Theme, appearance, config, hotkey, palettes, system, update};
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

thread_local! {
    static WINDOW: RefCell<Option<SettingsWindow>> = const { RefCell::new(None) };
    /// The lists as they're being edited; written to config.toml on every change.
    static ENGINES: RefCell<Vec<config::Engine>> = const { RefCell::new(Vec::new()) };
    static LINKS: RefCell<Vec<config::Quicklink>> = const { RefCell::new(Vec::new()) };
    /// A change waiting for "Turn On" in the GPU question.
    static PENDING: Cell<Option<(&'static str, i32)>> = const { Cell::new(None) };
}

/// Opens the window at a tab (0 General … 5 About), or brings it forward if it's open.
pub fn open(tab: i32) {
    let made = WINDOW.with_borrow_mut(|slot| {
        if slot.is_none() {
            match SettingsWindow::new() {
                Ok(w) => {
                    wire(&w);
                    *slot = Some(w);
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
    with_window(|w| {
        w.set_tab(tab);
        fill(w);
        if let Err(e) = w.show() {
            eprintln!("tendril: couldn't show settings: {e}");
        }
        let weak = w.as_weak();
        let _ = slint::spawn_local(async move {
            if let Some(w) = weak.upgrade()
                && let Ok(winit) = w.window().winit_window().await
            {
                // In the middle of the screen, where the system might put it anywhere.
                if let Some(monitor) = winit.current_monitor().or_else(|| winit.primary_monitor()) {
                    let (at, screen, size) = (monitor.position(), monitor.size(), winit.outer_size());
                    let x = at.x + (screen.width as i32 - size.width as i32) / 2;
                    let y = at.y + (screen.height as i32 - size.height as i32) / 2;
                    winit.set_outer_position(slint::winit_030::winit::dpi::PhysicalPosition::new(x, y));
                }
                winit.focus_window();
            }
        });
    });
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
        let _ = w.hide();
    });
    slint::Timer::single_shot(std::time::Duration::ZERO, || {
        WINDOW.with_borrow_mut(|slot| *slot = None);
    });
}

/// Everything the window shows, from config.toml as it is now.
fn fill(w: &SettingsWindow) {
    let config = config::load();
    let all = appearance::settings(&config);
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
                            about: s.about.into(),
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

    let keys: Vec<SharedString> = hotkey::keycaps(&config.hotkey).into_iter().map(Into::into).collect();
    w.set_shortcut_keys(ModelRc::new(VecModel::from(keys)));
    let mut status = (String::new(), true);
    with_app(|a| status = a.shortcut_status());
    w.set_shortcut_note(status.0.into());
    w.set_shortcut_ok(status.1);

    ENGINES.with_borrow_mut(|e| *e = config.search.clone());
    LINKS.with_borrow_mut(|l| *l = config.quicklink.clone());
    show_lists(w);
    w.set_version(update::VERSION.into());
    theme(w);
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
}

fn wire(w: &SettingsWindow) {
    w.on_chosen(|key, index| choose(&key, index));
    w.on_dismiss(close);
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
