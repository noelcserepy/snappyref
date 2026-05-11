//! Signed-manifest schema and verification for auto-update discovery.
//!
//! Schema is defined by `scripts/build-signed-manifest.sh`:
//! the canonical bytes are sorted-keys compact JSON with the `signature`
//! field stripped. The signature is a hex-encoded ed25519 PureEdDSA
//! signature over those bytes, verified against a public key compiled into
//! the app as a constant.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use serde_json::Value;

/// Ed25519 public key for the auto-update signing keypair, embedded at
/// compile time. PEM-decoded SubjectPublicKeyInfo bytes are 44 bytes; the
/// last 32 are the raw public key. Provisioned by issue #79; lives in the
/// repo at `docs/keys/signing-key.pub.pem`.
const SIGNING_PUBLIC_KEY_PEM: &str = include_str!("../../docs/keys/signing-key.pub.pem");

/// "stable" is the only channel that exists at runtime. The field is
/// reserved for future beta/insider channels (ADR-0015).
const EXPECTED_CHANNEL: &str = "stable";

/// Verified platform entry from a parsed manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlatformEntry {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// Parsed and signature-verified manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Manifest {
    pub version: String,
    pub notes_url: String,
    pub macos: PlatformEntry,
    pub windows: PlatformEntry,
}

impl Manifest {
    /// Returns the per-platform entry for the running target OS, if the
    /// manifest contains one. Slice 4 only consults the URL field, slice 5
    /// will use sha256 + size for download verification.
    #[allow(dead_code)]
    pub(crate) fn entry_for_current_platform(&self) -> Option<&PlatformEntry> {
        if cfg!(target_os = "macos") {
            Some(&self.macos)
        } else if cfg!(target_os = "windows") {
            Some(&self.windows)
        } else {
            None
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParseError {
    InvalidJson,
    MissingSignature,
    InvalidSignatureEncoding,
    SignatureMismatch,
    UnknownChannel,
    MissingField(&'static str),
    InvalidFieldType(&'static str),
    InvalidEmbeddedPublicKey,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::InvalidJson => write!(f, "manifest is not valid JSON"),
            ParseError::MissingSignature => write!(f, "manifest has no `signature` field"),
            ParseError::InvalidSignatureEncoding => {
                write!(f, "manifest `signature` is not 64 bytes of hex")
            }
            ParseError::SignatureMismatch => {
                write!(f, "manifest signature does not verify against embedded key")
            }
            ParseError::UnknownChannel => write!(f, "manifest channel is not the expected value"),
            ParseError::MissingField(name) => write!(f, "manifest is missing field `{name}`"),
            ParseError::InvalidFieldType(name) => {
                write!(f, "manifest field `{name}` has the wrong type")
            }
            ParseError::InvalidEmbeddedPublicKey => {
                write!(f, "embedded public key is not a valid ed25519 PEM")
            }
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Deserialize)]
struct PlatformsRaw {
    macos: Option<PlatformRaw>,
    windows: Option<PlatformRaw>,
}

#[derive(Deserialize)]
struct PlatformRaw {
    url: String,
    sha256: String,
    size: u64,
}

/// Parse + verify the manifest using the compiled-in public key.
pub(crate) fn parse_and_verify(bytes: &[u8]) -> Result<Manifest, ParseError> {
    let key = decode_pem_public_key(SIGNING_PUBLIC_KEY_PEM)
        .ok_or(ParseError::InvalidEmbeddedPublicKey)?;
    parse_and_verify_with_key(bytes, &key)
}

/// Test-friendly variant that takes the public key directly.
pub(crate) fn parse_and_verify_with_key(
    bytes: &[u8],
    key: &VerifyingKey,
) -> Result<Manifest, ParseError> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|_| ParseError::InvalidJson)?;

    let signature_hex = value
        .as_object_mut()
        .and_then(|map| map.remove("signature"))
        .ok_or(ParseError::MissingSignature)?;
    let signature_hex = signature_hex
        .as_str()
        .ok_or(ParseError::InvalidFieldType("signature"))?
        .to_owned();

    let signature_bytes =
        hex::decode(&signature_hex).map_err(|_| ParseError::InvalidSignatureEncoding)?;
    let signature_bytes: [u8; 64] = signature_bytes
        .as_slice()
        .try_into()
        .map_err(|_| ParseError::InvalidSignatureEncoding)?;
    let signature = Signature::from_bytes(&signature_bytes);

    // Recompute the canonical bytes the publisher signed. Must match
    // build-signed-manifest.sh exactly: sorted keys, no whitespace,
    // signature field stripped. serde_json's BTreeMap-backed `Value` already
    // serializes object keys in sorted order, and the default Formatter
    // emits no whitespace, so this matches Python's
    // `json.dumps(sort_keys=True, separators=(",", ":"))`.
    let canonical = serde_json::to_vec(&value).map_err(|_| ParseError::InvalidJson)?;

    key.verify(&canonical, &signature)
        .map_err(|_| ParseError::SignatureMismatch)?;

    // Signature OK — pull out the typed fields. Any missing-field error here
    // happens against a verified payload, so it indicates a publisher bug
    // rather than tampering.
    let object = value.as_object().ok_or(ParseError::InvalidJson)?;

    let channel = object
        .get("channel")
        .and_then(Value::as_str)
        .ok_or(ParseError::MissingField("channel"))?;
    if channel != EXPECTED_CHANNEL {
        return Err(ParseError::UnknownChannel);
    }

    let version = object
        .get("version")
        .and_then(Value::as_str)
        .ok_or(ParseError::MissingField("version"))?
        .to_owned();

    let notes_url = object
        .get("notes_url")
        .and_then(Value::as_str)
        .ok_or(ParseError::MissingField("notes_url"))?
        .to_owned();

    let platforms_value = object
        .get("platforms")
        .ok_or(ParseError::MissingField("platforms"))?
        .clone();
    let platforms: PlatformsRaw = serde_json::from_value(platforms_value)
        .map_err(|_| ParseError::InvalidFieldType("platforms"))?;

    let macos = platforms
        .macos
        .ok_or(ParseError::MissingField("platforms.macos"))?;
    let windows = platforms
        .windows
        .ok_or(ParseError::MissingField("platforms.windows"))?;

    Ok(Manifest {
        version,
        notes_url,
        macos: PlatformEntry {
            url: macos.url,
            sha256: macos.sha256,
            size: macos.size,
        },
        windows: PlatformEntry {
            url: windows.url,
            sha256: windows.sha256,
            size: windows.size,
        },
    })
}

/// Decode a PEM SubjectPublicKeyInfo blob containing an ed25519 key. The
/// raw 32-byte public key sits at the end of the DER body; the SPKI
/// AlgorithmIdentifier prefix is `30 2a 30 05 06 03 2b 65 70 03 21 00`
/// (12 bytes) for ed25519. Returning `None` on any mismatch keeps the
/// embedded-key validation simple and makes a bad key trigger
/// `InvalidEmbeddedPublicKey` rather than misverify.
fn decode_pem_public_key(pem: &str) -> Option<VerifyingKey> {
    let mut der_b64 = String::new();
    for line in pem.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("-----") {
            continue;
        }
        der_b64.push_str(line);
    }
    let der = base64_decode(&der_b64)?;
    // ed25519 SPKI is 44 bytes total: 12 byte header + 32 byte key.
    if der.len() != 44 {
        return None;
    }
    const SPKI_PREFIX: [u8; 12] = [
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    if der[..12] != SPKI_PREFIX {
        return None;
    }
    let raw: [u8; 32] = der[12..].try_into().ok()?;
    VerifyingKey::from_bytes(&raw).ok()
}

/// Minimal base64 decoder — only used at startup against a known-good blob
/// (`docs/keys/signing-key.pub.pem`) so a dedicated base64 dep is overkill.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lut = [u8::MAX; 256];
    for (i, byte) in ALPHABET.iter().enumerate() {
        lut[*byte as usize] = i as u8;
    }
    let mut bytes = Vec::with_capacity(input.len() / 4 * 3);
    let mut buf = [0u8; 4];
    let mut buf_len = 0;
    let mut pad = 0;
    for ch in input.bytes() {
        if ch == b'=' {
            pad += 1;
            buf[buf_len] = 0;
            buf_len += 1;
        } else if ch.is_ascii_whitespace() {
            continue;
        } else {
            let value = lut[ch as usize];
            if value == u8::MAX {
                return None;
            }
            buf[buf_len] = value;
            buf_len += 1;
        }
        if buf_len == 4 {
            bytes.push((buf[0] << 2) | (buf[1] >> 4));
            bytes.push((buf[1] << 4) | (buf[2] >> 2));
            bytes.push((buf[2] << 6) | buf[3]);
            buf_len = 0;
        }
    }
    if buf_len != 0 {
        return None;
    }
    bytes.truncate(bytes.len().saturating_sub(pad));
    Some(bytes)
}

#[cfg(test)]
pub(crate) fn embedded_public_key_for_tests() -> Option<VerifyingKey> {
    decode_pem_public_key(SIGNING_PUBLIC_KEY_PEM)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    fn build_signed(value: &serde_json::Value, signing_key: &SigningKey) -> Vec<u8> {
        // Match the publisher's canonical-bytes contract: serialize the
        // value (without `signature`) as sorted-keys compact JSON, sign,
        // splice the hex signature in, re-serialize.
        let unsigned_canonical = serde_json::to_vec(value).unwrap();
        let signature = signing_key.sign(&unsigned_canonical);
        let mut signed = value.clone();
        signed.as_object_mut().unwrap().insert(
            "signature".to_owned(),
            json!(hex::encode(signature.to_bytes())),
        );
        serde_json::to_vec(&signed).unwrap()
    }

    fn valid_unsigned() -> serde_json::Value {
        json!({
            "channel": "stable",
            "version": "0.2.0",
            "released_at": "2026-05-11T12:00:00Z",
            "notes_url": "https://github.com/example/notes",
            "platforms": {
                "macos": {
                    "url": "https://example/mac.zip",
                    "sha256": "00".repeat(32),
                    "size": 1024,
                },
                "windows": {
                    "url": "https://example/windows.zip",
                    "sha256": "11".repeat(32),
                    "size": 2048,
                },
            },
        })
    }

    fn fresh_keypair() -> SigningKey {
        // Deterministic for reproducible tests but not from any production
        // source.
        SigningKey::from_bytes(&[7u8; 32])
    }

    #[test]
    fn well_formed_manifest_verifies() {
        let key = fresh_keypair();
        let bytes = build_signed(&valid_unsigned(), &key);
        let manifest = parse_and_verify_with_key(&bytes, &key.verifying_key()).unwrap();
        assert_eq!(manifest.version, "0.2.0");
        assert_eq!(manifest.macos.url, "https://example/mac.zip");
        assert_eq!(manifest.windows.size, 2048);
        assert_eq!(manifest.notes_url, "https://github.com/example/notes");
    }

    #[test]
    fn tampered_bytes_are_rejected() {
        let key = fresh_keypair();
        let bytes = build_signed(&valid_unsigned(), &key);
        // Flip a byte in the version string. The signature, computed over
        // the canonical bytes, will not verify against the tampered body.
        let tampered = String::from_utf8(bytes)
            .unwrap()
            .replace("0.2.0", "9.9.9")
            .into_bytes();
        assert_eq!(
            parse_and_verify_with_key(&tampered, &key.verifying_key()),
            Err(ParseError::SignatureMismatch)
        );
    }

    #[test]
    fn signature_from_wrong_key_is_rejected() {
        let attacker = SigningKey::from_bytes(&[9u8; 32]);
        let bytes = build_signed(&valid_unsigned(), &attacker);
        let legitimate = fresh_keypair();
        assert_eq!(
            parse_and_verify_with_key(&bytes, &legitimate.verifying_key()),
            Err(ParseError::SignatureMismatch)
        );
    }

    #[test]
    fn missing_signature_field_is_rejected() {
        let key = fresh_keypair();
        let unsigned = serde_json::to_vec(&valid_unsigned()).unwrap();
        assert_eq!(
            parse_and_verify_with_key(&unsigned, &key.verifying_key()),
            Err(ParseError::MissingSignature)
        );
    }

    #[test]
    fn unknown_channel_is_rejected() {
        let key = fresh_keypair();
        let mut body = valid_unsigned();
        body.as_object_mut()
            .unwrap()
            .insert("channel".to_owned(), json!("insider"));
        let bytes = build_signed(&body, &key);
        assert_eq!(
            parse_and_verify_with_key(&bytes, &key.verifying_key()),
            Err(ParseError::UnknownChannel)
        );
    }

    #[test]
    fn missing_platform_field_is_rejected() {
        let key = fresh_keypair();
        let mut body = valid_unsigned();
        body["platforms"].as_object_mut().unwrap().remove("macos");
        let bytes = build_signed(&body, &key);
        assert_eq!(
            parse_and_verify_with_key(&bytes, &key.verifying_key()),
            Err(ParseError::MissingField("platforms.macos"))
        );
    }

    #[test]
    fn invalid_json_is_rejected() {
        let key = fresh_keypair();
        assert_eq!(
            parse_and_verify_with_key(b"not-json", &key.verifying_key()),
            Err(ParseError::InvalidJson)
        );
    }

    #[test]
    fn embedded_public_key_decodes() {
        // The PEM file shipped in the repo must be a valid ed25519 SPKI;
        // otherwise discovery would always fail at runtime with
        // InvalidEmbeddedPublicKey.
        assert!(embedded_public_key_for_tests().is_some());
    }
}
