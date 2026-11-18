//! How big the possum is: about 2 cm tall on every display.
//!
//! Its sprite is 32 pixels tall, so a sprite pixel should measure about
//! 0.625 mm: 3 points on a MacBook Pro at its default resolution (6 of the
//! Retina display's pixels). The OS knows how big each display is, from the
//! display's own description of itself (EDID): macOS through
//! `CGDisplayScreenSize`, Windows as the monitor's raw DPI, X11 through
//! RandR. That says how many pixels make a millimetre, and so how many make
//! a sprite pixel.
//!
//! Pixel art looks best when each sprite pixel is a whole number of the
//! display's pixels, so that's what it gets, as long as the possum stays
//! about 2 cm tall that way (18 to 23.7 mm); otherwise half pixels. A
//! display that doesn't say how big it is (projectors and virtual machines
//! often don't), or says something unbelievable, gets the size the possum
//! always had: 3 points on macOS, 3 pixels at 100% display scaling
//! elsewhere.
//!
//! Its speed goes with its size, so its legs keep the same rhythm and it
//! crosses the display in about the same time at any size: 260 units a
//! second at 3 units a sprite pixel.

use std::ops::RangeInclusive;

use crate::sprite;

/// What a sprite pixel aims to measure: 32 of them make 2 cm.
const PIXEL_MM: f64 = 20.0 / sprite::POSSUM_H as f64;
/// What still counts as about 2 cm: a whole number of the display's pixels
/// in this range wins over the exact size. It reaches further up than down
/// so that this Mac mini's 34-inch display (3 pixels of 0.236 mm, 0.71 mm a
/// sprite pixel) keeps the size it had.
const ABOUT_MM: RangeInclusive<f64> = 0.56..=0.74;

/// Screen units a sprite pixel when the display doesn't say how big it is,
/// at 100% display scaling.
pub const FALLBACK_SCALE: f64 = 3.0;
/// Screen units a second at `FALLBACK_SCALE`.
const SPEED_AT_FALLBACK: f64 = 260.0;

/// A display's pixel density, as far as its size is known.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    /// Device pixels per millimetre.
    density: f64,
    /// The shorter side, in device pixels.
    short: f64,
    /// Device pixels per screen unit: 2 for a Retina Mac's points, else 1.
    per_unit: f64,
}

impl Display {
    /// A display `px` device pixels wide and tall that measures `mm`
    /// (either way round: rotated displays sometimes report their size
    /// unrotated). `None` unless those add up to a believable display.
    #[cfg_attr(all(windows, not(test)), expect(dead_code))]
    pub fn new(px: (f64, f64), mm: (f64, f64), per_unit: f64) -> Option<Self> {
        let (pw, ph) = (px.0.max(px.1), px.0.min(px.1));
        let (mw, mh) = (mm.0.max(mm.1), mm.0.min(mm.1));
        // Projectors and virtual displays often say 0.
        if !(mh >= 40.0 && ph >= 1.0 && mw.is_finite() && pw.is_finite()) {
            return None;
        }
        // The same shape in pixels as in millimetres, give or take a
        // letterboxed resolution.
        if !(0.8..=1.25).contains(&((pw / ph) / (mw / mh))) {
            return None;
        }
        Self::with_density(pw.hypot(ph) / mw.hypot(mh), ph, per_unit)
    }

    /// A display with `dpi` device pixels per inch and `px` device pixels
    /// wide and tall (Windows says how big a monitor is in dots per inch).
    #[cfg_attr(all(not(windows), not(test)), expect(dead_code))]
    pub fn from_dpi(dpi: f64, px: (f64, f64)) -> Option<Self> {
        Self::with_density(dpi / 25.4, px.0.min(px.1), 1.0)
    }

    fn with_density(density: f64, short: f64, per_unit: f64) -> Option<Self> {
        // From 25 dpi (a TV across a room) to 635 (beyond any phone's).
        (1.0..=25.0).contains(&density).then_some(Self { density, short, per_unit })
    }

