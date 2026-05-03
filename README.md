# Latch

Latch is a local-first personal bookmark service. The core data source is a single `jsonl` file, served by a local Rust HTTP service and accessed by the browser extension and Raycast extension through `127.0.0.1`.

This project is currently optimized for personal use and local distribution: the CLI is installed through a Homebrew tap, while the Chrome and Raycast clients are downloaded as prebuilt GitHub Release assets instead of being submitted to extension marketplaces.

## Features

- Local bookmark CRUD, search, tag stats, and open-count tracking
- Single-file `latch.jsonl` storage with optional iCloud Drive location
- iCloud conflict-copy reconciliation on startup and file changes
- macOS LaunchAgent service management
- Chrome MV3 extension for saving, searching, updating, and deleting bookmarks
- Raycast extension for searching, adding, and browsing bookmarks by tag
- Browser bookmarks HTML import
- Local release script that defaults to Apple Silicon CLI builds

## Installation

```bash
brew install iashc/tap/latch
latch --version
```

Install and start the background service:

```bash
latch service install --force
latch status
latch doctor
```

The service listens on:

```text
http://127.0.0.1:52525
```

Common service commands:

```bash
latch start
latch stop
latch restart
latch logs -n 80
latch service status
```

## Configuration and Data

Default config path:

```text
~/.config/latch/config.toml
```

When `XDG_CONFIG_HOME` is set, the config path becomes `$XDG_CONFIG_HOME/latch/config.toml`.

Default runtime, cache, client package, and log directory:

```text
~/.latch
```

When `LATCH_HOME` is set, runtime files use that directory instead.

Default data file:

```text
~/.latch/data/latch.jsonl
```

Switch to iCloud Drive storage:

```bash
latch config use-icloud
latch restart
```

Switch back to local storage:

```bash
latch config use-local
latch restart
```

Show the current config:

```bash
latch config show
```

## Clients

Download and install the Chrome extension package:

```bash
latch chrome update
latch chrome path
```

Open `chrome://extensions`, enable Developer Mode, then load the directory printed by `latch chrome path`.

Download and install the Raycast extension package:

```bash
latch raycast update
latch raycast path
```

Import the printed directory with Raycast's `Import Extension` command. The package is prebuilt and should contain `search.js`, `add.js`, and `tags.js` at the extension root.

Use `--force` to re-download prebuilt packages:

```bash
latch chrome install --force
latch raycast install --force
```

## Import Browser Bookmarks

Export a Netscape bookmarks HTML file from Chrome, Edge, Firefox, or Safari, then import it:

```bash
latch import browser-html /path/to/bookmarks.html
```

The importer skips non-`http` / `https` URLs and deduplicates using the server-side URL normalization rules.

## Local Development

Server:

```bash
cd server
cargo run -- serve
cargo test
```

Browser extension:

```bash
cd browser
npm ci
npm run dev
npm run compile
npm run build
```

Raycast extension:

```bash
cd raycast
npm ci
npm run dev
npm run lint
npm run build
```

See [openapi.yaml](openapi.yaml) for the API contract and [ARCHITECTURE.md](ARCHITECTURE.md) for the system design reference.

## Release

Releases are built locally to avoid consuming GitHub Actions minutes. The default target is Apple Silicon:

```bash
scripts/release.sh --version v0.1.0
scripts/release.sh --version v0.1.0 --publish
```

To update a local Homebrew tap checkout during release:

```bash
LATCH_HOMEBREW_TAP_PATH=/absolute/path/to/homebrew-tap \
  scripts/release.sh --version v0.1.0 --publish --tap-commit
```

Release assets are written to `dist/release/<tag>` and include the CLI tarball, Chrome zip, Raycast zip, release manifest, and generated Homebrew formula.

## Project Structure

```text
latch/
├── server/       # Rust CLI + Axum HTTP service
├── browser/      # WXT + Vue 3 browser extension
├── raycast/      # Raycast extension
├── shared/       # Shared TypeScript types
├── scripts/      # Local release tooling
├── openapi.yaml  # REST API contract
└── ARCHITECTURE.md
```
