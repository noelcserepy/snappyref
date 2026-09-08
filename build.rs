use ed25519_dalek::{VerifyingKey, pkcs8::DecodePublicKey};

fn main() {
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
