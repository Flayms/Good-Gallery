//! Content search (spec 036) without IO: the models a user can download, CLIP's tokenizer, a photo's input to a model,
//! and vectors with their similarity.

use regex::Regex;
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;

/// One file of a model, in the model's folder under `name`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelFile {
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

/// A model a user can download (REQ-036-01): its files, how it reads a photo, and when a photo matches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentModel {
    /// Stable, stored in the settings and beside each vector.
    pub id: &'static str,
    pub name: &'static str,
    /// The image half, the text half and the tokenizer, as [`IMAGE_FILE`], [`TEXT_FILE`] and [`TOKENIZER_FILE`].
    pub files: [ModelFile; 3],
    /// The square the image half reads, in pixels.
    pub side: u32,
    /// Subtracted from each channel (0–1), then divided by `std`.
    pub mean: [f32; 3],
    pub std: [f32; 3],
    /// The lowest similarity that counts as a match (REQ-036-06).
    pub cutoff: f32,
    pub license: &'static str,
    /// What it is there for, in a sentence.
    pub note: &'static str,
    pub recommended: bool,
}

impl ContentModel {
    /// All files together, for the download size.
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|file| file.bytes).sum()
    }
}

pub const IMAGE_FILE: &str = "image.onnx";
pub const TEXT_FILE: &str = "text.onnx";
pub const TOKENIZER_FILE: &str = "tokenizer.json";

/// The listed models (REQ-036-01). Their URLs name a fixed revision, and their hashes are checked after the download.
pub const CATALOG: [ContentModel; 2] = [
    ContentModel {
        id: "mobileclip-s0",
        name: "MobileCLIP-S0",
        files: [
            ModelFile {
                name: IMAGE_FILE,
                url: "https://huggingface.co/Xenova/mobileclip_s0/resolve/757d59c9c6870a76a4b0306f05f5061bca15c39f/onnx/vision_model.onnx",
                sha256: "17d3c037b1d488c10c50e09f6009ea5a198caef4e0e8f4ea5617b7cb2d067ac0",
                bytes: 45_543_630,
            },
            ModelFile {
                name: TEXT_FILE,
                url: "https://huggingface.co/Xenova/mobileclip_s0/resolve/757d59c9c6870a76a4b0306f05f5061bca15c39f/onnx/text_model.onnx",
                sha256: "f6e9bd5742bfc515889e901634d8a2ff2a57fab8564e4ad3760e800b1a51b77c",
                bytes: 169_807_789,
            },
            ModelFile {
                name: TOKENIZER_FILE,
                url: "https://huggingface.co/Xenova/mobileclip_s0/resolve/757d59c9c6870a76a4b0306f05f5061bca15c39f/tokenizer.json",
                sha256: "72ed5c96db5729294468543e4bc75fce14ca63f58e37300290189ba1c1e52b85",
                bytes: 2_224_081,
            },
        ],
        side: 256,
        mean: [0.0; 3],
        std: [1.0; 3],
        cutoff: 0.18,
        license: "Apple research license",
        note: "Small and fast, and the best at telling photos apart in our tests.",
        recommended: true,
    },
    ContentModel {
        id: "clip-vit-b32",
        name: "CLIP ViT-B/32",
        files: [
            ModelFile {
                name: IMAGE_FILE,
                url: "https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/6b7c523703ae26e39be4b03774966f8571c7f523/onnx/vision_model.onnx",
                sha256: "fd6e1402a588279d1723c7534d4bcba5bc0b14b47dfab0e46f8c47b8270d7d40",
                bytes: 351_685_709,
            },
            ModelFile {
                name: TEXT_FILE,
                url: "https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/6b7c523703ae26e39be4b03774966f8571c7f523/onnx/text_model.onnx",
                sha256: "3f6571f5bad13a97c469c1622e1cfc4d9aef78b79fdbfcff804ca357bfada8cc",
                bytes: 254_058_553,
            },
            ModelFile {
                name: TOKENIZER_FILE,
                url: "https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/6b7c523703ae26e39be4b03774966f8571c7f523/tokenizer.json",
                sha256: "f7f3b7af117d467b58374797691a6438d3e6b9e9cef800dfd5dced7f697a90cd",
                bytes: 2_224_119,
            },
        ],
        side: 224,
        mean: [0.481_454_66, 0.457_827_5, 0.408_210_73],
        std: [0.268_629_54, 0.261_302_58, 0.275_777_11],
        cutoff: 0.24,
        license: "MIT",
        note: "The original CLIP, with a clear license for any use; larger, and less sure between similar scenes.",
        recommended: false,
    },
];

/// The listed model with this id.
pub fn model(id: &str) -> Option<&'static ContentModel> {
    CATALOG.iter().find(|model| model.id == id)
}

/// The tokens a text half reads, padded with 0 (REQ-036-05).
pub const CONTEXT: usize = 77;
const START: i64 = 49406;
const END: i64 = 49407;

