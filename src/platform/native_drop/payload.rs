use std::path::PathBuf;

use eframe::egui;

/// URL candidates from one drag. `object` holds URLs that name the dragged
/// item: URL drag formats and `<img src>` values. `context` holds the rest,
/// such as an HTML clip's SourceURL page, link targets and prose.
#[derive(Default)]
pub(super) struct DragUrls {
    object: Vec<String>,
    context: Vec<String>,
}

impl DragUrls {
    /// Appends a value that holds one complete URL, such as a browser's URL
    /// drag format. Parentheses, quotes and brackets stay part of it.
    pub(super) fn append_url(&mut self, value: &str) {
        append_http_url(value, &mut self.object);
    }

    pub(super) fn append_text(&mut self, text: &str) {
        let mut offset = 0;
        while let Some(start) = [
            text[offset..].find("http://"),
            text[offset..].find("https://"),
        ]
        .into_iter()
        .flatten()
        .min()
        {
            let start = offset + start;
            let candidate = &text[start..];
            let end = candidate
                .find(|ch: char| {
                    ch.is_whitespace()
                        || matches!(ch, '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']')
                })
                .unwrap_or(candidate.len());
            let urls = if is_image_source(&text[..start]) {
                &mut self.object
            } else {
                &mut self.context
            };
            append_http_url_candidate(&candidate[..end], urls);
            offset = start + end;
        }
    }

    /// Picks the most image-like URL. An image extension decides first. On a
    /// tie, object URLs beat context, and earlier candidates beat later ones,
    /// so callers append the formats that name the dragged item first.
    pub(super) fn best(&self) -> Vec<String> {
        self.object
            .iter()
            .chain(&self.context)
            .rev()
            .max_by_key(|url| drag_image_url_score(url))
            .cloned()
            .into_iter()
            .collect()
    }
}

fn is_image_source(before: &str) -> bool {
    let before = before.trim_end_matches(['"', '\'']);
    before
        .get(before.len().saturating_sub(4)..)
        .is_some_and(|tail| tail.eq_ignore_ascii_case("src="))
}

fn append_http_url_candidate(raw: &str, urls: &mut Vec<String>) {
    let trimmed = raw.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']' | ',' | ';'
        )
    });
    append_http_url(&trimmed.replace("&amp;", "&"), urls);
}

fn append_http_url(value: &str, urls: &mut Vec<String>) {
    let Ok(parsed) = url::Url::parse(value.trim()) else {
        return;
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return;
    }
    for (_, value) in parsed.query_pairs().filter(|(key, _)| {
        matches!(
            key.as_ref().to_ascii_lowercase().as_str(),
            "imgurl" | "mediaurl" | "imageurl" | "src"
        )
    }) {
        append_http_url_candidate(&value, urls);
    }
    let normalized = parsed.to_string();
    if !urls.contains(&normalized) {
        urls.push(normalized);
    }
}

fn drag_image_url_score(url: &str) -> i32 {
    let Ok(parsed) = url::Url::parse(url) else {
        return 0;
    };
    let mut score = 0;
    let path = parsed.path().to_ascii_lowercase();
    if matches!(
        path.rsplit('.').next(),
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "avif" | "bmp" | "tif" | "tiff")
    ) {
        score += 100;
    }
    if parsed.query_pairs().any(|(key, _)| {
        matches!(
            key.as_ref().to_ascii_lowercase().as_str(),
            "imgurl" | "mediaurl" | "imageurl" | "src"
        )
    }) {
        score -= 25;
    }
    if parsed
        .host_str()
        .is_some_and(|host| host.contains("google.") || host.contains("bing."))
    {
        score -= 10;
    }
    score
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NativeDropPayload {
    pub(crate) urls: Vec<String>,
    pub(crate) paths: Vec<PathBuf>,
    // Window-local egui points, ready for Canvas screen_to_world conversion.
    pub(crate) screen_pos: Option<egui::Pos2>,
}

impl NativeDropPayload {
    pub(super) fn is_empty(&self) -> bool {
        self.urls.is_empty() && self.paths.is_empty()
    }
}

#[derive(Default)]
pub(super) struct NativeDropState {
    pub(super) hovered: NativeDropPayload,
    pub(super) dropped: NativeDropPayload,
}

impl NativeDropState {
    pub(super) fn set_hovered(&mut self, mut payload: NativeDropPayload, pos: Option<egui::Pos2>) {
        payload.screen_pos = pos;
        self.hovered = payload;
    }

    pub(super) fn set_hovered_pos(&mut self, pos: Option<egui::Pos2>) {
        self.hovered.screen_pos = pos;
    }

