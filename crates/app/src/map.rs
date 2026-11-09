//! The world map (spec 044): land and borders from bundled Natural Earth data, and the photos with a position as
//! clusters that show their newest photo and split as the map zooms in.

use crate::state::AppState;
use crate::theme;
use domain::map::{Cluster, MapView, project};
use domain::thumbs::ThumbKey;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, vec2};
use std::sync::OnceLock;

/// The bundled map, `cargo xtask map` writes it.
const DATA: &[u8] = include_bytes!("../data/world.deflate");
/// Points a cluster's grid cell is wide: closer photos merge.
/// Every cluster's radius, its white ring around it (REQ-044-01).
const RADIUS: f32 = 26.0;
const RING: f32 = 2.0;
/// How close two clusters' middles may come: their width and a gap (REQ-044-02).
const SPACING: f64 = (2.0 * (RADIUS + RING) + 8.0) as f64;
/// A wheel step of this many points zooms one level.
const POINTS_PER_LEVEL: f32 = 240.0;

/// A land polygon on the world square (Web Mercator, 0–1), triangulated.
struct Land {
    /// West, north, east, south on the world square.
    bounds: [f32; 4],
    vertices: Vec<[f32; 2]>,
    triangles: Vec<u32>,
    /// How many of the vertices each ring has, the outer ring first, then its holes.
    #[cfg_attr(not(test), allow(dead_code))]
    rings: Vec<u32>,
}

/// The world as drawn: land and the borders between countries, on the world square.
struct World {
    land: Vec<Land>,
    borders: Vec<Vec<[f32; 2]>>,
}

static WORLD: OnceLock<World> = OnceLock::new();

/// Reads the bundled map once; a damaged one draws nothing.
fn world() -> &'static World {
    WORLD.get_or_init(|| {
        let bytes = miniz_oxide::inflate::decompress_to_vec(DATA).unwrap_or_default();
        read_world(&bytes).unwrap_or(World {
            land: Vec::new(),
            borders: Vec::new(),
        })
    })
}

fn read_world(bytes: &[u8]) -> Option<World> {
    let mut at = bytes.strip_prefix(b"GGMAP1")?;
    let mut u32_ = || -> Option<u32> {
        let (head, rest) = at.split_first_chunk::<4>()?;
        at = rest;
        Some(u32::from_le_bytes(*head))
    };
    let mut values = Vec::new();
    // Reads `n` u32 words, kept as they are.
    let mut words = |n: usize, values: &mut Vec<u32>| -> Option<()> {
        values.clear();
        for _ in 0..n {
            values.push(u32_()?);
        }
        Some(())
    };
    let projected = |lon: u32, lat: u32| {
        let p = project(f64::from(f32::from_bits(lon)), f64::from(f32::from_bits(lat)));
        [p[0] as f32, p[1] as f32]
    };
    words(1, &mut values)?;
    let polygons = values[0] as usize;
    let mut land = Vec::with_capacity(polygons);
    for _ in 0..polygons {
        words(4, &mut values)?;
        let bounds = values.clone();
        words(1, &mut values)?;
        let n = values[0] as usize;
        words(2 * n, &mut values)?;
        let vertices: Vec<[f32; 2]> = values.chunks_exact(2).map(|p| projected(p[0], p[1])).collect();
        words(1, &mut values)?;
        let n = values[0] as usize;
        words(n, &mut values)?;
        let triangles = values.clone();
        words(1, &mut values)?;
        let rings = values[0] as usize;
        words(rings, &mut values)?;
        let rings = values.clone();
        let [west, north] = projected(bounds[0], bounds[3]);
        let [east, south] = projected(bounds[2], bounds[1]);
        land.push(Land {
            bounds: [west, north, east, south],
            vertices,
            triangles,
            rings,
        });
    }
    words(1, &mut values)?;
    let lines = values[0] as usize;
    let mut borders = Vec::with_capacity(lines);
    for _ in 0..lines {
        words(1, &mut values)?;
        let n = values[0] as usize;
        words(2 * n, &mut values)?;
        borders.push(values.chunks_exact(2).map(|p| projected(p[0], p[1])).collect());
    }
    Some(World { land, borders })
}

