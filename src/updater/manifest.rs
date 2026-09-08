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

use crate::updater::platform::TargetPlatform;

/// Build-time decoding of the same PEM used by release signing verification.
/// The installed app needs only these embedded bytes, not the PEM file.
const SIGNING_PUBLIC_KEY: &[u8; 32] =
    include_bytes!(concat!(env!("OUT_DIR"), "/signing-public-key.bin"));

/// "stable" is the only channel that exists at runtime. The manifest keeps
/// the field so future beta/insider channels can be rejected explicitly by
/// older stable builds.
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
    pub macos: PlatformEntry,
    pub windows: PlatformEntry,
}

impl Manifest {
    /// Returns the per-platform entry for the supplied target. Both fields
    /// are required by the schema, so the lookup is infallible.
    pub(crate) fn entry_for(&self, platform: TargetPlatform) -> &PlatformEntry {
        match platform {
            TargetPlatform::Macos => &self.macos,
            TargetPlatform::Windows => &self.windows,
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
    let key = VerifyingKey::from_bytes(SIGNING_PUBLIC_KEY)
        .expect("build validated the embedded Ed25519 public key");
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

    // The signed wire schema requires this field; runtime does not retain it.
    object
        .get("notes_url")
        .and_then(Value::as_str)
        .ok_or(ParseError::MissingField("notes_url"))?;

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
        assert!(VerifyingKey::from_bytes(SIGNING_PUBLIC_KEY).is_ok());
    }
}
