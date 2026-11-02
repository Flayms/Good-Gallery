//! lucide icons (ISC, `assets/icons/LICENSE.lucide.txt`), embedded as SVG and rendered by egui_extras' SVG loader.
//! The SVGs are white, so [`Icon::image`] tints them to any color. Add one: add a line to `icons!` below and run
//! `cargo xtask icons`, which writes the SVG. `star-filled` is lucide's `star` with `fill="#fff"`.

use crate::theme;
use eframe::egui::{self, Color32, Image, Vec2};

/// Declares [`Icon`] with one variant per SVG file in `assets/icons/`, named once each.
macro_rules! icons {
    ($($variant:ident => $file:literal,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Icon {
            $($variant,)*
        }

        impl Icon {
            fn svg(self) -> (&'static str, &'static [u8]) {
                match self {
                    $(Self::$variant => (
                        concat!("bytes://icons/", $file, ".svg"),
                        include_bytes!(concat!("../assets/icons/", $file, ".svg")),
                    ),)*
                }
            }
        }
    };
}

icons! {
    ArrowDownWideNarrow => "arrow-down-wide-narrow",
    Crosshair => "crosshair",
    ArrowUpNarrowWide => "arrow-up-narrow-wide",
    Calendar => "calendar",
    Check => "check",
    ChevronDown => "chevron-down",
    ChevronLeft => "chevron-left",
    ChevronRight => "chevron-right",
    ChevronUp => "chevron-up",
    Circle => "circle",
    FileSearch => "file-search",
    File => "file",
    Film => "film",
    FolderOpen => "folder-open",
    FolderPlus => "folder-plus",
    Folder => "folder",
    GalleryHorizontal => "gallery-horizontal",
    ImageOff => "image-off",
    Image => "image",
    Images => "images",
    Info => "info",
    Layers => "layers",
    LayoutDashboard => "layout-dashboard",
    LayoutGrid => "layout-grid",
    List => "list",
    LoaderCircle => "loader-circle",
    MapPin => "map-pin",
    Minus => "minus",
    Pipette => "pipette",
    PanelLeft => "panel-left",
    Pause => "pause",
    Play => "play",
    Plus => "plus",
    Redo2 => "redo-2",
    RefreshCw => "refresh-cw",
    RotateCcw => "rotate-ccw",
    RotateCw => "rotate-cw",
    Rows3 => "rows-3",
    ScanFace => "scan-face",
    Search => "search",
    Settings => "settings",
    SlidersHorizontal => "sliders-horizontal",
    SquareSplitHorizontal => "square-split-horizontal",
    StarFilled => "star-filled",
    StarOff => "star-off",
    Star => "star",
    Tag => "tag",
    Trash2 => "trash-2",
    Undo2 => "undo-2",
    TriangleAlert => "triangle-alert",
    Unplug => "unplug",
    User => "user",
    Video => "video",
    Volume2 => "volume-2",
    VolumeX => "volume-x",
    X => "x",
    ZoomIn => "zoom-in",
    ZoomOut => "zoom-out",
}

impl Icon {
    /// The icon at `size` points in `color`.
    pub fn image(self, size: f32, color: Color32) -> Image<'static> {
        let (uri, bytes) = self.svg();
        Image::from_bytes(uri, bytes)
            .fit_to_exact_size(Vec2::splat(size))
            .tint(color)
    }

    /// The icon at the default size (16) in Paper.
    pub fn small(self) -> Image<'static> {
        self.image(theme::ICON_SIZE, theme::text())
    }
}

/// A rating star at `size` points: a given one filled in Star, one not given outlined in Muted (REQ-013-16).
pub fn rating_star(given: bool, size: f32) -> Image<'static> {
    let (icon, color) = rating_star_icon(given);
    icon.image(size, color)
}

fn rating_star_icon(given: bool) -> (Icon, Color32) {
    if given {
        (Icon::StarFilled, theme::star())
    } else {
        (Icon::Star, theme::muted())
    }
}

/// A frameless button showing only `icon`, named `label` for screen readers and tests, with `tooltip` on hover.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str, tooltip: &str) -> egui::Response {
    let response = ui
        .add(
            egui::Button::image(icon.small())
                .frame(false)
                .min_size(Vec2::splat(theme::CONTROL_HEIGHT - 4.0)),
        )
        .on_hover_text(tooltip);
    let label = label.to_string();
    let enabled = response.enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &label));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The opacity of the icon's center pixel, rendered at 24 px.
    fn center_alpha(icon: Icon) -> u8 {
        let image = egui_extras::image::load_svg_bytes(icon.svg().1, &Default::default()).unwrap();
        image[(image.size[0] / 2, image.size[1] / 2)].a()
    }

    #[test]
    fn req_013_16_given_stars_are_filled_and_missing_ones_outlined() {
        let (given, given_color) = rating_star_icon(true);
        let (missing, missing_color) = rating_star_icon(false);

        assert_eq!(center_alpha(given), 255, "a given star is filled");
        assert_eq!(given_color, theme::star());
        assert_eq!(center_alpha(missing), 0, "a star not given is only an outline");
        assert_eq!(missing_color, theme::muted());
    }
}
