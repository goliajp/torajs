//! `AccessorPair` — heap object backing a dynobj property defined with
//! a `{ get, set }` descriptor (RFC C3 accessor substrate).
//!
//! A property defined via `Object.defineProperty(o, k, { get, set })`
//! stores, in its dense-entry `value_anyv`, a **cell** pointing at an
//! `AccessorPair` rather than a data value. Reads/writes of the
//! property dispatch through the getter/setter closures instead of the
//! stored value. This is the V8 / JSC AccessorPair model: the NaN-box
//! `value_anyv` carries no free tag bits (a heap pointer is just a
//! cell), so accessor-ness is resolved by reading the pointee's
//! `HeapHeader::type_tag` ([`torajs_rc::Tag::AccessorPair`] = 18),
//! exactly like every other heap type.
//!
//! ```text
//! offset | size | field
//! -------|------|------
//!   0    |  8B  | universal heap header (refcount + type_tag=18 + flags)
//!   8    |  8B  | get_closure  (closure env ptr; 0 = no getter)
//!  16    |  8B  | set_closure  (closure env ptr; 0 = no setter)
//!  24    |  8B  | kinds        (byte 0 = getter ret kind, byte 1 = setter param kind)
//! ```
//!
//! The getter/setter are ordinary closures (env-first ABI, fn_ptr at
//! `+8`, drop_fn at `+16`). The `kinds` word records each closure's
//! native value-ABI class so the runtime invoke path (RFC C3b) can
//! box the getter's raw return / unbox the setter's argument across
//! the register-class boundary (an f64-returning getter leaves its
//! value in `d0`, an i64-returning one in `x0`).

use core::ffi::c_void;

unsafe extern "C" {
    /// torajs-mmalloc libc-compat calloc — zero-init alloc.
    #[link_name = "__torajs_calloc"]
    fn calloc(size: usize) -> *mut c_void;
    /// torajs-mmalloc libc-compat free — sized.
    #[link_name = "__torajs_free"]
    fn free(p: *mut c_void, size: usize);
    /// Cross-tier — refcount dec. Returns 1 iff the caller should free
    /// + release children; 0 otherwise.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    /// torajs-anyvalue — NaN-box a `(tag, value)` pair into an
    /// `AnyValue`. `tag`: 0=Null 1=Bool 2=I64 3=F64(bits) 4=Heap(ptr).
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// torajs-anyvalue — extract the payload (per-tag value) from a
    /// NaN-box `AnyValue` (F64 returns the bits, Heap the pointer).
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-anyvalue — borrow-shaped cell-pointer read: a heap cell
    /// answers its pointer bits, every immediate (ShortStr included)
    /// answers 0 with zero materialization.
    fn __torajs_anyv_cell_ptr(v: u64) -> i64;
}

/// True when an entry's `value_anyv` is an `AccessorPair` cell — the
/// single accessor-detection predicate shared by the GET (`get.rs`)
/// and SET (`set.rs`) paths. The cell gate answers 0 for every
/// immediate — ShortStr included, which matters for the struct-desc
/// lane's borrowed field reads (the pre-fix `unbox_tag` + `unbox_value`
/// probe materialized a ShortStr into an owned Str it then discarded).
///
/// # Safety
/// `v_anyv` is an entry / descriptor-field value; if it is a cell its
/// payload is a live heap pointer.
pub(crate) unsafe fn value_is_accessor(v_anyv: u64) -> bool {
    let ptr = unsafe { __torajs_anyv_cell_ptr(v_anyv) } as *const c_void;
    !ptr.is_null() && unsafe { *((ptr as *const u8).add(4) as *const u16) } == TAG_ACCESSOR_PAIR
}

/// Total `AccessorPair` heap-block size.
pub const ACCESSOR_PAIR_SIZE: usize = 32;

