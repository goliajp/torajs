//! 405-04 knife 2 — proto/class registry aliasing for GENERIC
//! specialization tags.
//!
//! A generic class's instances wear per-factory anon-pool tags
//! (404-01 / 405-03), minted off the shared counter — often beyond
//! `MAX_CLASSES` — while the `__proto_<C>` / `__class_<C>` registry
//! slots are filled under the class's MAIN tag only (the
//! class_globals emit). Resolve by name identity, the same verdict
//! `instanceof` uses (`instanceof_generic.rs`): the specialization
//! row and the main row naming the same class means they are the
//! same class — a class name is program-unique by the time it
//! reaches a row.
//!
//! The cache stores the resolved MAIN TAG, never the proto value —
//! a dynobj define may RESIZE the proto cell (rotation 186), and a
//! cached value would dangle; reading the registry slot through the
//! aliased tag always sees the current cell.

use crate::instanceof_generic::names_match;

unsafe extern "C" {
    /// torajs-structmeta — outer-entry flags bit 1 (generic row).
    fn __torajs_struct_row_is_generic(class_tag: u32) -> bool;
}

/// `(specialization tag, main tag)` pairs — a tiny direct-scan
/// cache. `static mut` matches this module's registry arrays (JS
/// execution is single-threaded, see the classmeta.rs module doc).
/// NOT `thread_local!`: std's LocalKey machinery silently kills the
/// AOT staticlib runtime — no std rt init ever runs there (same
/// family as the `eprintln!` SIGBUS note in the build-and-port
/// rules; verified empirically on this very table).
const CACHE_SLOTS: usize = 64;
static mut MAIN_TAG_CACHE: [(u32, u32); CACHE_SLOTS] = [(0, 0); CACHE_SLOTS];
static mut CACHE_LEN: usize = 0;

/// The registered main tag whose row names the same class as `tag`,
/// or `None` when `tag` carries no name or no registered namesake
/// exists. Positive hits are cached; misses are not (an early call
/// racing module-init registration order must not freeze a miss).
pub(super) fn main_tag_of(tag: i64) -> Option<usize> {
    let t = u32::try_from(tag).ok()?;
    // Rotation 408 hang fix — the alias triggers ONLY on rows the
    // compiler marked as generic specializations (outer-entry flags
    // bit 1). A non-generic tag with an empty registry slot must
    // keep the null/undefined answer: several consumers' termination
    // logic depends on a miss staying a miss (an Iterator-helper
    // reduce over a subclass instance span into an infinite loop
    // when the blanket by-name scan turned its miss into a hit —
    // test262 sm/Iterator/prototype/reduce, caught by the sweep's
    // pass→tr-timeout regression and bisected to the blanket form).
    if !unsafe { __torajs_struct_row_is_generic(t) } {
        return None;
    }
    // SAFETY: single-threaded JS runtime (module doc); plain indexed
    // reads, no reference to the mutable static is formed.
    unsafe {
        for i in 0..CACHE_LEN {
            let (spec, main) = MAIN_TAG_CACHE[i];
            if spec == t {
                return Some(main as usize);
            }
        }
    }
    // Only main tags ever get a proto slot, and those are named-class
    // tags below MAX_CLASSES — the scan face is at most 256 rodata
    // name reads, once per specialization tag (cached after; a
    // program with more than CACHE_SLOTS generic classes keeps the
    // overflow tags on the full scan — correctness is unaffected).
    for cand in 1..super::MAX_CLASSES {
        // SAFETY: single-threaded JS runtime (module doc).
        let filled = unsafe { super::PROTOS_BY_TAG_IMM[cand] != 0 };
        if !filled || cand as i64 == tag {
            continue;
        }
        if names_match(t, cand as u32) {
            // SAFETY: as above.
            unsafe {
                if CACHE_LEN < CACHE_SLOTS {
                    MAIN_TAG_CACHE[CACHE_LEN] = (t, cand as u32);
                    CACHE_LEN += 1;
                }
            }
            return Some(cand);
        }
    }
    None
}
