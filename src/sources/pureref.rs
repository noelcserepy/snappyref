use std::path::PathBuf;

use crate::image_preparation::ImportSource;
use crate::sources::url::normalize_import_url;

const PURE_REF_IMAGE_ITEM: &str = "GraphicsImageItem";

pub(crate) fn sources_from_binary(bytes: &[u8]) -> Vec<ImportSource> {
    let strings = length_prefixed_utf16be_strings(bytes);
    let mut sources = Vec::new();

    for pair in strings.windows(2) {
        if pair[0] != PURE_REF_IMAGE_ITEM {
            continue;
        }
        if let Some(source) = import_source_from_pureref_source(&pair[1])
            && !sources.contains(&source)
        {
            sources.push(source);
        }
    }

    sources
}

fn import_source_from_pureref_source(raw: &str) -> Option<ImportSource> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(url) = normalize_import_url(value) {
        return Some(ImportSource::Url(url));
    }
    if is_windows_path(value) || is_unc_path(value) {
        return Some(ImportSource::Path(PathBuf::from(value)));
    }
    None
}

fn is_windows_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn is_unc_path(value: &str) -> bool {
    value.starts_with("\\\\")
}

fn length_prefixed_utf16be_strings(bytes: &[u8]) -> Vec<String> {
    let mut strings = Vec::new();
    let mut index = 0;

    while index + 4 <= bytes.len() {
        let length = u32::from_be_bytes([
            bytes[index],
            bytes[index + 1],
            bytes[index + 2],
            bytes[index + 3],
        ]) as usize;
        let start = index + 4;
        let end = start.saturating_add(length);

        if length > 0
            && length % 2 == 0
            && end <= bytes.len()
            && let Some(value) = decode_utf16be_string(&bytes[start..end])
        {
            strings.push(value);
            index = end;
            continue;
        }

        index += 1;
    }

    strings
}

fn decode_utf16be_string(bytes: &[u8]) -> Option<String> {
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    let value = String::from_utf16(&units).ok()?;
    let trimmed = value.trim_matches(char::from(0)).trim().to_owned();
    (!trimmed.is_empty() && trimmed.chars().all(is_supported_pureref_string_char))
        .then_some(trimmed)
}

fn is_supported_pureref_string_char(character: char) -> bool {
    character == '\n' || character == '\r' || character == '\t' || !character.is_control()
}

#[cfg(test)]
mod tests {
    use super::sources_from_binary;
    use crate::image_preparation::ImportSource;

    #[test]
    fn extracts_image_item_urls_and_paths() {
        let bytes = payload(&[
            "selection-id",
            "GraphicsImageItem",
            "https://example.com/a.jpg",
            "a",
            "GraphicsImageItem",
            "C:/Users/Admin/Downloads/reference.webp",
            "reference",
        ]);

        assert_eq!(
            sources_from_binary(&bytes),
            vec![
                ImportSource::Url("https://example.com/a.jpg".to_owned()),
                ImportSource::Path("C:/Users/Admin/Downloads/reference.webp".into()),
            ]
        );
    }

    #[test]
    fn ignores_names_and_deduplicates_sources() {
        let bytes = payload(&[
            "GraphicsImageItem",
            "https://example.com/a.jpg",
            "a",
            "GraphicsImageItem",
            "https://example.com/a.jpg",
            "a-copy",
            "GraphicsImageItem",
            "not-a-source",
        ]);

        assert_eq!(
            sources_from_binary(&bytes),
            vec![ImportSource::Url("https://example.com/a.jpg".to_owned())]
        );
    }

    fn payload(strings: &[&str]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in strings {
            bytes.extend([0x12, 0x34, 0x56]);
            let encoded = value
                .encode_utf16()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>();
            bytes.extend((encoded.len() as u32).to_be_bytes());
            bytes.extend(encoded);
        }
        bytes
    }
}
