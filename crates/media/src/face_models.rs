//! Running face models (spec 032): a detector or recognizer on a photo, with what `domain::face_models` says each
//! needs. The engine is behind [`Net`], so another can join `tract` without touching the steps around it.

use domain::face_models::{
    Alignment, CROP_SIDE, DEFAULT_THRESHOLD, Detection, DetectorFormat, HINT_SCORE, INPUT_SIDE, InputStyle, ModelKind,
    Pass, decode_retinaface, decode_yunet, first_pass, keep_recognizable, more_passes, normalize, retinaface_priors,
    suppress,
};
use image::RgbImage;
use image::imageops::FilterType;
use std::path::Path;
use tract_onnx::prelude::*;

/// One output of a model: its shape and values.
pub type Output = (Vec<usize>, Vec<f32>);

/// A loaded model: a batch of one in, every output with its shape.
pub trait Net: Send + Sync {
    fn run(&self, input: Vec<f32>, side: usize) -> Result<Vec<Output>, String>;
}

struct TractNet {
    model: std::sync::Arc<TypedRunnableModel>,
}

/// Loads a model for a square input, which also shows whether `tract` can run it.
fn load_tract(file: &Path, side: usize) -> Result<TractNet, String> {
    let model = tract_onnx::onnx()
        .model_for_path(file)
        .and_then(|model| model.with_input_fact(0, f32::fact([1, 3, side, side]).into()))
        .and_then(|model| model.into_optimized())
        .and_then(|model| model.into_runnable())
        .map_err(|error| format!("can't load the model: {error}"))?;
    Ok(TractNet { model })
}

impl Net for TractNet {
    fn run(&self, input: Vec<f32>, side: usize) -> Result<Vec<Output>, String> {
        let tensor: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, side, side), input)
            .map_err(|error| error.to_string())?
            .into();
        let outputs = self
            .model
            .run(tvec!(tensor.into()))
            .map_err(|error| format!("the model failed: {error}"))?;
        outputs
            .iter()
            .map(|output| {
                let view = output
                    .to_plain_array_view::<f32>()
                    .map_err(|error| format!("the model gave no numbers: {error}"))?;
                Ok((view.shape().to_vec(), view.iter().copied().collect()))
            })
            .collect()
    }
}

/// Finds faces in photos.
pub struct Detector {
    net: Box<dyn Net>,
    format: DetectorFormat,
    priors: Vec<[f32; 4]>,
}

impl Detector {
    /// Loads `file` and checks on an empty picture that it answers as `format` does (REQ-032-03).
    pub fn load(file: &Path, format: DetectorFormat) -> Result<Self, String> {
        let net = load_tract(file, INPUT_SIDE)?;
        Self::new(Box::new(net), format)
    }

    fn new(net: Box<dyn Net>, format: DetectorFormat) -> Result<Self, String> {
        let priors = match format {
            DetectorFormat::YuNet => Vec::new(),
            DetectorFormat::RetinaFace => retinaface_priors(),
        };
        let detector = Self { net, format, priors };
        detector.run(vec![0.0; 3 * INPUT_SIDE * INPUT_SIDE], 1.0)?;
        Ok(detector)
    }

    fn run(&self, input: Vec<f32>, min_score: f32) -> Result<Vec<Detection>, String> {
        let outputs = self.net.run(input, INPUT_SIDE)?;
        match self.format {
            DetectorFormat::YuNet => {
                let wanted = [1, 1, 1, 1, 1, 1, 4, 4, 4, 10, 10, 10];
                let fits = outputs.len() == wanted.len()
                    && outputs
                        .iter()
                        .zip(wanted)
                        .enumerate()
                        .all(|(n, ((shape, values), per_cell))| {
                            let cells = (INPUT_SIDE >> (3 + n % 3)).pow(2);
                            shape.last() == Some(&per_cell) && values.len() == cells * per_cell
                        });
                if !fits {
                    return Err("the model doesn't answer as a YuNet detector does".into());
                }
                let level = |first: usize| std::array::from_fn(|n| outputs[first + n].1.clone());
                Ok(decode_yunet(&level(0), &level(3), &level(6), &level(9), min_score))
            }
            DetectorFormat::RetinaFace => {
                let anchors = self.priors.len();
                let fits = outputs.len() == 3
                    && outputs[0].1.len() == anchors * 4
                    && outputs[1].1.len() == anchors * 2
                    && outputs[2].1.len() == anchors * 10;
                if !fits {
                    return Err("the model doesn't answer as a RetinaFace detector does".into());
                }
                Ok(decode_retinaface(
                    &outputs[0].1,
                    &outputs[1].1,
                    &outputs[2].1,
                    &self.priors,
                    min_score,
                ))
            }
        }
    }

