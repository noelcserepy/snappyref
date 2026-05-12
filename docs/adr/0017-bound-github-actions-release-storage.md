# Bound GitHub Actions release storage

The full release workflow builds macOS, Windows, and model outputs, then passes
large intermediate archives between jobs as GitHub Actions artifacts. Release
builds also used Rust build caches with job- and commit-shaped keys. Repeated
runs accumulated more than 10 GB of Actions cache storage and caused artifact
uploads to fail with GitHub's storage-quota error before the release workflow
could package or publish assets.

## Decision

SnappyRef keeps release workflow storage bounded:

- The pull-request CI workflow is removed. Formatting and cargo checks are
  enforced by the local pre-push hook instead.
- macOS and Windows release app builds still use Rust caching, but with stable
  release cache families:
  - `release-macos-{build_profile}`
  - `release-windows-{build_profile}`
- Release build caches are saved only for `main` builds.
- The full release workflow prunes release caches after platform builds and
  keeps only the latest cache for each included platform/build-profile family.
- GitHub Actions artifacts remain short-lived job handoff files with
  `retention-days: 7`; durable release files belong on GitHub Releases in the
  source repo or `snappyref-dist`, not in Actions artifact storage.

## Consequences

- Repeated full release runs should no longer fill the repo's Actions cache
  quota through per-run Rust cache churn.
- The next release build still benefits from the most recent platform cache.
- GitHub caches are immutable, so dependency or toolchain changes can create a
  new cache entry; the prune job is responsible for deleting older entries in
  the same release cache family.
- If GitHub artifact quota is exceeded, deleting artifacts and caches may still
  require GitHub's storage accounting window to refresh before uploads work
  again.
- Removing PR CI means local hook setup is more important on fresh clones:
  `git config core.hooksPath .githooks`.
