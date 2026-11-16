//! What happens when you type, move, press Enter and open the actions: the state behind the window.

use crate::catalog::{Item, Kind, Run};
use crate::config::{self, Config};
use crate::icons::Icons;
use crate::search::{self, Line};
use crate::usage::Usage;
use crate::{ActionRow, Detail, Fact, Launcher, Row, Theme, appearance, apps, clipboard, palettes, switcher, system, update, window};
use nucleo_matcher::Matcher;
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, VecModel};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// How long the window stays on the view you left it on (Clipboard History, Appearance…).
/// What you typed is cleared on closing.
const KEEP_VIEW: Duration = Duration::from_secs(90);
/// Apps are looked for again on opening, at most this often.
const RESCAN_AFTER: Duration = Duration::from_secs(120);
/// Files are looked through again on opening, at most this often.
const REINDEX_AFTER: Duration = Duration::from_secs(20 * 60);
/// The least time the "looking for apps" shapes stay up.
const MIN_BUSY: Duration = Duration::from_millis(900);

/// Set when Tendril should start itself again after quitting (main.rs does it): a setting changed
/// that needs a different renderer.
pub static RESTART: AtomicBool = AtomicBool::new(false);
/// Whether the next Tendril opens on the Appearance view.
pub static RESTART_TO_APPEARANCE: AtomicBool = AtomicBool::new(false);

/// Whether the update thread may ask GitHub. Kept in step with config.toml by the UI thread, and
/// read by the update thread before every check.
static UPDATES_ON: AtomicBool = AtomicBool::new(false);
/// How often to look for a new version while Tendril runs. The thread sleeps in between.
const UPDATE_EVERY: Duration = Duration::from_secs(6 * 3600);
/// The first look, a little after starting, so it doesn't slow the first open.
const FIRST_UPDATE_CHECK: Duration = Duration::from_secs(30);

/// What the list is showing.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum View {
    /// Apps and commands.
    Root,
    Appearance,
    Clipboard,
    Windows,
}

/// Where an update stands.
enum UpdateState {
    Quiet,
    Available(update::Release),
    Installing(String),
    /// Installed; used from the next start.
    Ready(String),
}

/// The walkthrough's steps: welcome, shortcut, look, layout, done.
const STEPS: usize = 5;

const TILE_HEIGHT: f32 = 92.0;
const TILE_GAP: f32 = 8.0;

