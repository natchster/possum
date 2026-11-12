//! Windows: Win32.
//!
//! * Two layered popup windows (the possum and its speech bubble) that are
//!   always on top, transparent per pixel, click-through, and never take the
//!   focus. They're drawn with the software renderer and shown with
//!   `UpdateLayeredWindow`.
//! * A notification-area (tray) icon with the menu. It comes back by itself
//!   when Explorer restarts, and appears late rather than never when
//!   Possum starts before the taskbar does.
//! * Sounds are played from memory with `PlaySound`.
//! * Quiet times: session notifications (lock/unlock, switching users),
//!   display power notifications (on/off), system sleep, plus a check of
//!   the screensaver and the secure (lock) desktop every second.
//! * Per-monitor DPI aware: the possum is the same physical size as on a
//!   Mac at 100% scaling, bigger at 125%, 150%...

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW,
    MonitorFromPoint, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    BLENDFUNCTION, DIB_RGB_COLORS, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows_sys::Win32::System::Console::{
    AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Power::{RegisterPowerSettingNotification, POWERBROADCAST_SETTING};
use windows_sys::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop, DESKTOP_SWITCHDESKTOP};
use windows_sys::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, ChangeWindowMessageFilterEx, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, PostMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow,
    SystemParametersInfoW, TrackPopupMenu, TranslateMessage, DEVICE_NOTIFY_WINDOW_HANDLE, HICON, HWND_TOPMOST,
    ICONINFO, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, MSGFLT_ALLOW,
    PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, PBT_POWERSETTINGCHANGE, SPI_GETSCREENSAVERRUNNING,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, ULW_ALPHA, WM_APP, WM_CONTEXTMENU, WM_DISPLAYCHANGE, WM_LBUTTONUP, WM_NULL, WM_POWERBROADCAST,
    WM_RBUTTONUP, WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WTS_CONSOLE_CONNECT, WTS_CONSOLE_DISCONNECT, WTS_REMOTE_CONNECT,
    WTS_REMOTE_DISCONNECT, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK, UpdateLayeredWindow,
};

use crate::brain::{self, Brain, Host, Scene, Settings, SCREAM_NOW, SILENT};
use crate::render::{self, Art, Image};
use crate::scream::{Kind, Rng};
use crate::{quiet, screens, sprite, Options};

/// The tray icon's messages come back as this.
const WM_TRAY: u32 = WM_APP + 1;
const TIMER_ID: usize = 1;

const MENU_SCREAM: usize = 1;
const MENU_SCREAM_SILENTLY: usize = 2;
const MENU_SCREAM_NOW: usize = 3;
const MENU_QUIT: usize = 4;

/// What Windows told us since the last tick (see quiet.rs), and whether the
/// displays changed.
static SYSTEM_EVENTS: Mutex<Vec<quiet::Event>> = Mutex::new(Vec::new());
static DISPLAYS_CHANGED: AtomicBool = AtomicBool::new(false);

/// The "TaskbarCreated" message: Explorer broadcasts it when it creates the
/// taskbar anew (after it restarted or crashed, and on some display
/// changes). The notification area starts out empty then.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A GUI app has no console of its own. When started from a terminal,
/// borrow that one, so --help and --verbose print there.
pub fn prepare_console() {
    // SAFETY: plain Win32 calls with valid arguments.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        // Keep handles that were redirected (to a file, a pipe); point the
        // rest at the console.
        let console = wide("CONOUT$");
        for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let current = GetStdHandle(which);
            if current.is_null() || current as isize == -1 {
                let handle = CreateFileW(
                    console.as_ptr(),
                    0x4000_0000, // GENERIC_WRITE
                    FILE_SHARE_WRITE,
                    null(),
                    OPEN_EXISTING,
                    0,
                    null_mut(),
                );
                SetStdHandle(which, handle);
            }
        }
    }
}

