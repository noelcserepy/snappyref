use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ExternalImportSource {
    Path(PathBuf),
    Url(String),
}

pub(crate) fn normalize_import_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    ((parsed.scheme() == "http") || (parsed.scheme() == "https")).then(|| parsed.to_string())
}
