//! Which sidebar groups show, and in which order (REQ-008-10, REQ-008-11). The libraries always come first.

use serde_json::{Value, json};

/// A sidebar group the user can move or switch off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarGroup {
    People,
    Places,
    Ratings,
    Tags,
}

impl SidebarGroup {
    /// The default order.
    pub const ALL: [Self; 4] = [Self::People, Self::Places, Self::Ratings, Self::Tags];

    /// How it is stored; also the id of its collapsed state (`sidebar.group.<key>`).
    pub fn key(self) -> &'static str {
        match self {
            Self::People => "people",
            Self::Places => "places",
            Self::Ratings => "ratings",
            Self::Tags => "tags",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|group| group.key() == key)
    }

    /// Its name in the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::People => "People",
            Self::Places => "Places",
            Self::Ratings => "Ratings",
            Self::Tags => "Tags",
        }
    }
}

/// Every group once, in the user's order, each switched on or off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarLayout {
    groups: Vec<(SidebarGroup, bool)>,
}

impl Default for SidebarLayout {
    fn default() -> Self {
        Self {
            groups: SidebarGroup::ALL.iter().map(|&group| (group, true)).collect(),
        }
    }
}

impl SidebarLayout {
    /// All groups in order, with whether they are switched on.
    pub fn groups(&self) -> &[(SidebarGroup, bool)] {
        &self.groups
    }

    /// The switched-on groups in order.
    pub fn shown(&self) -> impl Iterator<Item = SidebarGroup> + '_ {
        self.groups.iter().filter(|(_, on)| *on).map(|&(group, _)| group)
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    fn position(&self, group: SidebarGroup) -> usize {
        self.groups
            .iter()
            .position(|&(g, _)| g == group)
            .expect("every group is in the layout")
    }

    /// Moves a group one place up; the first stays where it is.
    pub fn move_up(&mut self, group: SidebarGroup) {
        let i = self.position(group);
        if i > 0 {
            self.groups.swap(i, i - 1);
        }
    }

    /// Moves a group one place down; the last stays where it is.
    pub fn move_down(&mut self, group: SidebarGroup) {
        let i = self.position(group);
        if i + 1 < self.groups.len() {
            self.groups.swap(i, i + 1);
        }
    }

    pub fn set_shown(&mut self, group: SidebarGroup, on: bool) {
        let i = self.position(group);
        self.groups[i].1 = on;
    }

    /// How it is stored: `[{"group": "people", "shown": true}, …]`.
    pub fn to_value(&self) -> Value {
        self.groups
            .iter()
            .map(|&(group, on)| json!({ "group": group.key(), "shown": on }))
            .collect()
    }

    /// A stored value. Unknown and repeated entries are skipped, missing groups are added switched on at the end,
    /// and anything that isn't a list gives the default.
    pub fn from_value(value: &Value) -> Self {
        let mut groups: Vec<(SidebarGroup, bool)> = Vec::new();
        for entry in value.as_array().into_iter().flatten() {
            let Some(group) = entry
                .get("group")
                .and_then(Value::as_str)
                .and_then(SidebarGroup::from_key)
            else {
                continue;
            };
            if groups.iter().any(|&(g, _)| g == group) {
                continue;
            }
            let on = entry.get("shown").and_then(Value::as_bool).unwrap_or(true);
            groups.push((group, on));
        }
        for group in SidebarGroup::ALL {
            if !groups.iter().any(|&(g, _)| g == group) {
                groups.push((group, true));
            }
        }
        Self { groups }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SidebarGroup::*;

    fn order(layout: &SidebarLayout) -> Vec<SidebarGroup> {
        layout.groups().iter().map(|&(group, _)| group).collect()
    }

    #[test]
    fn req_008_10_moves_groups_and_switches_them_off() {
        let mut layout = SidebarLayout::default();
        layout.move_up(Ratings);
        layout.move_up(Ratings);
        layout.move_up(Ratings);
        assert_eq!(order(&layout), [Ratings, People, Places, Tags]);
        layout.move_down(People);
        layout.move_down(Tags);
        assert_eq!(order(&layout), [Ratings, Places, People, Tags]);

        layout.set_shown(Places, false);
        assert_eq!(layout.shown().collect::<Vec<_>>(), [Ratings, People, Tags]);
        layout.set_shown(Places, true);
        assert_eq!(layout.shown().collect::<Vec<_>>(), [Ratings, Places, People, Tags]);
    }

    #[test]
    fn req_008_11_reads_back_what_it_stores_and_repairs_odd_values() {
        let mut layout = SidebarLayout::default();
        layout.move_down(People);
        layout.set_shown(Tags, false);
        assert_eq!(SidebarLayout::from_value(&layout.to_value()), layout);

        let odd = json!([
            {"group": "tags", "shown": false},
            {"group": "albums", "shown": true},
            {"group": "tags", "shown": true},
            "places",
            {"group": "ratings"},
        ]);
        let repaired = SidebarLayout::from_value(&odd);
        assert_eq!(
            repaired.groups(),
            [(Tags, false), (Ratings, true), (People, true), (Places, true)]
        );
        assert_eq!(SidebarLayout::from_value(&json!("people")), SidebarLayout::default());
    }

    #[test]
    fn req_008_11_resets_to_the_default() {
        let default = SidebarLayout::default();
        assert_eq!(
            default.groups(),
            [(People, true), (Places, true), (Ratings, true), (Tags, true)]
        );
        assert!(default.is_default());
        let mut changed = default.clone();
        changed.set_shown(People, false);
        assert!(!changed.is_default());
        for group in SidebarGroup::ALL {
            assert_eq!(SidebarGroup::from_key(group.key()), Some(group));
        }
    }
}
