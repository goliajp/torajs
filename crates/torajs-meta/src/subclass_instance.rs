//! Exotic-subclass instance identity side table (RFC
//! 20260730-exotic-backed-class-instance blade 0).
//!
//! `class C extends Array` mints a REAL `Tag::Arr` cell — the exotic
//! layouts have no free header word for a class identity (every +8 is
//! taken: Arr=len, Promise=state, Map=n_entries), so the identity
//! lives here: `cell ptr → (class_tag, prototype cell)`, entered at
//! mint time and removed on drop. The cell itself carries only
//! `torajs_rc::FLAG_SUBCLASSED`; every reader gates on that bit first,
//! so plain builtin instances never reach this table.
//!
//! Unlike the fixed 512-slot class registry (`torajs-anyvalue`'s
//! `construct.rs` — classes are bounded), instances are unbounded: the
//! table is a growable open-addressed block behind an `AtomicPtr`.
//! Removal tombstones the key; growth rehashes live entries and drops
//! tombstones. Key 0 is empty, key 1 is the tombstone — heap cells are
//! 8-aligned, so neither collides with a real pointer.
//!
//! The prototype cell is the class's `__proto_<C>` singleton, owned
//! for the process by the class registry — the table stores the bits
//! and takes no reference, same contract as the ctor registry.
//!
//! Atomics per the multi-thread-ready substrate rule (design
//! principles §6.2), in the codebase's established sense (torajs-throw
//! THROW_ACTIVE, construct.rs CTOR_KEY): the SHAPE is retrofit-ready,
//! the ordering is Relaxed single-mutator semantics. The biased-ARC
//! retrofit (v1.0+) revisits growth/removal concurrency together with
//! every other runtime registry.

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

const KEY_EMPTY: u64 = 0;
const KEY_TOMBSTONE: u64 = 1;
const INITIAL_CAP: usize = 64;

/// One table entry: cell pointer key + the two identity words.
#[derive(Clone, Copy)]
struct Entry {
    key: u64,
    class_tag: u64,
    proto_cell: u64,
}

/// Growable open-addressed table. `used` counts live + tombstoned
/// slots (the probe-chain load); `live` counts real entries.
struct Table {
    cap: usize,
    live: usize,
    used: usize,
    entries: *mut Entry,
}

static TABLE: AtomicPtr<Table> = AtomicPtr::new(core::ptr::null_mut());

/// MurmurHash3 finalizer — same mixer the ctor registry uses; heap
/// pointers are 8-aligned so the low bits alone collide constantly.
#[inline]
fn mix(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51_afd7_ed55_8ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    k ^ (k >> 33)
}

fn alloc_table(cap: usize) -> *mut Table {
    let entries = unsafe {
        std::alloc::alloc_zeroed(std::alloc::Layout::array::<Entry>(cap).expect("entry layout"))
    } as *mut Entry;
    let table = Box::new(Table {
        cap,
        live: 0,
        used: 0,
        entries,
    });
    Box::into_raw(table)
}

fn table() -> &'static mut Table {
    let mut p = TABLE.load(Ordering::Relaxed);
    if p.is_null() {
        p = alloc_table(INITIAL_CAP);
        TABLE.store(p, Ordering::Relaxed);
    }
    unsafe { &mut *p }
}

/// Insert or overwrite `key`'s identity. Grows first when the probe
/// load (live + tombstones) crosses 3/4 — growth rehashes live entries
/// into a doubled block and forgets tombstones.
fn insert(key: u64, class_tag: u64, proto_cell: u64) {
    let t = table();
    if (t.used + 1) * 4 >= t.cap * 3 {
        grow(t);
    }
    let t = table();
    let mask = (t.cap - 1) as u64;
    let mut i = (mix(key) & mask) as usize;
    loop {
        let e = unsafe { &mut *t.entries.add(i) };
        if e.key == key {
            e.class_tag = class_tag;
            e.proto_cell = proto_cell;
            return;
        }
        if e.key == KEY_EMPTY || e.key == KEY_TOMBSTONE {
            if e.key == KEY_EMPTY {
                t.used += 1;
            }
            t.live += 1;
            *e = Entry {
                key,
                class_tag,
                proto_cell,
            };
            return;
        }
        i = (i + 1) & mask as usize;
    }
}

