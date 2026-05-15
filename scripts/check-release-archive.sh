#!/usr/bin/env bash
# Validate an app-only release archive: binary in place, Semantic Search assets
# absent, no user-owned data leaked, no UPDATE.txt emitted by this pipeline.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <archive.zip> <binary-name>" >&2
  exit 2
fi

archive=$1
binary_name=$2

if [[ ! -f "$archive" ]]; then
  echo "missing archive: $archive" >&2
  exit 1
fi

entries=$(zipinfo -1 "$archive" | sed 's#\\#/#g')

require_entry() {
  local entry=$1
  if ! grep -Fxq "$entry" <<<"$entries"; then
    echo "missing required app-owned path: $entry" >&2
    exit 1
  fi
}

forbid_entry() {
  local entry=$1
  if grep -Fxq "$entry" <<<"$entries"; then
    echo "archive contains forbidden path: $entry" >&2
    exit 1
  fi
}

forbid_path() {
  local path=$1
  while IFS= read -r entry; do
    case "$entry" in
      "$path"|"$path"/*)
        echo "archive includes forbidden user-owned path: $path" >&2
        exit 1
        ;;
    esac
  done <<<"$entries"
}

require_entry "$binary_name"

# Derive the install-root prefix (SnappyRef/) from the binary path.
case "$binary_name" in
  SnappyRef/SnappyRef.app/Contents/MacOS/*)
    install_prefix="SnappyRef"
    require_entry "$install_prefix/SnappyRef.app/Contents/Info.plist"
    require_entry "$install_prefix/SnappyRef.app/Contents/Resources/snappyref.icns"
    # ADR-0015: macOS keeps a helper inside the bundle for app-bundle swaps.
    require_entry "$install_prefix/SnappyRef.app/Contents/MacOS/snappyref-updater"
    ;;
  SnappyRef/*)
    install_prefix="SnappyRef"
    ;;
  *)
    echo "unexpected binary path: $binary_name" >&2
    exit 1
    ;;
esac

# UPDATE.txt is no longer emitted by any packaging script.
forbid_entry "$install_prefix/UPDATE.txt"
forbid_entry "UPDATE.txt"

# Semantic Search assets must not be shipped in app release zips.
if grep -E "^$install_prefix/models(/|$)" <<<"$entries" >/dev/null; then
  echo "archive contains Semantic Search models directory" >&2
  exit 1
fi

# Models must not be left inside the macOS bundle either.
case "$binary_name" in
  SnappyRef/SnappyRef.app/Contents/MacOS/*)
    if grep -E "^$install_prefix/SnappyRef\\.app/Contents/Resources/models(/|$)" <<<"$entries" >/dev/null; then
      echo "archive contains in-bundle models directory" >&2
      exit 1
    fi
    ;;
esac

# User-owned data must never appear inside a release zip.
forbid_path "$install_prefix/library"
forbid_path "$install_prefix/library.db"
forbid_path "$install_prefix/originals"
forbid_path "$install_prefix/thumbs"