/// CLIP's tokenizer: byte-level BPE with `</w>` at word ends, on lowercase NFC text, from a model's `tokenizer.json`.
pub struct Tokenizer {
    vocab: HashMap<String, i64>,
    ranks: HashMap<(String, String), usize>,
    byte_char: [char; 256],
    words: Regex,
}

impl Tokenizer {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let json: serde_json::Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
        let model = &json["model"];
        let vocab = model["vocab"]
            .as_object()
            .ok_or("the tokenizer has no vocabulary")?
            .iter()
            .filter_map(|(piece, id)| Some((piece.clone(), id.as_i64()?)))
            .collect();
        let ranks = model["merges"]
            .as_array()
            .ok_or("the tokenizer has no merges")?
            .iter()
            .enumerate()
            .filter_map(|(rank, merge)| {
                let (a, b) = match merge {
                    serde_json::Value::String(pair) => {
                        let (a, b) = pair.split_once(' ')?;
                        (a.to_owned(), b.to_owned())
                    }
                    serde_json::Value::Array(pair) => {
                        (pair.first()?.as_str()?.to_owned(), pair.get(1)?.as_str()?.to_owned())
                    }
                    _ => return None,
                };
                Some(((a, b), rank))
            })
            .collect();
        let words =
            Regex::new(r"'s|'t|'re|'ve|'m|'ll|'d|\p{L}+|\p{N}|[^\s\p{L}\p{N}]+").map_err(|error| error.to_string())?;
        Ok(Self {
            vocab,
            ranks,
            byte_char: byte_chars(),
            words,
        })
    }

    /// The tokens of `text`: start, the pieces, end, then 0 up to [`CONTEXT`]. A text half pools at its end token,
    /// the highest id, so padding with it would make every text alike (`docs/known-pitfalls.md`).
    pub fn encode(&self, text: &str) -> Vec<i64> {
        let text: String = text
            .nfc()
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let text = text.to_lowercase();
        let mut ids = vec![START];
        for word in self.words.find_iter(&text) {
            for piece in self.pieces(word.as_str()) {
                ids.push(*self.vocab.get(&piece).unwrap_or(&END));
            }
        }
        ids.truncate(CONTEXT - 1);
        ids.push(END);
        ids.resize(CONTEXT, 0);
        ids
    }

    fn pieces(&self, word: &str) -> Vec<String> {
        let mut parts: Vec<String> = word
            .bytes()
            .map(|byte| self.byte_char[usize::from(byte)].to_string())
            .collect();
        if let Some(last) = parts.last_mut() {
            last.push_str("</w>");
        }
        loop {
            let best = parts
                .windows(2)
                .enumerate()
                .filter_map(|(i, pair)| {
                    self.ranks
                        .get(&(pair[0].clone(), pair[1].clone()))
                        .map(|rank| (*rank, i))
                })
                .min();
            let Some((_, i)) = best else { break };
            let merged = format!("{}{}", parts[i], parts[i + 1]);
            parts.splice(i..i + 2, [merged]);
        }
        parts
    }
}

/// GPT-2's map of bytes to printable characters, which byte-level BPE vocabularies are written in.
fn byte_chars() -> [char; 256] {
    let printable =
        |byte: u32| (33..=126).contains(&byte) || (161..=172).contains(&byte) || (174..=255).contains(&byte);
    let mut chars = [' '; 256];
    let mut next = 256;
    for byte in 0..256u32 {
        let code = if printable(byte) {
            byte
        } else {
            next += 1;
            next - 1
        };
        chars[byte as usize] = char::from_u32(code).unwrap_or(' ');
    }
    chars
}

/// Where to crop a `width` × `height` photo, scaled so its shorter side is `side`: the centered square's left and top
/// in the scaled photo, and the scaled size.
pub fn center_crop(width: u32, height: u32, side: u32) -> ([u32; 2], [u32; 2]) {
    let shorter = width.min(height).max(1) as f32;
    let scale = side as f32 / shorter;
    let scaled = [
        ((width as f32 * scale).round() as u32).max(side),
        ((height as f32 * scale).round() as u32).max(side),
    ];
    ([(scaled[0] - side) / 2, (scaled[1] - side) / 2], scaled)
}

/// A square of `model.side` RGB pixels as the image half reads it: channels first, 0–1, minus the mean, over the std.
pub fn image_input(rgb: &[u8], model: &ContentModel) -> Vec<f32> {
    let side = model.side as usize;
    let mut input = vec![0.0; 3 * side * side];
    for (i, pixel) in rgb.chunks_exact(3).take(side * side).enumerate() {
        for c in 0..3 {
            input[c * side * side + i] = (f32::from(pixel[c]) / 255.0 - model.mean[c]) / model.std[c];
        }
    }
    input
}

/// A vector as stored (REQ-036-06): normalized, each part times 127 as a signed byte.
pub fn quantize(vector: &[f32]) -> Vec<i8> {
    let length = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if length == 0.0 {
        return vec![0; vector.len()];
    }
    vector
        .iter()
        .map(|x| (x / length * 127.0).round().clamp(-127.0, 127.0) as i8)
        .collect()
}

