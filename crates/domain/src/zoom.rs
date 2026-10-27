//! Gallery zoom (REQ-006-06): the number of Masonry and Grid columns, changed in steps.

pub const MIN_COLUMNS: usize = 2;
pub const MAX_COLUMNS: usize = 12;
pub const DEFAULT_COLUMNS: usize = 5;

/// Wheel distance per zoom step: one notch of a typical mouse wheel, in points.
pub const WHEEL_STEP: f32 = 100.0;

/// A stored zoom level: whole numbers within the limits are kept, out-of-range ones clamped, others the default.
pub fn clamp_columns(stored: Option<f64>) -> usize {
    match stored {
        Some(value) if value.fract() == 0.0 => value.clamp(MIN_COLUMNS as f64, MAX_COLUMNS as f64) as usize,
        _ => DEFAULT_COLUMNS,
    }
}

/// Columns after `steps` zoom steps (positive zooms out, i.e. adds columns).
pub fn step_columns(columns: usize, steps: i32) -> usize {
    (columns as i64 + i64::from(steps)).clamp(MIN_COLUMNS as i64, MAX_COLUMNS as i64) as usize
}

/// Adds a wheel delta (positive = scrolled down) to the accumulated distance and splits it into whole zoom steps and
/// the remainder, so touchpads with small deltas zoom as smoothly as a mouse wheel. A change of direction starts over,
/// so a reversal reacts right away.
pub fn accumulate_wheel(accumulated: f32, delta: f32) -> (i32, f32) {
    let start = if accumulated.signum() == -delta.signum() && accumulated != 0.0 && delta != 0.0 {
        0.0
    } else {
        accumulated
    };
    let total = start + delta;
    let steps = (total / WHEEL_STEP).trunc();
    (steps as i32, total - steps * WHEEL_STEP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_006_06_turns_a_wheel_notch_into_one_step() {
        assert_eq!(accumulate_wheel(0.0, WHEEL_STEP), (1, 0.0));
        assert_eq!(accumulate_wheel(0.0, -WHEEL_STEP), (-1, 0.0));
    }

    #[test]
    fn req_006_06_collects_small_touchpad_deltas_until_they_add_up_to_a_step() {
        let mut accumulated = 0.0;
        let mut steps = 0;
        for _ in 0..10 {
            let (s, rest) = accumulate_wheel(accumulated, 30.0);
            accumulated = rest;
            steps += s;
        }
        assert_eq!(steps, 3);
        assert!(accumulated.abs() < 1e-3);
    }

    #[test]
    fn req_006_06_splits_large_deltas_and_drops_the_remainder_when_the_direction_changes() {
        assert_eq!(accumulate_wheel(0.0, 250.0), (2, 50.0));
        assert_eq!(accumulate_wheel(90.0, -20.0), (0, -20.0));
    }

    #[test]
    fn req_006_06_zooms_between_2_and_12_columns_default_5() {
        assert_eq!(clamp_columns(None), 5);
        assert_eq!(clamp_columns(Some(7.0)), 7);
        assert_eq!(clamp_columns(Some(30.0)), 12);
        assert_eq!(clamp_columns(Some(1.0)), 2);
        assert_eq!(clamp_columns(Some(4.5)), 5);
        assert_eq!(step_columns(12, 1), 12);
        assert_eq!(step_columns(5, -2), 3);
        assert_eq!(step_columns(3, -4), 2);
    }
}