    /// What `pass` shows of `photo`, as this format's input: BGR, with RetinaFace's training mean taken off.
    fn input(&self, photo: &RgbImage, pass: &Pass) -> Vec<f32> {
        let (x, y) = (pass.x as u32, pass.y as u32);
        let side = pass.side.round() as u32;
        let (width, height) = (
            side.min(photo.width() - x.min(photo.width())),
            side.min(photo.height() - y.min(photo.height())),
        );
        let scale = pass.input / pass.side;
        let view = image::imageops::crop_imm(photo, x, y, width, height).to_image();
        let shrunk = image::imageops::resize(
            &view,
            ((width as f32 * scale).round() as u32).clamp(1, INPUT_SIDE as u32),
            ((height as f32 * scale).round() as u32).clamp(1, INPUT_SIDE as u32),
            FilterType::Triangle,
        );
        let mean: [f32; 3] = match self.format {
            DetectorFormat::YuNet => [0.0; 3],
            DetectorFormat::RetinaFace => [104.0, 117.0, 123.0],
        };
        let mut input = vec![0.0; 3 * INPUT_SIDE * INPUT_SIDE];
        for (px, py, pixel) in shrunk.enumerate_pixels() {
            let at = py as usize * INPUT_SIDE + px as usize;
            for (plane, channel) in [2, 1, 0].into_iter().enumerate() {
                input[plane * INPUT_SIDE * INPUT_SIDE + at] = f32::from(pixel[channel]) - mean[plane];
            }
        }
        input
    }

    /// The faces in an upright `photo`, in its pixels (REQ-032-06).
    pub fn find(&self, photo: &RgbImage) -> Result<Vec<Detection>, String> {
        self.find_from(photo, self.format.min_score())
    }

    /// One look at a photo of the model's input size, for measuring a model's own speed.
    pub fn find_one_look(&self, photo: &RgbImage) -> Result<Vec<Detection>, String> {
        let pass = first_pass(photo.width(), photo.height());
        self.run(self.input(photo, &pass), self.format.min_score())
    }

    /// As [`Self::find`], keeping faces from `lowest` on, while the same looks are taken. For measuring thresholds.
    pub fn find_from(&self, photo: &RgbImage, lowest: f32) -> Result<Vec<Detection>, String> {
        if photo.width() == 0 || photo.height() == 0 {
            return Ok(Vec::new());
        }
        let min_score = self.format.min_score();
        let floor = lowest.min(HINT_SCORE).min(min_score);
        let look = |pass: &Pass| -> Result<Vec<Detection>, String> {
            let found = self.run(self.input(photo, pass), floor)?;
            Ok(found.into_iter().map(|face| pass.to_photo(face)).collect())
        };
        let hints = look(&first_pass(photo.width(), photo.height()))?;
        let confident: Vec<Detection> = hints.iter().copied().filter(|face| face.score >= min_score).collect();
        let weak_enough = |face: &Detection| face.score >= HINT_SCORE.min(min_score);
        let usable: Vec<Detection> = hints.iter().copied().filter(weak_enough).collect();
        let mut found = hints;
        for pass in more_passes(photo.width(), photo.height(), &confident, &usable) {
            found.extend(look(&pass)?);
        }
        found.retain(|face| face.score >= lowest);
        Ok(keep_recognizable(suppress(found)))
    }
}

/// Turns faces into vectors to compare.
pub struct Recognizer {
    net: Box<dyn Net>,
    style: InputStyle,
}