/// Offset of `get_closure` within the block.
pub const ACC_GET_OFF: usize = 8;
/// Offset of `set_closure` within the block.
pub const ACC_SET_OFF: usize = 16;
/// Offset of the packed `kinds` word within the block.
pub const ACC_KINDS_OFF: usize = 24;

/// `type_tag` value for `AccessorPair` blocks. Mirrors
/// `torajs_rc::Tag::AccessorPair` = 18 (same hardcode-with-comment
/// pattern the rest of dynobj uses for `TAG_DYNOBJ` — torajs-dynobj
/// does not depend on torajs-rc). Updates to the Tag enum require a
/// mirroring edit here.
pub const TAG_ACCESSOR_PAIR: u16 = 18;

/// Closure layout — mirrored from `ssa_lower`'s `CLOSURE_*_OFF`
/// constants (the env-first closure ABI is a shared cross-tier
/// contract; same mirror pattern dynobj uses for the Str layout).
const CLOSURE_FN_ADDR_OFF: usize = 8;

// Accessor closure value-ABI kinds — the low byte of `kinds` records
// the getter's native SSA return type so the invoke path reads the
// correct return register and boxes accordingly. ssa_lower writes
// these at define time (RFC C3b) from the getter closure's signature.
//
/// Getter returns an `AnyValue` (already NaN-boxed) — pass through.
pub const ACC_KIND_ANY: u8 = 0;
/// Getter returns `I64` / `I32` — box as tag 2.
pub const ACC_KIND_I64: u8 = 1;
/// Getter returns `F64` (in `d0`) — bitcast + box as tag 3.
pub const ACC_KIND_F64: u8 = 2;
/// Getter returns `Bool` — box as tag 1.
pub const ACC_KIND_BOOL: u8 = 3;
/// Getter returns a refcounted heap pointer — a raw cell is already a
/// valid `AnyValue`; box as tag 4 (heap) so null collapses to `null`.
pub const ACC_KIND_PTR: u8 = 4;
/// Runtime-descriptor closures (RFC 20260712 chunk 2) — signature
/// unknown at define time, so invoke through the closure's boxed
/// dual entry (uniform `(env, argv, argc) -> AnyValue` ABI at cell
/// `+32`, same channel as any-world calls). A closure without an
/// adapter (entry 0) answers `undefined` / no-ops the setter.
pub const ACC_KIND_BOXED: u8 = 5;
/// Getter returns nothing — a throw-only / no-`return` getter lowers
/// to a native void fn (x0 is never written), so the invoke must NOT
/// read the return register: call through the void shape and answer
/// `undefined` (a JS getter with no return yields undefined). Pre-fix
/// these fell into the ANY catch-all and read x0 garbage — a
/// throwing `length` getter under an Array generic method SIGSEGV'd
/// in `__torajs_value_drop_heap` (RFC 20260713-accessor-void-kind).
pub const ACC_KIND_VOID: u8 = 6;
/// Kind-byte flag bit (OR'd onto a base kind): the face is a named
/// top-level fn behind a zero-capture env cell — its native signature
/// carries NO leading env param, so the invoke transmutes to the
/// env-less shape (RFC 20260713 chunk D; a getter tolerates the
/// spurious env argument either way, a setter's value would land in
/// the wrong register without this).
pub const ACC_KIND_NAKED: u8 = 0x80;
/// Kind-byte flag bit (OR'd onto a base kind, disjoint from
/// [`ACC_KIND_NAKED`] and the 0-6 kind values): the face closure's
/// lifted body takes the call-site `this` as its first declared param
/// (`FLAG_CLOSURE_RECV_FIRST` fn-expr faces, RFC
/// 20260717-fnexpr-this-channel knife 1). Invoke rides the boxed dual
/// entry with the receiver in argv[0] (getter argc 1; setter argc 2,
/// value at argv[1]). Faces without the bit are invoked exactly as
/// before — the receiver argument is ignored.
pub const ACC_KIND_RECV: u8 = 0x40;

/// Closure-cell boxed dual entry offset — ABI mirror of
/// `torajs_anyvalue`'s `CLOSURE_BOXED_ENTRY_OFF`.
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;

