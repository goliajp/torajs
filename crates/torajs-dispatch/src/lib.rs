//! The `__torajs_any_method_dispatch` link seam (RFC
//! 20260824-s2-5 selective registration, Phase B blade 0).
//!
//! `torajs-anyvalue`'s `any_method_call_inner` / redispatch reach
//! the dispatcher through an `extern "C"` declaration; this crate
//! owns the default definition as a SEPARATE staticlib member so:
//!
//! - normal links resolve the seam here and forward to the
//!   monolithic [`torajs_anyvalue::any_method_dispatch_impl`];
//! - a compiler-emitted specialized dispatcher in the user `.o`
//!   shadows this member (user definitions win the member closure),
//!   and the monolith — whose only reference is the forward below —
//!   dead-strips together with every family arm the program never
//!   uses.
//!
//! Keep this crate a single thin forwarder: any logic added here
//! becomes logic a specialized dispatcher must replicate.

#![no_std]

/// Default (monolithic) resolution of the dispatch seam.
///
/// # Safety
/// Same contract as `__torajs_any_method_call`: cell receivers are
/// valid heap pointers; `argv` holds `argc` live AnyValue slots;
/// `recv_slot` is NULL or the receiver variable's live slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_method_dispatch(
    recv: u64,
    mid: i64,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
    skip_wrapper_expando: bool,
) -> u64 {
    unsafe {
        torajs_anyvalue::any_method_dispatch_impl(
            recv,
            mid,
            name_str,
            recv_slot,
            argv,
            argc,
            skip_wrapper_expando,
        )
    }
}

// ---- The arm-seam族 (blade 2a) ----
//
// One default definition per dispatch family, each a thin forward
// to `torajs_anyvalue::dispatch_arms`. The skeleton's tag ladder
// calls these through `extern "C"` declarations, so a
// compiler-emitted loud-reject stub in the user `.o` shadows the
// family it never uses and the anyvalue kernel dead-strips.
//
// Shared safety contract (same as the seam declarations): `recv`
// boxes a live value whose shape matches the family; `argv` holds
// `argc` live AnyValue slots; `recv_slot` is NULL or the receiver
// variable's live slot.

macro_rules! default_arm {
    ($(#[doc = $doc:literal] $sym:ident => $impl_fn:ident;)+) => {
        $(
            #[doc = $doc]
            ///
            /// # Safety
            /// See the module-level arm-seam contract.
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn $sym(
                recv: u64,
                mid: i64,
                name_str: *const u8,
                recv_slot: *mut u64,
                argv: *const u64,
                argc: i64,
            ) -> u64 {
                unsafe {
                    torajs_anyvalue::dispatch_arms::$impl_fn(
                        recv, mid, name_str, recv_slot, argv, argc,
                    )
                }
            }
        )+
    };
}

default_arm! {
    #[doc = "Str-cell arm (also the materialized short-str path) — default (monolithic) definition."]
    __torajs_dispatch_str_arm => str_arm_impl;
    #[doc = "Arr-cell builtin surface — default (monolithic) definition."]
    __torajs_dispatch_arr_arm => arr_arm_impl;
    #[doc = "DynObj arm — default (monolithic) definition."]
    __torajs_dispatch_dynobj_arm => dynobj_arm_impl;
    #[doc = "Static-layout struct (Tag::Obj) arm — default (monolithic) definition."]
    __torajs_dispatch_struct_arm => struct_arm_impl;
    #[doc = "Map / Set arm — default (monolithic) definition."]
    __torajs_dispatch_mapset_arm => mapset_arm_impl;
    #[doc = "MapIter / ArrIter / IterHelper arm — default (monolithic) definition."]
    __torajs_dispatch_iter_arm => iter_arm_impl;
    #[doc = "ArrayBuffer / TypedArray / DataView arm — default (monolithic) definition."]
    __torajs_dispatch_buffer_arm => buffer_arm_impl;
    #[doc = "Date arm — default (monolithic) definition."]
    __torajs_dispatch_date_arm => date_arm_impl;
    #[doc = "Promise arm — default (monolithic) definition."]
    __torajs_dispatch_promise_arm => promise_arm_impl;
    #[doc = "RegExp arm — default (monolithic) definition."]
    __torajs_dispatch_regexp_arm => regexp_arm_impl;
    #[doc = "BigInt arm — default (monolithic) definition."]
    __torajs_dispatch_bigint_arm => bigint_arm_impl;
    #[doc = "Symbol-cell arm — default (monolithic) definition."]
    __torajs_dispatch_symbol_arm => symbol_arm_impl;
    #[doc = "Closure arm — default (monolithic) definition."]
    __torajs_dispatch_closure_arm => closure_arm_impl;
    #[doc = "WeakMap / WeakSet / WeakRef arm — default (monolithic) definition."]
    __torajs_dispatch_weak_arm => weak_arm_impl;
    #[doc = "Number-immediate arm — default (monolithic) definition."]
    __torajs_dispatch_num_arm => num_arm_impl;
}

/// Default (monolithic) resolution of the ns-static MINT seam —
/// the interned cell for a baked namespace-static id.
/// `torajs-anyvalue`'s minting sites (namespace-object fill,
/// globalThis fill, ctor own-static reads, the reify face) call
/// the `extern "C"` declaration; a compiler-emitted loud-reject
/// stub in the user `.o` shadows this member and the whole
/// ns-static dispatch universe (the boxed dispatch entry, the
/// per-id DISPATCH table, every static's kernel) dead-strips.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_ns_static_cell(id: i64) -> *mut u8 {
    torajs_anyvalue::ns_static_cell_impl(id)
}

