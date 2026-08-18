//! §13.2.8.4 GetTemplateObject — the tagged-template site kernel.
//!
//! The lowering streams one site as `begin(site, n)` → n ×
//! `str(cooked, raw)` → `end()`. Per ES the template object is
//! cached PER SITE: every evaluation of the same TemplateLiteral
//! parse node answers the identical object, so `begin` on a cached
//! site arms the hit path (the `str` calls become no-ops and `end`
//! answers the cached cell). A negative site never caches — the
//! reduced REPL/LSP pipelines skip the numbering pass and take a
//! fresh object per evaluation as the safe degradation.
//!
//! The object: a frozen Arr<Any> of the cooked Str cells whose
//! `.raw` expando is a frozen Arr<Any> of the raw Str cells
//! (§13.2.8.4 steps 8-14). `end` answers a BORROW — the cache holds
//! the one owning reference and the object is immortal.
//!
//! Single-threaded JS runtime: the collector state and the cache are
//! `static mut` arrays with direct index reads/writes (the classmeta
//! PROTOS_BY_TAG_IMM convention — no references taken, rust 2024
//! `static_mut_refs` clean). Multi-thread retrofit rides the §6.2
//! backlog with the rest of the runtime registries.

use core::ffi::c_void;

unsafe extern "C" {
    /// torajs-arr — allocate an Any-element array (16-byte slots).
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// torajs-arr — push one (tag, value) pair; answers the (possibly
    /// reallocated) array cell.
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    /// torajs-meta — SetIntegrityLevel(O, frozen).
    fn __torajs_anyv_freeze(obj_any: u64) -> u64;
    /// torajs-str — mint a Str cell from ASCII bytes (the ".raw" key).
    fn __torajs_str_alloc_ascii(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
}

const ANY_HEAP_TAG: u64 = 4;

/// Per-site cache — 0 = not built yet. Sites past the cap degrade to
/// uncached (fresh object per evaluation — slow but correct).
const MAX_TEMPLATE_SITES: usize = 4096;
static mut TMPL_CACHE: [u64; MAX_TEMPLATE_SITES] = [0u64; MAX_TEMPLATE_SITES];

/// Collector state between `begin` and `end`.
static mut CUR_HIT: u64 = 0;
static mut CUR_SITE: i64 = -1;
static mut CUR_COOKED: u64 = 0;
static mut CUR_RAW: u64 = 0;

/// # Safety
/// Single-threaded runtime; calls arrive in begin/str*/end order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_template_object_begin(site: i64, n: i64) {
    unsafe {
        CUR_SITE = site;
        if site >= 0 && (site as usize) < MAX_TEMPLATE_SITES {
            let cached = TMPL_CACHE[site as usize];
            if cached != 0 {
                CUR_HIT = cached;
                return;
            }
        }
        CUR_HIT = 0;
        CUR_COOKED = __torajs_arr_alloc_any(n as u64) as u64;
        CUR_RAW = __torajs_arr_alloc_any(n as u64) as u64;
    }
}

/// # Safety
/// `cooked` / `raw` are live Str cells (static interned literals).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_template_object_str(cooked: *mut c_void, raw: *mut c_void) {
    unsafe {
        if CUR_HIT != 0 {
            return;
        }
        // Static literal cells — rc is a no-op on them, the push
        // stores the pointer.
        CUR_COOKED =
            __torajs_arr_push_any(CUR_COOKED as *mut c_void, ANY_HEAP_TAG, cooked as u64) as u64;
        CUR_RAW = __torajs_arr_push_any(CUR_RAW as *mut c_void, ANY_HEAP_TAG, raw as u64) as u64;
    }
}

/// # Safety
/// Single-threaded runtime; follows a `begin` on the same site.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_template_object_end() -> u64 {
    unsafe {
        if CUR_HIT != 0 {
            let out = CUR_HIT;
            CUR_HIT = 0;
            return out;
        }
        // Freeze the raw array first, wire it as `.raw`, then freeze
        // the cooked array (per §13.2.8.4 the raw object is integrity-
        // locked before the definition lands, and the outer freeze
        // must come after the expando write or the write would be
        // refused).
        let raw_any =
            crate::nanbox_encode::__torajs_anyv_box_from_pair(ANY_HEAP_TAG as i64, CUR_RAW as i64);
        let _ = __torajs_anyv_freeze(raw_any);
        let mut cooked_any = crate::nanbox_encode::__torajs_anyv_box_from_pair(
            ANY_HEAP_TAG as i64,
            CUR_COOKED as i64,
        );
        let raw_key = __torajs_str_alloc_ascii(b"raw".as_ptr(), 3);
        crate::member_set::__torajs_any_member_set(
            &mut cooked_any,
            raw_key as *mut c_void,
            ANY_HEAP_TAG,
            CUR_RAW,
            -1,
        );
        __torajs_str_drop(raw_key as *mut c_void);
        let out = __torajs_anyv_freeze(cooked_any);
        if CUR_SITE >= 0 && (CUR_SITE as usize) < MAX_TEMPLATE_SITES {
            TMPL_CACHE[CUR_SITE as usize] = out;
        }
        CUR_COOKED = 0;
        CUR_RAW = 0;
        out
    }
}
