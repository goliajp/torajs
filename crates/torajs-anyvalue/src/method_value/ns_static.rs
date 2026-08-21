//! Namespace-static value family (RFC 20260719-ns-static-value-reify
//! B1) — `Math.max` read as a VALUE answers an interned immortal
//! closure cell whose boxed dual entry is a REAL dispatcher:
//! namespace statics are receiver-less, so a bare call (`const m =
//! Math.max; m(1, 5)`), a `.call/.apply` re-dispatch (thisArg
//! ignored per §21.3), an any-lane call and a HOF boxed callback all
//! work through the same entry. The compiler routes alias calls
//! through the variadic boxed lane (`variadic_locals`), so the cell
//! never needs a per-signature typed entry; `fn_addr` stays a loud
//! TypeError for the typed-slot direct-call boundary (RFC B4).
//!
//! Ids come from the shared `torajs_rc::ns_static` table (the same
//! truth the compiler interns at lower time); the dispatch table
//! below is index-lockstep with it (unit-tested). Math semantics
//! delegate to the SAME `__torajs_math_*` kernels the typed tier
//! calls — single source, zero drift.
//!
//! The `Disp` shape enum, the id-indexed `DISPATCH` table and the
//! kernel externs live in the `ns_static_table` sibling (file-size
//! rule); this module holds the cell mint, the reflection probes and
//! the per-shape dispatch arms.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::ns_static::ns_static_meta;
use torajs_rc::{FLAG_STATIC_LITERAL, Tag};

use crate::nanbox::{VALUE_UNDEFINED, box_double, box_int32};

use super::ns_static_table::{
    __torajs_anyv_freeze, __torajs_anyv_from_entries, __torajs_anyv_get_proto_of_any,
    __torajs_anyv_is_extensible, __torajs_anyv_is_sealed, __torajs_anyv_prevent_extensions,
    __torajs_anyv_seal, __torajs_anyv_set_prototype_of, __torajs_date_now_static,
    __torajs_obj_is_frozen_any, __torajs_throw_check, __torajs_throw_type_error, DISPATCH, Disp,
};

use super::{
    CELL_SIZE, CLOSURE_BOXED_ENTRY_OFF, CLOSURE_CAP_BASE_OFF, CLOSURE_DROP_FN_OFF,
    CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF, TABLE_SIZE, mint_immortal_str,
};

/// Arg-coercion / boxing / predicate helpers split to
/// [`super::ns_static_util`] (rotation 268 mechanical move);
/// re-exported so sibling arms keep their `super::ns_static::` path.
pub(super) use super::ns_static_util::{arg_at, arg_num, own, to_i64_mod32};
use super::ns_static_util::{box_bool, num_predicate, putc_out};

/// Per-id interned cells + `.name` Str cells — same immortal
/// atomic-static shape as `METHOD_CELLS` (headroom bound shared with
/// the mid table; lockstep-tested below).
static NS_CELLS: [AtomicU64; TABLE_SIZE] = [const { AtomicU64::new(0) }; TABLE_SIZE];
static NS_NAME_CELLS: [AtomicU64; TABLE_SIZE] = [const { AtomicU64::new(0) }; TABLE_SIZE];

/// Boxed dual entry of every ns-static cell — receiver-less real
/// dispatch (`this` is ignored: `invoke_with_this` never shifts a
/// non-recv-first cell, and a bare call passes no receiver at all).
unsafe extern "C" fn ns_dispatch_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    let id = unsafe { *(env.cast::<u8>().add(CLOSURE_CAP_BASE_OFF) as *const u64) as i64 };
    unsafe { dispatch(id, argv, argc) }
}

/// `fn_addr` of every ns-static cell — an any→typed fn-slot cast
/// that direct-calls the native entry throws instead of jumping into
/// a wrong-ABI body (RFC B4 records the typed-slot adapter face).
unsafe extern "C" fn ns_native_entry() -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"builtin namespace static called through a typed fn slot".as_ptr(),
        );
    }
    0
}

