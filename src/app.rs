//! What happens when you type, move, press Enter and open the actions: the state behind the window.

use crate::catalog::{Item, Kind, Run};
use crate::config::{self, Config};
use crate::icons::Icons;
use crate::search::{self, Line};
use crate::usage::Usage;
use crate::{ActionRow, Launcher, Row, Theme, apps, palettes, system, window};
use nucleo_matcher::Matcher;
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, VecModel};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

/// Runs `f` with the app, from anywhere on the UI thread.
pub fn with_app(f: impl FnOnce(&Rc<App>)) {
    let app = APP.with_borrow(|a| a.clone());
    if let Some(app) = app {
        f(&app);
    }
}

/// How long the window keeps what you typed while it's away, as Raycast does.
const KEEP_QUERY: Duration = Duration::from_secs(90);
/// Apps are looked for again on opening, at most this often.
const RESCAN_AFTER: Duration = Duration::from_secs(120);
/// The least time the "looking for apps" shapes stay up.
const MIN_BUSY: Duration = Duration::from_millis(900);
/// How long after opening a hover is taken to be the pointer already being there.
const POINTER_SETTLE: Duration = Duration::from_millis(400);

pub struct App {
    pub ui: slint::Weak<Launcher>,
    pub config: RefCell<Config>,
    catalog: RefCell<Vec<Item>>,
    /// Beside each row: the item it shows, or None for a heading.
    items: RefCell<Vec<Option<Item>>>,
    rows: Rc<VecModel<Row>>,
    usage: RefCell<Usage>,
    usage_changed: Cell<bool>,
    icons: RefCell<Icons>,
    matcher: RefCell<Matcher>,
    actions: RefCell<Vec<Act>>,
    shown_actions: RefCell<Vec<usize>>,
    action_rows: Rc<VecModel<ActionRow>>,
    pending: RefCell<Option<Item>>,
    toast_timer: Timer,
    scanned_at: Cell<Option<Instant>>,
    scanning: Cell<bool>,
    app_count: Cell<usize>,
    pub hidden_at: Cell<Option<Instant>>,
    pub shown_at: Cell<Option<Instant>>,
    hotkeys: RefCell<Option<crate::hotkey::Hotkeys>>,
}

/// Something the actions panel can do with the selected item.
#[derive(Clone)]
enum Act {
    Primary,
    Copy(&'static str, String),
    Reveal(PathBuf),
    Forget,
}

impl App {
    pub fn install(ui: &Launcher, config: Config) -> Rc<App> {
        let rows = Rc::new(VecModel::default());
        let action_rows = Rc::new(VecModel::default());
        ui.set_rows(ModelRc::from(rows.clone()));
        ui.set_actions(ModelRc::from(action_rows.clone()));
        let app = Rc::new(App {
            ui: ui.as_weak(),
            config: RefCell::new(config),
            catalog: RefCell::new(crate::catalog::commands()),
            items: RefCell::default(),
            rows,
            usage: RefCell::new(Usage::load()),
            usage_changed: Cell::new(false),
            icons: RefCell::default(),
            matcher: RefCell::new(Matcher::new(nucleo_matcher::Config::DEFAULT)),
            actions: RefCell::default(),
            shown_actions: RefCell::default(),
            action_rows,
            pending: RefCell::default(),
            toast_timer: Timer::default(),
            scanned_at: Cell::new(None),
            scanning: Cell::new(false),
            app_count: Cell::new(0),
            hidden_at: Cell::new(None),
            shown_at: Cell::new(None),
            hotkeys: RefCell::new(None),
        });
        APP.with_borrow_mut(|a| *a = Some(app.clone()));
        app.wire(ui);
        app.apply_theme();
        app.set_context(ui);
        app
    }

