# Riptide

Linux desktop app (Rust + GPUI) to download audio/video from YouTube links and playlists, with a warm retro-vinyl look and spring animations. Personal use.

## Stack
- Rust 1.98 (edition 2024), GPUI (`gpui` crate from crates.io, Zed), Linux only (Wayland + X11).
- Download engine: `yt-dlp` standalone binary managed by the app (pinned version + SHA-256, downloaded to `$XDG_DATA_HOME/riptide/bin/` on first run, updatable from the app). Never bundled in the repo.
- Conversion and tagging: system `ffmpeg` (required, checked at startup).
- Async: GPUI executors (`cx.background_spawn`); child processes via `std::process` / `smol`-compatible APIs. No tokio unless an ADR says so.

## Commands
- Run: `cargo run`
- Release build: `cargo build --release`
- Test: `cargo test`
- Verify (must pass before any commit): `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
- Format: `cargo fmt`

## Architecture
- `src/main.rs` — app bootstrap, window, theme.
- `src/engine/` — domain, no UI: yt-dlp management (install/update/verify), metadata probing (`yt-dlp -J --flat-playlist`), download jobs, progress parsing, format presets.
- `src/ui/` — GPUI views (input bar, preview card, track list, queue, settings) and the theme tokens (`ui/theme.rs`).
- `src/store/` — settings and history persisted as JSON in `$XDG_CONFIG_HOME/riptide/` / `$XDG_DATA_HOME/riptide/`.
- Engine talks to UI through typed events; the UI never parses yt-dlp output.

## Conventions
- Code, identifiers, comments, commits: English. UI text: French.
- Naming per `~/.claude/rules/dev-standards.md` (explicit names, `snake_case`, no `get_` prefix).
- All colors, radii, spacings, durations and spring constants are tokens in `src/ui/theme.rs`. No literal colors in views.
- Errors: `thiserror` enums in the engine, shown to the user as readable French messages. No `unwrap`/`expect` outside tests and commented invariants.

## Rules that must never be broken
- Never pass user input to a shell: spawn `yt-dlp`/`ffmpeg` with an argument vector, and pass URLs after `--`.
- Validate URLs (http/https, YouTube hosts) before spawning anything.
- Verify the SHA-256 of any downloaded `yt-dlp` binary before marking it executable.
- Never block the UI thread: all I/O and process work on background executors.
- Downloads write to a temp name and rename on success; cancellation cleans partial files.

## Git
- `main` stays stable. One branch per task (`feat/…`, `fix/…`), Conventional Commits.
- Agents may push branches and open PRs on `github.com/Paul-Carouge/riptide` (private). The lead merges after `code-reviewer` approves and verify is green. Never `--force`.

## Verification
"Done" = verify passes AND the real window was launched and looked at (screenshot of the running app, e.g. `grim` on Wayland) for every UI change, AND a real download of a short video to MP3 and MP4 succeeds for engine changes.

## Pointers
`ROADMAP.md` (milestones) · `PROGRESS.md` (session log, newest first) · `docs/adr/` (decisions) · `docs/design/DESIGN.md` (art direction, tokens).
