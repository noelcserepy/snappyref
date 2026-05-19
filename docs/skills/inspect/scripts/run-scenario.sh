#!/usr/bin/env bash
set -u

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/../../../.." && pwd)"

scenario="${1:-}"
if [[ -z "$scenario" ]]; then
  echo "status=fail"
  echo "error=missing scenario"
  echo "usage: $0 <scenario>"
  exit 2
fi

if [[ "$scenario" == "--help" || "$scenario" == "-h" ]]; then
  echo "usage: $0 <scenario>"
  echo "scenarios:"
  echo "  check-harness"
  echo "  search-drag-to-canvas"
  echo "  semantic-search-install"
  echo "  semantic-search-sidebar-status"
  echo "  semantic-search-real-install"
  echo "  sample-image-import"
  echo "  blue-image-search-drag"
  echo "  search-palette-footer"
  echo "  compact-equal-size-images"
  echo "  compact-mixed-column-images"
  echo "  compact-windows-column-images"
  exit 0
fi

if [[ "$scenario" == "--list" ]]; then
  echo "check-harness"
  echo "search-drag-to-canvas"
  echo "semantic-search-install"
  echo "semantic-search-sidebar-status"
  echo "semantic-search-real-install"
  echo "sample-image-import"
  echo "blue-image-search-drag"
  echo "search-palette-footer"
  echo "compact-equal-size-images"
  echo "compact-mixed-column-images"
  echo "compact-windows-column-images"
  exit 0
fi

case "$scenario" in
  search-drag-to-canvas)
    test_filter="inspect_search_result_drag_to_canvas_writes_artifact"
    ;;
  check-harness)
    test_filter="inspect_search_result_drag_to_canvas_writes_artifact"
    ;;
  semantic-search-install)
    test_filter="inspect_semantic_search_install_accepts_prompt"
    ;;
  semantic-search-sidebar-status)
    test_filter="inspect_semantic_search_sidebar_status_writes_artifact"
    ;;
  semantic-search-real-install)
    test_filter="inspect_semantic_search_real_install_writes_artifact"
    ;;
  sample-image-import)
    test_filter="inspect_sample_image_import_writes_artifact"
    ;;
  blue-image-search-drag)
    test_filter="inspect_blue_image_search_drag_writes_artifact"
    ;;
  search-palette-footer)
    test_filter="inspect_search_palette_footer_writes_artifact"
    ;;
  compact-equal-size-images)
    test_filter="inspect_compact_equal_size_images_writes_artifact"
    ;;
  compact-mixed-column-images)
    test_filter="inspect_compact_mixed_column_images_writes_artifact"
    ;;
  compact-windows-column-images)
    test_filter="inspect_compact_windows_column_images_writes_artifact"
    ;;
  *)
    echo "status=fail"
    echo "scenario=$scenario"
    echo "error=unknown scenario"
    exit 2
    ;;
esac

artifact_root="$repo_root/target/snappyref-inspect/$scenario"
rm -f "$artifact_root/latest.txt"

(
  cd "$repo_root"
  SNAPPYREF_INSPECT_SCENARIO="$scenario" cargo test "$test_filter" --features semantic-search -- --nocapture
)
cargo_status=$?

artifact=""
if [[ -f "$artifact_root/latest.txt" ]]; then
  artifact="$(cat "$artifact_root/latest.txt")"
elif [[ "$scenario" == "check-harness" && -f "$repo_root/target/snappyref-inspect/search-drag-to-canvas/latest.txt" ]]; then
  artifact="$(cat "$repo_root/target/snappyref-inspect/search-drag-to-canvas/latest.txt")"
fi

if [[ "$cargo_status" -eq 0 ]]; then
  echo "status=pass"
else
  echo "status=fail"
fi
echo "scenario=$scenario"
if [[ -n "$artifact" ]]; then
  echo "artifact=$artifact"
fi

exit "$cargo_status"
