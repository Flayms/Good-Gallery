//! Repository tasks, run with `cargo xtask <task>`.

mod check_specs;
mod libraries;

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
        _ => {
            eprintln!("usage: cargo xtask check-specs [--summary] | libraries …");
            ExitCode::FAILURE
        }
    }
}
