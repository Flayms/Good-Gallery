//! `cargo xtask release [major|minor|patch|1.2.3] [--dry-run] [--yes]`: cuts a release (docs/specs/014-ci-release).
//! Bumps the version from the conventional commits since the last tag, regenerates `CHANGELOG.md`, commits, tags and
//! pushes. The pushed tag starts `.github/workflows/release.yml`, which builds the installer and publishes the GitHub
//! release. git-cliff (`GIT_CLIFF` or on `PATH`) computes the version and writes the changelog from `cliff.toml`.

use std::path::{Path, PathBuf};
use std::process::Command;

const BRANCH: &str = "main";
const REMOTE: &str = "origin";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bump {
    Major,
    Minor,
    Patch,
}

impl Bump {
    fn parse(arg: &str) -> Option<Self> {
        match arg {
            "major" => Some(Self::Major),
            "minor" => Some(Self::Minor),
            "patch" => Some(Self::Patch),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Patch => "patch",
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Overrides the bump derived from the commits.
    pub bump: Option<Bump>,
    /// Releases exactly this version (without `v`).
    pub version: Option<String>,
    pub dry_run: bool,
    pub yes: bool,
}

/// A failure the user can fix; reported without a backtrace, one line per problem.
#[derive(Debug, PartialEq, Eq)]
pub struct ReleaseError(pub String);

impl std::fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReleaseError {}

fn error(message: impl Into<String>) -> ReleaseError {
    ReleaseError(message.into())
}

/// `1.2.3`: three numbers, nothing else.
fn is_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

pub fn parse_args(args: &[String]) -> Result<Options, ReleaseError> {
    let mut options = Options::default();
    for arg in args {
        let version = arg.strip_prefix('v').unwrap_or(arg);
        match arg.as_str() {
            "--" => {}
            "--dry-run" => options.dry_run = true,
            "--yes" | "-y" => options.yes = true,
            _ if Bump::parse(arg).is_some() => options.bump = Bump::parse(arg),
            _ if is_version(version) => options.version = Some(version.to_owned()),
            _ => {
                return Err(error(format!(
                    "Unknown argument \"{arg}\". Use major, minor, patch, a version like 1.2.3, --dry-run or --yes."
                )));
            }
        }
    }
    if options.bump.is_some() && options.version.is_some() {
        return Err(error("Pass either a bump or a version, not both."));
    }
    Ok(options)
}

pub struct RepoState<'a> {
    pub branch: &'a str,
    /// Output of `git status --porcelain`.
    pub status: &'a str,
    /// Commits on the remote branch that aren't checked out.
    pub behind: u32,
}

pub fn preflight_errors(state: &RepoState) -> Vec<String> {
    let mut errors = Vec::new();
    if state.branch != BRANCH {
        let branch = if state.branch.is_empty() {
            "no branch"
        } else {
            state.branch
        };
        errors.push(format!("Releases are cut from {BRANCH}, but {branch} is checked out."));
    }
    if !state.status.trim().is_empty() {
        errors.push("The working tree has uncommitted changes. Commit or stash them first.".to_owned());
    }
    if state.behind > 0 {
        errors.push(format!(
            "{BRANCH} is {} commit(s) behind {REMOTE}/{BRANCH}. Pull first.",
            state.behind
        ));
    }
    errors
}

pub fn version_errors(next: &str, latest_tag: Option<&str>, tags: &[String]) -> Vec<String> {
    let tag = format!("v{next}");
    if !is_version(next) {
        vec![format!("\"{next}\" isn't a version like 1.2.3.")]
    } else if latest_tag == Some(tag.as_str()) {
        vec![format!("Nothing to release: no new commits since {tag}.")]
    } else if tags.contains(&tag) {
        vec![format!("The tag {tag} already exists.")]
    } else {
        Vec::new()
    }
}

/// `cargo_toml` with `version` in its `[workspace.package]` table, everything else untouched.
pub fn with_version(cargo_toml: &str, version: &str) -> Result<String, ReleaseError> {
    let mut out = String::with_capacity(cargo_toml.len());
    let mut in_table = false;
    let mut replaced = false;
    for line in cargo_toml.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_table = trimmed == "[workspace.package]";
        } else if in_table && !replaced {
            let key = trimmed.split('=').next().unwrap_or_default().trim();
            if key == "version" {
                let ending = &line[line.trim_end().len()..];
                out.push_str(&format!("version = \"{version}\"{ending}"));
                replaced = true;
                continue;
            }
        }
        out.push_str(line);
    }
    if replaced {
        Ok(out)
    } else {
        Err(error("Cargo.toml has no version in [workspace.package]."))
    }
}

