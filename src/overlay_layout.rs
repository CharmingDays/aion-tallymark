//! Logical sizes for the compact overlay window.
//!
//! Kept here (not only in the Tauri shell) so tests can check that the minimum
//! height still fits five player rows after a density change.
//!
//! Heights were chosen from headless DOM measurements: header + totals + five
//! rows + the idle line must fit with a few pixels of bottom padding (no clip).

/// Design width used by the overlay's rem scaling (1rem = 10px at this width).
pub const DESIGN_WIDTH: f64 = 280.0;
/// Default window size when no saved rect exists.
pub const DEFAULT_SIZE: (f64, f64) = (320.0, 180.0);
/// Smallest the user can shrink the overlay (logical CSS pixels).
pub const MIN_SIZE: (f64, f64) = (200.0, 172.0);
/// Row pitch in CSS px when the root font-size is 10px (1.8rem row + 0.1rem gap).
pub const ROW_PITCH_PX: f64 = 19.0;
/// Header + slim summary + status + row list paddings at 10px rem.
pub const CHROME_PX: f64 = 78.0;
/// Spare space under the idle line at design rem (keeps the last row unclipped).
pub const BOTTOM_MARGIN_PX: f64 = 5.0;
/// Root font-size floor used by the overlay scale script.
pub const REM_FLOOR_PX: f64 = 9.5;

/// How many player rows fit in an overlay of `inner_height` with root `rem_px`.
pub fn row_capacity(inner_height: f64, rem_px: f64) -> u32 {
    let scale = if rem_px > 0.0 { rem_px / 10.0 } else { 1.0 };
    let pitch = ROW_PITCH_PX * scale;
    let chrome = CHROME_PX * scale;
    if pitch <= 0.0 {
        return 1;
    }
    let room = (inner_height - chrome).max(0.0);
    ((room / pitch).floor() as u32).clamp(1, 12)
}

/// Minimum inner height that fits `rows` at `rem_px`, including bottom margin.
pub fn height_for_rows(rows: u32, rem_px: f64) -> f64 {
    let scale = if rem_px > 0.0 { rem_px / 10.0 } else { 1.0 };
    CHROME_PX * scale + f64::from(rows) * ROW_PITCH_PX * scale + BOTTOM_MARGIN_PX * scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_rows_fit_at_the_minimum_window() {
        assert!(
            row_capacity(MIN_SIZE.1, REM_FLOOR_PX) >= 5,
            "min height {} with rem {} only fits {}",
            MIN_SIZE.1,
            REM_FLOOR_PX,
            row_capacity(MIN_SIZE.1, REM_FLOOR_PX)
        );
        assert!(
            MIN_SIZE.1 + 0.5 >= height_for_rows(5, REM_FLOOR_PX),
            "min {} is below measured need {}",
            MIN_SIZE.1,
            height_for_rows(5, REM_FLOOR_PX)
        );
    }

    #[test]
    fn default_size_fits_five_rows_snugly_at_design_rem() {
        assert_eq!(row_capacity(DEFAULT_SIZE.1, 10.0), 5);
        assert!(DEFAULT_SIZE.1 + 0.5 >= height_for_rows(5, 10.0));
        // One row of slack at most (no big empty band under the list).
        assert!(row_capacity(DEFAULT_SIZE.1 + ROW_PITCH_PX, 10.0) >= 6);
        assert!(DEFAULT_SIZE.1 <= height_for_rows(5, 10.0) + ROW_PITCH_PX);
    }

    #[test]
    fn default_size_is_above_the_minimum() {
        assert!(DEFAULT_SIZE.0 >= MIN_SIZE.0 && DEFAULT_SIZE.1 >= MIN_SIZE.1);
    }

    #[test]
    fn a_taller_window_shows_more_rows() {
        assert!(row_capacity(400.0, 10.0) > row_capacity(MIN_SIZE.1, REM_FLOOR_PX));
    }
}
