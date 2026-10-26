//! Checks that specs (`docs/specs/*/spec.md`) and tests reference each other consistently; see
//! `docs/specs/012-spec-consistency`. Run with `cargo xtask check-specs` (`--summary` for per-spec counts).

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

/// A file to check, with a `/`-separated path relative to the repository root.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecSummary {
    pub spec: String,
    pub pass: usize,
    pub partial: usize,
    pub no_test: usize,
}

#[derive(Debug, Default)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub summary: Vec<SpecSummary>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Pass,
    Partial,
    NoTest,
}

/// `REQ-012-01` in comments, `req_012_01` at the start of test names.
static REFERENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bREQ-(\d{3})-(\d{2})\b|\breq_(\d{3})_(\d{2})(?:_|\b)").unwrap());
static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^###\s+(~~)?\s*(REQ-\d{3}-\d{2})").unwrap());
static STATUS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\*\*Status:\*\*\s*(\w+)").unwrap());
static SPEC_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|/)((\d{3})-[^/]+)/spec\.md$").unwrap());
/// `mod tests {`, also `pub mod` / `pub(crate) mod`.
static TEST_MODULE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{$").unwrap());
static ROW_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^REQ-\d{3}-\d{2}$").unwrap());

struct Req {
    id: String,
    deprecated: bool,
}

struct Row {
    id: String,
    result: String,
}

struct ParsedSpec<'a> {
    path: &'a str,
    name: String,
    number: String,
    implemented: bool,
    reqs: Vec<Req>,
    rows: Vec<Row>,
}

fn parse_spec(file: &SourceFile) -> Option<ParsedSpec<'_>> {
    let captures = SPEC_PATH.captures(&file.path)?;
    let reqs = HEADING
        .captures_iter(&file.text)
        .map(|c| Req {
            id: c[2].to_string(),
            deprecated: c.get(1).is_some(),
        })
        .collect();
    let rows = file
        .text
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.trim().split('|').map(str::trim).collect();
            let cells = cells.get(1..cells.len().saturating_sub(1))?;
            let id = cells.first()?.replace("~~", "");
            let id = id.trim();
            ROW_ID.is_match(id).then(|| Row {
                id: id.to_string(),
                result: cells.get(2).unwrap_or(&"").to_string(),
            })
        })
        .collect();
    Some(ParsedSpec {
        path: &file.path,
        name: captures[1].to_string(),
        number: captures[2].to_string(),
        implemented: STATUS.captures(&file.text).is_some_and(|c| &c[1] == "Implemented"),
        reqs,
        rows,
    })
}

fn outcome_of(value: &str) -> Option<Outcome> {
    [
        ("pass", Outcome::Pass),
        ("partial", Outcome::Partial),
        ("no test", Outcome::NoTest),
    ]
    .into_iter()
    .find(|(name, _)| {
        value
            .strip_prefix(name)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
    })
    .map(|(_, outcome)| outcome)
}

