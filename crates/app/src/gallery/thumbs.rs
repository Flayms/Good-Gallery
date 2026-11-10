//! Thumbnails of tiles: requesting them, placeholders while they load (thumbhash), and painting them.

use super::*;

/// Thumbnail rendering and textures; absent in tests without a GPU.
pub struct Thumbnails {
    pub cache: ThumbCache,
    pub service: Option<Arc<ThumbnailService>>,
    pub ready: Receiver<ThumbnailReady>,
    pub streamer: TextureStreamer,
}

/// The ThumbHash placeholder of an item as a small texture (REQ-005-06).
pub(super) fn make_placeholder(
    placeholders: &mut HashMap<i64, egui::TextureHandle>,
    ctx: &egui::Context,
    item: &GalleryItem,
) -> Option<egui::TextureId> {
    let hash = base64::engine::general_purpose::STANDARD
        .decode(item.thumbhash.as_ref()?)
        .ok()?;
    let (w, h, rgba) = thumbhash::thumb_hash_to_rgba(&hash).ok()?;
    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
    if placeholders.len() >= MAX_PLACEHOLDERS {
        placeholders.clear();
    }
    let texture = ctx.load_texture(format!("placeholder {}", item.id), image, egui::TextureOptions::LINEAR);
    let id = texture.id();
    placeholders.insert(item.id, texture);
    Some(id)
}

pub(super) fn key_of(item: &GalleryItem) -> ThumbKey {
    ThumbKey {
        root_id: item.root_id,
        rel_path: item.rel_path.clone(),
        size: item.size,
        mtime: item.mtime,
    }
}

pub(super) fn full_uv() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

/// The part of a texture that covers a tile without distortion, centered (`object-fit: cover`).
pub fn cover_uv([width, height]: [u32; 2], tile: Vec2) -> Rect {
    let texture = width as f32 / height as f32;
    let target = tile.x / tile.y;
    if texture > target {
        let w = target / texture;
        Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
    } else {
        let h = texture / target;
        Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
    }
}

impl Gallery {
    /// Asks for the thumbnails the viewer shows, its photo's and its filmstrip's, on a page that draws no gallery under
    /// it (REQ-032-26).
    pub fn want_viewer_thumbnails(&mut self, pixels_per_point: f32) {
        // Without tiles drawn, nothing else loads their items.
        let strip = self.strip.clone();
        self.load_range(strip.start, strip.end);
        if let Some((index, _)) = self.viewed {
            self.load_range(index, index + 1);
        }
        self.request_textures(std::iter::empty(), pixels_per_point);
    }

    /// Takes in finished thumbnails. Once per frame, on every page that draws them, before it does.
    pub fn begin_thumbnails(&mut self) {
        self.textures_asked = false;
        if let Some(thumbnails) = &mut self.thumbnails {
            thumbnails.streamer.begin_frame();
        }
    }

    /// Asks for these media items' thumbnails, for the People page, which draws no tiles.
    pub fn want_thumbnails(&mut self, wanted: &[(i64, ThumbKey)]) {
        let Some(thumbnails) = &mut self.thumbnails else { return };
        let width = domain::thumbs::MAX_THUMB_WIDTH;
        thumbnails.streamer.request(
            wanted
                .iter()
                .map(|(media, key)| TextureRequest {
                    media: *media,
                    path: thumbnails.cache.path(key, width),
                    width,
                })
                .collect(),
        );
        for media in thumbnails.streamer.take_missing() {
            if let Some(service) = &thumbnails.service {
                service.request(media);
            }
        }
    }

    /// The loaded thumbnail of a media item, if any.
    pub fn thumbnail(&mut self, id: i64) -> Option<crate::textures::Loaded> {
        self.thumbnails.as_mut()?.streamer.get(id)
    }

    /// The ThumbHash placeholder of a loaded item (REQ-005-06).
    pub fn placeholder(&mut self, ctx: &egui::Context, id: i64) -> Option<egui::TextureId> {
        if let Some(texture) = self.placeholders.get(&id) {
            return Some(texture.id());
        }
        let item = self.items.get(&id)?;
        make_placeholder(&mut self.placeholders, ctx, item)
    }

