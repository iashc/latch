use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::{sync::mpsc, task::JoinHandle, time::timeout};
use tracing::{error, info, warn};

use crate::{
    models::{Bookmark, now_utc},
    store::AppStore,
};

#[derive(Debug)]
pub struct LoadSummary {
    pub bookmarks: Vec<Bookmark>,
    pub skipped_lines: usize,
    pub conflict_copy_count: usize,
    pub repaired: bool,
}

pub fn ensure_storage_ready(data_file: &Path) -> Result<()> {
    if let Some(parent) = data_file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create data directory {}", parent.display()))?;
    }

    let tmp_path = temp_path(data_file);
    if tmp_path.exists() {
        fs::remove_file(&tmp_path)
            .with_context(|| format!("Failed to remove stale temp file {}", tmp_path.display()))?;
    }

    if !data_file.exists() {
        File::create(data_file)
            .with_context(|| format!("Failed to create data file {}", data_file.display()))?;
    }

    Ok(())
}

pub fn load_and_reconcile(data_file: &Path) -> Result<LoadSummary> {
    ensure_storage_ready(data_file)?;

    let conflict_copies = find_conflict_copies(data_file)?;
    let mut all_bookmarks = Vec::new();
    let mut skipped_lines = 0;

    for path in std::iter::once(data_file.to_path_buf()).chain(conflict_copies.iter().cloned()) {
        let mut parsed = load_bookmarks_from_file(&path)?;
        skipped_lines += parsed.skipped_lines;
        all_bookmarks.append(&mut parsed.bookmarks);
    }

    let (mut bookmarks, had_duplicate_ids) = dedupe_by_id(all_bookmarks);
    let had_duplicate_urls = resolve_active_url_conflicts(&mut bookmarks);
    canonical_sort(&mut bookmarks);

    let repaired =
        skipped_lines > 0 || !conflict_copies.is_empty() || had_duplicate_ids || had_duplicate_urls;

    if repaired {
        write_bookmarks_atomic(data_file, &bookmarks)?;
        for copy in &conflict_copies {
            if let Err(error) = fs::remove_file(copy) {
                warn!(
                    path = %copy.display(),
                    error = %error,
                    "failed to remove conflict copy",
                );
            }
        }
    }

    Ok(LoadSummary {
        bookmarks,
        skipped_lines,
        conflict_copy_count: conflict_copies.len(),
        repaired,
    })
}

pub fn write_bookmarks_atomic(data_file: &Path, bookmarks: &[Bookmark]) -> Result<()> {
    let tmp_path = temp_path(data_file);
    let mut file = File::create(&tmp_path)
        .with_context(|| format!("Failed to create temp file {}", tmp_path.display()))?;

    for bookmark in bookmarks {
        serde_json::to_writer(&mut file, bookmark)
            .with_context(|| format!("Failed to serialize bookmark {}", bookmark.id))?;
        file.write_all(b"\n")
            .with_context(|| format!("Failed to write temp file {}", tmp_path.display()))?;
    }

    file.sync_all()
        .with_context(|| format!("Failed to fsync temp file {}", tmp_path.display()))?;
    fs::rename(&tmp_path, data_file).with_context(|| {
        format!(
            "Failed to atomically replace {} with {}",
            data_file.display(),
            tmp_path.display()
        )
    })?;

    if let Some(parent) = data_file.parent() {
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
    }

    Ok(())
}

pub fn spawn_file_watcher(store: AppStore) -> Result<JoinHandle<()>> {
    let watch_dir = store
        .data_file()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let data_file = store.data_file().to_path_buf();

    let handle = tokio::spawn(async move {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let mut watcher = match build_watcher(event_tx) {
            Ok(watcher) => watcher,
            Err(error) => {
                error!(error = %error, "failed to create file watcher");
                return;
            }
        };

        if let Err(error) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
            error!(path = %watch_dir.display(), error = %error, "failed to watch data directory");
            return;
        }

        loop {
            let Some(event) = event_rx.recv().await else {
                break;
            };

            let Ok(event) = event else {
                continue;
            };

            if !is_relevant_event(&data_file, &event) {
                continue;
            }

            while let Ok(Some(next_event)) =
                timeout(Duration::from_millis(500), event_rx.recv()).await
            {
                let Ok(next_event) = next_event else {
                    continue;
                };

                if !is_relevant_event(&data_file, &next_event) {
                    continue;
                }
            }

            info!(path = %data_file.display(), "file change detected; reloading store");
            if let Err(error) = store.reload_from_disk().await {
                error!(error = %error, "failed to reload store after file change");
            }
        }
    });

    Ok(handle)
}

fn build_watcher(
    event_tx: mpsc::UnboundedSender<notify::Result<Event>>,
) -> notify::Result<RecommendedWatcher> {
    RecommendedWatcher::new(
        move |event| {
            let _ = event_tx.send(event);
        },
        Config::default(),
    )
}

fn is_relevant_event(data_file: &Path, event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && event.paths.iter().any(|path| {
        path.extension()
            .is_some_and(|extension| extension == "jsonl")
            && path
                .parent()
                .zip(data_file.parent())
                .is_some_and(|(left, right)| left == right)
    })
}

