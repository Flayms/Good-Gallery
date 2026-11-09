//! Loading the photo, its small copy and its subject, on worker threads.

use super::{Loaded, Target};
use domain::edit::{Mask, Recipe, SavedEdit};
use domain::versions::Version;
use eframe::egui_wgpu::{self, wgpu};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::channel;

/// The small copy for the white balance picker and the CPU preview.
#[cfg(not(target_arch = "wasm32"))]
const SMALL_SIDE: u32 = 512;

/// Runs `job` on a thread of its own (in the browser, at once).
pub(super) fn spawn(name: &str, job: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::Builder::new()
        .name(name.into())
        .spawn(job)
        .expect("can't start a thread");
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        job();
    }
}

/// The unedited original of `target` and the recipe to start from: the source of `edit` and its recipe while the
/// source exists (REQ-021-11), else the file itself, unedited.
/// The original to edit from, the recipe to start with and the photo's versions (REQ-021-11, REQ-021-24): without
/// its original, the file itself with nothing applied and no versions, since none of them can be shown.
pub(super) fn resolve(target: &Target, edit: &SavedEdit) -> (String, Recipe, Vec<Version>) {
    let exists =
        db::media::absolute_path(&target.library.to_string_lossy(), &edit.source).is_some_and(|path| path.is_file());
    if exists {
        (edit.source.clone(), edit.recipe(), edit.versions())
    } else {
        (target.rel_path.clone(), Recipe::default(), Vec::new())
    }
}

/// Loads `source`, the unedited original of `target`, as a texture and a small copy.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn load(
    target: &Target,
    source: &str,
    render_state: Option<&egui_wgpu::RenderState>,
) -> Result<Loaded, String> {
    let path =
        db::media::absolute_path(&target.library.to_string_lossy(), source).ok_or("the path leaves its library")?;
    let max_side = render_state.map_or(SMALL_SIDE, |render_state| {
        render_state.device.limits().max_texture_dimension_2d.min(8192)
    });
    let image = media::original::load_original(&path, None, max_side, target.sidecar_orientation)
        .map_err(|error| error.to_string())?;
    let small = fit_small(&image);
    let view = render_state.map(|render_state| {
        let size = [image.width(), image.height()];
        let levels = crate::textures::mip_chain(size, image.into_raw());
        crate::textures::upload(&render_state.device, &render_state.queue, size, &levels)
            .create_view(&wgpu::TextureViewDescriptor::default())
    });
    Ok(Loaded {
        view,
        lens: None,
        small: Some(small),
        cpu: None,
    })
}

/// The browser version doesn't edit (REQ-021-01).
#[cfg(target_arch = "wasm32")]
pub(super) fn read_small(
    _render_state: &egui_wgpu::RenderState,
    _texture: &wgpu::Texture,
) -> Result<image::RgbaImage, String> {
    Err("editing isn't available here".into())
}

/// The browser version doesn't edit (REQ-021-01).
#[cfg(target_arch = "wasm32")]
pub(super) fn load(
    _target: &Target,
    _source: &str,
    _render_state: Option<&egui_wgpu::RenderState>,
) -> Result<Loaded, String> {
    Err("editing isn't available here".into())
}

/// `image` fit into the small copy's size.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn fit_small(image: &image::RgbaImage) -> image::RgbaImage {
    if image.width().max(image.height()) > SMALL_SIDE {
        let (width, height) = domain::thumbs::fit_inside(image.width(), image.height(), SMALL_SIDE, SMALL_SIDE, false);
        image::imageops::resize(image, width, height, image::imageops::FilterType::Triangle)
    } else {
        image.clone()
    }
}

/// The small copy of `texture`'s photo: its smallest mip level still at least as long as the small copy, read back
/// and fit into it (REQ-021-19).
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn read_small(
    render_state: &egui_wgpu::RenderState,
    texture: &wgpu::Texture,
) -> Result<image::RgbaImage, String> {
    let size = texture.size();
    let (mut level, mut width, mut height) = (0, size.width, size.height);
    while level + 1 < texture.mip_level_count() && (width / 2).max(height / 2) >= SMALL_SIDE {
        (level, width, height) = (level + 1, (width / 2).max(1), (height / 2).max(1));
    }
    let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let device = &render_state.device;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("edit small copy"),
        size: u64::from(row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: level,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    render_state.queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| error.to_string())?;
    rx.recv()
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    let data = slice.get_mapped_range().map_err(|error| error.to_string())?;
    let pixels = (0..height as usize)
        .flat_map(|y| {
            data[y * row as usize..y * row as usize + width as usize * 4]
                .iter()
                .copied()
        })
        .collect();
    drop(data);
    buffer.unmap();
    let image = image::RgbaImage::from_raw(width, height, pixels).ok_or("the read back copy has the wrong size")?;
    Ok(fit_small(&image))
}

/// Looks for the subject of `photo` (REQ-022-02).
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn find(photo: &image::RgbaImage) -> Result<Mask, String> {
    media::subject::find(photo)
}

#[cfg(target_arch = "wasm32")]
pub(super) fn find(_photo: &image::RgbaImage) -> Result<Mask, String> {
    Err("not available here".into())
}