/// A text's vector normalized, for comparing with stored ones.
pub fn normalized(vector: &[f32]) -> Vec<f32> {
    let length = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if length == 0.0 {
        return vec![0.0; vector.len()];
    }
    vector.iter().map(|x| x / length).collect()
}

/// The cosine similarity of a stored vector and a normalized text vector; 0 for vectors of different lengths.
pub fn similarity(stored: &[i8], text: &[f32]) -> f32 {
    if stored.len() != text.len() {
        return 0.0;
    }
    stored.iter().zip(text).map(|(a, b)| f32::from(*a) * b).sum::<f32>() / 127.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A vocabulary in CLIP's form: single byte characters, a few merges, `</w>` at word ends.
    fn tokenizer() -> Tokenizer {
        let json = serde_json::json!({
            "model": {
                "vocab": {
                    "g": 1, "r": 2, "a": 3, "s": 4, "s</w>": 5, "!</w>": 6, "gr": 7, "gra": 8, "ss</w>": 9,
                    "grass</w>": 10, "<|startoftext|>": 49406, "<|endoftext|>": 49407,
                },
                "merges": ["g r", "gr a", "s s</w>", "gra ss</w>"],
            }
        });
        Tokenizer::from_json(&json.to_string()).unwrap()
    }

    /// REQ-036-05: words are lowercased, split from punctuation, merged by the merges' order, and the tokens are
    /// framed by start and end and padded with 0.
    #[test]
    fn req_036_05_tokenizes_as_clip_does_and_pads_with_zero() {
        let ids = tokenizer().encode("  GRASS!  ");
        assert_eq!(ids.len(), CONTEXT);
        assert_eq!(&ids[..4], &[START, 10, 6, END]);
        assert!(ids[4..].iter().all(|&id| id == 0), "{ids:?}");
        // A piece the vocabulary doesn't have counts as unknown, not as nothing.
        assert_eq!(&tokenizer().encode("x")[..3], &[START, END, END]);
        let long = tokenizer().encode(&"grass ".repeat(100));
        assert_eq!((long.len(), long[CONTEXT - 1]), (CONTEXT, END));
    }

    /// REQ-036-05: GPT-2's byte map: printable ASCII as itself, a space as Ġ.
    #[test]
    fn req_036_05_maps_bytes_as_byte_level_bpe() {
        let chars = byte_chars();
        assert_eq!(chars[usize::from(b'a')], 'a');
        assert_eq!(chars[usize::from(b' ')], 'Ġ');
        assert_eq!(chars.iter().collect::<std::collections::HashSet<_>>().len(), 256);
    }

    /// REQ-036-01: each listed model has its three files, pinned by hash, and one is recommended.
    #[test]
    fn req_036_01_lists_complete_models() {
        assert_eq!(CATALOG.iter().filter(|model| model.recommended).count(), 1);
        for model in &CATALOG {
            let names: Vec<&str> = model.files.iter().map(|file| file.name).collect();
            assert_eq!(names, [IMAGE_FILE, TEXT_FILE, TOKENIZER_FILE], "{}", model.id);
            assert!(
                model
                    .files
                    .iter()
                    .all(|file| file.sha256.len() == 64 && file.url.starts_with("https://"))
            );
            assert!(model.cutoff > 0.0 && model.cutoff < 1.0 && model.bytes() > 0);
            assert_eq!(super::model(model.id), Some(model));
        }
    }

    /// REQ-036-04: the shorter side scales to the model's side, and the square is centered.
    #[test]
    fn req_036_04_crops_the_center_square() {
        assert_eq!(center_crop(800, 400, 256), ([128, 0], [512, 256]));
        assert_eq!(center_crop(300, 600, 224), ([0, 112], [224, 448]));
        assert_eq!(center_crop(256, 256, 256), ([0, 0], [256, 256]));
        let model = &CATALOG[1];
        let side = model.side as usize;
        let input = image_input(&vec![255; side * side * 3], model);
        assert_eq!(input.len(), 3 * side * side);
        assert!((input[0] - (1.0 - model.mean[0]) / model.std[0]).abs() < 1e-5);
        assert!((input[2 * side * side] - (1.0 - model.mean[2]) / model.std[2]).abs() < 1e-5);
    }

    /// REQ-036-06: stored vectors keep their direction in 8 bits: similar ones stay similar, and other lengths never
    /// match.
    #[test]
    fn req_036_06_compares_stored_vectors_with_text() {
        let photo = [0.3, -0.2, 0.9, 0.1];
        let stored = quantize(&photo);
        let same = similarity(&stored, &normalized(&photo));
        assert!((same - 1.0).abs() < 0.01, "{same}");
        let other = similarity(&stored, &normalized(&[-0.3, 0.2, 0.1, 0.9]));
        assert!(other < 0.2, "{other}");
        assert_eq!(similarity(&stored, &[1.0, 0.0]), 0.0);
        assert_eq!(quantize(&[0.0, 0.0]), [0, 0]);
    }
}
