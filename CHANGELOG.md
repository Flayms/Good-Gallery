# Changelog

All notable changes to Good Gallery. Versions follow [Semantic Versioning](https://semver.org), entries are generated from [Conventional Commits](https://www.conventionalcommits.org).
## [0.2.1](https://github.com/Flayms/Good-Gallery/releases/tag/v0.2.1) · 2026-10-29

### 📝 Documentation

- **AGENTS:** Add note to text created by agents ([`469b174`](https://github.com/Flayms/Good-Gallery/commit/469b1747fd99971b5c53b31c25a1ebe34f3f0005))

### 🔧 Maintenance

- **web:** Wait for tag suggestions before searching ([`7932792`](https://github.com/Flayms/Good-Gallery/commit/79327922554a3e363a8796f13d8956872d1d91b2))

**Full changelog:** [v0.2.0...v0.2.1](https://github.com/Flayms/Good-Gallery/compare/v0.2.0...v0.2.1)
## [0.2.0](https://github.com/Flayms/Good-Gallery/releases/tag/v0.2.0) · 2026-10-29

### ✨ Features

- Linux support ([`812cc1a`](https://github.com/Flayms/Good-Gallery/commit/812cc1ad7f2c98e67573e22415c2824cdd060f67))
- **meta:** Read EXIF, IPTC, XMP and MP4 metadata without exiftool ([`4d3e6b1`](https://github.com/Flayms/Good-Gallery/commit/4d3e6b115d7b0ef6eb36734cabe1e94e1d7fd861))
- **web:** Build the app for the browser ([`5665ef4`](https://github.com/Flayms/Good-Gallery/commit/5665ef4761ced9c69b1d2643802070095cc484c5))

### 🐛 Bug fixes

- **web:** Link SQLite's host services and log scan and thumbnail progress ([`f735a09`](https://github.com/Flayms/Good-Gallery/commit/f735a09402cccb3ffdee7128343fb55cfc660f06))
- **meta:** Reconcile IPTC and XMP keywords as exiftool's MWG mode does ([`974487e`](https://github.com/Flayms/Good-Gallery/commit/974487e0afd1146a4d42a3c4f4110acb4ba99007))
- **web:** Show libraries without folder access as needing access ([`37affe2`](https://github.com/Flayms/Good-Gallery/commit/37affe292b19cc492c6334596fc9a4b096875e9f))

### 📝 Documentation

- **specs:** Record the ruleset behind REQ-014-02 ([`ba9f763`](https://github.com/Flayms/Good-Gallery/commit/ba9f76350ff3f558666267e5982b9f07b23d7c92))
- **agents:** A PR is done when it is green, not when it is opened ([`f3d9ce5`](https://github.com/Flayms/Good-Gallery/commit/f3d9ce5db68421494832ef741e108173d53b72bd))
- **agents:** Shorten ([`c3390cc`](https://github.com/Flayms/Good-Gallery/commit/c3390cc68a4aca4e734906db4903dd15f4fa8bd6))
- **specs:** REQ-014-02 is partly tested by the workflow test ([`2ea18f0`](https://github.com/Flayms/Good-Gallery/commit/2ea18f090ff8e460405512b4ce80982241ff98f5))
- Move the roadmap to GitHub issues, add issue templates ([`fef9833`](https://github.com/Flayms/Good-Gallery/commit/fef9833776a013523647ada6fa088c947e1dbd0e))
- **AGENTS:** Update issue handling guidelines and definition of done ([`e5b0e54`](https://github.com/Flayms/Good-Gallery/commit/e5b0e542dedb39bfdf265da7a89019dd94dbe00e))
- **specs:** 019 web spec and plan (draft) ([`a9628c9`](https://github.com/Flayms/Good-Gallery/commit/a9628c9f5e1234e610f380d10aed2ed5e634eea1))
- **web:** Plan as built, verification, architecture, README and pitfalls ([`0f4ee45`](https://github.com/Flayms/Good-Gallery/commit/0f4ee45e417507ddf4f1f1a4877a2ef4a7b343ca))
- **web:** #45 closes with the PR ([`b4083a5`](https://github.com/Flayms/Good-Gallery/commit/b4083a53127e118178ee448ac27f1d50adbcb603))

### 🔧 Maintenance

- Skip CI on release commits pushed to main ([`29f9a70`](https://github.com/Flayms/Good-Gallery/commit/29f9a70671efa23f77fc3a1483eed9d753c2bbf8))
- Tidy merged worktrees at session start ([`f7c590f`](https://github.com/Flayms/Good-Gallery/commit/f7c590f3194e6ced7c0495026b19fb7262be0054))
- **titlebar:** Run the Linux window chrome tests on Linux only ([`fcdddc7`](https://github.com/Flayms/Good-Gallery/commit/fcdddc7d224b557b3e2eb99ebd952b0969c457b6))
- Report the required Lint and Test checks for all platforms ([`f8369aa`](https://github.com/Flayms/Good-Gallery/commit/f8369aa4aa92f70ac60f49015303062143e891c1))
- **thumbnails:** Make the full-cache eviction check independent of mtime ties ([`7bcaa72`](https://github.com/Flayms/Good-Gallery/commit/7bcaa728d5b26505116f3bbdd593109c45729f6c))
- **web:** Page shell and cargo xtask web ([`945f574`](https://github.com/Flayms/Good-Gallery/commit/945f5748584e6c4b380c9a84eb9bee306b4cb06c))
- **web:** End-to-end browser tests and no-network checks ([`4b26c72`](https://github.com/Flayms/Good-Gallery/commit/4b26c72dca192da3e23a71275575f87b3f3c8d00))
- **web:** Test the page on pull requests, publish it to GitHub Pages on release ([`51b5fd7`](https://github.com/Flayms/Good-Gallery/commit/51b5fd70eb3ba96e69a0af79153084e4d1606880))
- **web:** Cover picking again, rescans and removing a library ([`31eb41e`](https://github.com/Flayms/Good-Gallery/commit/31eb41ed2781c0f3cdd4f889b8f20b51b4ac6193))
- **web:** Wait out the access toast before the settings screenshot ([`18230f7`](https://github.com/Flayms/Good-Gallery/commit/18230f748d9dfba9d5bde0e79a04fb3b8521d75c))

**Full changelog:** [v0.1.2...v0.2.0](https://github.com/Flayms/Good-Gallery/compare/v0.1.2...v0.2.0)
## [0.1.2](https://github.com/Flayms/Good-Gallery/releases/tag/v0.1.2) · 2026-10-28

### 🐛 Bug fixes

- **ci:** Install NSIS before building the installer (#7) ([`3ac5b06`](https://github.com/Flayms/Good-Gallery/commit/3ac5b06938abd2185501bfe260853aa35b307194))

### 🔧 Maintenance

- Run checks on pushes to main and pull requests only (#6) ([`afe1ff5`](https://github.com/Flayms/Good-Gallery/commit/afe1ff5125bea40a607006fd5eafd9bba8393907))

**Full changelog:** [v0.1.1...v0.1.2](https://github.com/Flayms/Good-Gallery/compare/v0.1.1...v0.1.2)
## [0.1.1](https://github.com/Flayms/Good-Gallery/releases/tag/v0.1.1) · 2026-10-28

### 🐛 Bug fixes

- **thumbnails:** Keep temp files of writes in progress during the startup walk ([`37c22f5`](https://github.com/Flayms/Good-Gallery/commit/37c22f531aff53a3146e2a1a8231021295f9f603))

### 🔧 Maintenance

- Pin the time zone so snapshots match on every machine ([`490833f`](https://github.com/Flayms/Good-Gallery/commit/490833f0e0f52c4bdbd62fb07fd197dc6faa2152))
- Mask the whole location row in viewer snapshots ([`03a7e00`](https://github.com/Flayms/Good-Gallery/commit/03a7e009454dc151a7221b38e1f4196a395c81c2))
- Shrink dev debug info, remove merged worktrees ([`ecb95f1`](https://github.com/Flayms/Good-Gallery/commit/ecb95f172956befc8b40b82f3c4e84e720d57b28))

**Full changelog:** [v0.1.0...v0.1.1](https://github.com/Flayms/Good-Gallery/compare/v0.1.0...v0.1.1)
## [0.1.0](https://github.com/Flayms/Good-Gallery/releases/tag/v0.1.0) · 2026-10-28

### ✨ Features

- Add M0a grid and look spike ([`84e3d5e`](https://github.com/Flayms/Good-Gallery/commit/84e3d5efd0a6ec2b41c602bb0cad1d581bd0d896))
- Add M0b video spike (libmpv, software render, display-synced) ([`8041b84`](https://github.com/Flayms/Good-Gallery/commit/8041b840ca3a4398f842dd6089266f768a3b7a58))
- M1 foundation (workspace, check-specs, themed app shell) ([`993f782`](https://github.com/Flayms/Good-Gallery/commit/993f782cfd8518db96a6aee8feba768050251a7f))
- M3 database and libraries ([`efe9d5e`](https://github.com/Flayms/Good-Gallery/commit/efe9d5ed2288632e8f32bdc704e3e1c6e918d9ac))
- M4a indexing metadata (exiftool, extraction, index writer) ([`9f60aab`](https://github.com/Flayms/Good-Gallery/commit/9f60aab3e34010737f8619f463286ea93a3b22d3))
- M4b indexing scanning (walk, scans, scan queue, app progress) ([`7fbe734`](https://github.com/Flayms/Good-Gallery/commit/7fbe73493d39925afd49ecb952911dd9c89c45e4))
- M4c ([`cb83428`](https://github.com/Flayms/Good-Gallery/commit/cb834282789214f7389884df4b7ed731d48711da))
- M5 thumbnails ([`53c0578`](https://github.com/Flayms/Good-Gallery/commit/53c0578eef4ddb1bd6d2269f7d9fc65b60a23141))
- M6a gallery tiles ([`4fe8bb5`](https://github.com/Flayms/Good-Gallery/commit/4fe8bb5dfa1957fc609f20df68e1f02c283ebd7d))
- M6b gallery views and toolbar ([`40b7752`](https://github.com/Flayms/Good-Gallery/commit/40b77521019e440233c2da8ac32f8e1c5b9bba6a))
- M7a search model and queries ([`678133f`](https://github.com/Flayms/Good-Gallery/commit/678133fff5bf7ade739e50d789e71411df37bd4e))
- M7b search bar, filters and sidebar ([`9710948`](https://github.com/Flayms/Good-Gallery/commit/97109480441b57165eefeeb488269ec16604128d))
- M8a viewer core ([`384c6ab`](https://github.com/Flayms/Good-Gallery/commit/384c6abb872cc3f6729b03fb8ad64cd066fd1c59))
- M8b viewer panels and filmstrip ([`5d1f404`](https://github.com/Flayms/Good-Gallery/commit/5d1f4045bdd3f88de45fc48587649947765a0d84))
- M9 video ([`32ed00f`](https://github.com/Flayms/Good-Gallery/commit/32ed00f9faadb80d683c39e3a966a4e36a109fdf))
- M10 settings, design polish, glass ([`7501ccb`](https://github.com/Flayms/Good-Gallery/commit/7501ccbd3a165882585381dd43dbb23f6068ac95))
- M11 packaging and release ([`435f7b5`](https://github.com/Flayms/Good-Gallery/commit/435f7b5fe73b0fcd6b7cccb9ef8590a107f63408))

### 📝 Documentation

- M2 spec audit for the Rust rewrite ([`89093bd`](https://github.com/Flayms/Good-Gallery/commit/89093bde1e0877750f69543b51e6866c4eaec391))
- Fix progress log formatting ([`99a301f`](https://github.com/Flayms/Good-Gallery/commit/99a301fd90a79067a203d3c39b71506615c95615))
- Record the M11 release dry run ([`8e42ec3`](https://github.com/Flayms/Good-Gallery/commit/8e42ec32ffc422690640935d6aeec35d6e8e7e39))
- Finish the migration, move milestone plans into spec plans ([`469dfa5`](https://github.com/Flayms/Good-Gallery/commit/469dfa5234478b506f9c5e09b4bda0cf0abbeed4))
- List git-cliff as a release prerequisite ([`afdc9fe`](https://github.com/Flayms/Good-Gallery/commit/afdc9fe9b3702229f29238df840e10d051fe3723))

