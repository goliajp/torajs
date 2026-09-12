//! RFC 20260912-global-var-cell — a Script evaluated from INSIDE a
//! function (`$262.evalScript(src)` in a function body) runs §16.1.7
//! GlobalDeclarationInstantiation when it is CALLED, against the
//! global object as it is at that moment.
//!
//! The siblings answer the other two shapes: `globalthis_object_vars`
//! binds a declaration the PROGRAM makes (a static slot the compiled
//! bare name loads, handed to the object as a cell), and
//! `globalthis_object_idents` reads a name with no static lane. A
//! function-position Script has neither: its declarations do not
//! exist until the function runs, and whether they MAY be created is
//! a fact about the run — `Object.preventExtensions(this)` or a
//! `defineProperty(this, name, {configurable: false, …})` before the
//! call decides it (`language/global-code/script-decl-*-err`,
//! `script-decl-lex-restricted-global`). So the compiler synthesizes
//! a top-level function per such Script whose body is: the checks
//! (§16.1.7 steps 5.c / 8-10 / 12, one call per name, in the spec's
//! order, ALL before any binding is created), then the bindings
//! (steps 16-17), then the text. The bindings are plain properties —
//! there is no slot for a cell to point at — and every later use of
//! the name, in the text or anywhere else, is the unresolvable-name
//! lane next door (`globalthis_object_idents`).
//!
//! The static half of the checks (steps 3-6 against the names the
//! program itself declares) stays in the compiler
//! (`desugar_eval::script_form`); these kernels see only the object.

use core::ffi::c_void;
use core::sync::atomic::Ordering;

use crate::AnyValue;

use super::globalthis_object::{GLOBALTHIS_OBJECT, globalthis_bag_ptr};

unsafe extern "C" {
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_flags(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    fn __torajs_dynobj_define_plain(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_syntax_error(msg: *const core::ffi::c_char);
    fn __torajs_anyv_unbox_tag(v: AnyValue) -> i64;
    fn __torajs_anyv_unbox_value(v: AnyValue) -> i64;
}

/// `torajs_dynobj::layout` mirrors (the same set `globalthis_object_vars`
/// keeps): the header flag a `preventExtensions` sets, the three
/// get_flags bits, the define descriptor bits.
const HDR_FLAG_NON_EXTENSIBLE: u16 = 1 << 8;
const GET_FLAG_WRITABLE: u64 = 1 << 0;
const GET_FLAG_ENUMERABLE: u64 = 1 << 1;
const GET_FLAG_CONFIGURABLE: u64 = 1 << 2;
const DEFINE_FLAG_WRITABLE: u64 = 1 << 0;
const DEFINE_FLAG_ENUMERABLE: u64 = 1 << 1;
const DEFINE_PRESENT_WRITABLE: u64 = 1 << 3;
const DEFINE_PRESENT_ENUMERABLE: u64 = 1 << 4;
const DEFINE_PRESENT_CONFIGURABLE: u64 = 1 << 5;
const DEFINE_PRESENT_VALUE: u64 = 1 << 6;
/// The `__torajs_anyv_box_from_pair` tag space: a heap payload, the
/// accessor sentinel `get_tag` answers, `undefined`.
const PAIR_TAG_HEAP: u64 = 4;
const PAIR_TAG_UNDEFINED: u64 = 5;
const PAIR_TAG_ACCESSOR: u64 = 6;

/// §9.1.1.4.17 / §9.1.1.4.18 — the attributes of a binding a Script
/// creates: writable and enumerable, NOT configurable (`D` = false).
const SCRIPT_BINDING_FLAGS: u64 = DEFINE_PRESENT_VALUE
    | DEFINE_PRESENT_WRITABLE
    | DEFINE_FLAG_WRITABLE
    | DEFINE_PRESENT_ENUMERABLE
    | DEFINE_FLAG_ENUMERABLE
    | DEFINE_PRESENT_CONFIGURABLE;

/// The `kind` a preflight call carries — which of the three §16.1.7
/// questions it asks about `key`. The compiler spells them as three
/// synthetic callees (`ssa_lower_call_script_decl`).
pub const PREFLIGHT_LEX: u64 = 0;
pub const PREFLIGHT_FN: u64 = 1;
pub const PREFLIGHT_VAR: u64 = 2;

#[inline]
unsafe fn is_extensible(obj: *const c_void) -> bool {
    // SAFETY: `obj` is a live dynobj; the flags word sits at byte 6
    // of the universal header (the layout every sibling reads).
    let header_flags = unsafe { *(obj.cast::<u8>().add(6) as *const u16) };
    header_flags & HDR_FLAG_NON_EXTENSIBLE == 0
}

/// `__torajs_script_preflight(kind, key)` — one §16.1.7 check, on the
/// object as it is now. Records the SyntaxError / TypeError the step
/// specifies and returns; the caller's throw check propagates it and
/// nothing after it in the synthesized body runs, so a Script that
/// fails any check creates NO binding (the test262 assertion after
/// every `-err` case).
///
/// - [`PREFLIGHT_LEX`] — step 5.c HasRestrictedGlobalProperty: a
///   lexical name over a non-configurable own property is a
///   SyntaxError (`let undefined;`, or a `defineProperty(this, name,
///   {configurable: false})` the program ran).
/// - [`PREFLIGHT_FN`] — steps 9-10 CanDeclareGlobalFunction: an
///   absent name needs an extensible object; a present one is fine
///   when configurable, or when a writable AND enumerable data
///   property; anything else is a TypeError.
/// - [`PREFLIGHT_VAR`] — step 12 CanDeclareGlobalVar: a present name
///   is fine ("do nothing"); an absent one needs an extensible object.
///
/// # Safety
///
/// `key` is a live Str cell (borrowed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_script_preflight(kind: u64, key: *mut c_void) {
    let obj = globalthis_bag_ptr();
    // SAFETY: `obj` is the minted singleton; `key` a Str cell per the
    // caller invariant; every callee borrows both.
    unsafe {
        let present = __torajs_dynobj_has(obj, key) != 0;
        match kind {
            PREFLIGHT_LEX => {
                if present && __torajs_dynobj_get_flags(obj, key) & GET_FLAG_CONFIGURABLE == 0 {
                    __torajs_throw_syntax_error(
                        c"A lexical declaration in a Script cannot shadow a non-configurable global property."
                            .as_ptr(),
                    );
                }
            }
            PREFLIGHT_FN => {
                if !present {
                    refuse_unless_extensible(obj);
                    return;
                }
                let flags = __torajs_dynobj_get_flags(obj, key);
                let configurable = flags & GET_FLAG_CONFIGURABLE != 0;
                let plain_writable_enumerable = __torajs_dynobj_get_tag(obj, key)
                    != PAIR_TAG_ACCESSOR
                    && flags & GET_FLAG_WRITABLE != 0
                    && flags & GET_FLAG_ENUMERABLE != 0;
                if !configurable && !plain_writable_enumerable {
                    __torajs_throw_type_error(
                        c"Cannot declare a global function over a non-configurable property that is an accessor, read-only, or non-enumerable."
                            .as_ptr(),
                    );
                }
            }
            PREFLIGHT_VAR => {
                if !present {
                    refuse_unless_extensible(obj);
                }
            }
            // The compiler spells exactly the three kinds above; an
            // ABI value outside them asks nothing.
            _ => {}
        }
    }
}

/// §16.1.7 steps 10.b / 12.b — a binding an absent name would create
/// needs an extensible object; otherwise a TypeError.
///
/// # Safety
///
/// `obj` is a live dynobj.
unsafe fn refuse_unless_extensible(obj: *const c_void) {
    // SAFETY: propagated.
    if !unsafe { is_extensible(obj) } {
        // SAFETY: a static C string.
        unsafe {
            __torajs_throw_type_error(
                c"Cannot declare a global binding: the global object is not extensible.".as_ptr(),
            );
        }
    }
}

/// `__torajs_global_var_declare(key)` — §9.1.1.4.17
/// CreateGlobalVarBinding with `D` = false: an absent name on an
/// extensible object becomes `{undefined, W: true, E: true, C: false}`;
/// a present one is left as it is. The preflight already refused the
/// non-extensible case, so here it is the spec's silent "do nothing".
///
/// # Safety
///
/// `key` is a live Str cell (borrowed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_global_var_declare(key: *mut c_void) {
    let mut obj = globalthis_bag_ptr();
    // SAFETY: `obj` is the minted singleton; `key` a Str cell per the
    // caller invariant; `undefined` carries no payload to own.
    unsafe {
        if __torajs_dynobj_has(obj, key) != 0 || !is_extensible(obj) {
            return;
        }
        __torajs_dynobj_define_plain(&mut obj, key, PAIR_TAG_UNDEFINED, 0, SCRIPT_BINDING_FLAGS);
    }
    GLOBALTHIS_OBJECT.store(obj as u64, Ordering::Release);
}

