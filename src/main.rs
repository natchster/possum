//! Possum: a screaming pixel-art possum that runs counterclockwise around the
//! edge of your screen. Runs on macOS, Windows and Linux.
//!
//! * `brain.rs`: where it runs, when it screams, when it keeps quiet.
//! * `sprite.rs`, `scream.rs`: the pixel art and the sounds, generated at
//!   start-up.
//! * `track.rs`, `screens.rs`, `quiet.rs`: the path around the screen, which
//!   display to run on, and when not to make a sound.
//! * `platform/`: the operating-system parts (overlay window, sound, menu).
//! * `render.rs`: software drawing for Windows and Linux.

// A GUI app on Windows: no console window pops up. It attaches to the
// terminal it was started from, if any, for --help and --verbose.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod brain;
mod platform;
mod png;
mod quiet;
#[cfg_attr(target_os = "macos", allow(dead_code))]
mod render;
mod scream;
mod screens;
mod sprite;
mod track;

use std::sync::atomic::Ordering;

use sprite::Canvas;

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

pub struct Options {
    pub scale: f64,
    pub speed: f64,
    pub volume: f32,
    pub silent: bool,
    pub verbose: bool,
    pub start: f64,
    preview: Option<String>,
    portrait: Option<String>,
    iconset: Option<String>,
}

const HELP: &str = "\
possum - a screaming pixel-art possum runs counterclockwise around your screen

USAGE:
    possum [OPTIONS]

OPTIONS:
    --scale <N>          Size of one sprite pixel (default 3), in points on
                         macOS and display-scaled pixels elsewhere
    --speed <N>          Running speed in the same units per second (default 260)
    --volume <0..1>      Scream volume (default 0.6)
    --silent             Start in \"Scream silently\" mode: a speech bubble
                         instead of sound (switch in the menu too)
    --start <0..1>       Where on the border to start, as a fraction of one
                         lap from the bottom-left corner (default 0)
    --preview <FILE>     Write a PNG sprite sheet and exit
    --portrait <FILE>    Write a PNG of the screaming possum and exit
    --iconset <DIR>      Write a macOS .iconset folder and exit
    -v, --verbose        Log screams, quiet times and display changes to the
                         terminal
    -h, --help           Show this help

Quit from the possum's menu-bar or tray icon, or with Ctrl+C in the terminal.";

/// Parses a numeric option. NaN and infinity are rejected: they would slip
/// through `clamp` and wreck the window geometry.
fn parse_number(name: &str, v: &str) -> Result<f64, String> {
    match v.parse::<f64>() {
        Ok(x) if x.is_finite() => Ok(x),
        _ => Err(format!("possum: {name} expects a number, got {v:?}")),
    }
}

fn parse_args() -> Options {
    let mut o = Options {
        scale: 3.0,
        speed: 260.0,
        volume: 0.6,
        silent: false,
        verbose: false,
        start: 0.0,
        preview: None,
        portrait: None,
        iconset: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next().unwrap_or_else(|| {
                eprintln!("possum: {name} needs a value\n\n{HELP}");
                std::process::exit(2)
            })
        };
        let number = |name: &str, v: String| -> f64 {
            parse_number(name, &v).unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(2)
            })
        };
        match arg.as_str() {
            "--scale" => o.scale = number("--scale", value("--scale")).clamp(1.0, 12.0),
            "--speed" => o.speed = number("--speed", value("--speed")).clamp(10.0, 5000.0),
            "--volume" => o.volume = number("--volume", value("--volume")).clamp(0.0, 1.0) as f32,
            "--silent" | "--mute" => o.silent = true,
            "-v" | "--verbose" => o.verbose = true,
            "--start" => o.start = number("--start", value("--start")).rem_euclid(1.0),
            "--preview" => o.preview = Some(value("--preview")),
            "--portrait" => o.portrait = Some(value("--portrait")),
            "--iconset" => o.iconset = Some(value("--iconset")),
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0)
            }
            // Finder passes this when launching an app bundle on old macOS.
            s if s.starts_with("-psn_") => {}
            other => {
                eprintln!("possum: unknown argument {other:?}\n\n{HELP}");
                std::process::exit(2)
            }
        }
    }
    o
}

// ---------------------------------------------------------------------------
// Offline outputs: preview sheet, portrait and icon set
// ---------------------------------------------------------------------------

