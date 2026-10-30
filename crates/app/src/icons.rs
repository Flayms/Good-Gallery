//! lucide icons (ISC, `assets/icons/LICENSE.lucide.txt`), embedded as SVG and rendered by egui_extras' SVG loader.
//! The SVGs are white, so [`Icon::image`] tints them to any color. Add one: list it in `assets/icons/make-icons.mjs`,
//! run the script, add a line to `icons!` below.

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
    PanelLeft => "panel-left",
    Pause => "pause",
    Play => "play",
    Plus => "plus",
    RefreshCw => "refresh-cw",
    Rows3 => "rows-3",
    Search => "search",
    Settings => "settings",
    SlidersHorizontal => "sliders-horizontal",
    StarOff => "star-off",
    Star => "star",
    Tag => "tag",
    Trash2 => "trash-2",
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