    /// Device pixels per sprite pixel to make the possum about 2 cm tall:
    /// the nearest whole number that does, else the nearest half.
    fn sprite_pixel(&self) -> f64 {
        let exact = PIXEL_MM * self.density;
        let about = |n: &f64| *n >= 1.0 && ABOUT_MM.contains(&(n / self.density));
        let nearest = |step: f64| {
            let below = (exact / step).floor() * step;
            [below, below + step]
                .into_iter()
                .filter(about)
                .min_by(|a, b| (a - exact).abs().total_cmp(&(b - exact).abs()))
        };
        let half = |n: f64| ((n * 2.0).round() / 2.0).max(1.0);
        let n = nearest(1.0).or_else(|| nearest(0.5)).unwrap_or_else(|| half(exact));
        // Whatever the display says, the possum never takes up more than a
        // sixth of its height.
        n.min(half(self.short / 6.0 / sprite::POSSUM_H as f64 - 0.25))
    }
}

/// How big the possum is and how fast it runs, in screen units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    /// Screen units per sprite pixel.
    pub scale: f64,
    /// Screen units per second.
    pub speed: f64,
}

impl Size {
    /// `scale` and `speed` from the command line, if given, are in units at
    /// 100% display scaling; `system` is screen units per such unit (the
    /// display scaling on Windows, else 1).
    pub fn pick(scale: Option<f64>, speed: Option<f64>, display: Option<Display>, system: f64) -> Self {
        let scale = match scale {
            Some(scale) => scale * system,
            None => display.map_or(FALLBACK_SCALE * system, |d| d.sprite_pixel() / d.per_unit),
        };
        let speed = speed.map_or(SPEED_AT_FALLBACK / FALLBACK_SCALE * scale, |speed| speed * system);
        Self { scale, speed }
    }