    fn wire(&self, ui: &Launcher) {
        ui.set_mod_key(if cfg!(target_os = "macos") { "⌘".into() } else { "Ctrl".into() });
        ui.on_query_edited(|q| with_app(|a| a.refresh(&q)));
        ui.on_move(|d| with_app(|a| a.move_by(d)));
        ui.on_hover(|i| with_app(|a| a.select(i, true)));
        ui.on_activate(|i, alt| with_app(|a| a.activate(i, alt)));
        ui.on_escape(|| with_app(|a| a.escape()));
        ui.on_toggle_actions(|| with_app(|a| a.toggle_actions()));
        ui.on_action_query_edited(|q| with_app(|a| a.filter_actions(&q)));
        ui.on_action_move(|d| with_app(|a| a.move_action(d)));
        ui.on_action_hover(|i| with_app(|a| a.ui.upgrade().map(|ui| ui.set_action_selected(i)).unwrap_or_default()));
        ui.on_action_run(|i| with_app(|a| a.run_action(i)));
        ui.on_answer(|yes| with_app(|a| a.answer(yes)));
        ui.on_settings(|| with_app(|a| a.run(&settings_item(), false)));
        ui.on_scheme_changed(|| with_app(|a| a.apply_theme()));
        ui.on_close_about(|| {
            with_app(|a| {
                if let Some(ui) = a.ui.upgrade() {
                    ui.set_about_open(false);
                    ui.invoke_focus_search();
                }
            })
        });
    }

    pub fn set_hotkeys(&self, hotkeys: crate::hotkey::Hotkeys) {
        *self.hotkeys.borrow_mut() = Some(hotkeys);
    }

    pub fn apply_theme(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let config = self.config.borrow();
        let key = if config.theme.eq_ignore_ascii_case("system") {
            if ui.get_system_dark() { &config.dark_theme } else { &config.light_theme }
        } else {
            &config.theme
        };
        let palette = palettes::find(key).unwrap_or(&palettes::PALETTES[0]);
        palettes::apply(&ui.global::<Theme>(), palette);
    }

    fn set_context(&self, ui: &Launcher) {
        ui.set_context_icon(self.icons.borrow_mut().builtin("bolt"));
        ui.set_context_tint(1);
        ui.set_context_title("Flint".into());
    }

    /// Called when the window is about to open.
    pub fn opening(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        *self.config.borrow_mut() = config::load();
        if let Some(hotkeys) = &*self.hotkeys.borrow() {
            hotkeys.set(&self.config.borrow().hotkey);
        }
        self.apply_theme();
        let stale = self.hidden_at.get().is_none_or(|t| t.elapsed() > KEEP_QUERY);
        if stale {
            ui.set_query(SharedString::new());
        }
        if self.scanned_at.get().is_none_or(|t| t.elapsed() > RESCAN_AFTER) {
            self.rescan(false);
        }
        self.close_overlays(&ui);
        self.refresh(&ui.get_query());
    }

    pub fn closing(&self) {
        if let Some(ui) = self.ui.upgrade() {
            self.close_overlays(&ui);
            ui.set_toast(SharedString::new());
        }
        self.hidden_at.set(Some(Instant::now()));
        if self.usage_changed.replace(false) {
            self.usage.borrow().save();
        }
    }

    fn close_overlays(&self, ui: &Launcher) {
        ui.set_actions_open(false);
        ui.set_confirm_open(false);
        ui.set_about_open(false);
        self.pending.take();
    }

    /// Looks for installed apps on a background thread; the list updates when it's done.
    pub fn rescan(&self, tell: bool) {
        if self.scanning.replace(true) {
            return;
        }
        self.set_busy(true);
        let started = Instant::now();
        let spawned = std::thread::Builder::new().name("scan".into()).spawn(move || {
            let found = apps::scan();
            // A scan can finish in a few milliseconds; keep the shapes up long enough to be seen
            // rather than flashing.
            if let Some(rest) = MIN_BUSY.checked_sub(started.elapsed()) {
                std::thread::sleep(rest);
            }
            let _ = slint::invoke_from_event_loop(move || with_app(|a| a.scanned(found, tell)));
        });
        if spawned.is_err() {
            self.scanning.set(false);
            self.set_busy(false);
        }
    }

    fn set_busy(&self, busy: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_busy(busy);
        }
    }

    fn scanned(&self, mut found: Vec<Item>, tell: bool) {
        let count = found.len();
        found.sort_by_key(|a| a.title.to_lowercase());
        found.extend(crate::catalog::commands());
        *self.catalog.borrow_mut() = found;
        self.scanning.set(false);
        self.set_busy(false);
        self.app_count.set(count);
        self.scanned_at.set(Some(Instant::now()));
        if let Some(ui) = self.ui.upgrade() {
            self.refresh(&ui.get_query());
        }
        if tell {
            self.toast(&format!("Found {count} apps"), false);
        }
    }