/// `__torajs_global_fn_declare(key, value)` — §9.1.1.4.18
/// CreateGlobalFunctionBinding with `D` = false: an absent or
/// configurable property is (re)defined as `{value, W: true, E: true,
/// C: false}`; a non-configurable one (the preflight passed, so a
/// writable enumerable data property) takes the value through
/// [[Set]] and keeps its attributes — that path sees through a
/// `GlobalCell`, so a program-declared `function f` re-declared by
/// the Script writes the program's own slot.
///
/// The entry takes its own stake on a heap payload; the argument
/// stays the caller's.
///
/// # Safety
///
/// `key` is a live Str cell (borrowed); `v` a valid AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_global_fn_declare(key: *mut c_void, v: AnyValue) {
    let mut obj = globalthis_bag_ptr();
    // SAFETY: `obj` is the minted singleton; `key` a Str cell per the
    // caller invariant; the stake taken below transfers into the
    // entry through `define_plain` / `set`.
    unsafe {
        let tag = __torajs_anyv_unbox_tag(v) as u64;
        let value = __torajs_anyv_unbox_value(v) as u64;
        if tag == PAIR_TAG_HEAP {
            __torajs_rc_inc(value as *mut c_void);
        }
        let redefinable = __torajs_dynobj_has(obj, key) == 0
            || __torajs_dynobj_get_flags(obj, key) & GET_FLAG_CONFIGURABLE != 0;
        if redefinable {
            __torajs_dynobj_define_plain(&mut obj, key, tag, value, SCRIPT_BINDING_FLAGS);
        } else {
            __torajs_dynobj_set(&mut obj, key, tag, value);
        }
    }
    GLOBALTHIS_OBJECT.store(obj as u64, Ordering::Release);
}
