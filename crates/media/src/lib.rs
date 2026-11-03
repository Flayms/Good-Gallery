//! Reading media files: metadata through exiftool, thumbnails, video frames through ffmpeg. Writing edited photos.

// Rebuilt when another worktree built into the same target folder (`.cargo/config.toml`).
const _: Option<&str> = option_env!("GOOD_GALLERY_WORKTREE");

pub mod edit;
pub mod exiftool;
pub mod ffmpeg;
pub mod helpers;
pub mod metadata;
pub mod original;
pub mod rating;
pub mod replace;
pub mod subject;
pub mod tagging;
pub mod thumbnail;
pub mod update;
