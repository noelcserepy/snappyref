#!/usr/bin/env python3
"""Inspect scenario catalogue and required artifact evidence."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
# name: test filter, artifact scenario, refactor coverage, network/ignored
SCENARIOS = {
    'group-dock': ('inspect_group_dock_writes_artifact', 'group-dock', False, False),
    'member-dock': ('inspect_member_dock_writes_artifact', 'member-dock', False, False),
    'annotation-editing': ('inspect_annotation_creation_editing_and_styles_writes_artifact', 'annotation-editing', False, False),
    'annotation-endpoint-drag': ('inspect_annotation_endpoint_drag_writes_artifact', 'annotation-endpoint-drag', False, False),
    'image-format-gallery': ('inspect_image_format_gallery_writes_artifact', 'image-format-gallery', False, True),
    'group-picker-rename-create': ('inspect_group_picker_rename_and_create_writes_artifact', 'group-picker-rename-create', False, False),
    'check-harness': ('inspect_search_result_drag_to_canvas_writes_artifact', 'search-drag-to-canvas', False, False),
    'group-operations': ('inspect_group_operations_writes_artifact', 'group-operations', True, False),
    'search-drag-to-canvas': ('inspect_search_result_drag_to_canvas_writes_artifact', 'search-drag-to-canvas', True, False),
    'semantic-search-install': ('inspect_semantic_search_install_accepts_prompt', 'semantic-search-install', False, False),
    'semantic-search-sidebar-status': ('inspect_semantic_search_sidebar_status_writes_artifact', 'semantic-search-sidebar-status', False, False),
    'semantic-search-real-install': ('inspect_semantic_search_real_install_writes_artifact', 'semantic-search-real-install', False, True),
    'sample-image-import': ('inspect_sample_image_import_writes_artifact', 'sample-image-import', True, False),
    'pending-drop-batch-undo': ('inspect_pending_drop_batch_undo_writes_artifact', 'pending-drop-batch-undo', True, False),
    'blue-image-search-drag': ('inspect_blue_image_search_drag_writes_artifact', 'blue-image-search-drag', True, False),
    'delete-board-modal-flash': ('inspect_delete_board_modal_flash_writes_artifact', 'delete-board-modal-flash', False, False),
    'board-lifecycle-undo-redo': ('inspect_board_lifecycle_undo_redo_writes_artifact', 'board-lifecycle-undo-redo', True, False),
    'search-palette-footer': ('inspect_search_palette_footer_writes_artifact', 'search-palette-footer', False, False),
    'placement-click-hold-flash': ('inspect_placement_click_hold_flash_writes_artifact', 'placement-click-hold-flash', False, False),
    'delete-undo-redo-placement': ('inspect_delete_undo_redo_placement_writes_artifact', 'delete-undo-redo-placement', True, False),
    'crop-undo-redo-placement': ('inspect_crop_undo_redo_placement_writes_artifact', 'crop-undo-redo-placement', True, False),
    'compact-equal-size-images': ('inspect_compact_equal_size_images_writes_artifact', 'compact-equal-size-images', False, False),
    'compact-mixed-column-images': ('inspect_compact_mixed_column_images_writes_artifact', 'compact-mixed-column-images', False, False),
    'compact-windows-column-images': ('inspect_compact_windows_column_images_writes_artifact', 'compact-windows-column-images', False, False),
    'canvas-edit-operations': ('inspect_canvas_edit_operations_writes_artifact', 'canvas-edit-operations', True, False),
    'canvas-navigation-boards': ('inspect_canvas_navigation_and_boards_writes_artifact', 'canvas-navigation-boards', True, False),
    'session-restart-restore': ('inspect_session_restart_restore_writes_artifact', 'session-restart-restore', True, False),
    'persistence-write-retry': ('inspect_persistence_write_retry_writes_artifact', 'persistence-write-retry', True, False),
}


def inspect_root(root=ROOT):
    # Mirrors inspect_artifact_dir in src/app/tests/support.rs.
    target = Path(os.environ.get('CARGO_TARGET_DIR', 'target'))
    return (target if target.is_absolute() else root / target) / 'snappyref-inspect'


def check_artifact(scenario, started, root=ROOT):
    latest = inspect_root(root) / scenario / 'latest.txt'
    if not latest.is_file() or latest.stat().st_mtime < started:
        raise RuntimeError(f'missing or stale inspect artifact: {scenario}')
    artifact = Path(latest.read_text().strip())
    report_path = artifact / 'report.json'
    if not report_path.is_file() or report_path.stat().st_mtime < started:
        raise RuntimeError(f'missing or stale inspect report: {scenario}')
    report = json.loads(report_path.read_text())
    if (report.get('status') != 'pass' or not report.get('assertions') or not report.get('frames')
            or not all(item.get('passed') for item in report['assertions'])
            or not all(Path(frame).is_file() for frame in report['frames'])):
        raise RuntimeError(f'failed or incomplete inspect report: {scenario}')
    return dict(scenario=scenario, artifact=str(artifact), assertions=len(report['assertions']))


def main():
    parser = argparse.ArgumentParser(description=__doc__, epilog='scenarios: ' + ', '.join(SCENARIOS))
    parser.add_argument('scenario', nargs='?', choices=SCENARIOS)
    parser.add_argument('--list', action='store_true')
    args = parser.parse_args()
    if args.list:
        print('\n'.join(SCENARIOS))
        return 0
    if not args.scenario:
        parser.error('missing scenario')
    test, scenario, _, ignored = SCENARIOS[args.scenario]
    started = time.time()
    result = subprocess.run(['cargo', 'test', '--locked', '--lib', test, '--', '--nocapture', *(['--ignored'] if ignored else [])], cwd=ROOT)
    try:
        if result.returncode:
            raise RuntimeError(f'Cargo exited {result.returncode}')
        artifact = check_artifact(scenario, started)
        print(f"status=pass\nscenario={scenario}\nartifact={artifact['artifact']}")
        return 0
    except (RuntimeError, OSError, ValueError, KeyError) as error:
        print(f'status=fail\nscenario={scenario}\nerror={error}')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
