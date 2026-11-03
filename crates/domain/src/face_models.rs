//! Finding and recognizing faces (spec 031), without IO: the models on offer, what each expects, and the steps around
//! them. `media::face_models` runs them.

/// The input of every detection pass, in pixels; these models are at their best near it, and YuNet's file is fixed to it.
pub const INPUT_SIDE: usize = 640;
/// The input of every recognition model, in pixels.
pub const CROP_SIDE: usize = 112;
/// Faces smaller than this on the photo have too little in them to recognize.
pub const MIN_FACE_PIXELS: f32 = 24.0;
/// Where the eyes, nose and mouth corners lie in the crop (`arcface_dst` in InsightFace, the same in OpenCV's `alignCrop`).
pub const TEMPLATE: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

/// A face a detector found: its box (`x, y, w, h`), eyes, nose and mouth corners, in the pixels of what it looked at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detection {
    pub rect: [f32; 4],
    pub landmarks: [[f32; 2]; 5],
    pub score: f32,
}

/// What a detection model's output looks like, which decides its decoding and how its input is prepared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectorFormat {
    YuNet,
    RetinaFace,
}

impl DetectorFormat {
    /// The score below which a detection is dropped. Not calibrated yet (spec 031's benchmark).
    pub fn min_score(self) -> f32 {
        match self {
            DetectorFormat::YuNet => 0.8,
            DetectorFormat::RetinaFace => 0.7,
        }
    }
}

/// How a recognition model expects its RGB pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputStyle {
    /// Scaled to −1…1: most models trained as ArcFace is.
    ArcFace,
    /// 0–255, unscaled.
    SFace,
}

impl InputStyle {
    pub fn value(self, channel: u8) -> f32 {
        let channel = f32::from(channel);
        match self {
            InputStyle::ArcFace => (channel - 127.5) / 127.5,
            InputStyle::SFace => channel,
        }
    }
}

/// What a model does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelKind {
    Detect(DetectorFormat),
    /// `threshold`: how similar two faces must be to suggest they are the same person.
    Recognize {
        style: InputStyle,
        threshold: f32,
    },
}

/// The threshold of a recognition model the catalog doesn't know.
pub const DEFAULT_THRESHOLD: f32 = 0.4;

/// A model with a download button. The file is pinned by revision and hash, so a host can't swap it.
#[derive(Debug, Clone, Copy)]
pub struct CatalogModel {
    pub name: &'static str,
    pub kind: ModelKind,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub license: &'static str,
}

pub const CATALOG: &[CatalogModel] = &[
    CatalogModel {
        name: "YuNet",
        kind: ModelKind::Detect(DetectorFormat::YuNet),
        url: "https://huggingface.co/opencv/face_detection_yunet/resolve/3cc26e7f1014a5ee5d74a42acee58bafc9d0a310/face_detection_yunet_2023mar.onnx",
        sha256: "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
        bytes: 232_589,
        license: "MIT",
    },
    CatalogModel {
        name: "RetinaFace (MobileNetV2)",
        kind: ModelKind::Detect(DetectorFormat::RetinaFace),
        url: "https://github.com/yakhyo/retinaface-pytorch/releases/download/v0.0.1/retinaface_mv2.onnx",
        sha256: "3ca44c045651cabeed1193a1fae8946ad1f3a55da8fa74b341feab5a8319f757",
        bytes: 12_506_811,
        license: "MIT, see the comparison page",
    },
    CatalogModel {
        name: "RetinaFace (ResNet34)",
        kind: ModelKind::Detect(DetectorFormat::RetinaFace),
        url: "https://github.com/yakhyo/retinaface-pytorch/releases/download/v0.0.1/retinaface_r34.onnx",
        sha256: "bd0263dc2a465d32859555cb1741f2d98991eb0053696e8ee33fec583d30e630",
        bytes: 88_502_283,
        license: "MIT, see the comparison page",
    },
    CatalogModel {
        name: "SFace",
        kind: ModelKind::Recognize {
            style: InputStyle::SFace,
            threshold: 0.363,
        },
        url: "https://huggingface.co/opencv/face_recognition_sface/resolve/3d7082438a6e4551e840c9b2bb60b71e8da4b524/face_recognition_sface_2021dec.onnx",
        sha256: "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79",
        bytes: 38_696_353,
        license: "Apache-2.0",
    },
    CatalogModel {
        name: "AuraFace",
        kind: ModelKind::Recognize {
            style: InputStyle::ArcFace,
            threshold: DEFAULT_THRESHOLD,
        },
        url: "https://huggingface.co/fal/AuraFace-v1/resolve/af6d057c9b0ec4071d4c49c80e3539258798b609/glintr100.onnx",
        sha256: "a7933ea5330113b01c9b60351d8f4c33003f145d8470ac5f0e52ee2effe25c60",
        bytes: 260_694_151,
        license: "Apache-2.0, see the comparison page",
    },
];

