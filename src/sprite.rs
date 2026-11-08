//! The possum's pixel art.
//!
//! The head is hand-drawn as character grids (that is where the personality
//! lives: ear, eye, nose, teeth and that wide-open screaming mouth). The body,
//! tail and legs are drawn procedurally so the run cycle can be generated for
//! any number of frames. A final pass adds a crisp 1px dark outline around
//! the whole silhouette, which is what makes it read as "pixel art".

use std::f32::consts::{PI, TAU};

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

const CLEAR: u8 = 0;
const OUTLINE: u8 = 1;
const FUR_DARK: u8 = 2;
const FUR: u8 = 3;
const FUR_LIGHT: u8 = 4;
const WHITE: u8 = 5;
const CREAM: u8 = 6;
const BLACK: u8 = 7;
const PINK: u8 = 8;
const PINK_DARK: u8 = 9;
const NOSE: u8 = 10;
const MOUTH: u8 = 11;
const TONGUE: u8 = 12;
const TOOTH: u8 = 13;
const EYE: u8 = 14;
const SHINE: u8 = 15;
const LEG_FAR: u8 = 16;
const BUBBLE: u8 = 17;
const SHOUT: u8 = 18;
const FROST: u8 = 19;
const ICON_BG: u8 = 20;
const ICON_BG_DARK: u8 = 21;

const PALETTE: [[u8; 3]; 22] = [
    [0x00, 0x00, 0x00], // clear (unused)
    [0x22, 0x1a, 0x2a], // outline
    [0x5a, 0x55, 0x66], // fur dark
    [0x86, 0x82, 0x93], // fur
    [0xb3, 0xaf, 0xbd], // fur light
    [0xf7, 0xf3, 0xeb], // white face
    [0xd9, 0xcf, 0xc1], // cream (face shadow)
    [0x2c, 0x26, 0x34], // black (ears, near legs, eye patch)
    [0xf6, 0xaa, 0xb9], // pink (ears, paws, tail)
    [0xd1, 0x77, 0x8e], // dark pink (tail underside)
    [0xe9, 0x5a, 0x7c], // nose
    [0x5c, 0x0e, 0x22], // mouth interior
    [0xe2, 0x43, 0x66], // tongue
    [0xff, 0xff, 0xff], // teeth
    [0x0b, 0x08, 0x10], // eye
    [0xff, 0xff, 0xff], // eye shine
    [0x47, 0x41, 0x52], // far-side legs
    [0xff, 0xfd, 0xf5], // speech bubble
    [0xd9, 0x1c, 0x3c], // shout text
    [0xdf, 0xdb, 0xe6], // frosted guard hairs
    [0x3b, 0x2f, 0x6b], // icon background
    [0x26, 0x1e, 0x4a], // icon background shade
];

fn color_for(ch: u8) -> u8 {
    match ch {
        b'k' => OUTLINE,
        b'd' => FUR_DARK,
        b'g' => FUR,
        b'l' => FUR_LIGHT,
        b'w' => WHITE,
        b'c' => CREAM,
        b'b' => BLACK,
        b'p' => PINK,
        b'q' => PINK_DARK,
        b'n' => NOSE,
        b'm' => MOUTH,
        b'r' => TONGUE,
        b't' => TOOTH,
        b'e' => EYE,
        b'h' => SHINE,
        b'#' => SHOUT,
        _ => CLEAR,
    }
}

// ---------------------------------------------------------------------------
// Hand-drawn heads (facing right). 18 columns wide.
// ---------------------------------------------------------------------------

/// Running: mouth hanging open, panting.
const HEAD_OPEN: [&str; 15] = [
    "..bb..............",
    ".bppb.............",
    ".bppbb............",
    ".bpbgll...........",
    "..bgllwww.........",
    ".gglwwwwwww.......",
    "ggwwwbbwwwwwww....",
    "gwwwbehbwwwwwwwn..",
    "gwwwbeebwwwwwwwnn.",
    "gwwwwbbwwwtttttt..",
    "gwwwwwwmmmmrrmm...",
    "gcwwwwwmmrrrmt....",
    ".gcwwwwwttttcc....",
    ".gccwwwwwccc......",
    "..gccccccc........",
];