/// `undefined` NaN-box sentinel (mirrors
/// `torajs_anyvalue::nanbox::VALUE_UNDEFINED` = 0x0A). Returned when an
/// accessor has no getter (`get` omitted).
const VALUE_UNDEFINED: u64 = 0x0A;

/// `__torajs_accessor_pair_new(get_closure, set_closure, kinds)` —
/// allocate a fresh `+1`-rc `AccessorPair`. `get_closure` /
/// `set_closure` are closure env pointers (0 when the descriptor omits
/// that accessor); ownership of each closure ref transfers into the
/// pair (released by [`__torajs_accessor_drop`]). `kinds` packs the
/// getter ret kind (byte 0) and setter param kind (byte 1).
///
/// # Safety
/// `get_closure` / `set_closure` are null or live closure heap
/// pointers whose ref is being moved into the pair.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_accessor_pair_new(
    get_closure: *mut c_void,
    set_closure: *mut c_void,
    kinds: u64,
) -> *mut c_void {
    let p = unsafe { calloc(ACCESSOR_PAIR_SIZE) } as *mut u8;
    unsafe {
        // Header: rc=1, tag=AccessorPair, flags=0.
        *(p as *mut u32) = 1;
        *(p.add(4) as *mut u16) = TAG_ACCESSOR_PAIR;
        *(p.add(6) as *mut u16) = 0;
        *(p.add(ACC_GET_OFF) as *mut u64) = get_closure as u64;
        *(p.add(ACC_SET_OFF) as *mut u64) = set_closure as u64;
        *(p.add(ACC_KINDS_OFF) as *mut u64) = kinds;
    }
    p as *mut c_void
}

/// `__torajs_accessor_get_getter(pair)` — the getter closure pointer
/// (0 when absent). Used by `getOwnPropertyDescriptor` to populate the
/// descriptor's `get` field.
///
/// # Safety
/// `pair` is null or a live `AccessorPair` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_accessor_get_getter(pair: *const c_void) -> *mut c_void {
    if pair.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { *((pair as *const u8).add(ACC_GET_OFF) as *const u64) as *mut c_void }
}

/// `__torajs_accessor_get_setter(pair)` — the setter closure pointer
/// (0 when absent).
///
/// # Safety
/// `pair` is null or a live `AccessorPair` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_accessor_get_setter(pair: *const c_void) -> *mut c_void {
    if pair.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { *((pair as *const u8).add(ACC_SET_OFF) as *const u64) as *mut c_void }
}

/// `__torajs_accessor_get_kinds(pair)` — the packed `kinds` word.
///
/// # Safety
/// `pair` is null or a live `AccessorPair` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_accessor_get_kinds(pair: *const c_void) -> u64 {
    if pair.is_null() {
        return 0;
    }
    unsafe { *((pair as *const u8).add(ACC_KINDS_OFF) as *const u64) }
}

