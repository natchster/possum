//! The functions `Native.java` declares (`Java_<package>_Native_<name>`).
//! None of them lets a panic reach Java: it's logged (stderr goes to the
//! log) and the call returns something harmless instead.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::null_mut;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, MutexGuard, OnceLock};

use jni::JNIEnv;
use jni::objects::{JClass, JFloatArray, JIntArray};
use jni::sys::{JNI_FALSE, jboolean, jdouble, jfloat, jfloatArray, jint, jintArray, jshortArray};

use crate::{Core, Pictures, Voices, argb, brain, logcat, seed, sprite};

/// The most sound requests one tick can return (Java's array is this long).
const MAX_REQUESTS: usize = 8;

/// Made once per process: the pictures and the synthesised screams.
struct Shared {
    pictures: Pictures,
    voices: Voices,
}

static SHARED: OnceLock<Shared> = OnceLock::new();
static CORE: Mutex<Option<Core>> = Mutex::new(None);

fn shared() -> &'static Shared {
    SHARED.get_or_init(|| {
        logcat::capture_stderr();
        Shared { pictures: Pictures::new(), voices: Voices::new(seed()) }
    })
}

fn running() -> MutexGuard<'static, Option<Core>> {
    CORE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_init(_env: JNIEnv, _class: JClass) {
    guard((), || {
        shared();
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_run(
    _env: JNIEnv,
    _class: JClass,
    scale: jfloat,
    speed: jfloat,
    width: jfloat,
    height: jfloat,
) {
    guard((), || {
        let (scale, speed, width, height) = (scale as f64, speed as f64, width as f64, height as f64);
        let core = Core::new(&shared().voices, scale, speed, width, height, seed());
        core.log(format_args!("running around a {width}x{height} track, {scale} pixels per sprite pixel"));
        *running() = Some(core);
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_stop(_env: JNIEnv, _class: JClass) {
    guard((), || *running() = None)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_resize(
    _env: JNIEnv,
    _class: JClass,
    width: jfloat,
    height: jfloat,
) {
    guard((), || {
        if let Some(core) = running().as_mut() {
            core.resize(width as f64, height as f64);
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_tick(
    env: JNIEnv,
    _class: JClass,
    now: jdouble,
    silent: jboolean,
    quiet: jint,
    hush: jint,
    scene: JFloatArray,
    requests: JIntArray,
) -> jint {
    guard(0, || {
        let mut core = running();
        let Some(core) = core.as_mut() else { return 0 };
        let (numbers, ops) = core.tick(&shared().pictures, now, silent != JNI_FALSE, quiet, hush);
        let n = ops.len().min(MAX_REQUESTS);
        if env.set_float_array_region(&scene, 0, &numbers).is_err()
            || env.set_int_array_region(&requests, 0, &ops[..n]).is_err()
        {
            return 0;
        }
        n as jint
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_screamNow(_env: JNIEnv, _class: JClass) {
    brain::SCREAM_NOW.store(true, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_pictureCount(_env: JNIEnv, _class: JClass) -> jint {
    guard(0, || shared().pictures.count() as jint)
}

/// Picture `index` as `[width, height, colours...]`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_picture(
    env: JNIEnv,
    _class: JClass,
    index: jint,
) -> jintArray {
    guard(null_mut(), || {
        let Some(canvas) = shared().pictures.get(index as usize) else { return null_mut() };
        let mut data = vec![canvas.w, canvas.h];
        data.extend(argb(canvas));
        match env.new_int_array(data.len() as jint) {
            Ok(array) if env.set_int_array_region(&array, 0, &data).is_ok() => array.into_raw(),
            _ => null_mut(),
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_statusIcon(_env: JNIEnv, _class: JClass) -> jint {
    guard(0, || shared().pictures.status_icon() as jint)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_portrait(_env: JNIEnv, _class: JClass) -> jint {
    guard(0, || shared().pictures.portrait() as jint)
}

/// `[paws x, paws y, possum width, possum height, bubble width, bubble
/// height]`, in sprite pixels: the paws are where the possum touches the
/// screen edge, from the top-left of its pictures.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_layout(env: JNIEnv, _class: JClass) -> jfloatArray {
    guard(null_mut(), || {
        let p = &shared().pictures;
        let ((pw, ph), (bw, bh)) = (p.possum_size(), p.bubble_size());
        let data = [sprite::FEET_X, sprite::FEET_Y, pw as f32, ph as f32, bw as f32, bh as f32];
        match env.new_float_array(data.len() as jint) {
            Ok(array) if env.set_float_array_region(&array, 0, &data).is_ok() => array.into_raw(),
            _ => null_mut(),
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_sampleRate(_env: JNIEnv, _class: JClass) -> jint {
    guard(0, || shared().voices.sample_rate as jint)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_voiceCount(
    _env: JNIEnv,
    _class: JClass,
    kind: jint,
) -> jint {
    guard(0, || shared().voices.samples.get(kind as usize).map_or(0, |v| v.len() as jint))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_natchster_possum_Native_voice(
    env: JNIEnv,
    _class: JClass,
    kind: jint,
    variant: jint,
) -> jshortArray {
    guard(null_mut(), || {
        let voices = &shared().voices.samples;
        let Some(samples) = voices.get(kind as usize).and_then(|v| v.get(variant as usize)) else {
            return null_mut();
        };
        match env.new_short_array(samples.len() as jint) {
            Ok(array) if env.set_short_array_region(&array, 0, samples).is_ok() => array.into_raw(),
            _ => null_mut(),
        }
    })
}
