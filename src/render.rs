//! Software rendering, for the platforms without Core Animation (Windows and
//! Linux). Sprites are drawn into small premultiplied BGRA images: scaled
//! with nearest-neighbour sampling (crisp pixels) and, for the possum,
//! rotated around its paws. The platform shows them in transparent,
//! click-through windows that it moves along the screen edge.
//!
//! Images are y-down (row 0 at the top), like the screens they go on.

use crate::brain;
use crate::sprite::{self, Canvas};

/// A premultiplied BGRA image (alpha is only ever 0 or 255).
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub bgra: Vec<u8>,
}

impl Image {
    fn sample(w: usize, h: usize, mut source: impl FnMut(f64, f64) -> [u8; 4]) -> Self {
        let mut bgra = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let [r, g, b, a] = source(x as f64 + 0.5, y as f64 + 0.5);
                if a != 0 {
                    let i = (y * w + x) * 4;
                    bgra[i..i + 4].copy_from_slice(&[b, g, r, a]);
                }
            }
        }
        Self { w, h, bgra }
    }

    /// Same image as ARGB32 in network byte order (for Linux tray icons).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn argb_be(&self) -> Vec<u8> {
        self.bgra.chunks(4).flat_map(|p| [p[3], p[2], p[1], p[0]]).collect()
    }

    #[cfg(test)]
    pub fn opaque_pixels(&self) -> usize {
        self.bgra.chunks(4).filter(|p| p[3] != 0).count()
    }
}

/// All the sprite canvases the software renderer draws.
pub struct Art {
    /// Run-cycle frames.
    pub run: Vec<Canvas>,
    /// Scream frames, each with its two yell-line frames drawn in.
    pub scream: Vec<[Canvas; 2]>,
    /// Per kind of scream, per shout, per pointer corner.
    pub bubbles: [Vec<[Canvas; 4]>; 2],
}

impl Art {
    pub fn new() -> Self {
        let art = sprite::possum();
        let yells = [sprite::yell(0), sprite::yell(1)];
        let (mx, my) = (sprite::MOUTH_ANCHOR.0 as i32, sprite::MOUTH_ANCHOR.1 as i32);
        // The yell lines stick out in front of the mouth, so these canvases
        // are wider than the possum itself.
        let with_yell = |frame: &Canvas, yell: &Canvas| {
            let mut c = Canvas::new(mx + yell.w, sprite::POSSUM_H);
            c.blit(frame, 0, 0);
            c.blit(yell, mx, my - yell.h / 2);
            c
        };
        Self {
            scream: art.scream.iter().map(|f| [with_yell(f, &yells[0]), with_yell(f, &yells[1])]).collect(),
            run: art.run,
            bubbles: brain::shouts().map(|texts| {
                texts.iter().map(|t| sprite::Pointer::ALL.map(|p| sprite::bubble(t, p))).collect()
            }),
        }
    }

    /// The canvas for a scene's possum.
    pub fn possum(&self, scene: &brain::Scene) -> &Canvas {
        match scene.yell {
            Some(yell) if scene.screaming => &self.scream[scene.frame][yell],
            _ => &self.run[scene.frame],
        }
    }

    pub fn bubble(&self, bubble: &brain::Bubble) -> &Canvas {
        &self.bubbles[brain::kind_index(bubble.kind)][bubble.shout][bubble.quadrant]
    }
}

impl Default for Art {
    fn default() -> Self {
        Self::new()
    }
}

/// How far (in screen pixels) any drawn possum pixel can be from its paws,
/// in any rotation: the size of the square the rotated possum is drawn in
/// is twice this.
pub fn reach(art: &Art, scale: f64) -> usize {
    let (fx, fy) = (sprite::FEET_X as f64, sprite::FEET_Y as f64);
    let widest = art.scream[0][0].w.max(sprite::POSSUM_W) as f64;
    let corners = [(0.0, 0.0), (widest, 0.0), (0.0, sprite::POSSUM_H as f64), (widest, sprite::POSSUM_H as f64)];
    let far = corners.iter().map(|&(x, y)| ((x - fx).powi(2) + (y - fy).powi(2)).sqrt()).fold(0.0, f64::max);
    (far * scale).ceil() as usize + 1
}

/// The possum rotated by `heading` (counterclockwise on screen) around its
/// paws and scaled up: a square image `2 * reach` pixels wide whose centre
/// is exactly where the paws touch the screen edge.
pub fn rotated(canvas: &Canvas, scale: f64, heading: f64, reach: usize) -> Image {
    let side = reach * 2;
    let (sin, cos) = heading.sin_cos();
    // Snap tiny values so the straight edges get exact 90° rotations.
    let snap = |v: f64| if v.abs() < 1e-9 { 0.0 } else { v };
    let (sin, cos) = (snap(sin), snap(cos));
    let c = reach as f64;
    Image::sample(side, side, |px, py| {
        // Screen offset from the paws, y-up.
        let (dx, dy) = (px - c, c - py);
        // Rotate back into the sprite's own frame, and unscale.
        let lx = (dx * cos + dy * sin) / scale;
        let ly = (-dx * sin + dy * cos) / scale;
        let sx = sprite::FEET_X as f64 + lx;
        let sy = sprite::FEET_Y as f64 - ly;
        canvas.rgba_at(sx.floor() as i32, sy.floor() as i32)
    })
}

