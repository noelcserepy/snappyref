#!/usr/bin/env python3
"""Synthetic library sizes using the schema created by the running app.

JPEGs have identical pixels and distinct trailing bytes/hashes. Ready libraries
reuse the seed vector, so these measure cardinality, not search relevance.
"""
from __future__ import annotations

import ctypes
import hashlib
import json
import pathlib
import shutil
import sqlite3
import sys


def copy_file(source, destination):
    # APFS clones keep repeated 1000-asset fixtures cheap without shared writes.
    if sys.platform == "darwin":
        result = ctypes.CDLL(None, use_errno=True).clonefile(
            str(source).encode(), str(destination).encode(), 0
        )
        if result == 0:
            return destination
    return shutil.copy2(source, destination)


def seed_library(seed_library_path, destination_path, asset_count, placement_count,
                 *, ready=True):
    if asset_count < 1 or placement_count < 0:
        raise ValueError("need at least one asset and nonnegative placements")
    shutil.copytree(seed_library_path, destination_path, copy_function=copy_file)
    with sqlite3.connect(destination_path / "library.db") as db:
        db.row_factory = sqlite3.Row
        assets = db.execute("SELECT * FROM assets WHERE archived = 0").fetchall()
        if len(assets) != 1:
            raise ValueError("seed must contain exactly one asset")
        asset = dict(assets[0])
        embedding = db.execute("SELECT * FROM asset_embeddings WHERE status = 'ready'").fetchone()
        if embedding is None:
            raise ValueError("seed must have a ready embedding")
        source = pathlib.Path(asset["original_path"])
        source_bytes = source.read_bytes()
        if not source_bytes.startswith(b'\xff\xd8'):
            raise ValueError("scaling seed must be JPEG")
        db.execute("PRAGMA foreign_keys = ON")
        db.execute("DELETE FROM canvas_stack_items")
        db.execute("DELETE FROM board_items")
        db.execute("DELETE FROM board_history")
        db.execute("DELETE FROM asset_embeddings")
        db.execute("DELETE FROM assets")
        db.execute("UPDATE boards SET camera_pan_x=0, camera_pan_y=0, camera_zoom=1")
        for index in range(asset_count):
            item = asset.copy()
            item["id"] = index + 1
            path = destination_path / "originals" / f"perf-{index + 1:04}.jpg"
            copy_file(source, path)
            suffix = f"\nSnappyRef perf fixture {index + 1}\n".encode()
            with path.open("ab") as handle:
                handle.write(suffix)
            digest = hashlib.sha256(source_bytes)
            digest.update(suffix)
            item.update(original_path=str(path), filename=path.name,
                        original_sha256=digest.hexdigest(), file_size=path.stat().st_size)
            insert_row(db, "assets", item)
            if ready or index == 0:
                vector = dict(embedding)
                vector["asset_id"] = item["id"]
                insert_row(db, "asset_embeddings", vector)
        for index in range(placement_count):
            asset_id = index % asset_count + 1
            insert_row(db, "board_items", {
                "id": index + 1, "board_id": 1, "asset_id": asset_id,
                "x": float(index % 50 * 220), "y": float(index // 50 * 180),
                "scale": min(200 / asset["width"], 160 / asset["height"]),
                "width": asset["width"], "height": asset["height"],
                "rotation": 0, "z_index": index, "archived": 0,
                "placement_content_state": json.dumps({"Loaded": {"asset_id": asset_id}}),
                "placement_lifecycle": "committed", "content_resolution_intent": None,
            })
            insert_row(db, "canvas_stack_items", {
                "board_id": 1, "item_kind": "placement", "item_id": index + 1,
                "stack_index": index,
            })
    return background_state(destination_path)


def insert_row(db, table, row):
    columns = ', '.join(row)
    placeholders = ', '.join('?' for _ in row)
    db.execute(f"INSERT INTO {table} ({columns}) VALUES ({placeholders})", list(row.values()))


def background_state(library_path):
    with sqlite3.connect(library_path / "library.db") as db:
        assets = db.execute("SELECT count(*) FROM assets WHERE archived=0").fetchone()[0]
        # SVGs are deliberately excluded by library/storage/embeddings.rs.
        excluded = db.execute("SELECT count(*) FROM assets WHERE archived=0 AND lower(trim(mime_type))='image/svg+xml'").fetchone()[0]
        ready = db.execute("""SELECT count(*) FROM assets a WHERE a.archived=0 AND EXISTS
            (SELECT 1 FROM asset_embeddings e WHERE e.asset_id=a.id AND e.status='ready'
             AND e.embedding_blob IS NOT NULL)""").fetchone()[0]
        placements = db.execute("SELECT count(*) FROM board_items WHERE archived=0").fetchone()[0]
        failed = db.execute("SELECT count(*) FROM asset_embeddings WHERE status='failed'").fetchone()[0]
        return dict(assets=assets, placements=placements, ready_embeddings=ready, excluded_embeddings=excluded,
                    unfinished_embeddings=assets-excluded-ready, failed_embeddings=failed)