fn grow(t: &mut Table) {
    // The probe load that tripped this call is live + tombstones.
    // When tombstones dominate (mint/drop churn), rehashing at the
    // SAME capacity reclaims them; doubling is for a genuinely
    // growing live set. Without the distinction, churn alone doubles
    // the table forever.
    let new_cap = if t.live * 4 >= t.cap {
        t.cap * 2
    } else {
        t.cap
    };
    let new_entries = unsafe {
        std::alloc::alloc_zeroed(std::alloc::Layout::array::<Entry>(new_cap).expect("entry layout"))
    } as *mut Entry;
    let mask = (new_cap - 1) as u64;
    let mut moved = 0usize;
    for k in 0..t.cap {
        let e = unsafe { *t.entries.add(k) };
        if e.key == KEY_EMPTY || e.key == KEY_TOMBSTONE {
            continue;
        }
        let mut i = (mix(e.key) & mask) as usize;
        loop {
            let slot = unsafe { &mut *new_entries.add(i) };
            if slot.key == KEY_EMPTY {
                *slot = e;
                moved += 1;
                break;
            }
            i = (i + 1) & mask as usize;
        }
    }
    unsafe {
        std::alloc::dealloc(
            t.entries as *mut u8,
            std::alloc::Layout::array::<Entry>(t.cap).expect("entry layout"),
        );
    }
    t.entries = new_entries;
    t.cap = new_cap;
    t.live = moved;
    t.used = moved;
}

fn find(key: u64) -> Option<Entry> {
    let p = TABLE.load(Ordering::Relaxed);
    if p.is_null() {
        return None;
    }
    let t = unsafe { &*p };
    let mask = (t.cap - 1) as u64;
    let mut i = (mix(key) & mask) as usize;
    loop {
        let e = unsafe { &*t.entries.add(i) };
        if e.key == key {
            return Some(*e);
        }
        if e.key == KEY_EMPTY {
            return None;
        }
        i = (i + 1) & mask as usize;
    }
}

fn remove(key: u64) {
    let p = TABLE.load(Ordering::Relaxed);
    if p.is_null() {
        return;
    }
    let t = unsafe { &mut *p };
    let mask = (t.cap - 1) as u64;
    let mut i = (mix(key) & mask) as usize;
    loop {
        let e = unsafe { &mut *t.entries.add(i) };
        if e.key == key {
            e.key = KEY_TOMBSTONE;
            t.live -= 1;
            return;
        }
        if e.key == KEY_EMPTY {
            return;
        }
        i = (i + 1) & mask as usize;
    }
}

/// Record a freshly minted subclass instance's identity. Called by the
/// per-builtin subclass-alloc kernels (blade 1+) right after they set
/// `FLAG_SUBCLASSED` on the cell header.
///
/// # Safety
/// `cell` is a valid heap cell pointer; `proto_cell` is the class's
/// process-lifetime prototype singleton (no reference is taken).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_subclass_register(
    cell: *mut c_void,
    class_tag: i64,
    proto_cell: u64,
) {
    if cell.is_null() {
        return;
    }
    insert(cell as u64, class_tag as u64, proto_cell);
}

/// The instance's user-class tag, or -1 when the cell has no entry.
///
/// # Safety
/// `cell` is a valid heap cell pointer (readers gate on
/// `FLAG_SUBCLASSED` before calling).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_subclass_class_tag(cell: *const c_void) -> i64 {
    match find(cell as u64) {
        Some(e) => e.class_tag as i64,
        None => -1,
    }
}