/// A canvas scaled up without rotation (speech bubbles, icons), to exactly
/// `w` by `h` pixels.
pub fn scaled(canvas: &Canvas, w: usize, h: usize) -> Image {
    let (sx, sy) = (canvas.w as f64 / w as f64, canvas.h as f64 / h as f64);
    Image::sample(w, h, |px, py| canvas.rgba_at((px * sx).floor() as i32, (py * sy).floor() as i32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI};

    fn opaque_bbox(img: &Image) -> (usize, usize, usize, usize) {
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for y in 0..img.h {
            for x in 0..img.w {
                if img.bgra[(y * img.w + x) * 4 + 3] != 0 {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                }
            }
        }
        (x0, y0, x1, y1)
    }

    #[test]
    fn unrotated_possum_stands_on_its_paws_and_keeps_every_pixel() {
        let art = Art::new();
        let scale = 3.0;
        let r = reach(&art, scale);
        let canvas = &art.run[0];
        let img = rotated(canvas, scale, 0.0, r);
        let opaque_sprite = (0..canvas.h)
            .flat_map(|y| (0..canvas.w).map(move |x| (x, y)))
            .filter(|&(x, y)| canvas.rgba_at(x, y)[3] != 0)
            .count();
        assert_eq!(img.opaque_pixels(), opaque_sprite * 9, "each pixel becomes a 3x3 block");
        // Everything is above the paws (the centre row), facing right.
        let (_, _, _, bottom) = opaque_bbox(&img);
        assert!(bottom < r, "drawn below the screen edge: {bottom} >= {r}");
    }

    #[test]
    fn rotations_keep_the_possum_on_the_right_side_of_the_edge() {
        let art = Art::new();
        let r = reach(&art, 3.0);
        let canvas = &art.run[0];
        // Right edge (heading up): the body is to the left of the paws.
        let (_, _, right, _) = opaque_bbox(&rotated(canvas, 3.0, FRAC_PI_2, r));
        assert!(right < r, "{right} >= {r}");
        // The paws sit on the line between pixel rows/columns r-1 and r.
        // Top edge (heading left): the body hangs below the paws.
        let (_, top, _, _) = opaque_bbox(&rotated(canvas, 3.0, PI, r));
        assert!(top >= r, "{top} < {r}");
        // Left edge (heading down): the body is to the right of the paws.
        let (left, _, _, _) = opaque_bbox(&rotated(canvas, 3.0, 1.5 * PI, r));
        assert!(left >= r, "{left} < {r}");
    }

    #[test]
    fn a_half_turn_is_an_exact_flip() {
        let art = Art::new();
        let r = reach(&art, 2.0);
        let up = rotated(&art.run[3], 2.0, 0.0, r);
        let down = rotated(&art.run[3], 2.0, PI, r);
        let side = up.w;
        for y in 0..side {
            for x in 0..side {
                let a = &up.bgra[(y * side + x) * 4..][..4];
                let b = &down.bgra[((side - 1 - y) * side + (side - 1 - x)) * 4..][..4];
                assert_eq!(a, b, "at {x},{y}");
            }
        }
    }

    #[test]
    fn nothing_is_cut_off_at_any_angle() {
        let art = Art::new();
        let r = reach(&art, 3.0);
        let canvas = &art.scream[0][1]; // the widest: with yell lines
        let full = rotated(canvas, 3.0, 0.0, r).opaque_pixels() as f64;
        for i in 0..32 {
            let heading = i as f64 / 32.0 * std::f64::consts::TAU;
            let img = rotated(canvas, 3.0, heading, r);
            let (x0, y0, x1, y1) = opaque_bbox(&img);
            assert!(x0 > 0 && y0 > 0 && x1 < img.w - 1 && y1 < img.h - 1, "clipped at {heading}");
            // Nearest-neighbour rotation keeps the area roughly the same.
            let ratio = img.opaque_pixels() as f64 / full;
            assert!((0.9..1.1).contains(&ratio), "area off by {ratio} at {heading}");
        }
    }

    #[test]
    fn scaled_bubbles_are_crisp_blocks() {
        let art = Art::new();
        let canvas = &art.bubbles[0][0][0];
        let img = scaled(canvas, canvas.w as usize * 3, canvas.h as usize * 3);
        for y in 0..img.h {
            for x in 0..img.w {
                let expect = canvas.rgba_at((x / 3) as i32, (y / 3) as i32);
                let px = &img.bgra[(y * img.w + x) * 4..][..4];
                assert_eq!(px, [expect[2], expect[1], expect[0], expect[3]]);
            }
        }
    }

    #[test]
    fn tray_icons_convert_to_network_order_argb() {
        let img = Image { w: 1, h: 1, bgra: vec![1, 2, 3, 255] };
        assert_eq!(img.argb_be(), vec![255, 3, 2, 1]);
    }
}
