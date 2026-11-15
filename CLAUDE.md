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
bun test scripts               # the release script's tests (Bun only, no install step)
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
- `src/config.rs`: config.toml. Settings are written one line at a time with `config::set`, and the search engine and
  quicklink lists with `config::set_list`, both keeping comments.
- `ui/settings.slint` + `src/settings.rs`: the Settings window, made when opened and dropped when closed. Its rows come
  from `appearance::settings`, the same list the launcher's Appearance view uses; changes go through
  `App::apply_setting`, so both behave the same (GPU question, restart).
  It's made inside `window::plain`, so it doesn't get the launcher's window attributes (on top, out of the taskbar),
  and draws its own frame and title bar where `window::settings_frame` says the system's would look out of place.
- README.md says what each of the other files is for.

## Rules that keep it light

- **Nothing runs while closed.** Hiding destroys the window (except on Windows, where it's kept and hidden, as Flow
  Launcher does: a fresh window there opens slowly and often isn't allowed focus) and clears focus (which stops the
  caret timer). No thread polls: the hotkey, socket and event loop threads block until something happens. Check
  with `strace -f -p <pid> -e trace=epoll_pwait` while hidden: about one wakeup in five seconds is the baseline.
- **Animations exist only while on screen.** Anything that moves reads `animation-tick()` and sits behind an `if`.
  The CPU renderer can't rotate elements; turned shapes are paths (see `Shape` in motion.slint).
- **The CPU renderer is the default.** The GPU one (femtovg) is only for blur where X11 isn't available, because it
  holds tens of megabytes more for as long as Tendril runs.
- **Icons never load on the UI thread** unless they're vectors (SVG, Tendril's glyphs). PNG/ICO/icns files and Windows
  shell icons go to the icon thread (icons.rs), are scaled there with Lanczos to the exact pixels they're shown at
  (`icons::ROW`/`TILE`/`DETAIL` times the window's scale), and patched into the rows when they arrive. The CPU renderer
  doesn't smooth scaled images, so an icon drawn at any other size looks ragged. On Windows each shell icon can take
  tens of milliseconds; loading them inline is what made typing stall.
- **Anything that switches to the GPU renderer asks first** (`change_setting`), with the neutral confirm dialog.
- **New dependencies need a reason.** Prefer what's already in the tree (x11rb, windows, image).
- **Gradients with more than two stops** leave a gap in the CPU renderer: build them from two-stop pieces
  (`Spectrum` in motion.slint).

## Look and words

- Anything with its own icon keeps it, in its own colours: apps, files (icon theme / Windows shell), web rows (the
  default browser's, browser.rs), quicklinks (the site's, sites.rs). Builtin glyphs are for Tendril's own commands.
- The selected row is a white wash by default (Raycast-like); blue is for Tendril's mark and small accents. Other
  colours only where they mean something: red for destructive, green for done. Appearance's Style/Selection/Icons
  (`Theme.modern`, `blue-selection`, `bold-icons`) let people choose more. Flat style uses flat fills only: the CPU
  renderer doesn't round gradients. Gradients belong behind `Theme.modern`, which is only true on the GPU renderer.
- Icons are drawn on Lucide's 24 grid with an outline and a solid for the selected row. Add one by describing it in
  `scripts/icons.py`, running it, and listing it in `src/icons.rs`.
- Write like Blight: plain words from the user's side, short sentences, no marketing. Comments say why, not what.
  No AI features.

## Trying it on Windows without Windows

`.github/workflows/smoke.yml` runs `scripts/smoke.ps1` on a GitHub Windows runner, which has a real desktop: first
start, recording a shortcut, typing fast (timed through UI Automation), clicking away, the shortcut, starting it again,
clipboard history. It runs on pushes that touch src/ or ui/ and by hand. Read the results without downloading anything:
`curl https://raw.githubusercontent.com/devatloam/tendril/smoke-results/smoke/report.txt`, and the screenshots beside
it (`01-first-start.png` …). Run it after changing anything Windows-specific; Clippy only proves it compiles. On Windows
Tendril logs to `%LOCALAPPDATA%\tendril\tendril.log`.

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

- On Windows, people install with the setup.exe that `build.yml` makes from `packaging/tendril.iss` (Inno Setup, per
  user, no admin). Its AppId is in the updater too (`update::installed_version`), to keep the version Settings → Apps
  shows in step; change both or neither.

- `bun run bump patch|minor|major` (scripts/release.ts) sets Cargo.toml and Cargo.lock, commits "Release vX.Y.Z" and
  tags it. Don't edit the version by hand. `--dry-run` shows the version and notes without changing anything.
- A release is published by `.github/workflows/release.yml`, started in one of three ways:
  1. **Push the tag** (on your own machine): `bun run bump minor --push`, or `git push --atomic origin HEAD vX.Y.Z`.
  2. **Run the workflow from GitHub**, where tags can't be pushed: delete the local tag (`git tag -d vX.Y.Z`), push the
     bump commit to main, then Actions → Release → Run workflow on main, or `gh workflow run release.yml --ref main`.
     It checks Cargo.toml holds the version and that it isn't released yet, then makes the tag itself.
  3. **From a Claude cloud session**: tag pushes are refused there (the git proxy answers 403 on any tag, whatever
     permission was given), so do 2 with the GitHub tools: push the bump commit to main, then `actions_run_trigger`
     with `run_workflow`, `workflow_id: release.yml`, `ref: main` and `inputs: {"version": "X.Y.Z"}`. Don't retry the
     tag push.
- Nightlies come from `.github/workflows/nightly.yml`, once a night (03:17 UTC) or by "Run workflow": the newest commit
  on main that passed `check.yml`, skipped when it's already in a nightly or release, published as a prerelease
  `vX.Y.Z-nightly.<date>.<run>` where X.Y.Z is the next patch after the last release. CI passes the version in
  `TENDRIL_VERSION`; use `update::VERSION`, never `CARGO_PKG_VERSION`. `build.yml` builds and packages for both, with the
  same release profile.
- Release notes come from commit messages (T3 Code's layout): the subject is the bullet, the first sentence of a prose
  body is its detail, `fix:` counts as a fix, `docs:`/`chore:`/`ci:`/`test:`/`build:`/`style:`/`refactor:` only appear in
  the folded full changelog. Write subjects as what someone using Tendril gets. `notes/vX.Y.Z.md`, if present, replaces the
  bullets for that release.
- Version order (src/update.rs `compare`, scripts/release.ts `compareVersions`) must agree: a preview of 0.2.1 sorts
  after 0.2.0 and before 0.2.1.
