#!/usr/bin/env python3
"""Seed a perf library to a given size (assets and placements).

`scripts/run-existing-library-baseline.sh` first runs the regular
`import-1` perf scenario to produce a seed library with one **Asset** and
one **Placement**. That seed library is then cloned per case, and this
module multiplies its asset rows and active-board placements to the
target sizes.

Library-size scaling and board-size scaling are exposed as independent
knobs so a single run can distinguish library/search/backfill cost
(`assets=N, placements=0`) from active-board cost (`placements=N`).

The seeding logic is split out from the bash launcher so it can be
self-tested without touching the real app binary. Run

    scripts/_perf_library_seed.py --self-test

to execute the bundled unit tests.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import sqlite3
import sys
import tempfile
import unittest


def seed_library(
    seed_library_path: pathlib.Path,
    destination_path: pathlib.Path,
    asset_count: int,
    placement_count: int,
) -> dict:
    """Clone the seed library and multiply assets and placements.

    The seed library must already contain exactly one **Asset** and one
    **Placement** on board 1. After seeding:

    - ``assets`` table has ``asset_count`` rows. Row 1 keeps the original
      ``original_path``; additional rows reuse the same managed original
      bytes via hardlinks (or copies if hardlinks are not available) at
      unique paths so the ``UNIQUE`` constraint on ``original_path`` is
      respected.
    - ``board_items`` and ``canvas_stack_items`` on board 1 are cleared
      and then re-populated with ``placement_count`` placements. The
      first ``min(placement_count, asset_count)`` placements reference
      asset ids ``1..=placement_count``; if ``placement_count`` exceeds
      ``asset_count`` the extras cycle through existing asset ids.
    - ``asset_embeddings`` rows are left as-is from the seed (typically
      one ready row for asset 1). This keeps **Embedding Backfill** in a
      realistic state: most assets in the populated library start
      without embeddings, so backfill has work to do.

    Returns a small JSON-serializable summary describing what was done.
    """

    if asset_count < 1:
        raise ValueError("asset_count must be >= 1")
    if placement_count < 0:
        raise ValueError("placement_count must be >= 0")
    if not seed_library_path.is_dir():
        raise FileNotFoundError(seed_library_path)

    if destination_path.exists():
        shutil.rmtree(destination_path)
    shutil.copytree(seed_library_path, destination_path)

    db_path = destination_path / "library.db"
    originals_dir = destination_path / "originals"
    if not db_path.is_file():
        raise FileNotFoundError(db_path)
    if not originals_dir.is_dir():
        raise FileNotFoundError(originals_dir)

    seed_assets = _select_seed_assets(db_path)
    if len(seed_assets) != 1:
        raise RuntimeError(
            f"expected exactly one seed asset, found {len(seed_assets)}",
        )
    seed_asset = seed_assets[0]
    seed_original = pathlib.Path(seed_asset["original_path"])
    if not seed_original.is_file():
        raise FileNotFoundError(seed_original)

    asset_ids = [seed_asset["id"]]
    asset_files_created = []
    with sqlite3.connect(db_path) as connection:
        connection.execute("PRAGMA foreign_keys = ON")
        for index in range(1, asset_count):
            duplicate_path = _next_duplicate_path(seed_original, originals_dir, index)
            _materialize_copy(seed_original, duplicate_path)
            asset_files_created.append(duplicate_path)
            cursor = connection.execute(
                """
                INSERT INTO assets (
                    original_path, filename, mime_type, width, height,
                    file_size, imported_at, archived
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
                """,
                (
                    str(duplicate_path),
                    duplicate_path.name,
                    seed_asset["mime_type"],
                    seed_asset["width"],
                    seed_asset["height"],
                    seed_asset["file_size"],
                    seed_asset["imported_at"],
                ),
            )
            asset_ids.append(cursor.lastrowid)

        connection.execute("DELETE FROM canvas_stack_items")
        connection.execute("DELETE FROM board_items")
        for placement_index in range(placement_count):
            asset_id = asset_ids[placement_index % len(asset_ids)]
            x = float((placement_index % 50) * 220)
            y = float((placement_index // 50) * 180)
            scale = 0.0859152153134346
            connection.execute(
                """
                INSERT INTO board_items (
                    id, board_id, asset_id, x, y, scale, rotation,
                    z_index, archived
                ) VALUES (?1, 1, ?2, ?3, ?4, ?5, 0.0, ?6, 0)
                """,
                (
                    placement_index + 1,
                    asset_id,
                    x,
                    y,
                    scale,
                    placement_index,
                ),
            )
            connection.execute(
                """
                INSERT INTO canvas_stack_items (
                    board_id, item_kind, item_id, stack_index
                ) VALUES (1, 'placement', ?1, ?2)
                """,
                (placement_index + 1, placement_index),
            )
        connection.commit()

    return {
        "library": str(destination_path),
        "asset_count": asset_count,
        "placement_count": placement_count,
        "created_asset_files": [str(path) for path in asset_files_created],
    }


def background_state(library_path: pathlib.Path) -> dict:
    """Snapshot of background work signals for a populated library.

    Reports how much **Embedding Backfill** work is still queued in the
    database: assets without embeddings, rows in retryable failure
    states, and the total assets count. This lets a perf run record
    whether background work remains queued at the end of a measurement
    window without grepping log output.
    """

    db_path = library_path / "library.db"
    with sqlite3.connect(db_path) as connection:
        assets_row = connection.execute(
            "SELECT COUNT(*) FROM assets WHERE archived = 0"
        ).fetchone()
        ready_row = connection.execute(
            "SELECT COUNT(*) FROM asset_embeddings WHERE status = 'ready'"
        ).fetchone()
        pending_row = connection.execute(
            "SELECT COUNT(*) FROM asset_embeddings WHERE status = 'pending'"
        ).fetchone()
        failed_row = connection.execute(
            "SELECT COUNT(*) FROM asset_embeddings WHERE status = 'failed'"
        ).fetchone()
        total_embedded_row = connection.execute(
            "SELECT COUNT(*) FROM asset_embeddings"
        ).fetchone()
    total_assets = int(assets_row[0]) if assets_row else 0
    embedded = int(total_embedded_row[0]) if total_embedded_row else 0
    return {
        "assets_total": total_assets,
        "asset_embeddings_ready": int(ready_row[0]) if ready_row else 0,
        "asset_embeddings_pending": int(pending_row[0]) if pending_row else 0,
        "asset_embeddings_failed": int(failed_row[0]) if failed_row else 0,
        "assets_missing_embeddings": max(total_assets - embedded, 0),
    }


def _select_seed_assets(db_path: pathlib.Path) -> list[dict]:
    with sqlite3.connect(db_path) as connection:
        connection.row_factory = sqlite3.Row
        rows = connection.execute(
            """
            SELECT id, original_path, filename, mime_type, width, height,
                   file_size, imported_at
            FROM assets
            WHERE archived = 0
            ORDER BY id
            """
        ).fetchall()
    return [dict(row) for row in rows]


def _next_duplicate_path(
    seed_original: pathlib.Path,
    originals_dir: pathlib.Path,
    index: int,
) -> pathlib.Path:
    stem = seed_original.stem
    suffix = seed_original.suffix or ""
    return originals_dir / f"{stem}-perf-clone-{index:06d}{suffix}"


def _materialize_copy(source: pathlib.Path, destination: pathlib.Path) -> None:
    """Hardlink when possible, fall back to a real copy.

    Hardlinks keep the seeding step cheap for the 1000-asset case while
    still letting SQLite see distinct ``original_path`` values.
    """
    if destination.exists():
        destination.unlink()
    try:
        os.link(source, destination)
    except OSError:
        shutil.copyfile(source, destination)


# ------------------------------------------------------------------
# self-test
# ------------------------------------------------------------------


class _Tests(unittest.TestCase):
    def _make_seed(self, tmp: pathlib.Path) -> pathlib.Path:
        """Build a minimal seed library that matches the real schema."""
        seed = tmp / "seed"
        originals = seed / "originals"
        originals.mkdir(parents=True)
        (seed / "thumbs").mkdir()
        seed_file = originals / "seed.jpg"
        seed_file.write_bytes(b"fake-image-bytes")
        db = seed / "library.db"
        with sqlite3.connect(db) as connection:
            connection.executescript(
                """
                CREATE TABLE assets (
                    id INTEGER PRIMARY KEY,
                    original_path TEXT NOT NULL UNIQUE,
                    filename TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    width INTEGER NOT NULL,
                    height INTEGER NOT NULL,
                    file_size INTEGER NOT NULL,
                    imported_at INTEGER NOT NULL,
                    archived INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE boards (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    archived INTEGER NOT NULL DEFAULT 0,
                    camera_pan_x REAL NOT NULL DEFAULT 0.0,
                    camera_pan_y REAL NOT NULL DEFAULT 0.0,
                    camera_zoom REAL NOT NULL DEFAULT 1.0,
                    canvas_background TEXT NOT NULL DEFAULT 'none'
                );
                CREATE TABLE board_items (
                    id INTEGER PRIMARY KEY,
                    board_id INTEGER NOT NULL DEFAULT 1 REFERENCES boards(id),
                    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
                    x REAL NOT NULL,
                    y REAL NOT NULL,
                    scale REAL NOT NULL,
                    rotation REAL NOT NULL,
                    z_index INTEGER NOT NULL,
                    archived INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE canvas_stack_items (
                    board_id INTEGER NOT NULL DEFAULT 1 REFERENCES boards(id),
                    item_kind TEXT NOT NULL,
                    item_id INTEGER NOT NULL,
                    stack_index INTEGER NOT NULL,
                    PRIMARY KEY (board_id, item_kind, item_id)
                );
                CREATE TABLE asset_embeddings (
                    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
                    model_key TEXT NOT NULL,
                    dimension INTEGER NOT NULL,
                    status TEXT NOT NULL,
                    last_error TEXT,
                    failure_kind TEXT,
                    failure_count INTEGER NOT NULL DEFAULT 0,
                    next_retry_at INTEGER,
                    embedding_blob BLOB,
                    updated_at INTEGER NOT NULL,
                    PRIMARY KEY (asset_id, model_key)
                );
                INSERT INTO boards (id, name) VALUES (1, 'My Board');
                INSERT INTO board_items (
                    id, board_id, asset_id, x, y, scale, rotation, z_index
                ) VALUES (1, 1, 1, 0.0, 0.0, 1.0, 0.0, 0);
                INSERT INTO canvas_stack_items (
                    board_id, item_kind, item_id, stack_index
                ) VALUES (1, 'placement', 1, 0);
                INSERT INTO asset_embeddings (
                    asset_id, model_key, dimension, status, failure_count,
                    updated_at
                ) VALUES (1, 'mobileclip-s2', 384, 'ready', 0, 1000);
                """,
            )
            # executescript cannot bind params, so insert the asset row
            # with a real on-disk path here.
            connection.execute(
                """
                INSERT INTO assets (
                    id, original_path, filename, mime_type, width, height,
                    file_size, imported_at
                ) VALUES (1, ?1, 'seed.jpg', 'image/jpeg', 100, 200, 16, 1000)
                """,
                (str(seed_file),),
            )
            connection.commit()
        return seed

    def test_seed_creates_extra_assets_with_unique_paths(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = pathlib.Path(raw_tmp)
            seed = self._make_seed(tmp)
            destination = tmp / "case"
            result = seed_library(seed, destination, asset_count=5, placement_count=3)
            self.assertEqual(result["asset_count"], 5)
            self.assertEqual(result["placement_count"], 3)
            with sqlite3.connect(destination / "library.db") as connection:
                rows = connection.execute(
                    "SELECT id, original_path FROM assets ORDER BY id"
                ).fetchall()
            self.assertEqual([row[0] for row in rows], [1, 2, 3, 4, 5])
            paths = [row[1] for row in rows]
            self.assertEqual(len(set(paths)), 5, "asset paths must be unique")
            for path in paths:
                self.assertTrue(pathlib.Path(path).is_file(), path)

    def test_seed_clears_and_repopulates_placements(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = pathlib.Path(raw_tmp)
            seed = self._make_seed(tmp)
            destination = tmp / "case"
            seed_library(seed, destination, asset_count=2, placement_count=4)
            with sqlite3.connect(destination / "library.db") as connection:
                items = connection.execute(
                    "SELECT id, asset_id, z_index FROM board_items ORDER BY id"
                ).fetchall()
                stack = connection.execute(
                    "SELECT item_kind, item_id, stack_index "
                    "FROM canvas_stack_items ORDER BY stack_index"
                ).fetchall()
            self.assertEqual(len(items), 4)
            # Placements cycle through asset ids when placements > assets.
            self.assertEqual([row[1] for row in items], [1, 2, 1, 2])
            self.assertEqual([row[2] for row in items], [0, 1, 2, 3])
            self.assertEqual(len(stack), 4)
            self.assertEqual([row[0] for row in stack], ["placement"] * 4)
            self.assertEqual([row[1] for row in stack], [1, 2, 3, 4])

    def test_seed_allows_zero_placements(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = pathlib.Path(raw_tmp)
            seed = self._make_seed(tmp)
            destination = tmp / "case"
            seed_library(seed, destination, asset_count=3, placement_count=0)
            with sqlite3.connect(destination / "library.db") as connection:
                count = connection.execute(
                    "SELECT COUNT(*) FROM board_items"
                ).fetchone()[0]
                stack_count = connection.execute(
                    "SELECT COUNT(*) FROM canvas_stack_items"
                ).fetchone()[0]
                assets = connection.execute(
                    "SELECT COUNT(*) FROM assets"
                ).fetchone()[0]
            self.assertEqual(count, 0)
            self.assertEqual(stack_count, 0)
            self.assertEqual(assets, 3)

    def test_seed_rejects_bad_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = pathlib.Path(raw_tmp)
            seed = self._make_seed(tmp)
            destination = tmp / "case"
            with self.assertRaises(ValueError):
                seed_library(seed, destination, asset_count=0, placement_count=0)
            with self.assertRaises(ValueError):
                seed_library(seed, destination, asset_count=1, placement_count=-1)
            with self.assertRaises(FileNotFoundError):
                seed_library(
                    tmp / "no-such-seed",
                    destination,
                    asset_count=1,
                    placement_count=0,
                )

    def test_background_state_reports_pending_backfill(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = pathlib.Path(raw_tmp)
            seed = self._make_seed(tmp)
            destination = tmp / "case"
            seed_library(seed, destination, asset_count=10, placement_count=0)
            state = background_state(destination)
            self.assertEqual(state["assets_total"], 10)
            # Seed left exactly one ready embedding; the nine new assets
            # have no asset_embeddings row yet, so backfill has 9 missing.
            self.assertEqual(state["asset_embeddings_ready"], 1)
            self.assertEqual(state["assets_missing_embeddings"], 9)


def _self_test() -> int:
    loader = unittest.TestLoader()
    suite = loader.loadTestsFromTestCase(_Tests)
    runner = unittest.TextTestRunner(verbosity=2)
    result = runner.run(suite)
    return 0 if result.wasSuccessful() else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--seed", help="seed library path", default=None)
    parser.add_argument("--destination", help="destination library path", default=None)
    parser.add_argument("--assets", type=int, default=None)
    parser.add_argument("--placements", type=int, default=None)
    parser.add_argument(
        "--background-state",
        help="library path to inspect for queued background work",
        default=None,
    )
    args = parser.parse_args(argv)

    if args.self_test:
        return _self_test()
    if args.background_state:
        state = background_state(pathlib.Path(args.background_state))
        sys.stdout.write(json.dumps(state, indent=2) + "\n")
        return 0
    if not (args.seed and args.destination and args.assets is not None):
        parser.error("--seed, --destination, and --assets are required")
    summary = seed_library(
        pathlib.Path(args.seed),
        pathlib.Path(args.destination),
        asset_count=args.assets,
        placement_count=args.placements or 0,
    )
    sys.stdout.write(json.dumps(summary, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
