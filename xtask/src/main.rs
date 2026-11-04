//! Repository tasks, run with `cargo xtask <task>`.

// Rebuilt when another worktree built into the same target folder (`.cargo/config.toml`).
const _: Option<&str> = option_env!("GOOD_GALLERY_WORKTREE");

mod check_specs;
mod dist;
mod face_bench;
mod gallery_bench;
mod icons;
mod libraries;
mod process;
mod release;
mod screenshots;
mod search_bench;
mod thumbs;
mod web;

use std::path::Path;
use std::process::ExitCode;

/// The exit code of a task that returns `()`, with its error printed.
fn report(result: anyhow::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check-specs") => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
            let code = check_specs::run(&root, &args[1..], &mut |line| println!("{line}"));
            ExitCode::from(code as u8)
        }
        Some("libraries") => report(libraries::run(&args[1..])),
        Some("face-bench") => report(face_bench::run(&args[1..])),
        Some("gallery-bench") => report(gallery_bench::run(&args[1..])),
        Some("icons") => report(icons::run(&args[1..])),
        Some("dist") => report(dist::run(&args[1..])),
        Some("screenshots") => report(screenshots::run(&args[1..])),
        Some("release") => ExitCode::from(release::run(&args[1..])),
        Some("release-notes") => ExitCode::from(release::run_notes(&args[1..])),
        Some("search-bench") => match search_bench::run(&args[1..]) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("error: {error:#}");
                ExitCode::FAILURE
            }
        },
        Some("thumbs") => report(thumbs::run(&args[1..])),
        Some("web") => report(web::run(&args[1..])),
        Some("web-test") => report(web::test(&args[1..])),
        _ => {
            eprintln!(
                "usage: cargo xtask check-specs [--summary] | libraries … | thumbs … | gallery-bench … | search-bench … | release … | release-notes <tag> … | dist … | screenshots … | icons [--lucide <dir>] | web [--debug] | web-test …"
            );
            ExitCode::FAILURE
        }
    }
}
