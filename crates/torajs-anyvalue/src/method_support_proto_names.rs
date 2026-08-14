//! The ENUMERATION half of a builtin prototype's synthesized own
//! surface — the direction `method_support_proto` never had.
//!
//! That file answers "does `<Ctor>.prototype` own this key?" one key
//! at a time, which is all a read or a gOPD needs. Nothing answered
//! "which keys does it own", so `getOwnPropertyNames(Map.prototype)`
//! walked the real dict, found it empty, and said so — while
//! `Map.prototype.hasOwnProperty("entries")` said true off the same
//! ownership table. Two faces of one fact disagreeing is the shape
//! rotation 382 closed for `@@toStringTag`; this is its string-key
//! twin.
//!
//! Order is the method-id order of `torajs-rc`'s intern table, then
//! the family's accessors, then `constructor`. It is NOT bun's, and
//! it does not need to be: §10.1.11.1 fixes the three BUCKETS
//! (integer indices, then strings in creation order, then symbols),
//! but the creation order of a builtin's own properties is
//! implementation-defined — V8 / JSC / SpiderMonkey each answer a
//! different permutation, so no test262 case can pin it.
//!
//! Tombstones are honoured through `proto_tag_owns` /
//! `constructor_live`, so a `delete Map.prototype.get` drops the
//! name here too.

use core::ffi::c_void;

use crate::method_support_proto::{constructor_live, proto_tag_owns};
use crate::method_value::{TABLE_SIZE, builtin_method_name_cell, mint_immortal_str};
use core::sync::atomic::{AtomicU64, Ordering};

/// The accessor own properties a family carries, as (id, name) — the
/// enumeration twin of `method_support_proto::proto_tag_accessor_mid`,
/// which answers the same table one key at a time. Map / Set `size`
/// (§24.1.3.10 / §24.2.3.9) and Symbol `description` (§20.4.3.2) are
/// the ones tr materializes; RegExp's §22.2.6 flag accessors are not
/// modelled yet and are absent from BOTH faces, so the two still
/// agree.
fn proto_tag_accessors(tag: i64) -> &'static [(i64, &'static str)] {
    match tag {
        5 => &[(torajs_rc::ANY_METHOD_GET_DESCRIPTION, "description")],
        11 | 12 => &[(torajs_rc::ANY_METHOD_GET_SIZE, "size")],
        _ => &[],
    }
}

/// §B.2.2.15/16 — `String.prototype`'s Annex B aliases, own
/// properties in their own right: distinct KEYS over the same
/// reified function cells (the intern table maps all four spellings
/// onto two mids, which is what already makes `trimLeft ===
/// trimStart` true and `hasOwnProperty("trimLeft")` answer yes).
/// The enumeration walks mids — one name per mid — so the alias
/// keys need their own rows or the two faces disagree (probed:
/// `getOwnPropertyNames` said no while `hasOwnProperty` said yes).
/// The mid rides along for the ownership/tombstone gate: a
/// `delete` tombstones the MID, taking both spellings with it —
/// the granularity the whole surface shares.
fn proto_tag_alias_names(tag: i64) -> &'static [(i64, &'static AtomicU64, &'static [u8])] {
    static TRIM_LEFT_CELL: AtomicU64 = AtomicU64::new(0);
    static TRIM_RIGHT_CELL: AtomicU64 = AtomicU64::new(0);
    static STR_ALIASES: [(i64, &AtomicU64, &[u8]); 2] = [
        (
            torajs_rc::ANY_METHOD_TRIM_START,
            &TRIM_LEFT_CELL,
            b"trimLeft",
        ),
        (
            torajs_rc::ANY_METHOD_TRIM_END,
            &TRIM_RIGHT_CELL,
            b"trimRight",
        ),
    ];
    match tag {
        3 => &STR_ALIASES,
        _ => &[],
    }
}

/// Interned `"constructor"` name cell — every builtin prototype owns
/// one (§20.x.3.1 family), and it has no method id to intern under.
static CONSTRUCTOR_NAME_CELL: AtomicU64 = AtomicU64::new(0);

fn interned_name_cell(slot: &AtomicU64, name: &[u8]) -> *mut u8 {
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    let cell = mint_immortal_str(name);
    slot.store(cell as u64, Ordering::Relaxed);
    cell
}

fn constructor_name_cell() -> *mut u8 {
    interned_name_cell(&CONSTRUCTOR_NAME_CELL, b"constructor")
}

/// Run `f` over every synthesized own string key of the builtin
/// prototype tagged `tag`, in the documented order. Names are
/// immortal interned Str cells — borrows, no rc traffic.
fn for_each_own_name(tag: i64, mut f: impl FnMut(*mut u8)) {
    for mid in 0..TABLE_SIZE as i64 {
        if !proto_tag_owns(tag, mid) {
            continue;
        }
        let Some((name, _)) = torajs_rc::any_method_meta(mid) else {
            continue;
        };
        f(builtin_method_name_cell(mid, name));
    }
    for &(mid, slot, name) in proto_tag_alias_names(tag) {
        if proto_tag_owns(tag, mid) {
            f(interned_name_cell(slot, name));
        }
    }
    for &(mid, name) in proto_tag_accessors(tag) {
        // An accessor id is in no family's `*_supports` table, so
        // `proto_tag_owns` would answer false for it — its liveness
        // is purely the tombstone question, which is the same bitmask
        // `prop_delete` marks and `__torajs_builtin_proto_own_accessor_getter`
        // consults.
        // SAFETY: pure bitmask read; range-checked inside.
        if unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_is_deleted(tag, mid) } == 0 {
            f(builtin_method_name_cell(mid, name));
        }
    }
    if constructor_live(tag) {
        f(constructor_name_cell());
    }
}

/// Write the synthesized own string keys of `proto` into `out` (Str
/// cell pointers, borrowed) and answer how many keys exist.
///
/// Answers the FULL count even when it exceeds `cap`, so a caller can
/// size a buffer with a `cap == 0` probe and call again — the
/// `__torajs_dynobj_iter_symbol_order` protocol. 0 for a receiver
/// that is not a builtin prototype singleton.
///
/// # Safety
/// `proto` is NULL or a live heap cell; `out` is writable for `cap`
/// `u64`s (may be NULL when `cap` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_own_names(
    proto: *const c_void,
    out: *mut u64,
    cap: u64,
) -> u64 {
    let tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(proto) };
    if tag < 0 {
        return 0;
    }
    let mut n = 0u64;
    for_each_own_name(tag, |cell| {
        if n < cap && !out.is_null() {
            // SAFETY: n < cap and the caller's buffer holds cap u64s.
            unsafe { out.add(n as usize).write(cell as u64) };
        }
        n += 1;
    });
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every mid a prototype OWNS must have a name in
    /// `any_method_meta` — the enumeration above can only emit what
    /// that table names, so a missing row would drop the key from
    /// `getOwnPropertyNames` while `hasOwnProperty` kept answering
    /// true. That is exactly the two-faces-disagree bug this module
    /// exists to close, so it gets a mechanical guard rather than a
    /// silent `continue`.
    #[test]
    fn every_owned_mid_has_a_name() {
        let mut missing: Vec<(i64, i64)> = Vec::new();
        for tag in 0..torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS as i64 {
            for mid in 0..TABLE_SIZE as i64 {
                if proto_tag_owns(tag, mid) && torajs_rc::any_method_meta(mid).is_none() {
                    missing.push((tag, mid));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "owned but unnamed (tag, mid): {missing:?}"
        );
    }
}
