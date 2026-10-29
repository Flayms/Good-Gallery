//! Video playback in the browser (REQ-019-07): a `<video>` element (not in the page) plays the file; each UI frame
//! copies its current frame into a texture, so the viewer draws it with its chrome and glass over it, as on the
//! desktop.

use super::js;
use crate::video::{Playback, PlaybackState, VideoFrame};
use eframe::egui::TextureId;
use eframe::egui_wgpu::{self, wgpu};
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

pub struct WebPlayback {
    player: Rc<js::Player>,
    render_state: egui_wgpu::RenderState,
    texture: Option<(wgpu::Texture, TextureId, [u32; 2])>,
    /// Counts loads, so a file found after the user moved on doesn't start playing.
    load: Rc<Cell<u64>>,
}

impl WebPlayback {
    pub fn new(render_state: egui_wgpu::RenderState) -> Self {
        Self {
            player: Rc::new(js::Player::new()),
            render_state,
            texture: None,
            load: Rc::new(Cell::new(0)),
        }
    }

    fn free_texture(&mut self) {
        if let Some((texture, id, _)) = self.texture.take() {
            self.render_state.renderer.write().free_texture(&id);
            texture.destroy();
        }
    }

    fn texture(&mut self, size: [u32; 2]) -> (wgpu::Texture, TextureId) {
        if let Some((texture, id, at)) = &self.texture
            && *at == size
        {
            return (texture.clone(), *id);
        }
        self.free_texture();
        let texture = self.render_state.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("video"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            // Copies from a video need the texture to be a render target too.
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let id = crate::textures::register(&self.render_state, &texture);
        self.texture = Some((texture.clone(), id, size));
        (texture, id)
    }
}

impl Drop for WebPlayback {
    fn drop(&mut self) {
        self.player.stop();
        self.free_texture();
    }
}

impl Playback for WebPlayback {
    fn load(&mut self, file: &Path) {
        self.player.stop();
        let load = self.load.get() + 1;
        self.load.set(load);
        let (player, current, file) = (self.player.clone(), self.load.clone(), file.to_path_buf());
        super::spawn(async move {
            let found = super::folders::file_at(&file).await;
            if current.get() != load {
                return;
            }
            match found {
                Ok(file) => player.load(&file),
                Err(error) => player.fail(&error),
            }
        });
    }

    fn stop(&mut self) {
        self.load.set(self.load.get() + 1);
        self.player.stop();
        self.free_texture();
    }

    fn set_paused(&mut self, paused: bool) {
        self.player.set_paused(paused);
    }

    fn seek(&mut self, position: f64, exact: bool) {
        self.player.seek(position, exact);
    }

    fn set_volume(&mut self, volume: f64) {
        self.player.set_volume(volume);
    }

    fn set_muted(&mut self, muted: bool) {
        self.player.set_muted(muted);
    }

    fn state(&mut self) -> PlaybackState {
        let state = self.player.state();
        let number = |i| state.get(i).as_f64().unwrap_or(0.0);
        let flag = |i| state.get(i).as_bool().unwrap_or(false);
        let size = [number(3) as u32, number(4) as u32];
        PlaybackState {
            paused: flag(0),
            position: number(1),
            duration: number(2),
            video_size: (size[0] > 0 && size[1] > 0).then_some(size),
            // The browser shows videos upright already.
            quarters: 0,
            ended: flag(5),
            volume: number(6),
            muted: flag(7),
            error: state.get(9).as_string(),
        }
    }

    fn frame(&mut self, _render_size: Option<[u32; 2]>) -> Option<VideoFrame> {
        let state = self.player.state();
        let ready = state.get(8).as_bool().unwrap_or(false);
        let size = [
            state.get(3).as_f64().unwrap_or(0.0) as u32,
            state.get(4).as_f64().unwrap_or(0.0) as u32,
        ];
        if !ready || size[0] == 0 || size[1] == 0 {
            return self
                .texture
                .as_ref()
                .map(|(_, id, size)| VideoFrame { id: *id, size: *size });
        }
        let (texture, id) = self.texture(size);
        self.render_state.queue.copy_external_image_to_texture(
            &wgpu::CopyExternalImageSourceInfo {
                source: wgpu::ExternalImageSource::HTMLVideoElement(self.player.video()),
                origin: wgpu::Origin2d::ZERO,
                flip_y: false,
            },
            wgpu::CopyExternalImageDestInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
                color_space: wgpu::PredefinedColorSpace::Srgb,
                premultiplied_alpha: false,
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        Some(VideoFrame { id, size })
    }
}
