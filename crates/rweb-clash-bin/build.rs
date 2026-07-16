use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set"));
    let dest = out_dir.join("embedded_assets.rs");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");

    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("web/dist").display()
    );
    let core_target = core_target_dir();
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("packaging/cache/cores").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("packaging/cache/rule-sets").display()
    );

    let mut files = Vec::new();
    collect_files(&repo_root.join("web/dist"), "web", &mut files);
    collect_files(
        &repo_root.join("packaging/cache/cores").join(core_target),
        "core",
        &mut files,
    );
    collect_files(
        &repo_root.join("packaging/cache/rule-sets"),
        "rule-sets",
        &mut files,
    );
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut output = String::new();
    output.push_str("static EMBEDDED_FILES: &[rweb_clash::EmbeddedFile] = &[\n");
    for (logical_path, source_path) in files {
        output.push_str("    rweb_clash::EmbeddedFile { path: ");
        output.push_str(&format!("{logical_path:?}"));
        output.push_str(", bytes: include_bytes!(");
        output.push_str(&format!("{:?}", source_path.display().to_string()));
        output.push_str(") },\n");
    }
    output.push_str("];\n");
    output.push_str(
        "static EMBEDDED_ASSETS: rweb_clash::EmbeddedAssets = rweb_clash::EmbeddedAssets { files: EMBEDDED_FILES };\n",
    );
    fs::write(dest, output).expect("write embedded assets module");
}

fn core_target_dir() -> &'static str {
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("linux") && target.contains("aarch64") {
        "linux-arm64"
    } else {
        "linux-amd64"
    }
}

fn collect_files(dir: &Path, logical_root: &str, files: &mut Vec<(String, PathBuf)>) {
    if !dir.is_dir() {
        return;
    }
    collect_files_inner(dir, dir, logical_root, files);
}

fn collect_files_inner(
    root: &Path,
    dir: &Path,
    logical_root: &str,
    files: &mut Vec<(String, PathBuf)>,
) {
    let entries = fs::read_dir(dir).expect("read asset directory");
    for entry in entries {
        let entry = entry.expect("read asset entry");
        let path = entry.path();
        if path.is_dir() {
            collect_files_inner(root, &path, logical_root, files);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let relative = path.strip_prefix(root).expect("relative asset path");
        let relative = relative.to_string_lossy().replace('\\', "/");
        files.push((format!("{logical_root}/{relative}"), path));
    }
}