// CARVE-OUT: dispatch table — one thin delegation arm per `Disp`
// variant (1-6 lines each, index-lockstep with NS_STATIC_TABLE);
// splitting would break the dispatch locality the table exists for.
// Same family as `check/stmt.rs::check_stmt` and
// `ssa_lower_expr_inner::lower` (both carry the same marker).
unsafe fn dispatch(id: i64, argv: *const u64, argc: i64) -> u64 {
    let Some(disp) = (if id < 0 {
        None
    } else {
        DISPATCH.get(id as usize)
    }) else {
        // Unreachable through minted cells (the compiler bakes only
        // table hits); loud instead of garbage if it ever is.
        unsafe {
            __torajs_throw_type_error(c"unknown namespace-static id".as_ptr());
        }
        return VALUE_UNDEFINED;
    };
    unsafe {
        match disp {
            Disp::F(f) => match arg_num(argv, argc, 0) {
                Ok(x) => box_double(f(x)),
                Err(()) => VALUE_UNDEFINED,
            },
            Disp::Ff(f) => {
                let Ok(x) = arg_num(argv, argc, 0) else {
                    return VALUE_UNDEFINED;
                };
                let Ok(y) = arg_num(argv, argc, 1) else {
                    return VALUE_UNDEFINED;
                };
                box_double(f(x, y))
            }
            Disp::MinMax { is_max } => super::ns_static_math::min_max_fold(*is_max, argv, argc),
            Disp::Hypot => super::ns_static_math::hypot_fold(argv, argc),
            Disp::GroupBy { map } => super::ns_static_ctor::group_by(*map, argv, argc),
            Disp::I32Pair(f) => {
                let Ok(x) = arg_num(argv, argc, 0) else {
                    return VALUE_UNDEFINED;
                };
                let Ok(y) = arg_num(argv, argc, 1) else {
                    return VALUE_UNDEFINED;
                };
                box_int32(f(to_i64_mod32(x), to_i64_mod32(y)) as i32)
            }
            Disp::I32One(f) => match arg_num(argv, argc, 0) {
                Ok(x) => box_int32(f(to_i64_mod32(x)) as i32),
                Err(()) => VALUE_UNDEFINED,
            },
            Disp::Nullary(f) => box_double(f()),
            Disp::ConsoleLog { to_stderr } => {
                if *to_stderr {
                    torajs_io::__torajs_io_sink_to_stderr();
                }
                for i in 0..argc {
                    if i > 0 {
                        putc_out(b' ');
                    }
                    crate::inspect::any::__torajs_print_anyv_inline_top(*argv.add(i as usize));
                }
                putc_out(b'\n');
                if *to_stderr {
                    torajs_io::__torajs_io_sink_to_stdout();
                }
                VALUE_UNDEFINED
            }
            Disp::ParseInt => super::ns_static_coerce::parse_int_value(argv, argc),
            Disp::ParseFloat => super::ns_static_coerce::parse_float_value(argv, argc),
            Disp::NumPredicate(p) => box_bool(num_predicate(p, arg_at(argv, argc, 0))),
            Disp::ArrayIsArray => {
                let v = arg_at(argv, argc, 0);
                let hit = crate::nanbox::is_cell(v) && {
                    let ptr = crate::nanbox::as_void_ptr(v);
                    (ptr.cast::<u8>().add(4) as *const u16).read() == Tag::Arr as u16
                };
                box_bool(hit)
            }
            Disp::ObjectIs => box_bool(crate::nanbox_ffi::__torajs_anyv_same_value(
                arg_at(argv, argc, 0),
                arg_at(argv, argc, 1),
            )),
            Disp::OwnEnum(kind) => super::ns_static_obj::own_enum(kind, arg_at(argv, argc, 0)),
            Disp::ObjectAssign => super::ns_static_obj::object_assign(argv, argc),
            Disp::ObjectFreeze => own(__torajs_anyv_freeze(arg_at(argv, argc, 0))),
            Disp::ObjectIsFrozen => {
                box_bool(__torajs_obj_is_frozen_any(arg_at(argv, argc, 0) as i64))
            }
            // Already owned on return — inc'ing again would strand the
            // prototype cell at a refcount its owners never drop.
            Disp::ObjectGetProtoOf => __torajs_anyv_get_proto_of_any(arg_at(argv, argc, 0)),
            Disp::ObjectSetProtoOf => {
                let obj = arg_at(argv, argc, 0);
                __torajs_anyv_set_prototype_of(obj, arg_at(argv, argc, 1));
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                own(obj)
            }
            Disp::ObjectFromEntries => __torajs_anyv_from_entries(arg_at(argv, argc, 0)),
            Disp::SymbolFor => super::ns_static_obj::symbol_for_value(arg_at(argv, argc, 0)),
            Disp::SymbolKeyFor => super::ns_static_obj::symbol_key_for_value(arg_at(argv, argc, 0)),
            // Ctor-static arms (RFC 20260720 刀 1) — sibling module.
            Disp::DateNow => box_double(__torajs_date_now_static() as f64),
            Disp::DateParse => super::ns_static_ctor::date_parse(argv, argc),
            Disp::DateUtc => super::ns_static_ctor::date_utc(argv, argc),
            Disp::StrFromCodes { code_point } => {
                super::ns_static_ctor::str_from_codes(*code_point, argv, argc)
            }
            Disp::ObjectHasOwn => super::ns_static_ctor::object_has_own(argv, argc),
            Disp::ObjectPreventExtensions => {
                own(__torajs_anyv_prevent_extensions(arg_at(argv, argc, 0)))
            }
            Disp::ObjectIsExtensible => {
                box_bool(__torajs_anyv_is_extensible(arg_at(argv, argc, 0)))
            }
            Disp::ObjectSeal => own(__torajs_anyv_seal(arg_at(argv, argc, 0))),
            Disp::ObjectIsSealed => box_bool(__torajs_anyv_is_sealed(arg_at(argv, argc, 0))),
            Disp::BigIntAsN { signed } => super::ns_static_ctor::bigint_as_n(*signed, argv, argc),
            Disp::PromiseSettleFn { reject } => {
                super::ns_static_promise::promise_settle_fn(*reject, argv, argc)
            }
            Disp::PromiseCombinator { kind } => {
                super::ns_static_promise::promise_combinator_fn(*kind, argv, argc)
            }
            Disp::PromiseTryFn => super::ns_static_promise::promise_try_fn(argv, argc),
            Disp::PromiseWithResolversFn => {
                super::ns_static_promise::promise_with_resolvers_fn(argv, argc)
            }
            Disp::PromiseKeyed { settled } => {
                super::ns_static_promise::promise_keyed_fn(*settled, argv, argc)
            }
            Disp::Gopd => super::ns_static_ctor::gopd_static(argv, argc),
            Disp::DefineFace => super::ns_static_ctor::define_face_reject(),
            Disp::OwnSymbols => super::ns_static_obj::own_symbols_value(arg_at(argv, argc, 0)),
            // recv-first cells (this_aware_id): argv[0] is the
            // thisArg every honoring caller prepended.
            Disp::ArrayFromFace => super::ns_static_ctor::array_from_value(argv, argc),
            // Iterator statics delegate to the SAME kernels the
            // statics-wedge lowering bakes — a pending throw from a
            // refusal path rides out with the undefined answer.
            Disp::IteratorFrom => crate::iter_from::__torajs_iterator_from(arg_at(argv, argc, 0)),
            Disp::IteratorConcat => super::ns_static_argv::iterator_concat_pack(argv, argc),
            Disp::IteratorZip { keyed: false } => {
                crate::iter_zip::__torajs_iterator_zip(arg_at(argv, argc, 0), arg_at(argv, argc, 1))
            }
            Disp::IteratorZip { keyed: true } => {
                crate::iter_zip_keyed::__torajs_iterator_zip_keyed(
                    arg_at(argv, argc, 0),
                    arg_at(argv, argc, 1),
                )
            }
            Disp::ReflectGopd => super::ns_static_reflect::reflect_gopd_static(argv, argc),
            Disp::ReflectGetProto => super::ns_static_reflect::reflect_get_proto(argv, argc),
            Disp::ReflectPreventExtensions => {
                super::ns_static_reflect::reflect_prevent_extensions(argv, argc)
            }
            Disp::ReflectIsExtensible => {
                super::ns_static_reflect::reflect_is_extensible(argv, argc)
            }
            Disp::ReflectDeleteProperty => {
                super::ns_static_reflect::reflect_delete_property(argv, argc)
            }
            Disp::ReflectSetPrototypeOf => {
                super::ns_static_reflect::reflect_set_prototype_of(argv, argc)
            }
            Disp::ArrayOf => super::ns_static_obj::array_of_pack(argv, argc),
            Disp::RegExpEscape => super::ns_static_obj::regexp_escape_value(arg_at(argv, argc, 0)),
            Disp::ReflectDefineProperty => {
                super::ns_static_reflect::reflect_define_property(argv, argc)
            }
            Disp::ReflectApply => crate::reflect_apply::__torajs_reflect_apply(
                arg_at(argv, argc, 0),
                arg_at(argv, argc, 1),
                arg_at(argv, argc, 2),
            ),
            Disp::ReflectSet => super::ns_static_reflect::reflect_set(argv, argc),
            Disp::FromAsyncDyn => super::ns_static_ctor::from_async_dyn(argv, argc),
            Disp::JsonRawJson => crate::json_raw::__torajs_json_raw_json(arg_at(argv, argc, 0)),
            Disp::JsonIsRawJson => {
                crate::json_raw::__torajs_json_is_raw_json(arg_at(argv, argc, 0))
            }
            Disp::StringRaw => super::ns_static_argv::string_raw_value(argv, argc),
            Disp::JsonParse => super::ns_static_argv::json_parse_value(argv, argc),
            Disp::JsonStringify => super::ns_static_argv::json_stringify_value(argv, argc),
            Disp::ReflectGet => super::ns_static_reflect::reflect_get(argv, argc),
            Disp::ReflectHas => super::ns_static_reflect::reflect_has(argv, argc),
            Disp::ReflectOwnKeys => super::ns_static_reflect::reflect_own_keys(argv, argc),
            Disp::ReflectConstructDyn => {
                super::ns_static_reflect::reflect_construct_dyn(argv, argc)
            }
            // §19.2.1 — tr performs no runtime evaluation (direct
            // calls compile through the desugar_eval prefix); the
            // escaped cell's call face is the recorded loud reject.
            Disp::EvalDyn => {
                __torajs_throw_type_error(
                    c"eval through a runtime value is not supported".as_ptr(),
                );
                VALUE_UNDEFINED
            }
            Disp::GlobalNumTest { finite } => {
                super::ns_static_coerce::global_num_test(*finite, argv, argc)
            }
            Disp::UriKernel { encode, component } => {
                super::ns_static_coerce::uri_kernel_value(*encode, *component, argv, argc)
            }
        }
    }
}

