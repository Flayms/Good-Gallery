//! LUTs (spec 043): three-dimensional color lookup tables in the `.cube` format, which other editors and colorists
//! share as looks. One maps an sRGB-encoded color to another; between its points it is interpolated.

use std::fmt;

/// The largest grid accepted: 65 points per side is what grading tools export at most.
pub const MAX_SIZE: usize = 65;

/// A 3D LUT, read from a `.cube` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Lut {
    /// Its `TITLE`, if it has one.
    pub title: Option<String>,
    /// Points per side.
    pub size: usize,
    /// `size³` colors, red changing fastest, then green, then blue.
    pub table: Vec<[f32; 3]>,
    domain_min: [f32; 3],
    domain_max: [f32; 3],
}

/// Why a file isn't a LUT the app can use (REQ-043-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LutError {
    /// A one-dimensional LUT changes each channel alone; the app takes 3D ones.
    OneDimensional,
    NoSize,
    /// Fewer than 2 or more than [`MAX_SIZE`] points per side.
    Size(usize),
    /// The number of colors doesn't match the size.
    Count {
        expected: usize,
        found: usize,
    },
    /// A line that is neither a keyword nor three numbers.
    Line(usize),
}

impl fmt::Display for LutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OneDimensional => write!(f, "it's a 1D LUT; only 3D LUTs can be used"),
            Self::NoSize => write!(f, "it has no LUT_3D_SIZE, so it isn't a 3D .cube LUT"),
            Self::Size(size) => write!(f, "its size of {size} isn't between 2 and {MAX_SIZE}"),
            Self::Count { expected, found } => write!(f, "it has {found} colors instead of {expected}"),
            Self::Line(line) => write!(f, "line {line} can't be read"),
        }
    }
}

impl Lut {
    /// Reads a `.cube` file's text (REQ-043-01).
    pub fn parse(text: &str) -> Result<Self, LutError> {
        let mut title = None;
        let mut size = None;
        let (mut domain_min, mut domain_max) = ([0.0; 3], [1.0; 3]);
        let mut table = Vec::new();
        let three = |words: &[&str]| -> Option<[f32; 3]> {
            match words {
                [r, g, b] => Some([r.parse().ok()?, g.parse().ok()?, b.parse().ok()?]),
                _ => None,
            }
        };
        for (number, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let words: Vec<&str> = line.split_whitespace().collect();
            let bad = LutError::Line(number + 1);
            match words[0] {
                "TITLE" => title = Some(line["TITLE".len()..].trim().trim_matches('"').to_owned()),
                "LUT_1D_SIZE" => return Err(LutError::OneDimensional),
                "LUT_3D_SIZE" => size = Some(words.get(1).and_then(|n| n.parse::<usize>().ok()).ok_or(bad)?),
                "DOMAIN_MIN" => domain_min = three(&words[1..]).ok_or(bad)?,
                "DOMAIN_MAX" => domain_max = three(&words[1..]).ok_or(bad)?,
                // Other keywords (LUT_IN_VIDEO_RANGE and the like) change nothing here.
                keyword if keyword.starts_with(|c: char| c.is_ascii_alphabetic()) => {}
                _ => table.push(three(&words).ok_or(bad)?),
            }
        }
        let size = size.ok_or(LutError::NoSize)?;
        if !(2..=MAX_SIZE).contains(&size) {
            return Err(LutError::Size(size));
        }
        let expected = size * size * size;
        if table.len() != expected {
            return Err(LutError::Count {
                expected,
                found: table.len(),
            });
        }
        Ok(Self {
            title: title.filter(|title| !title.is_empty()),
            size,
            table,
            domain_min,
            domain_max,
        })
    }

    /// `rgb` (sRGB-encoded, 0–1) through the table, interpolated between its points (REQ-043-02).
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let last = (self.size - 1) as f32;
        // Where the color falls on the grid, per channel: the point below and how far towards the next.
        let at: [(usize, f32); 3] = std::array::from_fn(|c| {
            let span = (self.domain_max[c] - self.domain_min[c]).max(1e-6);
            let x = ((rgb[c] - self.domain_min[c]) / span).clamp(0.0, 1.0) * last;
            let below = (x.floor() as usize).min(self.size - 2);
            (below, x - below as f32)
        });
        let point = |r: usize, g: usize, b: usize| self.table[r + self.size * (g + self.size * b)];
        let mut out = [0.0; 3];
        for corner in 0..8 {
            let pick = |c: usize| (corner >> c) & 1;
            let weight: f32 = (0..3)
                .map(|c| if pick(c) == 1 { at[c].1 } else { 1.0 - at[c].1 })
                .product();
            if weight == 0.0 {
                continue;
            }
            let value = point(at[0].0 + pick(0), at[1].0 + pick(1), at[2].0 + pick(2));
            for c in 0..3 {
                out[c] += value[c] * weight;
            }
        }
        out
    }

    /// The table as the GPU takes it: RGBA rows, red fastest, and the domain to scale colors onto it.
    pub fn domain(&self) -> ([f32; 3], [f32; 3]) {
        (self.domain_min, self.domain_max)
    }
}