fn exec(cwd: &Path, program: &Path, args: &[&str]) -> Result<String, ReleaseError> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| error(format!("can't run {}: {e}", program.display())))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        return Err(error(match (stderr.is_empty(), stdout.is_empty()) {
            (false, _) => stderr,
            (true, false) => stdout,
            (true, true) => format!("{} {} failed ({})", program.display(), args.join(" "), output.status),
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn git(cwd: &Path, args: &[&str]) -> Result<String, ReleaseError> {
    exec(cwd, Path::new("git"), args)
}

/// git-cliff: `GIT_CLIFF`, else on `PATH`.
pub fn find_git_cliff() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("GIT_CLIFF") {
        return Some(path.into());
    }
    let name = if cfg!(windows) { "git-cliff.exe" } else { "git-cliff" };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// The programs a release runs besides git.
pub struct Tools {
    pub git_cliff: PathBuf,
    /// `cliff.toml`.
    pub config: PathBuf,
    pub cargo: PathBuf,
}

impl Tools {
    /// git-cliff with this repository's `cliff.toml`, and the cargo that runs xtask.
    pub fn find() -> Result<Self, ReleaseError> {
        let git_cliff = find_git_cliff()
            .ok_or_else(|| error("git-cliff wasn't found. Install it (cargo install git-cliff) or set GIT_CLIFF."))?;
        Ok(Self {
            git_cliff,
            config: Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("cliff.toml"),
            cargo: std::env::var_os("CARGO").map_or_else(|| PathBuf::from("cargo"), PathBuf::from),
        })
    }

    fn cliff(&self, cwd: &Path, args: &[&str]) -> Result<String, ReleaseError> {
        let config = self.config.to_string_lossy();
        let mut all = vec!["--config", config.as_ref()];
        all.extend_from_slice(args);
        exec(cwd, &self.git_cliff, &all)
    }
}

pub fn next_version(tools: &Tools, cwd: &Path, bump: Option<Bump>) -> Result<String, ReleaseError> {
    let mut args = vec!["--bumped-version"];
    if let Some(bump) = bump {
        args.extend(["--bump", bump.as_str()]);
    }
    let version = tools.cliff(cwd, &args)?;
    Ok(version.strip_prefix('v').unwrap_or(&version).to_owned())
}

/// Asks on the terminal; anything but y / yes is a no.
pub fn confirm(question: &str) -> bool {
    use std::io::Write as _;
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

pub fn release(
    cwd: &Path,
    args: &[String],
    tools: &Tools,
    log: &mut dyn FnMut(&str),
    ask: &mut dyn FnMut(&str) -> bool,
) -> Result<(), ReleaseError> {
    let options = parse_args(args)?;

    git(cwd, &["fetch", "--quiet", "--tags", REMOTE, BRANCH])?;
    let branch = git(cwd, &["branch", "--show-current"])?;
    let status = git(cwd, &["status", "--porcelain"])?;
    let behind = git(cwd, &["rev-list", "--count", &format!("HEAD..{REMOTE}/{BRANCH}")])?;
    let errors = preflight_errors(&RepoState {
        branch: &branch,
        status: &status,
        behind: behind.parse().unwrap_or(0),
    });
    if !errors.is_empty() {
        return Err(error(errors.join("\n")));
    }

    let tags: Vec<String> = git(cwd, &["tag", "--list", "v*"])?
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    let latest_tag = if tags.is_empty() {
        None
    } else {
        Some(git(cwd, &["describe", "--tags", "--abbrev=0", "--match", "v*"])?)
    };
    let next = match options.version {
        Some(version) => version,
        None => next_version(tools, cwd, options.bump)?,
    };
    let problems = version_errors(&next, latest_tag.as_deref(), &tags);
    if !problems.is_empty() {
        return Err(error(problems.join("\n")));
    }
    let tag = format!("v{next}");

    log(&format!(
        "\n{} → {tag}\n",
        latest_tag.as_deref().unwrap_or("First release")
    ));
    log(&tools.cliff(cwd, &["--unreleased", "--tag", &tag, "--strip", "all"])?);
    log("");

    if options.dry_run {
        log("Dry run: nothing was changed.");
        return Ok(());
    }
    if !options.yes && !ask(&format!("Commit, tag and push {tag}?")) {
        log("Cancelled.");
        return Ok(());
    }

    // A failure from here on (a rejected push, a missing tool) undoes everything, so the next run starts clean.
    let start = git(cwd, &["rev-parse", "HEAD"])?;
    let had_changelog = cwd.join("CHANGELOG.md").exists();
    if let Err(problem) = commit_tag_push(cwd, tools, &next, &tag) {
        let _ = git(cwd, &["tag", "--delete", &tag]);
        let _ = git(cwd, &["reset", "--quiet", "--hard", &start]);
        if !had_changelog {
            let _ = std::fs::remove_file(cwd.join("CHANGELOG.md"));
        }
        return Err(error(format!(
            "{problem}\nNothing was released; the local changes were undone."
        )));
    }

    let url = git(cwd, &["remote", "get-url", REMOTE])?;
    let url = url.strip_suffix(".git").unwrap_or(&url);
    let url = url
        .strip_prefix("git@github.com:")
        .map_or_else(|| url.to_owned(), |path| format!("https://github.com/{path}"));
    log(&format!(
        "Pushed {tag}. The release workflow builds the installer and publishes the release:\n  {url}/actions"
    ));
    Ok(())
}

/// Sets the version, writes the changelog, commits, tags and pushes.
fn commit_tag_push(cwd: &Path, tools: &Tools, next: &str, tag: &str) -> Result<(), ReleaseError> {
    let manifest = cwd.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|e| error(format!("can't read Cargo.toml: {e}")))?;
    std::fs::write(&manifest, with_version(&text, next)?).map_err(|e| error(format!("can't write Cargo.toml: {e}")))?;
    let mut files = vec!["Cargo.toml", "CHANGELOG.md"];
    if cwd.join("Cargo.lock").is_file() {
        // The workspace crates' versions are in the lock file too; a stale one fails `--locked` builds.
        exec(cwd, &tools.cargo, &["update", "--workspace", "--offline", "--quiet"])?;
        files.push("Cargo.lock");
    }
    tools.cliff(cwd, &["--tag", tag, "--output", "CHANGELOG.md"])?;

    let mut add = vec!["add"];
    add.extend(&files);
    git(cwd, &add)?;
    git(cwd, &["commit", "--quiet", "-m", &format!("chore(release): {tag}")])?;
    git(cwd, &["tag", "--annotate", tag, "-m", tag])?;
    git(cwd, &["push", "--quiet", "--atomic", REMOTE, BRANCH, tag])?;
    Ok(())
}

/// `cargo xtask release …` on this repository; the exit code.
pub fn run(args: &[String]) -> u8 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let result =
        Tools::find().and_then(|tools| release(&root, args, &tools, &mut |line| println!("{line}"), &mut confirm));
    match result {
        Ok(()) => 0,
        Err(ReleaseError(message)) => {
            for line in message.lines() {
                eprintln!("✖ {line}");
            }
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    // Runs against a throwaway repository with a bare `origin` and a tiny Cargo workspace, so nothing touches this
    // repository.

    struct Repo {
        _dir: tempfile::TempDir,
        repo: PathBuf,
        remote: PathBuf,
        tools: Tools,
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    impl Repo {
        /// `None` (test skipped) without git-cliff, unless `GG_REQUIRE_HELPERS` or `CI` is set.
        fn new() -> Option<Self> {
            let tools = match Tools::find() {
                Ok(tools) => tools,
                Err(_) if std::env::var_os("GG_REQUIRE_HELPERS").is_some() || std::env::var_os("CI").is_some() => {
                    panic!("git-cliff not found")
                }
                Err(_) => {
                    eprintln!("skipped: git-cliff not found");
                    return None;
                }
            };
            let dir = tempfile::tempdir().unwrap();
            let remote = dir.path().join("remote.git");
            let repo = dir.path().join("repo");
            git(
                dir.path(),
                &["init", "--quiet", "--bare", "--initial-branch=main", "remote.git"],
            )
            .unwrap();
            git(dir.path(), &["init", "--quiet", "--initial-branch=main", "repo"]).unwrap();
            for (key, value) in [
                ("user.name", "Test"),
                ("user.email", "test@example.com"),
                ("commit.gpgsign", "false"),
                ("tag.gpgsign", "false"),
                ("core.autocrlf", "false"),
            ] {
                git(&repo, &["config", key, value]).unwrap();
            }
            git(&repo, &["remote", "add", "origin", remote.to_str().unwrap()]).unwrap();
            std::fs::write(
                repo.join("Cargo.toml"),
                "[workspace]\nmembers = [\"app\"]\nresolver = \"3\"\n\n[workspace.package]\nversion = \"0.0.0\"\nedition = \"2024\"\n",
            )
            .unwrap();
            std::fs::create_dir_all(repo.join("app/src")).unwrap();
            std::fs::write(
                repo.join("app/Cargo.toml"),
                "[package]\nname = \"app\"\nversion.workspace = true\nedition.workspace = true\n",
            )
            .unwrap();
            std::fs::write(repo.join("app/src/main.rs"), "fn main() {}\n").unwrap();
            exec(&repo, &tools.cargo, &["generate-lockfile", "--offline", "--quiet"]).unwrap();
            let this = Self {
                _dir: dir,
                repo,
                remote,
                tools,
            };
            this.commit("chore: scaffold");
            git(&this.repo, &["push", "--quiet", "-u", "origin", "main"]).unwrap();
            Some(this)
        }

        fn commit(&self, message: &str) {
            {
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.repo.join("file.txt"))
                    .unwrap();
                writeln!(file, "{message}").unwrap();
            }
            git(&self.repo, &["add", "."]).unwrap();
            git(&self.repo, &["commit", "--quiet", "-m", message]).unwrap();
        }

        fn tag(&self, tag: &str) {
            git(&self.repo, &["tag", tag]).unwrap();
        }

        fn next(&self, bump: Option<Bump>) -> String {
            next_version(&self.tools, &self.repo, bump).unwrap()
        }

        fn run_with(&self, args: &[&str], answer: bool) -> Result<String, ReleaseError> {
            let mut lines = Vec::new();
            release(
                &self.repo,
                &strings(args),
                &self.tools,
                &mut |line| lines.push(line.to_owned()),
                &mut |_| answer,
            )?;
            Ok(lines.join("\n"))
        }

        fn run(&self, args: &[&str]) -> Result<String, ReleaseError> {
            self.run_with(args, true)
        }

        fn remote_tags(&self) -> Vec<String> {
            git(&self.remote, &["tag", "--list"])
                .unwrap()
                .lines()
                .map(str::to_owned)
                .collect()
        }
    }

    #[test]
    fn req_014_03_derives_the_next_version_from_the_commits() {
        let Some(repo) = Repo::new() else { return };
        assert_eq!(repo.next(None), "0.1.0");

        repo.tag("v0.1.0");
        repo.commit("fix: crash");
        assert_eq!(repo.next(None), "0.1.1");
        repo.commit("feat: search");
        assert_eq!(repo.next(None), "0.2.0");
        repo.commit("feat!: new index format");
        assert_eq!(repo.next(None), "0.2.0");

        repo.tag("v1.0.0");
        repo.commit("docs: readme");
        assert_eq!(repo.next(None), "1.0.1");
        repo.commit("refactor: indexer\n\nBREAKING CHANGE: drops the old cache");
        assert_eq!(repo.next(None), "2.0.0");
    }

    #[test]
    fn req_014_04_parses_overrides() {
        assert_eq!(parse_args(&[]), Ok(Options::default()));
        assert_eq!(
            parse_args(&strings(&["--", "minor", "--dry-run", "-y"])),
            Ok(Options {
                bump: Some(Bump::Minor),
                version: None,
                dry_run: true,
                yes: true,
            })
        );
        assert_eq!(
            parse_args(&strings(&["v1.2.3"])).unwrap().version.as_deref(),
            Some("1.2.3")
        );
        assert!(
            parse_args(&strings(&["1.2"]))
                .unwrap_err()
                .0
                .contains("Unknown argument \"1.2\"")
        );
        assert!(
            parse_args(&strings(&["--force"]))
                .unwrap_err()
                .0
                .contains("Unknown argument")
        );
        assert!(
            parse_args(&strings(&["major", "1.2.3"]))
                .unwrap_err()
                .0
                .contains("either a bump or a version")
        );
    }

    #[test]
    fn req_014_04_overrides_the_bump_or_the_version() {
        let Some(repo) = Repo::new() else { return };
        repo.tag("v0.1.0");
        repo.commit("fix: crash");
        assert_eq!(repo.next(Some(Bump::Major)), "1.0.0");
        assert!(repo.run(&["minor", "--dry-run"]).unwrap().contains("v0.1.0 → v0.2.0"));
        assert!(repo.run(&["3.0.0", "--dry-run"]).unwrap().contains("v0.1.0 → v3.0.0"));
    }

    #[test]
    fn req_014_05_lists_failed_preconditions() {
        let clean = RepoState {
            branch: "main",
            status: "",
            behind: 0,
        };
        assert!(preflight_errors(&clean).is_empty());
        assert_eq!(
            preflight_errors(&RepoState {
                branch: "feature",
                ..clean
            }),
            ["Releases are cut from main, but feature is checked out."]
        );
        assert_eq!(
            preflight_errors(&RepoState {
                status: " M src/a.rs\n",
                ..clean
            })
            .len(),
            1
        );
        assert_eq!(
            preflight_errors(&RepoState { behind: 2, ..clean }),
            ["main is 2 commit(s) behind origin/main. Pull first."]
        );
        let tags = strings(&["v0.1.0"]);
        assert!(version_errors("0.2.0", Some("v0.1.0"), &tags).is_empty());
        assert_eq!(
            version_errors("0.1.0", Some("v0.1.0"), &tags),
            ["Nothing to release: no new commits since v0.1.0."]
        );
        assert_eq!(
            version_errors("0.1.0", Some("v0.2.0"), &strings(&["v0.1.0", "v0.2.0"])),
            ["The tag v0.1.0 already exists."]
        );
    }

    #[test]
    fn req_014_05_stops_when_a_precondition_fails() {
        let Some(repo) = Repo::new() else { return };
        std::fs::write(repo.repo.join("dirty.txt"), "x").unwrap();
        assert!(repo.run(&["--yes"]).unwrap_err().0.contains("uncommitted changes"));
        std::fs::remove_file(repo.repo.join("dirty.txt")).unwrap();

        repo.run(&["--yes"]).unwrap();
        assert!(repo.run(&["--yes"]).unwrap_err().0.contains("Nothing to release"));
        assert_eq!(repo.remote_tags(), ["v0.1.0"]);
    }

    #[test]
    fn req_014_05_undoes_a_release_whose_push_fails() {
        let Some(repo) = Repo::new() else { return };
        repo.commit("feat: search");
        std::fs::write(repo.remote.join("hooks/pre-receive"), "#!/bin/sh\nexit 1\n").unwrap();
        let start = git(&repo.repo, &["rev-parse", "HEAD"]).unwrap();

        let problem = repo.run(&["--yes"]).unwrap_err().0;
        assert!(problem.contains("Nothing was released"), "{problem}");
        assert_eq!(git(&repo.repo, &["rev-parse", "HEAD"]).unwrap(), start);
        assert_eq!(git(&repo.repo, &["tag", "--list"]).unwrap(), "");
        assert_eq!(git(&repo.repo, &["status", "--porcelain"]).unwrap(), "");
        assert!(!repo.repo.join("CHANGELOG.md").exists());

        std::fs::remove_file(repo.remote.join("hooks/pre-receive")).unwrap();
        assert!(repo.run(&["--yes"]).is_ok(), "a rerun releases");
        assert_eq!(repo.remote_tags(), ["v0.1.0"]);
    }

    #[test]
    fn req_014_06_writes_the_changelog() {
        let Some(repo) = Repo::new() else { return };
        repo.commit("feat(viewer): zoom");
        repo.run(&["--yes"]).unwrap();
        repo.commit("fix: crash on empty folder");
        repo.commit("feat!: drop the old cache");
        repo.commit("perf: faster scans");
        repo.commit("update stuff");
        repo.run(&["--yes"]).unwrap();

        let changelog = std::fs::read_to_string(repo.repo.join("CHANGELOG.md")).unwrap();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let latest = &changelog[changelog.find("## [0.2.0]").unwrap()..changelog.find("## [0.1.0]").unwrap()];
        assert!(changelog.starts_with("# Changelog"));
        assert!(latest.contains(&format!(
            "## [0.2.0](https://github.com/Flayms/Good-Gallery/releases/tag/v0.2.0) · {today}"
        )));
        let order = [
            "### ⚠️ Breaking changes",
            "### ✨ Features",
            "### 🐛 Bug fixes",
            "### ⚡ Performance",
        ];
        let positions: Vec<usize> = order.iter().map(|heading| latest.find(heading).unwrap()).collect();
        assert!(positions.is_sorted(), "{latest}");
        let crash = regex::Regex::new(
            r"- Crash on empty folder \(\[`[0-9a-f]{7}`\]\(https://github\.com/Flayms/Good-Gallery/commit/",
        )
        .unwrap();
        assert!(crash.is_match(latest), "{latest}");
        assert!(latest.contains("[v0.1.0...v0.2.0](https://github.com/Flayms/Good-Gallery/compare/v0.1.0...v0.2.0)"));
        assert!(changelog.contains("- **viewer:** Zoom"));
        assert!(!changelog.contains("update stuff"));
        assert!(!changelog.contains("chore(release)"));
    }

    #[test]
    fn req_014_07_sets_the_workspace_version() {
        let manifest = "[workspace]\nmembers = [\"a\"]\n\n[workspace.package]\nedition = \"2024\"\nversion = \"0.0.0\"\n\n[dependencies]\nversion = \"1\"\n";
        assert_eq!(
            with_version(manifest, "1.2.3").unwrap(),
            manifest.replace("version = \"0.0.0\"", "version = \"1.2.3\"")
        );
        assert!(with_version("[package]\nversion = \"0.0.0\"\n", "1.2.3").is_err());
    }

    #[test]
    fn req_014_07_commits_tags_and_pushes_the_release() {
        let Some(repo) = Repo::new() else { return };
        repo.commit("feat: search");

        assert!(
            repo.run(&["--dry-run"])
                .unwrap()
                .contains("Dry run: nothing was changed.")
        );
        assert_eq!(repo.run_with(&[], false).unwrap().lines().last(), Some("Cancelled."));
        assert!(repo.remote_tags().is_empty());
        assert_eq!(git(&repo.repo, &["status", "--porcelain"]).unwrap(), "");

        let output = repo.run(&[]).unwrap();
        assert!(output.contains("First release → v0.1.0"), "{output}");
        assert!(output.contains("- Search"), "{output}");
        assert_eq!(repo.remote_tags(), ["v0.1.0"]);
        let remote = &repo.remote;
        assert_eq!(
            git(remote, &["log", "-1", "--format=%s", "main"]).unwrap(),
            "chore(release): v0.1.0"
        );
        assert_eq!(
            git(remote, &["rev-parse", "v0.1.0^{commit}"]).unwrap(),
            git(remote, &["rev-parse", "main"]).unwrap()
        );
        assert!(
            git(remote, &["show", "main:Cargo.toml"])
                .unwrap()
                .contains("version = \"0.1.0\"")
        );
        let lock = git(remote, &["show", "main:Cargo.lock"]).unwrap();
        assert!(lock.contains("name = \"app\"\nversion = \"0.1.0\""), "{lock}");
        assert!(
            git(remote, &["show", "main:CHANGELOG.md"])
                .unwrap()
                .contains("## [0.1.0]")
        );
        assert_eq!(git(&repo.repo, &["status", "--porcelain"]).unwrap(), "");
    }
}
