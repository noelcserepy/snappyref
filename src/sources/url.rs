pub(crate) fn normalize_import_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    ((parsed.scheme() == "http") || (parsed.scheme() == "https")).then(|| parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::normalize_import_url;
    #[test]
    fn normalize_import_url_accepts_http_and_https_only() {
        assert_eq!(
            normalize_import_url(" https://example.com/cat.png?size=large "),
            Some("https://example.com/cat.png?size=large".to_owned())
        );
        assert_eq!(
            normalize_import_url("http://example.com/dog.gif"),
            Some("http://example.com/dog.gif".to_owned())
        );
        assert_eq!(normalize_import_url("file:///tmp/cat.png"), None);
        assert_eq!(normalize_import_url("not a url"), None);
    }
}