/// How much two boxes (`x, y, w, h`) overlap, as the part of their union.
fn intersection(a: [f32; 4], b: [f32; 4]) -> f32 {
    let w = (a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0]);
    let h = (a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1]);
    w.max(0.0) * h.max(0.0)
}

/// Keeps the best of detections that overlap, best first (REQ-031-06).
pub fn suppress(mut found: Vec<Detection>) -> Vec<Detection> {
    const MAX_OVERLAP: f32 = 0.3;
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Detection> = Vec::new();
    for face in found {
        let overlaps = kept.iter().any(|other| {
            let shared = intersection(face.rect, other.rect);
            let union = face.rect[2] * face.rect[3] + other.rect[2] * other.rect[3] - shared;
            union > 0.0 && shared / union > MAX_OVERLAP
        });
        if !overlaps {
            kept.push(face);
        }
    }
    kept
}

/// The output of YuNet at [`INPUT_SIDE`], per stride 8, 16, 32 (as OpenCV's `FaceDetectorYN` reads it).
pub fn decode_yunet(
    cls: &[Vec<f32>; 3],
    obj: &[Vec<f32>; 3],
    bbox: &[Vec<f32>; 3],
    kps: &[Vec<f32>; 3],
    min_score: f32,
) -> Vec<Detection> {
    let mut found = Vec::new();
    for (level, stride) in [8usize, 16, 32].into_iter().enumerate() {
        let columns = INPUT_SIDE / stride;
        let stride = stride as f32;
        for cell in 0..cls[level].len().min(obj[level].len()) {
            let score = (cls[level][cell].clamp(0.0, 1.0) * obj[level][cell].clamp(0.0, 1.0)).sqrt();
            let (Some(box_), Some(points)) = (
                bbox[level].get(cell * 4..cell * 4 + 4),
                kps[level].get(cell * 10..cell * 10 + 10),
            ) else {
                continue;
            };
            if score < min_score {
                continue;
            }
            let (column, row) = ((cell % columns) as f32, (cell / columns) as f32);
            let (w, h) = (box_[2].exp() * stride, box_[3].exp() * stride);
            let (cx, cy) = ((column + box_[0]) * stride, (row + box_[1]) * stride);
            let landmarks =
                std::array::from_fn(|n| [(points[2 * n] + column) * stride, (points[2 * n + 1] + row) * stride]);
            found.push(Detection {
                rect: [cx - w / 2.0, cy - h / 2.0, w, h],
                landmarks,
                score,
            });
        }
    }
    found
}

/// RetinaFace's anchors at [`INPUT_SIDE`] (`cx, cy, w, h`, as fractions), in the order of its outputs.
pub fn retinaface_priors() -> Vec<[f32; 4]> {
    let side = INPUT_SIDE as f32;
    let mut priors = Vec::new();
    for (stride, sizes) in [(8usize, [16.0, 32.0]), (16, [64.0, 128.0]), (32, [256.0, 512.0])] {
        let cells = INPUT_SIDE.div_ceil(stride);
        for row in 0..cells {
            for column in 0..cells {
                for size in sizes {
                    priors.push([
                        (column as f32 + 0.5) * stride as f32 / side,
                        (row as f32 + 0.5) * stride as f32 / side,
                        size / side,
                        size / side,
                    ]);
                }
            }
        }
    }
    priors
}

