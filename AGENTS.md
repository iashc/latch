# AGENTS.md

## Project Overview

Latch is a local-first personal bookmark system. The Rust CLI runs a localhost HTTP service backed by a single JSONL data file, while the browser extension and Raycast extension act as personal clients.

Respond to the user in Chinese for this repository.

## Setup

Run server setup from `server/`:

```bash
cd server
cargo build
cargo test
```

Run browser extension setup from `browser/`:

```bash
cd browser
npm ci
npm run compile
```

Run Raycast extension setup from `raycast/`:

```bash
cd raycast
npm ci
npm run lint
```

## Build & Development

Use these commands for daily development:

```bash
cd server && cargo run -- serve
cd browser && npm run dev
cd raycast && npm run dev
```

Build release-mode local artifacts with:

```bash
scripts/release.sh --version v0.1.0
```

Publishing is explicit:

```bash
scripts/release.sh --version v0.1.0 --publish
```

Pass a Homebrew tap checkout explicitly with `--tap-path` or `LATCH_HOMEBREW_TAP_PATH`; do not infer it from the parent directory layout.

## Testing

Run the Rust unit tests:

```bash
cd server
cargo test
```

Run browser type and WXT checks:

```bash
cd browser
npm run compile
```

Run Raycast validation:

```bash
cd raycast
npm run lint
```

For CLI behavior changes, run commands under temporary `HOME`, `XDG_CONFIG_HOME`, and `LATCH_HOME` values when possible. Avoid mutating the user's real `~/.config/latch`, `~/.latch`, iCloud data file, or LaunchAgent unless the task explicitly asks for it.

## Architecture

- `server/` contains the Rust CLI, Axum routes, JSONL store, search ranking, iCloud conflict reconciliation, GitHub Release client downloads, and macOS LaunchAgent management.
- `browser/` contains the WXT + Vue 3 browser extension. The MVP intentionally avoids context menu behavior.
- `raycast/` contains the Raycast extension. Raycast commands use React because the Raycast extension API requires it.
- `shared/` contains TypeScript types shared by client code.
- `openapi.yaml` is the authoritative REST API contract.
- `ARCHITECTURE.md` is the detailed design reference.

## Code Style

Follow existing module boundaries instead of adding new abstractions early. Keep server-side URL normalization, tag normalization, soft-delete behavior, and import semantics centralized in the Rust model/store layers.

Client package commands must download GitHub Release assets, verify SHA-256 from `latch-release-manifest.json`, cache under `~/.latch/cache`, and install unpacked clients under `~/.latch/clients`.

Runtime side effects belong under `~/.latch`; user-editable configuration belongs under `~/.config/latch`.

## Release Notes

The release script builds locally to avoid GitHub Actions minutes. The default CLI target is `aarch64-apple-darwin` for Apple Silicon Macs. Do not add GitHub Release CI unless the user explicitly changes that distribution model.

The Homebrew formula is generated into `dist/release/<tag>/homebrew/latch.rb`; copying, committing, or pushing it requires an explicit tap checkout path.

## Common Pitfalls

- `latch service start`, `stop`, and `restart` call macOS `launchctl`; use fake commands or isolated environments for automated tests.
- `latch serve` defaults to port `52525`; tests should choose a free port through an isolated config file.
- iCloud sync uses `~/Library/Mobile Documents/com~apple~CloudDocs/latch/latch.jsonl`; Finder visibility and shell visibility can differ because iCloud Drive is a file provider location.
- Browser and Raycast client commands may hit GitHub. Use pinned `--version` values for reproducible checks.
- The Raycast release package must contain compiled command executables at the package root, such as `search.js`, `add.js`, and `tags.js`; packaging only the TypeScript source causes Raycast's "Could not find command's executable JS file" error.
- `dist/`, `server/target/`, `browser/.output/`, `browser/.wxt/`, and `node_modules/` are generated artifacts.
