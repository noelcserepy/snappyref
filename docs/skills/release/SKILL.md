---
name: release
description: Cut a SnappyRef app release, from the version bump PR and patchnotes through draft inspection, publication, signed update-channel verification and website refresh. Use when asked to release SnappyRef or prepare its release procedure.
---

# Release

Run from the repository root. Read [Release Operations](../../release-operations.md)
for asset, signing, secret and data-root contracts. Ordinary app releases reuse
the model package; use [model-packaging](../model-packaging/SKILL.md) only when
that package changes. The workflow publishes assets to this repository only.
The archived dist host remains online for older apps; no bridge upload is part
of a new release.

## Prepare the source

1. Establish the requested version and intended changes. In a fresh worktree,
   change `[package].version` in `Cargo.toml` and the `snappyref` package entry
   in `Cargo.lock`. Do not update unrelated dependencies. Open the version bump
   PR, run the [repository checks](../../../README.md#development-checks), and
   follow its review and merge process. Do not change a published release's zips.
2. Walk the [functional test plan](../../release-functional-test-plan.md) on
   macOS and Windows. Report remaining manual gaps. Update smokes remain
   [explicit-request-only](../../release-operations.md#update-smokes).
3. Resolve the intended merged source to its exact commit and record it as
   `release_sha`. Set `release_tag` to `v` plus that commit's package version.
   `python3 scripts/resolve-release-version.py --tag "$release_tag"` checks the version
   when run from that source checkout. Check existing tags and releases before
   dispatching; a published tag requires a new version. If an unpublished tag
   already exists, resolve it and require its commit to equal `release_sha`;
   stop on a mismatch before building or publishing.
4. Use [patchnotes](../patchnotes/SKILL.md) for the complete previous published
   app tag-to-`release_sha` range. Save the resulting notes to a scratch file
   for `gh release edit --notes-file` below. Prepare a `notes_url`, normally the
   new release's GitHub page. No model installation is needed.

## Build and inspect the draft

The workflow ref selects the workflow definition; its `ref` input selects the
source to build. Use `main` for the workflow definition and pin its source
input to the reviewed commit. Inspect the workflow on current main first;
`--ref` accepts a branch or tag, not a source SHA. Keep both platforms selected
when signing an update.

```sh
gh workflow run publish-full-release.yml --ref main \
  -f ref="$release_sha" -f release_tag="$release_tag" \
  -f notes_url="$notes_url" -f include_macos=true -f include_windows=true \
  -f publish_update=true
```

An optional dry run uses the same command with `publish_update=false`: it
attaches zips to a draft without `latest.json`. It still needs a new version.
After inspecting it, dispatch the signed run for that same source and tag.
A dry run cannot replace zips on a draft already holding a signed manifest.
A signed run can replace a draft's zips and manifest together; never rerun it
against a published release.

Find the dispatched run by workflow and dispatch time, confirm its source
input and the resolve job's exact SHA, and record its URL. The run's workflow
head can differ from the source input. Wait for every required job to succeed. Read packaging and signing logs, not
just the run headline. Then:

```sh
release_dir=$(mktemp -d)
gh release view "$release_tag" --json isDraft,isPrerelease,targetCommitish,assets,url
gh release download "$release_tag" --dir "$release_dir"
gh release edit "$release_tag" --title "SnappyRef $release_tag" --notes-file "$notes_file"
```

The final signed draft must be a non-prerelease and contain exactly the two platform
zips (`snappyref-mac-$release_tag.zip`, `snappyref-windows-$release_tag.zip`)
and `latest.json`; temporary app/descriptor handoff assets should be removed.
Confirm both archive validations passed and the files match the
[asset contract](../../release-operations.md#release-model), with no user data.
Verify manifest version, URLs, hashes, sizes and signature with the block below
before publishing. Confirm the workflow's resolved SHA is `release_sha`;
`targetCommitish` alone is not proof of what was built.

## Publish, then verify the public channel

Publish only the inspected app draft here, after the signed run and notes are
complete. The separate `semantic-search` prerelease stays a prerelease.

```sh
gh release edit "$release_tag" --draft=false --prerelease=false
git fetch origin tag "$release_tag"
git rev-parse "$release_tag^{commit}"
```

The resolved tag must equal `release_sha`. Fetch the actual latest alias after
publication into a new scratch directory, rather than trusting the draft copy:

```sh
release_dir=$(mktemp -d)
gh release download "$release_tag" --pattern 'snappyref-*.zip' --dir "$release_dir"
curl --fail --location --silent --show-error \
  https://github.com/noelcserepy/snappyref/releases/latest/download/latest.json \
  --output "$release_dir/latest.json"
```

This second verification uses fresh downloads of the published bytes. An
older alias, absent asset, wrong signature or hash is a failed release verification.
Do not overwrite published assets to repair it; investigate and ship a new
version if needed. Run this block from the release source checkout with
`release_dir` and `release_tag` set. It needs Python 3 and OpenSSL with Ed25519
`pkeyutl -rawin` support.

```sh
python3 - "$release_dir" "$release_tag" <<'PY'
import hashlib, json, subprocess, sys, tempfile
from pathlib import Path
root, tag = Path(sys.argv[1]), sys.argv[2]
manifest = json.loads((root / 'latest.json').read_text())
signature = bytes.fromhex(manifest.pop('signature'))
assert len(signature) == 64
assert manifest['channel'] == 'stable'
assert manifest['version'] == tag.removeprefix('v')
assert set(manifest['platforms']) == {'macos', 'windows'}
for platform, stem in [('macos', 'mac'), ('windows', 'windows')]:
    name = f'snappyref-{stem}-{tag}.zip'
    entry = manifest['platforms'][platform]
    assert entry['url'] == f'https://github.com/noelcserepy/snappyref/releases/download/{tag}/{name}'
    data = (root / name).read_bytes()
    assert entry['size'] == len(data), name
    assert entry['sha256'] == hashlib.sha256(data).hexdigest(), name
with tempfile.TemporaryDirectory() as scratch:
    canonical, sig = Path(scratch) / 'canonical', Path(scratch) / 'signature'
    canonical.write_bytes(json.dumps(manifest, sort_keys=True, separators=(',', ':')).encode('utf-8'))
    sig.write_bytes(signature)
    subprocess.run(['openssl', 'pkeyutl', '-verify', '-pubin', '-inkey',
                    'docs/keys/signing-key.pub.pem', '-rawin', '-in', str(canonical),
                    '-sigfile', str(sig)], check=True)
print(f'{tag}: signature and both published zip hashes/sizes verified')
PY
```

## Refresh the website

The workflow's configured webhook runs while the release is still a draft.
After publication, trigger the same refresh again with `{"tag":"<release-tag>"}`,
using the private `SNAPPYREF_WEB_RELEASE_REFRESH_URL` and
`SNAPPYREF_WEB_RELEASE_REFRESH_SECRET` values described in
[Release Operations](../../release-operations.md#signing-and-secrets). Keep the
secret out of commands shown in chat, logs and tracked files. Send a POST with
JSON content type and the `x-snappyref-release-refresh-secret` header. Check the
webhook response and the website's version and both download destinations.
If the credentials or website access are unavailable, report refresh pending;
a workflow notice that secrets were absent is not evidence of a refreshed site.
Do not rerun the publishing workflow just to refresh a published release.

Report the release URL, source/tag SHA, workflow URL, channel verification,
manual QA gaps and website refresh result. No update smoke runs unless the
user explicitly requested them.
