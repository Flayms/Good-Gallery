//! Pure types and logic: media, tags, search model, gallery layout, date groups. No IO.

// Rebuilt when another worktree built into the same target folder (`.cargo/config.toml`).
const _: Option<&str> = option_env!("GOOD_GALLERY_WORKTREE");

pub mod ambient;
pub mod appearance;
pub mod autoscroll;
pub mod caption;
pub mod date_groups;
pub mod edit;
pub mod faces;
pub mod folders;
pub mod layout;
pub mod media;
pub mod search;
pub mod sidebar;
pub mod tags;
pub mod thumbs;
pub mod time_zone;
pub mod video;
pub mod view;
pub mod viewer;
pub mod zoom;
