fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon/snappyref.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/app-icon/snappyref.ico");
        resource.set("ProductName", "SnappyRef");
        resource.set("FileDescription", "SnappyRef");
        resource.set("OriginalFilename", "snappyref.exe");
        resource.compile().expect("compile Windows app resource");
    }
}
