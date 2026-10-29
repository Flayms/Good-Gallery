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
    for krate in ["domain", "db", "media", "meta", "indexer", "app"] {
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

/// `web-sys` features that make requests or open connections.
const NETWORK_WEB_APIS: [&str; 9] = [
    "Request",
    "Response",
    "XmlHttpRequest",
    "WebSocket",
    "EventSource",
    "RtcPeerConnection",
    "WebTransport",
    "Headers",
    "ServiceWorkerContainer",
];

/// REQ-019-12: the browser version enables no web API that sends anything.
#[test]
fn req_019_12_the_page_enables_no_network_web_api() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(workspace())
        .args([
            "tree",
            "-p",
            "app",
            "-e",
            "normal,features",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .args(["--prefix", "none", "--format", "{p} {f}", "--offline"])
        .output()
        .expect("cargo tree runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let tree = String::from_utf8(output.stdout).unwrap();
    let features: Vec<&str> = tree
        .lines()
        .filter(|line| line.starts_with("web-sys v"))
        .flat_map(|line| line.split_whitespace().nth(2).unwrap_or_default().split(','))
        .collect();
    assert!(features.contains(&"HtmlCanvasElement"), "{features:?}");
    let network: Vec<&&str> = features.iter().filter(|f| NETWORK_WEB_APIS.contains(f)).collect();
    assert!(network.is_empty(), "network web APIs in the page: {network:?}");
}

/// REQ-019-12: the page's JavaScript makes no requests, and its content security policy allows none to other hosts.
#[test]
fn req_019_12_the_pages_scripts_send_nothing_and_its_policy_forbids_it() {
    let root = workspace();
    let scripts = ["crates/app/src/web/glue.js", "web/check.js"];
    for script in scripts {
        let text = std::fs::read_to_string(root.join(script)).unwrap();
        for api in [
            "fetch(",
            "XMLHttpRequest",
            "WebSocket",
            "sendBeacon",
            "EventSource",
            "RTCPeerConnection",
            "http",
        ] {
            assert!(!text.contains(api), "{script} uses {api}");
        }
    }
    let page = std::fs::read_to_string(root.join("web/index.html")).unwrap();
    for directive in ["default-src 'self'", "connect-src 'self'", "form-action 'none'"] {
        assert!(page.contains(directive), "index.html's policy lacks {directive}");
    }
    // `'wasm-unsafe-eval'` compiles the page's own WebAssembly; `'unsafe-eval'` would allow JavaScript's eval.
    assert!(!page.contains(" 'unsafe-eval'"), "index.html allows eval");
    assert!(
        !page.contains("script-src 'self' 'unsafe-inline'"),
        "index.html allows inline scripts"
    );
}
