# GitLumina TUI

A terminal-based Git client inspired by GitLumina, built with Rust.

## Features

- **Commit Graph** — Visual commit history with branch labels, merge indicators, and color-coded graph lines
- **File Staging** — Stage/unstage files (single or all), view diffs, write commit messages
- **Branch Management** — Create, checkout, and delete branches
- **Push / Pull / Fetch** — Full remote operations with SSH key and credential helper support
- **Mouse Support** — Click to select commits, files, branches, and tabs
- **Scrollable Diff** — Mouse wheel and PageUp/PageDn scroll through diffs with a scrollbar indicator

## Prerequisites

- [Rust toolchain](https://rustup.rs/) (1.70+)
- `libgit2` dev headers:
  - **Ubuntu/Debian**: `sudo apt install libgit2-dev pkg-config`
  - **macOS**: bundled automatically via `git2` crate
  - **Arch**: `sudo pacman -S libgit2`
- A Git repository to open

## Build & Run

```bash
cd git-lumina
cargo build --release

# Run inside any Git repository
cd /path/to/your/repo
/path/to/git-lumina/target/release/git-lumina
```

## Keybindings

### Global

| Key         | Action            |
| ----------- | ----------------- |
| `1` `2` `3` | Switch tabs       |
| `Tab`       | Next tab          |
| `Shift+Tab` | Previous tab      |
| `↑`/`k`     | Move up           |
| `↓`/`j`     | Move down         |
| `P`         | Push to remote    |
| `L`         | Pull from remote  |
| `F`         | Fetch from remote |
| `r`         | Refresh all data  |
| `?`         | Toggle help popup |
| `q`         | Quit              |
| `Ctrl+C`    | Force quit        |

### Files Tab

| Key           | Action                  |
| ------------- | ----------------------- |
| `←`/`→`       | Cycle pane focus        |
| `s`           | Stage selected file     |
| `u`           | Unstage selected file   |
| `S`           | Stage all files         |
| `U`           | Unstage all files       |
| `c`           | Start typing commit msg |
| `Enter`       | Commit (in commit pane) |
| `Esc`         | Cancel editing          |
| `PgUp`/`PgDn` | Scroll diff view        |

### Branches Tab

| Key     | Action                   |
| ------- | ------------------------ |
| `Enter` | Checkout selected branch |
| `n`     | Create new branch        |
| `d`     | Delete selected branch   |

### Mouse

| Action           | Effect                                |
| ---------------- | ------------------------------------- |
| Left click       | Select commits, files, branches, tabs |
| Scroll wheel     | Scroll diff view or navigate lists    |
| Click commit msg | Focus commit input and start editing  |

## Architecture

```
src/
├── main.rs       # Entry point, terminal setup, event loop, mouse routing
├── app.rs        # Application state, click regions, business logic
├── ui.rs         # All rendering (ratatui widgets, scrollbar, popups)
└── git_ops.rs    # Git operations (git2 wrapper + push/pull/fetch)
```

The architecture cleanly separates concerns:

- **`git_ops`** — Pure Git operations including remote push/pull/fetch with credential discovery
- **`app`** — State machine with click region tracking for mouse support
- **`ui`** — Pure rendering function of `App` state → screen output, with diff scrollbar
- **`main`** — Event loop handling keyboard, mouse clicks, and scroll events

## Remote Operations

Push/pull uses `git2`'s native transport with automatic credential discovery:

1. **SSH Agent** — Tries `ssh-agent` first
2. **SSH Keys** — Falls back to `~/.ssh/id_ed25519`, `id_rsa`, `id_ecdsa`
3. **Credential Helper** — Uses Git's configured credential helper for HTTPS

Pull performs a **fast-forward only** merge. If the branch has diverged, it will tell you to merge or rebase manually.

## Dependencies

| Crate       | Purpose                         |
| ----------- | ------------------------------- |
| `ratatui`   | TUI framework (widgets, layout) |
| `crossterm` | Terminal I/O + mouse events     |
| `git2`      | Rust bindings for libgit2       |
| `anyhow`    | Error handling                  |
| `chrono`    | Timestamp formatting            |
