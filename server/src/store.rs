use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use tokio::sync::RwLock;
use tracing::info;
use ulid::Ulid;

use crate::{
    error::{ApiError, ApiResult},
    models::{
        Bookmark, CreateBookmarkRequest, ImportBookmarksRequest, ImportResultResponse,
        NormalizedImportItem, TagSummary, UpdateBookmarkRequest, normalize_tags, normalize_url,
        now_utc,
    },
    search::{SearchFilters, filter_and_rank},
    sync,
};

#[derive(Debug, Clone)]
pub struct AppStore {
    data_file: Arc<PathBuf>,
    inner: Arc<RwLock<StoreState>>,
}

#[derive(Debug, Clone)]
struct StoreState {
    bookmarks: Vec<Bookmark>,
    by_id: HashMap<String, usize>,
    active_by_url: HashMap<String, String>,
}

impl AppStore {
    pub fn load(data_file: PathBuf) -> Result<Self> {
        let summary = sync::load_and_reconcile(&data_file)?;
        info!(
            skipped_lines = summary.skipped_lines,
            conflict_copy_count = summary.conflict_copy_count,
            repaired = summary.repaired,
            "loaded bookmark store from disk"
        );
        let state = StoreState::new(summary.bookmarks);
        Ok(Self {
            data_file: Arc::new(data_file),
            inner: Arc::new(RwLock::new(state)),
        })
    }

    pub fn data_file(&self) -> &Path {
        self.data_file.as_ref().as_path()
    }

    pub async fn count(&self) -> usize {
        self.inner.read().await.bookmarks.len()
    }

    pub async fn reload_from_disk(&self) -> Result<()> {
        let summary = sync::load_and_reconcile(self.data_file())?;
        let mut guard = self.inner.write().await;
        *guard = StoreState::new(summary.bookmarks);
        info!(
            skipped_lines = summary.skipped_lines,
            conflict_copy_count = summary.conflict_copy_count,
            repaired = summary.repaired,
            "reloaded bookmark store from disk"
        );
        Ok(())
    }

    pub async fn list(&self, filters: SearchFilters) -> (Vec<Bookmark>, usize) {
        let guard = self.inner.read().await;
        let ranked = filter_and_rank(&guard.bookmarks, &filters, now_utc());
        let total = ranked.len();
        let data = ranked
            .into_iter()
            .skip(filters.offset)
            .take(filters.limit)
            .collect();
        (data, total)
    }

    pub async fn get(&self, id: &str) -> ApiResult<Bookmark> {
        let guard = self.inner.read().await;
        guard
            .active_by_id(id)
            .cloned()
            .ok_or_else(ApiError::bookmark_not_found)
    }

    pub async fn tags(&self) -> Vec<TagSummary> {
        let guard = self.inner.read().await;
        let mut counts: HashMap<String, usize> = HashMap::new();

        for bookmark in guard
            .bookmarks
            .iter()
            .filter(|bookmark| !bookmark.is_deleted())
        {
            for tag in &bookmark.tags {
                *counts.entry(tag.clone()).or_default() += 1;
            }
        }

        let mut tags = counts
            .into_iter()
            .map(|(name, count)| TagSummary { name, count })
            .collect::<Vec<_>>();
        tags.sort_by(|left, right| left.name.cmp(&right.name));
        tags
    }

    pub async fn create(&self, request: CreateBookmarkRequest) -> ApiResult<Bookmark> {
        let url = normalize_url(&request.url)?;
        let now = now_utc();
        let bookmark = Bookmark {
            id: Ulid::new().to_string(),
            url,
            title: request.title,
            description: request.description,
            tags: normalize_tags(&request.tags),
            open_count: 0,
            last_opened: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        };

        let mut guard = self.inner.write().await;
        if guard
            .find_active_url_conflict(&bookmark.url, None)
            .is_some()
        {
            return Err(ApiError::duplicate_url(&bookmark.url));
        }

        let previous = guard.clone();
        guard.bookmarks.push(bookmark.clone());
        guard.rebuild_indexes();
        if let Err(error) = sync::write_bookmarks_atomic(self.data_file(), &guard.bookmarks) {
            *guard = previous;
            return Err(ApiError::internal(format!(
                "Failed to persist bookmark: {error}"
            )));
        }

        info!(bookmark_id = %bookmark.id, "created bookmark");
        Ok(bookmark)
    }

