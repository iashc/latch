# Latch — Architecture Reference

Single `.jsonl` file as the sole data source. No database, no external runtime.

## 1. System Overview

```
┌──────────────────────────────────────────────┐
│                  Clients                      │
│  Browser Extension        Raycast Extension   │
│  (WXT, Manifest V3)      (TypeScript)         │
│       REST ──────────┬──────── REST           │
└──────────────────────┼────────────────────────┘
                       │
         ┌─────────────▼──────────────┐
         │     latch serve             │
         │     (Rust + Axum)          │
         │                            │
         │  Vec<Bookmark>   in-memory │
         │  HashMap<id>     lookup    │
         │  HashMap<url>    dedup     │
         │         │                  │
         │  Atomic file I/O           │
         │         │                  │
         │  iCloud conflict merge     │
         └─────────┬──────────────────┘
                   │
         ┌─────────▼──────────────────┐
         │  config.toml  (~/.config)  │
         │  latch.jsonl  (~/.latch)   │
         └─────────┬──────────────────┘
                   │
           iCloud / Syncthing / custom path
```

## 2. Configuration

Config path: `$XDG_CONFIG_HOME/latch/config.toml`, fallback `~/.config/latch/config.toml`.
Runtime/cache/log/client files live under `~/.latch`.

```toml
# Data file path. Default: ~/.latch/data/latch.jsonl
# data_file = "/Users/me/Library/Mobile Documents/com~apple~CloudDocs/latch/latch.jsonl"

# Listen port. Default: 52525
# port = 52525

# Bind address is fixed to 127.0.0.1 (not configurable).

# Log level: error, warn, info, debug, trace. Default: info
# log_level = "info"
```

All fields are optional; missing fields use defaults. Unknown fields are ignored with a warning log.

On startup, missing config/data directories and an empty `latch.jsonl` are created automatically.

## 3. Data Format

`latch.jsonl` — one JSON object per line:

```jsonl
{"id":"01J5KA3X...","url":"https://rust-lang.org","title":"Rust","description":"","tags":["rust"],"open_count":12,"last_opened":"2026-04-20T10:00:00Z","created_at":"2026-04-10T08:30:00Z","updated_at":"2026-04-10T08:30:00Z","deleted_at":null}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | ULID (time-ordered, globally unique) |
| `url` | string | Bookmark URL (normalized on write) |
| `title` | string | Title (may be empty) |
| `description` | string | User note (may be empty) |
| `tags` | string[] | Tag array (default `[]`) |
| `open_count` | u32 | Open count for frecency ranking (server-managed) |
| `last_opened` | string? | Last opened time, UTC ISO8601 (server-managed) |
| `created_at` | string | Creation time, UTC ISO8601 |
| `updated_at` | string | Last update time, UTC ISO8601 (used as conflict resolution key) |
| `deleted_at` | string? | Soft-delete timestamp; non-null means deleted |

`id`, `open_count`, `last_opened`, `created_at`, `updated_at` are server-generated/managed; none of these fields can be set or modified through the API. `updated_at` is refreshed on every write operation: create, update, delete, record open, import restore. Tags are derived data — no separate tag storage.

### URL Normalization

Applied server-side before write; normalized result stored in `url`. Dedup is based on normalized URL.

Rules:
- Trim whitespace
- Lowercase host
- Strip default ports (`:80` for http, `:443` for https)
- Empty path becomes `/`
- Strip trailing `/` on non-root paths
- Preserve query params and fragments
- Do NOT upgrade `http` to `https`
- Only `http` and `https` schemes are accepted; other schemes return `400 invalid_request`

Examples:
- `  https://EXAMPLE.com  ` → `https://example.com/`
- `https://example.com:443/docs` → `https://example.com/docs`
- `https://example.com/docs/` → `https://example.com/docs`
- `https://example.com` → `https://example.com/`

### Tag Normalization

Applied on write: trim whitespace, drop empty values, deduplicate, lowercase.

- `[" Rust ", "rust", "RUST", " Docs ", ""]` → `["rust", "docs"]`

### Uniqueness

- Active (non-deleted) bookmarks must have unique URLs
- Deleted bookmarks do not participate in URL uniqueness checks

### Soft Delete Semantics

