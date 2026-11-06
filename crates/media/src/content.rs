//! Content search's models (spec 036): the folder they live in, their downloads, and running their halves with
//! `tract`. A model is a folder of three files named by `domain::content`; it counts as on this computer only once
//! every file arrived with the hash the catalog pins.

use crate::face_store::{Progress, StoreError, copy_hashing};
use domain::content::{ContentModel, IMAGE_FILE, TEXT_FILE, TOKENIZER_FILE, Tokenizer};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tract_onnx::prelude::*;

/// The content models on this computer: `<data>/content-models/<id>/`.
pub struct ContentStore {
    folder: PathBuf,
}

impl ContentStore {
    pub fn in_data_folder(data: &Path) -> Self {
        Self {
            folder: data.join("content-models"),
        }
    }

    /// The folder of a model's files.
    pub fn dir(&self, model: &ContentModel) -> PathBuf {
        self.folder.join(model.id)
    }

    /// Whether every file of `model` is here (REQ-036-02).
    pub fn has(&self, model: &ContentModel) -> bool {
        let dir = self.dir(model);
        model.files.iter().all(|file| dir.join(file.name).is_file())
    }

    /// Downloads all of a model's files into a folder beside its own, which takes its place only once every hash
    /// matched; anything else leaves nothing behind (REQ-036-02). `progress` gets the bytes of all files together.
    pub fn download(&self, model: &ContentModel, progress: Progress) -> Result<(), StoreError> {
        self.download_from(model, &|file| file.url.to_owned(), progress)
    }

    fn download_from(
        &self,
        model: &ContentModel,
        url_of: &dyn Fn(&domain::content::ModelFile) -> String,
        progress: Progress,
    ) -> Result<(), StoreError> {
        let failed = |error: std::io::Error| StoreError::Failed(error.to_string());
        std::fs::create_dir_all(&self.folder).map_err(failed)?;
        let incoming = tempfile::Builder::new()
            .prefix(&format!("incoming-{}-", model.id))
            .tempdir_in(&self.folder)
            .map_err(failed)?;
        let total = model.bytes();
        let mut done = 0;
        for file in &model.files {
            let url = url_of(file);
            let reader = fetch(&url)?;
            let target = std::fs::File::create(incoming.path().join(file.name)).map_err(failed)?;
            let mut each = |bytes: u64, _: Option<u64>| progress(done + bytes, Some(total));
            let hash = copy_hashing(reader, &target, Some(file.bytes), &mut each)?;
            if hash != file.sha256 {
                return Err(StoreError::WrongHash);
            }
            done += file.bytes;
        }
        let dir = self.dir(model);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(failed)?;
        }
        std::fs::rename(incoming.keep(), &dir).map_err(failed)
    }

    /// Removes a model's files.
    pub fn remove(&self, model: &ContentModel) -> std::io::Result<()> {
        let dir = self.dir(model);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }
}

fn fetch(url: &str) -> Result<impl std::io::Read, StoreError> {
    const CONNECT: Duration = Duration::from_secs(30);
    let response = ureq::get(url)
        .config()
        .https_only(url.starts_with("https://"))
        .timeout_connect(Some(CONNECT))
        .timeout_recv_response(Some(CONNECT))
        .build()
        .call()
        .map_err(|error| StoreError::Failed(format!("can't download: {error}")))?;
    Ok(response.into_body().into_reader())
}

type Runnable = std::sync::Arc<TypedRunnableModel>;

fn load(file: &Path, input: InferenceFact) -> Result<Runnable, String> {
    tract_onnx::onnx()
        // CLIP's export names a batch size its shapes then contradict; the input's fact decides.
        .with_ignore_value_info(true)
        .with_ignore_output_shapes(true)
        .model_for_path(file)
        .and_then(|model| model.with_input_fact(0, input))
        .and_then(|model| model.into_optimized())
        .and_then(|model| model.into_runnable())
        .map_err(|error| format!("can't load {}: {error}", file.display()))
}

