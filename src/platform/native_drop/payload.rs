use std::path::PathBuf;

use eframe::egui;

/// URL candidates from one drag, by how surely each names the dragged image.
/// Browsers write the dragged image element's markup as HTML, so its
/// `<img src>` is surest. A URL drag format names the image, or the link
/// around a linked image. Everything else, such as an HTML clip's SourceURL
/// page, link targets and prose, is context.
#[derive(Default)]
pub(super) struct DragUrls {
    image_sources: Vec<String>,
    url_formats: Vec<String>,
    context: Vec<String>,
}

impl DragUrls {
    /// Appends a value that holds one complete URL, such as a browser's URL
    /// drag format. Parentheses, quotes and brackets stay part of it.
    pub(super) fn append_url(&mut self, value: &str) {
        append_http_url(value, &mut self.url_formats);
    }

    pub(super) fn append_text(&mut self, text: &str) {
        for source in img_sources(text) {
            append_http_url(&source.replace("&amp;", "&"), &mut self.image_sources);
        }
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
            append_http_url_candidate(&candidate[..end], &mut self.context);
            offset = start + end;
        }
    }

    /// Picks the most image-like URL. An image extension decides first. On a
    /// tie, the surer kind of candidate wins, then the earlier one.
    pub(super) fn best(&self) -> Vec<String> {
        self.image_sources
            .iter()
            .chain(&self.url_formats)
            .chain(&self.context)
            .rev()
            .max_by_key(|url| drag_image_url_score(url))
            .cloned()
            .into_iter()
            .collect()
    }
}

// The src attribute of each <img> tag: whitespace may surround `=`, and the
// value may be quoted or bare. data-src and srcset are other attributes.
fn img_sources(html: &str) -> Vec<&str> {
    // ASCII lowercasing keeps byte offsets, so `lower` indexes `html`.
    let lower = html.to_ascii_lowercase();
    let mut sources = Vec::new();
    let mut offset = 0;
    while let Some(found) = lower[offset..].find("<img") {
        let tag_start = offset + found + "<img".len();
        let tag_end = lower[tag_start..]
            .find('>')
            .map_or(lower.len(), |end| tag_start + end);
        let tag = &lower[tag_start..tag_end];
        let mut search = 0;
        while let Some(found) = tag[search..].find("src") {
            let name = search + found;
            search = name + "src".len();
            if !tag[..name].ends_with(|ch: char| ch.is_ascii_whitespace()) {
                continue;
            }
            let Some(value) = tag[search..].trim_start().strip_prefix('=') else {
                continue;
            };
            let value_start = tag_start + tag.len() - value.trim_start().len();
            let value = &html[value_start..tag_end];
            sources.push(match value.chars().next() {
                Some(quote @ ('"' | '\'')) => value[1..].split(quote).next().unwrap_or(""),
                _ => value
                    .split(|ch: char| ch.is_ascii_whitespace())
                    .next()
                    .unwrap_or(""),
            });
            break;
        }
        offset = tag_end;
    }
    sources
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

    fn best_of_text(text: &str) -> Vec<String> {
        let mut urls = super::DragUrls::default();
        urls.append_text(text);
        urls.best()
    }

    #[test]
    fn text_selection_prefers_an_image_url_over_a_page_url() {
        assert_eq!(
            best_of_text("https://example.com/page http://cdn.example.com/photo.webp"),
            ["http://cdn.example.com/photo.webp"]
        );
    }

    #[test]
    fn img_source_beats_page_urls_around_it() {
        let image = "https://images.example.com/photo_(1)?w=800";
        for html in [
            // Windows HTML Format: a SourceURL header, then a linked image.
            format!(
                "SourceURL:https://example.com/gallery\r\n<a href=\"https://example.com/gallery\"><img src=\"{image}\"></a>"
            ),
            format!(r#"<a href="https://example.com/gallery"><img alt="x" src = "{image}"></a>"#),
            format!(r#"<img data-src="https://images.example.com/placeholder" src='{image}'>"#),
            format!(
                r#"<iframe src="https://example.com/embed"></iframe><IMG SRC={image} width=10>"#
            ),
        ] {
            assert_eq!(best_of_text(&html), [image], "{html}");
        }
    }

    #[test]
    fn linked_image_selects_the_img_source_over_the_link_url_format() {
        let mut urls = super::DragUrls::default();
        urls.append_url("https://example.com/gallery");
        urls.append_text(r#"<img src="https://images.example.com/photo-123?w=800">"#);
        assert_eq!(urls.best(), ["https://images.example.com/photo-123?w=800"]);
    }

    #[test]
    fn wrapper_urls_yield_their_nested_image() {
        assert_eq!(
            best_of_text(
                "https://www.google.com/imgres?imgurl=https%3A%2F%2Fcdn.example.com%2Fphoto.jpg&amp;imgrefurl=https%3A%2F%2Fexample.com%2Fpage"
            ),
            ["https://cdn.example.com/photo.jpg"]
        );
    }

    #[test]
    fn url_format_values_keep_characters_that_end_urls_in_text() {
        for value in [
            "https://example.com/photo_(1).jpg",
            "https://example.com/O'Brien.jpg",
            "http://[::1]:8765/web-drop.png",
        ] {
            let mut urls = super::DragUrls::default();
            urls.append_url(value);
            assert_eq!(urls.best(), [value]);
        }
        assert_eq!(
            best_of_text("(see https://example.com/a.jpg)"),
            ["https://example.com/a.jpg"]
        );
    }

    #[test]
    fn ignores_non_http_urls() {
        assert_eq!(
            best_of_text("file:///tmp/test.png ftp://example.com/photo.png https://example.com/ok"),
            ["https://example.com/ok"]
        );
    }
}