- `GET /api/bookmarks/:id`, `PATCH /api/bookmarks/:id`, `DELETE /api/bookmarks/:id`, `POST /api/bookmarks/:id/open` operate on active bookmarks only
- If the target bookmark is missing or soft-deleted, return `404 bookmark_not_found`
- There is no dedicated restore endpoint
- `POST /api/bookmarks` never restores a soft-deleted bookmark implicitly; if the same normalized URL only exists in deleted records, create a new bookmark with a new `id`
- `POST /api/bookmarks/import` is the only API that may restore a soft-deleted bookmark in place

## 4. Tech Stack

| Layer | Technology |
|-------|------------|
| Server | Rust + Axum |
| Serialization | serde + serde_json |
| Browser Extension | TypeScript + WXT (Chrome MV3 / Firefox / Edge) |
| Raycast Extension | TypeScript + @raycast/api |
| API | REST + JSON |

## 5. Search

In-memory scan of `Vec<Bookmark>` with frecency scoring:

- **Multi-word**: all terms must match across title/url/tags/description (AND logic)
- **Case-insensitive**: all comparisons in lowercase
- **Chinese**: substring character match, no tokenization
- **Title prefix bonus**: +10 if title starts with query term
- **Domain match bonus**: +8 if url contains `://<term>`
- **Frequency bonus**: `ln(open_count) * 3`
- **Recency decay**: higher weight for recently opened
- **Field weight order**: title > tags > url > description
- **Sort with `q`**: score desc, then `updated_at` desc, then `id` asc
- **Sort without `q`**: `updated_at` desc, then `id` asc

Deleted bookmarks (`deleted_at` non-null) are excluded from all query endpoints: search, listing, and tag aggregation.

## 6. File I/O & Concurrency

### Concurrency Control

- `tokio::sync::RwLock` protects in-memory state
- Read operations (list, search, get) acquire read lock — concurrent reads allowed
- Write operations (create, update, delete, record open, import, file reload) acquire write lock — exclusive
- On write: update memory first, then atomic file write; rollback memory on file write failure

### Atomic Write

1. Write to `latch.jsonl.tmp`
2. `fsync`
3. `rename` over original (atomic)

Crash at any step leaves the original file intact.

### Startup Recovery

- Remove stale `latch.jsonl.tmp` if present
- Parse `latch.jsonl` line by line; skip and warn on malformed lines
- If any lines were skipped, immediately rewrite the file to repair

## 7. iCloud Sync

Enable by setting `data_file` in `config.toml` to an iCloud Drive path.

### Conflict Resolution (on startup + file watch)

1. Scan directory for conflict copies (e.g. `latch 2.jsonl`)
2. Read all copies, build `HashMap<id, Bookmark>`
3. Same `id` → keep the canonical record by `updated_at` desc, then active (`deleted_at == null`) first, then `id` asc
4. Enforce active-URL uniqueness on the merged set:
   - Group active bookmarks by normalized `url`
   - If a group has more than one active bookmark, choose a canonical winner by `updated_at` desc, then `created_at` desc, then `id` asc
   - Merge survivor fields: `tags` = union, `open_count` = sum, `last_opened` = max, `created_at` = min, `title`/`description` = winner values
   - Rewrite the winner with `deleted_at = null` and `updated_at = resolved_at`
   - Convert every loser into a tombstone with preserved `id`, `deleted_at = resolved_at`, `updated_at = resolved_at`
5. Write merged result to main file
6. Delete conflict copies

`resolved_at` is one UTC timestamp generated once for the current conflict-resolution pass and reused for all URL-uniqueness rewrites in that pass. This makes the result deterministic and helps all replicas converge after the rewritten file syncs back out.

### File Watching (runtime)

- `notify` crate watches the data file's parent directory
- Filter: only `.jsonl` modify/create events
- Debounce: 500ms
- On trigger: acquire write lock → full reload with conflict merge
- If a write operation holds the lock, reload waits for it to complete

## 8. API

Base URL: `http://127.0.0.1:52525`

Authoritative spec: [openapi.yaml](openapi.yaml). This section is a summary only.

### Conventions

