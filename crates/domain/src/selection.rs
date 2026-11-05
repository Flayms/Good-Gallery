//! The gallery's selection (spec 025): some items, or all items of the search except some, and the anchor that a
//! range starts from.

use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Held {
    /// These items.
    Some(HashSet<i64>),
    /// All items of the search, except these: Ctrl+A on 500,000 items costs nothing (REQ-025-04).
    AllExcept(HashSet<i64>),
}

impl Default for Held {
    fn default() -> Self {
        Self::Some(HashSet::new())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    held: Held,
    /// The item last selected or deselected, where a range starts (REQ-025-02).
    anchor: Option<i64>,
}

impl Selection {
    /// Whether item `id` is selected.
    pub fn contains(&self, id: i64) -> bool {
        match &self.held {
            Held::Some(ids) => ids.contains(&id),
            Held::AllExcept(except) => !except.contains(&id),
        }
    }

    /// How many of the search's `ids` are selected.
    pub fn count(&self, ids: &[i64]) -> usize {
        match &self.held {
            Held::Some(selected) => selected.len(),
            // `except` only holds items of the search (see `retain`).
            Held::AllExcept(except) => ids.len().saturating_sub(except.len()),
        }
    }

    /// Whether nothing is selected, out of the search's `ids`.
    pub fn is_empty(&self, ids: &[i64]) -> bool {
        self.count(ids) == 0
    }

    /// Selects `id`, or deselects it if it was, keeping the others; it becomes the anchor (REQ-025-11, REQ-025-12).
    /// Deselecting the last item leaves no anchor, as there is no selection left to start a range from.
    pub fn toggle(&mut self, id: i64) {
        let (Held::Some(ids) | Held::AllExcept(ids)) = &mut self.held;
        if !ids.remove(&id) {
            ids.insert(id);
        }
        self.anchor = Some(id);
        if matches!(&self.held, Held::Some(ids) if ids.is_empty()) {
            self.anchor = None;
        }
    }

    /// The positions in the search's `ids` that a Shift-click on `id` covers: from the anchor to it, or only it without
    /// an anchor (or with one that left the search). `None` if `id` isn't in the search (REQ-025-02, REQ-025-14).
    pub fn range_to(&self, ids: &[i64], id: i64) -> Option<std::ops::RangeInclusive<usize>> {
        let position = |wanted: i64| ids.iter().position(|&other| other == wanted);
        let to = position(id)?;
        let from = self.anchor.and_then(position).unwrap_or(to);
        Some(from.min(to)..=from.max(to))
    }

    /// Adds the items from the anchor to `id`, in the order of the search's `ids`; without an anchor (or one that
    /// left the search) only `id`. `id` becomes the anchor (REQ-025-02).
    pub fn add_range(&mut self, ids: &[i64], id: i64) {
        let range = match self.range_to(ids, id) {
            Some(range) => &ids[range],
            None => std::slice::from_ref(&id),
        };
        match &mut self.held {
            Held::Some(selected) => selected.extend(range),
            Held::AllExcept(except) => range.iter().for_each(|id| {
                except.remove(id);
            }),
        }
        self.anchor = Some(id);
    }

    /// Selects every item of the search (REQ-025-04).
    pub fn select_all(&mut self) {
        self.held = Held::AllExcept(HashSet::new());
    }

    /// Selects nothing, and forgets the anchor (REQ-025-04, REQ-025-05).
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Forgets items that are no longer in the search's `ids`, e.g. removed from the index (REQ-025-05).
    pub fn retain(&mut self, ids: &[i64]) {
        let present: HashSet<i64> = ids.iter().copied().collect();
        let (Held::Some(held) | Held::AllExcept(held)) = &mut self.held;
        held.retain(|id| present.contains(id));
        if self.anchor.is_some_and(|anchor| !present.contains(&anchor)) {
            self.anchor = None;
        }
    }

