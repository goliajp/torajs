//! `__torajs_any_member_get_tag` / `_value` — the tag-gated
//! `(tag, value)` probe behind arbitrary-name member reads on `any`
//! receivers (the read mirror of `member_set.rs`; RFC 20260704 C4+).
//!
//! Pre-gate the lowering's fallback handed the receiver's payload
//! bits straight to `__torajs_dynobj_get_tag/value`, reading every
//! cell as a DynObj layout — an Arr receiver's expando probe missed
//! by accident (silent `undefined`), any other tag was an
//! out-of-layout read. The pair below gates first:
//!
//! - null / undefined receiver → catchable TypeError (the tag call
//!   records it; the value call stays silent so the pair doesn't
//!   double-throw), pair answers `(ANY_UNDEF, 0)`.
//! - `Tag::DynObj` → the ordinary own-property probe, accessor
//!   sentinel included (the lowering's `emit_dynobj_get_result`
//!   consumes it unchanged).
//! - `Tag::Arr` → the `arrprops` expando probe (NULL props slot
//!   answers absent).
//! - `Tag::Closure` (L3b #11 residue, chunk 529) → the lazy
//!   `props_dynobj` at `CLOSURE_PROPS_OFF` (T-27 Function-as-Object
//!   expandos; NULL slot answers absent). STATIC `.name` / `.length`
//!   member reads route to `__torajs_any_name_get` /
//!   `__torajs_any_length_get` (chunks 715/716) and never reach this
//!   pair; a DYNAMIC key (`f[k]`, chunk D RFC 20260711) lands here
//!   and answers the same metadata through `closure_virtual_pair`
//!   (immortal interned name cells — the pair is borrow-shaped).
//! - every other receiver (and an Arr / Closure expando miss) →
//!   the builtin-method reification probe (chunk 711,
//!   `method_value`): a supported method name answers the interned
//!   function cell; everything else is `(ANY_UNDEF, 0)` — a
//!   definite absent, never a layout mis-read.
//!
//! The pair is borrow-shaped exactly like the dynobj probe it
//! wraps — and so is the BOX the lowering assembles from it:
//! `anyv_box_from_pair` is a pure bit-encode (no refcount inc; see
//! nanbox_encode.rs), so the consumer slot is a view over the
//! bucket's stake, never an owner. The special-cased member
//! intrinsics (`any_length_get` / `any_name_get` / `any_size_get` /
//! `any_regexp_prop`) answer OWNED boxes instead — that owned/
//! borrow split across the fallback's arms is the recorded
//! 32B-per-read leak lane (L3b, chunk 716 churn probe; the fix
//! unifies every arm to owned).

use core::ffi::c_void;

use torajs_rc::{AnySlotTag, Tag};

pub(crate) use crate::member_get_own::canonical_index;
use crate::member_get_own::{
    arr_own_pair, array_proto_props, closure_virtual_pair, dynobj_proto_pair, function_proto_props,
    user_proto_cell,
};
use crate::nanbox::{AnyValue, is_null, is_short_str, is_undefined};