/// The REQ IDs a test text mentions, in order of first mention, normalized to `REQ-NNN-XX`.
fn references_in(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    REFERENCE
        .captures_iter(text)
        .map(|c| {
            let (spec, n) = match (c.get(1), c.get(2)) {
                (Some(spec), Some(n)) => (spec.as_str(), n.as_str()),
                _ => (&c[3], &c[4]),
            };
            format!("REQ-{spec}-{n}")
        })
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

/// Checks `specs` against `tests`, whose texts are only the test parts of their files (see [`test_region`]).
pub fn check_specs(specs: &[SourceFile], tests: &[SourceFile]) -> Report {
    let mut report = Report::default();
    let specs: Vec<ParsedSpec> = specs.iter().filter_map(parse_spec).collect();

    // REQ id → test files mentioning it, in order of first mention.
    let mut references: Vec<(String, Vec<&str>)> = Vec::new();
    let mut reference_index: HashMap<String, usize> = HashMap::new();
    for test in tests {
        for id in references_in(&test.text) {
            let index = *reference_index.entry(id.clone()).or_insert_with(|| {
                references.push((id, Vec::new()));
                references.len() - 1
            });
            references[index].1.push(&test.path);
        }
    }

    // REQ id → defining spec and deprecation.
    let mut defined: HashMap<&str, (&str, bool)> = HashMap::new();
    for spec in &specs {
        for req in &spec.reqs {
            let number = &req.id[4..7];
            if number != spec.number {
                report
                    .errors
                    .push(format!("{}: {} belongs in spec {number}", spec.path, req.id));
            }
            match defined.get(req.id.as_str()) {
                Some((earlier, _)) => report
                    .errors
                    .push(format!("{}: {} is also defined in {earlier}", spec.path, req.id)),
                None => {
                    defined.insert(&req.id, (spec.path, req.deprecated));
                }
            }
        }
    }

    for spec in &specs {
        let mut counts = SpecSummary {
            spec: spec.name.clone(),
            pass: 0,
            partial: 0,
            no_test: 0,
        };
        for req in &spec.reqs {
            let rows: Vec<&Row> = spec.rows.iter().filter(|row| row.id == req.id).collect();
            match rows.len() {
                0 => report
                    .errors
                    .push(format!("{}: {} has no verification row", spec.path, req.id)),
                1 => {}
                n => report
                    .errors
                    .push(format!("{}: {} has {n} verification rows", spec.path, req.id)),
            }
            let Some(row) = rows.first() else { continue };
            if req.deprecated {
                continue;
            }
            let tested_by = reference_index.get(&req.id).map_or(&[][..], |&i| &references[i].1[..]);
            match outcome_of(&row.result) {
                None => report.errors.push(format!(
                    "{}: {} has an unknown result \"{}\" (pass, partial or no test)",
                    spec.path, req.id, row.result
                )),
                Some(Outcome::NoTest) => {
                    counts.no_test += 1;
                    if let Some(first) = tested_by.first() {
                        report.errors.push(format!(
                            "{}: {} is marked no test, but {first} references it",
                            spec.path, req.id
                        ));
                    } else if spec.implemented {
                        report.warnings.push(format!("{}: {} has no test", spec.path, req.id));
                    }
                }
                Some(outcome) => {
                    let name = if outcome == Outcome::Pass {
                        counts.pass += 1;
                        "pass"
                    } else {
                        counts.partial += 1;
                        "partial"
                    };
                    if tested_by.is_empty() {
                        report.errors.push(format!(
                            "{}: {} is marked {name}, but no test references it",
                            spec.path, req.id
                        ));
                    }
                }
            }
        }
        let mut seen = HashSet::new();
        for row in &spec.rows {
            if seen.insert(&row.id) && !spec.reqs.iter().any(|req| req.id == row.id) {
                report
                    .errors
                    .push(format!("{}: verification row for undefined {}", spec.path, row.id));
            }
        }
        report.summary.push(counts);
    }

    for (id, paths) in &references {
        for path in paths {
            match defined.get(id.as_str()) {
                None => report.errors.push(format!("{path}: {id} is not defined in any spec")),
                Some((_, true)) => report.errors.push(format!("{path}: {id} is deprecated")),
                Some(_) => {}
            }
        }
    }

    report
}

/// The part of a Rust file that holds tests: everything from the first inline test module (a line `#[cfg(test)]`
/// followed by `mod … {`) on, or the whole file if it is below a `tests/` or `benches/` folder or is a test module of
/// its own (`tests.rs`). `None` if the file has no tests.
pub fn test_region<'a>(path: &str, text: &'a str) -> Option<&'a str> {
    let mut parts = path.split('/');
    let file_name = parts.next_back().unwrap_or_default();
    if file_name == "tests.rs" || parts.any(|part| part == "tests" || part == "benches") {
        return Some(text);
    }
    let mut offset = 0;
    let mut lines = text.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "#[cfg(test)]" {
            let next = lines.peek().map_or("", |next| next.trim());
            if TEST_MODULE.is_match(next) {
                return Some(&text[offset..]);
            }
        }
        offset += line.len();
    }
    None
}

/// Files below `root/dir` (recursively, skipping `target` folders) that `include` accepts, sorted by path.
fn list_files(root: &Path, dir: &str, include: &dyn Fn(&str) -> bool) -> Vec<SourceFile> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            // Links aren't followed: a link loop would recurse forever.
            let Ok(file_type) = entry.file_type() else { continue };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if entry.file_name() != "target" {
                    walk(root, &path, out);
                }
            } else if let Ok(relative) = path.strip_prefix(root) {
                let parts: Vec<_> = relative.components().map(|c| c.as_os_str().to_string_lossy()).collect();
                out.push(parts.join("/"));
            }
        }
    }
    let mut paths = Vec::new();
    walk(root, &root.join(dir), &mut paths);
    paths.retain(|path| include(path));
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(root.join(&path)).ok()?;
            Some(SourceFile { path, text })
        })
        .collect()
}

