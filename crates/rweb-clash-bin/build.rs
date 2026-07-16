use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const EXPECTED_RULE_SET_FILES: &[&str] = &[
    "rs_builtin_apple.list",
    "rs_builtin_applications.list",
    "rs_builtin_cncidr.list",
    "rs_builtin_direct.list",
    "rs_builtin_gfw.list",
    "rs_builtin_google.list",
    "rs_builtin_icloud.list",
    "rs_builtin_lancidr.list",
    "rs_builtin_private.list",
    "rs_builtin_proxy.list",
    "rs_builtin_reject.list",
    "rs_builtin_telegramcidr.list",
    "rs_builtin_tld_not_cn.list",
];
const MIN_GEOIP_DATABASE_BYTES: u64 = 1_048_576;

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
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("packaging/cache/cores").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("packaging/cache/rule-sets").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("packaging/cache/runtime").display()
    );
    if env::var_os("CARGO_FEATURE_EMBEDDED_ASSETS").is_some() {
        validate_embedded_runtime_assets(repo_root);
    }

    let mut files = Vec::new();
    collect_files(&repo_root.join("web/dist"), "web", &mut files);
    if let Some(core_target) = core_target_dir() {
        collect_files(
            &repo_root.join("packaging/cache/cores").join(core_target),
            "core",
            &mut files,
        );
    }
    collect_files(
        &repo_root.join("packaging/cache/rule-sets"),
        "rule-sets",
        &mut files,
    );
    collect_files(
        &repo_root.join("packaging/cache/runtime"),
        "runtime",
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

fn core_target_dir() -> Option<&'static str> {
    let target = env::var("TARGET").unwrap_or_default();
    match () {
        _ if target.contains("linux") && target.contains("aarch64") => Some("linux-arm64"),
        _ if target.contains("linux") && target.contains("x86_64") => Some("linux-amd64"),
        _ if target.contains("windows") && target.contains("x86_64") => Some("windows-amd64"),
        _ if target.contains("darwin") && target.contains("aarch64") => Some("macos-arm64"),
        _ => None,
    }
}

fn validate_embedded_runtime_assets(repo_root: &Path) {
    let core_name = if env::var("TARGET").unwrap_or_default().contains("windows") {
        "mihomo.exe"
    } else {
        "mihomo"
    };
    require_non_empty_file(
        &repo_root
            .join("packaging/cache/cores")
            .join(core_target_dir().unwrap_or_else(|| {
                panic!(
                    "embedded Mihomo core is unsupported for target {}",
                    env::var("TARGET").unwrap_or_default()
                )
            }))
            .join(core_name),
        "Mihomo core",
    );
    require_file_size(
        &repo_root.join("packaging/cache/runtime/geoip.metadb"),
        "GeoIP database",
        MIN_GEOIP_DATABASE_BYTES,
    );

    let rule_set_dir = repo_root.join("packaging/cache/rule-sets");
    let mut actual = fs::read_dir(&rule_set_dir)
        .unwrap_or_else(|error| {
            panic!(
                "failed to read embedded rule-set directory {}: {error}",
                rule_set_dir.display()
            )
        })
        .map(|entry| entry.expect("read embedded rule-set entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "list")
        })
        .map(|path| {
            require_non_empty_file(&path, "rule-set snapshot");
            path.file_name()
                .expect("rule-set snapshot has a file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    actual.sort();
    let mut expected = EXPECTED_RULE_SET_FILES
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(
        actual, expected,
        "embedded-assets requires exactly the 13 builtin rule-set snapshots"
    );
}

fn require_non_empty_file(path: &Path, label: &str) {
    require_file_size(path, label, 1);
}

fn require_file_size(path: &Path, label: &str, minimum_bytes: u64) {
    let metadata = fs::metadata(path)
        .unwrap_or_else(|error| panic!("missing {label} at {}: {error}", path.display()));
    assert!(
        metadata.is_file() && metadata.len() >= minimum_bytes,
        "{label} must be a regular file of at least {minimum_bytes} bytes at {}",
        path.display()
    );
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
