//! Linux: X11 (directly, or through XWayland on Wayland desktops).
//!
//! * Two small override-redirect windows (the possum and its speech bubble),
//!   drawn with the software renderer. The pixel art is either fully opaque
//!   or fully transparent, so instead of alpha blending each window is *cut
//!   to the shape* of its picture (the X11 Shape extension, as xeyes and
//!   oneko do). That needs no compositor, and it avoids 32-bit alpha
//!   windows, which crash Xwayland 24.1 (and with it GNOME Shell) when it
//!   runs without GPU buffer sharing, as in virtual machines. On Xwayland,
//!   the windows only appear once the desktop's compositor has taken over
//!   X11 (see `wait_for_xwayland_compositor`). An empty input
//!   shape makes the windows click-through. Override-redirect windows aren't
//!   managed by the window manager, so they sit on top of everything,
//!   including panels and docks.
//! * Sounds are written once to the user's runtime directory and played with
//!   whichever player the system has: `pw-play` (PipeWire), `paplay`
//!   (PulseAudio) or `aplay` (ALSA).
//! * Quiet times: a background thread asks systemd-logind whether the
//!   session is locked or inactive, the desktop whether its screensaver is
//!   active (over D-Bus), and X11's DPMS whether the display is off. It
//!   also asks the desktop whether Do Not Disturb is on: then the possum
//!   screams silently.
//! * The menu is a StatusNotifierItem tray icon (KDE, most desktops; GNOME
//!   with the AppIndicator extension, which Ubuntu ships enabled).
//! * Its size: RandR says how many millimetres the primary monitor measures
//!   (on Wayland desktops, as the compositor told Xwayland), which makes the
//!   possum about 2 cm tall (see size.rs).

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::dpms::ConnectionExt as _;
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    AtomEnum, ClipOrdering, ConfigureWindowAux, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask,
    ImageFormat, ImageOrder, PropMode, Rectangle, StackMode, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;