pub fn run(opts: &Options) {
    // SAFETY: called once, before any window exists.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    // SAFETY: registering (or looking up) a message by its name.
    TASKBAR_CREATED.store(unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) }, Ordering::Relaxed);

    let seed = super::seed();
    let mut rng = Rng::new(seed);
    let voices = brain::voices(seed, &mut rng);
    let durations = voices.each_ref().map(|v| v.iter().map(|s| s.seconds).collect());
    // PlaySound has no volume control: bake the volume into the samples.
    let mut sounds = Sounds(voices.map(|v| v.iter().map(|s| s.wav_at_volume(opts.volume)).collect()));

    let (screen, dpi_scale) = primary_monitor();
    // Windows works in physical pixels: scale sizes and speed like the
    // user's display scaling (100% = 96 DPI).
    let scale = opts.scale * dpi_scale;
    let settings = Settings { scale, speed: opts.speed * dpi_scale, start: opts.start, verbose: opts.verbose };
    let mut brain = Brain::new(&settings, screen.w, screen.h, durations, rng);
    brain.update(sounds.probe());
    brain.log(format_args!("running around the primary display, {}x{} pixels", screen.w, screen.h));

    // SAFETY: standard window-class registration and window creation.
    let (main, possum, bubble) = unsafe {
        let instance = GetModuleHandleW(null());
        let class = wide("PossumWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let title = wide("Possum");
        // A hidden window that receives the timer, tray, session and power
        // messages. (Message-only windows don't get broadcasts.)
        let main = CreateWindowExW(
            WS_EX_TOOLWINDOW, class.as_ptr(), title.as_ptr(), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), instance,
            null(),
        );
        let layered = || {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                instance,
                null(),
            )
        };
        (main, layered(), layered())
    };

    // SAFETY: `main` is a valid window owned by this thread.
    unsafe {
        WTSRegisterSessionNotification(main, NOTIFY_FOR_THIS_SESSION);
        RegisterPowerSettingNotification(main, &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);
        SetTimer(main, TIMER_ID, 15, None);
        // Let "TaskbarCreated" through even when Possum runs as
        // administrator (Explorer itself doesn't).
        let taskbar_created = TASKBAR_CREATED.load(Ordering::Relaxed);
        if taskbar_created != 0 {
            ChangeWindowMessageFilterEx(main, taskbar_created, MSGFLT_ALLOW, null_mut());
        }
    }
    TRAY.with(|tray| *tray.borrow_mut() = Some(Tray::new(main, opts.verbose)));

    let art = Art::new();
    let reach = render::reach(&art, scale);
    APP.with(|app| {
        *app.borrow_mut() = Some(App {
            art,
            sounds,
            brain,
            screen,
            scale,
            reach,
            clock: Instant::now(),
            possum: Layered::new(possum),
            bubble: Layered::new(bubble),
            possum_key: None,
            bubble_key: None,
        })
    });

    // SAFETY: the standard message loop.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    // Tidy up: silence, and take the icon out of the notification area.
    APP.with(|app| {
        if let Some(app) = app.borrow_mut().as_mut() {
            app.sounds.hush();
        }
    });
    TRAY.with(|tray| tray.borrow_mut().take());
}