    pub(super) fn clear_hovered(&mut self) {
        self.hovered = NativeDropPayload::default();
    }

    pub(super) fn set_dropped(&mut self, payload: NativeDropPayload, pos: Option<egui::Pos2>) {
        self.clear_hovered();
        self.dropped.urls.extend(payload.urls);
        self.dropped.paths.extend(payload.paths);
        self.dropped.screen_pos = pos;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn drop_consumes_paths_urls_and_position_together_and_clears_hover() {
        use super::{NativeDropPayload, NativeDropState};
        let payload = NativeDropPayload {
            paths: vec!["reference.png".into()],
            urls: vec!["https://example.com/reference.png".into()],
            screen_pos: None,
        };
        let mut state = NativeDropState::default();
        state.set_hovered(payload.clone(), Some(eframe::egui::pos2(10.0, 20.0)));
        let position = Some(eframe::egui::pos2(30.0, 40.0));
        state.set_dropped(payload.clone(), position);
        assert_eq!(state.hovered, NativeDropPayload::default());
        let dropped = std::mem::take(&mut state.dropped);
        assert_eq!(dropped.paths, payload.paths);
        assert_eq!(dropped.urls, payload.urls);
        assert_eq!(dropped.screen_pos, position);
        assert_eq!(state.dropped, NativeDropPayload::default());
    }

    #[test]
    fn extracts_plain_text_http_urls() {
        let mut urls = super::DragUrls::default();
        urls.append_text("https://example.com/a.jpg\nhttp://example.com/b.png");
        assert_eq!(
            urls.context,
            ["https://example.com/a.jpg", "http://example.com/b.png"]
        );
    }

    #[test]
    fn html_image_sources_name_the_object_and_links_give_context() {
        let mut urls = super::DragUrls::default();
        urls.append_text(
            r#"<img src="https://images.example.com/photo.webp"><a href="https://example.com/page">"#,
        );
        assert_eq!(urls.object, ["https://images.example.com/photo.webp"]);
        assert_eq!(urls.context, ["https://example.com/page"]);
    }

    #[test]
    fn extracts_nested_image_urls_from_wrapper_urls() {
        let mut urls = super::DragUrls::default();
        urls.append_text(
            "https://www.google.com/imgres?imgurl=https%3A%2F%2Fcdn.example.com%2Fphoto.jpg&amp;imgrefurl=https%3A%2F%2Fexample.com%2Fpage",
        );
        assert_eq!(
            urls.context,
            vec![
                "https://cdn.example.com/photo.jpg".to_owned(),
                "https://www.google.com/imgres?imgurl=https%3A%2F%2Fcdn.example.com%2Fphoto.jpg&imgrefurl=https%3A%2F%2Fexample.com%2Fpage".to_owned()
            ]
        );
    }

    #[test]
    fn drag_url_selection_prefers_actual_image_over_page_url() {
        let mut urls = super::DragUrls::default();
        urls.append_text("https://example.com/page https://cdn.example.com/photo.webp");
        assert_eq!(urls.best(), ["https://cdn.example.com/photo.webp"]);
    }

    #[test]
    fn extensionless_image_source_beats_page_urls_around_it() {
        // Windows HTML Format: a SourceURL header, then a linked image.
        let mut urls = super::DragUrls::default();
        urls.append_text(concat!(
            "SourceURL:https://example.com/gallery\r\n",
            r#"<a href="https://example.com/gallery"><img src="https://images.example.com/photo-123?w=800"></a>"#,
        ));
        assert_eq!(urls.best(), ["https://images.example.com/photo-123?w=800"]);
    }

    #[test]
    fn url_format_values_keep_characters_that_end_urls_in_text() {
        let mut urls = super::DragUrls::default();
        for value in [
            "https://example.com/photo_(1).jpg",
            "https://example.com/O'Brien.jpg",
            "http://[::1]:8765/web-drop.png",
        ] {
            urls.append_url(value);
        }
        assert_eq!(
            urls.object,
            [
                "https://example.com/photo_(1).jpg",
                "https://example.com/O'Brien.jpg",
                "http://[::1]:8765/web-drop.png"
            ]
        );
        urls.append_text("(see https://example.com/a.jpg)");
        assert_eq!(urls.context, ["https://example.com/a.jpg"]);
    }

    #[test]
    fn ignores_non_http_urls() {
        let mut urls = super::DragUrls::default();
        urls.append_text(
            "file:///tmp/test.png ftp://example.com/photo.png https://example.com/ok.jpg",
        );
        assert_eq!(urls.context, ["https://example.com/ok.jpg"]);
    }
}
