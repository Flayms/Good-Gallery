# 031 · Updates

**Status:** Implemented

## Goal

Users learn that a newer version exists without looking for it, and on Windows can install it with one button. The app checks by default, but nothing is downloaded or installed until the user asks for it. Since the downloads aren't code-signed, the app verifies an update itself before it runs it.

## Requirements

### REQ-031-01 · Checked by default

- **Given** an installed or portable app, with _Check for updates_ on, as it is by default
- **When** the app starts, and then every day it stays open
- **Then** it asks GitHub for the latest published release, without slowing anything down
- **And** a development build, a screenshot run and the browser version never ask

### REQ-031-02 · Turning it off

- **Given** _Check for updates_ turned off in the settings
- **When** the app runs
- **Then** it asks nothing, and shows no notice

### REQ-031-03 · A failed check stays quiet

- **Given** no network, no `curl`, or an answer from GitHub that can't be read
- **When** the check runs
- **Then** the app shows nothing and tries again at the next check

### REQ-031-04 · The notice

- **Given** a published release newer than the running version
- **When** the check has answered
- **Then** the toolbar shows _Update available_, and its menu and the settings' _About_ say which version it is and offer _See what's new_, which opens that release in the browser
- **And** the same or an older version shows nothing

### REQ-031-05 · Updating is the user's choice

- **Given** the notice, in an app installed with the Windows installer
- **When** the user presses _Update and restart_
- **Then** the app downloads that version's installer, shows that it is downloading, verifies it (REQ-031-06), starts it and closes; nothing is downloaded before the button is pressed

### REQ-031-06 · A download that can't be verified

- **Given** a downloaded installer whose signature is missing or doesn't match the app's built-in public key, its version or its contents
- **When** it would be installed
- **Then** it is deleted and never run, the app keeps running, and a toast says the update couldn't be verified

### REQ-031-07 · Builds that can't install themselves

- **Given** the portable zip or the Linux downloads
- **When** a newer version exists
- **Then** the notice of REQ-031-04 appears without _Update and restart_

### REQ-031-08 · Installing over the running app

- **Given** the installer started by the app (`/S /UPDATE /D=<folder>`) while the app is closing
- **When** it runs
- **Then** it waits for the app to end, installs into the same folder and starts the new version, keeping the data folder
- **And** if the app doesn't end within 30 seconds, it changes nothing and exits with an error

### REQ-031-09 · Signed releases

- **Given** a release
- **When** the release workflow publishes it
- **Then** the installer has a `.sig` file beside it, an Ed25519 signature over the file's name and contents, made with the key whose public half is built into the app
- **And** without that key, or with a different one, nothing is published

### REQ-031-10 · The one exception to no network

- **Given** the app (REQ-011-11)
- **When** it checks for or downloads an update
- **Then** the only connection is made by the system's `curl`, over HTTPS, to GitHub's release of this repository; the app itself has no network code and depends on no network crate

## Out of scope

- Updating the portable zip or the Linux packages from the app.
- Skipping a version, or remembering a dismissed notice.
- A progress bar for the download.
- Code signing (#29). A Mark of the Web isn't added to the download, so SmartScreen isn't expected to ask, but that needs a look on a real Windows machine.

## Assumptions

- Windows 10 version 1803 or later, which ship `curl.exe` in `System32`. Linux packages recommend `curl`; without it the check fails quietly (REQ-031-03).
- The repository secret `UPDATE_SIGNING_KEY` holds the private key. Someone who takes over the repository could sign too, so the signature protects against a damaged or replaced download, not against that.

## Verification

| REQ | Test | Result |
| --- | ---- | ------ |
| REQ-031-01 | `crates/app/src/updates.rs` › req_031_01_checks_at_the_start_and_then_daily, req_031_01_a_build_without_an_updater_never_checks; `crates/app/tests/updates.rs` › req_031_01_checks_when_the_app_starts_and_only_once, req_031_01_is_on_by_default_and_without_an_updater_nothing_shows; `xtask/tests/packaging.rs` › req_031_01_the_linux_packages_recommend_curl | pass |
| REQ-031-02 | `crates/app/src/updates.rs` › req_031_02_turned_off_it_doesnt_check_and_turned_on_it_checks_at_once, req_031_02_an_answer_after_turning_it_off_shows_nothing; `crates/app/tests/updates.rs` › req_031_02_turned_off_it_asks_nothing_and_hides_the_notice, req_031_02_the_settings_turn_it_off_and_on | pass |
| REQ-031-03 | `crates/domain/src/update.rs` › req_031_03_an_unreadable_answer_has_no_version; `crates/app/tests/updates.rs` › req_031_03_a_failed_check_shows_nothing | pass |
| REQ-031-04 | `crates/domain/src/update.rs` › req_031_04_compares_each_part_as_a_number; `crates/app/src/updates.rs` › req_031_04_a_later_check_doesnt_disturb_a_running_update; `crates/app/tests/updates.rs` › req_031_04_a_newer_version_shows_the_notice_and_its_release, req_031_04_the_notice_and_its_menu_look_right, req_031_04_the_same_or_an_older_version_shows_nothing | pass |
| REQ-031-05 | `crates/app/tests/updates.rs` › req_031_05_nothing_is_installed_until_the_button_is_pressed, req_031_05_the_app_shows_the_download_while_it_runs; `xtask/src/dist.rs` › req_031_05_the_updater_downloads_the_file_the_release_has; a real update on Windows is still to do | partial |
| REQ-031-06 | `crates/media/src/update.rs` › req_031_06_accepts_only_the_signature_of_this_file_and_this_version, req_031_06_without_a_valid_key_nothing_verifies, req_031_06_a_file_on_disk_is_read_for_verifying; `crates/app/tests/updates.rs` › req_031_06_a_failed_install_says_so_and_keeps_the_app_and_the_offer | pass |
| REQ-031-07 | `crates/media/src/update.rs` › req_031_07_only_a_folder_with_the_uninstaller_can_install; `crates/app/tests/updates.rs` › req_031_07_a_build_that_cant_install_only_links_to_the_release | pass |
| REQ-031-08 | `xtask/tests/packaging.rs` › req_031_08_an_update_waits_for_the_app_and_restarts_it; `xtask/tests/workflows.rs` › req_031_08_the_installer_is_tested_before_a_release; `packaging/smoke-test.ps1`, run by `windows-packages.yml`. The 30-second limit has no test | partial |
| REQ-031-09 | `crates/media/src/update.rs` › req_031_09_the_app_has_the_public_key_built_in; `crates/domain/src/update.rs` › req_031_09_the_signature_covers_the_name_and_the_contents; `xtask/tests/packaging.rs` › req_031_09_the_app_verifies_what_the_release_signs; `xtask/tests/workflows.rs` › req_031_09_the_release_signs_the_installer | partial: needs the key |
| REQ-031-10 | `crates/domain/src/update.rs` › req_031_10_reads_only_plain_versions, req_031_10_downloads_only_from_this_repositorys_releases; `crates/media/src/update.rs` › req_031_10_curl_stays_on_https_and_within_its_bounds; `xtask/tests/no_network.rs` › req_031_10_only_the_updater_starts_curl | pass |
