//! Browser entry points for the engine: song JSON in, stereo PCM and a song
//! sheet out. Plain C ABI, no bindgen: the host allocates with `sf_alloc`,
//! writes the JSON, calls `sf_render`, then reads the buffers the `sf_out_*`
//! getters point at. One render at a time; state is per instance.
//!
//! Rendering runs on the calling thread (rayon falls back to it where no
//! thread can be spawned), so the host should call from a Web Worker.

use std::cell::RefCell;

use engine::{MixSettings, Progress};

extern "C" {
    /// Host import: called after each render task finishes.
    fn sf_progress(done: u32, total: u32);
}

struct Host;
impl Progress for Host {
    fn advance(&self, done: usize, total: usize) {
        // SAFETY: the host provides `sf_progress` (see the module docs).
        unsafe { sf_progress(done as u32, total as u32) }
    }
}

#[derive(Default)]
struct Out {
    l: Vec<f32>,
    r: Vec<f32>,
    sheet: String,
    error: String,
}

thread_local! {
    static OUT: RefCell<Out> = RefCell::new(Out::default());
}

/// Allocates `len` bytes for the host to write into.
#[no_mangle]
pub extern "C" fn sf_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Frees a block from `sf_alloc`.
#[no_mangle]
pub extern "C" fn sf_free(p: *mut u8, len: usize) {
    // SAFETY: `p` came from `sf_alloc(len)` (capacity `len.max(1)`).
    unsafe { drop(Vec::from_raw_parts(p, 0, len.max(1))) }
}

fn run(json: &str, seed: u64) -> Result<Out, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let (song, _repairs) = song::normalize_value(&v).map_err(|e| e.to_string())?;
    let (prepared, stems) = engine::render(&song, seed, None, &Host);
    let settings = MixSettings::default_for(&stems);
    let m = engine::mix_with(&stems, &song.band, seed, &settings);
    drop(stems);
    let sheet = engine::sheet_from(&song, seed, &prepared);
    let sheet = serde_json::to_string(&sheet).map_err(|e| e.to_string())?;
    Ok(Out {
        l: m.l,
        r: m.r,
        sheet,
        error: String::new(),
    })
}

/// Renders the song JSON at `json`/`len` with `seed`. Returns 0 on success,
/// 1 on error (read `sf_out_error_*`).
#[no_mangle]
pub extern "C" fn sf_render(json: *const u8, len: usize, seed_lo: u32, seed_hi: u32) -> u32 {
    // SAFETY: the host wrote `len` bytes at `json` via `sf_alloc`.
    let bytes = unsafe { std::slice::from_raw_parts(json, len) };
    let seed = (seed_hi as u64) << 32 | seed_lo as u64;
    let res = match std::str::from_utf8(bytes) {
        Ok(s) => run(s, seed),
        Err(e) => Err(e.to_string()),
    };
    OUT.with(|o| match res {
        Ok(out) => {
            *o.borrow_mut() = out;
            0
        }
        Err(e) => {
            *o.borrow_mut() = Out {
                error: e,
                ..Out::default()
            };
            1
        }
    })
}

fn with<T>(f: impl FnOnce(&Out) -> T) -> T {
    OUT.with(|o| f(&o.borrow()))
}

#[no_mangle]
pub extern "C" fn sf_out_frames() -> usize {
    with(|o| o.l.len())
}
#[no_mangle]
pub extern "C" fn sf_out_left() -> *const f32 {
    with(|o| o.l.as_ptr())
}
#[no_mangle]
pub extern "C" fn sf_out_right() -> *const f32 {
    with(|o| o.r.as_ptr())
}
#[no_mangle]
pub extern "C" fn sf_out_sheet_ptr() -> *const u8 {
    with(|o| o.sheet.as_ptr())
}
#[no_mangle]
pub extern "C" fn sf_out_sheet_len() -> usize {
    with(|o| o.sheet.len())
}
#[no_mangle]
pub extern "C" fn sf_out_error_ptr() -> *const u8 {
    with(|o| o.error.as_ptr())
}
#[no_mangle]
pub extern "C" fn sf_out_error_len() -> usize {
    with(|o| o.error.len())
}
/// Sample rate of the output, in Hz.
#[no_mangle]
pub extern "C" fn sf_sample_rate() -> u32 {
    sfcore::SR as u32
}
/// Frees the output buffers.
#[no_mangle]
pub extern "C" fn sf_clear() {
    OUT.with(|o| *o.borrow_mut() = Out::default());
}