/// `__torajs_accessor_invoke_getter(pair, recv_anyv)` — call the
/// getter closure and return its result as an `AnyValue`. The getter
/// is invoked with the env-first closure ABI and **no** user argument
/// (a getter takes none). Its raw return lands in the register bank
/// dictated by its native SSA return type, so the packed getter ret
/// kind selects the matching fn-pointer transmute (an `F64` getter
/// leaves its value in `d0`, an `I64` one in `x0`) and the matching
/// box. A pair with no getter yields `undefined`.
///
/// `this` binding: `recv_anyv` is the NaN-boxed property-read
/// receiver, BORROWED into the call. An [`ACC_KIND_RECV`] face (a
/// fn-expr accessor whose body says `this`) rides the boxed dual
/// entry with the receiver in argv[0]; every other face ignores it
/// (the closure-capture accessor idiom carries no `this`).
///
/// # Safety
/// `pair` is null or a live `AccessorPair`; its `get_closure` (when
/// present) is a live closure whose fn at `+8` has the env-first ABI
/// and the return type encoded by the getter ret kind; `recv_anyv` is
/// a live receiver or `undefined`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_accessor_invoke_getter(
    pair: *const c_void,
    recv_anyv: u64,
) -> u64 {
    if pair.is_null() {
        return VALUE_UNDEFINED;
    }
    let getter = unsafe { *((pair as *const u8).add(ACC_GET_OFF) as *const u64) } as *mut c_void;
    if getter.is_null() {
        return VALUE_UNDEFINED;
    }
    let kinds = unsafe { *((pair as *const u8).add(ACC_KINDS_OFF) as *const u64) };
    let raw_ret = (kinds & 0xff) as u8;
    if raw_ret & ACC_KIND_RECV != 0 {
        // Receiver-first fn-expr face — boxed dual entry, argv[0] is
        // the receiver the body reads as `this`.
        let entry = unsafe { *((getter as *const u8).add(CLOSURE_BOXED_ENTRY_OFF) as *const u64) };
        if entry == 0 {
            return VALUE_UNDEFINED;
        }
        let call: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
            unsafe { core::mem::transmute(entry as usize) };
        let mut buf = [VALUE_UNDEFINED; 8];
        buf[0] = recv_anyv;
        return unsafe { call(getter, buf.as_ptr(), 1) };
    }
    let ret_kind = raw_ret & !ACC_KIND_NAKED;
    // A NAKED getter's env argument is spurious either way (the fn
    // reads no params) — the env-first transmutes below stay correct
    // for both shapes, so the flag only matters for the setter.
    if ret_kind == ACC_KIND_BOXED {
        let entry = unsafe { *((getter as *const u8).add(CLOSURE_BOXED_ENTRY_OFF) as *const u64) };
        if entry == 0 {
            return VALUE_UNDEFINED;
        }
        let call: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
            unsafe { core::mem::transmute(entry as usize) };
        // Zero-arg call still hands the adapter a live argv buffer —
        // it reads its declared param slots unconditionally.
        let buf = [VALUE_UNDEFINED; 8];
        return unsafe { call(getter, buf.as_ptr(), 0) };
    }
    let fn_addr = unsafe { *((getter as *const u8).add(CLOSURE_FN_ADDR_OFF) as *const usize) };
    unsafe {
        match ret_kind {
            ACC_KIND_F64 => {
                let f: unsafe extern "C" fn(*mut c_void) -> f64 = core::mem::transmute(fn_addr);
                __torajs_anyv_box_from_pair(3, f(getter).to_bits() as i64)
            }
            ACC_KIND_BOOL => {
                let f: unsafe extern "C" fn(*mut c_void) -> i64 = core::mem::transmute(fn_addr);
                __torajs_anyv_box_from_pair(1, i64::from(f(getter) != 0))
            }
            ACC_KIND_I64 => {
                let f: unsafe extern "C" fn(*mut c_void) -> i64 = core::mem::transmute(fn_addr);
                __torajs_anyv_box_from_pair(2, f(getter))
            }
            ACC_KIND_PTR => {
                let f: unsafe extern "C" fn(*mut c_void) -> u64 = core::mem::transmute(fn_addr);
                __torajs_anyv_box_from_pair(4, f(getter) as i64)
            }
            ACC_KIND_VOID => {
                let f: unsafe extern "C" fn(*mut c_void) = core::mem::transmute(fn_addr);
                f(getter);
                VALUE_UNDEFINED
            }
            // ACC_KIND_ANY (and any unknown): the getter already
            // returns a NaN-box AnyValue — pass it through verbatim.
            _ => {
                let f: unsafe extern "C" fn(*mut c_void) -> u64 = core::mem::transmute(fn_addr);
                f(getter)
            }
        }
    }
}