/// RetinaFace's output at [`INPUT_SIDE`]: per anchor a box, background and face scores, and landmarks.
pub fn decode_retinaface(
    loc: &[f32],
    conf: &[f32],
    land: &[f32],
    priors: &[[f32; 4]],
    min_score: f32,
) -> Vec<Detection> {
    const VARIANCE: [f32; 2] = [0.1, 0.2];
    let side = INPUT_SIDE as f32;
    let mut found = Vec::new();
    for (n, [px, py, pw, ph]) in priors.iter().copied().enumerate() {
        let (Some(&score), Some(box_), Some(points)) = (
            conf.get(n * 2 + 1),
            loc.get(n * 4..n * 4 + 4),
            land.get(n * 10..n * 10 + 10),
        ) else {
            break;
        };
        if score < min_score {
            continue;
        }
        let (cx, cy) = (px + box_[0] * VARIANCE[0] * pw, py + box_[1] * VARIANCE[0] * ph);
        let (w, h) = (pw * (box_[2] * VARIANCE[1]).exp(), ph * (box_[3] * VARIANCE[1]).exp());
        let landmarks = std::array::from_fn(|l| {
            [
                (px + points[2 * l] * VARIANCE[0] * pw) * side,
                (py + points[2 * l + 1] * VARIANCE[0] * ph) * side,
            ]
        });
        found.push(Detection {
            rect: [(cx - w / 2.0) * side, (cy - h / 2.0) * side, w * side, h * side],
            landmarks,
            score,
        });
    }
    found
}

/// One look at a photo: the square of it starting at `x, y`, `side` photo pixels wide, shrunk to `input` pixels in the
/// top-left of the model's square input. Past the photo's edge the input stays black.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pass {
    pub x: f32,
    pub y: f32,
    pub side: f32,
    pub input: f32,
}

impl Pass {
    /// What this pass found, in photo pixels.
    pub fn to_photo(&self, found: Detection) -> Detection {
        let scale = self.side / self.input;
        let [x, y, w, h] = found.rect;
        Detection {
            rect: [self.x + x * scale, self.y + y * scale, w * scale, h * scale],
            landmarks: found
                .landmarks
                .map(|[px, py]| [self.x + px * scale, self.y + py * scale]),
            score: found.score,
        }
    }
}

/// The whole photo, which every photo gets.
pub fn first_pass(width: u32, height: u32) -> Pass {
    Pass {
        x: 0.0,
        y: 0.0,
        side: width.max(height) as f32,
        input: INPUT_SIDE as f32,
    }
}

/// How sure a detector must be for a first look to count as a hint that faces may be there.
pub const HINT_SCORE: f32 = 0.3;

/// The looks a photo still needs after its first, which found `confident` faces and weaker `hints` (in photo pixels,
/// `hints` includes `confident`). Most photos need few, so a large library stays affordable. No confident face: perhaps
/// one too large for the model (YuNet is trained up to about 300 px), so the photo again at half the size. Small hints:
/// faces too small to be sure of, or more beside them, so overlapping tiles.
pub fn more_passes(width: u32, height: u32, confident: &[Detection], hints: &[Detection]) -> Vec<Pass> {
    /// A face smaller than this in the first pass suggests there are smaller ones.
    const SMALL_IN_INPUT: f32 = 40.0;
    let first = first_pass(width, height);
    let mut passes = Vec::new();
    if confident.is_empty() {
        passes.push(Pass {
            input: INPUT_SIDE as f32 / 2.0,
            ..first
        });
    }
    let scale = first.input / first.side;
    let small = hints
        .iter()
        .any(|face| face.rect[2].min(face.rect[3]) * scale < SMALL_IN_INPUT);
    if !small || first.side <= INPUT_SIDE as f32 * 1.5 {
        return passes;
    }
    let side = (first.side * 0.6).max(INPUT_SIDE as f32);
    let starts = |length: f32| {
        let mut starts = vec![0.0_f32];
        while starts[starts.len() - 1] + side < length {
            starts.push((starts[starts.len() - 1] + side * 0.8).min(length - side));
        }
        starts
    };
    for y in starts(height as f32) {
        for x in starts(width as f32) {
            passes.push(Pass {
                x,
                y,
                side,
                input: INPUT_SIDE as f32,
            });
        }
    }
    passes
}

