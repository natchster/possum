//! Linux: X11 (directly, or through XWayland on Wayland desktops).
//!
//! * Two small override-redirect windows (the possum and its speech bubble),
//!   drawn with the software renderer. The pixel art is either fully opaque
//!   or fully transparent, so instead of alpha blending each window is *cut
//!   to the shape* of its picture (the X11 Shape extension, as xeyes and
//!   oneko do). That needs no compositor, and it avoids 32-bit alpha
//!   windows, which crash Xwayland 24.1 (and with it GNOME Shell) when it
//!   runs without GPU buffer sharing, as in virtual machines. An empty input
//!   shape makes the windows click-through. Override-redirect windows aren't
//!   managed by the window manager, so they sit on top of everything,
//!   including panels and docks.
//! * Sounds are written once to the user's runtime directory and played with
//!   whichever player the system has: `pw-play` (PipeWire), `paplay`
//!   (PulseAudio) or `aplay` (ALSA).
//! * Quiet times: a background thread asks systemd-logind whether the
//!   session is locked or inactive, the desktop whether its screensaver is
//!   active (over D-Bus), and X11's DPMS whether the display is off.
//! * The menu is a StatusNotifierItem tray icon (KDE, most desktops; GNOME
//!   with the AppIndicator extension, which Ubuntu ships enabled).

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::dpms::ConnectionExt as _;
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    AtomEnum, ClipOrdering, ConfigureWindowAux, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask,
    ImageFormat, ImageOrder, PropMode, Rectangle, StackMode, Window, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use crate::brain::{self, Brain, Host, Scene, Settings, QUIT, SCREAM_NOW, SILENT};
use crate::render::{self, Art, Image};
use crate::scream::{Kind, Rng};
use crate::{quiet, screens, sprite, Options};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn run(opts: &Options) {
    if let Err(e) = try_run(opts) {
        eprintln!("possum: {e}");
        if std::env::var_os("DISPLAY").is_none() {
            eprintln!(
                "possum: it needs an X11 display. On Wayland desktops that's XWayland, which GNOME and \
                 KDE start automatically; check that DISPLAY is set."
            );
        }
        std::process::exit(1);
    }
}

fn try_run(opts: &Options) -> Result<()> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let x = X11::new(conn, screen_num)?;

    let seed = super::seed();
    let mut rng = Rng::new(seed);
    let voices = brain::voices(seed, &mut rng);
    let durations = voices.each_ref().map(|v| v.iter().map(|s| s.seconds).collect());
    let mut sounds = Sounds::new(&voices, opts.volume)?;

    let screen = x.primary_monitor();
    let settings = Settings { scale: opts.scale, speed: opts.speed, start: opts.start, verbose: opts.verbose };
    let mut brain = Brain::new(&settings, screen.w, screen.h, durations, rng);
    brain.update(sounds.probe());
    brain.log(format_args!("running around the primary display, {}x{}", screen.w, screen.h));
    if sounds.player.is_none() {
        eprintln!("possum: no audio player found (pw-play, paplay or aplay), so it screams silently");
    }

    let _tray = start_tray();
    let art = Art::new();
    let reach = render::reach(&art, opts.scale);
    let mut app =
        App { x, art, sounds, brain, screen, scale: opts.scale, reach, possum_key: None, bubble_key: None };

    let clock = Instant::now();
    let frame = Duration::from_secs_f64(1.0 / 60.0);
    let mut next = Instant::now();
    while !QUIT.load(Ordering::Relaxed) {
        app.tick(clock.elapsed().as_secs_f64())?;
        next += frame;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
    app.sounds.hush();
    Ok(())
}

// ---------------------------------------------------------------------------
// X11 windows
// ---------------------------------------------------------------------------

struct X11 {
    conn: RustConnection,
    root: Window,
    possum: Window,
    bubble: Window,
    gc: u32,
    /// Bytes of image data that fit in one request.
    max_chunk: usize,
    bubble_mapped: bool,
}