    /// Paints a media item's thumbnail into `rect`: its placeholder, the thumbnail fading in over it, or the
    /// "no image" icon when it can't be had (REQ-005-06, REQ-005-14).
    pub(super) fn paint_thumbnail(&mut self, ui: &egui::Ui, rect: Rect, radius: u8, id: i64, frame: &mut FrameState) {
        let Some(item) = self.items.get(&id) else {
            ui.painter().rect_filled(rect, radius, theme::placeholder());
            return;
        };
        // Placeholder: the ThumbHash's blurred colors, else gray.
        let placeholder = match self.placeholders.get(&id) {
            Some(texture) => Some(texture.id()),
            None if frame.made < PLACEHOLDERS_PER_FRAME => {
                frame.made += 1;
                make_placeholder(&mut self.placeholders, ui.ctx(), item)
            }
            None => None,
        };
        let painter = ui.painter();
        match placeholder {
            Some(texture) => {
                painter.add(
                    egui::epaint::RectShape::filled(rect, radius, Color32::WHITE).with_texture(texture, full_uv()),
                );
            }
            None => {
                painter.rect_filled(rect, radius, theme::placeholder());
            }
        }
        let failed = item.thumb_status == ThumbStatus::Error
            || self.failed.contains(&id)
            || self.thumbnails.as_ref().is_some_and(|t| t.streamer.is_broken(id));
        let loaded = self.thumbnails.as_mut().and_then(|t| t.streamer.get(id));
        // Under glass the placeholder is its plain color (the blur hides the difference), and only while the
        // thumbnail doesn't cover it yet (REQ-013-03).
        if loaded
            .as_ref()
            .is_none_or(|loaded| (frame.now - loaded.at).as_secs_f32() < theme::DURATION_BASE)
        {
            crate::glass::record_solid(ui.ctx(), rect, theme::placeholder(), false);
        }
        if let Some(loaded) = loaded {
            let t = (frame.now - loaded.at).as_secs_f32() / theme::DURATION_BASE;
            frame.fading |= t < 1.0;
            let tint = Color32::WHITE.gamma_multiply(t.clamp(0.0, 1.0));
            let uv = cover_uv(loaded.size, rect.size());
            painter.add(egui::epaint::RectShape::filled(rect, radius, tint).with_texture(loaded.id, uv));
            crate::glass::record(
                ui.ctx(),
                crate::glass::BackdropItem {
                    rect,
                    uv,
                    view: Some(loaded.view),
                    color: tint,
                    chrome: false,
                },
            );
        } else if failed {
            let size = (rect.width().min(rect.height()) * 0.6).min(24.0);
            let icon = Rect::from_center_size(rect.center(), Vec2::splat(size));
            Icon::ImageOff.image(size, theme::muted()).paint_at(ui, icon);
        }
    }

    /// Asks for the thumbnails of `wanted` (item index and width in points), most important first, and for rendering
    /// of those the cache doesn't have (REQ-005-07, REQ-006-13).
    pub(super) fn request_textures(&mut self, wanted: impl IntoIterator<Item = (usize, f32)>, pixels_per_point: f32) {
        self.textures_asked = true;
        let Some(data) = self.data.clone() else { return };
        let Some(thumbnails) = &mut self.thumbnails else { return };
        let mut requests = Vec::new();
        // The date calendar's days, while it is open (REQ-041-02): small, and over the tiles.
        let calendar = std::mem::take(&mut self.calendar_wanted);
        let calendar_width = pick_thumb_width(40.0, pixels_per_point);
        for (media, key) in &calendar {
            requests.push(TextureRequest {
                media: *media,
                path: thumbnails.cache.path(key, calendar_width),
                width: calendar_width,
            });
        }
        // The viewer's preview first: the largest thumbnail, unless one is loaded already (REQ-009-14).
        if let Some((_, id)) = self.viewed
            && let Some(item) = self.items.get(&id)
            && item.thumb_status != ThumbStatus::Error
            && thumbnails.streamer.get(id).is_none()
        {
            requests.push(TextureRequest {
                media: id,
                path: thumbnails.cache.path(&key_of(item), domain::thumbs::MAX_THUMB_WIDTH),
                width: domain::thumbs::MAX_THUMB_WIDTH,
            });
        }
        let strip = self.strip.clone().map(|index| (index, domain::viewer::FILMSTRIP_THUMB));
        for (index, points) in strip.chain(wanted) {
            let Some(item) = data.ids.get(index).and_then(|id| self.items.get(id)) else {
                continue;
            };
            if item.thumb_status == ThumbStatus::Error || self.failed.contains(&item.id) {
                continue;
            }
            let width = pick_thumb_width(points, pixels_per_point);
            requests.push(TextureRequest {
                media: item.id,
                path: thumbnails.cache.path(&key_of(item), width),
                width,
            });
        }
        thumbnails.streamer.request(requests);
        for media in thumbnails.streamer.take_missing() {
            let Some(service) = &thumbnails.service else { continue };
            let calendar_day = calendar.iter().any(|(id, _)| *id == media);
            if (calendar_day
                || self
                    .items
                    .get(&media)
                    .is_some_and(|item| item.thumb_status != ThumbStatus::Error))
                && self.awaiting.insert(media)
            {
                service.request(media);
            }
        }
    }
}
