//! For Windows builds: gives possum.exe its icon, the same pixel-art head as
//! the Mac app's, drawn by `sprite::app_icon` at build time. Windows shows
//! it for the file and in Task Manager; Wine on a Mac shows it in the Dock
//! (without it, Wine's own icon).
//!
//! The icons go into a resource file (`.res`: an icon group and one icon per
//! size), which both Microsoft's linker and lld-link take as an input.

#[allow(dead_code, reason = "only the PNG encoder is needed here")]
#[path = "src/png.rs"]
mod png;
#[allow(dead_code, reason = "only the app icon is needed here")]
#[path = "src/sprite.rs"]
mod sprite;

use std::path::PathBuf;

/// Resource types.
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
/// Movable, discardable (and pure, for the group): what resource compilers use.
const ICON_FLAGS: u16 = 0x1010;
const GROUP_FLAGS: u16 = 0x1030;
const LANG_EN_US: u16 = 0x0409;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/sprite.rs");
    println!("cargo:rerun-if-changed=src/png.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let res = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("possum.res");
    std::fs::write(&res, icon_resources()).expect("can't write possum.res");
    println!("cargo:rustc-link-arg-bins={}", res.display());
}

/// The resource file: an empty first entry (the format's header), the
/// icons, and the group that lists them (resource 1, the exe's icon).
fn icon_resources() -> Vec<u8> {
    let icon = sprite::app_icon(); // 32×32
    let small = icon.half();
    // Whole-pixel scales, so the pixel art stays crisp; Windows picks the
    // nearest size for anything in between.
    let images: Vec<(usize, Vec<u8>)> = [(16, &small, 1), (32, &icon, 1), (64, &icon, 2), (256, &icon, 8)]
        .into_iter()
        .map(|(size, canvas, scale)| {
            let rgba = canvas.rgba_scaled(scale);
            // Big icons are stored as PNG, the rest as bitmaps, as Windows'
            // own icons are.
            let data = if size == 256 { png::encode_rgba(256, 256, &rgba) } else { bitmap_icon(&rgba, size) };
            (size, data)
        })
        .collect();

    let mut out = Vec::new();
    entry(&mut out, 0, 0, 0, 0, &[]);
    let mut group = Vec::new();
    group.extend(0u16.to_le_bytes()); // reserved
    group.extend(1u16.to_le_bytes()); // icons (2 would be cursors)
    group.extend((images.len() as u16).to_le_bytes());
    for (i, (size, data)) in images.iter().enumerate() {
        let id = i as u16 + 1;
        entry(&mut out, RT_ICON, id, ICON_FLAGS, LANG_EN_US, data);
        let side = if *size >= 256 { 0 } else { *size as u8 }; // 0 means 256
        group.extend([side, side, 0, 0]); // width, height, palette size, reserved
        group.extend(1u16.to_le_bytes()); // planes
        group.extend(32u16.to_le_bytes()); // bits per pixel
        group.extend((data.len() as u32).to_le_bytes());
        group.extend(id.to_le_bytes());
    }
    entry(&mut out, RT_GROUP_ICON, 1, GROUP_FLAGS, LANG_EN_US, &group);
    out
}

/// One resource: a 32-byte header (numbered type and name), then the data,
/// padded to 4 bytes.
fn entry(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, language: u16, data: &[u8]) {
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes()); // header size
    out.extend([0xFF, 0xFF]);
    out.extend(kind.to_le_bytes());
    out.extend([0xFF, 0xFF]);
    out.extend(id.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // data version
    out.extend(flags.to_le_bytes());
    out.extend(language.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // version
    out.extend(0u32.to_le_bytes()); // characteristics
    out.extend(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

/// An icon image as Windows stores small ones: a 32-bit bitmap, bottom row
/// first, twice as tall as the image because a 1-bit mask follows it
/// (set where the image is transparent, for anything that ignores alpha).
fn bitmap_icon(rgba: &[u8], size: usize) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4;
    let mut out = Vec::with_capacity(40 + size * size * 4 + size * mask_row);
    out.extend(40u32.to_le_bytes()); // BITMAPINFOHEADER
    out.extend((size as i32).to_le_bytes());
    out.extend((2 * size as i32).to_le_bytes());
    out.extend(1u16.to_le_bytes()); // planes
    out.extend(32u16.to_le_bytes()); // bits per pixel
    out.extend(0u32.to_le_bytes()); // uncompressed
    out.extend(((size * size * 4 + size * mask_row) as u32).to_le_bytes());
    out.extend([0u8; 16]); // resolution, palette: unused
    let pixel = |x: usize, y: usize| &rgba[(y * size + x) * 4..(y * size + x) * 4 + 4];
    for y in (0..size).rev() {
        for x in 0..size {
            let [r, g, b, a] = pixel(x, y).try_into().expect("4 bytes");
            out.extend([b, g, r, a]);
        }
    }
    for y in (0..size).rev() {
        let mut row = vec![0u8; mask_row];
        for x in 0..size {
            if pixel(x, y)[3] == 0 {
                row[x / 8] |= 0x80 >> (x % 8);
            }
        }
        out.extend(row);
    }
    out
}