pub struct App {
    pub ui: slint::Weak<Launcher>,
    pub config: RefCell<Config>,
    catalog: RefCell<Vec<Item>>,
    /// The apps last found, kept apart so the catalog can be rebuilt when quicklinks change.
    apps: RefCell<Vec<Item>>,
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
    /// A setting change waiting for "Turn On" in the GPU dialog: its key and direction.
    pending_setting: Cell<Option<(&'static str, i32)>>,
    toast_timer: Timer,
    scanned_at: Cell<Option<Instant>>,
    scanning: Cell<bool>,
    /// Files found by name, from the last walk of the [files] folders.
    files: RefCell<crate::files::Index>,
    /// The [files] rules the last walk used, and when it started: a change to them, or enough
    /// time, means walking again.
    indexed_with: RefCell<Option<config::Files>>,
    indexed_at: Cell<Option<Instant>>,
    /// Say how many were found when the walk ends (Index Files was run).
    tell_indexed: Cell<bool>,
    update: RefCell<UpdateState>,
    /// With previews on "ask": the newest preview, offered below any update.
    preview_offer: RefCell<Option<update::Release>>,
    /// Checking or downloading an update right now.
    updating: Cell<bool>,
    update_thread: Cell<bool>,
    app_count: Cell<usize>,
    pub hidden_at: Cell<Option<Instant>>,
    pub shown_at: Cell<Option<Instant>>,
    hotkeys: RefCell<Option<crate::hotkey::Hotkeys>>,
    /// Showing the Appearance view instead of search results.
    view: Cell<View>,
    /// What you've copied, newest first.
    history: RefCell<clipboard::History>,
    /// Whether a thread hears about copies as they happen; if not, the clipboard is read on open.
    clipboard_watched: Cell<bool>,
    /// Open windows, listed when the Switch Windows view opens.
    windows: RefCell<Vec<switcher::Win>>,
    /// The default browser's icon, for rows that open web pages.
    browser: RefCell<Option<crate::catalog::Icon>>,
    /// The version Tendril was before it updated itself, until you've looked at what's new.
    updated_from: RefCell<Option<String>>,
    /// Where the pointer was at the last hover. Rows rebuilt or scrolled under a still pointer,
    /// a window opening under it, or focus arriving, all report a hover; only a pointer that
    /// has moved since should take the selection.
    pointer: Cell<Option<(f32, f32)>>,
    /// The first-run walkthrough's step, while it's showing.
    onboarding: Cell<Option<usize>>,
    /// Blur changed during the walkthrough on a system where that needs a restart: done at the end.
    restart_after_onboarding: Cell<bool>,
    /// For each Ctrl+number, the row it opens.
    quick: RefCell<Vec<usize>>,
}

/// Something the actions panel can do with the selected item.
#[derive(Clone)]
enum Act {
    Primary,
    Copy(&'static str, String),
    Reveal(PathBuf),
    Forget,
    NoPreviews,
    /// Copy a kept copy without pasting it.
    CopyClip(u64),
    DeleteClip(u64),
    ClearClips,
    CloseWindow(u64),
    /// Pin to (false) or unpin from (true) the favourites.
    Pin(bool),
    /// Windows: start the program elevated.
    #[cfg_attr(not(windows), allow(dead_code))]
    RunAsAdmin(PathBuf),
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
            apps: RefCell::default(),
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
            pending_setting: Cell::new(None),
            toast_timer: Timer::default(),
            scanned_at: Cell::new(None),
            scanning: Cell::new(false),
            files: RefCell::default(),
            indexed_with: RefCell::new(None),
            indexed_at: Cell::new(None),
            tell_indexed: Cell::new(false),
            update: RefCell::new(UpdateState::Quiet),
            preview_offer: RefCell::new(None),
            updating: Cell::new(false),
            update_thread: Cell::new(false),
            app_count: Cell::new(0),
            hidden_at: Cell::new(None),
            shown_at: Cell::new(None),
            hotkeys: RefCell::new(None),
            view: Cell::new(View::Root),
            history: RefCell::new(clipboard::History::default()),
            clipboard_watched: Cell::new(false),
            windows: RefCell::new(Vec::new()),
            browser: RefCell::new(None),
            updated_from: RefCell::new(updated_from()),
            pointer: Cell::new(None),
            onboarding: Cell::new(None),
            restart_after_onboarding: Cell::new(false),
            quick: RefCell::default(),
        });
        APP.with_borrow_mut(|a| *a = Some(app.clone()));
        app.wire(ui);
        app.apply_theme();
        app.apply_appearance();
        app.set_context(ui);
        app
    }

    fn wire(&self, ui: &Launcher) {
        ui.set_mod_key(if cfg!(target_os = "macos") { "⌘".into() } else { "Ctrl".into() });
        ui.on_query_edited(|q| with_app(|a| a.refresh(&q)));
        ui.on_move(|d| with_app(|a| a.move_by(d)));
        ui.on_hover(|i, x, y| with_app(|a| a.hover(i, x, y)));
        ui.on_activate(|i, alt| with_app(|a| a.activate(i, alt)));
        ui.on_escape(|| with_app(|a| a.escape()));
        ui.on_back(|| {
            let mut left = false;
            with_app(|a| left = a.back());
            left
        });
        ui.on_toggle_actions(|| with_app(|a| a.toggle_actions()));
        ui.on_action_query_edited(|q| with_app(|a| a.filter_actions(&q)));
        ui.on_action_move(|d| with_app(|a| a.move_action(d)));
        ui.on_action_hover(|i| with_app(|a| a.ui.upgrade().map(|ui| ui.set_action_selected(i)).unwrap_or_default()));
        ui.on_action_run(|i| with_app(|a| a.run_action(i)));
        ui.on_answer(|yes| with_app(|a| a.answer(yes)));
        ui.on_settings(|| with_app(|a| a.run(&settings_item(), false)));
        ui.on_scheme_changed(|| with_app(|a| a.apply_theme()));
        ui.on_nudge(|d| with_app(|a| a.step(d)));
        ui.on_onboard_next(|| with_app(|a| a.onboard(1)));
        ui.on_onboard_back(|| with_app(|a| a.onboard(-1)));
        ui.on_record(|text, control, alt, shift, meta| {
            let mut taken = false;
            with_app(|a| taken = a.record(&text, control, alt, shift, meta));
            taken
        });
        ui.on_quick(|key| {
            let mut opened = false;
            with_app(|a| opened = a.quick_open(&key));
            opened
        });
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
        if crate::fluent::on() {
            crate::fluent::apply(&ui.global::<Theme>());
        }
    }

    /// The window's look, from [appearance] in config.toml.
    pub fn apply_appearance(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let a = self.config.borrow().appearance.clone();
        ui.set_see_through(a.blur && window::can_see_through());
        ui.set_panel_opacity(a.opacity());
        ui.set_glow_mode(a.glow_mode());
        ui.set_gradient_divider(a.gradient_divider);
        let theme = ui.global::<Theme>();
        // Modern only once the GPU draws: until Tendril restarts, it stays flat.
        theme.set_modern(a.style == "modern" && window::is_gpu());
        theme.set_blue_selection(a.selection == "blue");
        theme.set_bold_icons(a.icons == "bold");
        let walkthrough = self.onboarding.get().is_some();
        ui.set_compact(a.compact() && !walkthrough);
        ui.set_details(a.details && !walkthrough);
        // Windows 11 and macOS round the window themselves.
        ui.set_corner(if cfg!(any(windows, target_os = "macos")) { 0.0 } else { 12.0 });
        self.show_shortcut(&ui, "Press the new keys", true);
    }

    fn set_context(&self, ui: &Launcher) {
        let (icon, title, placeholder) = match self.view.get() {
            View::Root => ("tendril", "Tendril", "Search for apps and commands…"),
            View::Appearance => ("palette", "Appearance", "Search settings…"),
            View::Clipboard => ("clipboard", "Clipboard History", "Search what you've copied…"),
            View::Windows => ("windows", "Switch Windows", "Search open windows…"),
        };
        ui.set_context_icon(self.icons.borrow_mut().builtin_solid(icon));
        // Blue: Tendril's one colour, as blue leads on Blight's dashboard.
        ui.set_context_tint(2);
        ui.set_context_title(title.into());
        ui.set_placeholder(placeholder.into());
    }

    /// Opens the Appearance view, as the "Appearance" command and `tendril --appearance` do.
    pub fn open_appearance(&self) {
        self.open_view(View::Appearance);
    }

    pub fn open_view(&self, view: View) {
        let Some(ui) = self.ui.upgrade() else { return };
        if view == View::Windows {
            *self.windows.borrow_mut() = switcher::list();
        }
        self.view.set(view);
        ui.set_query(SharedString::new());
        self.set_context(&ui);
        self.refresh("");
    }

    fn leave_view(&self, ui: &Launcher) {
        self.view.set(View::Root);
        self.windows.borrow_mut().clear();
        ui.set_query(SharedString::new());
        self.set_context(ui);
        self.refresh("");
    }

    /// Starts keeping copies, or stops and forgets them, as the setting says.
    pub fn start_clipboard(&self) {
        let on = self.config.borrow().clipboard_history;
        let was = clipboard::ON.swap(on, std::sync::atomic::Ordering::Relaxed);
        if on && !was {
            *self.history.borrow_mut() = clipboard::History::load();
            if !self.clipboard_watched.get() {
                self.clipboard_watched.set(clipboard::watch());
            }
        } else if !on {
            let mut history = self.history.borrow_mut();
            if !history.clips.is_empty() || was {
                history.clips.clear();
                history.save();
            }
        }
    }

    /// A copy, from the watcher thread or read when the window opens.
    pub fn copied(&self, text: String) {
        if !clipboard::ON.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let changed = {
            let mut history = self.history.borrow_mut();
            let changed = history.add(text, clipboard::now());
            if changed {
                history.save();
            }
            changed
        };
        if changed
            && self.view.get() == View::Clipboard
            && let Some(ui) = self.ui.upgrade()
            && ui.get_query().is_empty()
        {
            self.refresh("");
        }
    }

    /// Called when the window is about to open.
    pub fn opening(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        *self.config.borrow_mut() = config::load();
        self.rebuild_catalog();
        self.start_update_checks();
        self.start_clipboard();
        if !self.clipboard_watched.get()
            && let Some(text) = clipboard::read()
        {
            self.copied(text);
        }
        if let Some(hotkeys) = &*self.hotkeys.borrow() {
            hotkeys.set(&self.config.borrow().hotkey);
        }
        let refused = self.hotkeys.borrow().as_ref().and_then(|h| h.refused());
        self.apply_theme();
        self.apply_appearance();
        self.icons.borrow_mut().set_scale(ui.window().scale_factor());
        self.pointer.set(None);
        let stale = self.hidden_at.get().is_none_or(|t| t.elapsed() > KEEP_VIEW);
        if stale {
            self.view.set(View::Root);
            self.set_context(&ui);
        }
        // Windows come and go while Tendril is closed.
        if self.view.get() == View::Windows {
            *self.windows.borrow_mut() = switcher::list();
        }
        if self.scanned_at.get().is_none_or(|t| t.elapsed() > RESCAN_AFTER) {
            self.rescan(false);
        }
        if self.indexed_with.borrow().as_ref() != Some(&self.config.borrow().files)
            || self.indexed_at.get().is_none_or(|t| t.elapsed() > REINDEX_AFTER)
        {
            self.index_files(false);
        }
        self.close_overlays(&ui);
        if !self.config.borrow().onboarded && self.onboarding.get().is_none() {
            self.start_onboarding();
            return;
        }
        self.refresh(&ui.get_query());
        if let Some(keys) = refused {
            self.toast(
                &format!(
                    "Another app uses {}, so it can't open Tendril. Search for Appearance to pick other keys.",
                    keys.replace('+', " ")
                ),
                true,
            );
        }
    }

    pub fn start_onboarding(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        self.view.set(View::Root);
        ui.set_query(SharedString::new());
        self.show_step(0);
    }

    fn show_step(&self, step: usize) {
        let Some(ui) = self.ui.upgrade() else { return };
        self.onboarding.set(Some(step));
        self.apply_appearance();
        let wayland = !self.hotkeys.borrow().as_ref().is_some_and(|h| h.available());
        let (title, body) = match step {
            0 => ("Welcome to Tendril", "Let's set it up. It takes a minute, and all of it can be changed later."),
            1 if wayland => (
                "Your shortcut",
                "Wayland doesn't let apps take keys for themselves. In your desktop's keyboard settings, add a shortcut that runs tendril toggle.",
            ),
            1 if self.hotkeys.borrow().as_ref().and_then(|h| h.refused()).is_some() => {
                ("Your shortcut", "Another app already uses those keys (Raycast or PowerToys, often). Press the keys you'd like instead.")
            }
            1 => ("Your shortcut", "Press the keys you'd like to open Tendril with, from anywhere. Enter keeps what's there."),
            2 => ("How it looks", "Changes show straight away, behind this."),
            3 => ("How it's laid out", "What you see before you type, and as you do."),
            _ => ("You're set", "Search for Appearance whenever you'd like to change something."),
        };
        ui.set_onboarding(true);
        ui.set_step(step as i32);
        ui.set_steps(STEPS as i32);
        ui.set_step_title(title.into());
        ui.set_step_body(body.into());
        self.refresh("");
    }

    /// Continue (1) or back (-1). Back from the first step skips the walkthrough.
    fn onboard(&self, delta: i32) {
        let Some(step) = self.onboarding.get() else { return };
        let next = step as i32 + delta;
        if next < 0 || next as usize >= STEPS {
            self.finish_onboarding();
        } else {
            self.show_step(next as usize);
        }
    }

    fn finish_onboarding(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let _ = config::set(None, "onboarded", config::Value::Bool(true));
        *self.config.borrow_mut() = config::load();
        self.onboarding.set(None);
        ui.set_onboarding(false);
        self.apply_appearance();
        self.set_context(&ui);
        self.refresh("");
        if self.restart_after_onboarding.replace(false) {
            self.toast("Restarting to change how Tendril draws…", false);
            RESTART.store(true, Ordering::SeqCst);
            Timer::single_shot(Duration::from_millis(700), || {
                let _ = slint::quit_event_loop();
            });
        }
    }

    /// A key press while the shortcut row is selected: the new shortcut, if it is one.
    /// Takes `combo` as the shortcut that opens Tendril, and saves it.
    pub fn take_shortcut(&self, combo: &str) -> Result<(), String> {
        let hotkeys = self.hotkeys.borrow();
        let hotkeys = hotkeys.as_ref().ok_or("there's no global key to take here")?;
        hotkeys.change(combo)?;
        config::set(None, "hotkey", config::Value::Text(combo)).map_err(|e| format!("couldn't save it: {e}"))?;
        self.config.borrow_mut().hotkey = combo.to_string();
        Ok(())
    }

    /// Lets go of the shortcut while new keys are recorded, so pressing it doesn't open Tendril.
    pub fn pause_shortcut(&self, paused: bool) {
        if let Some(hotkeys) = &*self.hotkeys.borrow() {
            if paused { hotkeys.pause() } else { hotkeys.resume() }
        }
    }

    /// What to say about the shortcut, and whether it's working.
    pub fn shortcut_status(&self) -> (String, bool) {
        match &*self.hotkeys.borrow() {
            None => ("Bind tendril toggle in your desktop's keyboard settings".into(), false),
            Some(h) if !h.available() => ("Bind tendril toggle in your desktop's keyboard settings".into(), false),
            Some(h) if h.refused().is_some() => ("Another app uses these keys. Change them to open Tendril".into(), false),
            Some(_) => ("Opens Tendril from any app".into(), true),
        }
    }

    /// The palette in use, as apply_theme picks it.
    pub fn palette(&self) -> &'static palettes::Palette {
        let config = self.config.borrow();
        let dark = self.ui.upgrade().is_some_and(|ui| ui.get_system_dark());
        let key = if config.theme.eq_ignore_ascii_case("system") {
            if dark { &config.dark_theme } else { &config.light_theme }
        } else {
            &config.theme
        };
        palettes::find(key).unwrap_or(&palettes::PALETTES[0])
    }

    fn record(&self, text: &str, control: bool, alt: bool, shift: bool, meta: bool) -> bool {
        let Some(combo) = crate::hotkey::shortcut(text, control, alt, shift, meta) else { return false };
        let Some(ui) = self.ui.upgrade() else { return true };
        let taken = self.hotkeys.borrow().as_ref().map(|h| h.change(&combo));
        match taken {
            Some(Ok(())) => match config::set(None, "hotkey", config::Value::Text(&combo)) {
                Ok(()) => {
                    self.config.borrow_mut().hotkey = combo;
                    self.show_shortcut(&ui, "Saved. Press other keys to change it again.", true);
                }
                Err(e) => self.show_shortcut(&ui, &format!("Couldn't save it: {e}"), false),
            },
            Some(Err(e)) => self.show_shortcut(&ui, &format!("Not those: {e}."), false),
            None => {}
        }
        true
    }

    fn show_shortcut(&self, ui: &Launcher, note: &str, ok: bool) {
        let keys: Vec<SharedString> = crate::hotkey::keycaps(&self.config.borrow().hotkey).into_iter().map(Into::into).collect();
        ui.set_shortcut_keys(ModelRc::new(VecModel::from(keys)));
        ui.set_shortcut_note(note.into());
        ui.set_shortcut_ok(ok);
    }

    pub fn closing(&self) {
        if let Some(hotkeys) = &*self.hotkeys.borrow() {
            hotkeys.resume();
        }
        if let Some(ui) = self.ui.upgrade() {
            ui.set_recording(false);
            self.close_overlays(&ui);
            ui.set_toast(SharedString::new());
            ui.set_query(SharedString::new());
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
        self.pending_setting.take();
    }

    /// Looks for installed apps on a background thread; the list updates when it's done.
    /// The apps found last time, straight away; a fresh scan replaces them when it's done.
    pub fn load_cached_apps(&self) {
        let mut cached = apps::cached();
        if cached.is_empty() {
            return;
        }
        cached.sort_by_key(|a| a.title.to_lowercase());
        self.app_count.set(cached.len());
        *self.apps.borrow_mut() = cached;
        self.rebuild_catalog();
    }

    pub fn rescan(&self, tell: bool) {
        if self.scanning.replace(true) {
            return;
        }
        self.set_busy(true);
        let started = Instant::now();
        let spawned = std::thread::Builder::new().name("scan".into()).spawn(move || {
            let found = apps::scan();
            apps::remember(&found);
            // When you asked for it (Reload Apps), keep the shapes up long enough to be seen
            // rather than flashing. Otherwise nobody's waiting on it: no delay.
            if tell && let Some(rest) = MIN_BUSY.checked_sub(started.elapsed()) {
                std::thread::sleep(rest);
            }
            let _ = slint::invoke_from_event_loop(move || with_app(|a| a.scanned(found, tell)));
        });
        if spawned.is_err() {
            self.scanning.set(false);
            self.set_busy(false);
        }
    }

    /// Apps, then Tendril's commands, then your quicklinks.
    fn rebuild_catalog(&self) {
        let mut catalog = self.apps.borrow().clone();
        catalog.extend(crate::catalog::commands());
        catalog.extend(crate::catalog::quicklinks(&self.config.borrow().quicklink));
        *self.browser.borrow_mut() = crate::browser::icon(&catalog);
        for item in &mut catalog {
            self.decorate(item);
        }
        *self.catalog.borrow_mut() = catalog;
        let config = self.config.borrow();
        if config.site_icons {
            let urls: Vec<String> = config.quicklink.iter().map(|l| l.url.clone()).collect();
            crate::sites::fetch(&urls, || {
                with_app(|a| {
                    a.rebuild_catalog();
                    if let Some(ui) = a.ui.upgrade() {
                        a.refresh_keeping(&ui.get_query(), None);
                    }
                })
            });
        }
    }

    /// Icons the icon thread finished: the rows showing them get them, without rebuilding the list.
    pub fn icons_arrived(&self, batch: Vec<crate::icons::Pixels>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let arrived = self.icons.borrow_mut().arrived(batch);
        let items = self.items.borrow();
        for (i, item) in items.iter().enumerate() {
            let (Some(item), Some(mut row)) = (item, self.rows.row_data(i)) else { continue };
            let points = if row.kind == 4 { crate::icons::TILE } else { crate::icons::ROW };
            let Some(key) = self.icons.borrow().key(&item.icon, points) else { continue };
            if arrived.contains(&key) {
                let (icon, icon_on, tint) = self.icons.borrow_mut().get(&item.icon, item.tint, points);
                row.icon = icon;
                row.icon_on = icon_on;
                row.tint = tint;
                self.rows.set_row_data(i, row);
            }
        }
        drop(items);
        let selected = self.selected_item();
        if self.config.borrow().appearance.details
            && let Some(item) = selected
            && self.icons.borrow().key(&item.icon, crate::icons::DETAIL).is_some_and(|k| arrived.contains(&k))
        {
            ui.set_detail(self.detail(&item));
        }
    }

    /// Real icons on rows that open something outside Tendril: a quicklink wears its site's
    /// icon (sites.rs), and searches and links the default browser's.
    fn decorate(&self, item: &mut Item) {
        use crate::catalog::Icon;
        if matches!(item.run, Run::IndexFiles) {
            item.subtitle = crate::files::status();
            return;
        }
        if !matches!(item.kind, Kind::Link | Kind::Fallback) || !matches!(item.icon, Icon::Builtin(_)) {
            return;
        }
        let config = self.config.borrow();
        let link = item.id.strip_prefix("link:").or_else(|| item.id.strip_prefix("fallback:"));
        let link = link.and_then(|name| config.quicklink.iter().find(|l| l.name.eq_ignore_ascii_case(name)));
        let site = link.filter(|_| config.site_icons).and_then(|l| crate::sites::icon(&l.url));
        let web = matches!(item.run, Run::Url(_)) || link.is_some_and(|l| crate::sites::host(&l.url).is_some());
        let icon = match site {
            Some(path) => Some(Icon::File(path)),
            None if web => self.browser.borrow().clone(),
            None => None,
        };
        if let Some(icon) = icon {
            item.icon = icon;
            item.tint = 0;
        }
    }

    /// Walks the [files] folders again on a background thread, or forgets them if file search is off.
    pub fn index_files(&self, tell: bool) {
        if crate::files::walking() {
            // Rules changed meanwhile are picked up on a later open, once this walk is done.
            if tell {
                self.toast("Already looking through your files", false);
            }
            return;
        }
        let files = self.config.borrow().files.clone();
        *self.indexed_with.borrow_mut() = Some(files.clone());
        if !files.index {
            // Off means nothing kept about your files either.
            let _ = crate::files::forget();
            crate::files::set_off();
            *self.files.borrow_mut() = crate::files::Index::default();
            if tell {
                self.toast("File search is off. Appearance can turn it on.", false);
            }
            return;
        }
        self.indexed_at.set(Some(Instant::now()));
        self.tell_indexed.set(self.tell_indexed.get() || tell);
        // Nothing in memory yet (just started): last run's index meanwhile.
        let cached: Option<fn(crate::files::Index)> =
            (self.files.borrow().len() == 0).then_some(|index| with_app(|a| a.files_cached(index)));
        crate::files::start(files, cached, || with_app(|a| a.files_progress()), |index| with_app(|a| a.files_indexed(index)));
        self.set_busy(true);
    }

    /// Deletes something Tendril keeps (Settings' Privacy & Data tab), on disk and in memory.
    /// "all" is everything but your settings. Says what was done.
    pub fn delete_data(&self, key: &str) -> Result<String, String> {
        let gone = |r: std::io::Result<()>| match r {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
        let said = match key {
            "ranking" => {
                let mut usage = self.usage.borrow_mut();
                usage.forget_all();
                usage.save();
                self.usage_changed.set(false);
                "Ranking reset. Favourites stay pinned."
            }
            "clipboard" => {
                let mut history = self.history.borrow_mut();
                history.clips.clear();
                history.save();
                "Clipboard history cleared."
            }
            "files" => {
                crate::files::forget().map_err(|e| e.to_string())?;
                *self.files.borrow_mut() = crate::files::Index::default();
                *self.indexed_with.borrow_mut() = None;
                self.indexed_at.set(None);
                "File index deleted. It's made again the next time Tendril opens."
            }
            "apps" => {
                gone(std::fs::remove_file(apps::cache_path()))?;
                "App list deleted. It's made again on the next start."
            }
            "icons" => {
                crate::sites::forget().map_err(|e| e.to_string())?;
                self.rebuild_catalog();
                "Website icons deleted."
            }
            "all" => {
                for key in ["ranking", "clipboard", "files", "apps", "icons"] {
                    self.delete_data(key)?;
                }
                "Everything but your settings deleted."
            }
            _ => return Err(format!("nothing called {key}")),
        };
        if let Some(ui) = self.ui.upgrade() {
            self.refresh(&ui.get_query());
        }
        Ok(said.into())
    }

    /// Last run's index, while the walk runs. Only taken if the walk hasn't finished first.
    fn files_cached(&self, index: crate::files::Index) {
        if crate::files::walking() && self.files.borrow().len() == 0 {
            *self.files.borrow_mut() = index;
            if let Some(ui) = self.ui.upgrade()
                && !ui.get_query().is_empty()
            {
                let keep = self.selected_item().map(|i| i.id);
                self.refresh_keeping(&ui.get_query(), keep.as_deref());
            }
        }
    }

    /// A few times a second while the walk runs: the count, where it's shown.
    fn files_progress(&self) {
        self.set_busy(true);
        crate::settings::show_files_status();
        let showing = self.items.borrow().iter().flatten().any(|i| matches!(i.run, Run::IndexFiles | Run::Setting("index")));
        if showing && let Some(ui) = self.ui.upgrade() {
            let keep = self.selected_item().map(|i| i.id);
            self.refresh_keeping(&ui.get_query(), keep.as_deref());
        }
    }

    fn files_indexed(&self, index: crate::files::Index) {
        let count = index.len();
        *self.files.borrow_mut() = index;
        self.set_busy(false);
        // The rules changed while it walked (in Settings, say): walk again with the new ones.
        if self.indexed_with.borrow().as_ref() != Some(&self.config.borrow().files) {
            self.index_files(false);
        }
        crate::settings::refresh();
        if let Some(ui) = self.ui.upgrade() {
            let keep = self.selected_item().map(|i| i.id);
            self.refresh_keeping(&ui.get_query(), keep.as_deref());
        }
        if self.tell_indexed.replace(false) {
            self.toast(&format!("Found {} files and folders", crate::files::thousands(count)), false);
        }
    }

    fn set_busy(&self, _: bool) {
        if let Some(ui) = self.ui.upgrade() {
            let label = if self.updating.get() {
                "Updating Tendril…".to_string()
            } else if self.scanning.get() {
                "Looking for apps…".to_string()
            } else {
                format!("Indexing files… {}", crate::files::thousands(crate::files::found_so_far()))
            };
            ui.set_busy_label(label.into());
            ui.set_busy(self.scanning.get() || self.updating.get() || crate::files::walking());
        }
    }

    /// Starts the thread that looks for new versions, if updates are on and it isn't running.
    pub fn start_update_checks(&self) {
        let on = self.config.borrow().updates != "off";
        UPDATES_ON.store(on, Ordering::Relaxed);
        if !on || self.update_thread.replace(true) {
            return;
        }
        let _ = std::thread::Builder::new().name("updates".into()).stack_size(512 * 1024).spawn(|| {
            std::thread::sleep(FIRST_UPDATE_CHECK);
            loop {
                if UPDATES_ON.load(Ordering::Relaxed) {
                    let result = update::check();
                    let _ = slint::invoke_from_event_loop(move || with_app(|a| a.update_checked(result, false)));
                }
                std::thread::sleep(UPDATE_EVERY);
            }
        });
    }

    /// "Check for Updates": now, and say what came of it.
    pub fn check_updates_now(&self) {
        if self.updating.replace(true) {
            return;
        }
        self.set_busy(true);
        let spawned = std::thread::Builder::new().name("update-check".into()).spawn(|| {
            let result = update::check();
            let _ = slint::invoke_from_event_loop(move || {
                with_app(|a| {
                    a.updating.set(false);
                    a.set_busy(false);
                    a.update_checked(result, true);
                })
            });
        });
        if spawned.is_err() {
            self.updating.set(false);
            self.set_busy(false);
        }
    }

    /// The Settings window changed something the catalog depends on (quicklinks, engines).
    pub fn settings_changed(&self) {
        *self.config.borrow_mut() = config::load();
        self.rebuild_catalog();
        if let Some(ui) = self.ui.upgrade()
            && ui.window().is_visible()
        {
            self.refresh(&ui.get_query());
        }
    }

    pub fn update_checked(&self, result: Result<update::Found, String>, asked: bool) {
        let ours = update::VERSION;
        let (mode, previews) = {
            let config = self.config.borrow();
            (config.updates.clone(), config.previews.clone())
        };
        if mode == "off" && !asked {
            return;
        }
        let update::Found { release, preview } = match result {
            Ok(found) => found,
            Err(e) => {
                if asked {
                    self.toast(&format!("Couldn't check for updates: {e}"), true);
                }
                return;
            }
        };
        // Following previews, whichever of the two is newer; otherwise releases only.
        let preview_wins = previews == "always"
            && preview.as_ref().is_some_and(|p| release.as_ref().is_none_or(|r| update::newer(&p.version, &r.version)));
        let candidate = if preview_wins { preview.clone() } else { release }.filter(|c| update::newer(&c.version, ours));
        // On "ask", the newest preview is offered when it's ahead of everything else.
        let offer = preview.filter(|p| {
            previews == "ask"
                && update::newer(&p.version, ours)
                && candidate.as_ref().is_none_or(|c| update::newer(&p.version, &c.version))
                && update::cannot_install(p).is_none()
        });
        *self.preview_offer.borrow_mut() = offer;

        let Some(release) = candidate else {
            if asked {
                self.toast(&format!("You're on the latest version ({ours})"), false);
            }
            self.refresh_if_root();
            return;
        };
        let known = match &*self.update.borrow() {
            UpdateState::Ready(v) | UpdateState::Installing(v) => *v == release.version,
            _ => false,
        };
        if known {
            if asked {
                self.toast(&format!("Tendril {} is already on its way", release.version), false);
            }
            self.refresh_if_root();
            return;
        }
        if mode == "auto" && update::cannot_install(&release).is_none() {
            self.install_update(release, asked);
        } else {
            if asked {
                self.toast(&format!("Tendril {} is out", release.version), false);
            }
            *self.update.borrow_mut() = UpdateState::Available(release);
            self.refresh_if_root();
        }
    }

    /// "Install Preview Version?": from now on previews, starting with this one.
    fn install_preview(&self) {
        let Some(preview) = self.preview_offer.borrow_mut().take() else { return };
        if let Err(e) = config::set(None, "previews", config::Value::Text("always")) {
            self.toast(&format!("Couldn't save the setting: {e}"), true);
            return;
        }
        self.config.borrow_mut().previews = "always".into();
        self.install_update(preview, true);
    }

    /// "Don't Offer Previews" on that row.
    fn decline_previews(&self) {
        match config::set(None, "previews", config::Value::Text("never")) {
            Ok(()) => {
                self.config.borrow_mut().previews = "never".into();
                self.preview_offer.borrow_mut().take();
                self.toast("Previews won't be offered. Appearance can turn them back on.", false);
                self.refresh_if_root();
            }
            Err(e) => self.toast(&format!("Couldn't save the setting: {e}"), true),
        }
    }

    fn install_update(&self, release: update::Release, asked: bool) {
        *self.update.borrow_mut() = UpdateState::Installing(release.version.clone());
        self.updating.set(true);
        self.set_busy(true);
        let spawned = std::thread::Builder::new().name("update-install".into()).spawn(move || {
            let result = update::install(&release);
            let _ = slint::invoke_from_event_loop(move || {
                with_app(|a| {
                    a.updating.set(false);
                    a.set_busy(false);
                    match result {
                        Ok(()) => {
                            a.toast(&format!("Updated to {}. It starts next time.", release.version), false);
                            *a.update.borrow_mut() = UpdateState::Ready(release.version.clone());
                        }
                        Err(e) => {
                            if asked {
                                a.toast(&format!("Couldn't update: {e}"), true);
                            }
                            *a.update.borrow_mut() = UpdateState::Available(release);
                        }
                    }
                    a.refresh_if_root();
                })
            });
        });
        if spawned.is_err() {
            self.updating.set(false);
            self.set_busy(false);
        }
    }

    /// Rebuilds the list if it's showing the root, where update rows live.
    fn refresh_if_root(&self) {
        if let Some(ui) = self.ui.upgrade()
            && ui.get_query().is_empty()
            && self.view.get() == View::Root
            && self.onboarding.get().is_none()
        {
            self.refresh("");
        }
    }

    /// The rows offering an update or a preview, at the top of the list before you type.
    fn update_lines(&self) -> Vec<Line> {
        let row = |title: String, subtitle: String, icon: &'static str, run: Run| Item {
            id: "update".into(),
            title,
            subtitle,
            keywords: String::new(),
            kind: Kind::Command,
            icon: crate::catalog::Icon::Builtin(icon),
            tint: 7,
            run,
            path: None,
        };
        let ours = update::VERSION;
        let mut items = Vec::new();
        match &*self.update.borrow() {
            UpdateState::Available(r) => items.push(match update::cannot_install(r) {
                None => {
                    let what = if r.preview { "Preview" } else { "Release" };
                    row(format!("Update to {}", r.version), format!("{what} · you have {ours}"), "download", Run::InstallUpdate)
                }
                Some(_) => {
                    row(format!("Tendril {} is out", r.version), "Open the release page".into(), "download", Run::Url(r.page.clone()))
                }
            }),
            UpdateState::Ready(v) => {
                items.push(row("Restart to Finish Updating".into(), format!("to {v}"), "download", Run::RestartToUpdate))
            }
            UpdateState::Quiet | UpdateState::Installing(_) => {}
        }
        if let Some(p) = &*self.preview_offer.borrow()
            && !matches!(&*self.update.borrow(), UpdateState::Installing(v) | UpdateState::Ready(v) if *v == p.version)
        {
            let kind = update::kind(&p.version).unwrap_or("preview");
            let mut item = row(
                "Install Preview Version?".into(),
                format!("{} · a {kind} build of the newest changes, before a release", p.version),
                "flask",
                Run::InstallPreview,
            );
            item.id = "update-preview".into();
            items.push(item);
        }
        if let Some(before) = &*self.updated_from.borrow() {
            let mut item = row(
                format!("What's New in {}", update::VERSION),
                format!("Tendril updated itself from {before}"),
                "sparkle",
                Run::Url(format!("https://github.com/devatloam/tendril/releases/tag/v{}", update::VERSION)),
            );
            item.id = "whats-new".into();
            items.insert(0, item);
        }
        if items.is_empty() {
            return Vec::new();
        }
        std::iter::once(Line::Section("Update".into())).chain(items.into_iter().map(Line::Item)).collect()
    }

    fn scanned(&self, mut found: Vec<Item>, tell: bool) {
        let count = found.len();
        found.sort_by_key(|a| a.title.to_lowercase());
        *self.apps.borrow_mut() = found;
        self.rebuild_catalog();
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
        self.refresh_keeping(query, None);
    }

    /// Rebuilds the list. `keep` is a row id to stay on, if it's still there.
    fn refresh_keeping(&self, query: &str, keep: Option<&str>) {
        let started = Instant::now();
        self.build_list(query, keep);
        let took = started.elapsed();
        // Typing should never wait on this; say so when it does.
        if took > Duration::from_millis(8) {
            eprintln!("tendril: listing \"{query}\" took {} ms", took.as_millis());
        }
    }

    fn build_list(&self, query: &str, keep: Option<&str>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let config = self.config.borrow().clone();
        let a = &config.appearance;
        let walkthrough = self.onboarding.get();
        let lines = if let Some(step) = walkthrough {
            self.onboarding_lines(step, &config)
        } else if self.view.get() == View::Appearance {
            self.appearance_lines(query, &config)
        } else if self.view.get() == View::Clipboard {
            self.clipboard_lines(query)
        } else if self.view.get() == View::Windows {
            self.window_lines(query)
        } else {
            let mut lines = if query.trim().is_empty() { self.update_lines() } else { Vec::new() };
            lines.extend(search::lines(
                query,
                &self.catalog.borrow(),
                &self.files.borrow(),
                &self.usage.borrow(),
                &config.search,
                &config.quicklink,
                &mut self.matcher.borrow_mut(),
            ));
            for line in &mut lines {
                if let Line::Item(item) = line {
                    self.decorate(item);
                }
            }
            lines
        };
        let compact = a.compact() && walkthrough.is_none();
        let details = a.details && !compact && walkthrough.is_none();
        let headings = a.headings || walkthrough.is_some();
        let list_width = if compact {
            620.0
        } else if details {
            300.0
        } else {
            750.0
        } - 16.0;
        let columns = if compact {
            5
        } else if details {
            3
        } else {
            6
        };
        let mut icons = self.icons.borrow_mut();
        let mut rows = Vec::with_capacity(lines.len());
        let mut items = Vec::with_capacity(lines.len());
        let mut y = if headings { 0.0 } else { 8.0 };
        let mut groups = 0;
        let row_for = |item: &Item, kind: i32, x: f32, y: f32, width: f32, height: f32, icons: &mut Icons| {
            let (icon, icon_on, tint) = icons.get(&item.icon, item.tint, if kind == 4 { crate::icons::TILE } else { crate::icons::ROW });
            let accessory = match (&item.run, item.kind) {
                // A setting's current value travels in `keywords` (see appearance_lines).
                (Run::Setting(_), _) | (_, Kind::Calculator) => item.keywords.as_str(),
                // The details pane says what it is; the narrow list doesn't need to.
                _ if details => "",
                (_, kind) => kind.label(),
            };
            Row {
                kind,
                x,
                y,
                width,
                height,
                title: item.title.as_str().into(),
                subtitle: item.subtitle.as_str().into(),
                accessory: accessory.into(),
                icon,
                icon_on,
                tint,
                quick: SharedString::new(),
            }
        };
        for line in lines {
            match line {
                Line::Section(title) => {
                    groups += 1;
                    if headings {
                        let height = if y == 0.0 { 30.0 } else { 38.0 };
                        rows.push(Row { kind: 0, y, height, title: title.into(), ..Default::default() });
                        items.push(None);
                        y += height;
                    } else if groups > 1 {
                        y += 8.0;
                    }
                }
                Line::Hero(title, body) => {
                    let height = if walkthrough == Some(0) { 300.0 } else { 280.0 };
                    rows.push(Row { kind: 7, y, height, title: title.into(), subtitle: body.into(), ..Default::default() });
                    items.push(None);
                    y += height;
                }
                Line::Empty(message) => {
                    rows.push(Row { kind: 3, y, height: 150.0, title: message.into(), ..Default::default() });
                    items.push(None);
                    y += 150.0;
                }
                Line::Favourites(mut favourites) if a.grid() => {
                    groups += 1;
                    // Whole rows of tiles only: a last tile alone on its row looks like a mistake.
                    // What you pinned stays, whatever that leaves.
                    if favourites.len() > columns {
                        let usage = self.usage.borrow();
                        while favourites.len() % columns != 0 && favourites.last().is_some_and(|i| !usage.is_pinned(&i.id)) {
                            favourites.pop();
                        }
                    }
                    if headings {
                        rows.push(Row { kind: 0, y, height: 30.0, title: "Favourites".into(), ..Default::default() });
                        items.push(None);
                        y += 30.0;
                    }
                    let tile = (list_width - TILE_GAP * (columns as f32 - 1.0)) / columns as f32;
                    for (k, item) in favourites.into_iter().enumerate() {
                        let (col, line) = ((k % columns) as f32, (k / columns) as f32);
                        let row =
                            row_for(&item, 4, col * (tile + TILE_GAP), y + line * (TILE_HEIGHT + TILE_GAP), tile, TILE_HEIGHT, &mut icons);
                        rows.push(row);
                        items.push(Some(item));
                    }
                    let lines_used = items.iter().rev().take_while(|i| i.is_some()).count().div_ceil(columns) as f32;
                    y += lines_used * (TILE_HEIGHT + TILE_GAP) - TILE_GAP;
                }
                Line::Favourites(favourites) => {
                    groups += 1;
                    if headings {
                        rows.push(Row { kind: 0, y, height: 30.0, title: "Suggestions".into(), ..Default::default() });
                        items.push(None);
                        y += 30.0;
                    }
                    for item in favourites {
                        let height = if compact { 34.0 } else { 40.0 };
                        rows.push(row_for(&item, 1, 0.0, y, 0.0, height, &mut icons));
                        items.push(Some(item));
                        y += height;
                    }
                }
                Line::Item(item) => {
                    let (kind, height) = match (&item.run, item.kind) {
                        (_, Kind::Calculator) => (2, 100.0),
                        (Run::Setting(_), _) => (5, 44.0),
                        (Run::Shortcut, _) => (6, 64.0),
                        _ if compact => (1, 34.0),
                        _ => (1, 40.0),
                    };
                    rows.push(row_for(&item, kind, 0.0, y, 0.0, height, &mut icons));
                    items.push(Some(item));
                    y += height;
                }
            }
        }
        drop(icons);
        // Ctrl 1–9 for the first nine things you can open.
        let mut quick = Vec::new();
        if a.quick_keys {
            for (i, item) in items.iter().enumerate() {
                if item.is_some() && quick.len() < 9 {
                    quick.push(i);
                    rows[i].quick = (quick.len()).to_string().into();
                }
            }
        }
        *self.quick.borrow_mut() = quick;
        let kept = keep.and_then(|id| items.iter().position(|i| i.as_ref().is_some_and(|i| i.id == id)));
        let first = kept.or_else(|| items.iter().position(Option::is_some)).map_or(-1, |i| i as i32);
        *self.items.borrow_mut() = items;
        ui.set_glide(false);
        self.rows.set_vec(rows);
        ui.set_rows_height(y);
        ui.set_selected(-1);
        ui.set_selected(first);
        if kept.is_none() {
            ui.invoke_reset_scroll();
        }
        self.update_bar(&ui);
    }

    /// One step of the walkthrough.
    fn onboarding_lines(&self, step: usize, config: &Config) -> Vec<Line> {
        let settings = |keys: &[&str]| -> Vec<Line> {
            let all = appearance::settings(config);
            keys.iter().filter_map(|k| all.iter().find(|s| s.key == *k)).map(setting_item).map(Line::Item).collect()
        };
        match step {
            0 => vec![Line::Hero("Tendril".into(), "A keyboard launcher. No web views, no tracking.".into())],
            1 => {
                let mut lines = vec![Line::Section("Open Tendril with".into()), Line::Item(shortcut_item())];
                lines.push(Line::Section("Starting and updating".into()));
                lines.extend(settings(&["login", "updates", "previews"]));
                lines.push(Line::Section("Clipboard".into()));
                lines.extend(settings(&["clipboard_history"]));
                lines
            }
            2 => std::iter::once(Line::Section("Look".into())).chain(settings(appearance::LOOK)).collect(),
            3 => std::iter::once(Line::Section("Layout".into())).chain(settings(appearance::LAYOUT)).collect(),
            _ => vec![Line::Hero("to open Tendril from anywhere".into(), "Type to find apps, do sums and run commands.".into())],
        }
    }

    /// The Appearance view's rows: every setting with its current value, and the file itself.
    fn appearance_lines(&self, query: &str, config: &Config) -> Vec<Line> {
        let query = query.trim().to_lowercase();
        let mut lines = vec![Line::Section("Shortcut".into())];
        if query.is_empty() || "shortcut keys hotkey open".contains(&query) {
            lines.push(Line::Item(shortcut_item()));
        }
        let all = appearance::settings(config);
        let shown = |s: &&appearance::Setting| query.is_empty() || format!("{} {}", s.title, s.about).to_lowercase().contains(&query);
        lines.push(Line::Section("Appearance".into()));
        lines.extend(all.iter().filter(|s| !appearance::FILES.contains(&s.key)).filter(shown).map(|s| Line::Item(setting_item(s))));
        lines.push(Line::Section("Files".into()));
        lines.extend(all.iter().filter(|s| appearance::FILES.contains(&s.key)).filter(shown).map(|s| Line::Item(setting_item(s))));
        // Folders and rules are lists, edited in the file.
        for item in crate::catalog::commands().into_iter().filter(|i| matches!(i.run, Run::IndexFiles | Run::SettingsFile)) {
            if query.is_empty() || format!("{} {}", item.title, item.keywords).to_lowercase().contains(&query) {
                let mut item = item;
                if matches!(item.run, Run::SettingsFile) {
                    item.title = "Folders and Rules".into();
                    item.subtitle = "Which folders, and what's left out, in the settings file".into();
                }
                self.decorate(&mut item);
                lines.push(Line::Item(item));
            }
        }
        lines.push(Line::Section("Everything else".into()));
        lines.push(Line::Item(settings_item()));
        lines
    }

    /// Clipboard History's rows: copies, newest first, narrowed by what's typed.
    fn clipboard_lines(&self, query: &str) -> Vec<Line> {
        if !self.config.borrow().clipboard_history {
            return vec![Line::Empty("Clipboard history is off. Appearance can turn it on.".into())];
        }
        let query = query.trim().to_lowercase();
        let history = self.history.borrow();
        let now = clipboard::now();
        let items: Vec<Item> = history
            .clips
            .iter()
            .filter(|c| query.is_empty() || c.text.to_lowercase().contains(&query))
            .map(|c| {
                let lines = c.text.lines().count();
                let size = if lines > 1 { format!("{lines} lines") } else { format!("{} characters", c.text.chars().count()) };
                Item {
                    id: format!("clip:{}", c.id),
                    title: clipboard::title(&c.text),
                    subtitle: format!("{} · {size}", ago(now.saturating_sub(c.at))),
                    keywords: String::new(),
                    kind: Kind::Clip,
                    icon: crate::catalog::Icon::Builtin(if clipboard::is_link(&c.text) { "link" } else { "clipboard" }),
                    tint: 7,
                    run: Run::Clip(c.id),
                    path: None,
                }
            })
            .collect();
        if items.is_empty() {
            let message =
                if query.is_empty() { "Nothing copied yet. Text you copy shows up here." } else { "Nothing you've copied matches" };
            return vec![Line::Empty(message.into())];
        }
        std::iter::once(Line::Section("Copied".into())).chain(items.into_iter().map(Line::Item)).collect()
    }

    /// Switch Windows' rows: open windows, most recently used first.
    fn window_lines(&self, query: &str) -> Vec<Line> {
        let query = query.trim().to_lowercase();
        let catalog = self.catalog.borrow();
        let items: Vec<Item> = self
            .windows
            .borrow()
            .iter()
            .filter(|w| query.is_empty() || format!("{} {}", w.title, w.app).to_lowercase().contains(&query))
            .map(|w| {
                // The app's own icon and name when it's one Tendril found; X11 names windows by
                // class ("firefox"), which usually matches the app's file or name.
                let app = catalog.iter().find(|i| {
                    i.kind == Kind::Application
                        && !w.app.is_empty()
                        && (i.title.eq_ignore_ascii_case(&w.app)
                            || i.id.to_lowercase().contains(&format!(":{}", w.app.to_lowercase()))
                            || i.path
                                .as_ref()
                                .and_then(|p| p.file_stem())
                                .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(&w.app)))
                });
                #[cfg(windows)]
                let fallback = w
                    .program
                    .as_ref()
                    .map_or(crate::catalog::Icon::Builtin("window"), |p| crate::catalog::Icon::Shell(p.to_string_lossy().into_owned()));
                #[cfg(not(windows))]
                let fallback = crate::catalog::Icon::Builtin("window");
                Item {
                    id: format!("window:{}", w.id),
                    title: if w.title.is_empty() { w.app.clone() } else { w.title.clone() },
                    subtitle: app.map_or_else(|| w.app.clone(), |a| a.title.clone()),
                    keywords: String::new(),
                    kind: Kind::Window,
                    icon: app.map_or(fallback, |a| a.icon.clone()),
                    tint: app.map_or(7, |a| a.tint),
                    run: Run::Window(w.id),
                    path: None,
                }
            })
            .collect();
        if items.is_empty() {
            let message = if query.is_empty() { "No other windows are open" } else { "No open window matches" };
            return vec![Line::Empty(message.into())];
        }
        std::iter::once(Line::Section("Open windows".into())).chain(items.into_iter().map(Line::Item)).collect()
    }

    fn selected_item(&self) -> Option<Item> {
        let ui = self.ui.upgrade()?;
        let i = usize::try_from(ui.get_selected()).ok()?;
        self.items.borrow().get(i).cloned().flatten()
    }

    fn update_bar(&self, ui: &Launcher) {
        let item = self.selected_item();
        // While the shortcut row is selected, the keys are let go of, so pressing them records
        // them instead of closing Tendril.
        let recording = item.as_ref().is_some_and(|i| matches!(i.run, Run::Shortcut));
        if recording != ui.get_recording() {
            ui.set_recording(recording);
            if let Some(hotkeys) = &*self.hotkeys.borrow() {
                if recording {
                    hotkeys.pause();
                    let refused = hotkeys.refused().is_some();
                    let note = match () {
                        _ if !hotkeys.available() => "Set this in your desktop's keyboard settings",
                        _ if refused => "Another app uses these keys. Press the ones you'd like instead",
                        _ => "Press the new keys",
                    };
                    self.show_shortcut(ui, note, !refused);
                } else {
                    hotkeys.resume();
                }
            }
        }
        ui.set_primary_label(item.as_ref().map_or("", |i| i.verb()).into());
        if self.config.borrow().appearance.details {
            ui.set_detail(item.map(|i| self.detail(&i)).unwrap_or_default());
        }
    }

    /// What the details pane shows for an item.
    fn detail(&self, item: &Item) -> Detail {
        let (icon, _, tint) = self.icons.borrow_mut().get(&item.icon, item.tint, crate::icons::DETAIL);
        let mut facts = Vec::new();
        let mut fact = |label: &str, value: String| facts.push(Fact { label: label.into(), value: value.into() });
        match &item.run {
            Run::App(launch) => {
                if let Some(runs) = apps::describe(launch) {
                    fact("Runs", runs);
                }
            }
            Run::Url(url) => fact("Opens", url.clone()),
            Run::Copy(answer) => fact("Copies", answer.clone()),
            Run::IndexFiles | Run::Setting("index") => {
                for (label, value) in crate::files::facts(&self.config.borrow().files) {
                    fact(label, value);
                }
            }
            Run::Clip(id) => {
                if let Some(clip) = self.history.borrow().get(*id) {
                    fact("Copied", ago(clipboard::now().saturating_sub(clip.at)));
                    fact("Lines", clip.text.lines().count().to_string());
                    fact("Characters", clip.text.chars().count().to_string());
                }
            }
            Run::Setting(key) => {
                let config = self.config.borrow();
                if let Some(s) = appearance::settings(&config).into_iter().find(|s| s.key == *key) {
                    fact("Now", s.label().to_string());
                    fact("Choices", s.choices.iter().map(|c| c.0.as_str()).collect::<Vec<_>>().join(", "));
                }
            }
            _ => {}
        }
        if let Some(path) = &item.path {
            fact("File", path.to_string_lossy().into_owned());
        }
        if matches!(item.kind, Kind::Application | Kind::Command) && !matches!(item.run, Run::Setting(_)) {
            match self.usage.borrow().stats(&item.id) {
                Some((count, last)) => {
                    fact("Opened", if count == 1 { "Once".into() } else { format!("{count} times") });
                    fact("Last opened", ago(crate::usage::now().saturating_sub(last)));
                }
                None => fact("Opened", "Not from Tendril yet".into()),
            }
        }
        // A copy shows its text where other rows show a description.
        let subtitle = match &item.run {
            Run::Clip(id) => self.history.borrow().get(*id).map(|c| {
                let mut text: String = c.text.chars().take(400).collect();
                if c.text.chars().count() > 400 {
                    text.push('…');
                }
                text
            }),
            _ => None,
        }
        .unwrap_or_else(|| item.subtitle.clone());
        let kind = match item.kind {
            Kind::Calculator => "Calculator",
            Kind::Clip => "Text",
            Kind::Fallback => "Web",
            k => k.label(),
        };
        Detail {
            icon,
            tint,
            title: item.title.as_str().into(),
            subtitle: subtitle.into(),
            kind: kind.into(),
            facts: ModelRc::new(VecModel::from(facts)),
        }
    }

    /// Ctrl and a digit: opens that row, when quick keys are on.
    fn quick_open(&self, key: &str) -> bool {
        let Some(n) = key.parse::<usize>().ok().filter(|n| (1..=9).contains(n)) else { return false };
        let Some(&row) = self.quick.borrow().get(n - 1) else { return false };
        self.activate(row as i32, false);
        true
    }

    /// ← and →: along the favourite tiles, or through a setting's choices.
    fn step(&self, delta: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let Some(item) = self.selected_item() else { return };
        if let Run::Setting(key) = item.run {
            self.change_setting(key, delta);
            return;
        }
        let tiles = self.tiles();
        if let Some(at) = tiles.iter().position(|&i| i == ui.get_selected() as usize) {
            let next = (at as i32 + delta).clamp(0, tiles.len() as i32 - 1) as usize;
            self.select(tiles[next] as i32, false);
        }
    }

    fn tiles(&self) -> Vec<usize> {
        let rows = &self.rows;
        (0..rows.row_count()).filter(|&i| rows.row_data(i).is_some_and(|r| r.kind == 4)).collect()
    }

    /// When changing `key` by `delta` would switch Tendril to drawing with the GPU (Modern style,
    /// or blur where only the GPU can see through): what to ask, and the icon to ask it with.
    pub fn gpu_question(&self, key: &'static str, delta: i32) -> Option<(String, &'static str)> {
        let before = self.config.borrow().appearance.clone();
        let mut after = before.clone();
        let next = appearance::settings(&self.config.borrow()).into_iter().find(|s| s.key == key).map(|s| s.next(delta).clone());
        match (key, next) {
            ("style", Some(config::Value::Text(v))) => after.style = v.to_string(),
            ("blur", Some(config::Value::Bool(v))) => after.blur = v,
            _ => {}
        }
        if !(window::wants_gpu(&after) && !window::wants_gpu(&before) && !window::is_gpu()) {
            return None;
        }
        let what = if key == "style" { "The Modern style" } else { "Blur, on this system," };
        Some((
            format!(
                "{what} needs Tendril to draw with your graphics card. That keeps more memory and power in use \
                 for as long as Tendril runs. If your computer feels slower afterwards, turn this back off. \
                 Tendril restarts to switch."
            ),
            if key == "style" { "sparkle" } else { "blur" },
        ))
    }

    /// Changes a setting, asking first when the change would switch to the GPU.
    fn change_setting(&self, key: &'static str, delta: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        if let Some((message, icon)) = self.gpu_question(key, delta) {
            ui.set_confirm_title("Draw with the GPU?".into());
            ui.set_confirm_message(message.into());
            ui.set_confirm_label("Turn On".into());
            ui.set_confirm_danger(false);
            ui.set_confirm_icon(self.icons.borrow_mut().builtin_solid(icon));
            self.pending_setting.set(Some((key, delta)));
            ui.set_actions_open(false);
            ui.set_confirm_open(true);
            return;
        }
        self.apply_setting(key, delta);
    }

    pub fn apply_setting(&self, key: &'static str, delta: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let before = self.config.borrow().clone();
        let Some(setting) = appearance::settings(&before).into_iter().find(|s| s.key == key) else { return };
        if let Err(e) = setting.step(delta) {
            self.toast(&format!("Couldn't save that: {e}"), true);
            return;
        }
        *self.config.borrow_mut() = config::load();
        self.start_update_checks();
        self.start_clipboard();
        self.apply_theme();
        self.apply_appearance();
        if self.config.borrow().files != before.files {
            self.index_files(false);
        }
        self.refresh_keeping(&ui.get_query(), Some(&format!("setting:{key}")));
        let after = self.config.borrow().appearance.clone();
        let renderer_changed = window::wants_gpu(&after) != window::wants_gpu(&before.appearance) && window::needs_restart(&after);
        if renderer_changed && self.onboarding.get().is_some() {
            self.restart_after_onboarding.set(true);
            self.toast("That changes when you finish", false);
        } else if renderer_changed {
            RESTART_TO_APPEARANCE.store(true, Ordering::SeqCst);
            self.toast("Restarting to change how Tendril draws…", false);
            RESTART.store(true, Ordering::SeqCst);
            Timer::single_shot(Duration::from_millis(700), || {
                let _ = slint::quit_event_loop();
            });
        }
    }

    fn hover(&self, i: i32, x: f32, y: f32) {
        let was = self.pointer.replace(Some((x, y)));
        // The first report after opening is where the pointer already was.
        if was.is_some_and(|(px, py)| (px - x).abs() + (py - y).abs() >= 1.0) {
            self.select(i, true);
        }
    }

    fn select(&self, i: i32, by_pointer: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        // A pointer resting where the window opens reports itself as a hover; only a pointer
        // moved after opening should take the selection off the first row.
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
        drop(items);
        if selectable.is_empty() {
            return;
        }
        let current = ui.get_selected();
        // On the tiles, ↑ and ↓ go a whole row of tiles at a time, then on to the list.
        let tiles = self.tiles();
        if delta.abs() == 1
            && let Some(at) = tiles.iter().position(|&i| i as i32 == current)
        {
            let columns = self.tile_columns(&tiles);
            let target = at as i32 + delta * columns as i32;
            let next = if (0..tiles.len() as i32).contains(&target) {
                tiles[target as usize] as i32
            } else if delta > 0 {
                selectable.iter().copied().find(|&i| !tiles.contains(&(i as usize))).unwrap_or(current)
            } else {
                *selectable.last().unwrap_or(&current)
            };
            self.select(next, false);
            return;
        }
        let at = selectable.iter().position(|&i| i == current).unwrap_or(0) as i32;
        let n = selectable.len() as i32;
        // Single steps wrap around the ends; page steps stop at them.
        let next = if delta.abs() == 1 { (at + delta).rem_euclid(n) } else { (at + delta).clamp(0, n - 1) };
        self.select(selectable[next as usize], false);
    }

    /// How many tiles sit side by side: the ones sharing the first tile's row.
    fn tile_columns(&self, tiles: &[usize]) -> usize {
        let y = |i: usize| self.rows.row_data(i).map_or(0.0, |r| r.y);
        tiles.first().map_or(1, |&first| tiles.iter().take_while(|&&i| y(i) == y(first)).count().max(1))
    }

    fn escape(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_query().is_empty() && self.view.get() != View::Root {
            self.leave_view(&ui);
        } else if ui.get_query().is_empty() {
            window::hide(&ui);
        } else {
            ui.set_query(SharedString::new());
            self.refresh("");
        }
    }

    /// Backspace with nothing typed: out of a view, back to the main list.
    fn back(&self) -> bool {
        let Some(ui) = self.ui.upgrade() else { return false };
        if self.view.get() == View::Root {
            return false;
        }
        self.leave_view(&ui);
        true
    }

    fn activate(&self, i: i32, alternate: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        let item = usize::try_from(i).ok().and_then(|i| self.items.borrow().get(i).cloned().flatten());
        let Some(item) = item else { return };
        if let Run::Setting(key) = item.run {
            self.change_setting(key, 1);
            return;
        }
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
            ui.set_confirm_danger(true);
            ui.set_confirm_icon(self.icons.borrow_mut().builtin_solid(match &item.icon {
                crate::catalog::Icon::Builtin(name) => name,
                _ => "power",
            }));
            *self.pending.borrow_mut() = Some(item.clone());
            ui.set_actions_open(false);
            ui.set_confirm_open(true);
            return;
        }
        if item.id == "whats-new" {
            self.updated_from.borrow_mut().take();
        }
        if record
            && item.id != "whats-new"
            && !matches!(item.kind, Kind::Calculator | Kind::File | Kind::Clip | Kind::Window)
            && !matches!(
                item.run,
                Run::Setting(_) | Run::Shortcut | Run::InstallUpdate | Run::InstallPreview | Run::RestartToUpdate | Run::Prefill(_)
            )
        {
            self.usage.borrow_mut().record(&item.id, &ui.get_query());
            self.usage_changed.set(true);
        }
        let outcome = match &item.run {
            Run::App(launch) => apps::launch(launch).map_err(|e| e.to_string()).map(|_| Done::Hide),
            Run::Power(power) => system::power(*power).map_err(|e| e.to_string()).map(|_| Done::Hide),
            Run::Url(url) => system::open(url).map_err(|e| e.to_string()).map(|_| Done::Hide),
            Run::Copy(text) => system::copy(text).map(|_| Done::Toast("Copied to clipboard".into(), true)),
            Run::Settings => {
                crate::settings::open(0);
                Ok(Done::Hide)
            }
            Run::SettingsFile => system::open(&config::path().to_string_lossy()).map_err(|e| e.to_string()).map(|_| Done::Hide),
            Run::Theme(key) => {
                self.config.borrow_mut().theme = key.to_string();
                self.apply_theme();
                let name = palettes::find(key).map_or(*key, |p| p.name);
                config::set(None, "theme", config::Value::Text(key))
                    .map_err(|e| e.to_string())
                    .map(|_| Done::Toast(format!("Theme set to {name}"), false))
            }
            Run::Appearance => {
                self.open_appearance();
                Ok(Done::Stay)
            }
            Run::Onboarding => {
                self.start_onboarding();
                Ok(Done::Stay)
            }
            Run::Shortcut => Ok(Done::Stay),
            Run::Path(path, true) => {
                // Into the folder: the field becomes its path, listing what's in it.
                let next = crate::paths::into(path);
                ui.set_query(next.as_str().into());
                ui.invoke_caret_to_end();
                self.refresh(&next);
                Ok(Done::Stay)
            }
            Run::Path(path, false) => system::open(&path.to_string_lossy()).map_err(|e| e.to_string()).map(|_| Done::Hide),
            Run::Prefill(text) => {
                ui.set_query(text.as_str().into());
                ui.invoke_caret_to_end();
                self.refresh(text);
                Ok(Done::Stay)
            }
            Run::CheckUpdates => {
                self.check_updates_now();
                Ok(Done::Stay)
            }
            Run::InstallUpdate => {
                let release = match &*self.update.borrow() {
                    UpdateState::Available(r) => Some(r.clone()),
                    _ => None,
                };
                if let Some(release) = release {
                    self.install_update(release, true);
                }
                Ok(Done::Stay)
            }
            Run::Clipboard => {
                self.open_view(View::Clipboard);
                Ok(Done::Stay)
            }
            Run::Windows => {
                self.open_view(View::Windows);
                Ok(Done::Stay)
            }
            Run::Clip(id) => {
                let text = self.history.borrow().get(*id).map(|c| c.text.clone());
                let Some(text) = text else { return };
                match system::copy(&text) {
                    Ok(()) if clipboard::can_paste() => {
                        window::hide(&ui);
                        clipboard::paste_soon();
                        Ok(Done::Stay)
                    }
                    Ok(()) => Ok(Done::Toast("Copied. Paste it where you like.".into(), true)),
                    Err(e) => Err(e),
                }
            }
            Run::Window(id) => match switcher::focus(*id) {
                Ok(()) => Ok(Done::Hide),
                Err(e) => Err(e),
            },
            Run::ClearClipboard => {
                {
                    let mut history = self.history.borrow_mut();
                    history.clips.clear();
                    history.save();
                }
                Ok(Done::Toast("Clipboard history cleared".into(), false))
            }
            Run::InstallPreview => {
                self.install_preview();
                Ok(Done::Stay)
            }
            Run::RestartToUpdate => {
                RESTART.store(true, Ordering::SeqCst);
                let _ = slint::quit_event_loop();
                Ok(Done::Stay)
            }
            Run::Setting(key) => {
                self.change_setting(key, 1);
                Ok(Done::Stay)
            }
            Run::Reload => {
                self.rescan(true);
                Ok(Done::Stay)
            }
            Run::IndexFiles => {
                *self.config.borrow_mut() = config::load();
                self.index_files(true);
                self.refresh_keeping(&ui.get_query(), Some(&item.id));
                Ok(Done::Stay)
            }
            Run::About => {
                let apps = self.app_count.get();
                ui.set_about_detail(
                    format!("Version {} · {apps} {} found", update::VERSION, if apps == 1 { "app" } else { "apps" }).into(),
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
        if let Some((key, delta)) = self.pending_setting.take() {
            if yes {
                self.apply_setting(key, delta);
            }
            return;
        }
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
            rows.push(ActionRow {
                title: title.into(),
                icon: icons.builtin(icon),
                icon_on: icons.builtin_solid(icon),
                keys: ModelRc::new(VecModel::from(keys)),
                danger,
            });
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
            Act::NoPreviews => self.decline_previews(),
            Act::Pin(_) => {
                let pinned = self.usage.borrow_mut().toggle_pin(&item.id);
                self.usage.borrow().save();
                self.toast(if pinned { "Added to favourites" } else { "Removed from favourites" }, false);
                self.refresh(&ui.get_query());
            }
            #[cfg(windows)]
            Act::RunAsAdmin(path) => match apps::launch_as_admin(&path) {
                Ok(()) => window::hide(ui),
                Err(e) => self.toast(&format!("Couldn't start it: {e}"), true),
            },
            #[cfg(not(windows))]
            Act::RunAsAdmin(_) => {}
            Act::CopyClip(id) => {
                let text = self.history.borrow().get(id).map(|c| c.text.clone());
                match text.map(|t| system::copy(&t)) {
                    Some(Ok(())) => window::hide(ui),
                    Some(Err(e)) => self.toast(&format!("Couldn't copy: {e}"), true),
                    None => {}
                }
            }
            Act::DeleteClip(id) => {
                {
                    let mut history = self.history.borrow_mut();
                    history.remove(id);
                    history.save();
                }
                self.refresh(&ui.get_query());
            }
            Act::ClearClips => {
                {
                    let mut history = self.history.borrow_mut();
                    history.clips.clear();
                    history.save();
                }
                self.toast("Clipboard history cleared", false);
                self.refresh(&ui.get_query());
            }
            Act::CloseWindow(id) => match switcher::close(id) {
                Ok(()) => {
                    self.windows.borrow_mut().retain(|w| w.id != id);
                    self.refresh(&ui.get_query());
                }
                Err(e) => self.toast(&format!("Couldn't close it: {e}"), true),
            },
        }
    }
}

