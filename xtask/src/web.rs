//! `cargo xtask web [--debug]`: the browser version's page (spec 019) in `target/web`, ready for any static host.
//! Builds it with Trunk (`web/Trunk.toml`), then moves Trunk's inline start script into a file of its own, so the
//! page's content security policy needs no `'unsafe-inline'` for scripts (REQ-019-12).
//!
//! `cargo xtask web-test [--no-build] [<playwright options>…]`: builds the page, then runs its end-to-end tests
//! (`web/tests`) in Playwright's Chromium; e.g. `--update-snapshots` after a deliberate change to the UI.

use anyhow::{Context as _, bail};
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::Path;
use std::process::Command;

const USAGE: &str = "usage: cargo xtask web [--debug]";

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let release = match args {
        [] => true,
        [flag] if flag == "--debug" => false,
        _ => bail!(USAGE),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut trunk = Command::new("trunk");
    trunk.arg("build").current_dir(root.join("web"));
    if !release {
        trunk.arg("--release=false");
    }
    let status = trunk
        .status()
        .context("can't run trunk; install it with `cargo install --locked trunk`")?;
    if !status.success() {
        bail!("trunk build failed");
    }
    let dist = root.join("target/web");
    let index = dist.join("index.html");
    let html = std::fs::read_to_string(&index).with_context(|| format!("can't read {}", index.display()))?;
    let (html, script_name, script) = move_start_script(&html)?;
    std::fs::write(dist.join(&script_name), script)?;
    std::fs::write(&index, html)?;
    println!("the page is in {}", dist.display());
    Ok(())
}

/// Builds the page (unless `--no-build`) and runs the browser tests; other arguments go to Playwright.
pub fn test(args: &[String]) -> anyhow::Result<()> {
    let build = !args.iter().any(|arg| arg == "--no-build");
    let playwright_args: Vec<&String> = args.iter().filter(|arg| *arg != "--no-build").collect();
    if build {
        run(&[])?;
    }
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("../web/tests");
    // `npm` and `npx` are `.cmd` scripts on Windows, which `Command` doesn't run by their bare name.
    let (npm, npx) = if cfg!(windows) {
        ("npm.cmd", "npx.cmd")
    } else {
        ("npm", "npx")
    };
    if !tests.join("node_modules").exists() {
        let status = Command::new(npm)
            .arg("ci")
            .current_dir(&tests)
            .status()
            .context("can't run npm; install Node.js")?;
        if !status.success() {
            bail!("npm ci failed");
        }
        let status = Command::new(npx)
            .args(["playwright", "install", "chromium"])
            .current_dir(&tests)
            .status()?;
        if !status.success() {
            bail!("can't install Playwright's Chromium");
        }
    }
    let status = Command::new(npx)
        .args(["playwright", "test"])
        .args(playwright_args)
        .current_dir(&tests)
        .status()
        .context("can't run Playwright")?;
    if !status.success() {
        bail!("the browser tests failed");
    }
    Ok(())
}

/// Moves the page's inline module script into a file: the new page, the file's name and its content. The script only
/// starts the app when `check.js` found the browser able to run it (REQ-019-01).
pub fn move_start_script(html: &str) -> anyhow::Result<(String, String, String)> {
    const OPEN: &str = "<script type=\"module\">";
    const CLOSE: &str = "</script>";
    let start = html.find(OPEN).context("the page has no inline start script")?;
    let body_start = start + OPEN.len();
    let body_len = html[body_start..]
        .find(CLOSE)
        .context("the start script isn't closed")?;
    let body = &html[body_start..body_start + body_len];
    if html[body_start + body_len..].contains(OPEN) {
        bail!("the page has more than one inline script");
    }
    let (imports, rest): (Vec<&str>, Vec<&str>) = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .partition(|line| line.starts_with("import "));
    let mut script = imports.join("\n");
    script.push_str("\nif (!window.goodGalleryUnsupported) {\n");
    for line in rest {
        script.push_str("  ");
        script.push_str(line);
        script.push('\n');
    }
    script.push_str("}\n");
    let mut hasher = DefaultHasher::new();
    script.hash(&mut hasher);
    let name = format!("start-{:016x}.js", hasher.finish());
    let tag = format!("<script type=\"module\" src=\"./{name}\"></script>");
    let page = format!(
        "{}{tag}{}",
        &html[..start],
        &html[body_start + body_len + CLOSE.len()..]
    );
    Ok((page, name, script))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "<head>\n<script src=\"check.js\"></script>\n<script type=\"module\">\nimport init, * as bindings from './app-1.js';\nconst wasm = await init({ module_or_path: './app-1_bg.wasm' });\n\nwindow.wasmBindings = bindings;\n</script>\n</head>";

    /// REQ-019-12: no inline script is left for the content security policy to allow.
    #[test]
    fn req_019_12_moves_the_inline_start_script_into_a_file() {
        let (page, name, script) = move_start_script(PAGE).unwrap();
        assert!(name.starts_with("start-") && name.ends_with(".js"));
        assert!(page.contains(&format!("<script type=\"module\" src=\"./{name}\"></script>")));
        assert!(!page.contains("import init"));
        assert!(page.contains("<script src=\"check.js\"></script>"));
        assert_eq!(
            script,
            "import init, * as bindings from './app-1.js';\nif (!window.goodGalleryUnsupported) {\n  const wasm = await init({ module_or_path: './app-1_bg.wasm' });\n  window.wasmBindings = bindings;\n}\n"
        );
    }

    #[test]
    fn a_page_without_a_start_script_is_an_error() {
        assert!(move_start_script("<head></head>").is_err());
    }
}
