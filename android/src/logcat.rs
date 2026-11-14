//! Android's log, which `adb logcat -s possum` shows. The shared code logs
//! to stderr, which goes nowhere on Android, so stderr is piped here line by
//! line.

use std::ffi::CString;
use std::io::{BufRead, BufReader};
use std::os::fd::FromRawFd;
use std::os::raw::{c_char, c_int};
use std::sync::Once;

#[link(name = "log")]
extern "C" {
    fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

extern "C" {
    fn pipe(fds: *mut c_int) -> c_int;
    fn dup2(from: c_int, to: c_int) -> c_int;
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
        let mut fds = [0 as c_int; 2];
        // SAFETY: `fds` has room for the two descriptors pipe() writes.
        if unsafe { pipe(fds.as_mut_ptr()) } != 0 || unsafe { dup2(fds[1], 2) } < 0 {
            write("can't capture stderr; the possum's own log is lost");
            return;
        }
        // SAFETY: the read end is a fresh descriptor that nothing else owns.
        let reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
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