- HTTP status codes for success/failure (no `200 + business code` wrapper)
- Single resource endpoints return the resource object directly
- Paginated list: `{ "object": "list", "data": [...], "offset", "limit", "total" }`
- Errors: `{ "error": { "code": "...", "message": "..." } }`
- Error codes: `invalid_request`, `bookmark_not_found`, `duplicate_url`, `import_invalid_item`, `internal_error`
- All timestamps in UTC ISO8601; only `Z` suffix accepted, time zone offsets rejected
- Deleted bookmarks are excluded from all query endpoints
- Multiple list filters are combined with AND semantics
- `since` / `until` are inclusive filters on `updated_at`
- `url` query params are normalized with the same rules as write-time normalization before exact match
- `tag` query params are normalized with the same rules as write-time normalization before exact match
- `PATCH /api/bookmarks/:id` requires at least one field in the JSON body; `{}` returns `400 invalid_request`
- `PATCH` url that normalizes to the bookmark's own current URL is not a conflict (updates `updated_at`, no `409`)
- To clear fields, clients must send `""` for `title` / `description` and `[]` for `tags`; `null` is not accepted
- Request body must use `Content-Type: application/json`; unknown fields in the JSON body return `400 invalid_request`

### Endpoints

```
GET    /health                  # Health check
GET    /api/bookmarks           # List + search + filter
GET    /api/bookmarks/:id       # Get single bookmark
POST   /api/bookmarks           # Create (url required; 409 if active duplicate)
PATCH  /api/bookmarks/:id       # Partial update
DELETE /api/bookmarks/:id       # Soft delete (sets deleted_at + updated_at)
POST   /api/bookmarks/:id/open  # Record open (increment open_count)
GET    /api/bookmarks/tags      # Tag stats (active bookmarks only, sorted by name asc)
POST   /api/bookmarks/import    # Bulk import
```

### Query Parameters (GET /api/bookmarks)

`q`, `tag`, `url`, `since`, `until`, `offset`, `limit`

- `q`: trimmed, case-insensitive substring search; empty or whitespace-only value is ignored (treated as absent)
- `tag`: exact match after tag normalization
- `url`: exact match after URL normalization
- `since` / `until`: inclusive `updated_at` range
- `offset`: 0-based, default `0`; if `offset` ≥ `total`, returns empty `data`
- `limit`: default `50`, max `100`

### Record Open (POST /api/bookmarks/:id/open)

- No request body required
- Increments `open_count` by 1, sets `last_opened` and `updated_at` to current time
- Returns the updated bookmark
- 404 if bookmark is missing or soft-deleted

### Import Behavior (POST /api/bookmarks/import)

- Each item requires at least `url`
- Empty items array `{"items": []}` is valid and returns `200` with all counters at zero
- Validation is atomic: if any item is invalid, the whole request fails with `400 import_invalid_item` and nothing is written
- `import_invalid_item` includes the failing `item_index` and `field` in error details
- Duplicate URL with an active bookmark → skip and keep the existing record unchanged
- Duplicate URL with a soft-deleted bookmark → restore the existing record in place (clear `deleted_at`, update mutable fields from import data, preserve `id`, `created_at`, `open_count`, `last_opened`)
- Duplicate normalized URLs within the same import request are processed in array order; the first effective item wins, later duplicates are skipped
- Import result returns `created`, `restored`, `skipped`, `total`

### Status Codes

| Code | Usage |
|------|-------|
| 200 | Query, update, delete, import success |
| 201 | Create success |
| 400 | Invalid params or malformed JSON |
| 404 | Bookmark not found |
| 409 | URL conflict |
| 500 | Internal error |

### Access & CORS

- Listens on `127.0.0.1` only, no auth token
- CORS is not enabled for ordinary web pages
- Browser extensions rely on extension host permissions for localhost access

## 9. Logging

- `tracing` + `tracing-subscriber`, structured output to stderr
- Level configured via `log_level` in `config.toml` (default `info`)
- Startup: log config path, data file path, loaded bookmark count, listen address/port
- Write operations: info log (operation type, bookmark id)
- Malformed lines on load: warn log (line number, raw content)
- File watch reload: info log
- Conflict merge: info log (conflict copy count, merged total)

## 10. Data Migration

- No version field in `latch.jsonl`
- New fields: `#[serde(default)]` fills defaults for missing fields
- Renamed fields: `#[serde(alias = "old_name")]` for backward compat; writes use new name
- Removed fields: `deny_unknown_fields` is NOT enabled; unknown fields silently ignored
- Migration is automatic: first load + atomic rewrite persists the updated schema
- Breaking changes: provide a standalone one-time migration tool

