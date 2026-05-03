use anyhow::{Result, bail};
use regex::Regex;

use crate::models::ImportBookmarkItem;

pub fn parse_browser_bookmarks_html(html: &str) -> Result<Vec<ImportBookmarkItem>> {
    let token_re = Regex::new(
        r#"(?is)<h3\b(?P<h3_attrs>[^>]*)>(?P<h3_text>.*?)</h3>|<a\b(?P<a_attrs>[^>]*)>(?P<a_text>.*?)</a>|<dl\b[^>]*>|</dl>"#,
    )?;
    let href_re = Regex::new(r#"(?is)\bhref\s*=\s*(?:"([^"]*)"|'([^']*)')"#)?;
    let tag_re = Regex::new(r"(?is)<[^>]+>")?;

    let mut items = Vec::new();
    let mut folders = Vec::new();
    let mut pending_folder = None;
    let mut dl_stack = Vec::new();

    for captures in token_re.captures_iter(html) {
        if let Some(attrs) = captures.name("a_attrs") {
            let Some(href_match) = href_re.captures(attrs.as_str()) else {
                continue;
            };

            let href = href_match
                .get(1)
                .or_else(|| href_match.get(2))
                .map(|value| decode_html_entities(value.as_str()))
                .unwrap_or_default();
            if href.trim().is_empty() {
                continue;
            }

            let title = captures
                .name("a_text")
                .map(|value| normalize_text(value.as_str(), &tag_re))
                .unwrap_or_default();

            items.push(ImportBookmarkItem {
                url: href,
                title,
                description: String::new(),
                tags: folders.clone(),
            });
            continue;
        }

        if let Some(folder_name) = captures.name("h3_text") {
            let folder_name = normalize_text(folder_name.as_str(), &tag_re);
            pending_folder = (!folder_name.is_empty()).then_some(folder_name);
            continue;
        }

        let token = captures
            .get(0)
            .map(|value| value.as_str())
            .unwrap_or_default();
        if token.to_ascii_lowercase().starts_with("<dl") {
            if let Some(folder_name) = pending_folder.take() {
                folders.push(folder_name);
                dl_stack.push(true);
            } else {
                dl_stack.push(false);
            }
            continue;
        }

        if token.eq_ignore_ascii_case("</dl>") && dl_stack.pop().unwrap_or(false) {
            folders.pop();
        }
    }

    if items.is_empty() {
        bail!("No bookmarks were found in the browser export HTML");
    }

    Ok(items)
}

fn normalize_text(raw: &str, tag_re: &Regex) -> String {
    let without_tags = tag_re.replace_all(raw, "");
    collapse_whitespace(&decode_html_entities(without_tags.as_ref()))
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_html_entities(input: &str) -> String {
    let mut decoded = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '&' {
            decoded.push(ch);
            continue;
        }

        let mut entity = String::new();
        while let Some(next) = chars.peek().copied() {
            entity.push(next);
            chars.next();
            if next == ';' || entity.len() > 16 {
                break;
            }
        }

        match decode_entity(&entity) {
            Some(value) => decoded.push(value),
            None => {
                decoded.push('&');
                decoded.push_str(&entity);
            }
        }
    }

    decoded
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp;" => Some('&'),
        "lt;" => Some('<'),
        "gt;" => Some('>'),
        "quot;" => Some('"'),
        "apos;" | "#39;" => Some('\''),
        _ if entity.starts_with("#x") && entity.ends_with(';') => {
            u32::from_str_radix(&entity[2..entity.len() - 1], 16)
                .ok()
                .and_then(char::from_u32)
        }
        _ if entity.starts_with('#') && entity.ends_with(';') => entity[1..entity.len() - 1]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_browser_bookmarks_html;

    #[test]
    fn parses_netscape_bookmark_html_with_folder_tags() {
        let html = r#"
<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
  <DT><H3>Bookmarks Bar</H3>
  <DL><p>
    <DT><A HREF="https://example.com/docs/">Example &amp; Docs</A>
    <DT><H3>Rust</H3>
    <DL><p>
      <DT><A HREF="https://rust-lang.org/">Rust Lang</A>
    </DL><p>
  </DL><p>
</DL><p>
"#;

        let items = parse_browser_bookmarks_html(html).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].url, "https://example.com/docs/");
        assert_eq!(items[0].title, "Example & Docs");
        assert_eq!(items[0].tags, vec!["Bookmarks Bar"]);
        assert_eq!(items[1].tags, vec!["Bookmarks Bar", "Rust"]);
    }

    #[test]
    fn errors_when_no_bookmarks_exist() {
        let html = "<html><body><h1>Empty</h1></body></html>";
        assert!(parse_browser_bookmarks_html(html).is_err());
    }
}