/// The model's vector: the output with two dimensions, `[1, n]`.
fn vector_of(outputs: &TVec<TValue>) -> Result<Vec<f32>, String> {
    outputs
        .iter()
        .filter_map(|output| output.to_plain_array_view::<f32>().ok())
        .find(|view| view.ndim() == 2)
        .map(|view| view.iter().copied().collect())
        .ok_or_else(|| "the model gave no vector".to_owned())
}

/// The image half: a photo in, a vector out (REQ-036-04).
pub struct ImageHalf {
    net: Runnable,
    model: &'static ContentModel,
}

impl ImageHalf {
    pub fn load(store: &ContentStore, model: &'static ContentModel) -> Result<Self, String> {
        let side = model.side as usize;
        let net = load(&store.dir(model).join(IMAGE_FILE), f32::fact([1, 3, side, side]).into())?;
        Ok(Self { net, model })
    }

    /// The vector of a photo: its shorter side scaled to the model's, the center square cut out.
    pub fn vector(&self, photo: &image::RgbImage) -> Result<Vec<f32>, String> {
        let side = self.model.side;
        let (at, scaled) = domain::content::center_crop(photo.width(), photo.height(), side);
        let resized = image::imageops::resize(photo, scaled[0], scaled[1], image::imageops::FilterType::CatmullRom);
        let square = image::imageops::crop_imm(&resized, at[0], at[1], side, side).to_image();
        let input = domain::content::image_input(square.as_raw(), self.model);
        let tensor: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, side as usize, side as usize), input)
            .map_err(|error| error.to_string())?
            .into();
        let outputs = self
            .net
            .run(tvec!(tensor.into()))
            .map_err(|error| format!("the model failed: {error}"))?;
        vector_of(&outputs)
    }
}

/// The text half and its tokenizer: words in, a vector out (REQ-036-05).
pub struct TextHalf {
    net: Runnable,
    tokenizer: Tokenizer,
}

impl TextHalf {
    pub fn load(store: &ContentStore, model: &ContentModel) -> Result<Self, String> {
        let dir = store.dir(model);
        let tokenizer = std::fs::read_to_string(dir.join(TOKENIZER_FILE))
            .map_err(|error| error.to_string())
            .and_then(|text| Tokenizer::from_json(&text))?;
        let net = load(&dir.join(TEXT_FILE), i64::fact([1, domain::content::CONTEXT]).into())?;
        Ok(Self { net, tokenizer })
    }

