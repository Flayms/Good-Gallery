//! Metadata of media files (spec 004). [`rules`] turns tags named as exiftool names them into `MediaMetadata`; the
//! readers here produce such tags from the file's bytes without exiftool, for the web build (REQ-019-04).
//!
//! The readers do no IO: [`read`] returns [`Missing`] when it needs bytes the caller hasn't read yet.

mod bytes;
mod formats;
pub mod rules;
mod tiff;
mod xmp;

pub use bytes::{Bytes, Missing};
use rules::Tags;

/// Bytes a caller should read first: enough for the metadata of most photos.
pub const FIRST_READ: u64 = 128 << 10;

/// The tags of a media file from its bytes, by its name's extension, or which bytes are needed for them. Unknown
/// formats (and MKV / WebM, whose metadata the browser reports instead) have no tags but their MIME type.
pub fn read(file_name: &str, data: &Bytes) -> Result<Tags, Missing> {
    let mut tags = Tags::new();
    let extension = file_name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "jpg" | "jpeg" => formats::jpeg(data, &mut tags)?,
        "png" => formats::png(data, &mut tags)?,
        "webp" => formats::webp(data, &mut tags)?,
        "gif" => formats::gif(data, &mut tags)?,
        "avif" => formats::heif(data, &mut tags)?,
        "mp4" => formats::movie(data, "video/mp4", &mut tags)?,
        "m4v" => formats::movie(data, "video/x-m4v", &mut tags)?,
        "mov" => formats::movie(data, "video/quicktime", &mut tags)?,
        "webm" => {
            tags.insert("MIMEType".into(), "video/webm".into());
        }
        "mkv" => {
            tags.insert("MIMEType".into(), "video/x-matroska".into());
        }
        _ => {}
    }
    Ok(tags)
}

/// The tags of an XMP sidecar.
pub fn read_sidecar(data: &[u8]) -> Tags {
    let mut tags = Tags::new();
    xmp::read(data, &mut tags);
    tags
}