use crate::brain::{self, Brain, Host, QUIT, SCREAM_NOW, SILENT, STOPPED, Scene, Settings};
use crate::quiet::Hush;
use crate::render::{self, Art, Image};
use crate::scream::{Kind, Rng};
use crate::size::{self, Size};
use crate::{Options, quiet, screens, sprite};

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
    wait_for_xwayland_compositor(&conn, screen_num, opts.verbose)?;
    let x = X11::new(conn, screen_num)?;

    let seed = super::seed();
    let mut rng = Rng::new(seed);
    let voices = brain::voices(seed, &mut rng);
    let durations = voices.each_ref().map(|v| v.iter().map(|s| s.seconds).collect());
    let mut sounds = Sounds::new(&voices, opts.volume)?;

    let (screen, display) = x.primary_monitor();
    let size = Size::pick(opts.scale, opts.speed, display, 1.0);
    let settings = Settings { scale: size.scale, speed: size.speed, start: opts.start, verbose: opts.verbose };
    let mut brain = Brain::new(&settings, screen.w, screen.h, durations, rng);
    brain.update(sounds.probe());
    brain.log(format_args!("running around the primary display, {}x{}", screen.w, screen.h));
    brain.log(format_args!("{}", size.describe(display, opts.scale.is_some())));
    if sounds.player.is_none() {
        eprintln!("possum: no audio player found (pw-play, paplay or aplay), so it screams silently");
    }

    let tray = start_tray();
    let art = Art::new();
    let reach = render::reach(&art, size.scale);
    let mut app = App {
        x,
        art,
        sounds,
        brain,
        screen,
        options: (opts.scale, opts.speed),
        scale: size.scale,
        reach,
        possum_key: None,
        bubble_key: None,
    };

    let clock = Instant::now();
    let frame = Duration::from_secs_f64(1.0 / 60.0);
    let mut next = Instant::now();
    let menu_state = || (brain::silent_now(), STOPPED.load(Ordering::Relaxed));
    let mut menu_shows = menu_state();
    while !QUIT.load(Ordering::Relaxed) {
        app.tick(clock.elapsed().as_secs_f64())?;
        // The menu shows whether it screams silently (which Do Not Disturb
        // changes too) and whether it's stopped.
        if menu_state() != menu_shows {
            menu_shows = menu_state();
            if let Some(tray) = &tray {
                tray.update(|_| {});
            }
        }
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

/// On Xwayland, waits (a few seconds at most) until the desktop's compositor
/// has taken over X11, before Possum shows any window.
///
/// Xwayland 24.1 (Ubuntu 26.04's, for one) gives a top-level window its
/// Wayland surface only if the compositor has already redirected it when the
/// window is shown. A window shown earlier never gets one, and the first
/// time it moves, Xwayland follows the missing surface and crashes
/// (`damage_report` in `xwayland-window.c`; later Xwayland releases
/// restructured that code). GNOME starts Xwayland only when the first X11
/// app connects, so when that app is Possum (started from the app list or at
/// login), its first requests arrive a few milliseconds before GNOME has
/// redirected windows.
///
/// GNOME claims the compositor selection (`_NET_WM_CM_S<n>`) before it
/// redirects windows, so that's not enough. Right after redirecting, it sets
/// the RandR primary output (mutter's `on_x11_display_setup`), and one
/// client's requests are handled in order, so a primary output proves the
/// redirect is done. Plain X11 servers (no `XWAYLAND` extension) and an
/// Xwayland whose compositor is already set up don't wait at all; a
/// compositor that never sets a primary output costs a few seconds once.
fn wait_for_xwayland_compositor(conn: &RustConnection, screen_num: usize, verbose: bool) -> Result<()> {
    const PATIENCE: Duration = Duration::from_secs(4);
    if conn.extension_information("XWAYLAND")?.is_none() {
        return Ok(());
    }
    let root = conn.setup().roots[screen_num].root;
    let name = format!("_NET_WM_CM_S{screen_num}");
    let selection = conn.intern_atom(false, name.as_bytes())?.reply()?.atom;
    let start = Instant::now();
    let mut waited = false;
    loop {
        let compositor = conn.get_selection_owner(selection)?.reply()?.owner != x11rb::NONE;
        let primary = conn.randr_get_output_primary(root)?.reply()?.output != x11rb::NONE;
        if (compositor && primary) || start.elapsed() >= PATIENCE {
            if verbose && waited {
                let what = if compositor && primary { "ready" } else { "not ready, going ahead" };
                eprintln!("possum: Xwayland's compositor {what} after {:.3}s", start.elapsed().as_secs_f64());
            }
            return Ok(());
        }
        waited = true;
        std::thread::sleep(Duration::from_millis(5));
    }
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
            conn.change_property(
                PropMode::REPLACE,
                win,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                8,
                name.len() as u32,
                name.as_bytes(),
            )?;
            let class = b"possum\0Possum\0";
            conn.change_property(
                PropMode::REPLACE,
                win,
                AtomEnum::WM_CLASS,
                AtomEnum::STRING,
                8,
                class.len() as u32,
                class,
            )?;
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

    /// The primary monitor in root-window coordinates (y-down), and how big
    /// it is, if it says.
    fn primary_monitor(&self) -> (screens::Rect, Option<size::Display>) {
        let monitors = self.conn.randr_get_monitors(self.root, true).ok().and_then(|c| c.reply().ok());
        if let Some(m) = monitors.as_ref().and_then(|r| r.monitors.iter().find(|m| m.primary).or(r.monitors.first())) {
            let (w, h) = (m.width as f64, m.height as f64);
            let mm = (m.width_in_millimeters as f64, m.height_in_millimeters as f64);
            return (screens::Rect::new(m.x as f64, m.y as f64, w, h), size::Display::new((w, h), mm, 1.0));
        }
        // No RandR: the core protocol's millimetres are made up from 96 dpi.
        let s = &self.conn.setup().roots[0];
        (screens::Rect::new(0.0, 0.0, s.width_in_pixels as f64, s.height_in_pixels as f64), None)
    }

    /// Draws an image into a window, cuts the window to the image's shape,
    /// and moves it to `(x, y)`.
    fn show(&self, win: Window, img: &Image, x: i32, y: i32) -> Result<()> {
        self.conn
            .configure_window(win, &ConfigureWindowAux::new().x(x).y(y).width(img.w as u32).height(img.h as u32))?;
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
        let dir =
            std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("possum");
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
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        loop {
            let mut probe = probe_session();
            probe.screensaver = screensaver_active();
            probe.hush = (do_not_disturb(&desktop) == Some(true)).then_some(Hush::DoNotDisturb);
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
    if let Ok(id) = std::env::var("XDG_SESSION_ID")
        && !id.is_empty()
    {
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

/// Parses a gdbus reply like `(true,)`, or `(<true>,)` for a property.
fn parse_gdbus_bool(text: &str) -> Option<bool> {
    let value = text.trim().trim_prefix('(').trim_suffix(')').trim_suffix(',');
    parse_setting(value.trim_prefix('<').trim_suffix('>'))
}

/// Parses a boolean setting, as gsettings and xfconf-query print them.
fn parse_setting(text: &str) -> Option<bool> {
    match text.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Whether the desktop's Do Not Disturb is on. GNOME (and Ubuntu's),
/// Cinnamon, MATE and Xfce keep it in their settings; KDE Plasma (and other
/// notification servers) say so through the freedesktop notification
/// server's `Inhibited` property. None when it can't tell.
fn do_not_disturb(desktop: &str) -> Option<bool> {
    let read = |program: &str, args: &[&str]| -> Option<bool> {
        let out = Command::new(program).args(args).stderr(Stdio::null()).output().ok()?;
        out.status.success().then(|| parse_setting(&String::from_utf8_lossy(&out.stdout)))?
    };
    let on = |names: &[&str]| desktop.split(':').any(|d| names.iter().any(|n| d.eq_ignore_ascii_case(n)));
    let from_settings = if on(&["GNOME", "Unity"]) {
        // Its Do Not Disturb switch turns the banners off.
        read("gsettings", &["get", "org.gnome.desktop.notifications", "show-banners"]).map(|banners| !banners)
    } else if on(&["X-Cinnamon", "Cinnamon"]) {
        read("gsettings", &["get", "org.cinnamon.desktop.notifications", "display-notifications"]).map(|shown| !shown)
    } else if on(&["MATE"]) {
        read("gsettings", &["get", "org.mate.NotificationDaemon", "do-not-disturb"])
    } else if on(&["XFCE"]) {
        read("xfconf-query", &["-c", "xfce4-notifyd", "-p", "/do-not-disturb"])
    } else {
        None
    };
    from_settings.or_else(notifications_inhibited)
}

/// The freedesktop notification server's `Inhibited` property, if one is
/// running. (Asking a server that isn't running would start one.)
fn notifications_inhibited() -> Option<bool> {
    let gdbus = |dest: &str, path: &str, method: &str, args: &[&str]| -> Option<bool> {
        let out = Command::new("gdbus")
            .args(["call", "--session", "--dest", dest, "--object-path", path, "--method", method])
            .args(args)
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status.success().then(|| parse_gdbus_bool(&String::from_utf8_lossy(&out.stdout)))?
    };
    let server = "org.freedesktop.Notifications";
    let bus = ("org.freedesktop.DBus", "/org/freedesktop/DBus");
    gdbus(bus.0, bus.1, "org.freedesktop.DBus.NameHasOwner", &[server])?.then_some(())?;
    gdbus(server, "/org/freedesktop/Notifications", "org.freedesktop.DBus.Properties.Get", &[server, "Inhibited"])
}

/// Says why "Scream" stays silent for now: a notification marked critical,
/// the kind desktops still show in Do Not Disturb (it's the answer to a
/// click in the menu). Critical ones stay until closed, so a new one
/// replaces the last, and none is kept in the notification list.
fn tell_why_silent(hush: Hush) {
    static LAST: AtomicU32 = AtomicU32::new(0);
    let (headline, rest) = hush.respect("computer");
    std::thread::spawn(move || {
        let replaces = LAST.load(Ordering::Relaxed).to_string();
        let hints = "{'urgency': <byte 2>, 'transient': <true>}";
        let out = Command::new("gdbus")
            .args(["call", "--session", "--dest", "org.freedesktop.Notifications"])
            .args(["--object-path", "/org/freedesktop/Notifications"])
            .args(["--method", "org.freedesktop.Notifications.Notify"])
            .args(["Possum", &replaces, "", &headline, rest, "[]", hints, "10000"])
            .stderr(Stdio::null())
            .output();
        match out.ok().filter(|o| o.status.success()) {
            Some(o) => LAST.store(parse_gdbus_u32(&String::from_utf8_lossy(&o.stdout)).unwrap_or(0), Ordering::Relaxed),
            None => eprintln!("possum: {headline}. {rest}"),
        }
    });
}

/// Parses a gdbus reply like `(uint32 7,)`.
fn parse_gdbus_u32(text: &str) -> Option<u32> {
    text.trim().trim_prefix('(').trim_suffix(')').trim_suffix(',').trim_prefix("uint32 ").parse().ok()
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
        let stopped = STOPPED.load(Ordering::Relaxed);
        let status = if stopped { "Possum is standing still" } else { "Possum is running!" };
        vec![
            StandardItem { enabled: false, ..item(status) }.into(),
            ksni::MenuItem::Separator,
            RadioGroup {
                // In Do Not Disturb it screams silently, whatever was picked.
                selected: brain::silent_now() as usize,
                select: Box::new(|_: &mut Self, i: usize| {
                    SILENT.store(i == 1, Ordering::Relaxed);
                    if let Some(hush) = brain::hush().filter(|_| i == 0) {
                        tell_why_silent(hush);
                    }
                }),
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
            // ksni builds the menu again after a click, with the other label.
            StandardItem {
                activate: Box::new(|_: &mut Self| {
                    STOPPED.fetch_xor(true, Ordering::Relaxed);
                }),
                ..item(if stopped { "Run" } else { "Stop" })
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                activate: Box::new(|_: &mut Self| QUIT.store(true, Ordering::Relaxed)),
                ..item("Quit Possum")
            }
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
    /// `--scale` and `--speed`, if given.
    options: (Option<f64>, Option<f64>),
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

    /// Follows the primary monitor, and keeps the possum about 2 cm tall on
    /// it.
    fn check_screen(&mut self) {
        let (primary, display) = self.x.primary_monitor();
        let size = Size::pick(self.options.0, self.options.1, display, 1.0);
        let moved = screens::follow(Some(primary), self.screen, self.screen).is_some();
        if !moved && size.scale == self.scale {
            return;
        }
        self.screen = primary;
        self.brain.set_screen(primary.w, primary.h, size.scale, size.speed);
        if size.scale != self.scale {
            self.brain.log(format_args!("{}", size.describe(display, self.options.0.is_some())));
            self.scale = size.scale;
            self.reach = render::reach(&self.art, size.scale);
            self.possum_key = None;
            self.bubble_key = None;
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
        // A property comes as a variant.
        assert_eq!(parse_gdbus_bool("(<true>,)\n"), Some(true));
        assert_eq!(parse_gdbus_bool("(<false>,)"), Some(false));
        // A notification's number.
        assert_eq!(parse_gdbus_u32("(uint32 7,)\n"), Some(7));
        assert_eq!(parse_gdbus_u32("Error"), None);
        // gsettings and xfconf-query.
        assert_eq!(parse_setting("false\n"), Some(false));
        assert_eq!(parse_setting("Property \"/do-not-disturb\" does not exist"), None);
    }
}