    pub fn refresh(&self, query: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let lines = search::lines(
            query,
            &self.catalog.borrow(),
            &self.usage.borrow(),
            &self.config.borrow().search,
            &mut self.matcher.borrow_mut(),
        );
        let mut icons = self.icons.borrow_mut();
        let mut rows = Vec::with_capacity(lines.len());
        let mut items = Vec::with_capacity(lines.len());
        let mut y = 0.0;
        for line in lines {
            match line {
                Line::Section(title) => {
                    let height = if y == 0.0 { 30.0 } else { 38.0 };
                    rows.push(Row { kind: 0, y, height, title: title.into(), ..Default::default() });
                    items.push(None);
                    y += height;
                }
                Line::Empty(message) => {
                    rows.push(Row { kind: 3, y, height: 150.0, title: message.into(), ..Default::default() });
                    items.push(None);
                    y += 150.0;
                }
                Line::Item(item) => {
                    let calc = item.kind == Kind::Calculator;
                    let height = if calc { 100.0 } else { 40.0 };
                    let (icon, tint) = icons.get(&item.icon, item.tint);
                    rows.push(Row {
                        kind: if calc { 2 } else { 1 },
                        y,
                        height,
                        title: item.title.as_str().into(),
                        subtitle: item.subtitle.as_str().into(),
                        accessory: if calc { item.keywords.as_str() } else { item.kind.label() }.into(),
                        icon,
                        tint,
                    });
                    items.push(Some(item));
                    y += height;
                }
            }
        }
        drop(icons);
        let first = items.iter().position(Option::is_some).map_or(-1, |i| i as i32);
        *self.items.borrow_mut() = items;
        ui.set_glide(false);
        self.rows.set_vec(rows);
        ui.set_rows_height(y);
        ui.set_selected(-1);
        ui.set_selected(first);
        ui.invoke_reset_scroll();
        self.update_bar(&ui);
    }

    fn selected_item(&self) -> Option<Item> {
        let ui = self.ui.upgrade()?;
        let i = usize::try_from(ui.get_selected()).ok()?;
        self.items.borrow().get(i).cloned().flatten()
    }

    fn update_bar(&self, ui: &Launcher) {
        let label = self.selected_item().map_or("", |i| i.verb());
        ui.set_primary_label(label.into());
    }

    fn select(&self, i: i32, by_pointer: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        // A pointer resting where the window opens reports itself as a hover; only a pointer
        // moved after opening should take the selection off the first row.
        if by_pointer && self.shown_at.get().is_some_and(|t| t.elapsed() < POINTER_SETTLE) {
            return;
        }
        let ok = usize::try_from(i).ok().and_then(|i| self.items.borrow().get(i).map(Option::is_some)).unwrap_or(false);
        if ok && !ui.get_actions_open() {
            ui.set_glide(!by_pointer);
            ui.set_selected(i);
            self.update_bar(&ui);
        }
    }

    fn move_by(&self, delta: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let items = self.items.borrow();
        let selectable: Vec<i32> = items.iter().enumerate().filter(|(_, it)| it.is_some()).map(|(i, _)| i as i32).collect();
        if selectable.is_empty() {
            return;
        }
        let at = selectable.iter().position(|&i| i == ui.get_selected()).unwrap_or(0) as i32;
        let n = selectable.len() as i32;
        // Single steps wrap around the ends; page steps stop at them.
        let next = if delta.abs() == 1 { (at + delta).rem_euclid(n) } else { (at + delta).clamp(0, n - 1) };
        drop(items);
        self.select(selectable[next as usize], false);
    }

    fn escape(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_query().is_empty() {
            window::hide(&ui);
        } else {
            ui.set_query(SharedString::new());
            self.refresh("");
        }
    }

    fn activate(&self, i: i32, alternate: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        let item = usize::try_from(i).ok().and_then(|i| self.items.borrow().get(i).cloned().flatten());
        let Some(item) = item else { return };
        if alternate {
            // Ctrl/Cmd Enter: the second action in the panel.
            let acts = actions_for(&item, &self.usage.borrow());
            if let Some(act) = acts.get(1).cloned() {
                self.perform(&item, act, &ui);
                return;
            }
        }
        self.run(&item, true);
    }

