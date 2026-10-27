//! Originals for the viewer (REQ-009-04, REQ-009-13): worker threads decode the wanted photos from the library, the
//! shown one first, then its neighbours nearest first. Like thumbnails (`textures`), a worker builds the mip chain and
//! creates the `wgpu::Texture` itself, so even a 24 MP photo never stalls a frame. Without a wgpu render state (tests)
//! decoded photos go through egui's own textures.

use crate::textures::{mip_chain, register, upload};
use eframe::egui::{self, TextureId};
use eframe::egui_wgpu::{self, wgpu};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};

/// Decoding a large JPEG takes one core for a while; two keep the shown photo and the next one going.
const WORKERS: usize = 2;

/// A photo the viewer wants: the media item and its file in the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginalRequest {
    pub media: i64,
    pub path: PathBuf,
}

/// How far an original is.
#[derive(Debug, Clone, PartialEq)]
pub enum OriginalState {
    /// Queued or decoding, or not wanted.
    Loading,
    Loaded(Loaded),
    /// The file couldn't be read or decoded (REQ-009-15).
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loaded {
    pub id: TextureId,
    /// Pixels.
    pub size: [u32; 2],
}

enum Texture {
    Gpu(wgpu::Texture),
    Egui(#[expect(dead_code, reason = "held: egui frees the texture with its last handle")] egui::TextureHandle),
}

enum Decoded {
    Gpu(wgpu::Texture),
    Egui(egui::ColorImage),
}

struct Done {
    media: i64,
    outcome: Outcome,
}

enum Outcome {
    Loaded([u32; 2], Decoded),
    Failed(String),
    /// Left the wanted set before or while it loaded.
    Cancelled,
}

#[derive(Default)]
struct Queue {
    /// Not started yet, in priority order; replaced by [`Originals::set`].
    waiting: Vec<OriginalRequest>,
    /// The whole wanted set: a worker checks it before the expensive steps.
    wanted: HashSet<i64>,
    /// Taken by a worker and not yet applied by the UI thread, so they aren't queued twice.
    in_flight: HashSet<i64>,
    stop: bool,
}

pub struct Originals {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    results: Receiver<Done>,
    ctx: egui::Context,
    render_state: Option<egui_wgpu::RenderState>,
    loaded: HashMap<i64, (Loaded, Texture)>,
    failed: HashMap<i64, String>,
    wanted: HashSet<i64>,
    /// A cancelled load came back for a photo that is wanted again: the next [`Originals::set`] queues it anew.
    requeue: bool,
}

impl Originals {
    /// Starts the workers; they make wgpu textures with `render_state`, else egui textures. Without `ffmpeg`, AVIF
    /// originals fail.
    pub fn new(ctx: egui::Context, render_state: Option<egui_wgpu::RenderState>, ffmpeg: Option<PathBuf>) -> Self {
        let max_side = match &render_state {
            Some(render_state) => render_state.device.limits().max_texture_dimension_2d,
            None => ctx.input(|input| input.max_texture_side) as u32,
        };
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let (tx, results) = channel();
        for i in 0..WORKERS {
            let (queue, tx, ffmpeg) = (queue.clone(), tx.clone(), ffmpeg.clone());
            let (ctx, render_state) = (ctx.clone(), render_state.clone());
            // Detached: a worker may hang in a read from a stalled network share, which must not hold up closing.
            std::thread::Builder::new()
                .name(format!("originals {i}"))
                .spawn(move || work(&queue, &tx, ffmpeg.as_deref(), render_state.as_ref(), max_side, &ctx))
                .expect("can't start an originals thread");
        }
        Self {
            queue,
            results,
            ctx,
            render_state,
            loaded: HashMap::new(),
            failed: HashMap::new(),
            wanted: HashSet::new(),
            requeue: false,
        }
    }

    /// Replaces the wanted photos, most important first. Photos that drop out stop loading and are freed; a failed one
    /// is tried again once it is wanted anew.
    pub fn set(&mut self, wanted: Vec<OriginalRequest>) {
        self.poll();
        let set: HashSet<i64> = wanted.iter().map(|request| request.media).collect();
        if set == self.wanted && !self.requeue && self.queue_is_current(&wanted) {
            return;
        }
        self.requeue = false;
        for media in self.loaded.keys().copied().collect::<Vec<_>>() {
            if !set.contains(&media) {
                self.free(media);
            }
        }
        self.failed.retain(|media, _| set.contains(media));
        let waiting = wanted
            .into_iter()
            .filter(|request| !self.loaded.contains_key(&request.media) && !self.failed.contains_key(&request.media))
            .collect();
        let (lock, wake) = &*self.queue;
        let mut queue = lock.lock().unwrap_or_else(|p| p.into_inner());
        queue.waiting = waiting;
        queue.wanted = set.clone();
        wake.notify_all();
        self.wanted = set;
    }

