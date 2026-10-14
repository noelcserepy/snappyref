#!/usr/bin/env bash
# Upload app zips, and latest.json when given, to this repo's draft release for
# <tag>, creating the draft when the tag has none.
#
# Installed apps check each zip against the signed latest.json, so a release's
# zips and manifest must always match:
# - a published release is never rewritten
# - a draft that holds a signed latest.json takes new zips only together with
#   a new latest.json
#
# Usage: upload-release-assets.sh <tag> <target-sha or ""> <file>...
set -euo pipefail

tag="$1"
target="$2"
shift 2

uploads_manifest=false
for file in "$@"; do
  [[ "$(basename "$file")" == latest.json ]] && uploads_manifest=true
done

if ! gh release view "$tag" >/dev/null 2>&1; then
  gh release create "$tag" ${target:+--target "$target"} \
    --title "SnappyRef $tag" \
    --notes "Draft SnappyRef release." \
    --draft
else
  read -r is_draft has_manifest < <(gh release view "$tag" --json isDraft,assets \
    --jq '"\(.isDraft) \(any(.assets[]; .name == "latest.json"))"')
  if [[ "$is_draft" != true ]]; then
    echo "$tag is already published. Bump the version for a new release." >&2
    exit 1
  fi
  if [[ "$has_manifest" == true && "$uploads_manifest" == false ]]; then
    echo "The $tag draft holds a signed latest.json for its current zips." >&2
    echo "Rerun Publish Full Release with publish_update, or delete the draft first." >&2
    exit 1
  fi
  # A draft keeps its first target until publishing creates the tag.
  if [[ -n "$target" ]]; then
    gh release edit "$tag" --target "$target"
  fi
fi

gh release upload "$tag" "$@" --clobber