    pub async fn update(&self, id: &str, request: UpdateBookmarkRequest) -> ApiResult<Bookmark> {
        let mut guard = self.inner.write().await;
        let index = guard
            .active_index_by_id(id)
            .ok_or_else(ApiError::bookmark_not_found)?;

        let normalized_url = match request.url.as_deref() {
            Some(url) => Some(normalize_url(url)?),
            None => None,
        };

        let current_url = guard.bookmarks[index].url.clone();
        if let Some(url) = normalized_url.as_deref() {
            if url != current_url && guard.find_active_url_conflict(url, Some(id)).is_some() {
                return Err(ApiError::duplicate_url(url));
            }
        }

        let previous = guard.clone();
        {
            let bookmark = &mut guard.bookmarks[index];
            if let Some(url) = normalized_url {
                bookmark.url = url;
            }
            if let Some(title) = request.title {
                bookmark.title = title;
            }
            if let Some(description) = request.description {
                bookmark.description = description;
            }
            if let Some(tags) = request.tags {
                bookmark.tags = normalize_tags(&tags);
            }
            bookmark.updated_at = now_utc();
        }

        let updated = guard.bookmarks[index].clone();
        guard.rebuild_indexes();
        if let Err(error) = sync::write_bookmarks_atomic(self.data_file(), &guard.bookmarks) {
            *guard = previous;
            return Err(ApiError::internal(format!(
                "Failed to persist bookmark update: {error}"
            )));
        }

        info!(bookmark_id = %updated.id, "updated bookmark");
        Ok(updated)
    }

    pub async fn delete(&self, id: &str) -> ApiResult<Bookmark> {
        let mut guard = self.inner.write().await;
        let index = guard
            .active_index_by_id(id)
            .ok_or_else(ApiError::bookmark_not_found)?;

        let previous = guard.clone();
        {
            let bookmark = &mut guard.bookmarks[index];
            let now = now_utc();
            bookmark.deleted_at = Some(now);
            bookmark.updated_at = now;
        }

        let deleted = guard.bookmarks[index].clone();
        guard.rebuild_indexes();
        if let Err(error) = sync::write_bookmarks_atomic(self.data_file(), &guard.bookmarks) {
            *guard = previous;
            return Err(ApiError::internal(format!(
                "Failed to persist bookmark delete: {error}"
            )));
        }

        info!(bookmark_id = %deleted.id, "deleted bookmark");
        Ok(deleted)
    }

    pub async fn record_open(&self, id: &str) -> ApiResult<Bookmark> {
        let mut guard = self.inner.write().await;
        let index = guard
            .active_index_by_id(id)
            .ok_or_else(ApiError::bookmark_not_found)?;

        let previous = guard.clone();
        {
            let bookmark = &mut guard.bookmarks[index];
            let now = now_utc();
            bookmark.open_count = bookmark.open_count.saturating_add(1);
            bookmark.last_opened = Some(now);
            bookmark.updated_at = now;
        }

        let opened = guard.bookmarks[index].clone();
        if let Err(error) = sync::write_bookmarks_atomic(self.data_file(), &guard.bookmarks) {
            *guard = previous;
            return Err(ApiError::internal(format!(
                "Failed to persist bookmark open event: {error}"
            )));
        }

        info!(bookmark_id = %opened.id, "recorded bookmark open");
        Ok(opened)
    }