/// A LUT's identity: the first 16 hex digits of its file's SHA-1, so the same file imported twice is one LUT and an
/// edit names exactly the table it was made with (REQ-043-03).
pub fn lut_id(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    Sha1::digest(bytes)
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The LUT an edit applies, by the file's id, and how strongly (REQ-043-02, REQ-043-03).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LutChoice {
    /// [`lut_id`] of the file.
    pub id: String,
    /// What the editor calls it: the LUT's title, else its file name.
    pub name: String,
    /// 0 to 100.
    pub strength: f32,
}

impl LutChoice {
    pub const STRENGTH: (f32, f32) = (0.0, 100.0);

    pub fn sanitize(&mut self) {
        self.strength = if self.strength.is_finite() {
            self.strength.clamp(Self::STRENGTH.0, Self::STRENGTH.1)
        } else {
            100.0
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-043-02, REQ-043-03: an edit's LUT goes first, under the adjustments, at its strength; it is kept with the
    /// recipe, and without its table on this computer the photo renders without it.
    #[test]
    fn req_043_02_req_043_03_an_edit_applies_its_lut_first_at_its_strength() {
        use crate::edit::{Params, Recipe};
        use std::sync::Arc;
        let swap = Arc::new(Lut::parse(SWAP).unwrap());
        let choice = LutChoice {
            id: lut_id(SWAP.as_bytes()),
            name: "Swap".into(),
            strength: 100.0,
        };
        let recipe = Recipe {
            lut: Some(choice.clone()),
            ..Recipe::default()
        };
        assert!(!recipe.is_neutral());
        let json = serde_json::to_string(&recipe).unwrap();
        assert_eq!(serde_json::from_str::<Recipe>(&json).unwrap().lut, Some(choice.clone()));

        let red = [0.8, 0.1, 0.1];
        let at = |params: &Params| params.pixel(red, [0.5, 0.5], [100.0, 100.0]);
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 2e-3);
        assert!(close(
            at(&Params::new(&recipe).with_lut(Some(swap.clone()))),
            [0.1, 0.1, 0.8]
        ));
        let half = Recipe {
            lut: Some(LutChoice {
                strength: 50.0,
                ..choice
            }),
            ..Recipe::default()
        };
        assert!(close(
            at(&Params::new(&half).with_lut(Some(swap.clone()))),
            [0.45, 0.1, 0.45]
        ));
        assert!(close(at(&Params::new(&recipe)), red), "the table isn't here");
        // Under the adjustments: more exposure brightens what the LUT made.
        let brighter = Recipe {
            exposure: 1.0,
            ..recipe
        };
        assert!(at(&Params::new(&brighter).with_lut(Some(swap)))[2] > 0.85);
    }

    /// A 2-point LUT that swaps red and blue.
    const SWAP: &str = "# swaps red and blue\nTITLE \"Swap\"\nLUT_3D_SIZE 2\n\n\
        0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n";

    #[test]
    fn req_043_01_reads_a_cube_file() {
        let lut = Lut::parse(SWAP).unwrap();
        assert_eq!((lut.title.as_deref(), lut.size, lut.table.len()), (Some("Swap"), 2, 8));
        assert_eq!(lut.table[1], [0.0, 0.0, 1.0], "red changes fastest");
    }

    #[test]
    fn req_043_01_says_why_a_file_cant_be_used() {
        assert_eq!(Lut::parse("LUT_1D_SIZE 4\n0 0 0\n"), Err(LutError::OneDimensional));
        assert_eq!(Lut::parse("0 0 0\n"), Err(LutError::NoSize));
        assert_eq!(Lut::parse("LUT_3D_SIZE 1\n0 0 0\n"), Err(LutError::Size(1)));
        assert_eq!(
            Lut::parse("LUT_3D_SIZE 2\n0 0 0\n"),
            Err(LutError::Count { expected: 8, found: 1 })
        );
        assert_eq!(Lut::parse("LUT_3D_SIZE 2\n0 zero 0\n"), Err(LutError::Line(2)));
        assert_eq!(
            Lut::parse("\u{feff}LUT_3D_SIZE 2\r\n0 0 0\r\n").unwrap_err(),
            LutError::Count { expected: 8, found: 1 },
            "a BOM and CRLF"
        );
    }

    #[test]
    fn req_043_02_maps_colors_and_interpolates_between_points() {
        let swap = Lut::parse(SWAP).unwrap();
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5);
        assert!(close(swap.apply([1.0, 0.0, 0.0]), [0.0, 0.0, 1.0]));
        assert!(
            close(swap.apply([0.2, 0.5, 0.9]), [0.9, 0.5, 0.2]),
            "between the points"
        );
        assert!(
            close(swap.apply([-1.0, 2.0, 0.5]), [0.5, 1.0, 0.0]),
            "outside 0–1, clamped"
        );
        // A domain other than 0–1 scales the input onto the grid.
        let wide = "LUT_3D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
        assert!(close(
            Lut::parse(wide).unwrap().apply([1.0, 0.5, 0.0]),
            [0.5, 0.25, 0.0]
        ));
    }

    #[test]
    fn req_043_03_the_same_file_has_the_same_id() {
        assert_eq!(lut_id(SWAP.as_bytes()), lut_id(SWAP.as_bytes()));
        assert_ne!(lut_id(SWAP.as_bytes()), lut_id(b"LUT_3D_SIZE 2\n"));
        assert_eq!(lut_id(SWAP.as_bytes()).len(), 16);
    }
}