/// `__torajs_accessor_invoke_setter(pair, recv_anyv, value_anyv)` —
/// call the setter closure with the assigned value, returning `1` when
/// a setter ran and `0` when the accessor has no setter (the caller
/// raises the "only a getter" assignment error). The setter is invoked
/// env-first with the value unboxed per the stored setter param kind
/// (an `F64` setter takes its argument in `d0`, an `I64` one in `x0`);
/// an `Any` setter receives the NaN-box `AnyValue` verbatim.
///
/// `this` binding: `recv_anyv` is the NaN-boxed assignment receiver,
/// BORROWED into the call. An [`ACC_KIND_RECV`] face rides the boxed
/// dual entry with the receiver in argv[0] and the value at argv[1];
/// every other face ignores it.
///
/// # Safety
/// `pair` is null or a live `AccessorPair`; its `set_closure` (when
/// present) is a live closure whose fn at `+8` has the env-first ABI
/// and the param type encoded by the setter param kind; `recv_anyv` is
/// a live receiver or `undefined`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_accessor_invoke_setter(
    pair: *const c_void,
    recv_anyv: u64,
    value_anyv: u64,
) -> i32 {
    if pair.is_null() {
        return 0;
    }
    let setter = unsafe { *((pair as *const u8).add(ACC_SET_OFF) as *const u64) } as *mut c_void;
    if setter.is_null() {
        return 0;
    }
    let kinds = unsafe { *((pair as *const u8).add(ACC_KINDS_OFF) as *const u64) };
    let raw_kind = ((kinds >> 8) & 0xff) as u8;
    if raw_kind & ACC_KIND_RECV != 0 {
        // Receiver-first fn-expr face — boxed dual entry, argv[0] is
        // the receiver the body reads as `this`, argv[1] the value.
        let entry = unsafe { *((setter as *const u8).add(CLOSURE_BOXED_ENTRY_OFF) as *const u64) };
        if entry == 0 {
            return 1;
        }
        let call: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
            unsafe { core::mem::transmute(entry as usize) };
        let mut buf = [VALUE_UNDEFINED; 8];
        buf[0] = recv_anyv;
        buf[1] = value_anyv;
        unsafe { call(setter, buf.as_ptr(), 2) };
        return 1;
    }
    let naked = raw_kind & ACC_KIND_NAKED != 0;
    let param_kind = raw_kind & !ACC_KIND_NAKED;
    if naked {
        // Named top-level fn — no leading env param; the value is
        // its FIRST argument.
        let fn_addr = unsafe { *((setter as *const u8).add(CLOSURE_FN_ADDR_OFF) as *const usize) };
        unsafe {
            match param_kind {
                ACC_KIND_F64 => {
                    let v = f64::from_bits(__torajs_anyv_unbox_value(value_anyv) as u64);
                    let f: unsafe extern "C" fn(f64) = core::mem::transmute(fn_addr);
                    f(v);
                }
                ACC_KIND_BOOL | ACC_KIND_I64 => {
                    let f: unsafe extern "C" fn(i64) = core::mem::transmute(fn_addr);
                    f(__torajs_anyv_unbox_value(value_anyv));
                }
                ACC_KIND_PTR => {
                    let f: unsafe extern "C" fn(*mut c_void) = core::mem::transmute(fn_addr);
                    f(__torajs_anyv_unbox_value(value_anyv) as *mut c_void);
                }
                _ => {
                    let f: unsafe extern "C" fn(u64) = core::mem::transmute(fn_addr);
                    f(value_anyv);
                }
            }
        }
        return 1;
    }
    if param_kind == ACC_KIND_BOXED {
        let entry = unsafe { *((setter as *const u8).add(CLOSURE_BOXED_ENTRY_OFF) as *const u64) };
        if entry == 0 {
            return 1;
        }
        let call: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
            unsafe { core::mem::transmute(entry as usize) };
        let mut buf = [VALUE_UNDEFINED; 8];
        buf[0] = value_anyv;
        unsafe { call(setter, buf.as_ptr(), 1) };
        return 1;
    }
    let fn_addr = unsafe { *((setter as *const u8).add(CLOSURE_FN_ADDR_OFF) as *const usize) };
    unsafe {
        match param_kind {
            ACC_KIND_F64 => {
                let v = f64::from_bits(__torajs_anyv_unbox_value(value_anyv) as u64);
                let f: unsafe extern "C" fn(*mut c_void, f64) = core::mem::transmute(fn_addr);
                f(setter, v);
            }
            ACC_KIND_BOOL | ACC_KIND_I64 => {
                let v = __torajs_anyv_unbox_value(value_anyv);
                let f: unsafe extern "C" fn(*mut c_void, i64) = core::mem::transmute(fn_addr);
                f(setter, v);
            }
            ACC_KIND_PTR => {
                let v = __torajs_anyv_unbox_value(value_anyv) as *mut c_void;
                let f: unsafe extern "C" fn(*mut c_void, *mut c_void) =
                    core::mem::transmute(fn_addr);
                f(setter, v);
            }
            // ACC_KIND_ANY (and any unknown): the setter takes an
            // AnyValue — pass the NaN-box through verbatim.
            _ => {
                let f: unsafe extern "C" fn(*mut c_void, u64) = core::mem::transmute(fn_addr);
                f(setter, value_anyv);
            }
        }
    }
    1
}