    /// Whether the same set is wanted and nothing was taken out of order: then the queue needs no update.
    fn queue_is_current(&self, wanted: &[OriginalRequest]) -> bool {
        // A request's path may change (a rescan moved the file); compare whole requests of what is still waiting.
        let queue = self.queue.0.lock().unwrap_or_else(|p| p.into_inner());
        queue.waiting.iter().all(|request| wanted.contains(request))
    }

    /// Applies finished loads. Called by [`Self::set`]; call it once per frame otherwise.
    pub fn poll(&mut self) {
        while let Ok(done) = self.results.try_recv() {
            self.apply(done);
        }
    }

    fn apply(&mut self, done: Done) {
        self.queue
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .in_flight
            .remove(&done.media);
        let wanted = self.wanted.contains(&done.media);
        match done.outcome {
            Outcome::Loaded(size, decoded) => {
                if !wanted {
                    // Stepped away meanwhile (REQ-009-13).
                    if let Decoded::Gpu(texture) = decoded {
                        texture.destroy();
                    }
                    return;
                }
                let (id, texture) = match (decoded, &self.render_state) {
                    (Decoded::Gpu(texture), Some(render_state)) => {
                        (register(render_state, &texture), Texture::Gpu(texture))
                    }
                    (Decoded::Gpu(texture), None) => {
                        texture.destroy();
                        return;
                    }
                    (Decoded::Egui(image), _) => {
                        let handle = self.ctx.load_texture(
                            format!("original {}", done.media),
                            image,
                            egui::TextureOptions::LINEAR,
                        );
                        (handle.id(), Texture::Egui(handle))
                    }
                };
                let loaded = Loaded { id, size };
                self.loaded.insert(done.media, (loaded, texture));
            }
            Outcome::Failed(error) => {
                if wanted {
                    tracing::warn!("can't load an original: {error}");
                    self.failed.insert(done.media, error);
                }
            }
            Outcome::Cancelled => self.requeue |= wanted,
        }
    }

    fn free(&mut self, media: i64) {
        let Some((loaded, texture)) = self.loaded.remove(&media) else {
            return;
        };
        // egui frees its own textures with their last handle.
        if let (Texture::Gpu(texture), Some(render_state)) = (texture, &self.render_state) {
            render_state.renderer.write().free_texture(&loaded.id);
            texture.destroy();
        }
    }

    pub fn get(&self, media: i64) -> OriginalState {
        if let Some((loaded, _)) = self.loaded.get(&media) {
            OriginalState::Loaded(*loaded)
        } else if let Some(error) = self.failed.get(&media) {
            OriginalState::Failed(error.clone())
        } else {
            OriginalState::Loading
        }
    }

    /// Waits until every wanted photo has loaded or failed, and applies them (tests).
    pub fn wait(&mut self) {
        loop {
            self.poll();
            let busy = {
                let queue = self.queue.0.lock().unwrap_or_else(|p| p.into_inner());
                !queue.waiting.is_empty() || !queue.in_flight.is_empty()
            };
            if !busy {
                return;
            }
            if let Ok(done) = self.results.recv_timeout(std::time::Duration::from_millis(50)) {
                self.apply(done);
            }
        }
    }
}

impl Drop for Originals {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        lock.lock().unwrap_or_else(|p| p.into_inner()).stop = true;
        wake.notify_all();
        for media in self.loaded.keys().copied().collect::<Vec<_>>() {
            self.free(media);
        }
    }
}

fn next_request(queue: &(Mutex<Queue>, Condvar)) -> Option<OriginalRequest> {
    let (lock, wake) = queue;
    let mut queue = lock.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        if queue.stop {
            return None;
        }
        while !queue.waiting.is_empty() {
            let request = queue.waiting.remove(0);
            if queue.in_flight.insert(request.media) {
                return Some(request);
            }
        }
        queue = wake.wait(queue).unwrap_or_else(|p| p.into_inner());
    }
}

fn still_wanted(queue: &(Mutex<Queue>, Condvar), media: i64) -> bool {
    queue
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .wanted
        .contains(&media)
}

fn work(
    queue: &(Mutex<Queue>, Condvar),
    results: &Sender<Done>,
    ffmpeg: Option<&std::path::Path>,
    render_state: Option<&egui_wgpu::RenderState>,
    max_side: u32,
    ctx: &egui::Context,
) {
    media::helpers::lower_thread_priority();
    while let Some(request) = next_request(queue) {
        let outcome = if still_wanted(queue, request.media) {
            load(&request, ffmpeg, render_state, max_side, || {
                still_wanted(queue, request.media)
            })
        } else {
            Outcome::Cancelled
        };
        if results
            .send(Done {
                media: request.media,
                outcome,
            })
            .is_err()
        {
            return;
        }
        ctx.request_repaint();
    }
}