/// The instance's prototype cell, or 0 when the cell has no entry.
///
/// # Safety
/// As [`__torajs_subclass_class_tag`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_subclass_proto(cell: *const c_void) -> u64 {
    match find(cell as u64) {
        Some(e) => e.proto_cell,
        None => 0,
    }
}

/// Remove a dying instance's entry. Called from the per-tag drop
/// kernels behind a `FLAG_SUBCLASSED` gate — a stale entry would
/// resurrect the identity on the next cell minted at the same address.
///
/// # Safety
/// `cell` was previously registered (or the call is a no-op).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_subclass_drop_entry(cell: *mut c_void) {
    remove(cell as u64);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        // Each test rebuilds from an empty table: drop whatever a
        // previous test left (tests run in one process).
        let p = TABLE.swap(core::ptr::null_mut(), Ordering::Relaxed);
        if !p.is_null() {
            let t = unsafe { Box::from_raw(p) };
            unsafe {
                std::alloc::dealloc(
                    t.entries as *mut u8,
                    std::alloc::Layout::array::<Entry>(t.cap).expect("entry layout"),
                );
            }
        }
    }

    #[test]
    fn register_lookup_remove_roundtrip() {
        reset();
        unsafe {
            __torajs_subclass_register(0x1000 as *mut c_void, 42, 0xBEEF);
            assert_eq!(__torajs_subclass_class_tag(0x1000 as *const c_void), 42);
            assert_eq!(__torajs_subclass_proto(0x1000 as *const c_void), 0xBEEF);
            __torajs_subclass_drop_entry(0x1000 as *mut c_void);
            assert_eq!(__torajs_subclass_class_tag(0x1000 as *const c_void), -1);
            assert_eq!(__torajs_subclass_proto(0x1000 as *const c_void), 0);
        }
    }

    #[test]
    fn miss_answers_sentinels() {
        reset();
        unsafe {
            assert_eq!(__torajs_subclass_class_tag(0x2000 as *const c_void), -1);
            assert_eq!(__torajs_subclass_proto(0x2000 as *const c_void), 0);
        }
    }

    #[test]
    fn reregister_overwrites() {
        reset();
        unsafe {
            __torajs_subclass_register(0x3000 as *mut c_void, 7, 0xA);
            __torajs_subclass_register(0x3000 as *mut c_void, 9, 0xB);
            assert_eq!(__torajs_subclass_class_tag(0x3000 as *const c_void), 9);
            assert_eq!(__torajs_subclass_proto(0x3000 as *const c_void), 0xB);
        }
        assert_eq!(table().live, 1);
    }

    #[test]
    fn churn_growth_and_tombstones_stay_bounded() {
        reset();
        // Mint/drop churn far past the initial capacity: the table
        // must stay correct and its capacity bounded by the live set,
        // not the total churn count (tombstones are dropped on grow).
        for round in 0u64..10_000 {
            let cell = (0x10_0000 + round * 8) as *mut c_void;
            unsafe {
                __torajs_subclass_register(cell, round as i64, round);
                assert_eq!(
                    __torajs_subclass_class_tag(cell as *const c_void),
                    round as i64
                );
                __torajs_subclass_drop_entry(cell);
            }
        }
        let t = table();
        assert_eq!(t.live, 0);
        assert!(t.cap <= 1024, "cap {} grew with churn, not live set", t.cap);
    }

    #[test]
    fn grow_keeps_all_live_entries() {
        reset();
        let n = 500u64;
        for k in 0..n {
            let cell = (0x20_0000 + k * 8) as *mut c_void;
            unsafe { __torajs_subclass_register(cell, k as i64, k + 1) };
        }
        for k in 0..n {
            let cell = (0x20_0000 + k * 8) as *const c_void;
            unsafe {
                assert_eq!(__torajs_subclass_class_tag(cell), k as i64);
                assert_eq!(__torajs_subclass_proto(cell), k + 1);
            }
        }
        assert_eq!(table().live, n as usize);
    }
}
