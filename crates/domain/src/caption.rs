//! The desktop's caption button layout (REQ-018-10): which window controls the title bar shows, on which side and in
//! which order.

/// A window control in the title bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionButton {
    Minimize,
    Maximize,
    Close,
}

/// The window controls at the left and at the right of the title bar, each side in its order from left to right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonLayout {
    pub left: Vec<CaptionButton>,
    pub right: Vec<CaptionButton>,
}

impl Default for ButtonLayout {
    /// Minimize, maximize, close at the right, as on Windows.
    fn default() -> Self {
        Self {
            left: Vec::new(),
            right: vec![CaptionButton::Minimize, CaptionButton::Maximize, CaptionButton::Close],
        }
    }
}

impl ButtonLayout {
    /// GNOME's `org.gnome.desktop.wm.preferences button-layout`, as `gsettings get` prints it (`'appmenu:close'`):
    /// the buttons left of the colon, then those right of it, comma separated. Names the app has no button for
    /// (`appmenu`, `icon`, `spacer`) are left out, and a button named twice is shown once, where it comes first. `None`
    /// if the value has no colon.
    pub fn from_gnome(value: &str) -> Option<Self> {
        let value = value.trim().trim_matches('\'');
        let (left, right) = value.split_once(':')?;
        let names = |side: &str| -> Vec<CaptionButton> {
            side.split(',')
                .filter_map(|name| match name.trim() {
                    "minimize" => Some(CaptionButton::Minimize),
                    "maximize" => Some(CaptionButton::Maximize),
                    "close" => Some(CaptionButton::Close),
                    _ => None,
                })
                .collect()
        };
        Some(Self::without_repeats(names(left), names(right)))
    }

    /// KDE's `ButtonsOnLeft` and `ButtonsOnRight` of `kwinrc` (`[org.kde.kdecoration2]`), one letter per button:
    /// `I` minimize, `A` maximize, `X` close; the other letters are buttons the app doesn't have. An empty side is
    /// KDE's default for it: `MS` at the left (none of the app's) and `IAX` at the right.
    pub fn from_kde(left: &str, right: &str) -> Self {
        let letters = |side: &str, default: &str| -> Vec<CaptionButton> {
            let side = side.trim();
            let side = if side.is_empty() { default } else { side };
            side.chars()
                .filter_map(|letter| match letter {
                    'I' => Some(CaptionButton::Minimize),
                    'A' => Some(CaptionButton::Maximize),
                    'X' => Some(CaptionButton::Close),
                    _ => None,
                })
                .collect()
        };
        Self::without_repeats(letters(left, "MS"), letters(right, "IAX"))
    }

    fn without_repeats(left: Vec<CaptionButton>, right: Vec<CaptionButton>) -> Self {
        let mut seen = Vec::new();
        let mut keep = |button: &CaptionButton| {
            let first = !seen.contains(button);
            seen.push(*button);
            first
        };
        let left = left.into_iter().filter(&mut keep).collect();
        let right = right.into_iter().filter(&mut keep).collect();
        Self { left, right }
    }
}

#[cfg(test)]
mod tests {
    use super::CaptionButton::{Close, Maximize, Minimize};
    use super::*;

    fn layout(left: &[CaptionButton], right: &[CaptionButton]) -> ButtonLayout {
        ButtonLayout {
            left: left.to_vec(),
            right: right.to_vec(),
        }
    }

    #[test]
    fn req_018_10_reads_gnomes_button_layout() {
        // Ubuntu's default, as gsettings prints it.
        assert_eq!(
            ButtonLayout::from_gnome("':minimize,maximize,close'\n"),
            Some(ButtonLayout::default())
        );
        // GNOME's default: only close.
        assert_eq!(ButtonLayout::from_gnome("'appmenu:close'"), Some(layout(&[], &[Close])));
        // macOS-like, at the left, in the setting's order.
        assert_eq!(
            ButtonLayout::from_gnome("'close,minimize,maximize:appmenu'"),
            Some(layout(&[Close, Minimize, Maximize], &[]))
        );
        // Both sides, unknown names and spacers left out.
        assert_eq!(
            ButtonLayout::from_gnome("'close,spacer,icon:menu,minimize'"),
            Some(layout(&[Close], &[Minimize]))
        );
    }

    #[test]
    fn req_018_10_a_button_named_twice_is_shown_once() {
        assert_eq!(
            ButtonLayout::from_gnome("'close:minimize,close'"),
            Some(layout(&[Close], &[Minimize]))
        );
    }

    #[test]
    fn req_018_10_an_unreadable_gnome_value_is_none() {
        assert_eq!(ButtonLayout::from_gnome(""), None);
        assert_eq!(ButtonLayout::from_gnome("'close,minimize'"), None);
    }

    #[test]
    fn req_018_10_reads_kdes_button_layout() {
        // Unset: KDE's defaults.
        assert_eq!(ButtonLayout::from_kde("", "\n"), ButtonLayout::default());
        assert_eq!(
            ButtonLayout::from_kde("XIA\n", "M\n"),
            layout(&[Close, Minimize, Maximize], &[])
        );
        assert_eq!(ButtonLayout::from_kde("MS", "HX"), layout(&[], &[Close]));
    }
}
