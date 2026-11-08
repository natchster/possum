//! Which display the possum runs on, and when to move it.
//!
//! The possum runs around the *primary* display: the one with the menu bar,
//! which macOS always lists first and places at the origin of the global
//! coordinate space. It doesn't care about the others.
//!
//! When displays change (a resolution change, a monitor plugged in or
//! unplugged, or the menu bar dragged to another monitor in System
//! Settings → Displays → Arrange), the overlay window has to follow the
//! primary display. `main.rs` asks `follow` both when macOS posts a
//! "screen parameters changed" notification and once a second as a backup.

use objc2_core_foundation::CGRect;

/// Where the overlay window should go, or `None` to leave it alone.
///
/// * `primary`: the primary display's frame now (`None` while macOS
///   reports no display, e.g. mid-reconfiguration: then wait).
/// * `current`: the frame the possum's track was built for.
/// * `window`: where the overlay window actually is.
///
/// Comparing `window` too matters: when the menu bar moves to another
/// monitor of the same size, the new primary frame is identical to the old
/// one (both start at the origin), but the window is still on the old
/// monitor, whose global coordinates have changed.
pub fn follow(primary: Option<CGRect>, current: CGRect, window: CGRect) -> Option<CGRect> {
    let primary = primary?;
    (primary != current || window != primary).then_some(primary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_foundation::{CGPoint, CGSize};

    fn frame(x: f64, y: f64, w: f64, h: f64) -> CGRect {
        CGRect::new(CGPoint::new(x, y), CGSize::new(w, h))
    }

    const MACBOOK: (f64, f64) = (1512.0, 982.0);

    #[test]
    fn nothing_changed_stay_put() {
        let main = frame(0.0, 0.0, MACBOOK.0, MACBOOK.1);
        assert_eq!(follow(Some(main), main, main), None);
    }

    #[test]
    fn resolution_change_moves_to_the_new_frame() {
        let before = frame(0.0, 0.0, 1512.0, 982.0);
        let after = frame(0.0, 0.0, 1800.0, 1169.0); // "More Space"
        assert_eq!(follow(Some(after), before, before), Some(after));
    }

    #[test]
    fn menu_bar_moved_to_a_different_size_monitor() {
        // Laptop was primary; now the 4K monitor to its right is.
        let laptop = frame(0.0, 0.0, 1512.0, 982.0);
        let monitor = frame(0.0, 0.0, 2560.0, 1440.0);
        // The window is still on the laptop, now at negative x.
        let window = frame(-1512.0, 0.0, 1512.0, 982.0);
        assert_eq!(follow(Some(monitor), laptop, window), Some(monitor));
    }

    #[test]
    fn menu_bar_moved_to_an_identical_monitor() {
        // Two identical monitors: the primary frame looks the same before
        // and after, only the window's position gives the change away.
        let main = frame(0.0, 0.0, 2560.0, 1440.0);
        let window_on_old_monitor = frame(2560.0, 0.0, 2560.0, 1440.0);
        assert_eq!(follow(Some(main), main, window_on_old_monitor), Some(main));
    }

    #[test]
    fn plugging_in_a_second_monitor_does_not_move_the_possum() {
        // A new secondary display doesn't change the primary one, and the
        // window stays where it is.
        let main = frame(0.0, 0.0, MACBOOK.0, MACBOOK.1);
        assert_eq!(follow(Some(main), main, main), None);
    }

    #[test]
    fn unplugging_the_primary_monitor_moves_to_the_laptop() {
        let monitor = frame(0.0, 0.0, 3440.0, 1440.0);
        let laptop = frame(0.0, 0.0, 1512.0, 982.0);
        assert_eq!(follow(Some(laptop), monitor, monitor), Some(laptop));
    }

    #[test]
    fn no_display_for_a_moment_waits() {
        let main = frame(0.0, 0.0, MACBOOK.0, MACBOOK.1);
        assert_eq!(follow(None, main, main), None);
    }
}