/// What a click on the map asked for.
enum Clicked {
    /// A cluster's photos, newest first.
    Photos(Vec<i64>),
}

/// The map page (REQ-044-01 … REQ-044-03).
pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    let header_height = crate::gallery::TOOLBAR_HEIGHT;
    let area = ui.max_rect();
    let map = Rect::from_min_max(area.min + vec2(0.0, header_height), area.max);
    let mut clicked = None;
    if let Some(gallery) = &mut state.gallery {
        gallery.begin_thumbnails();
        let points = gallery.map_points();
        let mut view = gallery.map_view;
        let size = [f64::from(map.width()), f64::from(map.height())];
        view.fit(size);
        let response = ui.interact(map, ui.make_persistent_id("map"), Sense::click_and_drag());
        navigate(ui, &response, map, &mut view);
        view.fit(size);
        let painter = ui.painter_at(map);
        painter.rect_filled(map, 0.0, ocean());
        paint_world(&painter, map, &view);
        match &points {
            None => {}
            Some(points) if points.is_empty() => {
                painter.text(
                    map.center(),
                    Align2::CENTER_CENTER,
                    "No photos with a GPS position here yet",
                    FontId::proportional(theme::SIZE_TITLE),
                    theme::muted(),
                );
            }
            Some(points) => {
                let clusters = gallery.map_clusters(points, domain::map::cluster_level(&view), SPACING);
                let mut wanted: Vec<(i64, ThumbKey)> = Vec::new();
                for cluster in clusters.iter() {
                    if let Some(ids) = cluster_ui(ui, gallery, map, &view, cluster, &mut wanted) {
                        clicked = Some(Clicked::Photos(ids));
                    }
                }
                gallery.want_thumbnails(&wanted);
            }
        }
        zoom_buttons(ui, map, &mut view);
        gallery.map_view = view;
    }
    let header = Rect::from_min_size(area.min, vec2(area.width(), header_height));
    crate::settings_page::page_header(ui, header, state, "Map");
    match clicked {
        // Its photos in the gallery, chosen on the map (REQ-044-03).
        Some(Clicked::Photos(ids)) => state.show_map_photos(ids),
        None => {}
    }
}

fn ocean() -> Color32 {
    theme::recess()
}

fn land_color() -> Color32 {
    theme::selected()
}

/// Dragging pans, the wheel and a pinch zoom around the pointer, a double-click zooms in (REQ-044-02).
fn navigate(ui: &egui::Ui, response: &egui::Response, map: Rect, view: &mut MapView) {
    if response.dragged() {
        let delta = response.drag_delta();
        view.pan([f64::from(delta.x), f64::from(delta.y)]);
    }
    let center = map.center();
    if let Some(pointer) = response.hover_pos() {
        let at = [f64::from(pointer.x - center.x), f64::from(pointer.y - center.y)];
        let (scroll, pinch) = ui.input(|input| (input.smooth_scroll_delta.y, input.zoom_delta()));
        if scroll != 0.0 {
            view.zoom_at(f64::from(scroll / POINTS_PER_LEVEL), at);
        }
        if pinch != 1.0 {
            view.zoom_at(f64::from(pinch.log2()), at);
        }
        if response.double_clicked() {
            view.zoom_at(1.0, at);
        }
    }
}

