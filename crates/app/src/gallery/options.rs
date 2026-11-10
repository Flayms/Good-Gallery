//! The view choices (spec 006, 016, 017) and storing them in the settings.

use super::*;

impl Gallery {
    pub fn view(&self) -> GalleryView {
        self.view
    }

    pub fn group_by_day(&self) -> bool {
        self.group_by_day
    }

    pub fn tag_indicators(&self) -> &TagIndicatorChoice {
        &self.indicators
    }

    pub(super) fn store(&self, key: &str, value: serde_json::Value) {
        let conn = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(error) = db::settings::set_value(&conn, key, &value) {
            tracing::warn!("can't store {key}: {error}");
        }
    }

    /// Shows the gallery in another view and remembers it (REQ-006-01).
    pub fn set_view(&mut self, view: GalleryView) {
        if view != self.view {
            self.view = view;
            self.store(VIEW_KEY, view.key().into());
        }
    }

    /// Shows or hides the day groups and remembers it (REQ-016-03).
    pub fn set_group_by_day(&mut self, on: bool) {
        if on != self.group_by_day {
            self.group_by_day = on;
            self.store(GROUP_BY_DAY_KEY, group_by_day_value(on).into());
        }
    }

    /// Shows or hides tag indicators, or changes the picked ones, and remembers it (REQ-017-03).
    pub fn set_tag_indicators(&mut self, choice: TagIndicatorChoice) {
        if choice != self.indicators {
            self.store(TAG_INDICATORS_KEY, choice.to_value());
            self.indicators = choice;
        }
    }

    /// Zooms by `steps` (positive adds columns) and remembers the level (REQ-006-06).
    pub fn zoom(&mut self, steps: i32) {
        self.set_columns(zoom::step_columns(self.columns, steps));
    }

    pub fn set_columns(&mut self, columns: usize) {
        let columns = columns.clamp(zoom::MIN_COLUMNS, zoom::MAX_COLUMNS);
        if columns != self.columns {
            self.columns = columns;
            self.store(COLUMNS_KEY, columns.into());
        }
    }

    /// A stored view choice, e.g. whether the viewer shows its filmstrip.
    pub fn setting(&self, key: &str) -> Option<String> {
        let conn = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        db::settings::value(&conn, key)
            .ok()
            .flatten()
            .and_then(|value| value.as_str().map(String::from))
    }

    /// Stores a view choice.
    pub fn store_setting(&self, key: &str, value: &str) {
        self.store(key, value.into());
    }
}
