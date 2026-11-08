//! The racetrack: the screen border as a rounded rectangle, traversed
//! counterclockwise.
//!
//! Coordinates are y-up (AppKit / Core Animation convention), with the origin
//! at the bottom-left of the screen. Counterclockwise therefore means:
//! bottom edge going right → right edge going up → top edge going left →
//! left edge going down. The heading angle simply keeps increasing
//! (0 → π/2 → π → 3π/2), which is exactly the rotation to apply to a sprite
//! whose feet are its anchor point.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    /// Direction of travel in radians, counterclockwise from +x.
    pub heading: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Track {
    w: f64,
    h: f64,
    r: f64,
}

impl Track {
    /// `corner_radius` is how tightly the possum turns at the corners.
    pub fn new(w: f64, h: f64, corner_radius: f64) -> Self {
        let r = corner_radius.min(w / 2.0).min(h / 2.0).max(0.0);
        Self { w, h, r }
    }

    pub fn perimeter(&self) -> f64 {
        2.0 * (self.w - 2.0 * self.r) + 2.0 * (self.h - 2.0 * self.r) + TAU * self.r
    }

    /// Pose after travelling `distance` from the bottom-left end of the
    /// bottom edge.
    pub fn pose(&self, distance: f64) -> Pose {
        let (w, h, r) = (self.w, self.h, self.r);
        let horizontal = w - 2.0 * r;
        let vertical = h - 2.0 * r;
        let arc = FRAC_PI_2 * r;

        let mut d = distance.rem_euclid(self.perimeter());
        // (length, kind): straight segments carry start point + heading,
        // arcs carry centre + start angle.
        enum Seg {
            Line { x: f64, y: f64, heading: f64 },
            Arc { cx: f64, cy: f64, start: f64 },
        }
        let segments = [
            (horizontal, Seg::Line { x: r, y: 0.0, heading: 0.0 }),
            (arc, Seg::Arc { cx: w - r, cy: r, start: -FRAC_PI_2 }),
            (vertical, Seg::Line { x: w, y: r, heading: FRAC_PI_2 }),
            (arc, Seg::Arc { cx: w - r, cy: h - r, start: 0.0 }),
            (horizontal, Seg::Line { x: w - r, y: h, heading: PI }),
            (arc, Seg::Arc { cx: r, cy: h - r, start: FRAC_PI_2 }),
            (vertical, Seg::Line { x: 0.0, y: h - r, heading: 3.0 * FRAC_PI_2 }),
            (arc, Seg::Arc { cx: r, cy: r, start: PI }),
        ];
        let last = segments.len() - 1;
        for (i, (len, seg)) in segments.iter().enumerate() {
            // The last segment also absorbs any floating-point leftovers.
            if d <= *len || i == last {
                return match *seg {
                    Seg::Line { x, y, heading } => Pose {
                        x: x + d * heading.cos(),
                        y: y + d * heading.sin(),
                        heading,
                    },
                    Seg::Arc { cx, cy, start } => {
                        let a = if r > 0.0 { start + d / r } else { start };
                        Pose { x: cx + r * a.cos(), y: cy + r * a.sin(), heading: a + FRAC_PI_2 }
                    }
                };
            }
            d -= len;
        }
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn goes_counterclockwise() {
        let t = Track::new(1000.0, 600.0, 50.0);
        let p = t.perimeter();
        // Bottom edge: moving right along y = 0.
        let a = t.pose(100.0);
        assert!(close(a.y, 0.0) && close(a.heading, 0.0) && close(a.x, 150.0));
        // A quarter of the way in we're on the right edge, moving up.
        let b = t.pose(900.0 + 25.0 * PI + 100.0);
        assert!(close(b.x, 1000.0) && close(b.heading, FRAC_PI_2) && close(b.y, 150.0));
        // Top edge, moving left.
        let c = t.pose(900.0 + 500.0 + 50.0 * PI + 10.0);
        assert!(close(c.y, 600.0) && close(c.heading, PI) && close(c.x, 940.0));
        // Left edge, moving down.
        let d = t.pose(2.0 * 900.0 + 500.0 + 75.0 * PI + 10.0);
        assert!(close(d.x, 0.0) && close(d.heading, 1.5 * PI) && close(d.y, 540.0));
        // Wraps around.
        let e = t.pose(p + 100.0);
        assert!(close(e.x, a.x) && close(e.y, a.y));
    }

    #[test]
    fn path_is_continuous() {
        let t = Track::new(1512.0, 982.0, 72.0);
        let mut prev = t.pose(0.0);
        let step = 0.5;
        let mut d = step;
        while d < t.perimeter() * 1.01 {
            let p = t.pose(d);
            let jump = ((p.x - prev.x).powi(2) + (p.y - prev.y).powi(2)).sqrt();
            assert!(jump <= step + 1e-6, "jump of {jump} at {d}");
            prev = p;
            d += step;
        }
    }

    #[test]
    fn heading_turns_smoothly_at_corners() {
        let t = Track::new(800.0, 600.0, 40.0);
        let mid_corner = t.pose(720.0 + 10.0 * PI); // halfway round the first arc
        assert!(close(mid_corner.heading, PI / 4.0));
    }
}