/// Land and borders where the view shows them.
fn paint_world(painter: &egui::Painter, map: Rect, view: &MapView) {
    let world = world();
    let center = map.center();
    let scale = view.scale() as f32;
    let [cx, cy] = view.center.map(|c| c as f32);
    let to_screen = |[x, y]: [f32; 2]| Pos2::new(center.x + (x - cx) * scale, center.y + (y - cy) * scale);
    let shown = |[west, north, east, south]: [f32; 4]| {
        let rect = Rect::from_min_max(to_screen([west, north]), to_screen([east, south]));
        rect.intersects(map)
    };
    let mut mesh = egui::Mesh::default();
    for land in world.land.iter().filter(|land| shown(land.bounds)) {
        let base = mesh.vertices.len() as u32;
        for &vertex in &land.vertices {
            mesh.colored_vertex(to_screen(vertex), land_color());
        }
        mesh.indices.extend(land.triangles.iter().map(|index| base + index));
    }
    painter.add(egui::Shape::mesh(mesh));
    let border = Stroke::new(0.7, theme::rim());
    for line in &world.borders {
        let points: Vec<Pos2> = line.iter().map(|&p| to_screen(p)).collect();
        let bounds = Rect::from_points(&points);
        if bounds.intersects(map) && bounds.size().max_elem() > 2.0 {
            painter.add(egui::Shape::line(points, border));
        }
    }
}

/// How a cluster is called, to the pointer and to screen readers: "12 photos near Munich".
fn cluster_name(cluster: &Cluster) -> String {
    let count = cluster.ids.len();
    let photos = if count == 1 {
        "1 photo".to_owned()
    } else {
        format!("{} photos", crate::format::count(count as u64))
    };
    let (longitude, latitude) = domain::map::unproject(cluster.center);
    let place = domain::media::Location::new(latitude, longitude).and_then(domain::places::place_at);
    match place {
        Some(place) => format!("{photos} near {}", place.city),
        None => photos,
    }
}

/// One cluster: its newest photo in a circle, and how many it holds; the ids when it was clicked.
fn cluster_ui(
    ui: &egui::Ui,
    gallery: &mut crate::gallery::Gallery,
    map: Rect,
    view: &MapView,
    cluster: &Cluster,
    wanted: &mut Vec<(i64, ThumbKey)>,
) -> Option<Vec<i64>> {
    let [x, y] = view.to_screen(cluster.center);
    let at = map.center() + vec2(x as f32, y as f32);
    let count = cluster.ids.len();
    let radius = RADIUS;
    let rect = Rect::from_center_size(at, vec2(radius, radius) * 2.0);
    if !rect.intersects(map) {
        return None;
    }
    let name = cluster_name(cluster);
    let response = ui
        .interact(rect, ui.scope_id().with(("cluster", cluster.ids[0])), Sense::click())
        .on_hover_text(&name)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    let painter = ui.painter_at(map);
    painter.circle_filled(at + vec2(0.0, 2.0), radius + RING, Color32::from_black_alpha(90));
    painter.circle_filled(at, radius + RING, Color32::WHITE);
    let newest = cluster.ids[0];
    let texture = gallery.map_key(newest).and_then(|key| {
        wanted.push((newest, key));
        gallery.thumbnail(newest)
    });
    let inner = Rect::from_center_size(at, vec2(radius, radius) * 2.0);
    match texture {
        Some(texture) => {
            egui::Image::new((texture.id, inner.size()))
                .uv(square_uv(texture.size))
                .corner_radius(radius)
                .paint_at(ui, inner);
        }
        None => {
            painter.circle_filled(at, radius, theme::hover());
            let pin = Rect::from_center_size(at, vec2(16.0, 16.0));
            crate::icons::Icon::MapPin.image(16.0, theme::muted()).paint_at(ui, pin);
        }
    }
    if response.hovered() {
        painter.circle_stroke(at, radius + RING, Stroke::new(2.0, theme::text()));
    }
    if count > 1 {
        let text = crate::format::count(count as u64);
        let galley = painter.layout_no_wrap(text, FontId::proportional(theme::SIZE_CAPTION), Color32::WHITE);
        let badge = Rect::from_center_size(
            at + vec2(radius * 0.75, -radius * 0.75),
            galley.size() + vec2(10.0, 4.0),
        );
        painter.rect_filled(badge, badge.height() / 2.0, Color32::from_rgb(30, 30, 30));
        painter.galley(badge.center() - galley.size() / 2.0, galley, Color32::WHITE);
    }
    response.clicked().then(|| cluster.ids.clone())
}

