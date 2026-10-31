//! Finding a photo's main subject (spec 022): U²-Net-p, a network for salient object detection, run on the CPU with
//! tract. The model is part of the executable, so it needs neither a download nor a helper (REQ-011-11).

use domain::edit::Mask;
use std::sync::{Arc, OnceLock};
use tract_onnx::prelude::*;

/// The network's input and output size; the photo is squeezed to it, and the mask stretched back over the photo.
const SIDE: usize = 320;
/// ImageNet's mean and deviation, which the network was trained with.
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const DEVIATION: [f32; 3] = [0.229, 0.224, 0.225];

static MODEL: &[u8] = include_bytes!("../models/u2netp.onnx");

fn model() -> Result<&'static Arc<TypedRunnableModel>, String> {
    static LOADED: OnceLock<Result<Arc<TypedRunnableModel>, String>> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            tract_onnx::onnx()
                .model_for_read(&mut &MODEL[..])
                .and_then(|model| model.with_input_fact(0, f32::fact([1, 3, SIDE, SIDE]).into()))
                .and_then(|model| model.into_optimized())
                .and_then(|model| model.into_runnable())
                .map_err(|error| format!("can't load the subject model: {error}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// How much each spot of `image` belongs to its main subject (REQ-022-02). Takes about a second; call it off the UI
/// thread. An error when nothing stands out.
pub fn find(image: &image::RgbaImage) -> Result<Mask, String> {
    let model = model()?;
    let small = image::imageops::resize(image, SIDE as u32, SIDE as u32, image::imageops::FilterType::Triangle);
    let input: Tensor = tract_ndarray::Array4::from_shape_fn((1, 3, SIDE, SIDE), |(_, c, y, x)| {
        let pixel = small.get_pixel(x as u32, y as u32);
        // Transparent pixels count as black, as in a flattened photo.
        let value = f32::from(pixel[c]) / 255.0 * f32::from(pixel[3]) / 255.0;
        (value - MEAN[c]) / DEVIATION[c]
    })
    .into();
    let outputs = model.run(tvec!(input.into())).map_err(|error| error.to_string())?;
    let output = outputs
        .first()
        .ok_or("the subject model gave no mask")?
        .to_plain_array_view::<f32>()
        .map_err(|error| error.to_string())?;
    let values: Vec<f32> = output.iter().copied().collect();
    // Scaled to 0–1 between its least and most likely spot, as rembg does.
    let (low, high) = values
        .iter()
        .fold((f32::MAX, f32::MIN), |(low, high), &v| (low.min(v), high.max(v)));
    let spread = high - low;
    if spread.is_nan() || spread <= 1e-3 {
        return Err("no subject stands out".into());
    }
    let values = values.into_iter().map(|v| (v - low) / spread).collect();
    Mask::new(SIDE as u32, SIDE as u32, values).ok_or_else(|| "the subject model gave a mask of another size".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-022-02: a bright disc on a plain wall is the subject; the wall's corners aren't.
    #[test]
    fn req_022_02_finds_the_subject_that_stands_out() {
        let (width, height) = (480u32, 360u32);
        let image = image::RgbaImage::from_fn(width, height, |x, y| {
            let (dx, dy) = (x as f32 - 240.0, y as f32 - 180.0);
            if dx * dx + dy * dy < 90.0 * 90.0 {
                image::Rgba([230, 80, 40, 255])
            } else {
                image::Rgba([70, 90, 110, 255])
            }
        });
        let mask = find(&image).unwrap();
        let center = mask.sample([0.5, 0.5]);
        let corners = [[0.05, 0.05], [0.95, 0.05], [0.05, 0.95], [0.95, 0.95]].map(|uv| mask.sample(uv));
        assert!(center > 0.8, "center {center}");
        assert!(corners.iter().all(|&c| c < 0.2), "corners {corners:?}");
    }
}
