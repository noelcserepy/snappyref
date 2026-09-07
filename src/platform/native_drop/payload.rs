use std::path::PathBuf;

use eframe::egui;

pub(super) fn append_http_urls_from_text(text: &str, urls: &mut Vec<String>) {
    let mut remainder = text;
    while let Some(start) = remainder
        .find("http://")
        .or_else(|| remainder.find("https://"))
    {
        let candidate = &remainder[start..];
        let end = candidate
            .find(|ch: char| {
                ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']')
            })
            .unwrap_or(candidate.len());
        append_http_url_candidate(&candidate[..end], urls);
        remainder = &candidate[end..];
    }
}

fn append_http_url_candidate(raw: &str, urls: &mut Vec<String>) {
    let trimmed = raw.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']' | ',' | ';'
        )
    });
    let unescaped = trimmed.replace("&amp;", "&");
    let Ok(parsed) = url::Url::parse(&unescaped) else {
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

pub(super) fn best_drag_image_urls(urls: Vec<String>) -> Vec<String> {
    urls.into_iter()
        .max_by_key(|url| drag_image_url_score(url))
        .into_iter()
        .collect()
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
        let mut urls = Vec::new();
        super::append_http_urls_from_text(
            "https://example.com/a.jpg\nhttps://example.com/b.png",
            &mut urls,
        );
        assert_eq!(
            urls,
            vec![
                "https://example.com/a.jpg".to_owned(),
                "https://example.com/b.png".to_owned()
            ]
        );
    }

    #[test]
    fn extracts_http_urls_embedded_in_html() {
        let mut urls = Vec::new();
        super::append_http_urls_from_text(
            r#"<img src="https://images.example.com/photo.webp"><a href="https://example.com/page">"#,
            &mut urls,
        );
        assert_eq!(
            urls,
            vec![
                "https://images.example.com/photo.webp".to_owned(),
                "https://example.com/page".to_owned()
            ]
        );
    }

    #[test]
    fn extracts_nested_image_urls_from_wrapper_urls() {
        let mut urls = Vec::new();
        super::append_http_urls_from_text(
            "https://www.google.com/imgres?imgurl=https%3A%2F%2Fcdn.example.com%2Fphoto.jpg&amp;imgrefurl=https%3A%2F%2Fexample.com%2Fpage",
            &mut urls,
        );
        assert_eq!(
            urls,
            vec![
                "https://cdn.example.com/photo.jpg".to_owned(),
                "https://www.google.com/imgres?imgurl=https%3A%2F%2Fcdn.example.com%2Fphoto.jpg&imgrefurl=https%3A%2F%2Fexample.com%2Fpage".to_owned()
            ]
        );
    }

    #[test]
    fn drag_url_selection_prefers_actual_image_over_page_url() {
        let urls = super::best_drag_image_urls(vec![
            "https://example.com/page".to_owned(),
            "https://cdn.example.com/photo.webp".to_owned(),
        ]);
        assert_eq!(urls, vec!["https://cdn.example.com/photo.webp".to_owned()]);
    }

    #[test]
    fn ignores_non_http_urls() {
        let mut urls = Vec::new();
        super::append_http_urls_from_text(
            "file:///tmp/test.png ftp://example.com/photo.png https://example.com/ok.jpg",
            &mut urls,
        );
        assert_eq!(urls, vec!["https://example.com/ok.jpg".to_owned()]);
    }
}