/// Checks the repository at `root`; returns the exit code.
pub fn run(root: &Path, args: &[String], log: &mut dyn FnMut(&str)) -> i32 {
    let specs = list_files(root, "docs/specs", &|path| SPEC_PATH.is_match(path));
    let tests: Vec<SourceFile> = ["crates", "xtask"]
        .into_iter()
        .flat_map(|dir| list_files(root, dir, &|path| path.ends_with(".rs")))
        .filter_map(|file| {
            let text = test_region(&file.path, &file.text)?.to_string();
            Some(SourceFile { path: file.path, text })
        })
        .collect();
    let report = check_specs(&specs, &tests);

    for error in &report.errors {
        log(&format!("error: {error}"));
    }
    for warning in &report.warnings {
        log(&format!("warning: {warning}"));
    }
    if args.iter().any(|arg| arg == "--summary") {
        for s in &report.summary {
            log(&format!(
                "{}: {} pass, {} partial, {} no test",
                s.spec, s.pass, s.partial, s.no_test
            ));
        }
    }
    log(&format!(
        "{} specs: {} errors, {} warnings",
        specs.len(),
        report.errors.len(),
        report.warnings.len()
    ));
    i32::from(!report.errors.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // IDs are built, never written out: this file is a test file too, and literal IDs would count as references.
    fn req(spec: u32, n: u32) -> String {
        format!("{}-{spec:03}-{n:02}", "REQ")
    }

    #[derive(Clone)]
    struct FixtureReq {
        id: String,
        result: &'static str,
        deprecated: bool,
    }

    fn ok(id: String) -> FixtureReq {
        FixtureReq {
            id,
            result: "pass",
            deprecated: false,
        }
    }

    fn with(id: String, result: &'static str) -> FixtureReq {
        FixtureReq { result, ..ok(id) }
    }

    fn deprecated(id: String, result: &'static str) -> FixtureReq {
        FixtureReq {
            deprecated: true,
            ..with(id, result)
        }
    }

    fn spec_text(reqs: &[FixtureReq], status: &str, rows: Option<&[FixtureReq]>) -> String {
        let mut lines = vec![
            "# Title".to_string(),
            format!("**Status:** {status}"),
            "## Requirements".into(),
        ];
        for r in reqs {
            lines.push(if r.deprecated {
                format!("### ~~{} · Old~~", r.id)
            } else {
                format!("### {} · Name", r.id)
            });
        }
        lines.extend(["## Verification", "| REQ | Test | Result |", "| --- | --- | --- |"].map(String::from));
        for r in rows.unwrap_or(reqs) {
            lines.push(format!("| {} | `crates/a/src/lib.rs` › name | {} |", r.id, r.result));
        }
        lines.join("\n")
    }

    fn implemented(reqs: &[FixtureReq]) -> String {
        spec_text(reqs, "Implemented", None)
    }

    fn spec(folder: &str, text: String) -> SourceFile {
        SourceFile {
            path: format!("docs/specs/{folder}/spec.md"),
            text,
        }
    }

    fn test_file(ids: &[String]) -> SourceFile {
        let text = ids
            .iter()
            .map(|id| format!("/// {id}: works\n#[test]\nfn works() {{}}"))
            .collect::<Vec<_>>()
            .join("\n");
        SourceFile {
            path: "crates/a/src/lib.rs".into(),
            text,
        }
    }

    const SPEC_A: &str = "docs/specs/001-a/spec.md";

    #[test]
    fn accepts_consistent_specs_and_tests() {
        let report = check_specs(
            &[spec("001-a", implemented(&[ok(req(1, 1))]))],
            &[test_file(&[req(1, 1)])],
        );

        assert_eq!(report.errors, Vec::<String>::new());
        assert_eq!(report.warnings, Vec::<String>::new());
    }

    #[test]
    fn req_012_01_reports_test_references_to_unknown_reqs() {
        let report = check_specs(
            &[spec("001-a", implemented(&[ok(req(1, 1))]))],
            &[test_file(&[req(1, 1), req(1, 41)])],
        );

        assert_eq!(
            report.errors,
            [format!(
                "crates/a/src/lib.rs: {} is not defined in any spec",
                req(1, 41)
            )]
        );
    }

    #[test]
    fn req_012_01_counts_test_names_as_references() {
        let name = format!("fn {}_works() {{}}", req(1, 41).to_lowercase().replace('-', "_"));
        let tests = [SourceFile {
            path: "crates/a/src/lib.rs".into(),
            text: name,
        }];

        let report = check_specs(&[spec("001-a", implemented(&[with(req(1, 1), "no test")]))], &tests);

        assert_eq!(
            report.errors,
            [format!(
                "crates/a/src/lib.rs: {} is not defined in any spec",
                req(1, 41)
            )]
        );
    }

    #[test]
    fn req_012_02_reports_test_references_to_deprecated_reqs() {
        let text = implemented(&[ok(req(1, 1)), deprecated(req(1, 2), "pass")]);

        let report = check_specs(&[spec("001-a", text)], &[test_file(&[req(1, 1), req(1, 2)])]);

        assert_eq!(
            report.errors,
            [format!("crates/a/src/lib.rs: {} is deprecated", req(1, 2))]
        );
    }

    #[test]
    fn req_012_03_reports_missing_duplicate_and_unknown_verification_rows() {
        let rows = [ok(req(1, 1)), ok(req(1, 3)), ok(req(1, 3)), ok(req(1, 9))];
        let text = spec_text(
            &[ok(req(1, 1)), ok(req(1, 2)), ok(req(1, 3))],
            "Implemented",
            Some(&rows),
        );

        let report = check_specs(&[spec("001-a", text)], &[test_file(&[req(1, 1), req(1, 2), req(1, 3)])]);

        assert_eq!(
            report.errors,
            [
                format!("{SPEC_A}: {} has no verification row", req(1, 2)),
                format!("{SPEC_A}: {} has 2 verification rows", req(1, 3)),
                format!("{SPEC_A}: verification row for undefined {}", req(1, 9)),
            ]
        );
    }

    #[test]
    fn req_012_04_reports_results_that_dont_match_the_tests() {
        let text = implemented(&[
            with(req(1, 1), "pass"),
            with(req(1, 2), "partial: only X"),
            with(req(1, 3), "no test"),
            with(req(1, 4), "maybe"),
            deprecated(req(1, 5), "no test"),
        ]);

        let report = check_specs(&[spec("001-a", text)], &[test_file(&[req(1, 3), req(1, 4)])]);

        assert_eq!(
            report.errors,
            [
                format!("{SPEC_A}: {} is marked pass, but no test references it", req(1, 1)),
                format!("{SPEC_A}: {} is marked partial, but no test references it", req(1, 2)),
                format!(
                    "{SPEC_A}: {} is marked no test, but crates/a/src/lib.rs references it",
                    req(1, 3)
                ),
                format!(
                    "{SPEC_A}: {} has an unknown result \"maybe\" (pass, partial or no test)",
                    req(1, 4)
                ),
            ]
        );
    }

    #[test]
    fn req_012_05_reports_duplicate_reqs_and_reqs_in_the_wrong_spec() {
        let report = check_specs(
            &[
                spec("001-a", implemented(&[ok(req(1, 1)), ok(req(2, 1))])),
                spec("003-c", implemented(&[ok(req(1, 1)), ok(req(3, 1))])),
            ],
            &[test_file(&[req(1, 1), req(2, 1), req(3, 1)])],
        );

        assert_eq!(
            report.errors,
            [
                format!("{SPEC_A}: {} belongs in spec 002", req(2, 1)),
                format!("docs/specs/003-c/spec.md: {} belongs in spec 001", req(1, 1)),
                format!("docs/specs/003-c/spec.md: {} is also defined in {SPEC_A}", req(1, 1)),
            ]
        );
    }

    #[test]
    fn req_012_06_reports_untested_reqs_of_implemented_specs_as_warnings() {
        let report = check_specs(
            &[
                spec("001-a", implemented(&[with(req(1, 1), "no test")])),
                spec("002-b", spec_text(&[with(req(2, 1), "no test")], "Draft", None)),
            ],
            &[],
        );

        assert_eq!(report.errors, Vec::<String>::new());
        assert_eq!(report.warnings, [format!("{SPEC_A}: {} has no test", req(1, 1))]);
    }

    #[test]
    fn req_012_08_summarizes_results_per_spec() {
        let text = implemented(&[
            with(req(1, 1), "pass"),
            with(req(1, 2), "partial: x"),
            with(req(1, 3), "no test"),
            deprecated(req(1, 4), "pass"),
        ]);

        let report = check_specs(&[spec("001-a", text)], &[test_file(&[req(1, 1), req(1, 2)])]);

        assert_eq!(
            report.summary,
            [SpecSummary {
                spec: "001-a".into(),
                pass: 1,
                partial: 1,
                no_test: 1
            }]
        );
    }

    #[test]
    fn test_region_starts_at_the_test_module_or_covers_test_files() {
        let text = "fn a() {}\n#[cfg(test)]\nmod tests {\n}";

        assert_eq!(
            test_region("crates/a/src/lib.rs", text),
            Some("#[cfg(test)]\nmod tests {\n}")
        );
        assert_eq!(test_region("crates/a/src/lib.rs", "fn a() {}"), None);
        assert_eq!(test_region("crates/a/tests/it.rs", "fn a() {}"), Some("fn a() {}"));
        assert_eq!(test_region("crates/a/benches/b.rs", "fn a() {}"), Some("fn a() {}"));
        assert_eq!(test_region("crates/a/src/db/tests.rs", "fn a() {}"), Some("fn a() {}"));
        let visible = "fn a() {}
#[cfg(test)]
pub(crate) mod tests {
}";
        assert_eq!(
            test_region("crates/a/src/lib.rs", visible),
            Some(
                "#[cfg(test)]
pub(crate) mod tests {
}"
            )
        );
    }

    #[test]
    fn test_region_ignores_other_cfg_test_items_and_mentions() {
        let text = "/// Starts at `#[cfg(test)]`.\n#[cfg(test)]\nuse std::fs;\n#[cfg(test)]\nmod helpers;\nfn a() {}";

        assert_eq!(test_region("crates/a/src/lib.rs", text), None);
    }

    #[test]
    fn references_need_whole_ids() {
        let id = req(1, 1);
        let name = id.to_lowercase().replace('-', "_");

        assert_eq!(references_in(&format!("fn {name}_works() {{}}")), [id.as_str()]);
        assert_eq!(references_in(&format!("fn {name}() {{}}")), [id.as_str()]);
        assert_eq!(references_in(&format!("/// {id}: works")), [id.as_str()]);
        assert!(references_in(&format!("/// {id}1 X{id} fn snapshot_{name}()")).is_empty());
    }

    fn with_repo(files: &[(&str, String)], check: impl FnOnce(&Path)) {
        let root = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        check(root.path());
    }

    fn run_logged(root: &Path, args: &[&str]) -> (i32, Vec<String>) {
        let mut lines = Vec::new();
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let code = run(root, &args, &mut |line| lines.push(line.to_string()));
        (code, lines)
    }

    #[test]
    fn req_012_07_exits_non_zero_only_on_errors() {
        let mut files = vec![
            (
                "docs/specs/001-a/spec.md",
                implemented(&[ok(req(1, 1)), with(req(1, 2), "no test")]),
            ),
            ("docs/specs/README.md", format!("Template: {}", req(1, 99))),
            (
                "crates/x/src/lib.rs",
                format!("fn a() {{}}\n#[cfg(test)]\nmod tests {{\n/// {}: works\n}}", req(1, 1)),
            ),
            ("crates/x/src/plain.rs", format!("// {}", req(1, 2))),
            ("crates/x/target/debug/gen.rs", format!("// {}", req(1, 8))),
            ("xtask/src/main.rs", "fn main() {}".into()),
        ];
        with_repo(&files, |root| {
            let (code, lines) = run_logged(root, &[]);
            assert_eq!(code, 0, "{lines:?}");
            assert!(lines.contains(&format!("warning: {SPEC_A}: {} has no test", req(1, 2))));
        });

        files.push(("crates/x/tests/smoke.rs", format!("/// {}: x", req(1, 7))));
        with_repo(&files, |root| {
            let (code, lines) = run_logged(root, &[]);
            assert_eq!(code, 1);
            assert!(lines.contains(&format!(
                "error: crates/x/tests/smoke.rs: {} is not defined in any spec",
                req(1, 7)
            )));
        });
    }

    #[test]
    fn req_012_08_prints_the_summary_on_request() {
        let files = [("docs/specs/001-a/spec.md", implemented(&[with(req(1, 1), "no test")]))];
        with_repo(&files, |root| {
            let (_, lines) = run_logged(root, &["--summary"]);
            assert!(
                lines.contains(&"001-a: 0 pass, 0 partial, 1 no test".to_string()),
                "{lines:?}"
            );
        });
    }
}