enum Done {
    Hide,
    Toast(String, bool),
    Stay,
}

fn shortcut_item() -> Item {
    Item {
        id: "shortcut".into(),
        title: "Shortcut".into(),
        subtitle: String::new(),
        keywords: String::new(),
        kind: Kind::Command,
        icon: crate::catalog::Icon::Builtin("tendril"),
        tint: 7,
        run: Run::Shortcut,
        path: None,
    }
}

fn setting_item(setting: &appearance::Setting) -> Item {
    Item {
        id: format!("setting:{}", setting.key),
        title: setting.title.into(),
        subtitle: setting.about.clone(),
        // A setting's current value travels in `keywords` (see refresh_keeping).
        keywords: setting.label().into(),
        kind: Kind::Command,
        icon: crate::catalog::Icon::Builtin(setting.icon),
        tint: setting.tint,
        run: Run::Setting(setting.key),
        path: None,
    }
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
        Run::InstallPreview => acts.push(Act::NoPreviews),
        Run::Clip(id) => {
            if crate::clipboard::can_paste() {
                acts.push(Act::CopyClip(*id));
            }
            acts.push(Act::DeleteClip(*id));
            acts.push(Act::ClearClips);
        }
        Run::Window(id) => acts.push(Act::CloseWindow(*id)),
        _ => {}
    }
    if let Some(path) = &item.path {
        acts.push(Act::Reveal(path.clone()));
        acts.push(Act::Copy("path", path.to_string_lossy().into_owned()));
    }
    #[cfg(windows)]
    if item.kind == Kind::Application
        && let Some(path) = &item.path
        && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("lnk"))
    {
        acts.push(Act::RunAsAdmin(path.clone()));
    }
    if matches!(item.kind, Kind::Application | Kind::Command | Kind::Link) && !item.id.starts_with("cmd:ms-settings") {
        acts.push(Act::Pin(usage.is_pinned(&item.id)));
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
        Act::NoPreviews => ("Don't Offer Previews".into(), "eraser", keys, false),
        Act::CopyClip(_) => ("Copy".into(), "copy", keys, false),
        Act::DeleteClip(_) => ("Delete This Copy".into(), "eraser", keys, true),
        Act::ClearClips => ("Clear Clipboard History".into(), "eraser", keys, true),
        Act::CloseWindow(_) => ("Close Window".into(), "quit", keys, true),
        Act::Pin(false) => ("Add to Favourites".into(), "star", keys, false),
        Act::Pin(true) => ("Remove from Favourites".into(), "star", keys, false),
        Act::RunAsAdmin(_) => ("Run as Administrator".into(), "open", keys, false),
    }
}

/// "3 hours ago", from a number of seconds.
pub fn ago(seconds: u64) -> String {
    let (minutes, hours, days) = (seconds / 60, seconds / 3600, seconds / 86_400);
    match () {
        _ if minutes < 1 => "Just now".into(),
        _ if hours < 1 => format!("{minutes} minute{} ago", if minutes == 1 { "" } else { "s" }),
        _ if days < 1 => format!("{hours} hour{} ago", if hours == 1 { "" } else { "s" }),
        _ if days == 1 => "Yesterday".into(),
        _ => format!("{days} days ago"),
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// The version that ran last time, when it isn't this one: Tendril has been updated since. Each
/// run notes its own version in the data folder for the next to compare with.
fn updated_from() -> Option<String> {
    let path = dirs::data_dir()?.join("tendril").join("version");
    let before = std::fs::read_to_string(&path).ok().map(|v| v.trim().to_string());
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, update::VERSION);
    before.filter(|b| !b.is_empty() && b != update::VERSION && update::newer(update::VERSION, b))
}
