# 0002 — yt-dlp as a managed sidecar instead of a pure-Rust extractor

- **Status**: accepted (2026-11-13)

## Context
YouTube changes its anti-download measures (signature ciphers, PO tokens, SABR streaming) several times a year. Pure-Rust extractors (`rusty_ytdl`, `rustypipe`) lag behind and break; writing our own extractor would be a permanent maintenance burden. Perceived speed comes from the UI and parallel downloads, not from the extractor.

## Decision
Drive the official `yt-dlp` standalone Linux binary as a child process. The app downloads a pinned release to `$XDG_DATA_HOME/riptide/bin/yt-dlp`, verifies its SHA-256 against the release's `SHA2-256SUMS`, and offers an in-app update. Conversion and tagging use the system `ffmpeg`. GPUI is the UI framework (GPU-rendered, native Rust).

## Consequences
- Robust to YouTube changes: an update is a binary swap, no rebuild.
- Requires `ffmpeg` on the system and network access on first run.
- Progress comes from parsing yt-dlp's `--progress-template` output, kept in one tested module.
- Linux only for now; the engine stays portable.