    /// For the log: how big the possum is, and why.
    pub fn describe(&self, display: Option<Display>, from_options: bool) -> String {
        let size = format!("a sprite pixel is {} units", self.scale);
        match display {
            _ if from_options => format!("{size}, as --scale says"),
            None => format!("the display doesn't say how big it is: {size}"),
            Some(d) => {
                let pixels = self.scale * d.per_unit;
                format!(
                    "the display has {:.2} pixels a millimetre ({:.0} dpi): {size} ({pixels} pixels), \
                     so the possum is {:.1} mm tall",
                    d.density,
                    d.density * 25.4,
                    sprite::POSSUM_H as f64 * pixels / d.density,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(display: Option<Display>) -> f64 {
        Size::pick(None, None, display, 1.0).scale
    }

    fn height_mm(display: Display) -> f64 {
        sprite::POSSUM_H as f64 * auto(Some(display)) * display.per_unit / display.density
    }

    #[test]
    fn the_sizes_that_were_just_right_stay_the_same() {
        // This Mac mini's Samsung Odyssey G85SB (34", 3440x1440 at 1x), as
        // CGDisplayScreenSize reports it.
        let odyssey = Display::new((3440.0, 1440.0), (809.037, 348.343), 1.0).unwrap();
        assert_eq!(auto(Some(odyssey)), 3.0);
        // MacBook Pros at their default resolution: 14" and 16", 254 dpi,
        // two pixels a point.
        let mbp14 = Display::new((3024.0, 1964.0), (302.4, 196.4), 2.0).unwrap();
        let mbp16 = Display::new((3456.0, 2234.0), (345.6, 223.4), 2.0).unwrap();
        assert_eq!(auto(Some(mbp14)), 3.0);
        assert_eq!(auto(Some(mbp16)), 3.0);
    }

    #[test]
    fn full_hd_laptops_get_a_bigger_possum() {
        // 1920x1080 panels: 13.3", 14" and 15.6".
        for (mm, pixels) in [((294.0, 165.0), 4.0), ((309.0, 174.0), 4.0), ((344.0, 194.0), 4.0)] {
            let d = Display::new((1920.0, 1080.0), mm, 1.0).unwrap();
            assert_eq!(auto(Some(d)), pixels, "{mm:?}");
        }
    }

    #[test]
    fn about_two_centimetres_on_any_display() {
        // From a 65" TV (1.3 pixels a millimetre) through full HD monitors
        // (3 to 4) to a 4K 13" laptop (12.5) and beyond.
        let mut density = 1.0;
        while density <= 25.0 {
            let d = Display::with_density(density, 4000.0, 1.0).unwrap();
            let pixels = auto(Some(d));
            assert_eq!((pixels * 2.0).fract(), 0.0, "{density} px/mm: {pixels} isn't a whole or half pixel");
            assert!(pixels >= 1.0);
            // Below 2.7 pixels a millimetre (68 dpi: TVs, not monitors, even
            // a 32" full HD one has 2.75) half a pixel is too coarse a step
            // to always land within that.
            let mm = height_mm(d);
            let about = ABOUT_MM.contains(&(mm / sprite::POSSUM_H as f64));
            assert!(density < 2.7 || about, "{density} px/mm: {mm} mm");
            density += 0.005;
        }
    }

    #[test]
    fn a_rotated_display_gets_the_same_size() {
        let landscape = Display::new((2560.0, 1440.0), (597.0, 336.0), 1.0);
        assert_eq!(Display::new((1440.0, 2560.0), (597.0, 336.0), 1.0), landscape);
        assert_eq!(Display::new((1440.0, 2560.0), (336.0, 597.0), 1.0), landscape);
    }

    #[test]
    fn displays_that_dont_say_or_make_no_sense_keep_the_old_size() {
        assert_eq!(Display::new((1920.0, 1080.0), (0.0, 0.0), 1.0), None); // a projector
        assert_eq!(Display::new((1920.0, 1080.0), (16.0, 9.0), 1.0), None); // just its shape
        assert_eq!(Display::new((1920.0, 1080.0), (300.0, 300.0), 1.0), None); // the wrong shape
        assert_eq!(Display::from_dpi(0.0, (1920.0, 1080.0)), None);
        assert_eq!(auto(None), 3.0);
        // At 150% display scaling on Windows.
        assert_eq!(Size::pick(None, None, None, 1.5).scale, 4.5);
    }

    #[test]
    fn a_display_that_claims_to_be_tiny_cant_make_the_possum_huge() {
        let d = Display::new((1920.0, 1080.0), (80.0, 45.0), 1.0).unwrap();
        assert!(auto(Some(d)) * sprite::POSSUM_H as f64 <= 1080.0 / 6.0);
    }

    #[test]
    fn windows_raw_dpi_works_the_same() {
        // 96 dpi (a 23" full HD monitor), 141 dpi (15.6" full HD) and 163
        // dpi (27" 4K).
        assert_eq!(auto(Display::from_dpi(96.0, (1920.0, 1080.0))), 2.5);
        assert_eq!(auto(Display::from_dpi(141.0, (1920.0, 1080.0))), 4.0);
        assert_eq!(auto(Display::from_dpi(163.0, (3840.0, 2160.0))), 4.0);
    }

    #[test]
    fn the_command_line_wins_and_speed_goes_with_size() {
        let odyssey = Display::new((3440.0, 1440.0), (809.037, 348.343), 1.0);
        let fhd14 = Display::new((1920.0, 1080.0), (309.0, 174.0), 1.0);
        assert_eq!(Size::pick(None, None, odyssey, 1.0), Size { scale: 3.0, speed: 260.0 });
        assert_eq!(Size::pick(None, None, fhd14, 1.0).speed, 260.0 / 3.0 * 4.0);
        assert_eq!(Size::pick(Some(5.0), None, fhd14, 1.0).scale, 5.0);
        assert_eq!(Size::pick(Some(3.0), Some(400.0), fhd14, 1.25), Size { scale: 3.75, speed: 500.0 });
    }
}
