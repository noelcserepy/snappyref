//! Semver comparison used to decide whether a manifest's version is newer than
//! the running app.
//!
//! Build metadata is ignored per semver spec; pre-releases are ordered below
//! their associated normal release.

use semver::Version;

/// Returns `true` when `candidate` strictly outranks `current` per semver
/// precedence (build metadata ignored).
///
/// Invalid versions on either side return `false` rather than panicking — the
/// caller treats that as "not newer" so a malformed manifest can never
/// trigger the Available state.
pub(crate) fn is_newer_than(candidate: &str, current: &str) -> bool {
    match (Version::parse(current), Version::parse(candidate)) {
        (Ok(current), Ok(candidate)) => candidate.cmp_precedence(&current).is_gt(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_release_is_newer_than_lower_patch() {
        assert!(is_newer_than("0.1.1", "0.1.0"));
        assert!(is_newer_than("0.2.0", "0.1.9"));
        assert!(is_newer_than("2.0.0", "1.2.3"));
    }

    #[test]
    fn equal_versions_are_not_newer() {
        assert!(!is_newer_than("0.1.0", "0.1.0"));
    }

    #[test]
    fn lower_versions_are_not_newer() {
        assert!(!is_newer_than("1.2.2", "1.2.3"));
        assert!(!is_newer_than("1.99.99", "2.0.0"));
    }

    #[test]
    fn build_metadata_is_ignored() {
        // Build metadata MUST be ignored when determining precedence.
        assert!(!is_newer_than("1.0.0+build.5", "1.0.0"));
        assert!(!is_newer_than("1.0.0", "1.0.0+build.5"));
        assert!(!is_newer_than("1.0.0+b", "1.0.0+a"));
    }

    #[test]
    fn pre_release_ranks_below_associated_normal_release() {
        assert!(is_newer_than("1.0.0", "1.0.0-alpha"));
        assert!(!is_newer_than("1.0.0-alpha", "1.0.0"));
    }

    #[test]
    fn invalid_versions_are_never_newer() {
        assert!(!is_newer_than("1.0.0", "not-a-version"));
        assert!(!is_newer_than("also-not-a-version", "1.0.0"));
        assert!(!is_newer_than("", ""));
    }
}