/// Drops faces too small to recognize.
pub fn keep_recognizable(mut found: Vec<Detection>) -> Vec<Detection> {
    found.retain(|face| face.rect[2].min(face.rect[3]) >= MIN_FACE_PIXELS);
    found
}

/// Turns, scales and moves a face's landmarks as close as possible onto [`TEMPLATE`], without skewing it.
#[derive(Debug, Clone, Copy)]
pub struct Alignment {
    a: f32,
    b: f32,
    tx: f32,
    ty: f32,
}

impl Alignment {
    /// The least-squares fit of the five points.
    pub fn new(landmarks: &[[f32; 2]; 5]) -> Self {
        let mean = |points: &[[f32; 2]; 5]| {
            let [x, y] = points.iter().fold([0.0, 0.0], |sum, p| [sum[0] + p[0], sum[1] + p[1]]);
            [x / 5.0, y / 5.0]
        };
        let (from, to) = (mean(landmarks), mean(&TEMPLATE));
        let (mut a, mut b, mut length) = (0.0, 0.0, 0.0);
        for (source, target) in landmarks.iter().zip(TEMPLATE) {
            let (sx, sy) = (source[0] - from[0], source[1] - from[1]);
            let (dx, dy) = (target[0] - to[0], target[1] - to[1]);
            a += sx * dx + sy * dy;
            b += sx * dy - sy * dx;
            length += sx * sx + sy * sy;
        }
        let (a, b) = if length > f32::EPSILON {
            (a / length, b / length)
        } else {
            (1.0, 0.0)
        };
        Self {
            a,
            b,
            tx: to[0] - (a * from[0] - b * from[1]),
            ty: to[1] - (b * from[0] + a * from[1]),
        }
    }

    /// From the photo to the crop.
    pub fn apply(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [self.a * x - self.b * y + self.tx, self.b * x + self.a * y + self.ty]
    }

    /// From the crop back to the photo.
    pub fn invert(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        let (dx, dy) = (x - self.tx, y - self.ty);
        let det = (self.a * self.a + self.b * self.b).max(f32::EPSILON);
        [(self.a * dx + self.b * dy) / det, (self.a * dy - self.b * dx) / det]
    }
}

/// Scales to length 1; all zeros stay.
pub fn normalize(vector: &mut [f32]) {
    let length = vector.iter().map(|n| n * n).sum::<f32>().sqrt();
    if length > 0.0 {
        vector.iter_mut().for_each(|n| *n /= length);
    }
}

/// Similarity of two normalized vectors, 1 for the same; −1, the least, for vectors of different sizes.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return -1.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Whether a detected box and a region (fractions of the photo) mark the same face. Programs draw boxes differently,
/// so it is enough that most of the smaller lies in the larger, if they are of similar size.
pub fn same_face(a: [f32; 4], b: [f32; 4]) -> bool {
    const MIN_COVERED: f32 = 0.5;
    const MAX_SIZE_RATIO: f32 = 6.0;
    let (first, second) = (a[2] * a[3], b[2] * b[3]);
    let (smaller, larger) = (first.min(second), first.max(second));
    smaller > 0.0 && larger / smaller <= MAX_SIZE_RATIO && intersection(a, b) / smaller >= MIN_COVERED
}

/// How similar `face` is to a person: the mean of the best few of their faces, so one mislabeled face counts for little.
const BEST_FEW: usize = 3;