impl X11 {
    fn new(conn: RustConnection, screen_num: usize) -> Result<Self> {
        let setup = conn.setup();
        let screen = &setup.roots[screen_num];
        let root = screen.root;
        // Ordinary 24-bit windows: pixels go in as 32-bit little-endian BGRx,
        // which is exactly our images minus the (unused) alpha byte.
        let format_ok = setup.pixmap_formats.iter().any(|f| f.depth == 24 && f.bits_per_pixel == 32);
        if screen.root_depth != 24 || !format_ok || setup.image_byte_order != ImageOrder::LSB_FIRST {
            return Err(format!("unsupported display format (depth {})", screen.root_depth).into());
        }
        let (visual, depth) = (screen.root_visual, screen.root_depth);

        let make_window = |name: &str| -> Result<Window> {
            let win = conn.generate_id()?;
            let aux = CreateWindowAux::new().override_redirect(1).event_mask(EventMask::EXPOSURE);
            conn.create_window(depth, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_OUTPUT, visual, &aux)?;
            // Invisible until there's a picture: an empty bounding shape.
            conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, ClipOrdering::UNSORTED, win, 0, 0, &[])?;
            // An empty input shape: clicks go straight through.
            conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, win, 0, 0, &[])?;
            conn.change_property(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, 8, name.len() as u32, name.as_bytes())?;
            let class = b"possum\0Possum\0";
            conn.change_property(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, 8, class.len() as u32, class)?;
            Ok(win)
        };
        let possum = make_window("Possum")?;
        let bubble = make_window("Possum bubble")?;
        let gc = conn.generate_id()?;
        conn.create_gc(gc, possum, &CreateGCAux::new())?;
        // Hear about display changes right away.
        conn.randr_select_input(root, randr::NotifyMask::SCREEN_CHANGE | randr::NotifyMask::CRTC_CHANGE)?;
        conn.map_window(possum)?;
        conn.flush()?;
        let max_chunk = conn.maximum_request_bytes().saturating_sub(64);
        Ok(Self { conn, root, possum, bubble, gc, max_chunk, bubble_mapped: false })
    }

    /// The primary monitor in root-window coordinates (y-down).
    fn primary_monitor(&self) -> screens::Rect {
        let monitors = self.conn.randr_get_monitors(self.root, true).ok().and_then(|c| c.reply().ok());
        if let Some(m) = monitors.as_ref().and_then(|r| r.monitors.iter().find(|m| m.primary).or(r.monitors.first())) {
            return screens::Rect::new(m.x as f64, m.y as f64, m.width as f64, m.height as f64);
        }
        let s = &self.conn.setup().roots[0];
        screens::Rect::new(0.0, 0.0, s.width_in_pixels as f64, s.height_in_pixels as f64)
    }

    /// Draws an image into a window, cuts the window to the image's shape,
    /// and moves it to `(x, y)`.
    fn show(&self, win: Window, img: &Image, x: i32, y: i32) -> Result<()> {
        self.conn.configure_window(win, &ConfigureWindowAux::new().x(x).y(y).width(img.w as u32).height(img.h as u32))?;
        let row = img.w * 4;
        let rows_per_chunk = (self.max_chunk / row).max(1);
        for (i, chunk) in img.bgra.chunks(row * rows_per_chunk).enumerate() {
            let rows = chunk.len() / row;
            let y0 = (i * rows_per_chunk) as i16;
            self.conn.put_image(ImageFormat::Z_PIXMAP, win, self.gc, img.w as u16, rows as u16, 0, y0, 0, 24, chunk)?;
        }
        let shape: Vec<Rectangle> = img
            .opaque_runs()
            .into_iter()
            .map(|(x, y, w)| Rectangle { x: x as i16, y: y as i16, width: w as u16, height: 1 })
            .collect();
        self.conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, ClipOrdering::YX_SORTED, win, 0, 0, &shape)?;
        Ok(())
    }

    fn move_to(&self, win: Window, x: i32, y: i32) -> Result<()> {
        self.conn.configure_window(win, &ConfigureWindowAux::new().x(x).y(y))?;
        Ok(())
    }

    fn raise(&self) -> Result<()> {
        for win in [self.possum, self.bubble] {
            self.conn.configure_window(win, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))?;
        }
        Ok(())
    }

    /// Handles pending X events: whether the displays changed, and which
    /// windows lost their contents (X doesn't keep what's drawn in a window
    /// that isn't on screen, so those need drawing again).
    fn events(&self) -> Result<XEvents> {
        let mut seen = XEvents::default();
        while let Some(event) = self.conn.poll_for_event()? {
            match event {
                Event::RandrScreenChangeNotify(_) | Event::RandrNotify(_) => seen.displays_changed = true,
                Event::Expose(e) if e.window == self.possum => seen.possum_exposed = true,
                Event::Expose(e) if e.window == self.bubble => seen.bubble_exposed = true,
                _ => {}
            }
        }
        Ok(seen)
    }

}