/// Default (monolithic) resolution of the ctor-date seam — §21.4.2
/// Date called as a function through a first-class ctor value.
/// Stubbed together with the date family arm.
///
/// # Safety
/// No inputs; forwards to the anyvalue kernel sequence.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_date_call() -> u64 {
    unsafe { torajs_anyvalue::ctor_date_call_impl() }
}

// ---- Typed-kernel exotic slow paths (RFC 20260824-s2-5 刀 4 A2/A3) ----
//
// `torajs-arr`'s typed join kernels and the species guard reach
// their exotic-receiver slow paths through these seams; the link
// judgment (`torajs-link::dead_strip_elide`, guard = the arr crate's
// flag / props writer entries) shadows them with a loud-reject stub
// when no array in the artifact can become exotic or grow a props
// bag. Same archive-member argument as the arm seams above.

unsafe extern "C" {
    fn __torajs_arr_join_exotic_impl(
        arr: *const u8,
        sep: *const u8,
        kind: u64,
        locale: i64,
    ) -> *mut u8;
    fn __torajs_arr_species_guard_props(arr: *const u8) -> i64;
    fn __torajs_arrprops_drop_entry(arr: *mut core::ffi::c_void);
    fn __torajs_subclass_drop_entry(p: *mut core::ffi::c_void);
}

/// Default resolution of the exotic-join seam — the any-world join
/// walk in torajs-arr / torajs-anyvalue.
///
/// # Safety
/// `arr` is a live array heap block; `sep` a live Str (unused by
/// the locale walk).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_exotic(
    arr: *const u8,
    sep: *const u8,
    kind: u64,
    locale: i64,
) -> *mut u8 {
    unsafe { __torajs_arr_join_exotic_impl(arr, sep, kind, locale) }
}

/// Default resolution of the species-guard slow seam — the props-bag
/// `constructor` classification in torajs-arr.
///
/// # Safety
/// `arr` is a live array heap block whose props slot is non-NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_species_guard_slow(arr: *const u8) -> i64 {
    unsafe { __torajs_arr_species_guard_props(arr) }
}

/// Default resolution of the scalar-array drop's props leg.
///
/// # Safety
/// `arr` is a live array heap block reaching rc=0 in the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop_props_slow(arr: *mut core::ffi::c_void) {
    unsafe { __torajs_arrprops_drop_entry(arr) }
}

/// Default resolution of the scalar-array drop's subclass leg.
///
/// # Safety
/// `arr` is a live `FLAG_SUBCLASSED` array heap block reaching rc=0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop_subclass_slow(arr: *mut core::ffi::c_void) {
    unsafe { __torajs_subclass_drop_entry(arr) }
}

/// The loud-reject landing pad for compiler-emitted family stubs
/// (RFC 20260824-s2-5 blade 2b). A specialized program's user `.o`
/// defines `__torajs_dispatch_<family>_arm` as a single
/// `b __torajs_dispatch_stub_reject` for every family whose
/// mid-domain the program provably never enters; reaching it at
/// runtime means the compiler's emitted-mid analysis was WRONG —
/// the throw is a guard against compiler bugs, never a semantics
/// device.
///
/// # Safety
/// C-ABI entry; no pointer params are dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dispatch_stub_reject(
    _recv: u64,
    _mid: i64,
    _name_str: *const u8,
    _recv_slot: *mut u64,
    _argv: *const u64,
    _argc: i64,
    _pad: u64,
    fam_id: u64,
) -> u64 {
    // fam_id rides x7 (stamped by the stub's movz): 0..14 = the arm
    // roster order, 16..40 = printer kernels, 40+ = link-judged
    // exotic slow paths. The name makes a wrong
    // judgment attributable from the failure line alone.
    let msg: &core::ffi::CStr = match fam_id {
        0 => c"str method family stripped (dispatch judgment bug)",
        1 => c"arr method family stripped (dispatch judgment bug)",
        2 => c"dynobj method family stripped (dispatch judgment bug)",
        3 => c"struct method family stripped (dispatch judgment bug)",
        4 => c"mapset method family stripped (dispatch judgment bug)",
        5 => c"iter method family stripped (dispatch judgment bug)",
        6 => c"buffer method family stripped (dispatch judgment bug)",
        7 => c"date method family stripped (dispatch judgment bug)",
        8 => c"promise method family stripped (dispatch judgment bug)",
        9 => c"regexp method family stripped (dispatch judgment bug)",
        10 => c"bigint method family stripped (dispatch judgment bug)",
        11 => c"symbol method family stripped (dispatch judgment bug)",
        12 => c"closure method family stripped (dispatch judgment bug)",
        13 => c"weak method family stripped (dispatch judgment bug)",
        14 => c"num method family stripped (dispatch judgment bug)",
        15 => c"namespace-static world stripped (dispatch judgment bug)",
        // 40+: the typed kernels' exotic slow paths (link-judged on
        // the arr crate's writer entries, r500).
        40 => c"exotic-array join stripped (link judgment bug)",
        41 => c"array species probe stripped (link judgment bug)",
        42 => c"scalar-array props drop stripped (link judgment bug)",
        43 => c"scalar-array subclass drop stripped (link judgment bug)",
        n if (16..40).contains(&n) => c"printer kernel stripped (dispatch judgment bug)",
        _ => c"method family stripped (dispatch judgment bug)",
    };
    unsafe {
        __torajs_throw_type_error(msg.as_ptr());
    }
    torajs_anyvalue::VALUE_UNDEFINED
}

unsafe extern "C" {
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}