/// Hand an argv borrow back as the OWNED result the boxed entry's
/// contract promises: every consumer of an any-lane call result drops
/// it, so a static that answers one of its own arguments has to raise
/// the count first (the typed tier does the same at
/// `ssa_lower_call_object_get_prototype_of.rs:112`). Immediates carry
/// no refcount, so only cells inc.
/// The statics whose spec reads `this` (Array.from / fromAsync / of
/// take it as the constructor C, §23.1.2.1 step 1 / §23.1.2.3 step 2) — their cells carry
/// `FLAG_CLOSURE_RECV_FIRST`, so EVERY caller that honors the
/// receiver channel (`.call`/`.apply`, the variadic alias lane, HOF
/// loops, bind) prepends the thisArg in argv[0] (undefined on a bare
/// call), and the dispatch arm reads it there. Keyed off the
/// dispatch table itself — one source, no name comparison.
fn this_aware_id(id: i64) -> bool {
    id >= 0
        && matches!(
            DISPATCH.get(id as usize),
            Some(
                Disp::ArrayFromFace
                    | Disp::FromAsyncDyn
                    | Disp::ArrayOf
                    | Disp::PromiseSettleFn { .. }
                    | Disp::PromiseCombinator { .. }
                    | Disp::PromiseTryFn
                    | Disp::PromiseWithResolversFn
                    | Disp::PromiseKeyed { .. }
            )
        )
}

