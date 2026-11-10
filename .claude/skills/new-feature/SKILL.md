---
name: new-feature
description: Steps for adding a feature or a spec'd behavior to Good Gallery, from spec to PR. Use when implementing a GitHub issue that adds or changes behavior (tier 1–3 in AGENTS.md).
---

1. **Spec.** Find the feature's spec in `docs/specs/README.md`, or copy the template there into a new `docs/specs/NNN-name/spec.md` and add it to the index. Write Given/When/Then requirements with new `REQ-NNN-XX` IDs (never reuse one). Tier 3 also gets a `plan.md`.
2. **Red.** Write a test named `req_NNN_XX_…` (or with a `/// REQ-NNN-XX: …` comment) and watch it fail.
   - Pure logic goes into `domain` (or `meta`), with unit tests in that crate.
   - UI behavior goes into `crates/app/tests/<area>.rs`. A new file there needs a `mod` line in `tests/ui.rs`. Build the state through `tests/common`, and call `wait_for_jobs` before stepping the harness.
3. **Green.**
   - Feature state goes into `crates/app/src/state/<feature>.rs` (like `rename.rs`), not into `AppState`'s fields.
   - Slow work runs on a thread and sends an `Event` back, which is applied in `AppState::poll`. A write into a library file goes through `spawn_write`.
   - SQL goes into `db`, and a schema change is a new migration. Files and helpers go into `media`.
4. **Look at it.** For UI, add or update a snapshot (`UPDATE_SNAPSHOTS=1`), or take `cargo run -p app -- --screenshot shot.png`, and look at the image.
5. **Check.** Run `cargo test -p <crate>` while working, then the full lint and tests from `AGENTS.md`. Fill in the spec's Verification table and Status.
6. **PR.** Follow _Git_ and _Definition of Done_ in `AGENTS.md`: attach screenshots for UI changes and use a closing keyword.