## 11. Server Lifecycle

```
Install:    brew install iashc/tap/latch
Run:        latch serve
Autostart:  latch service install
Plist:      ~/.latch/launchd/com.iashc.latch.plist
System link: ~/Library/LaunchAgents/com.iashc.latch.plist
Config:     ~/.config/latch/config.toml
Runtime:    ~/.latch
Port:       52525 (configurable)
Dup check:  Binding fails if the configured port is already occupied; clients can probe /health
```

### Graceful Shutdown

- On SIGTERM / SIGINT: stop accepting new requests
- Let in-flight Axum requests complete through graceful shutdown
- Any write already holding the store lock completes before the handler returns
- LaunchAgent keeps stdout/stderr in `~/.latch/logs/server.log`

### CLI Commands

- `latch status`, `latch doctor`, `latch logs`
- `latch start`, `latch stop`, `latch restart`
- `latch service install|uninstall|start|stop|restart|status|print-plist`
- `latch config show|use-local|use-icloud`
- `latch chrome install|update|path|open|uninstall`
- `latch browser ...` as an alias for `latch chrome ...`
- `latch raycast install|update|path|open|uninstall`
- `latch import browser-html <bookmarks.html>`

Chrome and Raycast client commands download prebuilt GitHub Release assets, verify sha256 from `latch-release-manifest.json`, cache archives in `~/.latch/cache`, and install unpacked clients under `~/.latch/clients`. The Raycast asset must be the `ray build` output and include command executables such as `search.js`, `add.js`, and `tags.js` at the package root.

### Release Process

Releases are built locally to avoid consuming GitHub Actions minutes.

```bash
scripts/release.sh --version v0.1.0
scripts/release.sh --version v0.1.0 --publish
LATCH_HOMEBREW_TAP_PATH=/absolute/path/to/homebrew-tap scripts/release.sh --version v0.1.0 --publish --tap-commit
```

The script writes assets to `dist/release/<tag>`:

- `latch-cli-aarch64-apple-darwin.tar.gz`
- `latch-chrome-mv3.zip`
- `latch-raycast.zip`
- `latch-release-manifest.json`
- `homebrew/latch.rb`

The default CLI target is Apple silicon (`aarch64-apple-darwin`). The generated Homebrew formula is ARM-only and follows the same personal-use scope as this project. Publishing is explicit. `--publish` uses the local `gh` CLI, and Homebrew tap updates require an explicit tap checkout path via `--tap-path` or `LATCH_HOMEBREW_TAP_PATH`; the script does not assume any parent-directory layout.

## 12. Client Features

### Browser Extension

- One-click save current page (popup / shortcut)
- Tag selection on save
- Search saved bookmarks from popup
- Local unpacked install via `latch chrome install`

### Raycast Extension

- Search bookmarks (List view + live search)
- Quick add bookmark (Form view)
- Browse by tag
- Open bookmark / copy URL

## 13. Project Structure

```
latch/
├── server/                     # Rust server
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs             # CLI entry and command dispatch
│       ├── paths.rs            # ~/.latch and ~/.config path helpers
│       ├── service.rs          # macOS LaunchAgent management
│       ├── client_packages.rs  # GitHub Release client downloads
│       ├── store.rs            # In-memory store + file I/O
│       ├── search.rs           # Search + frecency scoring
│       ├── sync.rs             # iCloud conflict merge + file watch
│       └── routes.rs           # REST API routes
├── browser/                    # Browser extension (WXT + Vue 3)
│   ├── package.json
│   ├── wxt.config.ts
│   ├── entrypoints/
│   │   ├── popup/
│   │   └── options/
│   └── src/
│       ├── components/
│       ├── composables/
│       └── lib/
├── raycast/                    # Raycast extension
│   ├── package.json
│   └── src/
│       ├── search.tsx
│       ├── add.tsx
│       └── lib/api.ts
├── scripts/
│   └── release.sh              # Local build and publish script
└── shared/                     # Shared types
    └── types.ts
```

## 14. Implementation Phases

```
Phase 1 → Server core: store + CRUD API + search
Phase 2 → Browser extension
Phase 3 → Raycast extension
Phase 4 → iCloud sync
```