    /// Does what Enter does for an item.
    fn run(&self, item: &Item, record: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        if let Run::Power(power) = &item.run
            && let Some((title, message, label)) = power.confirmation()
        {
            ui.set_confirm_title(title.into());
            ui.set_confirm_message(message.into());
            ui.set_confirm_label(label.into());
            ui.set_confirm_icon(self.icons.borrow_mut().builtin(match &item.icon {
                crate::catalog::Icon::Builtin(name) => name,
                _ => "power",
            }));
            *self.pending.borrow_mut() = Some(item.clone());
            ui.set_actions_open(false);
            ui.set_confirm_open(true);
            return;
        }
        if record && item.kind != Kind::Calculator {
            self.usage.borrow_mut().record(&item.id, &ui.get_query());
            self.usage_changed.set(true);
        }
        let outcome = match &item.run {
            Run::App(launch) => apps::launch(launch).map_err(|e| e.to_string()).map(|_| Done::Hide),
            Run::Power(power) => system::power(*power).map_err(|e| e.to_string()).map(|_| Done::Hide),
            Run::Url(url) => system::open(url).map_err(|e| e.to_string()).map(|_| Done::Hide),
            Run::Copy(text) => system::copy(text).map(|_| Done::Toast("Copied to clipboard".into(), true)),
            Run::Settings => system::open(&config::path().to_string_lossy()).map_err(|e| e.to_string()).map(|_| Done::Hide),
            Run::Theme(key) => {
                self.config.borrow_mut().theme = key.to_string();
                self.apply_theme();
                let name = palettes::find(key).map_or(*key, |p| p.name);
                config::save_theme(key).map_err(|e| e.to_string()).map(|_| Done::Toast(format!("Theme set to {name}"), false))
            }
            Run::Reload => {
                self.rescan(true);
                Ok(Done::Stay)
            }
            Run::About => {
                let apps = self.app_count.get();
                ui.set_about_detail(
                    format!("Version {} · {apps} {} found", env!("CARGO_PKG_VERSION"), if apps == 1 { "app" } else { "apps" }).into(),
                );
                ui.set_actions_open(false);
                ui.set_about_open(true);
                Ok(Done::Stay)
            }
            Run::Quit => {
                let _ = slint::quit_event_loop();
                Ok(Done::Stay)
            }
        };
        match outcome {
            Ok(Done::Hide) => window::hide(&ui),
            Ok(Done::Toast(text, then_hide)) => {
                self.toast(&text, false);
                if then_hide {
                    Timer::single_shot(Duration::from_millis(500), || {
                        with_app(|a| {
                            if let Some(ui) = a.ui.upgrade() {
                                window::hide(&ui);
                            }
                        })
                    });
                }
            }
            Ok(Done::Stay) => {}
            Err(e) => self.toast(&format!("Couldn't do that: {e}"), true),
        }
    }

    fn answer(&self, yes: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.set_confirm_open(false);
        let pending = self.pending.take();
        if let (true, Some(item)) = (yes, pending) {
            self.usage.borrow_mut().record(&item.id, &ui.get_query());
            self.usage_changed.set(true);
            if let Run::Power(power) = item.run {
                match system::power(power) {
                    Ok(()) => window::hide(&ui),
                    Err(e) => self.toast(&format!("Couldn't do that: {e}"), true),
                }
            }
        }
    }

    pub fn toast(&self, text: &str, failed: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.set_toast(text.into());
        ui.set_toast_failed(failed);
        self.toast_timer.start(slint::TimerMode::SingleShot, Duration::from_millis(2500), || {
            with_app(|a| {
                if let Some(ui) = a.ui.upgrade() {
                    ui.set_toast(SharedString::new());
                }
            })
        });
    }

    fn toggle_actions(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_actions_open() {
            ui.set_actions_open(false);
            ui.invoke_focus_search();
            return;
        }
        let Some(item) = self.selected_item() else { return };
        *self.actions.borrow_mut() = actions_for(&item, &self.usage.borrow());
        ui.set_actions_title(item.title.as_str().into());
        ui.set_action_query(SharedString::new());
        self.filter_actions("");
        ui.set_actions_open(true);
        ui.invoke_focus_actions();
    }

