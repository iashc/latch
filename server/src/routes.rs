use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection, rejection::QueryRejection},
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;

use crate::{
    error::{ApiError, ApiResult},
    models::{
        BookmarkListResponse, CreateBookmarkRequest, HealthResponse, ImportBookmarksRequest,
        TagListResponse, UpdateBookmarkRequest, normalize_tag_query, normalize_url,
        parse_utc_timestamp,
    },
    search::SearchFilters,
    store::AppStore,
};

#[derive(Clone)]
struct AppState {
    store: AppStore,
}

#[derive(Debug, Deserialize)]
struct ListBookmarksQuery {
    q: Option<String>,
    tag: Option<String>,
    url: Option<String>,
    since: Option<String>,
    until: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
}

pub fn router(store: AppStore) -> Router {
    let state = AppState { store };

    Router::new()
        .route("/health", get(health))
        .route("/api/bookmarks", get(list_bookmarks).post(create_bookmark))
        .route("/api/bookmarks/tags", get(list_tags))
        .route("/api/bookmarks/import", post(import_bookmarks))
        .route(
            "/api/bookmarks/{id}",
            get(get_bookmark)
                .patch(update_bookmark)
                .delete(delete_bookmark),
        )
        .route("/api/bookmarks/{id}/open", post(record_open))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

async fn list_bookmarks(
    State(state): State<AppState>,
    query: Result<Query<ListBookmarksQuery>, QueryRejection>,
) -> ApiResult<Json<BookmarkListResponse>> {
    let Query(query) = query.map_err(|error| {
        ApiError::invalid_request(format!("Invalid query parameters: {}", error.body_text()))
    })?;

    let limit = query.limit.unwrap_or(50).min(100);
    if limit == 0 {
        return Err(ApiError::invalid_request("limit must be at least 1"));
    }

    let filters = SearchFilters {
        q: query.q.and_then(|value| {
            let trimmed = value.trim().to_owned();
            (!trimmed.is_empty()).then_some(trimmed)
        }),
        tag: query.tag.map(|tag| normalize_tag_query(&tag)).transpose()?,
        url: query.url.map(|url| normalize_url(&url)).transpose()?,
        since: query
            .since
            .map(|value| parse_utc_timestamp(&value, "since"))
            .transpose()?,
        until: query
            .until
            .map(|value| parse_utc_timestamp(&value, "until"))
            .transpose()?,
        offset: query.offset.unwrap_or(0),
        limit,
    };

    let (data, total) = state.store.list(filters.clone()).await;
    Ok(Json(BookmarkListResponse {
        object: "list",
        data,
        offset: filters.offset,
        limit: filters.limit,
        total,
    }))
}

async fn get_bookmark(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<crate::models::Bookmark>> {
    Ok(Json(state.store.get(&id).await?))
}

async fn create_bookmark(
    State(state): State<AppState>,
    payload: Result<Json<CreateBookmarkRequest>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<crate::models::Bookmark>)> {
    let Json(request) = parse_json(payload)?;
    let bookmark = state.store.create(request).await?;
    Ok((StatusCode::CREATED, Json(bookmark)))
}

async fn update_bookmark(
    State(state): State<AppState>,
    Path(id): Path<String>,
    payload: Result<Json<UpdateBookmarkRequest>, JsonRejection>,
) -> ApiResult<Json<crate::models::Bookmark>> {
    let Json(request) = parse_json(payload)?;
    if request.is_empty() {
        return Err(ApiError::invalid_request(
            "PATCH request body must include at least one field",
        ));
    }

    Ok(Json(state.store.update(&id, request).await?))
}

async fn delete_bookmark(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<crate::models::Bookmark>> {
    Ok(Json(state.store.delete(&id).await?))
}

async fn record_open(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<crate::models::Bookmark>> {
    Ok(Json(state.store.record_open(&id).await?))
}

async fn list_tags(State(state): State<AppState>) -> ApiResult<Json<TagListResponse>> {
    let data = state.store.tags().await;
    let total = data.len();
    Ok(Json(TagListResponse {
        object: "list",
        data,
        total,
    }))
}

async fn import_bookmarks(
    State(state): State<AppState>,
    payload: Result<Json<ImportBookmarksRequest>, JsonRejection>,
) -> ApiResult<Json<crate::models::ImportResultResponse>> {
    let Json(request) = parse_json(payload)?;
    Ok(Json(state.store.import(request).await?))
}

fn parse_json<T>(payload: Result<Json<T>, JsonRejection>) -> ApiResult<Json<T>> {
    payload.map_err(|error| {
        ApiError::invalid_request(format!("Invalid JSON body: {}", error.body_text()))
    })
}
