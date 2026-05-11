#!/usr/bin/env bash
# Tests for scripts/build-signed-manifest.sh.
#
# Uses a throwaway ed25519 keypair to exercise the full sign + verify
# round-trip without touching real secrets.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/build-signed-manifest.sh"

if [[ ! -x "$SCRIPT" ]]; then
  echo "FAIL: $SCRIPT is not executable" >&2
  exit 1
fi

PASS=0
FAIL=0

run_case() {
  local name=$1
  echo "--- $name"
  if "$name"; then
    echo "ok"
    PASS=$((PASS + 1))
  else
    echo "FAIL"
    FAIL=$((FAIL + 1))
  fi
}

ROOT_TMP="$(mktemp -d "${TMPDIR:-/tmp}/build-manifest-test.XXXXXX")"
trap 'rm -rf "$ROOT_TMP"' EXIT

new_workspace() {
  TMP_DIR="$(mktemp -d "$ROOT_TMP/case.XXXXXX")"
  openssl genpkey -algorithm ed25519 -out "$TMP_DIR/priv.pem" 2>/dev/null
  openssl pkey -in "$TMP_DIR/priv.pem" -pubout -out "$TMP_DIR/pub.pem" 2>/dev/null
  printf 'fake-macos-app-zip-payload' > "$TMP_DIR/snappyref-mac.zip"
  printf 'fake-windows-app-zip-payload-which-is-longer' > "$TMP_DIR/snappyref-windows.zip"
}

invoke_script() {
  local pub_key=${1:-"$TMP_DIR/pub.pem"}
  "$SCRIPT" \
    --version "0.2.0" \
    --released-at "2026-05-12T10:00:00Z" \
    --notes-url "https://example.com/notes" \
    --macos-zip "$TMP_DIR/snappyref-mac.zip" \
    --macos-url "https://example.com/snappyref-mac.zip" \
    --windows-zip "$TMP_DIR/snappyref-windows.zip" \
    --windows-url "https://example.com/snappyref-windows.zip" \
    --private-key "$TMP_DIR/priv.pem" \
    --public-key "$pub_key" \
    --output "$TMP_DIR/latest.json"
}

test_produces_required_keys() {
  new_workspace
  invoke_script >/dev/null
  python3 - <<PY
import json
m = json.load(open("$TMP_DIR/latest.json"))
assert m["channel"] == "stable", m
assert m["version"] == "0.2.0", m
assert m["released_at"] == "2026-05-12T10:00:00Z", m
assert m["notes_url"] == "https://example.com/notes", m
assert set(m["platforms"]) == {"macos", "windows"}, m
for p in ("macos", "windows"):
    plat = m["platforms"][p]
    assert set(plat) == {"url", "sha256", "size"}, plat
    assert plat["url"].startswith("https://"), plat
    assert len(plat["sha256"]) == 64, plat
    assert isinstance(plat["size"], int) and plat["size"] > 0, plat
assert isinstance(m["signature"], str) and len(m["signature"]) > 0, m
PY
}

test_sha256_and_size_match_inputs() {
  new_workspace
  invoke_script >/dev/null

  local expected_mac_sha expected_win_sha expected_mac_size expected_win_size
  expected_mac_sha=$(shasum -a 256 "$TMP_DIR/snappyref-mac.zip" | awk '{print $1}')
  expected_win_sha=$(shasum -a 256 "$TMP_DIR/snappyref-windows.zip" | awk '{print $1}')
  expected_mac_size=$(wc -c < "$TMP_DIR/snappyref-mac.zip" | tr -d ' ')
  expected_win_size=$(wc -c < "$TMP_DIR/snappyref-windows.zip" | tr -d ' ')

  local actual_mac_sha actual_win_sha actual_mac_size actual_win_size
  actual_mac_sha=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["platforms"]["macos"]["sha256"])' "$TMP_DIR/latest.json")
  actual_win_sha=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["platforms"]["windows"]["sha256"])' "$TMP_DIR/latest.json")
  actual_mac_size=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["platforms"]["macos"]["size"])' "$TMP_DIR/latest.json")
  actual_win_size=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["platforms"]["windows"]["size"])' "$TMP_DIR/latest.json")

  [[ "$actual_mac_sha" == "$expected_mac_sha" ]]
  [[ "$actual_win_sha" == "$expected_win_sha" ]]
  [[ "$actual_mac_size" == "$expected_mac_size" ]]
  [[ "$actual_win_size" == "$expected_win_size" ]]
}

test_signature_verifies_against_canonical_bytes() {
  new_workspace
  invoke_script >/dev/null

  # Reconstruct the canonical signing bytes the way a verifying client must.
  TMP_DIR="$TMP_DIR" python3 - <<'PY'
import json, os
tmp = os.environ["TMP_DIR"]
m = json.load(open(f"{tmp}/latest.json"))
sig_hex = m.pop("signature")
open(f"{tmp}/canonical.bin", "wb").write(
    json.dumps(m, sort_keys=True, separators=(",", ":")).encode("utf-8")
)
open(f"{tmp}/signature.hex", "w").write(sig_hex)
PY

  xxd -r -p "$TMP_DIR/signature.hex" > "$TMP_DIR/signature.bin"
  openssl pkeyutl -verify -pubin -inkey "$TMP_DIR/pub.pem" \
    -rawin -in "$TMP_DIR/canonical.bin" \
    -sigfile "$TMP_DIR/signature.bin" >/dev/null
}

test_verify_with_wrong_pubkey_fails() {
  new_workspace
  openssl genpkey -algorithm ed25519 -out "$TMP_DIR/other-priv.pem" 2>/dev/null
  openssl pkey -in "$TMP_DIR/other-priv.pem" -pubout -out "$TMP_DIR/other-pub.pem" 2>/dev/null

  if invoke_script "$TMP_DIR/other-pub.pem" >/dev/null 2>&1; then
    echo "expected script to fail when public key does not match private key" >&2
    return 1
  fi
}

test_canonical_form_is_sorted_and_compact() {
  new_workspace
  invoke_script >/dev/null

  TMP_DIR="$TMP_DIR" python3 - <<'PY'
import json, os
m = json.load(open(os.environ["TMP_DIR"] + "/latest.json"))
m.pop("signature")
text = json.dumps(m, sort_keys=True, separators=(",", ":"))
assert " " not in text, "canonical form contains whitespace"
assert "\n" not in text, "canonical form contains newlines"

def assert_sorted(node):
    if isinstance(node, dict):
        ks = list(node.keys())
        assert ks == sorted(ks), f"unsorted keys: {ks}"
        for v in node.values():
            assert_sorted(v)
    elif isinstance(node, list):
        for v in node:
            assert_sorted(v)

assert_sorted(m)
PY
}

run_case test_produces_required_keys
run_case test_sha256_and_size_match_inputs
run_case test_signature_verifies_against_canonical_bytes
run_case test_verify_with_wrong_pubkey_fails
run_case test_canonical_form_is_sorted_and_compact

echo "---"
echo "passed: $PASS"
echo "failed: $FAIL"
if [[ "$FAIL" -ne 0 ]]; then
  exit 1
fi
