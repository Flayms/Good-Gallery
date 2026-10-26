# 014 · CI and releases

**Status:** Approved (ported from the reference, not yet implemented in Rust)

## Goal

Every commit is checked automatically, so a broken change never lands on `main` unnoticed and a pull request can only be merged when its checks pass. Cutting a release is one command: the version follows from the conventional commits since the last release, the changelog writes itself, and the GitHub release comes with readable notes and the Windows installer to download.

## Requirements

### REQ-014-01 · Checks on every push

- **Given** a commit pushed to any branch, or a pull request
- **When** CI runs
- **Then** it runs the linter, the type check, the spec check, the unit tests and the e2e tests, and the run fails if any of them fails

### REQ-014-02 · Checks gate merges

- **Given** a pull request into `main`
- **When** its CI run hasn't passed
- **Then** it can't be merged

### REQ-014-03 · Version from the commits

- **Given** commits since the last release (a `vX.Y.Z` tag)
- **When** the release script runs
- **Then** the next version bumps the major version for a breaking change (the minor version before 1.0.0), the minor version for a `feat` and the patch version otherwise. Without any release yet, the version is 0.1.0

### REQ-014-04 · Overriding the version

- **Given** the release script is run with `major`, `minor`, `patch` or an explicit version like `1.2.3`
- **When** it computes the next version
- **Then** it uses that bump or version instead of the one derived from the commits. Unknown arguments, or a bump together with a version, are rejected

### REQ-014-05 · Preconditions

- **Given** the checked-out branch isn't `main`, the working tree has uncommitted changes, `main` is behind `origin/main`, there are no commits since the last release, or the tag of the next version already exists
- **When** the release script runs
- **Then** it stops with a message saying what to fix, and changes nothing

### REQ-014-06 · Changelog

- **Given** a release
- **When** the release script writes `CHANGELOG.md`
- **Then** it has a section per version with its date, breaking changes first, then the commits grouped by type (features, bug fixes, performance, refactoring, documentation, reverts, maintenance), each linking its commit, and a link comparing it with the previous version. Release commits and commits that don't follow the conventional format aren't listed

### REQ-014-07 · Commit, tag and push

- **Given** the preconditions hold
- **When** the release script runs
- **Then** it prints the next version and its notes, asks for confirmation, then sets the version in `package.json`, commits it with `CHANGELOG.md` as `chore(release): vX.Y.Z`, tags the commit `vX.Y.Z` and pushes both. With `--dry-run` it stops after printing, with `--yes` it doesn't ask

### REQ-014-08 · Published release

- **Given** a pushed `vX.Y.Z` tag
- **When** the release workflow runs
- **Then** it runs the checks of REQ-014-01, builds the Windows installer (`setup.exe`) and the portable zip, and publishes a GitHub release named after the version, whose notes are the version's changelog section plus download instructions and which has both attached

## Out of scope

- Code signing (Windows SmartScreen warns about unsigned downloads). Free option for later: SignPath Foundation.
- Keeping the portable version's data next to the executable: it uses the same user data folder as an installed app.
- Auto-updates in the app.
- Builds for macOS or Linux.
- Pre-releases and release branches.

## Assumptions

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org) (`feat: …`, `fix(scope): …`, `feat!: …` / `BREAKING CHANGE:` for breaking changes).
- Merge protection is a GitHub branch protection rule (or ruleset) on `main` that requires the CI checks; it lives in the repository settings, not in the repository.

## Verification

| REQ        | Test                                                                                       | Result  |
| ---------- | ------------------------------------------------------------------------------------------ | ------- |
| REQ-014-01 | — | no test |
| REQ-014-02 | — | no test |
| REQ-014-03 | — | no test |
| REQ-014-04 | — | no test |
| REQ-014-05 | — | no test |
| REQ-014-06 | — | no test |
| REQ-014-07 | — | no test |
| REQ-014-08 | — | no test |