/// The interned cell for an ns-static id — lazily allocated,
/// immortal, same closure layout as `builtin_method_cell`.
pub(crate) fn ns_static_cell(id: i64) -> *mut u8 {
    let slot = &NS_CELLS[id as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    // SAFETY: fresh CELL_SIZE allocation, fully initialized below.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = if this_aware_id(id) {
            FLAG_STATIC_LITERAL | torajs_rc::FLAG_CLOSURE_RECV_FIRST
        } else {
            FLAG_STATIC_LITERAL
        };
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = ns_native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = ns_dispatch_entry as *const () as u64;
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = id as u64;
        slot.store(cell as u64, Ordering::Relaxed);
        cell
    }
}

/// Compiler face — the interned cell for a baked table id. The
/// result is a Closure-repr borrow of an immortal cell (rc traffic
/// no-ops on the static flag).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_ns_static_cell(id: i64) -> *mut u8 {
    ns_static_cell(id)
}

/// The ns-static id a cell carries — `None` for every other closure
/// shape (discriminated by the boxed entry's address, the same
/// scheme `builtin_method_mid` uses).
pub(crate) unsafe fn ns_static_id_of(ptr: *const c_void) -> Option<i64> {
    unsafe {
        let entry = *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry == ns_dispatch_entry as *const () as u64 {
            Some(*(ptr.cast::<u8>().add(CLOSURE_CAP_BASE_OFF) as *const u64) as i64)
        } else {
            None
        }
    }
}

