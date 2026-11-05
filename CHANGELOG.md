# Changelog

All notable changes to Good Gallery. Versions follow [Semantic Versioning](https://semver.org), entries are generated from [Conventional Commits](https://www.conventionalcommits.org).
## [0.6.0](https://github.com/Flayms/Good-Gallery/releases/tag/v0.6.0) · 2026-11-05

### ✨ Features

- **viewer:** Zoom a photo with a touchpad's pinch (#168) (#184) ([`ff2f7e0`](https://github.com/Flayms/Good-Gallery/commit/ff2f7e0f159ea092cfd63be2f18f902af6d841ec))
- **selection:** A selected item shows only its ring (#185) ([`704d59c`](https://github.com/Flayms/Good-Gallery/commit/704d59c8ff44d73bad66756ed5b0a6a419b239c4))
- **faces:** Recognize a photo's faces right after finding them (#182) (#189) ([`3a74bca`](https://github.com/Flayms/Good-Gallery/commit/3a74bca6dc75bc6defce2e2bc5691e7fe23bebfe))
- **faces:** Add a model by choosing or dropping a file, no more links (#187) ([`f9fd549`](https://github.com/Flayms/Good-Gallery/commit/f9fd5496040f45e75b91b71b738fb41b9277f1f8))
- **processing:** What's running moves behind an icon (#190) ([`112c74b`](https://github.com/Flayms/Good-Gallery/commit/112c74b2e7e49421a5d9b47a5da11de0a4082118))
- **updates:** Check when the app starts, and with a button (#181) (#197) ([`68c44fd`](https://github.com/Flayms/Good-Gallery/commit/68c44fdbe18b3e9377c8ff129557188c642f2b1b))
- **selection:** A click opens, a circle starts a selection (#191) (#198) ([`563bbb8`](https://github.com/Flayms/Good-Gallery/commit/563bbb8951e55f85aba03e321ef08a8062fa74a6))
- **gallery:** The scrollbar shows years and the month while dragging (#199) (#208) ([`655523b`](https://github.com/Flayms/Good-Gallery/commit/655523ba13c05b28cd4d6d3668cccc0fb7add423))

### 🐛 Bug fixes

- **viewer:** Keep the control bar's buttons apart (#183) ([`7ad1e07`](https://github.com/Flayms/Good-Gallery/commit/7ad1e071ab284dc7337179fe2d640b2ed0a81bbc))
- **faces:** Shorter model descriptions without scores (#186) ([`46ed546`](https://github.com/Flayms/Good-Gallery/commit/46ed54672a85b22e5a79eb0bcd5c6f33e07fdfad))
- **faces:** Show the faces of a group on the People page (#194) (#195) ([`cbd5ef2`](https://github.com/Flayms/Good-Gallery/commit/cbd5ef2e3665e7806edacd6b74571ddeb3a4486b))
- **faces:** Say clearly what the search is doing, from the start (#193, #192) (#196) ([`0852df8`](https://github.com/Flayms/Good-Gallery/commit/0852df8d0f937bc9bb4d945f4d76f1809ec3ec66))

**Full changelog:** [v0.5.1...v0.6.0](https://github.com/Flayms/Good-Gallery/compare/v0.5.1...v0.6.0)
## [0.5.1](https://github.com/Flayms/Good-Gallery/releases/tag/v0.5.1) · 2026-11-05

### 🔧 Maintenance

- **faces:** Keep the version out of the settings snapshot (#180) ([`77d6731`](https://github.com/Flayms/Good-Gallery/commit/77d6731090dacd647abe5256ad9a7f647a5756a6))

**Full changelog:** [v0.5.0...v0.5.1](https://github.com/Flayms/Good-Gallery/compare/v0.5.0...v0.5.1)
## [0.5.0](https://github.com/Flayms/Good-Gallery/releases/tag/v0.5.0) · 2026-11-04

### ✨ Features

- **updates:** Show a newer version and install it from a button (#160) ([`709cbbf`](https://github.com/Flayms/Good-Gallery/commit/709cbbfafda56a7b6478305d9e15cc168e82023f))
- **faces:** Find and recognize faces with models the user picks (#166) ([`a27ebc6`](https://github.com/Flayms/Good-Gallery/commit/a27ebc6e5d151ff8be846536b5b9ba23e0c8d956))
- **selection:** A click selects, a double-click opens (#164) (#170) ([`77f9681`](https://github.com/Flayms/Good-Gallery/commit/77f968145c9357e0ec351f0c946a603e797e9f90))
- **installer:** Choose shortcuts and Open with, switch Open with in the settings (#161) (#172) ([`3ab631b`](https://github.com/Flayms/Good-Gallery/commit/3ab631b1ddf8f11236537f8f8c6a1ef029b3ccd6))
- **open-with:** Show files outside libraries without indexing them (#162) (#171) ([`f2e137b`](https://github.com/Flayms/Good-Gallery/commit/f2e137b054ea7b27794b328324e42f800d8ccd90))
- **faces:** Recommended models, charts, a status line and looking again (#178) ([`78f2c1e`](https://github.com/Flayms/Good-Gallery/commit/78f2c1e4f9c454b0929c0a67070229f2c349c5ce))

### 🐛 Bug fixes

- **open-with:** Index an opened file ahead of other scans (#162) (#169) ([`80c70c8`](https://github.com/Flayms/Good-Gallery/commit/80c70c8be502bb02580d771220529f7be6cb4cab))

### 📝 Documentation

- **agents.md:** Improve worktree storage usage ([`77a3f0c`](https://github.com/Flayms/Good-Gallery/commit/77a3f0c9701c42c6a6ce6aa6c6d11aaa30888dba))
- Improve face-models text ([`c0d28b8`](https://github.com/Flayms/Good-Gallery/commit/c0d28b88357d780a91da4253284d860c6dc847b6))

### 🔧 Maintenance

- **release:** Build side by side, from a cache main keeps warm (#157) (#159) ([`147625c`](https://github.com/Flayms/Good-Gallery/commit/147625c9a792e838965f37741622518b0b447252))

**Full changelog:** [v0.4.0...v0.5.0](https://github.com/Flayms/Good-Gallery/compare/v0.4.0...v0.5.0)
## [0.4.0](https://github.com/Flayms/Good-Gallery/releases/tag/v0.4.0) · 2026-11-03

### ✨ Features

- **design:** Fill the stars of a rating (#92) ([`2ec8e8d`](https://github.com/Flayms/Good-Gallery/commit/2ec8e8d1a204ce51eb17fe6394f7e61097c17a94))
- **viewer:** Show embedded face regions on the photo (#93) ([`93a9c25`](https://github.com/Flayms/Good-Gallery/commit/93a9c2583809a5e86940bdf3be8f760cf905f068))
- **linux:** Follow the desktop's caption button layout (#95) ([`2415ee4`](https://github.com/Flayms/Good-Gallery/commit/2415ee4ee559b0d6435533688ac10227f3466a78))
- **video:** Time video to the measured display rate on Linux (#97) ([`7af502c`](https://github.com/Flayms/Good-Gallery/commit/7af502ce95f44c4c036dba7eb889de4624110d56))
- **indexing:** Read unreadable files again and list them in the toolbar (#105) ([`d717705`](https://github.com/Flayms/Good-Gallery/commit/d7177056ff15752170f22ae5792d85c4656414ae))
- **viewer:** Keep the titles of details an item lacks in the info panel (#114) ([`572bc65`](https://github.com/Flayms/Good-Gallery/commit/572bc65601df92040c8b92b1f024fd0effb9a22c))
- **edit:** Add a hue slider to the color section (#115) ([`44563f2`](https://github.com/Flayms/Good-Gallery/commit/44563f22b92ac9be3ea884ebc8ab77a00e55e5c8))
- **viewer:** Rate items with 1–5 and the info panel's stars (#120) ([`46a06a6`](https://github.com/Flayms/Good-Gallery/commit/46a06a66e299363a33fbef349fe46e23f92e0f31))
- **gallery:** Select several items and rate them at once (#123) ([`dc5b67a`](https://github.com/Flayms/Good-Gallery/commit/dc5b67af7102153cf2862b7b09859935901eec03))
- **viewer:** Add and remove tags, People and Places in the info views (#124) ([`1bdf52b`](https://github.com/Flayms/Good-Gallery/commit/1bdf52be81583581d3608b2c3213a083acdc0f63))
- **gallery:** Tag the selection (#125) ([`88c0de6`](https://github.com/Flayms/Good-Gallery/commit/88c0de6c848b6f0ada4b54b1f63f2687badc25b2))
- **web:** Fit the layout to narrow screens such as phones (#127) ([`5ddbcaa`](https://github.com/Flayms/Good-Gallery/commit/5ddbcaacf8e07ddb08b26cbe083486f906030aaa))
- **app:** Open photos and videos with Good Gallery from the file manager (#129) ([`3a83457`](https://github.com/Flayms/Good-Gallery/commit/3a83457e8930145a552561568a9957bcfd837c00))
- **edit:** Film looks with strength and grain (#131) ([`df88120`](https://github.com/Flayms/Good-Gallery/commit/df8812058d5210a5bffd6aee5b6527e154834e12))
- **web:** Attach the page to releases as a zip for self-hosting (#132) ([`71ea63c`](https://github.com/Flayms/Good-Gallery/commit/71ea63c0d29e815d59e6b1ce834a49da8ebd5437))
- **settings:** Add a Reduce transparency setting (#133) ([`3a7508f`](https://github.com/Flayms/Good-Gallery/commit/3a7508f21b1e5bca8a79523d4e28c5532e3dfb7f))
- **edit:** Save crops and quarter turns, with face regions moved along (#134) ([`982c8c3`](https://github.com/Flayms/Good-Gallery/commit/982c8c31c9d7d447afbd368b8dbe9853c861ea9f))
- **linux:** Build .deb and .rpm packages for releases (#135) ([`80620b8`](https://github.com/Flayms/Good-Gallery/commit/80620b8bf85cf1b4564dfa49eb8f8bfb258adfba))
- **edit:** Rotate photos in the editor by quarter turns (#137) ([`31277a5`](https://github.com/Flayms/Good-Gallery/commit/31277a588511456e808808041c451df2445076f1))
- **edit:** Crop photos in the editor with a frame and fixed shapes (#138) ([`b15749d`](https://github.com/Flayms/Good-Gallery/commit/b15749dba00957e48dba8d4e00be233a18825721))
- **settings:** Show the version and a link to the latest release (#141) ([`7678691`](https://github.com/Flayms/Good-Gallery/commit/7678691525f5da92921743d5c557aa7268aa71f1))
- **edit:** Film grain for looks, and a Superia 200 look (#142) ([`a558b85`](https://github.com/Flayms/Good-Gallery/commit/a558b85440a4ce60f7d832a46a0234dfc8c2aefd))
- **xtask:** Measure the glass pass with the viewer open and glass off (#143) ([`de355f5`](https://github.com/Flayms/Good-Gallery/commit/de355f5a07a3330bf4b49363287610f92f820dd6))
- **edit:** Looks change colors per hue (#144) ([`3aea3da`](https://github.com/Flayms/Good-Gallery/commit/3aea3dad9ee5471baf2ae88dcdd00b97faf6a733))
- **edit:** Bright lights glow red in film looks (#145) ([`50d5311`](https://github.com/Flayms/Good-Gallery/commit/50d5311da596ff99fa5a642d535efae3eee0d84a))
- **edit:** Gradient, luminance and color masks (#147) ([`18f2231`](https://github.com/Flayms/Good-Gallery/commit/18f22314041ad18e2b332321cff1a599f65f66cd))
- **edit:** Masks made of several shapes, with the subject among them (#148) ([`a6d094e`](https://github.com/Flayms/Good-Gallery/commit/a6d094e3c121d0a9e7c56c19448bb773ea8889b5))
- **edit:** Brush and freeform masks (#149) ([`132db5d`](https://github.com/Flayms/Good-Gallery/commit/132db5d206355328b7526456153afd532eb50dff))
- **edit:** What to edit comes first, and a mask has one shape (#154) ([`df71f0d`](https://github.com/Flayms/Good-Gallery/commit/df71f0d9235e8b26079a188d1f359ebcca52a334))

### 🐛 Bug fixes

- **release:** List the changes of unpublished versions in the release notes (#82) ([`bc37189`](https://github.com/Flayms/Good-Gallery/commit/bc3718922e4849aec4a95921231ca39102e69a42))
- **indexer:** Give a reconnecting network share time before it counts as offline (#83) ([`42b3d28`](https://github.com/Flayms/Good-Gallery/commit/42b3d28b999912b7471870a06629813ba53ac149))
- **viewer:** Let Tab move focus into the viewer and its filmstrip (#84) ([`42db6b5`](https://github.com/Flayms/Good-Gallery/commit/42db6b5fb2cb22cf2b962d8a433c5fb3cd21316a))
- **metadata:** Turn photos oriented only in XMP upright (#85) ([`4f2d971`](https://github.com/Flayms/Good-Gallery/commit/4f2d97167013a60263baf97c70f6d50f0d4186ab))
- **thumbnails:** Retry renames on Windows while Defender holds the file (#86) ([`071fd9e`](https://github.com/Flayms/Good-Gallery/commit/071fd9e7086d184284205729ffdf7b7111db90e8))
- **design:** Make the date picker popups glass (#87) ([`92ea2de`](https://github.com/Flayms/Good-Gallery/commit/92ea2de288c89e9890d4a90a0b0e521bf231bccc))
- **viewer:** Blur the photo under the viewer's chrome and menus (#88) ([`fc6cca8`](https://github.com/Flayms/Good-Gallery/commit/fc6cca8cc7bc4c8c9ac6aaeda05f137342a3fdaf))
- **dist:** Replace downloads that another process still holds (#119) ([`29b98e9`](https://github.com/Flayms/Good-Gallery/commit/29b98e9c7831eae7d0cca2d0d712b38126f7d76b))
- **video:** Keep libmpv loaded, and wait out a toast in the web test (#122) ([`de65299`](https://github.com/Flayms/Good-Gallery/commit/de652992482de741621b80a286b53891f5d3de18))
- **media:** Never rename over a library file, and read writes back (#140) (#146) ([`ec8ebf7`](https://github.com/Flayms/Good-Gallery/commit/ec8ebf77bbba9e6b718ffff32d3a806d580d608f))
- **viewer:** Keep showing an item that leaves the search (#150) (#153) ([`bc49670`](https://github.com/Flayms/Good-Gallery/commit/bc4967035e4ecebbfbea192037c46cc81b7726cd))
- **edit:** Show the saved file until the subject is found again (#151) (#156) ([`557cc8a`](https://github.com/Flayms/Good-Gallery/commit/557cc8aaa4114b494a320fa59a927437023bd6c7))

### ⚡ Performance

- **search:** Drive broad tag and file name searches by their matches (#89) ([`c205bb0`](https://github.com/Flayms/Good-Gallery/commit/c205bb0f0c94982f125ea10f0e5fac6a562d1fb9))
- **search:** Meet the search speed budget, with its own for broad results (#94) ([`9b38540`](https://github.com/Flayms/Good-Gallery/commit/9b38540318b339bb931f82c954c0995c5005a6b3))
- **thumbnails:** Render thumbnails below the UI's priority (#106) ([`3b657d4`](https://github.com/Flayms/Good-Gallery/commit/3b657d421f9bfe470341df9f66201dc0a3882785))
- **edit:** Open editing at once with the viewer's original (#113) ([`018395c`](https://github.com/Flayms/Good-Gallery/commit/018395c18cf6a8e4c7d77d880be603ac4bfba4e9))

### 📝 Documentation

- Tidy up the known pitfalls (#104) ([`4e24672`](https://github.com/Flayms/Good-Gallery/commit/4e2467287f64f51e4b6d398ba270d8be15b08dfc))
- Sort the known pitfalls into notes and issues to fix (#111) ([`461f920`](https://github.com/Flayms/Good-Gallery/commit/461f920de5e0b0a283596c953ba72b75e33b21e5))
- Rewrite the architecture as an overview, decisions into the plans (#128) ([`1cd36fe`](https://github.com/Flayms/Good-Gallery/commit/1cd36fe4c92d00472e4f1421eacc99ff93807d76))
- Record the glass pass cost and the ICC profile count (#14, #22) (#155) ([`77cab66`](https://github.com/Flayms/Good-Gallery/commit/77cab660c6f2decb88132858117474d0ff34d717))

### 🔧 Maintenance

- **indexing:** Cover folders that can't be listed (REQ-002-11) (#91) ([`da0f57f`](https://github.com/Flayms/Good-Gallery/commit/da0f57ffd55ad7b3973633b0d6a1ff1c8bcfb100))
- **xtask:** Add cargo xtask icons (#100) ([`11c3d56`](https://github.com/Flayms/Good-Gallery/commit/11c3d560818ed9a64bf05089b1a9bc7ddc501f46))
- Rebuild workspace crates when another worktree built into the shared target (#112) ([`cab2f11`](https://github.com/Flayms/Good-Gallery/commit/cab2f11150c5f6b581f1b462fc348c5edfa4d9aa))
- **startup:** Start once on an empty data folder (#116) ([`b2876af`](https://github.com/Flayms/Good-Gallery/commit/b2876af50358cefbc97230e209f8365a6360c96f))
- **ui:** Keep fixture folders under /tmp whatever TMPDIR is (#117) ([`6d36ee8`](https://github.com/Flayms/Good-Gallery/commit/6d36ee8ee48fa6a59c0862c5a8f840bc32ea5e3f))
- **helpers:** Find fetched helpers in development builds (#118) ([`9317653`](https://github.com/Flayms/Good-Gallery/commit/93176531d1edd5031a689fe7837f3837b2cf88de))
- **ui:** Line up the Linux caption buttons in snapshots as in the app (#126) ([`3f739f3`](https://github.com/Flayms/Good-Gallery/commit/3f739f30ea8221503cf9e12be7df0ddb4cf714a3))
- **web:** Update the narrow-screen snapshots for the whole-window harness (#130) ([`8ae51b8`](https://github.com/Flayms/Good-Gallery/commit/8ae51b88d7bb4d5bc16b6eb952228a6c9dad7544))

**Full changelog:** [v0.3.0...v0.4.0](https://github.com/Flayms/Good-Gallery/compare/v0.3.0...v0.4.0)
## [0.3.0](https://github.com/Flayms/Good-Gallery/releases/tag/v0.3.0) · 2026-10-31

### ✨ Features

- **sidebar:** Hide and show the sidebar ([`1a388d4`](https://github.com/Flayms/Good-Gallery/commit/1a388d4d8e747b8530d0aef845f47010ab023ca3))
- **release:** Name downloads after the system and architecture ([`247ec27`](https://github.com/Flayms/Good-Gallery/commit/247ec2780b405a6ceb4f5989bd71f7c8cd43f8bf))
- **gallery:** Autoscroll with the middle mouse button ([`296ae99`](https://github.com/Flayms/Good-Gallery/commit/296ae997bce85408bea42c4a3ace278716153fc1))
- Light mode ([`455c956`](https://github.com/Flayms/Good-Gallery/commit/455c956aa622d8ecc7e7175f961a7604f47edfe9))
- **viewer:** Edit photos with light, curves, color and effects (#67) ([`ecd62c5`](https://github.com/Flayms/Good-Gallery/commit/ecd62c5d1cbfccf7f87c14aef6912cbc6eb22a75))
- Order and hide the sidebar groups in the settings (#66) ([`5d61b1e`](https://github.com/Flayms/Good-Gallery/commit/5d61b1e4f5c521f2467a2220c03d9a55c9f4e9bf))
- **viewer:** Brightness, histogram, and subject and background editing (#78) ([`3b4797a`](https://github.com/Flayms/Good-Gallery/commit/3b4797aa125d0165bfa1e607dda4d7fa7f4d9f9a))
- **viewer:** Blur behind the subject, tilt-shift and radial blur (#80) ([`f21c69e`](https://github.com/Flayms/Good-Gallery/commit/f21c69e0ffd8cb008984237a95722fcf36ca7c03))

### 🐛 Bug fixes

- **gallery:** Make autoscroll feel like a browser's ([`402ed89`](https://github.com/Flayms/Good-Gallery/commit/402ed8922b4b41f909dd7479ec3ad7b6d0be4e66))
- Keep the window controls dark over the viewer in light mode ([`b9c08b8`](https://github.com/Flayms/Good-Gallery/commit/b9c08b811c33253af973af2ad56b166efdd5d1a6))
- **viewer:** Don't crash when discarding an edit (#73) ([`d120e9d`](https://github.com/Flayms/Good-Gallery/commit/d120e9d303e686a5c0bae93eb05ce58298b2c1bc))

### 📝 Documentation

- **pitfalls:** Worktrees sharing a target dir test each other's code ([`1151b1d`](https://github.com/Flayms/Good-Gallery/commit/1151b1dcf5df08fce52d85a95d411887addb36d7))
- Update agents.md ([`1d132af`](https://github.com/Flayms/Good-Gallery/commit/1d132af6facbe88e5d539cd35b2f536a7666c641))
- **readme:** Split the development requirements by platform and list NSIS ([`ab8b429`](https://github.com/Flayms/Good-Gallery/commit/ab8b4296bca21fb2f9f6e795729a26654dcad413))
- **specs:** Add 020 light mode spec and plan ([`6170645`](https://github.com/Flayms/Good-Gallery/commit/6170645e3d2632e00722b7316c71bc846395ebb1))
- Update README for clarity and add web version link ([`3857b6a`](https://github.com/Flayms/Good-Gallery/commit/3857b6a107d2c7b72e352800e89e14b988671860))
- Update AGENTS.md ([`b95ef95`](https://github.com/Flayms/Good-Gallery/commit/b95ef9568e5fbc1859efa7e073d29789f101f966))
- **readme:** Rewrite it, with editing, shortcuts and a screenshot of the editor (#72) ([`568a26d`](https://github.com/Flayms/Good-Gallery/commit/568a26dab28909ec715ec9805b84251cd6aeb3b5))

### 🔧 Maintenance

- **web:** Update screenshots for the Hide sidebar button ([`f8ca7df`](https://github.com/Flayms/Good-Gallery/commit/f8ca7df74b2d82e3b09fa98f6a093c615805f9af))
- Upload new snapshot renders with the diffs ([`fca770f`](https://github.com/Flayms/Good-Gallery/commit/fca770f1070a8c9495d4fff3dc527078ef70b900))
- **web:** Screenshots with the Appearance setting, and in light ([`ae7f847`](https://github.com/Flayms/Good-Gallery/commit/ae7f8472540061aeb8b66a51845c288c8ca47201))
- Windows snapshots for light mode and the Appearance setting ([`1da615a`](https://github.com/Flayms/Good-Gallery/commit/1da615a95ac2aae9600af31909e9e53493f98444))
- Add vscode debugging ([`06c1f78`](https://github.com/Flayms/Good-Gallery/commit/06c1f789e992e15d7557316843b64dcdb5b1e138))
- Improve launch configuration ([`9757560`](https://github.com/Flayms/Good-Gallery/commit/9757560835c23f64189516c74c3803bd3724ac08))
- Run the tests with nextest and save the Rust cache only on main (#79) ([`38d3596`](https://github.com/Flayms/Good-Gallery/commit/38d3596614fa73d8d095e9f0a820a3c6d7a0f976))

**Full changelog:** [v0.2.1...v0.3.0](https://github.com/Flayms/Good-Gallery/compare/v0.2.1...v0.3.0)
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