/// The middle square of a texture.
fn square_uv([width, height]: [u32; 2]) -> Rect {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    if w >= h {
        let side = h / w;
        Rect::from_min_size(egui::pos2((1.0 - side) / 2.0, 0.0), vec2(side, 1.0))
    } else {
        let side = w / h;
        Rect::from_min_size(egui::pos2(0.0, (1.0 - side) / 2.0), vec2(1.0, side))
    }
}

/// _Zoom in_ and _Zoom out_ at the map's bottom right (REQ-044-02).
fn zoom_buttons(ui: &mut egui::Ui, map: Rect, view: &mut MapView) {
    let size = vec2(theme::CONTROL_HEIGHT, theme::CONTROL_HEIGHT);
    for (i, (label, icon, steps)) in [
        ("Zoom out", crate::icons::Icon::Minus, -1.0),
        ("Zoom in", crate::icons::Icon::Plus, 1.0),
    ]
    .into_iter()
    .enumerate()
    {
        let rect = Rect::from_min_size(
            map.right_bottom() - vec2(16.0 + size.x, 16.0 + size.y * (i as f32 + 1.0) + 4.0 * i as f32),
            size,
        );
        let response = ui.put(rect, egui::Button::image(icon.small()).fill(theme::raised()));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        if response.on_hover_text(label).clicked() {
            view.zoom_at(steps, [0.0, 0.0]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-044-01: the bundled map reads, with the continents' land and the countries' borders.
    #[test]
    fn req_044_01_reads_the_bundled_world() {
        let world = world();
        assert!(world.land.len() > 1000, "{} land polygons", world.land.len());
        assert!(world.borders.len() > 100, "{} borders", world.borders.len());
        // Somewhere in central Europe is land.
        let germany = project(10.0, 51.0).map(|c| c as f32);
        assert!(world.land.iter().any(|land| {
            let [west, north, east, south] = land.bounds;
            (west..=east).contains(&germany[0]) && (north..=south).contains(&germany[1])
        }));
        assert!(world.land.iter().all(|land| land.triangles.len() % 3 == 0));
    }

    /// REQ-044-01: on the projected map, the land's triangles cover each polygon once: no overlaps that would show
    /// brighter, no gaps (#297). Their areas add up to the polygon's, its holes taken out.
    #[test]
    fn req_044_01_land_triangles_cover_each_polygon_once() {
        let at = |land: &Land, i: u32| land.vertices[i as usize].map(f64::from);
        let twice_area =
            |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        let mut worst = (0.0, 0);
        for (index, land) in world().land.iter().enumerate() {
            let triangles: f64 = land
                .triangles
                .chunks_exact(3)
                .map(|t| twice_area(at(land, t[0]), at(land, t[1]), at(land, t[2])).abs())
                .sum();
            let mut start = 0;
            let mut polygon = 0.0;
            for (ring, &count) in land.rings.iter().enumerate() {
                let ring_area: f64 = (0..count)
                    .map(|i| {
                        let (a, b) = (at(land, start + i), at(land, start + (i + 1) % count));
                        a[0] * b[1] - b[0] * a[1]
                    })
                    .sum::<f64>()
                    .abs();
                polygon += if ring == 0 { ring_area } else { -ring_area };
                start += count;
            }
            let off = (triangles - polygon).abs() / polygon.max(1e-12);
            if off > worst.0 {
                worst = (off, index);
            }
        }
        assert!(worst.0 < 1e-3, "polygon {} is off by {:.4}%", worst.1, worst.0 * 100.0);
    }
}