/// Reflection name of an ns-static cell (`[Function: max]` /
/// toString native form), `None` for other closures.
pub(crate) unsafe fn ns_static_name(ptr: *const c_void) -> Option<&'static str> {
    let id = unsafe { ns_static_id_of(ptr) }?;
    ns_static_meta(id).map(|r| r.name)
}

/// Interned `.name` Str cell of an ns-static cell.
pub(crate) unsafe fn ns_static_name_cell_of(ptr: *const c_void) -> Option<*mut u8> {
    let id = unsafe { ns_static_id_of(ptr) }?;
    let row = ns_static_meta(id)?;
    let slot = &NS_NAME_CELLS[id as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return Some(p as *mut u8);
    }
    let cell = mint_immortal_str(row.name.as_bytes());
    slot.store(cell as u64, Ordering::Relaxed);
    Some(cell)
}

/// ES-spec `length` of an ns-static cell.
pub(crate) unsafe fn ns_static_arity(ptr: *const c_void) -> Option<u32> {
    let id = unsafe { ns_static_id_of(ptr) }?;
    ns_static_meta(id).map(|r| r.length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_rc::ns_static::NS_STATIC_TABLE;

    #[test]
    fn dispatch_table_lockstep() {
        assert_eq!(DISPATCH.len(), NS_STATIC_TABLE.len());
        assert!(NS_STATIC_TABLE.len() <= TABLE_SIZE);
    }

    #[test]
    fn to_i64_mod32_edges() {
        assert_eq!(to_i64_mod32(3.7), 3);
        assert_eq!(to_i64_mod32(-1.0), -1);
        assert_eq!(to_i64_mod32(4294967297.0), 1);
        assert_eq!(to_i64_mod32(f64::NAN), 0);
        assert_eq!(to_i64_mod32(f64::INFINITY), 0);
        assert_eq!(
            to_i64_mod32(1e20) as i32,
            (1e20_f64 % 4294967296.0) as i64 as i32
        );
    }

    #[test]
    fn cell_shape_and_probes() {
        let id = torajs_rc::ns_static::ns_static_id("Math", "max");
        let cell = ns_static_cell(id);
        assert_eq!(cell, ns_static_cell(id), "interned identity");
        unsafe {
            assert_eq!(ns_static_id_of(cell as *const c_void), Some(id));
            assert_eq!(ns_static_name(cell as *const c_void), Some("max"));
            assert_eq!(ns_static_arity(cell as *const c_void), Some(2));
            // A mid-keyed method cell never answers the ns probe.
            let mid_cell =
                crate::method_value::builtin_method_cell(-1, torajs_rc::ANY_METHOD_TO_STRING);
            assert_eq!(ns_static_id_of(mid_cell as *const c_void), None);
        }
    }
}
