//! REQ-011-11: the app opens no network connections of its own. It has no network code and no dependency that brings
//! one (HTTP clients, TLS, async network runtimes); these tests keep it that way.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Crates that make network connections or exist to support them.
const NETWORK_CRATES: [&str; 22] = [
    "reqwest",
    "hyper",
    "hyper-util",
    "h2",
    "h3",
    "quinn",
    "ureq",
    "curl",
    "curl-sys",
    "isahc",
    "surf",
    "attohttpc",
    "rustls",
    "native-tls",
    "openssl",
    "openssl-sys",
    "tokio",
    "async-std",
    "smol",
    "mio",
    "socket2",
    "tungstenite",
];

/// Network crates allowed when only the named crate brings them in: notify's Linux backend waits for inotify events
/// with mio's poller, and opens no sockets.
const ALLOWED_THROUGH: [(&str, &str); 1] = [("mio", "notify")];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// The crates the app is built from on this platform (normal dependencies, not build or dev dependencies).
fn app_dependencies() -> Vec<String> {
    tree(&[])
}

/// The crates that depend on `krate` directly, in the app's dependency tree.
fn dependents(krate: &str) -> Vec<String> {
    tree(&["--invert", krate, "--depth", "1"]).split_off(1)
}

/// `cargo tree` of the app's normal dependencies with `args`: one crate name per line.
fn tree(args: &[&str]) -> Vec<String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(workspace())
        .args([
            "tree",
            "-p",
            "app",
            "-e",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
            "--offline",
        ])
        .args(args)
        .output()
        .expect("cargo tree runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect()
}

#[test]
fn req_011_11_the_app_depends_on_no_network_crate() {
    let dependencies = app_dependencies();
    assert!(dependencies.iter().any(|name| name == "eframe"), "{dependencies:?}");
    let network: Vec<&String> = dependencies
        .iter()
        .filter(|name| NETWORK_CRATES.contains(&name.as_str()))
        .filter(|name| {
            let allowed = ALLOWED_THROUGH.iter().find(|(krate, _)| krate == name);
            !allowed.is_some_and(|(krate, parent)| dependents(krate).iter().all(|dependent| dependent == parent))
        })
        .collect();
    assert!(network.is_empty(), "network crates in the app: {network:?}");
}

/// Rust files below `dir`.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn req_011_11_the_app_has_no_network_code() {
    let mut files = Vec::new();
    for krate in ["domain", "db", "media", "indexer", "app"] {
        sources(&workspace().join("crates").join(krate).join("src"), &mut files);
    }
    assert!(files.len() > 20, "{files:?}");
    let socket_apis = [
        "std::net",
        "TcpStream",
        "TcpListener",
        "UdpSocket",
        "WSAStartup",
        "WinHttp",
        "URLDownload",
    ];
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for api in socket_apis {
            assert!(!text.contains(api), "{} uses {api}", file.display());
        }
    }
}
