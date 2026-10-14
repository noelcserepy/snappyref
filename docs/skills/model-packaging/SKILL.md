---
name: model-packaging
description: Build or publish SnappyRef's standalone Semantic Search model package and update its app descriptor. Use when changing or republishing model assets, not for installing the model in the app or cutting an ordinary app release.
---

# Package Semantic Search

Ordinary app releases reuse the existing model package. This procedure follows
[scripts/semantic_model.py](../../../scripts/semantic_model.py) and
[publish-semantic-search.yml](../../../.github/workflows/publish-semantic-search.yml).
The packaging redesign is tracked in [issue #295](https://github.com/noelcserepy/snappyref/issues/295).
Keep that redesign separate from a package publication.

## Build and inspect

1. Resolve the intended source ref to an exact commit. Read `MODEL_KEY`,
   `REPO_ID`, `REVISION` and `FILES` in `scripts/semantic_model.py`. Those are
   the pinned inputs. Change them only when the requested model change calls
   for it.
2. To build locally, put the output outside the checkout:

   ```sh
   package_dir=$(mktemp -d)
   python3 scripts/semantic_model.py bundle "$package_dir/snappyref-semantic-search.zip"
   python3 scripts/semantic_model.py check "$package_dir/snappyref-semantic-search.zip"
   unzip -l "$package_dir/snappyref-semantic-search.zip"
   ```

   `bundle` downloads the pinned Hugging Face revision into temporary storage.
   The archive contains `models/<model key>/`, the tokenizer, both int8 ONNX
   files, Apache-2.0 `LICENSE`, `NOTICE`, and `MODEL_BUNDLE.txt`. `check` verifies
   the model key and the three asset hashes recorded in that provenance file.
   Keep downloaded model files and ZIPs out of git.

## Publish and update the descriptor

1. Dispatch from this repo with the exact source commit as `ref`. Set
   `package_ref` to that commit before running:

   ```sh
   gh workflow run publish-semantic-search.yml --repo noelcserepy/snappyref \
     --ref main -f ref="$package_ref" -f release_tag=semantic-search
   gh run list --repo noelcserepy/snappyref --workflow publish-semantic-search.yml --limit 5
   gh run watch <run-id> --repo noelcserepy/snappyref --exit-status
   ```

   The workflow builds its own ZIP, uploads `snappyref-semantic-search.zip` to
   this repo's `semantic-search` prerelease, and prints the URL, SHA256 and
   byte size in its summary. The prerelease does not take the app updater's
   `releases/latest` alias.
2. An existing asset is refused by default. Use `replace_existing=true` only
   for an intentional replacement. Changing bytes at an existing URL makes
   older apps' embedded hash differ from the download. The workflow also
   supports a new `release_tag` for changed assets. Record which tag was used.
3. Download the published asset and verify those bytes. Set `package_tag` to
   the tag just published, normally `semantic-search`:

   ```sh
   published_dir=$(mktemp -d)
   gh release view "$package_tag" --repo noelcserepy/snappyref \
     --json isPrerelease,assets,url
   gh release download "$package_tag" --repo noelcserepy/snappyref \
     --pattern snappyref-semantic-search.zip --dir "$published_dir"
   python3 scripts/semantic_model.py check "$published_dir/snappyref-semantic-search.zip"
   shasum -a 256 "$published_dir/snappyref-semantic-search.zip"
   wc -c < "$published_dir/snappyref-semantic-search.zip"
   ```

   Confirm the release is a prerelease. Match the downloaded ZIP's SHA256 and
   byte size against the workflow summary. Use the published bytes, since a
   separately built ZIP can have different archive timestamps.
4. Open a PR updating [assets/semantic-search-descriptor.env](../../../assets/semantic-search-descriptor.env)
   with the published `semantic_search_url`, `semantic_search_sha256`, and
   `semantic_search_size`. The URL names this repo, the recorded tag and
   `snappyref-semantic-search.zip`. Cargo and the app-build workflows consume
   the descriptor. Follow the [repo check policy](../../../AGENTS.md#build-check-test) and
   run `python3 -m unittest discover -s scripts -p test_release_packaging.py`
   for packaging changes. Merge the descriptor PR before building apps that
   should use the replacement.
5. Remove local scratch packages once the checks and descriptor evidence are
   recorded. Report the workflow run, release URL/tag, published digest and
   size, descriptor PR, and any verification left open.

The package and descriptor contracts live in
[release operations](../../release-operations.md#publish-semantic-search-asset).