impl Recognizer {
    /// Loads `file` and checks on an empty crop that it answers with one vector (REQ-032-03).
    pub fn load(file: &Path, style: InputStyle) -> Result<Self, String> {
        let net = load_tract(file, CROP_SIDE)?;
        Self::new(Box::new(net), style)
    }

    fn new(net: Box<dyn Net>, style: InputStyle) -> Result<Self, String> {
        let recognizer = Self { net, style };
        recognizer.run(vec![0.0; 3 * CROP_SIDE * CROP_SIDE])?;
        Ok(recognizer)
    }

    fn run(&self, input: Vec<f32>) -> Result<Vec<f32>, String> {
        let mut outputs = self.net.run(input, CROP_SIDE)?;
        const MIN_LENGTH: usize = 32;
        const MAX_LENGTH: usize = 4096;
        match outputs.as_mut_slice() {
            [(_, values)] if (MIN_LENGTH..=MAX_LENGTH).contains(&values.len()) => {
                let mut vector = std::mem::take(values);
                normalize(&mut vector);
                Ok(vector)
            }
            _ => Err("the model doesn't answer with one vector as a recognition model does".into()),
        }
    }

    /// The vector of the face with `landmarks` in `photo`, aligned onto the template (REQ-032-07).
    pub fn embed(&self, photo: &RgbImage, landmarks: &[[f32; 2]; 5]) -> Result<Vec<f32>, String> {
        let crop = aligned_crop(photo, landmarks);
        let mut input = vec![0.0; 3 * CROP_SIDE * CROP_SIDE];
        for (x, y, pixel) in crop.enumerate_pixels() {
            let at = y as usize * CROP_SIDE + x as usize;
            for channel in 0..3 {
                input[channel * CROP_SIDE * CROP_SIDE + at] = self.style.value(pixel[channel]);
            }
        }
        self.run(input)
    }
}

/// What a model file is, by trying what it answers as (REQ-032-03). Recognition comes first: a large recognizer takes
/// seconds to refuse as a detector, while a detector refuses at once as a recognizer. `style` can't be seen from the file.
pub fn identify(file: &Path, style: InputStyle) -> Result<ModelKind, String> {
    let as_recognizer = match Recognizer::load(file, style) {
        Ok(_) => {
            return Ok(ModelKind::Recognize {
                style,
                threshold: DEFAULT_THRESHOLD,
            });
        }
        Err(error) => error,
    };
    for format in [DetectorFormat::YuNet, DetectorFormat::RetinaFace] {
        if Detector::load(file, format).is_ok() {
            return Ok(ModelKind::Detect(format));
        }
    }
    Err(format!("it isn't a face model the app can run ({as_recognizer})"))
}