#[derive(Default)]
struct XEvents {
    displays_changed: bool,
    possum_exposed: bool,
    bubble_exposed: bool,
}

// ---------------------------------------------------------------------------
// Sound
// ---------------------------------------------------------------------------

struct Sounds {
    player: Option<&'static str>,
    files: [Vec<PathBuf>; 2],
    playing: Vec<Child>,
    /// The latest reading from the probe thread.
    probe: Arc<Mutex<quiet::Probe>>,
}

impl Sounds {
    fn new(voices: &[Vec<crate::scream::Scream>; 2], volume: f32) -> Result<Self> {
        // The runtime directory is private to the user and cleared at logout.
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("possum");
        std::fs::create_dir_all(&dir)?;
        let mut files: [Vec<PathBuf>; 2] = Default::default();
        for (k, kind) in ["aaaa", "hiss"].iter().enumerate() {
            for (i, s) in voices[k].iter().enumerate() {
                let path = dir.join(format!("{kind}-{i}.wav"));
                std::fs::write(&path, s.wav_at_volume(volume))?;
                files[k].push(path);
            }
        }
        let player = ["pw-play", "paplay", "aplay"].into_iter().find(|p| on_path(p));
        let probe = Arc::new(Mutex::new(quiet::Probe::default()));
        start_probe_thread(Arc::clone(&probe));
        Ok(Self { player, files, playing: Vec::new(), probe })
    }

    /// Forget players that have finished.
    fn reap(&mut self) {
        self.playing.retain_mut(|child| matches!(child.try_wait(), Ok(None)));
    }
}

