//! The search shown (spec 007), content search (spec 036), tag suggestions and the sidebar's lists.

use super::*;

impl Gallery {
    pub fn search(&self) -> &GallerySearch {
        &self.search
    }

    /// Shows another search, from the top (spec 007).
    /// Lets content words match from now on (REQ-036-05); a search with words is loaded again.
    pub fn set_content_matcher(&mut self, matcher: Arc<dyn ContentMatches>) {
        *self.loader.content.lock().unwrap_or_else(|p| p.into_inner()) = Some(matcher);
        if self.search.content.is_some() {
            self.reload();
        }
    }

    /// How many photos content search's model still has to look at, for the empty state (REQ-036-06).
    pub fn set_content_left(&mut self, left: u64) {
        self.content_left = left;
    }

    /// Whether content search is on: a model is chosen (REQ-036-03).
    pub fn content_search_on(&self) -> bool {
        self.setting(db::content::MODEL_KEY).is_some()
    }

    /// Searches for these words in what the photos show, or not at all (REQ-036-05).
    pub fn set_content(&mut self, words: Option<String>) {
        let words = words.map(|words| words.split_whitespace().collect::<Vec<_>>().join(" "));
        self.set_search(GallerySearch {
            content: words.filter(|words| !words.is_empty()),
            ..self.search.clone()
        });
    }

    pub fn set_search(&mut self, search: GallerySearch) {
        if search == self.search {
            return;
        }
        let library_changed = search.library != self.search.library;
        self.search = search;
        // No hidden item stays selected (REQ-025-05).
        self.selection.clear();
        self.anchor = 0;
        self.anchor_id = None;
        self.scroll_to = Some(0.0);
        if !self.started {
            return;
        }
        self.generation += 1;
        self.loader.send(Request::Layout {
            generation: self.generation,
            search: self.search.clone(),
        });
        // The sidebar's lists don't depend on the filters, only its folder tree on the library.
        if library_changed {
            self.reload_sidebar();
        }
    }

    /// The sidebar's lists, once loaded.
    pub fn sidebar(&self) -> Option<&SidebarData> {
        self.sidebar.as_ref()
    }

    /// Existing tags by group, for adding one (REQ-026-01).
    pub fn tag_paths(&self) -> Arc<TagPaths> {
        self.tag_paths.clone()
    }

    /// Asks for tag suggestions for `prefix` (REQ-007-04); answered in [`Self::suggestions`].
    pub fn request_suggestions(&mut self, prefix: &str, limit: usize) {
        self.start();
        self.loader.send(Request::Suggest {
            prefix: prefix.to_string(),
            limit,
        });
    }

    /// The latest suggestions and the prefix they are for.
    pub fn suggestions(&self) -> Option<(&str, &[TagCount])> {
        self.suggestions
            .as_ref()
            .map(|(prefix, tags)| (prefix.as_str(), tags.as_slice()))
    }

    /// Whether the gallery is sorted by a date, so it can be grouped by day (REQ-016-04).
    pub fn is_date_sort(&self) -> bool {
        self.search.sort_by.is_date()
    }
}
