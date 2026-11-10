//! Photo editing (spec 021): an edit is a [`Recipe`] of adjustments, rendered per pixel by [`Params::pixel`] from the
//! unedited original. The viewer's preview runs the same formulas on the GPU (`app::edit`'s shader mirrors `params`);
//! saving runs them here. Also the undo [`History`], the name of a saved copy and where backups of originals go.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

mod adjustment;
mod blur;
mod color;
mod curve;
mod geometry;
mod history;
mod mask;
mod params;
mod recipe;

pub use adjustment::*;
pub use blur::*;
pub use color::*;
pub use curve::*;
pub use geometry::*;
pub use history::*;
pub use mask::*;
pub use params::*;
pub use recipe::*;

/// The recipe format. Recipes of a newer version than this are not read (a newer app wrote them). A recipe is written
/// with the oldest version that describes it: 2 only with flips or straightening (REQ-029-10, REQ-029-11), which an
/// app that knows 1 would leave out.
pub const RECIPE_VERSION: u32 = 2;

/// Where originals replaced by an edit are kept, relative to their library (a housekeeping folder, so never indexed).
pub const BACKUP_DIR: &str = ".good-gallery/originals";

/// Samples per channel of the composed curve tables; looked up with linear interpolation.
pub const CURVE_SAMPLES: usize = 256;

/// At most this many points per curve, endpoints included.
pub const MAX_CURVE_POINTS: usize = 16;

/// How far the white balance sliders reach, in stops between red and blue (temperature) or green and the others
/// (tint).
const WHITE_BALANCE_STOPS: f32 = 1.0;

/// What saving does to a file's date modified: the setting _When saving edits_ (REQ-021-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MtimeOnSave {
    /// The original's date modified, so photos without a date taken keep their place.
    #[default]
    Keep,
    /// The time of saving.
    Update,
}

impl MtimeOnSave {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Update => "update",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "keep" => Some(Self::Keep),
            "update" => Some(Self::Update),
            _ => None,
        }
    }
}

/// The edit a file was saved with, read from its XMP (`gg:EditRecipe`, `gg:EditSource`) and kept in the index
/// (REQ-021-11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedEdit {
    /// The recipe's JSON ([`Recipe::to_json`]).
    pub recipe: String,
    /// The unedited original, relative to the library: the backup after _Save_, the source after _Save as copy_.
    pub source: String,
    /// Its versions as stored (`gg:EditHistory`, [`crate::versions`]); empty for none (#247).
    pub history: String,
}

impl SavedEdit {
    /// From a file's tags: both present, a recipe this version reads, and a source inside the library (REQ-011-08).
    pub fn parse(recipe: Option<&str>, source: Option<&str>) -> Option<Self> {
        let (recipe, source) = (recipe?, source?);
        (Recipe::from_json(recipe).is_some() && crate::media::is_safe_rel_path(source)).then(|| Self {
            recipe: recipe.to_owned(),
            source: source.to_owned(),
            history: String::new(),
        })
    }

    /// With the file's `gg:EditHistory`, if it has one.
    pub fn with_history(mut self, history: Option<&str>) -> Self {
        self.history = history.unwrap_or_default().to_owned();
        self
    }

    /// The versions the file was saved with, ending with its edit (REQ-021-24).
    pub fn versions(&self) -> Vec<crate::versions::Version> {
        crate::versions::of_file(&self.recipe(), &self.history)
    }

    pub fn recipe(&self) -> Recipe {
        Recipe::from_json(&self.recipe).unwrap_or_default()
    }

    /// Whether the edit replaced its original, which is then kept as a backup (REQ-021-12).
    pub fn replaced_original(&self, rel_path: &str) -> bool {
        self.source == backup_rel_path(rel_path)
    }
}

/// Whether a photo file can be edited, by extension (REQ-021-01): JPEG, PNG and WebP. GIFs may be animated, AVIF has
/// no encoder here.
pub fn is_editable(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    [".jpg", ".jpeg", ".png", ".webp"]
        .iter()
        .any(|ext| lower.len() > ext.len() && lower.ends_with(ext))
}

/// Where the unedited original of `rel_path` is kept once an edit replaced it (REQ-021-10).
pub fn backup_rel_path(rel_path: &str) -> String {
    format!("{BACKUP_DIR}/{rel_path}")
}

/// The file name of an edited copy of `file_name`: `IMG_1 (edited).jpg`, numbered from 2 while `taken` says a name is
/// in use (REQ-021-09).
pub fn copy_name(file_name: &str, taken: impl Fn(&str) -> bool) -> String {
    let (stem, ext) = match file_name.rfind('.') {
        Some(dot) if dot > 0 => file_name.split_at(dot),
        _ => (file_name, ""),
    };
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("{stem} (edited){ext}")
        } else {
            format!("{stem} (edited {n}){ext}")
        };
        if !taken(&name) {
            return name;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests;
