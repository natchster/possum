//! Drives Possum's tray menu from inside Wine (or Windows) without the real
//! mouse or keyboard, for windows-test.sh: it posts Possum the messages a
//! click on its tray icon and keys in its menu would bring, and looks at its
//! windows. Steps run in order:
//!
//!   click X Y          a click on the tray icon, which is at (X, Y)
//!   key KEY...         keys for the open menu: up, down, enter, escape
//!   wait MS            a pause, in milliseconds
//!   menu open|closed   fails unless Possum's menu is open (or closed)
//!
//! For example: `wine-driver click 400 300 wait 800 menu open key escape`.
//! The message numbers are Possum's own (src/platform/windows.rs).

#[cfg(windows)]
fn main() {
    if let Err(e) = driver::run(std::env::args().skip(1).collect()) {
        eprintln!("wine-driver: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("wine-driver drives the Windows build: build it for Windows and run it with Wine");
    std::process::exit(2);
}

#[cfg(windows)]
mod driver {
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP};
    use windows_sys::Win32::UI::Shell::NIN_SELECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_APP, WM_KEYDOWN,
        WM_KEYUP,
    };

    /// Possum's tray message (WM_TRAY in windows.rs) and icon id.
    const WM_TRAY: u32 = WM_APP + 1;
    const TRAY_ICON_ID: u32 = 1;

    pub fn run(args: Vec<String>) -> Result<(), String> {
        let main = find_window(|class, visible, _| class == "PossumWindow" && !visible)
            .ok_or("Possum isn't running (no hidden PossumWindow)")?;
        let possum_process = process_of(main);
        let mut steps = args.iter().map(String::as_str).peekable();
        while let Some(step) = steps.next() {
            match step {
                "click" => {
                    let mut number = || -> Result<i32, String> {
                        let arg = steps.next().ok_or("click needs X and Y")?;
                        arg.parse().map_err(|_| format!("{arg:?} isn't a number"))
                    };
                    let (x, y) = (number()?, number()?);
                    // As the notification area sends it in its version 4:
                    // the event and icon id in lparam, the place in wparam.
                    let wparam = (x as u16 as u32 | ((y as u16 as u32) << 16)) as WPARAM;
                    let lparam = (NIN_SELECT | (TRAY_ICON_ID << 16)) as LPARAM;
                    post(main, WM_TRAY, wparam, lparam);
                }
                "key" => {
                    while let Some(&name) = steps.peek() {
                        let key = match name {
                            "up" => VK_UP,
                            "down" => VK_DOWN,
                            "enter" => VK_RETURN,
                            "escape" => VK_ESCAPE,
                            _ => break,
                        };
                        steps.next();
                        // The menu's loop reads keys from the thread's queue.
                        post(main, WM_KEYDOWN, key as WPARAM, 1);
                        post(main, WM_KEYUP, key as WPARAM, 0xC000_0001_u32 as LPARAM);
                        std::thread::sleep(Duration::from_millis(150));
                    }
                }
                "wait" => {
                    let ms = steps.next().and_then(|a| a.parse().ok()).ok_or("wait needs milliseconds")?;
                    std::thread::sleep(Duration::from_millis(ms));
                }
                "menu" => {
                    // Popup menus are windows of class #32768.
                    let open = find_window(|class, visible, process| {
                        class == "#32768" && visible && process == possum_process
                    })
                    .is_some();
                    match steps.next() {
                        Some("open") if !open => return Err("the menu isn't open".into()),
                        Some("closed") if open => return Err("the menu is still open".into()),
                        Some("open" | "closed") => {}
                        _ => return Err("menu needs open or closed".into()),
                    }
                }
                other => return Err(format!("unknown step {other:?}")),
            }
        }
        Ok(())
    }

    fn post(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) {
        // SAFETY: posting a message copies its plain-number arguments.
        unsafe { PostMessageW(window, message, wparam, lparam) };
    }

    fn process_of(window: HWND) -> u32 {
        let mut process = 0;
        // SAFETY: a valid out-pointer; a stale handle just gives 0.
        unsafe { GetWindowThreadProcessId(window, &mut process) };
        process
    }

    /// The first top-level window for which `wanted(class, visible, process)`.
    fn find_window(wanted: impl Fn(&str, bool, u32) -> bool) -> Option<HWND> {
        struct Search<'a> {
            wanted: &'a dyn Fn(&str, bool, u32) -> bool,
            found: Option<HWND>,
        }
        unsafe extern "system" fn each(window: HWND, search: LPARAM) -> windows_sys::core::BOOL {
            // SAFETY: `search` is the Search below, alive during EnumWindows.
            let search = unsafe { &mut *(search as *mut Search) };
            let mut class = [0u16; 64];
            // SAFETY: the buffer's length is passed along.
            let n = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) } as usize;
            // SAFETY: any window handle EnumWindows gives is valid to ask.
            let visible = unsafe { IsWindowVisible(window) } != 0;
            if (search.wanted)(&String::from_utf16_lossy(&class[..n]), visible, process_of(window)) {
                search.found = Some(window);
                return 0;
            }
            1
        }
        let mut search = Search { wanted: &wanted, found: None };
        // SAFETY: the callback only uses `search`, which outlives the call.
        unsafe { EnumWindows(Some(each), &mut search as *mut Search as LPARAM) };
        search.found
    }
}