/// Screaming, after the famous "бежит орёт" ("runs, screams") opossum: head
/// thrown back, snout pointing at the sky, jaws agape with every fang showing.
/// 20 columns wide.
const HEAD_SCREAM: [&str; 15] = [
    "..bb............wnn.",
    ".bppb........wwwwnn.",
    ".bppbb....wwwwwwwww.",
    ".bpbgllwwwwwwwwwwt..",
    "..bgllwwwwwwwwwttm..",
    ".ggwwbbwwwwwwtttmm..",
    "ggwwbehbwwwttmmtm...",
    "gwwwbeebwttmmmmmm...",
    "gwwwwbbwtmmmmmmmm...",
    "gcwwwwwwmmmmmtmmm...",
    "gcwwwwwmmrrrrtttt...",
    ".gcwwwwcctttttccc...",
    ".gccwwccccccccc.....",
    "..gccccccccc........",
    "...ccccc............",
];

// ---------------------------------------------------------------------------
// Tiny 5px-tall font for the speech bubble.
// ---------------------------------------------------------------------------

/// The pixel-font glyph for `ch`, if the font has one.
fn glyph(ch: char) -> Option<&'static [&'static str]> {
    Some(match ch {
        'A' => &[".##.", "#..#", "####", "#..#", "#..#"],
        'E' => &["###", "#..", "##.", "#..", "###"],
        'H' => &["#..#", "#..#", "####", "#..#", "#..#"],
        'R' => &["###.", "#..#", "###.", "#.#.", "#..#"],
        'G' => &[".###", "#...", "#.##", "#..#", ".###"],
        'S' => &[".###", "#...", ".##.", "...#", "###."],
        '!' => &["#", "#", "#", ".", "#"],
        _ => return None,
    })
}

/// A one-pixel-wide space, drawn for characters the font doesn't have.
const BLANK_GLYPH: &[&str] = &[".", ".", ".", ".", "."];

fn glyph_or_blank(ch: char) -> &'static [&'static str] {
    glyph(ch).unwrap_or(BLANK_GLYPH)
}

/// Whether `ch` has a real glyph (anything else renders as a blank).
#[cfg(test)]
pub fn has_glyph(ch: char) -> bool {
    glyph(ch).is_some()
}

// ---------------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------------

/// An indexed-colour pixel canvas. Row 0 is the top.
#[derive(Clone)]
pub struct Canvas {
    pub w: i32,
    pub h: i32,
    px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Self {
        Self { w, h, px: vec![CLEAR; (w * h) as usize] }
    }