/// Release ONE held closure ref through the rc-gated universal drop
/// dispatcher (its `Tag::Closure` arm decs and only invokes the
/// env's `drop_fn` on hit-zero). The pre-fix body called `drop_fn`
/// directly — correct while the pair was provably the cell's sole
/// owner, but a canonical named-fn singleton (RFC 20260717-namedfn-
/// canonical-cell) sits in pairs at rc ≥ 2 (hidden-slot stake +
/// pair stake) and the ungated call freed it out from under the
/// slot: every later use read freed memory and every later pair
/// teardown re-freed the block (diag: teardown rc climbed 2/3/4
/// across iterations on the same freed cell). Null is a no-op
/// inside the dispatcher.
///
/// # Safety
/// `closure` is null or a closure heap pointer holding a ref the
/// pair owns.
#[inline]
unsafe fn drop_closure_value(closure: *mut c_void) {
    unsafe extern "C" {
        fn __torajs_value_drop_heap(p: *mut c_void);
    }
    unsafe { __torajs_value_drop_heap(closure) };
}

/// `__torajs_accessor_drop(pair)` — refcount dec; on hit-zero release
/// the getter/setter closures then free the block. Wired into
/// `__torajs_value_drop_heap`'s [`Tag::AccessorPair`] dispatch so a
/// dynobj entry walk (or any cell drop) reaches it.
///
/// # Safety
/// `pair` is null or a live `AccessorPair` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_accessor_drop(pair: *mut c_void) {
    if pair.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(pair) } == 0 {
        return;
    }
    unsafe {
        let getter = *((pair as *const u8).add(ACC_GET_OFF) as *const u64) as *mut c_void;
        let setter = *((pair as *const u8).add(ACC_SET_OFF) as *const u64) as *mut c_void;
        drop_closure_value(getter);
        drop_closure_value(setter);
        free(pair, ACCESSOR_PAIR_SIZE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering};

    /// A minimal fake closure block: 24 bytes is enough to hold the
    /// header + fn_ptr (+8) + drop_fn (+16). `drop_fn` points at a
    /// test hook that records the drop and frees nothing (the test
    /// owns the box).
    #[repr(C)]
    struct FakeClosure {
        header: u64,
        fn_ptr: u64,
        drop_fn: u64,
    }

    static DROP_HITS: AtomicU32 = AtomicU32::new(0);

    unsafe extern "C" fn record_drop(_closure: *mut c_void) {
        DROP_HITS.fetch_add(1, Ordering::Relaxed);
    }

    /// `record_drop`'s address as the `u64` a closure's `drop_fn` slot
    /// holds (cast through a pointer per `function_casts_as_integer`).
    fn record_drop_ptr() -> u64 {
        record_drop as unsafe extern "C" fn(*mut c_void) as *const () as u64
    }

    #[test]
    fn new_sets_header_and_fields() {
        let kinds = 0x0201u64; // get ret kind = 1, set param kind = 2
        let get = 0xAAAA_0000u64 as *mut c_void;
        let set = 0xBBBB_0000u64 as *mut c_void;
        let p = unsafe { __torajs_accessor_pair_new(get, set, kinds) } as *mut u8;
        assert!(!p.is_null());
        unsafe {
            assert_eq!(*(p as *const u32), 1, "refcount");
            assert_eq!(*(p.add(4) as *const u16), TAG_ACCESSOR_PAIR, "type_tag");
            assert_eq!(*(p.add(6) as *const u16), 0, "flags");
            assert_eq!(*(p.add(ACC_GET_OFF) as *const u64), get as u64, "get");
            assert_eq!(*(p.add(ACC_SET_OFF) as *const u64), set as u64, "set");
            assert_eq!(*(p.add(ACC_KINDS_OFF) as *const u64), kinds, "kinds");

            assert_eq!(__torajs_accessor_get_getter(p as *const c_void), get);
            assert_eq!(__torajs_accessor_get_setter(p as *const c_void), set);
            assert_eq!(__torajs_accessor_get_kinds(p as *const c_void), kinds);

            free(p as *mut c_void, ACCESSOR_PAIR_SIZE);
        }
    }

    #[test]
    fn getters_null_safe() {
        unsafe {
            assert!(__torajs_accessor_get_getter(core::ptr::null()).is_null());
            assert!(__torajs_accessor_get_setter(core::ptr::null()).is_null());
            assert_eq!(__torajs_accessor_get_kinds(core::ptr::null()), 0);
        }
    }

    #[test]
    fn drop_releases_both_closures() {
        DROP_HITS.store(0, Ordering::Relaxed);
        let mut getter = FakeClosure {
            // rc = 1 (u32 at +0) — the pair holds the sole test ref;
            // the gated teardown decs it to zero then fires drop_fn.
            header: 1,
            fn_ptr: 0,
            drop_fn: record_drop_ptr(),
        };
        let mut setter = FakeClosure {
            // rc = 1 (u32 at +0) — the pair holds the sole test ref;
            // the gated teardown decs it to zero then fires drop_fn.
            header: 1,
            fn_ptr: 0,
            drop_fn: record_drop_ptr(),
        };
        let gp = &mut getter as *mut FakeClosure as *mut c_void;
        let sp = &mut setter as *mut FakeClosure as *mut c_void;
        let p = unsafe { __torajs_accessor_pair_new(gp, sp, 0) };
        // rc starts at 1 → drop transitions to free path, firing both
        // closures' drop hooks.
        unsafe { __torajs_accessor_drop(p) };
        assert_eq!(
            DROP_HITS.load(Ordering::Relaxed),
            2,
            "both getter + setter drop hooks fire"
        );
    }

    #[test]
    fn drop_null_safe() {
        unsafe { __torajs_accessor_drop(core::ptr::null_mut()) };
    }

    #[test]
    fn drop_skips_absent_accessor() {
        DROP_HITS.store(0, Ordering::Relaxed);
        let mut getter = FakeClosure {
            // rc = 1 (u32 at +0) — the pair holds the sole test ref;
            // the gated teardown decs it to zero then fires drop_fn.
            header: 1,
            fn_ptr: 0,
            drop_fn: record_drop_ptr(),
        };
        let gp = &mut getter as *mut FakeClosure as *mut c_void;
        // No setter (null) — only the getter hook should fire.
        let p = unsafe { __torajs_accessor_pair_new(gp, core::ptr::null_mut(), 0) };
        unsafe { __torajs_accessor_drop(p) };
        assert_eq!(
            DROP_HITS.load(Ordering::Relaxed),
            1,
            "only the present getter drop fires"
        );
    }
}