/// The primary monitor's frame in virtual-screen pixels (y-down), and its
/// display scaling (1.0 at 100%).
fn primary_monitor() -> (screens::Rect, f64) {
    // SAFETY: plain Win32 queries with valid out-pointers.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        let r = info.rcMonitor;
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let rect = screens::Rect::new(r.left as f64, r.top as f64, (r.right - r.left) as f64, (r.bottom - r.top) as f64);
        (rect, dpi_x.max(96) as f64 / 96.0)
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            APP.with(|app| {
                // A menu's modal loop can deliver timer ticks while the menu
                // code runs; it never holds the app, so this can't clash.
                if let Ok(mut app) = app.try_borrow_mut() {
                    if let Some(app) = app.as_mut() {
                        app.tick();
                    }
                }
            });
            with_tray(Tray::retry);
            0
        }
        WM_TRAY => {
            let event = (lparam & 0xFFFF) as u32;
            if matches!(event, WM_RBUTTONUP | WM_LBUTTONUP | WM_CONTEXTMENU) {
                show_menu(hwnd);
            }
            0
        }
        WM_WTSSESSION_CHANGE => {
            let event = match wparam as u32 {
                WTS_SESSION_LOCK => Some(quiet::Event::ScreenLocked),
                WTS_SESSION_UNLOCK => Some(quiet::Event::ScreenUnlocked),
                WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => Some(quiet::Event::SessionResigned),
                WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => Some(quiet::Event::SessionActivated),
                _ => None,
            };
            if let Some(event) = event {
                SYSTEM_EVENTS.lock().unwrap_or_else(|e| e.into_inner()).push(event);
            }
            0
        }
        WM_POWERBROADCAST => {
            let event = match wparam as u32 {
                PBT_APMSUSPEND => Some(quiet::Event::SystemWillSleep),
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => Some(quiet::Event::SystemDidWake),
                PBT_POWERSETTINGCHANGE => {
                    // SAFETY: for this message, lparam points at the setting.
                    let setting = &*(lparam as *const POWERBROADCAST_SETTING);
                    let display = |guid: &windows_sys::core::GUID| {
                        (guid.data1, guid.data2, guid.data3, guid.data4)
                            == (
                                GUID_CONSOLE_DISPLAY_STATE.data1,
                                GUID_CONSOLE_DISPLAY_STATE.data2,
                                GUID_CONSOLE_DISPLAY_STATE.data3,
                                GUID_CONSOLE_DISPLAY_STATE.data4,
                            )
                    };
                    if display(&setting.PowerSetting) {
                        // 0 = off, 1 = on, 2 = dimmed (still on).
                        match setting.Data[0] {
                            0 => Some(quiet::Event::DisplaysSlept),
                            1 => Some(quiet::Event::DisplaysWoke),
                            _ => None,
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(event) = event {
                SYSTEM_EVENTS.lock().unwrap_or_else(|e| e.into_inner()).push(event);
            }
            1
        }
        WM_DISPLAYCHANGE => {
            DISPLAYS_CHANGED.store(true, Ordering::Relaxed);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            with_tray(Tray::taskbar_created);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ---------------------------------------------------------------------------
// Tray icon and menu
// ---------------------------------------------------------------------------

fn tray_data(main: HWND) -> NOTIFYICONDATAW {
    // SAFETY: NOTIFYICONDATAW is plain data; all-zero is a valid start.
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = main;
    data.uID = 1;
    data
}

/// The notification-area icon. Explorer forgets every icon when it restarts
/// (after a crash, an update, or a restart from Task Manager) and then says
/// so with "TaskbarCreated"; Possum then adds its icon again. Adding can
/// also fail, for example when Possum starts at login before the taskbar
/// exists: Possum then tries again every couple of seconds.
struct Tray {
    main: HWND,
    icon: HICON,
    shown: bool,
    retry_at: Instant,
    verbose: bool,
}

impl Tray {
    const RETRY: Duration = Duration::from_secs(2);

    fn new(main: HWND, verbose: bool) -> Self {
        let icon = icon_from(&sprite::status_icon(), 32);
        let mut tray = Self { main, icon, shown: false, retry_at: Instant::now(), verbose };
        if !tray.show() && verbose {
            eprintln!("possum: couldn't add the tray icon (no taskbar yet?); trying again");
        }
        tray
    }

    /// Adds the icon, or refreshes it if the notification area still has
    /// it. Returns whether it's there now.
    fn show(&mut self) -> bool {
        let mut data = tray_data(self.main);
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = self.icon;
        let tip = wide("Possum is screaming");
        data.szTip[..tip.len()].copy_from_slice(&tip);
        // SAFETY: `data` is fully initialised for NIM_ADD and NIM_MODIFY.
        let shown = unsafe { Shell_NotifyIconW(NIM_ADD, &data) != 0 || Shell_NotifyIconW(NIM_MODIFY, &data) != 0 };
        if self.verbose && shown != self.shown {
            if shown {
                eprintln!("possum: the tray icon is in the notification area");
            } else {
                eprintln!("possum: couldn't add the tray icon (no taskbar yet?); trying again");
            }
        }
        if !shown {
            self.retry_at = Instant::now() + Self::RETRY;
        }
        self.shown = shown;
        shown
    }

    /// Explorer made a new taskbar, without the icon.
    fn taskbar_created(&mut self) {
        if self.verbose {
            eprintln!("possum: the taskbar was created anew; adding the tray icon again");
        }
        self.shown = false;
        self.show();
    }

    /// On every timer tick: try again if adding the icon failed.
    fn retry(&mut self) {
        if !self.shown && Instant::now() >= self.retry_at {
            self.show();
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let data = tray_data(self.main);
        // SAFETY: removes the icon added above; the HICON is ours to destroy.
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &data);
            DestroyIcon(self.icon);
        }
    }
}

/// Runs `f` on the tray icon, unless it's busy (a menu's modal loop can
/// deliver messages while tray code runs).
fn with_tray(f: impl FnOnce(&mut Tray)) {
    TRAY.with(|tray| {
        if let Ok(mut tray) = tray.try_borrow_mut() {
            if let Some(tray) = tray.as_mut() {
                f(tray);
            }
        }
    });
}

/// An icon from a sprite canvas, `size` pixels square, with real alpha.
fn icon_from(canvas: &sprite::Canvas, size: usize) -> HICON {
    // Keep the aspect ratio: fit the canvas into the square, centred.
    let k = size as f64 / canvas.w.max(canvas.h) as f64;
    let (w, h) = ((canvas.w as f64 * k) as usize, (canvas.h as f64 * k) as usize);
    let img = render::scaled(canvas, w, h);
    let mut square = vec![0u8; size * size * 4];
    let (ox, oy) = ((size - w) / 2, (size - h) / 2);
    for y in 0..h {
        let src = &img.bgra[y * w * 4..(y + 1) * w * 4];
        let start = ((oy + y) * size + ox) * 4;
        square[start..start + w * 4].copy_from_slice(src);
    }
    // SAFETY: standard DIB + mask icon creation; the bitmaps are freed after
    // the icon copies them.
    unsafe {
        let (color, bits) = dib_section(size, size);
        std::ptr::copy_nonoverlapping(square.as_ptr(), bits, square.len());
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, null());
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}

fn show_menu(main: HWND) {
    let silent = SILENT.load(Ordering::Relaxed);
    // SAFETY: a standard popup menu, destroyed before returning. Nothing
    // here touches the app state, so timer ticks during the menu's modal
    // loop are fine.
    unsafe {
        let menu = CreatePopupMenu();
        let add = |flags: u32, id: usize, text: &str| {
            let text = wide(text);
            AppendMenuW(menu, flags, id, text.as_ptr());
        };
        add(MF_STRING | MF_GRAYED, 0, "Possum is running!");
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        add(MF_STRING | if silent { 0 } else { MF_CHECKED }, MENU_SCREAM, "Scream");
        add(MF_STRING | if silent { MF_CHECKED } else { 0 }, MENU_SCREAM_SILENTLY, "Scream silently");
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        add(MF_STRING, MENU_SCREAM_NOW, "Scream Now");
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        add(MF_STRING, MENU_QUIT, "Quit Possum");

        let mut at = POINT { x: 0, y: 0 };
        GetCursorPos(&mut at);
        // Without this, the menu wouldn't close when clicking elsewhere.
        SetForegroundWindow(main);
        let choice = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, at.x, at.y, 0, main, null());
        PostMessageW(main, WM_NULL, 0, 0);
        DestroyMenu(menu);
        match choice as usize {
            MENU_SCREAM => SILENT.store(false, Ordering::Relaxed),
            MENU_SCREAM_SILENTLY => SILENT.store(true, Ordering::Relaxed),
            MENU_SCREAM_NOW => SCREAM_NOW.store(true, Ordering::Relaxed),
            MENU_QUIT => PostQuitMessage(0),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// A top-down 32-bit DIB section and a pointer to its pixels.
unsafe fn dib_section(w: usize, h: usize) -> (windows_sys::Win32::Graphics::Gdi::HBITMAP, *mut u8) {
    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w as i32,
        biHeight: -(h as i32), // negative: rows top-down, like our images
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..std::mem::zeroed()
    };
    let mut bits: *mut c_void = null_mut();
    let bitmap = CreateDIBSection(null_mut(), &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    (bitmap, bits as *mut u8)
}

/// One of the layered windows.
struct Layered {
    hwnd: HWND,
    visible: bool,
}

impl Layered {
    fn new(hwnd: HWND) -> Self {
        Self { hwnd, visible: false }
    }

    /// Shows `img` with its top-left corner at `(x, y)`.
    fn show(&mut self, img: &Image, x: i32, y: i32) {
        // SAFETY: standard layered-window update; every GDI object created
        // here is released before returning.
        unsafe {
            let screen = GetDC(null_mut());
            let memory = CreateCompatibleDC(screen);
            let (bitmap, bits) = dib_section(img.w, img.h);
            std::ptr::copy_nonoverlapping(img.bgra.as_ptr(), bits, img.bgra.len());
            let old = SelectObject(memory, bitmap);
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let at = POINT { x, y };
            let size = SIZE { cx: img.w as i32, cy: img.h as i32 };
            let origin = POINT { x: 0, y: 0 };
            UpdateLayeredWindow(self.hwnd, screen, &at, &size, memory, &origin, 0, &blend, ULW_ALPHA);
            SelectObject(memory, old);
            DeleteObject(bitmap);
            DeleteDC(memory);
            ReleaseDC(null_mut(), screen);
            if !self.visible {
                ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                self.visible = true;
            }
        }
    }

    fn move_to(&self, x: i32, y: i32) {
        // SAFETY: moves our own window without activating it.
        unsafe { SetWindowPos(self.hwnd, null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
    }

    fn hide(&mut self) {
        if self.visible {
            // SAFETY: hides our own window.
            unsafe { ShowWindow(self.hwnd, SW_HIDE) };
            self.visible = false;
        }
    }

    /// Back on top of other always-on-top windows (the taskbar re-asserts
    /// itself when clicked).
    fn raise(&self) {
        // SAFETY: re-orders our own window without moving or activating it.
        unsafe { SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    }
}

// ---------------------------------------------------------------------------
// Sound and quiet times
// ---------------------------------------------------------------------------

/// The screams as in-memory WAVs for `PlaySound`.
struct Sounds([Vec<Vec<u8>>; 2]);

impl Host for Sounds {
    fn play(&mut self, kind: Kind, variant: usize) -> bool {
        let wav = &self.0[brain::kind_index(kind)][variant];
        // SAFETY: with SND_MEMORY the "name" is the WAV data, which lives as
        // long as the app (PlaySound reads it while playing).
        unsafe { PlaySoundW(wav.as_ptr().cast(), null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT) != 0 }
    }

    fn hush(&mut self) {
        // SAFETY: a null sound stops whatever is playing.
        unsafe { PlaySoundW(null(), null_mut(), 0) };
    }

    fn probe(&mut self) -> quiet::Probe {
        // SAFETY: plain Win32 queries with valid out-pointers.
        unsafe {
            let mut running: i32 = 0;
            SystemParametersInfoW(SPI_GETSCREENSAVERRUNNING, 0, (&mut running as *mut i32).cast(), 0);
            // While locked, the input desktop is the secure (Winlogon)
            // desktop, which a user app can't open.
            let desktop = OpenInputDesktop(0, 0, DESKTOP_SWITCHDESKTOP);
            let locked = desktop.is_null();
            if !locked {
                CloseDesktop(desktop);
            }
            quiet::Probe {
                screen_locked: Some(locked),
                screensaver: Some(running != 0),
                // Display power and session switches come as messages.
                display_asleep: None,
                on_console: None,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The possum on screen
// ---------------------------------------------------------------------------

struct App {
    art: Art,
    sounds: Sounds,
    brain: Brain,
    /// The primary monitor, y-down virtual-screen pixels.
    screen: screens::Rect,
    scale: f64,
    reach: usize,
    clock: Instant,
    possum: Layered,
    bubble: Layered,
    possum_key: Option<(bool, usize, Option<usize>, u64)>,
    bubble_key: Option<(Kind, usize, usize)>,
}

impl App {
    fn tick(&mut self) {
        let now = self.clock.elapsed().as_secs_f64();
        let events = std::mem::take(&mut *SYSTEM_EVENTS.lock().unwrap_or_else(|e| e.into_inner()));
        for event in events {
            self.brain.apply(event);
        }
        if DISPLAYS_CHANGED.swap(false, Ordering::Relaxed) {
            self.check_screen();
        }
        let scene = self.brain.tick(now, &mut self.sounds);
        if scene.check_displays {
            self.check_screen();
            self.possum.raise();
            self.bubble.raise();
        }
        self.draw(&scene);
    }

    fn check_screen(&mut self) {
        let (primary, _) = primary_monitor();
        if let Some(frame) = screens::follow(Some(primary), self.screen, self.screen) {
            self.screen = frame;
            self.brain.set_screen(frame.w, frame.h);
        }
    }

    fn draw(&mut self, scene: &Scene) {
        // Brain coordinates are y-up within the monitor; Windows' are y-down.
        let s = self.screen;
        let (paws_x, paws_y) = (s.x + scene.x, s.y + s.h - scene.y);
        let (x, y) = ((paws_x - self.reach as f64) as i32, (paws_y - self.reach as f64) as i32);
        let key = (scene.screaming, scene.frame, scene.yell, scene.heading.to_bits());
        if self.possum_key != Some(key) {
            self.possum_key = Some(key);
            let img = render::rotated(self.art.possum(scene), self.scale, scene.heading, self.reach);
            self.possum.show(&img, x, y);
        } else {
            self.possum.move_to(x, y);
        }

        match scene.bubble {
            Some(b) => {
                let (w, h) = (b.w.round() as usize, b.h.round() as usize);
                let (bx, by) = ((s.x + b.left) as i32, (s.y + s.h - b.bottom - h as f64) as i32);
                let key = (b.kind, b.shout, b.quadrant);
                if self.bubble_key != Some(key) || !self.bubble.visible {
                    self.bubble_key = Some(key);
                    self.bubble.show(&render::scaled(self.art.bubble(&b), w, h), bx, by);
                } else {
                    self.bubble.move_to(bx, by);
                }
            }
            None => self.bubble.hide(),
        }
    }
}
