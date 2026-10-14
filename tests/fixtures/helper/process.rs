// Deterministic executable for the image-preparation process boundary tests.
use std::{env, fs, path::PathBuf, thread, time::Duration};

fn main() {
    let args: Vec<String> = env::args().collect();
    let source = PathBuf::from(args.iter().find_map(|a| a.strip_prefix("--source=")).unwrap());
    let output = PathBuf::from(args.iter().find_map(|a| a.strip_prefix("--rgba-output=")).unwrap());
    let name = source.file_name().unwrap().to_str().unwrap();
    let mode = ["crash", "invalid", "hang"].into_iter().find(|mode| name.ends_with(&format!("{mode}.png"))).unwrap_or("valid");
    match mode {
        "crash" => { eprintln!("fixture crash"); std::process::exit(9); }
        "invalid" => { println!("invalid helper JSON"); return; }
        "hang" => thread::sleep(Duration::from_secs(3)),
        _ => {}
    }
    fs::write(&output, [30, 100, 220, 255]).unwrap();
    let path = output.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
    println!("{{\"animated\":false,\"rgba_path\":\"{path}\",\"width\":1,\"height\":1,\"source_width\":1,\"source_height\":1,\"mime_type\":\"image/png\",\"cache_path\":null,\"cache_extension\":null}}");
}
