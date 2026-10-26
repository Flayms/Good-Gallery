# Good Gallery (Rust)

A desktop gallery for large photo and video libraries (50k–500k files), with libraries on network shares, a local thumbnail cache, and search over the tags already embedded in your files.

This is a work-in-progress rewrite of Good Gallery (the Electron app, checked out next to this repo as `../good-gallery`) in Rust (eframe / egui on wgpu). Progress: [docs/progress.md](docs/progress.md). Plan: [docs/rewrite/plan.md](docs/rewrite/plan.md).

## Development

Requirements: Rust (pinned in `rust-toolchain.toml`, installed through rustup) and, on Windows, the MSVC C++ build tools.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

## License

GPL-3.0-only
