//! Repository tasks, run with `cargo xtask <task>`.

mod check_specs;
mod gallery_bench;
mod libraries;
mod oracle;
mod oracle_search;
mod oracle_thumbs;
mod search_bench;
mod thumbs;

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check-specs") => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
            let code = check_specs::run(&root, &args[1..], &mut |line| println!("{line}"));
            ExitCode::from(code as u8)
        }
        Some("libraries") => match libraries::run(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error:#}");
                ExitCode::FAILURE
            }
        },
        Some("oracle") => match oracle::run(&args[1..]) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("error: {error:#}");
                ExitCode::FAILURE
            }
        },
        Some("gallery-bench") => match gallery_bench::run(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error:#}");
                ExitCode::FAILURE
            }
        },
        Some("oracle-search") => match args.get(1..=2) {
            Some([flag, index]) if flag == "--index" => {
                let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
                let reference = root.join("..").join("good-gallery");
                match std::path::absolute(&reference)
                    .map_err(anyhow::Error::from)
                    .and_then(|reference| oracle_search::run_on_copy(&reference, Path::new(index)))
                {
                    Ok(true) => ExitCode::SUCCESS,
                    Ok(false) => ExitCode::FAILURE,
                    Err(error) => {
                        eprintln!("error: {error:#}");
                        ExitCode::FAILURE
                    }
                }
            }
            _ => {
                eprintln!("usage: cargo xtask oracle-search --index <reference index of the fixture library>");
                ExitCode::FAILURE
            }
        },
        Some("search-bench") => match search_bench::run(&args[1..]) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("error: {error:#}");
                ExitCode::FAILURE
            }
        },
        Some("thumbs") => match thumbs::run(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error:#}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!(
                "usage: cargo xtask check-specs [--summary] | libraries … | oracle … | thumbs … | oracle-search … | gallery-bench … | search-bench …"
            );
            ExitCode::FAILURE
        }
    }
}