/// The face moved onto the template, [`CROP_SIDE`] square. Several samples per pixel, since a face may be far larger on
/// the photo than in the crop.
pub fn aligned_crop(photo: &RgbImage, landmarks: &[[f32; 2]; 5]) -> RgbImage {
    const SAMPLES: u32 = 3;
    let align = Alignment::new(landmarks);
    let (width, height) = (photo.width() as i64, photo.height() as i64);
    RgbImage::from_fn(CROP_SIDE as u32, CROP_SIDE as u32, |x, y| {
        let mut sum = [0u32; 3];
        for sy in 0..SAMPLES {
            for sx in 0..SAMPLES {
                let at = |n: u32, sub: u32| n as f32 + (sub as f32 + 0.5) / SAMPLES as f32;
                let [px, py] = align.invert([at(x, sx), at(y, sy)]);
                let (px, py) = (px.floor() as i64, py.floor() as i64);
                // Outside the photo is black, as in InsightFace's `warpAffine`.
                if (0..width).contains(&px) && (0..height).contains(&py) {
                    let pixel = photo.get_pixel(px as u32, py as u32);
                    (0..3).for_each(|channel| sum[channel] += u32::from(pixel[channel]));
                }
            }
        }
        image::Rgb(sum.map(|total| (total / (SAMPLES * SAMPLES)) as u8))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::face_models::{TEMPLATE, cosine};
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/faces")
            .join(name)
    }

    fn astronaut() -> RgbImage {
        image::open(fixture("astronaut.jpg")).unwrap().to_rgb8()
    }

    fn yunet() -> Detector {
        Detector::load(&fixture("face_detection_yunet_2023mar.onnx"), DetectorFormat::YuNet).unwrap()
    }

    /// Where the fixture's face is, by eye: about 225, 95 in its 512 × 512 pixels.
    fn is_her_face(face: &Detection) -> bool {
        let (cx, cy) = (face.rect[0] + face.rect[2] / 2.0, face.rect[1] + face.rect[3] / 2.0);
        (cx - 225.0).abs() < 40.0 && (cy - 95.0).abs() < 40.0
    }

    #[test]
    fn req_032_06_finds_a_real_face_with_its_landmarks() {
        let found = yunet().find(&astronaut()).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        let face = &found[0];
        assert!(is_her_face(face), "{face:?}");
        // Left eye left of the right, both above the nose, the nose above the mouth.
        let [left, right, nose, mouth_left, _] = face.landmarks;
        assert!(
            left[0] < right[0] && left[1] < nose[1] && nose[1] < mouth_left[1],
            "{face:?}"
        );
    }

    /// REQ-032-06: two faces about 80 px wide on a 4000 px photo, about 13 px in the first look at it.
    #[test]
    fn req_032_06_finds_small_faces_on_a_large_photo() {
        let face = image::imageops::crop_imm(&astronaut(), 120, 0, 240, 240).to_image();
        let mut large = RgbImage::from_pixel(4000, 3000, image::Rgb([90, 100, 110]));
        image::imageops::replace(&mut large, &face, 2600, 1800);
        image::imageops::replace(&mut large, &face, 300, 200);
        let found = yunet().find(&large).unwrap();
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|f| f.rect[2] > 50.0 && f.rect[2] < 150.0), "{found:?}");
        assert!(found.iter().any(|f| f.rect[0] > 2500.0), "{found:?}");
        assert!(found.iter().any(|f| f.rect[0] < 800.0), "{found:?}");
    }

    /// REQ-032-06: a face filling the frame is beyond what YuNet is trained on; the second look finds it.
    #[test]
    fn req_032_06_finds_a_face_that_fills_the_photo() {
        let face = image::imageops::crop_imm(&astronaut(), 165, 25, 120, 140).to_image();
        let photo = image::imageops::resize(&face, 1200, 1400, FilterType::Triangle);
        let found = yunet().find(&photo).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].rect[2] > 400.0, "{found:?}");
    }

    #[test]
    fn req_032_03_refuses_a_file_that_is_no_model_or_the_wrong_kind() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk.onnx");
        std::fs::write(&junk, b"not a model").unwrap();
        assert!(Detector::load(&junk, DetectorFormat::YuNet).is_err());
        assert!(Recognizer::load(&junk, InputStyle::ArcFace).is_err());
        let yunet = fixture("face_detection_yunet_2023mar.onnx");
        assert!(Detector::load(&yunet, DetectorFormat::RetinaFace).is_err());
        assert!(Recognizer::load(&yunet, InputStyle::ArcFace).is_err());
    }

    #[test]
    fn req_032_03_works_out_what_a_model_is() {
        let yunet = fixture("face_detection_yunet_2023mar.onnx");
        assert_eq!(
            identify(&yunet, InputStyle::ArcFace),
            Ok(ModelKind::Detect(DetectorFormat::YuNet))
        );
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk.onnx");
        std::fs::write(&junk, b"not a model").unwrap();
        assert!(identify(&junk, InputStyle::ArcFace).is_err());
    }

    #[test]
    fn req_032_07_moves_the_landmarks_onto_the_template_in_the_crop() {
        // A white dot at each landmark of a face at another place and scale.
        let landmarks = TEMPLATE.map(|[x, y]| [x * 2.0 + 100.0, y * 2.0 + 50.0]);
        let mut photo = RgbImage::new(400, 400);
        for [x, y] in landmarks {
            for dy in -2..=2 {
                for dx in -2..=2 {
                    photo.put_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32, image::Rgb([255; 3]));
                }
            }
        }
        let crop = aligned_crop(&photo, &landmarks);
        for [x, y] in TEMPLATE {
            assert!(crop.get_pixel(x as u32, y as u32)[0] > 200, "{x},{y}");
        }
        assert_eq!(crop.get_pixel(5, 5)[0], 0);
    }

    /// A net that repeats back what it was given, to look at the prepared input.
    struct Echo;
    impl Net for Echo {
        fn run(&self, input: Vec<f32>, _side: usize) -> Result<Vec<Output>, String> {
            Ok(vec![(vec![1, 64], input.into_iter().take(64).collect())])
        }
    }

    #[test]
    fn req_032_07_gives_a_normalized_vector_prepared_for_its_style() {
        let photo = RgbImage::from_pixel(300, 300, image::Rgb([255, 0, 0]));
        let landmarks = TEMPLATE.map(|[x, y]| [x + 50.0, y + 50.0]);
        let vector = Recognizer::new(Box::new(Echo), InputStyle::SFace)
            .unwrap()
            .embed(&photo, &landmarks)
            .unwrap();
        assert!((cosine(&vector, &vector) - 1.0).abs() < 1e-4);
        assert!(
            (vector[0] - 1.0 / 8.0).abs() < 1e-4,
            "64 equal values of the red plane: {}",
            vector[0]
        );
    }

    /// Real recognition, with models that aren't in the repository: `GG_FACE_MODELS=<folder> cargo test -- --ignored`.
    fn evaluation_folder() -> PathBuf {
        PathBuf::from(std::env::var("GG_FACE_MODELS").expect("GG_FACE_MODELS"))
    }

    /// REQ-032-06, REQ-032-07: the same person in other light and size is closer than someone else.
    fn tells_people_apart(recognizer: &Recognizer, detector: &Detector) {
        let folder = evaluation_folder();
        let her = astronaut();
        let vector = |photo: &RgbImage| {
            let found = detector.find(photo).unwrap();
            let face = found.first().unwrap_or_else(|| panic!("no face"));
            recognizer.embed(photo, &face.landmarks).unwrap()
        };
        let original = vector(&her);
        let changed = image::imageops::brighten(&image::imageops::resize(&her, 380, 380, FilterType::Triangle), 25);
        let same = cosine(&original, &vector(&changed));
        let lena = image::open(folder.join("lena.jpg")).unwrap().to_rgb8();
        let messi = image::open(folder.join("messi.jpg")).unwrap().to_rgb8();
        let (other, third) = (cosine(&original, &vector(&lena)), cosine(&original, &vector(&messi)));
        println!("same {same}, others {other}, {third}");
        assert!(
            same > 0.6 && same > other + 0.3 && same > third + 0.3,
            "same {same}, others {other}, {third}"
        );
    }

    #[test]
    #[ignore = "needs models outside the repository"]
    fn req_032_07_sface_tells_people_apart() {
        let folder = evaluation_folder();
        let recognizer =
            Recognizer::load(&folder.join("face_recognition_sface_2021dec.onnx"), InputStyle::SFace).unwrap();
        tells_people_apart(&recognizer, &yunet());
    }

    #[test]
    #[ignore = "needs models outside the repository"]
    fn req_032_07_auraface_tells_people_apart() {
        let folder = evaluation_folder();
        let recognizer = Recognizer::load(&folder.join("glintr100.onnx"), InputStyle::ArcFace).unwrap();
        tells_people_apart(&recognizer, &yunet());
    }

    #[test]
    #[ignore = "needs models outside the repository"]
    fn req_032_06_retinaface_finds_a_real_face() {
        let folder = evaluation_folder();
        let detector = Detector::load(&folder.join("retinaface_mv2.onnx"), DetectorFormat::RetinaFace).unwrap();
        let found = detector.find(&astronaut()).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(is_her_face(&found[0]), "{found:?}");
        let [left, right, nose, mouth_left, _] = found[0].landmarks;
        assert!(
            left[0] < right[0] && left[1] < nose[1] && nose[1] < mouth_left[1],
            "{:?}",
            found[0]
        );
    }
}
