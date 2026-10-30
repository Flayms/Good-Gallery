//! The appearance setting (REQ-020-01): follow the system, or always light or dark.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    /// Light or dark as the system (or the browser) is, dark where it doesn't say (REQ-020-02).
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// How it is stored.
    pub fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// A stored value; `None` for anything else.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|appearance| appearance.key() == key)
    }

    /// Its name in the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_020_01_defaults_to_the_system_and_reads_back_what_it_stores() {
        assert_eq!(Appearance::default(), Appearance::System);
        for appearance in Appearance::ALL {
            assert_eq!(Appearance::from_key(appearance.key()), Some(appearance));
        }
        assert_eq!(Appearance::from_key("sepia"), None);
        assert_eq!(Appearance::from_key("Light"), None);
    }
}