    /// The selected items, in the order of the search's `ids`.
    pub fn ids(&self, ids: &[i64]) -> Vec<i64> {
        ids.iter().copied().filter(|&id| self.contains(id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDS: [i64; 6] = [10, 11, 12, 13, 14, 15];

    /// REQ-025-14: what a Shift-click would add, either way round; only the item without an anchor.
    #[test]
    fn req_025_14_says_what_a_shift_click_covers() {
        let mut selection = Selection::default();
        assert_eq!(selection.range_to(&IDS, 13), Some(3..=3));
        selection.toggle(11);
        assert_eq!(selection.range_to(&IDS, 14), Some(1..=4));
        assert_eq!(selection.range_to(&IDS, 10), Some(0..=1));
        assert_eq!(selection.range_to(&IDS, 99), None);
    }

    /// REQ-025-12: deselecting the last item leaves nothing for a range to start from.
    #[test]
    fn req_025_12_the_last_item_leaves_no_anchor() {
        let mut selection = Selection::default();
        selection.toggle(11);
        selection.toggle(11);
        assert_eq!(selection.range_to(&IDS, 14), Some(4..=4));
    }

    /// REQ-025-12: selecting toggles an item, keeping the others.
    #[test]
    fn req_025_12_toggles_items() {
        let mut selection = Selection::default();
        assert!(selection.is_empty(&IDS));
        selection.toggle(12);
        selection.toggle(14);
        assert!(selection.contains(12) && selection.contains(14) && !selection.contains(13));
        assert_eq!(selection.count(&IDS), 2);
        selection.toggle(12);
        assert_eq!(selection.ids(&IDS), [14]);
        selection.toggle(14);
        assert!(selection.is_empty(&IDS));
    }

    /// REQ-025-02: Shift-click adds the items between the anchor and the clicked one, either way round.
    #[test]
    fn req_025_02_adds_the_range_from_the_anchor() {
        let mut selection = Selection::default();
        selection.add_range(&IDS, 13);
        assert_eq!(selection.ids(&IDS), [13], "without an anchor, only the clicked item");

        selection.toggle(11);
        selection.add_range(&IDS, 14);
        assert_eq!(selection.ids(&IDS), [11, 12, 13, 14]);

        let mut selection = Selection::default();
        selection.toggle(15);
        selection.add_range(&IDS, 13);
        assert_eq!(selection.ids(&IDS), [13, 14, 15]);
        // The clicked item is the next range's anchor.
        selection.add_range(&IDS, 11);
        assert_eq!(selection.ids(&IDS), [11, 12, 13, 14, 15]);

        let mut selection = Selection::default();
        selection.toggle(99);
        selection.toggle(98);
        selection.toggle(98);
        selection.retain(&IDS);
        selection.add_range(&IDS, 10);
        assert_eq!(
            selection.ids(&IDS),
            [10],
            "an anchor that left the search counts as none"
        );
    }

    /// REQ-025-04: Ctrl+A selects all of the search, and toggling then deselects single items; Escape clears.
    #[test]
    fn req_025_04_selects_all_and_clears() {
        let mut selection = Selection::default();
        selection.select_all();
        assert_eq!(selection.count(&IDS), 6);
        assert!(selection.contains(99), "all of the search, also items not loaded yet");
        selection.toggle(12);
        assert_eq!(selection.ids(&IDS), [10, 11, 13, 14, 15]);
        selection.toggle(12);
        assert_eq!(selection.count(&IDS), 6);

        selection.clear();
        assert!(selection.is_empty(&IDS));
        selection.add_range(&IDS, 14);
        assert_eq!(selection.ids(&IDS), [14], "clearing forgets the anchor");
    }

    /// REQ-025-05: items that left the search leave the selection, and an emptied selection is empty.
    #[test]
    fn req_025_05_forgets_items_that_left() {
        let mut selection = Selection::default();
        selection.toggle(11);
        selection.toggle(13);
        selection.retain(&[10, 12, 13]);
        assert_eq!(selection.ids(&[10, 12, 13]), [13]);
        assert!(!selection.contains(11));

        let mut all = Selection::default();
        all.select_all();
        all.toggle(11);
        all.retain(&[10, 12]);
        assert_eq!(all.count(&[10, 12]), 2);
        assert_eq!(
            all.count(&[10, 11, 12]),
            3,
            "the left item isn't remembered as deselected"
        );
    }
}