fn write_png(path: &std::path::Path, c: &Canvas, scale: usize, bg: Option<[u8; 3]>) {
    let mut rgba = c.rgba_scaled(scale);
    if let Some([r, g, b]) = bg {
        for px in rgba.chunks_mut(4) {
            if px[3] == 0 {
                px.copy_from_slice(&[r, g, b, 255]);
            }
        }
    }
    let png = png::encode_rgba(c.w as u32 * scale as u32, c.h as u32 * scale as u32, &rgba);
    std::fs::write(path, png).unwrap_or_else(|e| {
        eprintln!("possum: cannot write {}: {e}", path.display());
        std::process::exit(1)
    });
}

/// Sprite sheet: 8 run frames, 8 scream frames, and the full screaming pose.
fn write_preview(path: &str) {
    let art = sprite::possum();
    let (cw, ch) = (sprite::POSSUM_W + 4, sprite::POSSUM_H + 4);
    let (pose, (px, _)) = sprite::screaming_pose(brain::AAAA_SHOUTS[0]);
    let pose_y = ch * 4 + 2;
    let mut sheet = Canvas::new(cw * 4, pose_y + pose.h + 3);
    for (i, frame) in art.run.iter().chain(art.scream.iter()).enumerate() {
        let i = i as i32;
        sheet.blit(frame, (i % 4) * cw + 2, (i / 4) * ch + 2);
    }
    // Line the pose's possum up with the frames above it.
    sheet.blit(&pose, 2 - px, pose_y);
    write_png(path.as_ref(), &sheet, 6, Some([0x7a, 0xa5, 0xd6]));
    println!("wrote {path}");
}

/// The screaming possum with its bubble, on a transparent background: the
/// picture at the top of the README (`docs/possum.png`).
fn write_portrait(path: &str) {
    let (pose, _) = sprite::screaming_pose(brain::AAAA_SHOUTS[0]);
    write_png(path.as_ref(), &pose, 8, None);
    println!("wrote {path}");
}

fn write_iconset(dir: &str) {
    let dir = std::path::Path::new(dir);
    std::fs::create_dir_all(dir).unwrap_or_else(|e| {
        eprintln!("possum: cannot create {}: {e}", dir.display());
        std::process::exit(1)
    });
    let icon = sprite::app_icon(); // 32x32
    // (file name, pixel size); every size is an integer multiple of 32 except
    // the 16px one, which gets a 2:1 nearest-neighbour downscale.
    let sizes = [
        ("icon_16x16.png", 16),
        ("icon_16x16@2x.png", 32),
        ("icon_32x32.png", 32),
        ("icon_32x32@2x.png", 64),
        ("icon_128x128.png", 128),
        ("icon_128x128@2x.png", 256),
        ("icon_256x256.png", 256),
        ("icon_256x256@2x.png", 512),
        ("icon_512x512.png", 512),
        ("icon_512x512@2x.png", 1024),
    ];
    let small = icon.half();
    for (name, px) in sizes {
        let path = dir.join(name);
        if px >= 32 {
            write_png(&path, &icon, px / 32, None);
        } else {
            write_png(&path, &small, 1, None);
        }
    }
    println!("wrote {}", dir.display());
}

// ---------------------------------------------------------------------------

fn main() {
    platform::prepare_console();
    let opts = parse_args();
    if let Some(path) = &opts.preview {
        return write_preview(path);
    }
    if let Some(path) = &opts.portrait {
        return write_portrait(path);
    }
    if let Some(dir) = &opts.iconset {
        return write_iconset(dir);
    }
    brain::SILENT.store(opts.silent, Ordering::Relaxed);
    platform::run(&opts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_must_be_finite() {
        assert_eq!(parse_number("--scale", "3.5"), Ok(3.5));
        assert_eq!(parse_number("--speed", "-2"), Ok(-2.0));
        for bad in ["nan", "NaN", "inf", "-inf", "1e400", "", "fast"] {
            assert!(parse_number("--scale", bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn every_shout_can_be_drawn_in_the_pixel_font() {
        for text in brain::AAAA_SHOUTS.iter().chain(brain::HISS_SHOUTS.iter()) {
            for ch in text.chars() {
                assert!(sprite::has_glyph(ch), "no glyph for {ch:?} in {text:?}");
            }
        }
    }
}