unsafe extern "C" {
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — own-entry existence (disambiguates a stored
    /// `undefined` from absent: `get_tag` answers 5 for both).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-arr — expando twin of the dynobj_has probe.
    fn __torajs_arrprops_has(arr: *mut c_void, key: *const c_void) -> i32;
    /// torajs-arr — expando probe through the props slot.
    fn __torajs_arrprops_get_tag(arr: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_arrprops_get_value(arr: *mut c_void, key: *const c_void) -> u64;
    /// torajs-structmeta — read side over `__torajs_class_layouts`
    /// (mirror of `method_call_dynobj`'s declares). The field/accessor
    /// PROBE over a struct cell lives in `struct_probe.rs`; the method
    /// existence test below is the only direct walk left here.
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-regex — boxed-form lastIndex peek (BORROW; 0 = numeric
    /// form) + the numeric f64 getter.
    fn __torajs_regex_last_index_raw(re: *const c_void) -> u64;
    fn __torajs_regex_get_last_index(re: *const c_void) -> f64;
}

// Cell-layout mirrors + tag/flag probes — split to
// `member_get_layout.rs` (file-size HARD RULE); the re-export keeps
// every `crate::member_get::` consumer face unchanged.
pub(crate) use crate::member_get_layout::{
    CLOSURE_PROPS_OFF, STR_DATA_OFF, STR_LEN_OFF, closure_props, header_flag, header_flag_set,
    is_wrapper_tag, promise_props, recv_cell, wrapper_props,
};

/// See module doc.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str or
/// Symbol cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_tag(recv: AnyValue, key: *const c_void) -> u64 {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return 5;
    }
    // §10.5.8 — a Proxy receiver answers the accessor sentinel from
    // BOTH channels without touching the handler; the single [[Get]]
    // happens in `__torajs_any_accessor_get` (RFC 20260823-proxy-
    // substrate 刀 1). Ahead of the symbol split because a `get`
    // trap takes symbol keys too.
    if crate::proxy::is_proxy(recv) {
        return crate::struct_probe::ANY_ACCESSOR_TAG;
    }
    // §6.1.7 — a symbol key takes its own short walk (own dict, then
    // what the receiver inherits); the cascade below is string-keyed.
    if unsafe { crate::member_get_symbol::key_is_symbol(key) } {
        return unsafe { crate::member_get_symbol::symbol_key_pair(recv, key) }.0;
    }
    // §10.4.3 — a ShortStr receiver is an inline NaN-box, not a cell,
    // so it never reaches the match below; its own face answers here
    // (`member_get_str`).
    if is_short_str(recv)
        && let Some((tag, _)) = unsafe { crate::member_get_str::str_own_pair(recv, key) }
    {
        return tag;
    }
    match recv_cell(recv) {
        // Entry miss falls through to the builtin-proto own-method
        // probe (RFC 20260712 chunk 2) — a builtin `<Ctor>.prototype`
        // singleton hands out its interned family cells so
        // `(String.prototype as any).small` reads the same immortal
        // cell the static form does. Ordinary dynobjs answer 0 there.
        Some((ptr, t)) if t == Tag::DynObj as u16 => unsafe { dynobj_arm_tag(ptr, recv, key) },
        Some((ptr, t)) if t == Tag::Arr as u16 => unsafe { arr_arm_tag(ptr, recv, key) },
        Some((ptr, t)) if t == Tag::Closure as u16 => unsafe { closure_arm_tag(ptr, recv, key) },
        // RFC 20260810-sloppy-goal-arguments rotation 353 (plan-state
        // L3b ①) — promise-cell own-property probe via the +32 lazy
        // expando the defineProperty arm writes (rotation 352
        // `478088d4` stored entries only `then`-dispatch could read
        // back; `(p as any).foo` answered undefined). Mirror of the
        // closure arm's bag segment; the miss falls through to the
        // builtin reify (`.then` / `.catch` / `.finally` /
        // `.constructor`).
        Some((ptr, t)) if t == Tag::Promise as u16 => unsafe {
            let props = promise_props(ptr);
            if !props.is_null() {
                let tag = __torajs_dynobj_get_tag(props, key);
                if tag != 5 {
                    return tag;
                }
                // Stored-undefined expando shadows the builtin reify.
                if __torajs_dynobj_has(props, key) != 0 {
                    return 5;
                }
            }
            reify_tag(recv, key)
        },
        // RFC 20260716 刀 5 (rotation 121 chunk 4) — wrapper cell
        // own-property probe via the +16 lazy expando (mirror of the
        // closure arm above). Miss falls through to `reify_tag`,
        // which handles the wrapper's inherited built-in surface
        // (`.valueOf` / `.toString` / `.length` on StringWrapper etc.)
        // via the per-wrapper method tables.
        Some((ptr, t)) if is_wrapper_tag(t) => unsafe {
            crate::member_get_own::wrapper_arm_tag(ptr, key, t, recv)
        },
        // §22.2.4.1 — a RegExp instance owns exactly `lastIndex`; a
        // DYNAMIC key spelling it must answer like the static hint
        // lane (`any_regexp_prop`). Boxed verbatim form (non-numeric
        // any-lane store) unboxes to the pair (borrow — the cell's
        // slot keeps the stake, same convention as the dynobj
        // bucket); numeric form is an F64 pair. Any other key falls
        // to the builtin-method reify probe.
        // §25.1.6 — the four ArrayBuffer.prototype accessors read
        // as a value, so they answer on the probe pair. Only the
        // [[Get]] face: own-key enumeration is a different kernel
        // and still says a buffer owns nothing.
        // §25.1.6 / §23.2.3 — the buffer family's accessors read as
        // values, so they answer on the probe pair. Only the [[Get]]
        // face: own-key enumeration is a different kernel and still
        // says a buffer owns nothing.
        Some((_, t)) if crate::member_get_buffer::is_buffer_family(t) => unsafe {
            match crate::member_get_buffer::buffer_family_prop(recv, key, t) {
                Some((tag, _)) => tag,
                None => reify_tag(recv, key),
            }
        },
        Some((ptr, t)) if t == Tag::RegExp as u16 => unsafe { regexp_arm_tag(ptr, key, recv) },
        // Chunk 744 — struct cell: class-layout field probe before
        // the builtin reify (a struct has no builtin methods, so a
        // field miss falling through is exact).
        Some((ptr, t)) if t == Tag::Obj as u16 => unsafe {
            if let Some((tag, _)) = crate::struct_probe::struct_field_pair(ptr, key) {
                // An absent error `message` / `name` reads through
                // the prototype chain (§10.1.8.1 step 3; `__proto_Error`
                // carries the spec `""` and each `__proto_<C>` its own
                // `name`). Same live key drives both the miss test and
                // the chain walk.
                if crate::struct_error_msg::error_absent_key(ptr, key) {
                    return crate::struct_error_msg::struct_proto_chain_pair(ptr, key).0;
                }
                return tag;
            }
            // Blade 5 — an accessor property answers the sentinel; the
            // probe pair must NOT invoke (it runs twice, once per
            // channel). The emitted accessor arm does the single
            // [[Get]] through `__torajs_any_accessor_get`.
            if crate::struct_probe::struct_accessor_key(ptr, key) {
                return crate::struct_probe::ANY_ACCESSOR_TAG;
            }
            // RFC 20260714-struct-dynamic-props blade 2 — the +24
            // expando dict is an OWN face: it answers before the
            // prototype chain (§10.1.8.1 own-first). NULL slot =
            // never written, fall through.
            let props = crate::member_get_layout::struct_props(ptr);
            if !props.is_null() && __torajs_dynobj_has(props, key) != 0 {
                return __torajs_dynobj_get_tag(props, key);
            }
            // L3b ⑧ — an own-face miss reads through the class
            // prototype chain (§10.1.8.1 step 3): the reified method
            // face, the wired `constructor`, a prototype expando. A
            // fully missing chain keeps the undefined answer.
            let (ptag, pval) = crate::struct_error_msg::struct_proto_chain_pair(ptr, key);
            if ptag != AnySlotTag::Undef as u64 || pval != 0 {
                return ptag;
            }
            reify_tag(recv, key)
        },
        // §20.4.3.2 get Symbol.prototype.description — the desc Str
        // (borrow-shaped like every probe answer; the stake lives on
        // the symbol cell), undefined for `Symbol()`.
        Some((ptr, t)) if t == Tag::Symbol as u16 => unsafe {
            if crate::prop_has::key_is(key, b"description") {
                if crate::member_get_layout::symbol_desc(ptr).is_null() {
                    return AnySlotTag::Undef as u64;
                }
                return AnySlotTag::Heap as u64;
            }
            reify_tag(recv, key)
        },
        // §10.4.3 — a heap Str / Substr receiver's own face: `length`
        // and the canonical index domain. A miss (a method name, an
        // index past the end) falls to the reify tail.
        Some((_, t)) if t == Tag::Str as u16 => unsafe {
            if let Some((tag, _)) = crate::member_get_str::str_own_pair(recv, key) {
                return tag;
            }
            reify_tag(recv, key)
        },
        _ => unsafe { reify_tag(recv, key) },
    }
}

/// The `Tag::Closure` arm — the lazy expando bag, the virtual
/// name/length pair, the plain-fn `.prototype` materialization (RFC
/// 20260721 刀 9 — writes into props, so the value twin and every
/// later read hit the props probe), the user [[Prototype]] chain
/// (405-01 — a re-parented function value answers through it before
/// the implicit %Function.prototype% surface, and an explicit null
/// ends the chain, §10.1.8.1), the inherited Function.prototype
/// expando (monkey-patches land in the tag-13 singleton dynobj), and
/// the reify tail. Extracted under the 200-line function rule when
/// the chain hop landed.
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell; `key` is a live Str cell;
/// `recv` NaN-boxes the closure.
unsafe fn closure_arm_tag(ptr: *mut c_void, recv: AnyValue, key: *const c_void) -> u64 {
    unsafe {
        let props = closure_props(ptr);
        if !props.is_null() {
            let tag = __torajs_dynobj_get_tag(props, key);
            if tag != 5 {
                return tag;
            }
            // Stored-undefined expando shadows the virtual
            // name/length pair and the builtin reify.
            if __torajs_dynobj_has(props, key) != 0 {
                return 5;
            }
        }
        if let Some((tag, _)) = closure_virtual_pair(ptr, key) {
            return tag;
        }
        if crate::prop_has::key_is(key, b"prototype")
            && let Some((tag, _)) = crate::closure_proto::fn_prototype_pair(ptr)
        {
            return tag;
        }
        match crate::member_get_own::closure_user_proto(ptr) {
            Some(Some(parent)) => return __torajs_any_member_get_tag(parent, key),
            Some(None) => return 5,
            None => {}
        }
        let fp = function_proto_props();
        if !fp.is_null() {
            let tag = __torajs_dynobj_get_tag(fp, key);
            if tag != 5 {
                return tag;
            }
            if __torajs_dynobj_has(fp, key) != 0 {
                return 5;
            }
        }
        reify_tag(recv, key)
    }
}

/// `Tag::Arr` `[[GetOwnProperty]]` tag probe — own index / `length`
/// domain, then the expando bag, the inherited `Array.prototype`
/// expando, and the singleton's own interned surface, before the
/// builtin-method reify tail. Extracted from the cascade under the
/// 200-line function rule when the §10.4.3 Str arm landed, mirroring
/// [`wrapper_arm_tag`].
///
/// # Safety
/// `ptr` is a live `Tag::Arr` cell; `key` is a live Str cell; `recv`
/// NaN-boxes the array.
unsafe fn arr_arm_tag(ptr: *mut c_void, recv: AnyValue, key: *const c_void) -> u64 {
    unsafe {
        if let Some((tag, _)) = arr_own_pair(ptr, key) {
            return tag;
        }
        let tag = __torajs_arrprops_get_tag(ptr, key);
        if tag != 5 {
            return tag;
        }
        // Stored-undefined expando shadows the builtin surface
        // (`arr.join = undefined` reads undefined, not the reified
        // join cell).
        if __torajs_arrprops_has(ptr, key) != 0 {
            return 5;
        }
        // Inherited Array.prototype expando (tag-2 singleton).
        let ap = array_proto_props();
        if !ap.is_null() {
            let tag = __torajs_dynobj_get_tag(ap, key);
            if tag != 5 {
                return tag;
            }
            if __torajs_dynobj_has(ap, key) != 0 {
                return 5;
            }
        }
        // The Array.prototype singleton is an Arr CELL, so a read of
        // its own interned surface (`constructor`, the family
        // methods) lands in this arm, not the dynobj one — same
        // synthesis probe (rotation 131).
        if crate::method_support::__torajs_builtin_proto_own_method_cell(ptr, key) != 0 {
            return 4;
        }
        reify_tag(recv, key)
    }
}

/// Builtin-method reification probe (chunk 711) — a supported
/// method name on a builtin receiver answers a heap tag (the
/// interned function cell); everything else stays absent.
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn reify_tag(recv: AnyValue, key: *const c_void) -> u64 {
    // L3b ④ — `.constructor` answers the receiver family's interned
    // builtin-constructor cell (own-face shadows already probed).
    if unsafe { crate::method_value::ctor_cell_for_recv(recv, key) }.is_some() {
        return 4;
    }
    // RFC 20260721 刀 3 — a builtin ctor cell answers its table
    // statics / `prototype` / Number data constants as own reads
    // (borrow-shaped, matching the pair protocol).
    if let Some((ptr, t)) = recv_cell(recv)
        && t == Tag::Closure as u16
        && let Some((tag, _)) = unsafe { crate::method_value::ctor_own_read_cell(ptr, key) }
    {
        return tag;
    }
    if unsafe { crate::method_value::builtin_method_lookup(recv, key) }.is_some() {
        4
    } else {
        5
    }
}

/// DynObj-arm builtin tail (tag channel) — the own-method reify,
/// Function.prototype's virtual meta pair (§20.2.3, RFC 20260722
/// 刀 3), then the inherited Object.prototype reify (valueOf /
/// toLocaleString / the universal probes), same fallthrough as the
/// Arr / Closure / struct arms.
unsafe fn dynobj_builtin_tail_tag(ptr: *mut c_void, recv: AnyValue, key: *const c_void) -> u64 {
    unsafe {
        if crate::method_support::__torajs_builtin_proto_own_method_cell(ptr, key) != 0 {
            return 4;
        }
        if let Some((mtag, _)) = crate::method_support_proto_meta::builtin_proto_own_meta(ptr, key)
        {
            return mtag;
        }
        reify_tag(recv, key)
    }
}

// Value channel — split to `member_get_value.rs` (file-size HARD
// RULE); the re-export keeps every `crate::member_get::` consumer
// face unchanged.
pub(crate) use crate::member_get_value::__torajs_any_member_get_value;

/// §22.2.4.1 — a RegExp instance owns exactly `lastIndex`, and the
/// tag channel answers it from the raw slot. Lifted out of the
/// dispatch match in the `closure_arm_tag` shape, so the match stays
/// a table of one-liners.
///
/// # Safety
/// `ptr` is a live RegExp cell; `key` is NULL or a live Str cell;
/// `recv` boxes `ptr`.
unsafe fn regexp_arm_tag(ptr: *mut c_void, key: *const c_void, recv: AnyValue) -> u64 {
    unsafe {
        if crate::prop_has::key_is(key, b"lastIndex") {
            let raw = __torajs_regex_last_index_raw(ptr);
            if raw != 0 {
                return crate::__torajs_anyv_unbox_tag(raw) as u64;
            }
            return AnySlotTag::F64 as u64;
        }
        reify_tag(recv, key)
    }
}

/// The dynobj own-then-chain-then-reify walk, lifted out of the
/// dispatch match in the `arr_arm_tag` / `closure_arm_tag` shape.
/// The order inside is load-bearing and is documented step by step
/// where each step sits.
///
/// # Safety
/// `ptr` is a live DynObj cell; `key` is NULL or a live Str cell;
/// `recv` boxes `ptr`.
unsafe fn dynobj_arm_tag(ptr: *mut c_void, recv: AnyValue, key: *const c_void) -> u64 {
    unsafe {
        let tag = __torajs_dynobj_get_tag(ptr, key);
        if tag != 5 {
            return tag;
        }
        // An own entry STORING undefined shadows the proto
        // surface (`o.toString = undefined` must not reify the
        // builtin) — `get_tag` answers 5 for both shapes, the
        // has probe disambiguates (777e756c's read-side leg).
        if __torajs_dynobj_has(ptr, key) != 0 {
            return 5;
        }
        // Annex B §B.2.2.1 — a dynamic key spelling `__proto__`
        // answers the RECEIVER's [[Prototype]] via the inherited
        // accessor (the own DATA probe above already covered the
        // shadow case); the chain walk below must not run — the
        // internal simulation slot no longer carries the
        // user-spellable name, so the walk would miss to the
        // reify surface.
        if crate::prop_has::key_is(key, b"__proto__") {
            return dynobj_proto_pair(ptr).0;
        }
        // Knife 2 — the user [[Prototype]] chain answers before
        // the builtin surface reifies (§10.1.8.1 OrdinaryGet
        // walks the chain; the recursion covers grandparents).
        if let Some(parent) = user_proto_cell(ptr) {
            return __torajs_any_member_get_tag(parent, key);
        }
        // §10.1.8.1 OrdinaryGet step 2 — an explicit null proto
        // cuts the chain: no builtin reify surface either.
        if crate::member_get_own::dynobj_null_proto(ptr) {
            return 5;
        }
        // RFC 20260807-global-object G2 — a KNOWN builtin the
        // globalThis singleton's fill list is missing must stay
        // LOUD (bun answers a function; a silent undefined here
        // would mask it). Unknown names keep the ordinary miss.
        // None of the missing names collides with the builtin
        // tails below, so probing first loses nothing.
        if crate::method_value::globalthis_object::globalthis_missing_known(ptr, key) {
            __torajs_throw_type_error(
                c"not yet supported: this builtin is not reachable through globalThis".as_ptr(),
            );
            return 5;
        }
        dynobj_builtin_tail_tag(ptr, recv, key)
    }
}
