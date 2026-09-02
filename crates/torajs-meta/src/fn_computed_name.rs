//! §10.2.9 SetFunctionName for an object-literal member under a
//! COMPUTED key (565-03) — the twin of the class-member face
//! 564-01 built, for the one value shape that cannot carry the name
//! on the cell: an object-literal method / arrow / anonymous
//! function expression is an ordinary compiler-minted closure, and
//! a per-instance word on THAT layout would be a word on every
//! closure in the program.
//!
//! The spec itself says where it goes. SetFunctionName is a
//! `DefinePropertyOrThrow(F, "name", {[[Value]]: name,
//! [[Writable]]: false, [[Enumerable]]: false, [[Configurable]]:
//! true})` — an own property, and a closure cell already has the
//! bag own properties live in. The `.name` read consults that bag
//! before the fn-addr registry, so nothing on the read side has to
//! learn about this case.
//!
//! The INSPECT face is a separate question with a different answer,
//! and it needs nothing here: bun reads the SOURCE, where a
//! computed member has no name, and prints `[Function]`. tr reaches
//! the same place by not handing the parser's `__computed_<n>__`
//! sentinel to NamedEvaluation at all (`ast::named_eval`), which
//! leaves the fn-addr registry row empty — the anonymous form.
//!
//! 567-02 — an anonymous CLASS expression under a computed key
//! (`{ [k]: class {} }`) is named by the same §10.2.9 step, and its
//! class object is a dynobj that already carries a `name` own entry
//! (§10.2.3 MakeConstructor put it there). So there is nothing to
//! attach: the same descriptor lands on the cell itself, as a
//! found-key redefine. The two shapes differ only in which cell the
//! entry table belongs to, which the heap tag answers.

use core::ffi::c_void;

use crate::reflect::{TAG_DYNOBJ, TAG_STR, heap_type_tag};

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_define_plain(
        obj_slot: *mut *mut c_void,
        key: *const u8,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    /// torajs-rc — the one first-attach of a user closure's props
    /// bag (link-judged; see `torajs_rc::closure_entry`).
    fn __torajs_closure_props_attach(cell: *mut u8, props: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_concat(a: *const u8, b: *const u8) -> *mut u8;
    fn __torajs_str_drop(s: *mut u8);
    /// torajs-str — §10.2.9's `"[<description>]"` spelling of a
    /// Symbol property key as a function name (564-01); fresh Str.
    fn __torajs_symbol_fn_name(p: *const c_void) -> *mut u8;
}

/// Closure-cell props-bag slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
const CLOSURE_PROPS_OFF: usize = 24;

/// Entry payload tag for a heap value.
const ANY_HEAP: u64 = 4;

/// The §10.2.9 `name` descriptor: `{[[Value]] present, writable:
/// false, enumerable: false, configurable: true}` — all three
/// attribute sentinels present, only `configurable` set.
const DEFINE_NAME_FLAGS: u64 = (1 << 6) | (1 << 3) | (1 << 4) | (1 << 5) | (1 << 2);

/// Give `cell` the §10.2.9 name of the property key it is being
/// defined under: a Str key IS the name (a numeric key already
/// arrived as its Str spelling through ToPropertyKey), and a Symbol
/// key reads `"[<description>]"` — empty when it has none.
///
/// `prefix` is SetFunctionName's third argument: 0 for a plain
/// member, 1 for `"get "` and 2 for `"set "` — an accessor face's
/// name is the prefixed form (`{ get gg() {} }` → `"get gg"`), and
/// the prefix applies to a computed key's spelling too.
///
/// Called from the object-literal init lane for a computed field
/// whose value is an ANONYMOUS function definition, right after the
/// value is minted and before it is stored (§13.2.5.5 evaluation
/// order: the key is already evaluated). Both arguments stay
/// caller-owned — the define takes its own key reference and the
/// name Str is minted fresh here.
///
/// # Safety
/// `cell` is a live `Tag::Closure` heap cell whose props slot is
/// either NULL or a live dynobj, or a live `Tag::Dynobj` class
/// object; `key` is a live Str / Symbol cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fn_computed_name_define(
    cell: *mut u8,
    key: *const u8,
    prefix: i64,
) {
    if cell.is_null() || key.is_null() {
        return;
    }
    unsafe {
        let bare = if heap_type_tag(key as *const c_void) == TAG_STR {
            __torajs_rc_inc(key as *mut c_void);
            key as *mut u8
        } else {
            __torajs_symbol_fn_name(key as *const c_void)
        };
        let name = match prefix {
            1 | 2 => {
                let p = if prefix == 1 {
                    __torajs_str_alloc(c"get ".as_ptr() as *const u8, 4)
                } else {
                    __torajs_str_alloc(c"set ".as_ptr() as *const u8, 4)
                };
                let joined = __torajs_str_concat(p as *const u8, bare as *const u8);
                __torajs_str_drop(p);
                __torajs_str_drop(bare);
                joined
            }
            _ => bare,
        };
        // 567-02 — a class object IS the entry table the descriptor
        // goes in; a closure keeps its own properties in a bag
        // hanging off the cell, which may not exist yet.
        let class_object = heap_type_tag(cell as *const c_void) == TAG_DYNOBJ;
        let mut props = if class_object {
            cell as *mut c_void
        } else {
            *(cell.add(CLOSURE_PROPS_OFF) as *mut *mut c_void)
        };
        let fresh = props.is_null();
        if fresh {
            props = __torajs_dynobj_alloc();
            if props.is_null() {
                __torajs_str_drop(name);
                return;
            }
        }
        let name_key = __torajs_str_alloc(c"name".as_ptr() as *const u8, 4);
        __torajs_dynobj_define_plain(
            &mut props,
            name_key,
            ANY_HEAP,
            name as u64,
            DEFINE_NAME_FLAGS,
        );
        __torajs_str_drop(name_key);
        if class_object {
            return;
        }
        // A first bag goes through the link-judged attach seam; a
        // bag the define grew (and so relocated) is written back
        // through the slot, which is not an attach.
        if fresh {
            __torajs_closure_props_attach(cell, props);
        } else {
            *(cell.add(CLOSURE_PROPS_OFF) as *mut *mut c_void) = props;
        }
    }
}
