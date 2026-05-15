#!/usr/bin/env bash
# Build and sign the auto-update `latest.json` manifest.
#
# Computes sha256 + size of each platform's app-only zip, emits the manifest
# JSON, signs the canonical (sorted-keys, no-whitespace, signature-excluded)
# bytes with the ed25519 private key, and verifies the signature against the
# matching public key before returning. Any verification failure aborts so the
# publish workflow never uploads a manifest the embedded public key cannot
# verify.
#
# See ADR-0015 (Automatic app updates) and docs/release-operations.md.
set -euo pipefail

usage() {
  cat >&2 <<EOF
usage: $0 \\
  --version <version> \\
  --released-at <iso8601> \\
  --notes-url <url> \\
  --macos-zip <path> --macos-url <url> \\
  --windows-zip <path> --windows-url <url> \\
  --private-key <path> --public-key <path> \\
  --output <latest.json>

All flags are required. Both --private-key and --public-key must be PEM-encoded
ed25519 keys; the script verifies the freshly-produced signature against the
public key before exiting.
EOF
}

version=""
released_at=""
notes_url=""
macos_zip=""
macos_url=""
windows_zip=""
windows_url=""
private_key=""
public_key=""
output=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) version=${2:-}; shift 2 ;;
    --released-at) released_at=${2:-}; shift 2 ;;
    --notes-url) notes_url=${2:-}; shift 2 ;;
    --macos-zip) macos_zip=${2:-}; shift 2 ;;
    --macos-url) macos_url=${2:-}; shift 2 ;;
    --windows-zip) windows_zip=${2:-}; shift 2 ;;
    --windows-url) windows_url=${2:-}; shift 2 ;;
    --private-key) private_key=${2:-}; shift 2 ;;
    --public-key) public_key=${2:-}; shift 2 ;;
    --output) output=${2:-}; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage; exit 2 ;;
  esac
done

for required in version released_at notes_url macos_zip macos_url windows_zip windows_url private_key public_key output; do
  if [[ -z "${!required}" ]]; then
    echo "missing required flag: --${required//_/-}" >&2
    usage
    exit 2
  fi
done

for f in "$macos_zip" "$windows_zip" "$private_key" "$public_key"; do
  if [[ ! -f "$f" ]]; then
    echo "missing file: $f" >&2
    exit 1
  fi
done

work_dir=$(mktemp -d "${TMPDIR:-/tmp}/build-signed-manifest.XXXXXX")
trap 'rm -rf "$work_dir"' EXIT

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

byte_size() {
  # macOS `stat -f%z`, GNU `stat -c%s`, both `wc -c` work; `wc -c` is most
  # portable.
  wc -c < "$1" | tr -d ' \n'
}

macos_sha=$(sha256_file "$macos_zip")
windows_sha=$(sha256_file "$windows_zip")
macos_size=$(byte_size "$macos_zip")
windows_size=$(byte_size "$windows_zip")

# Build the unsigned manifest body and emit the canonical bytes the signature
# must cover. Python is used because canonical JSON serialization (sorted
# keys, no whitespace) is part of the manifest contract and `jq -S -c` does
# not guarantee identical output across versions for nested structures.
canonical_bin="$work_dir/manifest.canonical.bin"
unsigned_json="$work_dir/manifest.unsigned.json"

VERSION="$version" RELEASED_AT="$released_at" NOTES_URL="$notes_url" \
  MACOS_URL="$macos_url" MACOS_SHA="$macos_sha" MACOS_SIZE="$macos_size" \
  WINDOWS_URL="$windows_url" WINDOWS_SHA="$windows_sha" WINDOWS_SIZE="$windows_size" \
  CANONICAL_OUT="$canonical_bin" UNSIGNED_OUT="$unsigned_json" \
  python3 - <<'PY'
import json, os
m = {
    "channel": "stable",
    "version": os.environ["VERSION"],
    "released_at": os.environ["RELEASED_AT"],
    "notes_url": os.environ["NOTES_URL"],
    "platforms": {
        "macos": {
            "url": os.environ["MACOS_URL"],
            "sha256": os.environ["MACOS_SHA"],
            "size": int(os.environ["MACOS_SIZE"]),
        },
        "windows": {
            "url": os.environ["WINDOWS_URL"],
            "sha256": os.environ["WINDOWS_SHA"],
            "size": int(os.environ["WINDOWS_SIZE"]),
        },
    },
}
canonical = json.dumps(m, sort_keys=True, separators=(",", ":")).encode("utf-8")
open(os.environ["CANONICAL_OUT"], "wb").write(canonical)
# Save the unsigned body too, for completeness/debugging.
open(os.environ["UNSIGNED_OUT"], "w").write(canonical.decode("utf-8"))
PY

# Sign the canonical bytes with the ed25519 private key. ed25519 in OpenSSL
# uses Ed25519ph-incompatible PureEdDSA semantics, which is what we want:
# `-rawin` signs the file contents directly without an extra hash step.
signature_bin="$work_dir/manifest.sig.bin"
openssl pkeyutl -sign \
  -inkey "$private_key" \
  -rawin -in "$canonical_bin" \
  -out "$signature_bin"

# Verify the freshly-produced signature against the supplied public key
# before we hand it off. A signing key that does not match the embedded
# public key would brick clients, so abort the workflow here.
if ! openssl pkeyutl -verify \
    -pubin -inkey "$public_key" \
    -rawin -in "$canonical_bin" \
    -sigfile "$signature_bin" >/dev/null; then
  echo "signature verification against --public-key failed" >&2
  exit 1
fi

signature_hex=$(xxd -p -c 256 "$signature_bin" | tr -d '\n')

# Splice the signature into the manifest and write it out in canonical form
# (sorted keys, no whitespace) so the published bytes match what consumers
# verify against.
CANONICAL_IN="$canonical_bin" SIGNATURE_HEX="$signature_hex" OUTPUT="$output" \
  python3 - <<'PY'
import json, os
m = json.loads(open(os.environ["CANONICAL_IN"], "rb").read().decode("utf-8"))
m["signature"] = os.environ["SIGNATURE_HEX"]
open(os.environ["OUTPUT"], "w").write(
    json.dumps(m, sort_keys=True, separators=(",", ":"))
)
PY

echo "wrote signed manifest: $output"
echo "  macos:   sha256=$macos_sha size=$macos_size"
echo "  windows: sha256=$windows_sha size=$windows_size"
echo "  signature: ed25519 (verified against $public_key)"
