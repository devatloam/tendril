# Tendril

A keyboard launcher in the style of Raycast, by Loam (alongside Blight, the Discord bot). Rust, with the UI in
Slint. The point of it is to be light: no web view, no GPU unless blur needs one, nothing running while it's closed.

## Commands

```bash
cargo run                      # debug build, opens the window
cargo test                     # unit tests live beside the code, in #[cfg(test)] modules
cargo clippy --all-targets     # CI runs it with -D warnings, on Linux, Windows and macOS
cargo fmt                      # rustfmt.toml: 140 columns
python3 scripts/icons.py       # redraws ui/icons/ from the descriptions in that script
```

Windows and macOS can be checked from Linux: `rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin`, then
`cargo clippy --all-targets --target <triple>`. Do that after touching anything behind `#[cfg(windows)]` or
`#[cfg(target_os = "macos")]`; nothing else exercises that code before CI.

## Where things are

- `ui/*.slint`: the window. `app.slint` is the launcher and its keyboard handling, `parts.slint` the pieces,
  `motion.slint` the tumbling shapes, glow and loading line, `theme.slint` the colour tokens Rust fills in.
- `src/app.rs`: the state behind the window (list, selection, actions, Appearance view, first run, updates).
- `src/apps/{linux,windows,macos}.rs`: finding and launching apps, one module per system with the same functions.
- `src/window.rs`: showing, placing, hiding, and asking the system for blur.
- `src/config.rs`: config.toml. The Appearance view writes it one line at a time with `config::set`, keeping comments.
- README.md says what each of the other files is for.

## Rules that keep it light

- **Nothing runs while closed.** Hiding destroys the window and clears focus (which stops the caret timer). No thread
  polls: the hotkey, socket and event loop threads block until something happens. Check with
  `strace -f -p <pid> -e trace=epoll_pwait` while hidden: about one wakeup in five seconds is the baseline.
- **Animations exist only while on screen.** Anything that moves reads `animation-tick()` and sits behind an `if`.
  The CPU renderer can't rotate elements; turned shapes are paths (see `Shape` in motion.slint).
- **The CPU renderer is the default.** The GPU one (femtovg) is only for blur where X11 isn't available, because it
  holds tens of megabytes more for as long as Tendril runs.
- **Icons are loaded when a row first shows them** and scaled to 64px before they're kept.
- **New dependencies need a reason.** Prefer what's already in the tree (x11rb, windows, image).
- **Gradients with more than two stops** leave a gap in the CPU renderer: build them from two-stop pieces
  (`Spectrum` in motion.slint).

## Look and words

- White leads, blue highlights (`Theme.highlight`): the selected row and its icon, Tendril's mark. Other colours
  only where they mean something: red for destructive, green for done. Same rule as Blight's dashboard.
- Icons are drawn on Lucide's 24 grid with an outline and a solid for the selected row. Add one by describing it in
  `scripts/icons.py`, running it, and listing it in `src/icons.rs`.
- Write like Blight: plain words from the user's side, short sentences, no marketing. Comments say why, not what.
  No AI features.

## Trying it without a desktop

Under Xvfb, with picom for blur and a wallpaper window behind:

```bash
Xvfb :99 -screen 0 1920x1080x24 &
DISPLAY=:99 picom --backend xrender --blur-background --blur-method gaussian --blur-size 21 &
DISPLAY=:99 feh --geometry 1920x1080+0+0 --borderless wallpaper.png &
DISPLAY=:99 cargo run
```

There's no window manager, so give the window keyboard focus yourself (python-xlib's `set_input_focus`), and set
`hide_on_blur = false` in config.toml so it doesn't close when focus moves. `libxkbcommon-x11-0` must be installed.

## Releasing

Push a tag like `v0.2.0` (matching `version` in Cargo.toml). `.github/workflows/release.yml` builds every system and
attaches archives and bare binaries to a GitHub release; the in-app updater reads that release.
