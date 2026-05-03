use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::ApiError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bookmark {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub open_count: u32,
    #[serde(default)]
    pub last_opened: Option<DateTime<Utc>>,
    #[serde(default = "now_utc")]
    pub created_at: DateTime<Utc>,
    #[serde(default = "now_utc")]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Bookmark {
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateBookmarkRequest {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateBookmarkRequest {
    pub url: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
}

impl UpdateBookmarkRequest {
    pub fn is_empty(&self) -> bool {
        self.url.is_none()
            && self.title.is_none()
            && self.description.is_none()
            && self.tags.is_none()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportBookmarksRequest {
    pub items: Vec<ImportBookmarkItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportBookmarkItem {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BookmarkListResponse {
    pub object: &'static str,
    pub data: Vec<Bookmark>,
    pub offset: usize,
    pub limit: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagSummary {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagListResponse {
    pub object: &'static str,
    pub data: Vec<TagSummary>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportResultResponse {
    pub object: &'static str,
    pub created: usize,
    pub restored: usize,
    pub skipped: usize,
    pub total: usize,
}

#[derive(Debug, Clone)]
pub struct NormalizedImportItem {
    pub url: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
}

pub fn now_utc() -> DateTime<Utc> {
    DateTime::<Utc>::from(std::time::SystemTime::now())
}

pub fn normalize_url(input: &str) -> Result<String, ApiError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ApiError::invalid_request("URL is required"));
    }

    let mut url = Url::parse(trimmed)
        .map_err(|_| ApiError::invalid_request(format!("Invalid URL: {trimmed}")))?;

    match url.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(ApiError::invalid_request(
                "Only http and https URLs are accepted",
            ));
        }
    }

    let should_strip_port = matches!(
        (url.scheme(), url.port()),
        ("http", Some(80)) | ("https", Some(443))
    );
    if should_strip_port {
        let _ = url.set_port(None);
    }

    let path = url.path().to_owned();
    if path.is_empty() {
        url.set_path("/");
    } else if path != "/" {
        let trimmed_path = path.trim_end_matches('/').to_owned();
        url.set_path(&trimmed_path);
    }

    Ok(url.to_string())
}

pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for tag in tags {
        let candidate = tag.trim().to_lowercase();
        if candidate.is_empty() || normalized.contains(&candidate) {
            continue;
        }
        normalized.push(candidate);
    }
    normalized
}

pub fn normalize_tag_query(tag: &str) -> Result<String, ApiError> {
    let normalized = normalize_tags(&[tag.to_owned()]);
    normalized
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::invalid_request("Tag must not be empty"))
}

pub fn parse_utc_timestamp(input: &str, field_name: &str) -> Result<DateTime<Utc>, ApiError> {
    if !input.ends_with('Z') {
        return Err(ApiError::invalid_request(format!(
            "{field_name} must use UTC with Z suffix"
        )));
    }

    let parsed = DateTime::parse_from_rfc3339(input).map_err(|_| {
        ApiError::invalid_request(format!("{field_name} must be a valid RFC 3339 timestamp"))
    })?;

    if parsed.offset().local_minus_utc() != FixedOffset::east_opt(0).unwrap().local_minus_utc() {
        return Err(ApiError::invalid_request(format!(
            "{field_name} must use UTC with Z suffix"
        )));
    }

    Ok(parsed.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::{normalize_tags, normalize_url};

    #[test]
    fn normalizes_urls() {
        assert_eq!(
            normalize_url("  https://EXAMPLE.com  ").unwrap(),
            "https://example.com/"
        );
        assert_eq!(
            normalize_url("https://example.com:443/docs").unwrap(),
            "https://example.com/docs"
        );
        assert_eq!(
            normalize_url("https://example.com/docs/").unwrap(),
            "https://example.com/docs"
        );
    }

    #[test]
    fn normalizes_tags() {
        let tags = vec![
            " Rust ".to_owned(),
            "rust".to_owned(),
            "RUST".to_owned(),
            " Docs ".to_owned(),
            "".to_owned(),
        ];
        assert_eq!(normalize_tags(&tags), vec!["rust", "docs"]);
    }
}
