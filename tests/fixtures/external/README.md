# External Image Fixtures

`manifest.json` lists small, pinned fixtures for real-world decoder coverage. The downloaded files live under `tests/fixtures/external/files/`, which is intentionally gitignored.

Fetch and verify them with:

```bash
scripts/fetch-image-fixtures.sh
```

Keep entries pinned to immutable URLs and include a SHA-256 hash, expected dimensions, license/source note, and intended decoder path. Do not commit downloaded fixture binaries unless they are tiny, clearly licensed, and needed for fast unit tests.