    fn filter_actions(&self, query: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let Some(item) = self.selected_item() else { return };
        let query = query.trim().to_lowercase();
        let acts = self.actions.borrow();
        let mut icons = self.icons.borrow_mut();
        let mut shown = Vec::new();
        let mut rows = Vec::new();
        for (i, act) in acts.iter().enumerate() {
            let (title, icon, keys, danger) = describe(act, &item, i == 1);
            if !query.is_empty() && !title.to_lowercase().contains(&query) {
                continue;
            }
            shown.push(i);
            let keys: Vec<SharedString> = keys.iter().map(|k| SharedString::from(*k)).collect();
            rows.push(ActionRow { title: title.into(), icon: icons.builtin(icon), keys: ModelRc::new(VecModel::from(keys)), danger });
        }
        *self.shown_actions.borrow_mut() = shown;
        self.action_rows.set_vec(rows);
        ui.set_action_selected(0);
    }

    fn move_action(&self, delta: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let n = self.action_rows.row_count() as i32;
        if n > 0 {
            ui.set_action_selected((ui.get_action_selected() + delta).rem_euclid(n));
        }
    }

    fn run_action(&self, i: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let act = usize::try_from(i)
            .ok()
            .and_then(|i| self.shown_actions.borrow().get(i).copied())
            .and_then(|i| self.actions.borrow().get(i).cloned());
        let (Some(act), Some(item)) = (act, self.selected_item()) else { return };
        ui.set_actions_open(false);
        ui.invoke_focus_search();
        self.perform(&item, act, &ui);
    }

    fn perform(&self, item: &Item, act: Act, ui: &Launcher) {
        match act {
            Act::Primary => self.run(item, true),
            Act::Copy(what, text) => match system::copy(&text) {
                Ok(()) => self.toast(&format!("Copied {what}"), false),
                Err(e) => self.toast(&format!("Couldn't copy: {e}"), true),
            },
            Act::Reveal(path) => match system::reveal(&path) {
                Ok(()) => window::hide(ui),
                Err(e) => self.toast(&format!("Couldn't show it: {e}"), true),
            },
            Act::Forget => {
                self.usage.borrow_mut().forget(&item.id);
                self.usage_changed.set(true);
                self.toast("Ranking reset", false);
                self.refresh(&ui.get_query());
            }
        }
    }
}

enum Done {
    Hide,
    Toast(String, bool),
    Stay,
}

fn settings_item() -> Item {
    crate::catalog::commands().into_iter().find(|i| matches!(i.run, Run::Settings)).expect("settings is a command")
}

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else if cfg!(windows) {
        "Show in Explorer"
    } else {
        "Show in Files"
    }
}

fn actions_for(item: &Item, usage: &Usage) -> Vec<Act> {
    let mut acts = vec![Act::Primary];
    match &item.run {
        Run::Copy(_) => acts.push(Act::Copy("sum", format!("{} = {}", item.title, item.subtitle))),
        Run::Url(url) => acts.push(Act::Copy("link", url.clone())),
        _ => {}
    }
    if let Some(path) = &item.path {
        acts.push(Act::Reveal(path.clone()));
        acts.push(Act::Copy("path", path.to_string_lossy().into_owned()));
    }
    if item.kind == Kind::Application || item.kind == Kind::Command {
        acts.push(Act::Copy("name", item.title.clone()));
        if usage.bonus(&item.id, "") > 0 {
            acts.push(Act::Forget);
        }
    }
    acts
}

/// Title, icon, shortcut keys and whether it's destructive. The second action is also on
/// Ctrl/Cmd Enter (see App::activate), so it shows those keys.
fn describe(act: &Act, item: &Item, second: bool) -> (String, &'static str, Vec<&'static str>, bool) {
    let mod_key = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" };
    let keys = if second { vec![mod_key, "↵"] } else { vec![] };
    match act {
        Act::Primary => (item.verb().to_string(), if item.kind == Kind::Application { "open" } else { "arrow" }, vec!["↵"], false),
        Act::Copy(what, _) => (format!("Copy {}", capitalise(what)), "copy", keys, false),
        Act::Reveal(_) => (reveal_label().to_string(), "folder", keys, false),
        Act::Forget => ("Reset Ranking".into(), "eraser", keys, true),
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}
