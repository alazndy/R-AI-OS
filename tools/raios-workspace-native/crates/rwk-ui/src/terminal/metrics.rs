//! Cell geometry: how many whole cells fit in a pixel area, and when a resize is a real change.

use rwk_core::sessions::TerminalSize;

/// Smallest column count the engine accepts.
pub const MIN_COLS: u16 = 2;
/// Largest column count the engine accepts.
pub const MAX_COLS: u16 = 500;
/// Smallest row count the engine accepts.
pub const MIN_ROWS: u16 = 2;
/// Largest row count the engine accepts.
pub const MAX_ROWS: u16 = 300;

/// The whole-cell grid that fits in `width` x `height` pixels, clamped to the engine's limits.
pub fn grid_for(width: f32, height: f32, cell_width: f32, cell_height: f32) -> TerminalSize {
    let fit = |extent: f32, cell: f32, min: u16, max: u16| -> u16 {
        if cell <= 0.0 || extent <= 0.0 {
            return min;
        }
        ((extent / cell).floor() as u32).clamp(u32::from(min), u32::from(max)) as u16
    };
    TerminalSize {
        cols: fit(width, cell_width, MIN_COLS, MAX_COLS),
        rows: fit(height, cell_height, MIN_ROWS, MAX_ROWS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_cells_only() {
        let size = grid_for(805.0, 400.0, 8.0, 16.0);
        assert_eq!((size.cols, size.rows), (100, 25));
    }

    #[test]
    fn a_sub_cell_resize_changes_nothing() {
        assert_eq!(
            grid_for(805.0, 400.0, 8.0, 16.0),
            grid_for(807.9, 401.0, 8.0, 16.0)
        );
    }

    #[test]
    fn the_grid_is_clamped_to_the_engine_limits() {
        let tiny = grid_for(1.0, 1.0, 8.0, 16.0);
        assert_eq!((tiny.cols, tiny.rows), (MIN_COLS, MIN_ROWS));
        let huge = grid_for(100_000.0, 100_000.0, 8.0, 16.0);
        assert_eq!((huge.cols, huge.rows), (MAX_COLS, MAX_ROWS));
    }
}
