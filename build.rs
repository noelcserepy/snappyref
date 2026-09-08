use ed25519_dalek::{VerifyingKey, pkcs8::DecodePublicKey};

fn main() {
    let descriptor_path = "assets/semantic-search-descriptor.env";
    println!("cargo:rerun-if-changed={descriptor_path}");
    let descriptor =
        std::fs::read_to_string(descriptor_path).expect("read Semantic Search descriptor");
    let mut values = std::collections::BTreeMap::new();
    for line in descriptor.lines() {
        let (key, value) = line
            .split_once('=')
            .expect("descriptor requires key=value lines");
        assert!(
            values.insert(key, value).is_none(),
            "duplicate descriptor key"
        );
    }
    assert_eq!(
        values.len(),
        3,
        "descriptor must contain URL, SHA256 and size only"
    );
    for field in ["url", "sha256", "size"] {
        let key = format!("semantic_search_{field}");
        let environment = format!("SNAPPYREF_SEMANTIC_SEARCH_{}", field.to_uppercase());
        println!("cargo:rerun-if-env-changed={environment}");
        let default = values.get(key.as_str()).expect("missing descriptor key");
        let value = std::env::var(&environment)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| default.to_string());
        assert!(!value.contains(['\n', '\r']), "invalid descriptor value");
        match field {
            "url" => assert!(
                (value.starts_with("https://") || value.starts_with("http://"))
                    && value
                        .split_once("://")
                        .is_some_and(|(_, rest)| !rest.is_empty() && !rest.starts_with('/'))
                    && !value.chars().any(char::is_whitespace),
                "descriptor requires HTTP URL"
            ),
            "sha256" => assert!(
                value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "descriptor requires SHA256"
            ),
            "size" => assert!(
                value.bytes().all(|byte| byte.is_ascii_digit())
                    && !value.starts_with('0')
                    && value.parse::<u64>().is_ok_and(|size| size > 0),
                "descriptor requires positive byte size"
            ),
            _ => unreachable!(),
        }
        println!("cargo:rustc-env={environment}={value}");
    }

    let public_key_path = "docs/keys/signing-key.pub.pem";
    println!("cargo:rerun-if-changed={public_key_path}");
    let pem = std::fs::read_to_string(public_key_path).expect("read update signing public key");
    let key = VerifyingKey::from_public_key_pem(&pem)
        .expect("update signing public key must be an Ed25519 public key in PEM format");
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    std::fs::write(output.join("signing-public-key.bin"), key.to_bytes())
        .expect("write embedded update signing public key");

    println!("cargo:rerun-if-changed=assets/app-icon/snappyref.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/app-icon/snappyref.ico");
        resource.set("ProductName", "SnappyRef");
        resource.set("FileDescription", "SnappyRef");
        resource.set("OriginalFilename", "snappyref.exe");
        resource.set_manifest(
            r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#,
        );
        resource.compile().expect("compile Windows app resource");
    }
}
