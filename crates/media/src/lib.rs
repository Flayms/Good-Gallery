//! Reading media files: metadata through exiftool, thumbnails, video frames through ffmpeg. Writing edited photos.

// Rebuilt when another worktree built into the same target folder (`.cargo/config.toml`).
const _: Option<&str> = option_env!("GOOD_GALLERY_WORKTREE");

pub mod content;
pub mod edit;
pub mod exiftool;
pub mod face_models;
pub mod face_regions;
pub mod face_store;
pub mod ffmpeg;
pub mod helpers;
pub mod metadata;
pub mod original;
pub mod rating;
pub mod rename;
pub mod replace;
pub mod rotate;
pub mod subject;
pub mod tagging;
pub mod thumbnail;
pub mod update;
