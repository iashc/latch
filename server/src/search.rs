use chrono::{DateTime, Utc};

use crate::models::Bookmark;

#[derive(Debug, Clone)]
pub struct SearchFilters {
    pub q: Option<String>,
    pub tag: Option<String>,
    pub url: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub offset: usize,
    pub limit: usize,
}

pub fn filter_and_rank(
    bookmarks: &[Bookmark],
    filters: &SearchFilters,
    now: DateTime<Utc>,
) -> Vec<Bookmark> {
    let terms = filters.q.as_deref().map(split_terms).unwrap_or_default();
    let use_search = !terms.is_empty();

    let mut ranked = Vec::new();

    for bookmark in bookmarks.iter().filter(|bookmark| !bookmark.is_deleted()) {
        if let Some(tag) = &filters.tag {
            if !bookmark.tags.iter().any(|candidate| candidate == tag) {
                continue;
            }
        }

        if let Some(url) = &filters.url {
            if &bookmark.url != url {
                continue;
            }
        }

        if let Some(since) = filters.since {
            if bookmark.updated_at < since {
                continue;
            }
        }

        if let Some(until) = filters.until {
            if bookmark.updated_at > until {
                continue;
            }
        }

        let score = if use_search {
            match score_bookmark(bookmark, &terms, now) {
                Some(score) => score,
                None => continue,
            }
        } else {
            0.0
        };

        ranked.push((bookmark.clone(), score));
    }

    if use_search {
        ranked.sort_by(
            |(left_bookmark, left_score), (right_bookmark, right_score)| {
                right_score
                    .partial_cmp(left_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| right_bookmark.updated_at.cmp(&left_bookmark.updated_at))
                    .then_with(|| left_bookmark.id.cmp(&right_bookmark.id))
            },
        );
    } else {
        ranked.sort_by(|(left, _), (right, _)| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
    }

    ranked.into_iter().map(|(bookmark, _)| bookmark).collect()
}

fn split_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|term| term.trim().to_lowercase())
        .filter(|term| !term.is_empty())
        .collect()
}

fn score_bookmark(bookmark: &Bookmark, terms: &[String], now: DateTime<Utc>) -> Option<f64> {
    let title = bookmark.title.to_lowercase();
    let url = bookmark.url.to_lowercase();
    let description = bookmark.description.to_lowercase();
    let tags = bookmark
        .tags
        .iter()
        .map(|tag| tag.to_lowercase())
        .collect::<Vec<_>>();

    let mut score = 0.0;

    for term in terms {
        let title_match = title.contains(term);
        let tag_matches = tags.iter().filter(|tag| tag.contains(term)).count();
        let url_match = url.contains(term);
        let description_match = description.contains(term);

        if !title_match && tag_matches == 0 && !url_match && !description_match {
            return None;
        }

        if title_match {
            score += 6.0;
            if title.starts_with(term) {
                score += 10.0;
            }
        }

        if tag_matches > 0 {
            score += 5.0 * tag_matches as f64;
        }

        if url_match {
            score += 4.0;
            if url.contains(&format!("://{term}")) {
                score += 8.0;
            }
        }

        if description_match {
            score += 2.0;
        }
    }

    if bookmark.open_count > 0 {
        score += (bookmark.open_count as f64).ln() * 3.0;
    }

    if let Some(last_opened) = bookmark.last_opened {
        let age_hours = (now - last_opened).num_seconds().max(0) as f64 / 3600.0;
        score += 12.0 / (1.0 + age_hours / 24.0);
    }

    Some(score)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use crate::models::{Bookmark, now_utc};

    use super::{SearchFilters, filter_and_rank};

    fn make_bookmark(id: &str, title: &str, url: &str, tags: &[&str]) -> Bookmark {
        let now = now_utc();
        Bookmark {
            id: id.to_owned(),
            url: url.to_owned(),
            title: title.to_owned(),
            description: String::new(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            open_count: 3,
            last_opened: Some(now - Duration::hours(1)),
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }

    #[test]
    fn search_requires_all_terms() {
        let now = Utc::now();
        let bookmarks = vec![
            make_bookmark("1", "Rust Book", "https://rust-lang.org", &["rust"]),
            make_bookmark("2", "Axum Guide", "https://docs.rs/axum", &["rust", "web"]),
        ];

        let filters = SearchFilters {
            q: Some("rust guide".to_owned()),
            tag: None,
            url: None,
            since: None,
            until: None,
            offset: 0,
            limit: 50,
        };

        let results = filter_and_rank(&bookmarks, &filters, now);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "2");
    }
}