/// The person most similar to `face`, if similar enough and not `rejected` (REQ-031-10).
pub fn suggest<'a>(
    face: &[f32],
    known: &[(&'a str, &[f32])],
    threshold: f32,
    rejected: &[&str],
) -> Option<(&'a str, f32)> {
    let mut people: Vec<(&'a str, Vec<f32>)> = Vec::new();
    for (name, vector) in known {
        if rejected.contains(name) {
            continue;
        }
        let similarity = cosine(face, vector);
        match people.iter_mut().find(|(other, _)| other == name) {
            Some((_, similarities)) => similarities.push(similarity),
            None => people.push((name, vec![similarity])),
        }
    }
    people
        .into_iter()
        .map(|(name, mut similarities)| {
            similarities.sort_by(|a, b| b.total_cmp(a));
            similarities.truncate(BEST_FEW);
            (name, similarities.iter().sum::<f32>() / similarities.len() as f32)
        })
        .filter(|(_, similarity)| *similarity >= threshold)
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// Groups similar faces, largest first, leaving out groups smaller than `min_size` (REQ-031-15). Each face joins the
/// group whose mean it is closest to, so the work grows with faces × groups.
pub fn group(faces: &[&[f32]], threshold: f32, min_size: usize) -> Vec<Vec<usize>> {
    let mut groups: Vec<(Vec<f32>, Vec<f32>, Vec<usize>)> = Vec::new();
    for (index, face) in faces.iter().enumerate() {
        let best = groups
            .iter()
            .enumerate()
            .map(|(n, (mean, _, _))| (n, cosine(face, mean)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, similarity)| *similarity >= threshold);
        match best {
            Some((n, _)) => {
                let (mean, sum, members) = &mut groups[n];
                sum.iter_mut()
                    .zip(face.iter())
                    .for_each(|(total, value)| *total += value);
                mean.clone_from(sum);
                normalize(mean);
                members.push(index);
            }
            None => groups.push((face.to_vec(), face.to_vec(), vec![index])),
        }
    }
    let mut found: Vec<Vec<usize>> = groups
        .into_iter()
        .map(|(_, _, members)| members)
        .filter(|members| members.len() >= min_size)
        .collect();
    found.sort_by_key(|members| std::cmp::Reverse(members.len()));
    found
}

/// Whether known faces look no more alike within a person than between people, which suggests a model set up wrongly
/// (REQ-031-17). `false` while there are too few to say: at least two people with three faces each.
pub fn tells_nobody_apart(known: &[(&str, &[f32])]) -> bool {
    const MIN_FACES: usize = 3;
    const MAX_FACES: usize = 20;
    const MIN_DIFFERENCE: f32 = 0.1;
    let mut people: Vec<(&str, Vec<&[f32]>)> = Vec::new();
    for (name, vector) in known {
        match people.iter_mut().find(|(other, _)| other == name) {
            Some((_, faces)) => faces.push(vector),
            None => people.push((name, vec![vector])),
        }
    }
    people.retain(|(_, faces)| faces.len() >= MIN_FACES);
    if people.len() < 2 {
        return false;
    }
    let (mut same, mut same_pairs, mut other, mut other_pairs) = (0.0, 0u32, 0.0, 0u32);
    for (n, (_, faces)) in people.iter().enumerate() {
        let faces = &faces[..faces.len().min(MAX_FACES)];
        for (i, a) in faces.iter().enumerate() {
            for b in &faces[i + 1..] {
                same += cosine(a, b);
                same_pairs += 1;
            }
            for (_, others) in &people[n + 1..] {
                for b in &others[..others.len().min(MAX_FACES)] {
                    other += cosine(a, b);
                    other_pairs += 1;
                }
            }
        }
    }
    let difference = same / same_pairs.max(1) as f32 - other / other_pairs.max(1) as f32;
    difference < MIN_DIFFERENCE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    fn face(x: f32, y: f32, w: f32, h: f32, score: f32) -> Detection {
        Detection {
            rect: [x, y, w, h],
            landmarks: [[0.0; 2]; 5],
            score,
        }
    }

    #[test]
    fn req_031_06_finds_the_same_face_only_once() {
        let kept = suppress(vec![
            face(10.0, 10.0, 100.0, 100.0, 0.7),
            face(12.0, 12.0, 100.0, 100.0, 0.9),
            face(300.0, 10.0, 100.0, 100.0, 0.8),
        ]);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].score, 0.9);
        assert_eq!(kept[1].score, 0.8);
    }

    #[test]
    fn req_031_06_decodes_yunets_output() {
        // One cell of the coarsest grid (stride 32, 20 × 20), row 1 and column 2, is a face.
        let grid = |side: usize, len: usize| vec![0.0; side * side * len];
        let mut cls = [grid(80, 1), grid(40, 1), grid(20, 1)];
        let mut obj = cls.clone();
        let mut bbox = [grid(80, 4), grid(40, 4), grid(20, 4)];
        let mut kps = [grid(80, 10), grid(40, 10), grid(20, 10)];
        let cell = 22;
        cls[2][cell] = 0.81;
        obj[2][cell] = 1.0;
        bbox[2][cell * 4..cell * 4 + 4].copy_from_slice(&[0.5, 0.5, 0.0, 1.0_f32.ln()]);
        kps[2][cell * 10..cell * 10 + 2].copy_from_slice(&[0.25, 0.75]);
        let found = decode_yunet(&cls, &obj, &bbox, &kps, 0.5);
        assert_eq!(found.len(), 1);
        let face = &found[0];
        assert!(close(face.score, 0.9), "the geometric mean: {}", face.score);
        // Center (2.5, 1.5) cells and 32 px wide and high.
        assert_eq!(face.rect.map(|n| n.round()), [64.0, 32.0, 32.0, 32.0]);
        assert_eq!(face.landmarks[0], [72.0, 56.0]);
    }

    #[test]
    fn req_031_06_decodes_retinafaces_output() {
        let priors = retinaface_priors();
        // Three sizes of grids with two anchors each.
        assert_eq!(priors.len(), 2 * (80 * 80 + 40 * 40 + 20 * 20));
        // The first anchor: cell (0, 0) of stride 8, 16 px, in fractions of 640.
        assert_eq!(
            priors[0],
            [0.5 * 8.0 / 640.0, 0.5 * 8.0 / 640.0, 16.0 / 640.0, 16.0 / 640.0]
        );

        let n = priors.len();
        let (mut loc, mut conf, mut land) = (vec![0.0; n * 4], vec![0.0; n * 2], vec![0.0; n * 10]);
        conf[1] = 0.9;
        loc[2] = 0.0;
        land[0] = 10.0; // 10 × 0.1 × the anchor's width right of its center
        let found = decode_retinaface(&loc, &conf, &land, &priors, 0.5);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rect, [-4.0, -4.0, 16.0, 16.0]);
        assert_eq!(found[0].landmarks[0], [20.0, 4.0]);
    }

    /// REQ-031-06: small faces are looked for closer only where the whole photo hints at them, and a face that fills
    /// the photo is looked for with the photo smaller.
    #[test]
    fn req_031_06_chooses_passes_by_what_the_first_found() {
        let first = Pass {
            x: 0.0,
            y: 0.0,
            side: 4000.0,
            input: INPUT_SIDE as f32,
        };
        assert_eq!(first_pass(4000, 3000), first);
        let half = Pass { input: 320.0, ..first };
        assert_eq!(more_passes(4000, 3000, &[], &[]), vec![half]);

        let big = [face(100.0, 100.0, 2000.0, 2000.0, 0.9)];
        assert!(more_passes(4000, 3000, &big, &big).is_empty());

        let small = [face(100.0, 100.0, 100.0, 100.0, 0.9)];
        let tiles = more_passes(4000, 3000, &small, &small);
        assert!(tiles.len() >= 4, "{tiles:?}");
        // Together the tiles cover the photo, overlapping so a face on an edge lies whole in one.
        assert!(
            tiles
                .iter()
                .all(|tile| tile.side < 2500.0 && tile.input == INPUT_SIDE as f32)
        );
        assert!(tiles.iter().any(|tile| tile.x + tile.side >= 4000.0));
        assert!(tiles.iter().any(|tile| tile.y + tile.side >= 3000.0));
        assert!(tiles[1].x < tiles[0].x + tiles[0].side - 100.0);
        // A small photo has nothing more to show.
        let tiny = [face(0.0, 0.0, 30.0, 30.0, 0.9)];
        assert!(more_passes(640, 480, &tiny, &tiny).is_empty());

        // Only a weak hint of a small face is enough to look closer, and the half-size look still happens.
        let weak = more_passes(4000, 3000, &[], &small);
        assert_eq!(weak[0], half);
        assert_eq!(weak.len(), tiles.len() + 1);
    }

    #[test]
    fn req_031_06_turns_what_a_pass_found_into_photo_pixels_and_drops_tiny_faces() {
        let tile = Pass {
            x: 1000.0,
            y: 500.0,
            side: 1280.0,
            input: 640.0,
        };
        let mut found = face(10.0, 20.0, 100.0, 100.0, 0.9);
        found.landmarks[0] = [50.0, 60.0];
        let placed = tile.to_photo(found);
        assert_eq!(placed.rect, [1020.0, 540.0, 200.0, 200.0]);
        assert_eq!(placed.landmarks[0], [1100.0, 620.0]);

        let kept = keep_recognizable(vec![
            face(0.0, 0.0, MIN_FACE_PIXELS - 1.0, 100.0, 0.9),
            face(0.0, 0.0, MIN_FACE_PIXELS, MIN_FACE_PIXELS, 0.9),
        ]);
        assert_eq!(kept.len(), 1);
    }

    /// REQ-031-07: the transform moves the landmarks onto the template, whatever the face's place, size and tilt.
    #[test]
    fn req_031_07_aligns_a_turned_face_onto_the_template() {
        let (angle, scale): (f32, f32) = (0.5, 3.0);
        let (cos, sin) = (angle.cos(), angle.sin());
        let landmarks =
            TEMPLATE.map(|[x, y]| [scale * (cos * x - sin * y) + 200.0, scale * (sin * x + cos * y) + 90.0]);
        let align = Alignment::new(&landmarks);
        for (point, wanted) in landmarks.iter().zip(TEMPLATE) {
            let [x, y] = align.apply(*point);
            assert!(close(x, wanted[0]) && close(y, wanted[1]), "{x},{y} for {wanted:?}");
        }
        // From the crop back to the photo, as sampling the crop needs.
        let [x, y] = align.invert([TEMPLATE[2][0], TEMPLATE[2][1]]);
        assert!(close(x, landmarks[2][0]) && close(y, landmarks[2][1]));
    }

    #[test]
    fn req_031_07_prepares_pixels_as_the_style_expects() {
        assert_eq!(InputStyle::ArcFace.value(255), 1.0);
        assert_eq!(InputStyle::ArcFace.value(0), -1.0);
        assert_eq!(InputStyle::SFace.value(200), 200.0);
    }

    #[test]
    fn req_031_07_normalizes_and_compares_only_the_same_size() {
        let mut vector = vec![3.0, 4.0];
        normalize(&mut vector);
        assert!(close(vector[0], 0.6) && close(vector[1], 0.8));
        assert!(close(cosine(&vector, &vector), 1.0));
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0, 0.0]), -1.0);
        let mut zero = vec![0.0, 0.0];
        normalize(&mut zero);
        assert_eq!(zero, [0.0, 0.0]);
    }

    #[test]
    fn req_031_08_a_loose_region_of_another_program_is_the_same_face() {
        let detected = [0.40, 0.40, 0.10, 0.10];
        assert!(same_face(detected, [0.38, 0.36, 0.16, 0.18]));
        assert!(same_face(detected, [0.42, 0.42, 0.08, 0.08]));
        assert!(!same_face(detected, [0.60, 0.40, 0.10, 0.10]));
        assert!(!same_face(detected, [0.47, 0.47, 0.10, 0.10]));
        // A region around a whole group isn't this face.
        assert!(!same_face(detected, [0.0, 0.0, 1.0, 1.0]));
    }

    fn unit(angle: f32) -> Vec<f32> {
        vec![angle.cos(), angle.sin()]
    }

    #[test]
    fn req_031_10_suggests_the_most_similar_person_unless_rejected() {
        let (anna, bob) = (unit(0.0), unit(1.5));
        let known = [
            ("Anna", anna.as_slice()),
            ("Anna", anna.as_slice()),
            ("Bob", bob.as_slice()),
        ];
        let suggestion = suggest(&unit(0.1), &known, 0.5, &[]).unwrap();
        assert_eq!(suggestion.0, "Anna");
        assert_eq!(suggest(&unit(0.1), &known, 0.5, &["Anna"]), None);
        assert_eq!(suggest(&unit(0.8), &known, 0.9, &[]), None);
        assert_eq!(suggest(&unit(0.1), &[], 0.5, &[]), None);
    }

    /// REQ-031-10: one mislabeled face doesn't outweigh a person's other faces.
    #[test]
    fn req_031_10_one_wrong_face_of_a_person_counts_for_little() {
        let (wrong, right, far) = (unit(0.0), unit(0.2), unit(1.5));
        let known = [
            ("Bob", wrong.as_slice()),
            ("Bob", far.as_slice()),
            ("Bob", far.as_slice()),
            ("Anna", right.as_slice()),
            ("Anna", right.as_slice()),
            ("Anna", right.as_slice()),
        ];
        assert_eq!(suggest(&unit(0.0), &known, 0.3, &[]).unwrap().0, "Anna");
    }

    #[test]
    fn req_031_15_groups_similar_faces_largest_first_without_small_groups() {
        let faces = [
            unit(0.0),
            unit(1.5),
            unit(0.02),
            unit(1.52),
            unit(0.04),
            unit(1.54),
            unit(1.56),
            unit(3.0),
            unit(3.02),
        ];
        let refs: Vec<&[f32]> = faces.iter().map(Vec::as_slice).collect();
        assert_eq!(group(&refs, 0.9, 3), vec![vec![1, 3, 5, 6], vec![0, 2, 4]]);
    }

    #[test]
    fn req_031_17_notices_a_model_that_tells_nobody_apart() {
        let same = unit(0.0);
        let mixed = [("Anna", same.as_slice()); 3];
        let both: Vec<(&str, &[f32])> = mixed.iter().copied().chain([("Bob", same.as_slice()); 3]).collect();
        assert!(tells_nobody_apart(&both));

        let (anna, bob) = (unit(0.0), unit(1.5));
        let apart: Vec<(&str, &[f32])> = [("Anna", anna.as_slice()); 3]
            .into_iter()
            .chain([("Bob", bob.as_slice()); 3])
            .collect();
        assert!(!tells_nobody_apart(&apart));
        // Too few people or faces to say.
        assert!(!tells_nobody_apart(&both[..3]));
        assert!(!tells_nobody_apart(&both[2..4]));
    }

    /// REQ-031-02: hashes are the published files', links are pinned, and nothing non-commercial has a button.
    #[test]
    fn req_031_02_the_catalog_pins_what_it_downloads() {
        let names: Vec<&str> = CATALOG.iter().map(|model| model.name).collect();
        assert_eq!(names.len(), 5, "{names:?}");
        for model in CATALOG {
            assert_eq!(model.sha256.len(), 64, "{}", model.name);
            assert!(model.url.starts_with("https://"), "{}", model.name);
            assert!(!model.url.contains("/main/"), "pin a revision: {}", model.url);
        }
        assert!(
            CATALOG
                .iter()
                .any(|model| matches!(model.kind, ModelKind::Detect(DetectorFormat::YuNet)))
        );
        assert!(CATALOG.iter().any(|model| matches!(
            model.kind,
            ModelKind::Recognize {
                style: InputStyle::SFace,
                ..
            }
        )));
    }
}