    pub fn vector(&self, words: &str) -> Result<Vec<f32>, String> {
        let ids = self.tokenizer.encode(words);
        let tensor: Tensor = tract_ndarray::Array2::from_shape_vec((1, domain::content::CONTEXT), ids)
            .map_err(|error| error.to_string())?
            .into();
        let outputs = self
            .net
            .run(tvec!(tensor.into()))
            .map_err(|error| format!("the model failed: {error}"))?;
        vector_of(&outputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::content::{CATALOG, ModelFile};
    use sha2::{Digest, Sha256};
    use std::io::Write;

    /// A local web server answering each request with the body for its path.
    fn serve(bodies: Vec<(&'static str, Vec<u8>)>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut request = [0; 2048];
                let read = std::io::Read::read(&mut stream, &mut request).unwrap_or(0);
                let line = String::from_utf8_lossy(&request[..read]).to_string();
                let body = bodies
                    .iter()
                    .find(|(path, _)| line.contains(&format!("GET /{path} ")))
                    .map(|(_, body)| body.clone())
                    .unwrap_or_default();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    fn sha(bytes: &[u8]) -> &'static str {
        Box::leak(
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                .into_boxed_str(),
        )
    }

    /// A model of three small files served locally; `wrong` gives the text file a hash it won't have.
    fn test_model(wrong: bool) -> (ContentModel, String) {
        let bodies = vec![
            ("image", b"image".to_vec()),
            ("text", b"text".to_vec()),
            ("tok", b"{}".to_vec()),
        ];
        let file = |name, path: &'static str, body: &[u8]| ModelFile {
            name,
            url: path,
            sha256: if wrong && path == "text" {
                sha(b"other")
            } else {
                sha(body)
            },
            bytes: body.len() as u64,
        };
        let model = ContentModel {
            id: "test",
            files: [
                file(IMAGE_FILE, "image", b"image"),
                file(TEXT_FILE, "text", b"text"),
                file(TOKENIZER_FILE, "tok", b"{}"),
            ],
            ..CATALOG[0]
        };
        (model, serve(bodies))
    }

    fn entries(folder: &Path) -> usize {
        std::fs::read_dir(folder).map_or(0, |entries| entries.count())
    }

    /// REQ-036-02: a model counts only once all its files arrived with their hashes; a mismatch or a cancel leaves
    /// nothing behind.
    #[test]
    fn req_036_02_keeps_a_model_only_if_every_hash_matches() {
        let data = tempfile::tempdir().unwrap();
        let store = ContentStore::in_data_folder(data.path());

        let (model, base) = test_model(false);
        let url_of = |file: &ModelFile| format!("{base}/{}", file.url);
        let mut seen = 0;
        store
            .download_from(&model, &url_of, &mut |bytes, total| {
                seen = seen.max(bytes);
                assert_eq!(total, Some(11));
                true
            })
            .unwrap();
        assert!(store.has(&model));
        assert_eq!(seen, 11);
        assert_eq!(entries(&store.folder), 1, "only the model's folder");

        let (wrong, base) = test_model(true);
        let wrong = ContentModel { id: "wrong", ..wrong };
        let url_of = |file: &ModelFile| format!("{base}/{}", file.url);
        assert_eq!(
            store.download_from(&wrong, &url_of, &mut |_, _| true),
            Err(StoreError::WrongHash)
        );
        assert!(!store.has(&wrong));
        let cancelled = ContentModel {
            id: "cancelled",
            ..model
        };
        assert_eq!(
            store.download_from(&cancelled, &url_of, &mut |_, _| false),
            Err(StoreError::Cancelled)
        );
        assert_eq!(entries(&store.folder), 1, "nothing left of the failed ones");

        store.remove(&model).unwrap();
        assert!(!store.has(&model));
    }

    /// REQ-036-04, REQ-036-05: with a real model on this computer (`GG_CONTENT_MODELS`, a folder of `<id>/` folders),
    /// a photo's vector is closest to the words describing it. Skipped without one: the files are 200 MB.
    #[test]
    fn req_036_04_req_036_05_a_real_model_matches_words_and_photos() {
        let Some(models) = std::env::var_os("GG_CONTENT_MODELS") else {
            eprintln!("skipped: GG_CONTENT_MODELS isn't set");
            return;
        };
        let store = ContentStore { folder: models.into() };
        for model in &CATALOG {
            if !store.has(model) {
                continue;
            }
            let image = ImageHalf::load(&store, model).unwrap();
            let text = TextHalf::load(&store, model).unwrap();
            // A blue sky over green grass, and a black night.
            let meadow = image::RgbImage::from_fn(320, 240, |_, y| {
                if y < 120 {
                    image::Rgb([110, 170, 235])
                } else {
                    image::Rgb([60, 150, 50])
                }
            });
            let night = image::RgbImage::from_pixel(320, 240, image::Rgb([5, 5, 12]));
            let words = domain::content::normalized(&text.vector("a green meadow under a blue sky").unwrap());
            let score = |photo: &image::RgbImage| {
                domain::content::similarity(&domain::content::quantize(&image.vector(photo).unwrap()), &words)
            };
            let (meadow, night) = (score(&meadow), score(&night));
            assert!(meadow > night + 0.05, "{}: meadow {meadow}, night {night}", model.id);
        }
    }
}