    pub async fn import(&self, request: ImportBookmarksRequest) -> ApiResult<ImportResultResponse> {
        let items = request
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                normalize_import_item(index, item).map(|normalized| (index, normalized))
            })
            .collect::<ApiResult<Vec<_>>>()?;

        if items.is_empty() {
            return Ok(ImportResultResponse {
                object: "import_result",
                created: 0,
                restored: 0,
                skipped: 0,
                total: 0,
            });
        }

        let mut guard = self.inner.write().await;
        let previous = guard.clone();
        let mut created = 0;
        let mut restored = 0;
        let mut skipped = 0;

        for (_index, item) in items {
            if guard.find_active_url_conflict(&item.url, None).is_some() {
                skipped += 1;
                continue;
            }

            if let Some(existing_index) = guard.deleted_index_by_url(&item.url) {
                let bookmark = &mut guard.bookmarks[existing_index];
                bookmark.url = item.url;
                bookmark.title = item.title;
                bookmark.description = item.description;
                bookmark.tags = item.tags;
                bookmark.deleted_at = None;
                bookmark.updated_at = now_utc();
                restored += 1;
                guard.rebuild_indexes();
                continue;
            }

            let now = now_utc();
            let bookmark = Bookmark {
                id: Ulid::new().to_string(),
                url: item.url,
                title: item.title,
                description: item.description,
                tags: item.tags,
                open_count: 0,
                last_opened: None,
                created_at: now,
                updated_at: now,
                deleted_at: None,
            };
            guard.bookmarks.push(bookmark);
            guard.rebuild_indexes();
            created += 1;
        }

        if let Err(error) = sync::write_bookmarks_atomic(self.data_file(), &guard.bookmarks) {
            *guard = previous;
            return Err(ApiError::internal(format!(
                "Failed to persist import: {error}"
            )));
        }

        info!(created, restored, skipped, "imported bookmarks");
        Ok(ImportResultResponse {
            object: "import_result",
            created,
            restored,
            skipped,
            total: created + restored + skipped,
        })
    }
}

impl StoreState {
    fn new(bookmarks: Vec<Bookmark>) -> Self {
        let mut state = Self {
            bookmarks,
            by_id: HashMap::new(),
            active_by_url: HashMap::new(),
        };
        state.rebuild_indexes();
        state
    }

    fn rebuild_indexes(&mut self) {
        self.by_id.clear();
        self.active_by_url.clear();

        for (index, bookmark) in self.bookmarks.iter().enumerate() {
            self.by_id.insert(bookmark.id.clone(), index);
            if !bookmark.is_deleted() {
                self.active_by_url
                    .insert(bookmark.url.clone(), bookmark.id.clone());
            }
        }
    }

    fn active_by_id(&self, id: &str) -> Option<&Bookmark> {
        self.by_id
            .get(id)
            .and_then(|index| self.bookmarks.get(*index))
            .filter(|bookmark| !bookmark.is_deleted())
    }

    fn active_index_by_id(&self, id: &str) -> Option<usize> {
        self.by_id
            .get(id)
            .copied()
            .filter(|index| self.bookmarks[*index].deleted_at.is_none())
    }

    fn find_active_url_conflict(&self, url: &str, exclude_id: Option<&str>) -> Option<&Bookmark> {
        let bookmark_id = self.active_by_url.get(url)?;
        if exclude_id.is_some_and(|exclude_id| exclude_id == bookmark_id) {
            return None;
        }
        self.active_by_id(bookmark_id)
    }

    fn deleted_index_by_url(&self, url: &str) -> Option<usize> {
        self.bookmarks
            .iter()
            .enumerate()
            .find(|(_, bookmark)| bookmark.url == url && bookmark.is_deleted())
            .map(|(index, _)| index)
    }
}

fn normalize_import_item(
    index: usize,
    item: &crate::models::ImportBookmarkItem,
) -> ApiResult<NormalizedImportItem> {
    let url = normalize_url(&item.url)
        .map_err(|error| ApiError::import_invalid_item(index, "url", format_import_error(error)))?;

    Ok(NormalizedImportItem {
        url,
        title: item.title.clone(),
        description: item.description.clone(),
        tags: normalize_tags(&item.tags),
    })
}

fn format_import_error(error: ApiError) -> String {
    error.message().to_owned()
}