impl Host for Sounds {
    fn play(&mut self, kind: Kind, variant: usize) -> bool {
        let Some(player) = self.player else { return false };
        let path = &self.files[brain::kind_index(kind)][variant];
        let mut cmd = Command::new(player);
        if player == "aplay" {
            cmd.arg("-q");
        }
        match cmd.arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(child) => {
                self.playing.push(child);
                true
            }
            Err(_) => false,
        }
    }

    fn hush(&mut self) {
        for mut child in self.playing.drain(..) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn probe(&mut self) -> quiet::Probe {
        *self.probe.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn has_audio(&self) -> bool {
        self.player.is_some()
    }
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Quiet times
// ---------------------------------------------------------------------------

/// Probing means running a few small programs, which can take tens of
/// milliseconds; doing it on a thread keeps the animation smooth.
fn start_probe_thread(latest: Arc<Mutex<quiet::Probe>>) {
    std::thread::spawn(move || {
        // DPMS needs its own connection on this thread.
        let x = x11rb::connect(None).ok();
        loop {
            let mut probe = probe_session();
            probe.screensaver = screensaver_active();
            if let Some((conn, _)) = &x {
                probe.display_asleep = conn
                    .dpms_info()
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .map(|i| i.state && i.power_level != x11rb::protocol::dpms::DPMSMode::ON);
            }
            *latest.lock().unwrap_or_else(|e| e.into_inner()) = probe;
            std::thread::sleep(Duration::from_millis(700));
        }
    });
}

/// Asks systemd-logind (via `loginctl`) whether this session is locked, and
/// whether it is the active one on its seat (it isn't after switching
/// users).
fn probe_session() -> quiet::Probe {
    let session = session_id();
    let out = Command::new("loginctl")
        .args(["show-session", &session, "-p", "LockedHint", "-p", "Active"])
        .stderr(Stdio::null())
        .output();
    let Some(out) = out.ok().filter(|o| o.status.success()) else { return quiet::Probe::default() };
    parse_loginctl(&String::from_utf8_lossy(&out.stdout))
}

/// Which logind session to ask about. Apps started from a terminal know it
/// (`XDG_SESSION_ID`); apps the desktop starts (autostart, launchers) often
/// run as systemd user services outside any session, so ask logind for the
/// user's graphical ("display") session instead.
fn session_id() -> String {
    if let Some(id) = std::env::var("XDG_SESSION_ID").ok().filter(|id| !id.is_empty()) {
        return id;
    }
    std::env::var("USER")
        .ok()
        .and_then(|user| {
            let out = Command::new("loginctl")
                .args(["show-user", &user, "-p", "Display", "--value"])
                .stderr(Stdio::null())
                .output()
                .ok()?;
            let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (out.status.success() && !id.is_empty()).then_some(id)
        })
        .unwrap_or_else(|| "auto".into())
}

/// Parses `loginctl show-session -p LockedHint -p Active` output.
fn parse_loginctl(text: &str) -> quiet::Probe {
    let mut probe = quiet::Probe::default();
    for line in text.lines() {
        match line.trim().split_once('=') {
            Some(("LockedHint", v)) => probe.screen_locked = Some(v == "yes"),
            Some(("Active", v)) => probe.on_console = Some(v == "yes"),
            _ => {}
        }
    }
    probe
}

/// Asks the desktop's screensaver over D-Bus whether it's active: the
/// freedesktop interface (KDE, Xfce...) or GNOME's own.
fn screensaver_active() -> Option<bool> {
    let services = [
        ("org.freedesktop.ScreenSaver", "/org/freedesktop/ScreenSaver"),
        ("org.gnome.ScreenSaver", "/org/gnome/ScreenSaver"),
    ];
    services.iter().find_map(|(dest, path)| {
        let method = format!("{dest}.GetActive");
        let out = Command::new("gdbus")
            .args(["call", "--session", "--dest", dest, "--object-path", path, "--method", &method])
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status.success().then(|| parse_gdbus_bool(&String::from_utf8_lossy(&out.stdout)))?
    })
}

/// Parses a gdbus reply like `(true,)`.
fn parse_gdbus_bool(text: &str) -> Option<bool> {
    match text.trim().trim_start_matches('(').trim_end_matches(')').trim_end_matches(',') {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tray icon
// ---------------------------------------------------------------------------

struct Tray {
    icon: ksni::Icon,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "possum".into()
    }

    fn title(&self) -> String {
        "Possum".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![self.icon.clone()]
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{RadioGroup, RadioItem, StandardItem};
        let item = |label: &str| StandardItem { label: label.into(), ..Default::default() };
        vec![
            StandardItem { enabled: false, ..item("Possum is running!") }.into(),
            ksni::MenuItem::Separator,
            RadioGroup {
                selected: SILENT.load(Ordering::Relaxed) as usize,
                select: Box::new(|_: &mut Self, i: usize| SILENT.store(i == 1, Ordering::Relaxed)),
                options: vec![
                    RadioItem { label: "Scream".into(), ..Default::default() },
                    RadioItem { label: "Scream silently".into(), ..Default::default() },
                ],
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                activate: Box::new(|_: &mut Self| SCREAM_NOW.store(true, Ordering::Relaxed)),
                ..item("Scream Now")
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem { activate: Box::new(|_: &mut Self| QUIT.store(true, Ordering::Relaxed)), ..item("Quit Possum") }
                .into(),
        ]
    }
}

/// Starts the tray icon, if the desktop supports one.
fn start_tray() -> Option<ksni::blocking::Handle<Tray>> {
    use ksni::blocking::TrayMethods;
    let icon = sprite::status_icon();
    let img = render::scaled(&icon, icon.w as usize * 2, icon.h as usize * 2);
    let tray = Tray { icon: ksni::Icon { width: img.w as i32, height: img.h as i32, data: img.argb_be() } };
    match tray.spawn() {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!(
                "possum: no tray icon ({e}). The tray needs a desktop session with \
                 StatusNotifierItem support (KDE; GNOME with the AppIndicator extension). \
                 Quit with Ctrl+C or `pkill possum`."
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// The possum on screen
// ---------------------------------------------------------------------------

struct App {
    x: X11,
    art: Art,
    sounds: Sounds,
    brain: Brain,
    /// The primary monitor, y-down root coordinates.
    screen: screens::Rect,
    scale: f64,
    reach: usize,
    possum_key: Option<(bool, usize, Option<usize>, u64)>,
    bubble_key: Option<(Kind, usize, usize)>,
}

impl App {
    fn tick(&mut self, now: f64) -> Result<()> {
        self.sounds.reap();
        let events = self.x.events()?;
        if events.displays_changed {
            self.check_screen();
        }
        if events.possum_exposed {
            self.possum_key = None;
        }
        if events.bubble_exposed {
            self.bubble_key = None;
        }
        let scene = self.brain.tick(now, &mut self.sounds);
        if scene.check_displays {
            self.check_screen();
            self.x.raise()?;
        }
        self.draw(&scene)?;
        self.x.conn.flush()?;
        Ok(())
    }

    fn check_screen(&mut self) {
        let primary = Some(self.x.primary_monitor());
        if let Some(frame) = screens::follow(primary, self.screen, self.screen) {
            self.screen = frame;
            self.brain.set_screen(frame.w, frame.h);
        }
    }

    fn draw(&mut self, scene: &Scene) -> Result<()> {
        // Brain coordinates are y-up within the monitor; X11's are y-down.
        let s = self.screen;
        let paws_x = s.x + scene.x;
        let paws_y = s.y + s.h - scene.y;
        let (x, y) = ((paws_x - self.reach as f64) as i32, (paws_y - self.reach as f64) as i32);
        let key = (scene.screaming, scene.frame, scene.yell, scene.heading.to_bits());
        if self.possum_key != Some(key) {
            self.possum_key = Some(key);
            let img = render::rotated(self.art.possum(scene), self.scale, scene.heading, self.reach);
            self.x.show(self.x.possum, &img, x, y)?;
        } else {
            self.x.move_to(self.x.possum, x, y)?;
        }

        match scene.bubble {
            Some(b) => {
                let (w, h) = (b.w.round() as usize, b.h.round() as usize);
                let (bx, by) = ((s.x + b.left) as i32, (s.y + s.h - b.bottom - h as f64) as i32);
                // Map before drawing: X throws away drawing done in an
                // unmapped window.
                if !self.x.bubble_mapped {
                    self.x.conn.map_window(self.x.bubble)?;
                    self.x.bubble_mapped = true;
                    self.bubble_key = None;
                }
                let key = (b.kind, b.shout, b.quadrant);
                if self.bubble_key != Some(key) {
                    self.bubble_key = Some(key);
                    let img = render::scaled(self.art.bubble(&b), w, h);
                    self.x.show(self.x.bubble, &img, bx, by)?;
                } else {
                    self.x.move_to(self.x.bubble, bx, by)?;
                }
            }
            None if self.x.bubble_mapped => {
                self.x.conn.unmap_window(self.x.bubble)?;
                self.x.bubble_mapped = false;
            }
            None => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_loginctl_output() {
        let p = parse_loginctl("LockedHint=yes\nActive=yes\n");
        assert_eq!((p.screen_locked, p.on_console), (Some(true), Some(true)));
        let p = parse_loginctl("LockedHint=no\nActive=no\n");
        assert_eq!((p.screen_locked, p.on_console), (Some(false), Some(false)));
        assert_eq!(parse_loginctl(""), quiet::Probe::default());
    }

    #[test]
    fn reads_gdbus_booleans() {
        assert_eq!(parse_gdbus_bool("(true,)\n"), Some(true));
        assert_eq!(parse_gdbus_bool("(false,)"), Some(false));
        assert_eq!(parse_gdbus_bool("Error: no such name"), None);
    }
}
