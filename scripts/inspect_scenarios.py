#!/usr/bin/env python3
"""Inspect scenario catalogue and required artifact evidence."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
# name: test filter, artifact scenario, network/ignored
SCENARIOS = {
    'rotated-corner-resize': ('inspect_rotated_corner_resize_outside_canvas_writes_artifact', 'rotated-corner-resize', False),
    'snapped-drag-commit': ('inspect_snapped_drag_release_and_command_bypass_writes_artifact', 'snapped-drag-commit', False),
    'marquee-clipped-group': ('inspect_marquee_clipped_group_and_shift_add_writes_artifact', 'marquee-clipped-group', False),
    'group-dock-narrow': ('inspect_group_dock_narrow_writes_artifact', 'group-dock-narrow', False),
    'group-dock': ('inspect_group_dock_writes_artifact', 'group-dock', False),
    'context-menu': ('inspect_context_menu_writes_artifact', 'context-menu', False),
    'member-dock': ('inspect_member_dock_writes_artifact', 'member-dock', False),
    'annotation-editing': ('inspect_annotation_creation_editing_and_styles_writes_artifact', 'annotation-editing', False),
    'annotation-endpoint-drag': ('inspect_annotation_endpoint_drag_writes_artifact', 'annotation-endpoint-drag', False),
    'annotation-board': ('inspect_annotation_board_writes_artifact', 'annotation-board', False),
    'annotation-style-keys': ('inspect_annotation_style_keys_writes_artifact', 'annotation-style-keys', False),
    'readme-board': ('inspect_readme_board_writes_artifact', 'readme-board', False),
    'pureref-import': ('inspect_pureref_import_writes_artifact', 'pureref-import', False),
    'pureref-export': ('inspect_pureref_export_writes_artifact', 'pureref-export', False),
    'image-format-gallery': ('inspect_image_format_gallery_writes_artifact', 'image-format-gallery', True),
    'group-picker-rename-create': ('inspect_group_picker_rename_and_create_writes_artifact', 'group-picker-rename-create', False),
    'check-harness': ('inspect_search_result_drag_to_canvas_writes_artifact', 'search-drag-to-canvas', False),
    'group-operations': ('inspect_group_operations_writes_artifact', 'group-operations', False),
    'search-drag-to-canvas': ('inspect_search_result_drag_to_canvas_writes_artifact', 'search-drag-to-canvas', False),
    'semantic-search-install': ('inspect_semantic_search_install_accepts_prompt', 'semantic-search-install', False),
    'semantic-search-sidebar-status': ('inspect_semantic_search_sidebar_status_writes_artifact', 'semantic-search-sidebar-status', False),
    'ai-control': ('inspect_ai_control_writes_artifact', 'ai-control', False),
    'sidebar-pages': ('inspect_sidebar_pages_writes_artifact', 'sidebar-pages', False),
    'semantic-search-real-install': ('inspect_semantic_search_real_install_writes_artifact', 'semantic-search-real-install', True),
    'sample-image-import': ('inspect_sample_image_import_writes_artifact', 'sample-image-import', False),
    'pending-drop-batch-undo': ('inspect_pending_drop_batch_undo_writes_artifact', 'pending-drop-batch-undo', False),
    'blue-image-search-drag': ('inspect_blue_image_search_drag_writes_artifact', 'blue-image-search-drag', False),
    'delete-board-modal-flash': ('inspect_delete_board_modal_flash_writes_artifact', 'delete-board-modal-flash', False),
    'board-lifecycle-undo-redo': ('inspect_board_lifecycle_undo_redo_writes_artifact', 'board-lifecycle-undo-redo', False),
    'search-composer': ('inspect_search_composer_writes_artifact', 'search-composer', False),
    'placement-click-hold-flash': ('inspect_placement_click_hold_flash_writes_artifact', 'placement-click-hold-flash', False),
    'delete-undo-redo-placement': ('inspect_delete_undo_redo_placement_writes_artifact', 'delete-undo-redo-placement', False),
    'crop-undo-redo-placement': ('inspect_crop_undo_redo_placement_writes_artifact', 'crop-undo-redo-placement', False),
    'compact-equal-size-images': ('inspect_compact_equal_size_images_writes_artifact', 'compact-equal-size-images', False),
    'compact-mixed-column-images': ('inspect_compact_mixed_column_images_writes_artifact', 'compact-mixed-column-images', False),
    'compact-windows-column-images': ('inspect_compact_windows_column_images_writes_artifact', 'compact-windows-column-images', False),
    'canvas-edit-operations': ('inspect_canvas_edit_operations_writes_artifact', 'canvas-edit-operations', False),
    'canvas-navigation-boards': ('inspect_canvas_navigation_and_boards_writes_artifact', 'canvas-navigation-boards', False),
    'session-restart-restore': ('inspect_session_restart_restore_writes_artifact', 'session-restart-restore', False),
    'persistence-write-retry': ('inspect_persistence_write_retry_writes_artifact', 'persistence-write-retry', False),
    'slow-url-inactive-mixed-batch': ('inspect_slow_url_and_inactive_mixed_batch_writes_artifact', 'slow-url-inactive-mixed-batch', False),
    'multi-transform-layout-contracts': ('inspect_multi_transform_and_layout_contracts_writes_artifact', 'multi-transform-layout-contracts', False),
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
    test, scenario, ignored = SCENARIOS[args.scenario]
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
        latest = inspect_root() / scenario / 'latest.txt'
        if latest.is_file() and latest.stat().st_mtime >= started:
            print(f'artifact={latest.read_text().strip()}')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
