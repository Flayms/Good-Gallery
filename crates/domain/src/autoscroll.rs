//! Autoscroll with the middle mouse button (REQ-006-14), as in a browser: holding the button and moving the pointer
//! scrolls until the button is released; a click without moving keeps scrolling until the next click.

/// Distance from where the button was pressed within which the gallery doesn't scroll, in points. Leaving it while the
/// button is held makes releasing the button stop autoscroll.
pub const DEAD_ZONE: f32 = 12.0;
/// Scroll speed per point beyond the dead zone, in points per second.
pub const SPEED: f32 = 8.0;
/// The fastest autoscroll, in points per second.
pub const MAX_SPEED: f32 = 8000.0;

/// Whether and how autoscroll is on. `origin` is the pointer's height where the middle button was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Autoscroll {
    #[default]
    Off,
    /// The middle button is down; `left_dead_zone` once the pointer got farther than [`DEAD_ZONE`] from `origin`.
    Held { origin: f32, left_dead_zone: bool },
    /// Started by a middle click: on until the next click.
    Latched { origin: f32 },
}

impl Autoscroll {
    pub fn is_on(self) -> bool {
        self != Self::Off
    }

    /// Where it started, while it is on.
    pub fn origin(self) -> Option<f32> {
        match self {
            Self::Off => None,
            Self::Held { origin, .. } | Self::Latched { origin } => Some(origin),
        }
    }

    /// The middle button went down at height `y`.
    pub fn press_middle(&mut self, y: f32) {
        *self = match self {
            Self::Off => Self::Held {
                origin: y,
                left_dead_zone: false,
            },
            // Clicking again stops it.
            Self::Held { .. } | Self::Latched { .. } => Self::Off,
        };
    }

    /// The pointer is at height `y`.
    pub fn pointer_at(&mut self, y: f32) {
        if let Self::Held { origin, left_dead_zone } = self
            && (y - *origin).abs() > DEAD_ZONE
        {
            *left_dead_zone = true;
        }
    }

    /// The middle button went up: stops after a drag, latches after a click.
    pub fn release_middle(&mut self) {
        if let Self::Held { origin, left_dead_zone } = *self {
            *self = if left_dead_zone {
                Self::Off
            } else {
                Self::Latched { origin }
            };
        }
    }

    /// Another button was pressed, or Escape.
    pub fn stop(&mut self) {
        *self = Self::Off;
    }

    /// How fast to scroll with the pointer at height `y`, in points per second (positive scrolls down).
    pub fn velocity(self, y: f32) -> f32 {
        let Some(origin) = self.origin() else { return 0.0 };
        let distance = y - origin;
        let beyond = (distance.abs() - DEAD_ZONE).max(0.0);
        (beyond * SPEED).min(MAX_SPEED).copysign(distance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_006_14_scrolls_toward_the_pointer_faster_the_farther_it_is() {
        let mut autoscroll = Autoscroll::default();
        assert_eq!(autoscroll.velocity(500.0), 0.0, "off");
        autoscroll.press_middle(100.0);
        assert_eq!(autoscroll.velocity(100.0 + DEAD_ZONE), 0.0, "within the dead zone");
        assert_eq!(autoscroll.velocity(100.0 - DEAD_ZONE), 0.0);
        let near = autoscroll.velocity(150.0);
        let far = autoscroll.velocity(300.0);
        assert!(near > 0.0 && far > near, "{near} {far}");
        assert_eq!(
            autoscroll.velocity(-200.0),
            -autoscroll.velocity(400.0),
            "up as fast as down"
        );
        assert_eq!(autoscroll.velocity(1_000_000.0), MAX_SPEED);
    }

    #[test]
    fn req_006_14_releasing_after_moving_stops_it() {
        let mut autoscroll = Autoscroll::default();
        autoscroll.press_middle(100.0);
        autoscroll.pointer_at(180.0);
        autoscroll.pointer_at(100.0);
        assert!(autoscroll.is_on());
        autoscroll.release_middle();
        assert_eq!(autoscroll, Autoscroll::Off);
    }

    #[test]
    fn req_006_14_a_click_keeps_it_on_until_the_next_click() {
        let mut autoscroll = Autoscroll::default();
        autoscroll.press_middle(100.0);
        autoscroll.pointer_at(100.0 + DEAD_ZONE);
        autoscroll.release_middle();
        assert_eq!(autoscroll, Autoscroll::Latched { origin: 100.0 });
        autoscroll.pointer_at(400.0);
        assert!(autoscroll.velocity(400.0) > 0.0);

        autoscroll.press_middle(400.0);
        assert_eq!(autoscroll, Autoscroll::Off, "a middle click stops it");
        autoscroll.release_middle();
        assert_eq!(autoscroll, Autoscroll::Off, "and the release doesn't start it again");

        autoscroll.press_middle(100.0);
        autoscroll.release_middle();
        autoscroll.stop();
        assert_eq!(autoscroll, Autoscroll::Off, "another button or Escape stops it");
    }
}