fn load_bookmarks_from_file(path: &Path) -> Result<ParsedFile> {
    let file =
        File::open(path).with_context(|| format!("Failed to open data file {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut bookmarks = Vec::new();
    let mut skipped_lines = 0;

    for (line_number, line_result) in reader.lines().enumerate() {
        let line = line_result.with_context(|| format!("Failed to read {}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<Bookmark>(&line) {
            Ok(bookmark) => bookmarks.push(bookmark),
            Err(error) => {
                skipped_lines += 1;
                warn!(
                    path = %path.display(),
                    line_number = line_number + 1,
                    raw = %line,
                    error = %error,
                    "skipping malformed jsonl line",
                );
            }
        }
    }

    Ok(ParsedFile {
        bookmarks,
        skipped_lines,
    })
}

fn find_conflict_copies(data_file: &Path) -> Result<Vec<PathBuf>> {
    let Some(parent) = data_file.parent() else {
        return Ok(Vec::new());
    };
    let main_stem = data_file
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();

    let mut copies = Vec::new();
    for entry in fs::read_dir(parent)
        .with_context(|| format!("Failed to scan directory {}", parent.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path == data_file {
            continue;
        }

        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if stem.starts_with(main_stem) {
            copies.push(path);
        }
    }

    copies.sort();
    Ok(copies)
}

fn dedupe_by_id(bookmarks: Vec<Bookmark>) -> (Vec<Bookmark>, bool) {
    let mut merged: HashMap<String, Bookmark> = HashMap::new();
    let mut had_duplicates = false;

    for bookmark in bookmarks {
        match merged.get(&bookmark.id) {
            Some(current) if !is_better_same_id(&bookmark, current) => {
                had_duplicates = true;
            }
            Some(_) => {
                had_duplicates = true;
                merged.insert(bookmark.id.clone(), bookmark);
            }
            None => {
                merged.insert(bookmark.id.clone(), bookmark);
            }
        }
    }

    (merged.into_values().collect(), had_duplicates)
}

fn resolve_active_url_conflicts(bookmarks: &mut [Bookmark]) -> bool {
    let mut grouped: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, bookmark) in bookmarks.iter().enumerate() {
        if !bookmark.is_deleted() {
            grouped.entry(bookmark.url.clone()).or_default().push(index);
        }
    }

    let resolved_at = now_utc();
    let mut had_conflicts = false;

    for indices in grouped.into_values() {
        if indices.len() <= 1 {
            continue;
        }

        had_conflicts = true;
        let winner_index = indices
            .iter()
            .copied()
            .max_by(|left, right| compare_active_url_winner(&bookmarks[*left], &bookmarks[*right]))
            .unwrap_or(indices[0]);

        let mut merged_tags = BTreeSet::new();
        let mut merged_open_count = 0_u32;
        let mut merged_last_opened: Option<DateTime<Utc>> = None;
        let mut merged_created_at = bookmarks[winner_index].created_at;

        for index in &indices {
            let bookmark = &bookmarks[*index];
            merged_open_count = merged_open_count.saturating_add(bookmark.open_count);
            merged_created_at = merged_created_at.min(bookmark.created_at);
            merged_last_opened = match (merged_last_opened, bookmark.last_opened) {
                (Some(current), Some(candidate)) => Some(current.max(candidate)),
                (None, Some(candidate)) => Some(candidate),
                (current, None) => current,
            };

            for tag in &bookmark.tags {
                merged_tags.insert(tag.clone());
            }
        }

        {
            let winner = &mut bookmarks[winner_index];
            winner.tags = merged_tags.into_iter().collect();
            winner.open_count = merged_open_count;
            winner.last_opened = merged_last_opened;
            winner.created_at = merged_created_at;
            winner.deleted_at = None;
            winner.updated_at = resolved_at;
        }

        for index in indices {
            if index == winner_index {
                continue;
            }

            let loser = &mut bookmarks[index];
            loser.deleted_at = Some(resolved_at);
            loser.updated_at = resolved_at;
        }
    }

    had_conflicts
}

fn compare_active_url_winner(left: &Bookmark, right: &Bookmark) -> std::cmp::Ordering {
    left.updated_at
        .cmp(&right.updated_at)
        .then_with(|| left.created_at.cmp(&right.created_at))
        .then_with(|| right.id.cmp(&left.id))
}

fn is_better_same_id(candidate: &Bookmark, current: &Bookmark) -> bool {
    candidate.updated_at > current.updated_at
        || (candidate.updated_at == current.updated_at
            && !candidate.is_deleted()
            && current.is_deleted())
        || (candidate.updated_at == current.updated_at
            && candidate.is_deleted() == current.is_deleted()
            && candidate.id < current.id)
}

fn canonical_sort(bookmarks: &mut [Bookmark]) {
    bookmarks.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn temp_path(data_file: &Path) -> PathBuf {
    data_file.with_extension("jsonl.tmp")
}

struct ParsedFile {
    bookmarks: Vec<Bookmark>,
    skipped_lines: usize,
}