    fn get(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            CLEAR
        } else {
            self.px[(y * self.w + x) as usize]
        }
    }

    fn set(&mut self, x: i32, y: i32, c: u8) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = c;
        }
    }

    fn stamp(&mut self, grid: &[&str], ox: i32, oy: i32) {
        for (gy, row) in grid.iter().enumerate() {
            for (gx, ch) in row.bytes().enumerate() {
                let c = color_for(ch);
                if c != CLEAR {
                    self.set(ox + gx as i32, oy + gy as i32, c);
                }
            }
        }
    }

    /// Filled disc; a pixel is inside if its centre is within `r`.
    fn disc(&mut self, cx: f32, cy: f32, r: f32, c: u8) {
        let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                if dx * dx + dy * dy <= r * r {
                    self.set(x, y, c);
                }
            }
        }
    }

    /// Adds a 1px outline around every opaque region (4-neighbourhood, so
    /// diagonals stay clean instead of getting chunky corners).
    fn outline(&mut self) {
        let src = self.clone();
        for y in 0..self.h {
            for x in 0..self.w {
                if src.get(x, y) != CLEAR {
                    continue;
                }
                let touches = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|&(dx, dy)| src.get(x + dx, y + dy) != CLEAR);
                if touches {
                    self.set(x, y, OUTLINE);
                }
            }
        }
    }

    /// Nearest-neighbour upscaled RGBA. Alpha is only ever 0 or 255, so it
    /// is valid as both straight and premultiplied alpha.
    pub fn rgba_scaled(&self, s: usize) -> Vec<u8> {
        let (w, h) = (self.w as usize, self.h as usize);
        let mut out = vec![0u8; w * s * h * s * 4];
        for y in 0..h * s {
            for x in 0..w * s {
                let c = self.px[(y / s) * w + x / s];
                if c != CLEAR {
                    let [r, g, b] = PALETTE[c as usize];
                    let i = (y * w * s + x) * 4;
                    out[i..i + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
        }
        out
    }

    /// Half-size copy that keeps every other pixel (for the 16px icon).
    pub fn half(&self) -> Canvas {
        let mut out = Canvas::new(self.w / 2, self.h / 2);
        for y in 0..out.h {
            for x in 0..out.w {
                out.set(x, y, self.get(x * 2, y * 2));
            }
        }
        out
    }

    /// Paste another canvas on top (opaque pixels only).
    pub fn blit(&mut self, other: &Canvas, ox: i32, oy: i32) {
        for y in 0..other.h {
            for x in 0..other.w {
                let c = other.get(x, y);
                if c != CLEAR {
                    self.set(ox + x, oy + y, c);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The possum
// ---------------------------------------------------------------------------

/// Sprite canvas size in pixels.
pub const POSSUM_W: i32 = 64;
pub const POSSUM_H: i32 = 32;
/// Row the paws stand on.
const GROUND: i32 = 29;
/// The point (in sprite pixels, from the top-left) that touches the screen
/// edge: horizontally centred under the body, at the bottom of the outline.
pub const FEET_X: f32 = 34.0;
pub const FEET_Y: f32 = (GROUND + 2) as f32;
/// Where the heads are stamped. The screaming head is wider (its snout
/// points up and forward), so it starts a pixel further left.
const HEAD_X: i32 = 44;
const SCREAM_HEAD_X: i32 = HEAD_X - 1;
const HEAD_Y: i32 = 4;
/// Just above the screaming head's raised snout: where the speech bubble
/// points to.
pub const BUBBLE_ANCHOR: (f32, f32) = ((SCREAM_HEAD_X + 12) as f32, (HEAD_Y - 1) as f32);
/// Just in front of the screaming head's open mouth: where the yell lines
/// start.
pub const MOUTH_ANCHOR: (f32, f32) = ((SCREAM_HEAD_X + 19) as f32, (HEAD_Y + 6) as f32);

pub const RUN_FRAMES: usize = 8;
const STRIDE: f32 = 8.0;
const LIFT: f32 = 2.6;

struct Leg {
    hip: (f32, f32),
    phase: f32,
    near: bool,
}

const LEGS: [Leg; 4] = [
    Leg { hip: (23.0, 23.0), phase: 0.7, near: false },     // back, far side
    Leg { hip: (40.0, 21.5), phase: PI + 0.7, near: false }, // front, far side
    Leg { hip: (26.5, 23.5), phase: 0.0, near: true },      // back, near side
    Leg { hip: (43.5, 22.0), phase: PI, near: true },       // front, near side
];

/// Paw position for a gait phase: on the ground sliding backwards for the
/// first half of the cycle, then lifted and swung forward.
fn paw(hip_x: f32, phase: f32) -> (f32, f32) {
    let p = phase.rem_euclid(TAU);
    if p < PI {
        let s = p / PI;
        (hip_x + STRIDE * (0.5 - s), 0.0)
    } else {
        let s = (p - PI) / PI;
        let eased = s * s * (3.0 - 2.0 * s);
        (hip_x + STRIDE * (eased - 0.5), LIFT * (PI * s).sin())
    }
}

fn draw_leg(c: &mut Canvas, hip: (f32, f32), phase: f32, dy: f32, near: bool) {
    let (px, lift) = paw(hip.0, phase);
    let foot_y = GROUND as f32 - lift;
    let hip = (hip.0, hip.1 + dy);
    let color = if near { BLACK } else { LEG_FAR };
    // Thick thigh tapering into a thin shin.
    let steps = 24;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = hip.0 + (px - hip.0) * t;
        let y = hip.1 + (foot_y - 0.5 - hip.1) * t;
        c.disc(x, y, 1.6 - 0.7 * t, color);
    }
    // Pink paw with toes pointing forward.
    let (fx, fy) = (px.round() as i32, foot_y.round() as i32);
    for x in fx - 1..=fx + 2 {
        c.set(x, fy, PINK);
    }
}

fn draw_tail(c: &mut Canvas, dy: f32, wiggle: f32) {
    let p0 = (20.0, 19.5 + dy);
    let p1 = (7.0, 27.0 + dy);
    let p2 = (1.5, 13.5 + dy + wiggle);
    let steps = 80;
    // Two passes: a dark underside, then the top, offset by half a pixel.
    for pass in 0..2 {
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let u = 1.0 - t;
            let x = u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0;
            let y = u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1;
            let r = 1.9 - 1.3 * t;
            let furry = t < 0.16; // possum tails are furry only at the base
            if pass == 0 {
                c.disc(x, y + 0.7, r, if furry { FUR_DARK } else { PINK_DARK });
            } else {
                c.disc(x, y, r * 0.9, if furry { FUR } else { PINK });
            }
        }
    }
}

/// Cheap integer hash for stable "random" fur texture.
fn hash(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1);
    h = (h ^ (h >> 15)).wrapping_mul(0x85eb_ca6b);
    h ^ (h >> 13)
}

fn draw_body(c: &mut Canvas, dy: f32) {
    let inside = |x: f32, y: f32| {
        let e = |cx: f32, cy: f32, rx: f32, ry: f32| {
            ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0
        };
        // Big haunches at the back, narrower shoulders at the front.
        e(29.5, 19.0, 11.5, 6.6) || e(41.0, 17.6, 8.5, 6.0)
    };
    let shift = dy as i32;
    for x in 0..c.w {
        let col: Vec<i32> = (0..c.h)
            .filter(|&y| inside(x as f32 + 0.5, (y - shift) as f32 + 0.5))
            .collect();
        let (Some(&top), Some(&bot)) = (col.first(), col.last()) else { continue };
        for &y in &col {
            let t = (y - top) as f32 / (bot - top).max(1) as f32;
            let ly = y - shift; // body-local y, so the fur texture bobs along
            let mut color = if t < 0.24 {
                FUR_LIGHT
            } else if t > 0.72 {
                FUR_DARK
            } else {
                FUR
            };
            // Grizzled fur: scattered frosty guard hairs.
            let speckle = hash(x, ly) % 100;
            if color == FUR && speckle < 22 {
                color = FUR_LIGHT;
            } else if color == FUR_LIGHT && speckle < 30 {
                color = FROST;
            } else if color == FUR_DARK && speckle < 15 {
                color = FUR;
            }
            // Pale throat and chest blending into the white face.
            if x >= 46 && t > 0.35 {
                color = if (x + ly) % 3 == 0 { CREAM } else { FROST };
            }
            c.set(x, y, color);
        }
    }
}

/// One frame of the run cycle.
fn possum_frame(frame: usize, screaming: bool) -> Canvas {
    let mut c = Canvas::new(POSSUM_W, POSSUM_H);
    let phase = frame as f32 / RUN_FRAMES as f32 * TAU;
    // Bob up twice per stride, when the paws are pushing off.
    let dy = if matches!(frame % 4, 1 | 2) { -1.0 } else { 0.0 };

    draw_tail(&mut c, dy, (phase).sin() * 1.6);
    for leg in LEGS.iter().filter(|l| !l.near) {
        draw_leg(&mut c, leg.hip, phase + leg.phase, dy, false);
    }
    draw_body(&mut c, dy);
    for leg in LEGS.iter().filter(|l| l.near) {
        draw_leg(&mut c, leg.hip, phase + leg.phase, dy, true);
    }
    let (head, hx): (&[&str], i32) =
        if screaming { (&HEAD_SCREAM, SCREAM_HEAD_X) } else { (&HEAD_OPEN, HEAD_X) };
    // While screaming the head shakes with the effort of it.
    let shake = if screaming && frame % 2 == 1 { -1 } else { 0 };
    c.stamp(head, hx, HEAD_Y + dy as i32 + shake);
    c.outline();
    c
}

pub struct PossumArt {
    pub run: Vec<Canvas>,
    pub scream: Vec<Canvas>,
}

pub fn possum() -> PossumArt {
    PossumArt {
        run: (0..RUN_FRAMES).map(|f| possum_frame(f, false)).collect(),
        scream: (0..RUN_FRAMES).map(|f| possum_frame(f, true)).collect(),
    }
}

// ---------------------------------------------------------------------------
// Speech bubble and yell lines
// ---------------------------------------------------------------------------

/// Which corner of the speech bubble its pointer sticks out of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    BottomLeft,
    BottomRight,
    TopRight,
    TopLeft,
}

impl Pointer {
    pub const ALL: [Pointer; 4] =
        [Pointer::BottomLeft, Pointer::BottomRight, Pointer::TopRight, Pointer::TopLeft];

    fn at_top(self) -> bool {
        matches!(self, Pointer::TopLeft | Pointer::TopRight)
    }

    fn at_right(self) -> bool {
        matches!(self, Pointer::BottomRight | Pointer::TopRight)
    }

    /// Tip of the pointer in unit coordinates (y-up, like `anchorPoint`).
    pub fn tip(self, w: i32) -> (f64, f64) {
        let x = 1.5 / w as f64;
        let x = if self.at_right() { 1.0 - x } else { x };
        (x, if self.at_top() { 1.0 } else { 0.0 })
    }
}

/// A speech bubble with pixel text and a pointer at the given corner. The
/// text is never mirrored, so it reads correctly whichever way it points.
pub fn bubble(text: &str, pointer: Pointer) -> Canvas {
    let glyphs: Vec<&[&str]> = text.chars().map(glyph_or_blank).collect();
    let text_w: i32 = glyphs.iter().map(|g| g[0].len() as i32).sum::<i32>()
        + glyphs.len().saturating_sub(1) as i32;
    let pad = 3;
    let (bw, bh) = (text_w + pad * 2, 5 + pad * 2);
    let tail = 4;
    let (w, h) = (bw + 2, bh + tail + 2);
    let mut c = Canvas::new(w, h);
    let flip_x = |x: i32| if pointer.at_right() { w - 1 - x } else { x };
    let top = if pointer.at_top() { tail + 1 } else { 1 };

    for y in 0..bh {
        for x in 0..bw {
            let corner = (x == 0 || x == bw - 1) && (y == 0 || y == bh - 1);
            if !corner {
                c.set(x + 1, top + y, BUBBLE);
            }
        }
    }
    // Pointer slanting out of the corner, widest where it meets the bubble.
    let rows: [(i32, i32); 4] = [(3, 7), (2, 5), (2, 3), (1, 2)];
    for (k, &(x0, x1)) in rows.iter().enumerate() {
        let y = if pointer.at_top() { tail - k as i32 } else { bh + 1 + k as i32 };
        for x in x0..=x1 {
            c.set(flip_x(x), y, BUBBLE);
        }
    }

    let mut x = 1 + pad;
    for g in glyphs {
        c.stamp(g, x, top + pad);
        x += g[0].len() as i32 + 1;
    }
    c.outline();
    c
}

/// The full screaming pose, as the app shows it in "Scream silently" mode:
/// the screaming possum, its speech bubble saying `shout`, and its yell
/// lines, with a 1px margin. Also returns where the possum's top-left
/// corner sits inside the canvas.
pub fn screaming_pose(shout: &str) -> (Canvas, (i32, i32)) {
    let bubble = bubble(shout, Pointer::BottomLeft);
    let yell = yell(1);
    let (bx, by) = (BUBBLE_ANCHOR.0 as i32, BUBBLE_ANCHOR.1 as i32);
    let (mx, my) = (MOUTH_ANCHOR.0 as i32, MOUTH_ANCHOR.1 as i32);
    // The bubble's pointer tip is its bottom-left pixel, (1, h - 1).
    let (ox, oy) = (1, bubble.h - by + 1);
    let w = (ox + POSSUM_W).max(ox + bx - 1 + bubble.w).max(ox + mx + yell.w) + 1;
    let mut c = Canvas::new(w, oy + POSSUM_H + 1);
    c.blit(&possum_frame(0, true), ox, oy);
    c.blit(&bubble, ox + bx - 1, oy + by - bubble.h);
    c.blit(&yell, ox + mx, oy + my - yell.h / 2);
    (c, (ox, oy))
}

/// Little white "sound" strokes radiating from the mouth: one rising, one
/// level, one falling. Frame 1 has them pushed further out.
pub fn yell(frame: usize) -> Canvas {
    let mut c = Canvas::new(13, 17);
    let o = 1 + 2 * frame as i32;
    for k in 0..4 {
        c.set(o + k, 5 - k, WHITE); // rising
        c.set(o + 1 + k, 8, WHITE); // level
        c.set(o + k, 11 + k, WHITE); // falling
    }
    c.outline();
    c
}

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------

/// Small screaming head for the menu bar.
pub fn status_icon() -> Canvas {
    let mut c = Canvas::new(22, 17);
    c.stamp(&HEAD_SCREAM, 1, 1);
    c.outline();
    c
}

/// App icon: the screaming possum head on a rounded "night sky" square.
pub fn app_icon() -> Canvas {
    let n = 32;
    let mut c = Canvas::new(n, n);
    let r = 6.0f32;
    for y in 1..n - 1 {
        for x in 1..n - 1 {
            // Rounded square with inset corners.
            let cx = (x as f32 + 0.5).clamp(1.0 + r, n as f32 - 1.0 - r);
            let cy = (y as f32 + 0.5).clamp(1.0 + r, n as f32 - 1.0 - r);
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            if dx * dx + dy * dy <= r * r {
                c.set(x, y, if y > n / 2 + 6 { ICON_BG_DARK } else { ICON_BG });
            }
        }
    }
    c.blit(&status_icon(), 2, 11);
    // A tiny "A!" shout in the corner.
    let mut shout = Canvas::new(12, 9);
    for y in 1..8 {
        for x in 1..11 {
            if !((x == 1 || x == 10) && (y == 1 || y == 7)) {
                shout.set(x, y, BUBBLE);
            }
        }
    }
    shout.stamp(glyph_or_blank('A'), 3, 2);
    shout.stamp(glyph_or_blank('!'), 8, 2);
    shout.outline();
    c.blit(&shout, 18, 3);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_grids_are_rectangular() {
        for row in HEAD_OPEN {
            assert_eq!(row.len(), 18, "bad row: {row:?}");
        }
        for row in HEAD_SCREAM {
            assert_eq!(row.len(), 20, "bad row: {row:?}");
        }
    }

    #[test]
    fn frames_have_expected_size_and_content() {
        let art = possum();
        assert_eq!(art.run.len(), RUN_FRAMES);
        for f in art.run.iter().chain(art.scream.iter()) {
            assert_eq!((f.w, f.h), (POSSUM_W, POSSUM_H));
            let opaque = f.px.iter().filter(|&&p| p != CLEAR).count();
            assert!(opaque > 500, "frame is suspiciously empty");
            // Paws must touch the ground row in every frame.
            assert!((0..f.w).any(|x| f.get(x, GROUND) == PINK));
        }
    }

    #[test]
    fn bubble_pointer_tip_is_where_the_anchor_says() {
        for p in Pointer::ALL {
            let b = bubble("AAAA!", p);
            let (ux, uy) = p.tip(b.w);
            // Convert the unit-space tip back to the pixel just inside it.
            let x = (ux * b.w as f64).floor() as i32;
            let y = if uy > 0.5 { 0 } else { b.h - 1 };
            assert_ne!(b.get(x, y), CLEAR, "{p:?}: nothing at tip ({x},{y})");
            // ...and the opposite corner of that edge is empty.
            assert_eq!(b.get(b.w - 1 - x, y), CLEAR, "{p:?}");
        }
    }
}
