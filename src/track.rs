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

    /// Lengths of the 8 segments, counterclockwise from the bottom edge:
    /// bottom edge, corner, right edge, corner, top edge, corner, left edge,
    /// corner.
    fn segment_lengths(&self) -> [f64; 8] {
        let horizontal = self.w - 2.0 * self.r;
        let vertical = self.h - 2.0 * self.r;
        let arc = FRAC_PI_2 * self.r;
        [horizontal, arc, vertical, arc, horizontal, arc, vertical, arc]
    }

    /// Where `distance` falls on the border: which segment (see
    /// `segment_lengths`) and how far into it, from 0.0 to 1.0.
    pub fn locate(&self, distance: f64) -> (usize, f64) {
        let mut d = distance.rem_euclid(self.perimeter());
        let lengths = self.segment_lengths();
        for (i, len) in lengths.iter().enumerate() {
            if d <= *len || i == lengths.len() - 1 {
                let fraction = if *len > 0.0 { (d / len).clamp(0.0, 1.0) } else { 0.0 };
                return (i, fraction);
            }
            d -= len;
        }
        unreachable!()
    }

    /// The distance of a spot given as segment and fraction: the inverse of
    /// `locate`.
    pub fn distance_at(&self, segment: usize, fraction: f64) -> f64 {
        let lengths = self.segment_lengths();
        lengths[..segment].iter().sum::<f64>() + lengths[segment] * fraction
    }

    /// The same spot on another track (after a screen change): on the same
    /// edge or corner, at the same relative position along it. Keeping just
    /// the fraction of the whole lap could land on a different edge when the
    /// screen's shape changes.
    pub fn carry_over(&self, distance: f64, to: &Track) -> f64 {
        let (segment, fraction) = self.locate(distance);
        to.distance_at(segment, fraction)
    }

    /// Pose after travelling `distance` from the bottom-left end of the
    /// bottom edge.
    pub fn pose(&self, distance: f64) -> Pose {
        let (w, h, r) = (self.w, self.h, self.r);
        let mut d = distance.rem_euclid(self.perimeter());
        // (length, kind): straight segments carry start point + heading,
        // arcs carry centre + start angle.
        enum Seg {
            Line { x: f64, y: f64, heading: f64 },
            Arc { cx: f64, cy: f64, start: f64 },
        }
        let lengths = self.segment_lengths();
        let segments = [
            (lengths[0], Seg::Line { x: r, y: 0.0, heading: 0.0 }),
            (lengths[1], Seg::Arc { cx: w - r, cy: r, start: -FRAC_PI_2 }),
            (lengths[2], Seg::Line { x: w, y: r, heading: FRAC_PI_2 }),
            (lengths[3], Seg::Arc { cx: w - r, cy: h - r, start: 0.0 }),
            (lengths[4], Seg::Line { x: w - r, y: h, heading: PI }),
            (lengths[5], Seg::Arc { cx: r, cy: h - r, start: FRAC_PI_2 }),
            (lengths[6], Seg::Line { x: 0.0, y: h - r, heading: 3.0 * FRAC_PI_2 }),
            (lengths[7], Seg::Arc { cx: r, cy: r, start: PI }),
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
    fn works_on_any_screen_shape() {
        // A tiny window, common MacBook and desktop sizes, an ultrawide, a 6K
        // display, and a portrait monitor; at the smallest, default and
        // largest --scale (which sets the corner radius).
        let screens = [
            (320.0, 240.0),
            (1280.0, 800.0),
            (1512.0, 982.0),
            (2560.0, 1440.0),
            (3440.0, 1440.0),
            (6016.0, 3384.0),
            (1080.0, 1920.0),
        ];
        for (w, h) in screens {
            for scale in [1.0, 3.0, 12.0] {
                let t = Track::new(w, h, 26.0 * scale);
                let n = 4000;
                for i in 0..=n {
                    let p = t.pose(t.perimeter() * i as f64 / n as f64);
                    let on_screen = (-1e-6..=w + 1e-6).contains(&p.x) && (-1e-6..=h + 1e-6).contains(&p.y);
                    assert!(on_screen, "{w}x{h} at scale {scale}: {p:?} is off screen");
                }
            }
        }
    }

    #[test]
    fn locate_and_distance_at_are_inverses() {
        let t = Track::new(1512.0, 982.0, 78.0);
        let n = 1000;
        for i in 0..n {
            let d = t.perimeter() * i as f64 / n as f64;
            let (segment, fraction) = t.locate(d);
            assert!(segment < 8 && (0.0..=1.0).contains(&fraction));
            assert!((t.distance_at(segment, fraction) - d).abs() < 1e-6, "at {d}");
        }
    }

    #[test]
    fn carry_over_keeps_the_same_edge_and_spot_on_any_new_screen() {
        // From a MacBook to an ultrawide, a portrait monitor and a tiny screen.
        let from = Track::new(1512.0, 982.0, 78.0);
        let targets = [
            Track::new(3440.0, 1440.0, 78.0),
            Track::new(1080.0, 1920.0, 78.0),
            Track::new(400.0, 300.0, 78.0),
            Track::new(1512.0, 982.0, 156.0), // same screen, bigger --scale
        ];
        let n = 400;
        for to in targets {
            for i in 0..n {
                let d = from.perimeter() * (i as f64 + 0.5) / n as f64;
                let carried = from.carry_over(d, &to);
                let (a, b) = (from.pose(d), to.pose(carried));
                assert!((a.heading - b.heading).abs() < 1e-6, "edge changed: {a:?} -> {b:?}");
                let (sa, fa) = from.locate(d);
                let (sb, fb) = to.locate(carried);
                assert_eq!(sa, sb);
                assert!((fa - fb).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn heading_turns_smoothly_at_corners() {
        let t = Track::new(800.0, 600.0, 40.0);
        let mid_corner = t.pose(720.0 + 10.0 * PI); // halfway round the first arc
        assert!(close(mid_corner.heading, PI / 4.0));
    }
}
