//! REQ-019-12: the browser version sends nothing. The desktop app downloads face models (spec 031).

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
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
