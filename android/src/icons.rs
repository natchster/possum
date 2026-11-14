//! Writes the Android launcher icon: the desktop app's 32×32 icon at each
//! screen density, scaled by whole pixels (and padded where the density's
//! size isn't a multiple of 32) so the pixel art stays crisp.
//!
//! Usage: possum-icons <res folder>

#[path = "../../src/png.rs"]
mod png;
#[allow(dead_code, reason = "only the icon is drawn here")]
#[path = "../../src/sprite.rs"]
mod sprite;

fn main() {
    let Some(res) = std::env::args().nth(1) else {
        eprintln!("usage: possum-icons <res folder>");
        std::process::exit(2)
    };
    let icon = sprite::app_icon();
    // Android's launcher icon sizes: 48 dp at each density.
    for (density, size) in [("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192)] {
        let scale = (size / icon.w).max(1);
        let side = size / scale;
        let mut canvas = sprite::Canvas::new(side, side);
        canvas.blit(&icon, (side - icon.w) / 2, (side - icon.h) / 2);
        let dir = std::path::Path::new(&res).join(format!("mipmap-{density}"));
        let path = dir.join("ic_launcher.png");
        let png = png::encode_rgba(size as u32, size as u32, &canvas.rgba_scaled(scale as usize));
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, png)) {
            eprintln!("possum-icons: cannot write {}: {e}", path.display());
            std::process::exit(1)
        }
    }
}
