//! Android's log, which `adb logcat -s possum` shows. The shared code logs
//! to stderr, which goes nowhere on Android, so stderr is piped here line by
//! line.

use std::ffi::CString;
use std::io::{BufRead, BufReader};
use std::os::fd::AsRawFd;
use std::os::raw::{c_char, c_int};
use std::sync::Once;

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

unsafe extern "C" {
    /// Makes descriptor `to` refer to what `from` does. Plain integers in
    /// and out, so it can't break memory safety.
    safe fn dup2(from: c_int, to: c_int) -> c_int;
}

const INFO: c_int = 4;

pub fn write(line: &str) {
    if let Ok(text) = CString::new(line) {
        // SAFETY: both strings are NUL-terminated and outlive the call.
        unsafe { __android_log_write(INFO, c"possum".as_ptr(), text.as_ptr()) };
    }
}

/// Sends everything written to stderr from now on to the log.
pub fn capture_stderr() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // stderr becomes a copy of the pipe's write end, which then isn't
        // needed itself.
        let reader = match std::io::pipe() {
            Ok((reader, writer)) if dup2(writer.as_raw_fd(), 2) >= 0 => reader,
            _ => {
                write("can't capture stderr; the possum's own log is lost");
                return;
            }
        };
        let spawned = std::thread::Builder::new().name("possum-log".into()).spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                write(&line);
            }
        });
        if spawned.is_err() {
            write("can't start the log thread");
        }
    });
}
