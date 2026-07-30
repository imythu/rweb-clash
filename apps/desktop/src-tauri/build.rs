use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    generate_embedded_core();
    tauri_build::build()
}

fn generate_embedded_core() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let file_name = if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        "mihomo.exe"
    } else {
        "mihomo"
    };
    let core_path = manifest_dir.join("resources/core").join(file_name);
    println!("cargo:rerun-if-changed={}", core_path.display());

    let mut output =
        String::from("static DESKTOP_EMBEDDED_FILES: &[rweb_clash::EmbeddedFile] = &[\n");
    if core_path.is_file() {
        output.push_str("    rweb_clash::EmbeddedFile { path: ");
        output.push_str(&format!("{:?}", format!("core/{file_name}")));
        output.push_str(", bytes: include_bytes!(");
        output.push_str(&format!("{:?}", core_path.display().to_string()));
        output.push_str(") },\n");
    }
    output.push_str("];\n");
    output.push_str(
        "static DESKTOP_EMBEDDED_ASSETS: rweb_clash::EmbeddedAssets = rweb_clash::EmbeddedAssets { files: DESKTOP_EMBEDDED_FILES };\n",
    );

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set"));
    fs::write(out_dir.join("desktop_embedded_assets.rs"), output)
        .expect("write desktop embedded assets module");
}