fn load(
    request: &OriginalRequest,
    ffmpeg: Option<&std::path::Path>,
    render_state: Option<&egui_wgpu::RenderState>,
    max_side: u32,
    wanted: impl Fn() -> bool,
) -> Outcome {
    let image = match media::original::load_original(&request.path, ffmpeg, max_side) {
        Ok(image) => image,
        Err(error) => return Outcome::Failed(format!("{}: {error}", request.path.display())),
    };
    let size = [image.width(), image.height()];
    if !wanted() {
        return Outcome::Cancelled;
    }
    let decoded = match render_state {
        Some(render_state) => {
            let levels = mip_chain(size, image.into_raw());
            Decoded::Gpu(upload(&render_state.device, &render_state.queue, size, &levels))
        }
        None => Decoded::Egui(egui::ColorImage::from_rgba_unmultiplied(
            [size[0] as usize, size[1] as usize],
            image.as_raw(),
        )),
    };
    Outcome::Loaded(size, decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(dir: &std::path::Path, name: &str, width: u32, height: u32) -> PathBuf {
        let path = dir.join(name);
        image::RgbImage::from_pixel(width, height, image::Rgb([200, 30, 30]))
            .save(&path)
            .unwrap();
        path
    }

    fn request(media: i64, path: PathBuf) -> OriginalRequest {
        OriginalRequest { media, path }
    }

    fn originals() -> Originals {
        Originals::new(egui::Context::default(), None, None)
    }

    #[test]
    fn req_009_04_req_009_13_loads_the_wanted_originals_at_full_size() {
        let dir = tempfile::tempdir().unwrap();
        let mut originals = originals();
        originals.set(vec![
            request(1, photo(dir.path(), "a.png", 300, 200)),
            request(2, photo(dir.path(), "b.png", 40, 60)),
        ]);
        originals.wait();
        let OriginalState::Loaded(a) = originals.get(1) else {
            panic!("not loaded")
        };
        assert_eq!(a.size, [300, 200]);
        assert!(matches!(originals.get(2), OriginalState::Loaded(_)));
    }

    #[test]
    fn req_009_13_frees_originals_that_are_no_longer_wanted() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (photo(dir.path(), "a.png", 30, 20), photo(dir.path(), "b.png", 30, 20));
        let mut originals = originals();
        originals.set(vec![request(1, a.clone()), request(2, b.clone())]);
        originals.wait();
        originals.set(vec![request(2, b)]);
        assert_eq!(originals.get(1), OriginalState::Loading);
        assert!(matches!(originals.get(2), OriginalState::Loaded(_)));
        // Wanted again: loaded anew.
        originals.set(vec![request(1, a)]);
        originals.wait();
        assert!(matches!(originals.get(1), OriginalState::Loaded(_)));
    }

    #[test]
    fn req_009_13_drops_photos_that_stopped_being_wanted_before_they_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let mut originals = originals();
        let requests: Vec<_> = (0..20)
            .map(|i| request(i, photo(dir.path(), &format!("{i}.png"), 400, 300)))
            .collect();
        originals.set(requests);
        originals.set(Vec::new());
        originals.wait();
        assert!(originals.loaded.is_empty());
    }

    /// REQ-009-13: a photo dropped while it loaded and wanted again right after loads; it isn't taken as failed.
    #[test]
    fn req_009_13_loads_a_photo_wanted_again_while_its_cancelled_load_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let mut originals = originals();
        for i in 0..20 {
            let wanted = request(i, photo(dir.path(), &format!("{i}.png"), 600, 400));
            originals.set(vec![wanted.clone()]);
            originals.set(Vec::new());
            originals.set(vec![wanted.clone()]);
            originals.wait();
            // The viewer asks every frame.
            originals.set(vec![wanted]);
            originals.wait();
            assert!(
                matches!(originals.get(i), OriginalState::Loaded(_)),
                "photo {i}: {:?}",
                originals.get(i)
            );
        }
    }

    #[test]
    fn req_009_15_remembers_failures_until_wanted_anew() {
        let dir = tempfile::tempdir().unwrap();
        let gone = request(1, dir.path().join("gone.jpg"));
        let mut originals = originals();
        originals.set(vec![gone.clone()]);
        originals.wait();
        assert!(matches!(originals.get(1), OriginalState::Failed(_)));
        originals.set(vec![gone.clone()]);
        assert!(
            matches!(originals.get(1), OriginalState::Failed(_)),
            "not retried every frame"
        );
        originals.set(Vec::new());
        photo(dir.path(), "gone.jpg", 10, 10);
        originals.set(vec![gone]);
        originals.wait();
        assert!(matches!(originals.get(1), OriginalState::Loaded(_)));
    }
}
