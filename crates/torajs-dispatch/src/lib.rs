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

pub mod link_seams;
pub mod link_seams_in;

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
    unsafe { throw_reject(fam_id) };
    torajs_anyvalue::VALUE_UNDEFINED
}

/// The landing pad for a stubbed entry that answers a POINTER (the
/// ns-static mint seam, the Date ISO printer, the exotic join, the
/// typed `string[]` keyed read): the same named TypeError, then NULL,
/// which their callers read as a miss and leave the pending throw to
/// the caller's check. The value-shaped pad's `undefined` bits would
/// be dereferenced as a cell before that check runs.
///
/// # Safety
/// C-ABI entry; no pointer params are dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dispatch_stub_reject_ptr(
    _a0: u64,
    _a1: u64,
    _a2: u64,
    _a3: u64,
    _a4: u64,
    _a5: u64,
    _a6: u64,
    fam_id: u64,
) -> *mut u8 {
    unsafe { throw_reject(fam_id) };
    core::ptr::null_mut()
}

/// Record the named TypeError for `fam_id`. It rides x7 (stamped by
/// the stub's movz): 0..14 = the arm roster order, 16..40 = printer
/// kernels, 40..=58 and 60..=65 = link-judged exotic slow paths, 59 = a
/// class method row's dropped twin column. The name makes a wrong judgment
/// attributable from the failure line alone.
unsafe fn throw_reject(fam_id: u64) {
    let mut msg = [0u8; REJECT_MSG_CAP];
    reject_message(fam_id, &mut msg);
    unsafe {
        __torajs_throw_type_error(msg.as_ptr().cast());
    }
}

/// The reject subjects, one string: the fifteen arm families first (fam_id 0..14, the order
/// `torajs_rc::any_method_family` numbers its bits), then the named worlds. One string plus a
/// byte offset table instead of an array of `&str`: fifteen fat pointers were 240 B of
/// `__DATA,__const` (each needs a rebase fixup), a whole `__DATA` page in every program whose
/// dispatch reject stub was live. Offsets into a string are plain rodata, no fixups.
const SUBJECTS: &str = "str\
arr\
dynobj\
struct\
mapset\
iter\
buffer\
date\
promise\
regexp\
bigint\
symbol\
closure\
weak\
num\
namespace-static world\
printer kernel\
exotic-array join\
array species probe\
scalar-array props drop\
scalar-array subclass drop\
closure props drop\
closure unbuffer\
uncaught error render\
struct props drop\
struct cycle buffering\
struct unbuffer\
struct field-write guard\
typed-array exotic sort\
typed-array keyed read\
typed-array keyed str read\
array map hole mark\
unsized cell free\
typed-array keyed any read\
struct field-read guard\
struct absent set\
class method twin\
boxed lastIndex read\
shift on ro length\
unhandled error render\
thenable probe\
re-parented struct in\
exotic struct json walk\
method family";
/// `SUBJECTS[OFF[i]..OFF[i + 1]]` is subject `i` (u16: the string is 652 B).
const OFF: [u16; 45] = [
    0, 3, 6, 12, 18, 24, 28, 34, 38, 45, 51, 57, 63, 70, 74, 77, 99, 113, 130, 149, 172, 198, 216,
    232, 253, 270, 292, 307, 331, 354, 376, 402, 421, 438, 464, 487, 504, 521, 541, 559, 581, 595,
    616, 639, 652,
];
const ARM_FAMILY_COUNT: u64 = 15;
/// Subject indices past the arm families.
const SUBJ_NS_STATIC: usize = 15;
const SUBJ_PRINTER: usize = 16;
/// fam 40..=65 in order; 59, the twin column, is the one among them
/// the dispatch judgment makes.
const SUBJ_LINK_FIRST: usize = 17;
const FAM_TWIN: u64 = 59;
const SUBJ_FALLBACK: usize = 43;

const REJECT_METHOD_FAMILY: &str = " method family";
const REJECT_DISPATCH_TAIL: &str = " stripped (dispatch judgment bug)";
const REJECT_LINK_TAIL: &str = " stripped (link judgment bug)";

/// Longest composed reject line + NUL: `"scalar-array subclass drop"`
/// (26) + the link tail (29) is the widest, with room to spare.
const REJECT_MSG_CAP: usize = 96;

fn subject(i: usize) -> &'static str {
    match (OFF.get(i), OFF.get(i + 1)) {
        (Some(&a), Some(&b)) => SUBJECTS.get(a as usize..b as usize).unwrap_or(""),
        _ => "",
    }
}

/// Compose the reject line for `fam_id` into `buf`, NUL-terminated;
/// returns the byte length before the NUL.
///
/// Every line is `<subject><tail>`, and the fifteen arm families
/// share ` method family` between the two — so the table holds one
/// short subject per id and the shared pieces once. Spelled out per
/// id, the 29 lines cost 1.6 KB of `__text` in a class program whose
/// every family was stubbed (s3 rotation 504 census); the subjects
/// alone are a quarter of that. The bytes each id produces are the
/// ones the per-id literals produced — the unit tests pin them.
fn reject_message(fam_id: u64, buf: &mut [u8; REJECT_MSG_CAP]) -> usize {
    let (subj, mid, tail) = match fam_id {
        n if n < ARM_FAMILY_COUNT => (n as usize, REJECT_METHOD_FAMILY, REJECT_DISPATCH_TAIL),
        15 => (SUBJ_NS_STATIC, "", REJECT_DISPATCH_TAIL),
        16..40 => (SUBJ_PRINTER, "", REJECT_DISPATCH_TAIL),
        40..=65 => (
            SUBJ_LINK_FIRST + (fam_id - 40) as usize,
            "",
            if fam_id == FAM_TWIN {
                REJECT_DISPATCH_TAIL
            } else {
                REJECT_LINK_TAIL
            },
        ),
        _ => (SUBJ_FALLBACK, "", REJECT_DISPATCH_TAIL),
    };
    // Filled through the iterator forms rather than range indexing:
    // the indexed forms carry a formatted panic path, and a runtime
    // kernel that owns one drags `core::fmt` into every program that
    // links it (the r503 census).
    let mut len = 0;
    for part in [subject(subj), mid, tail] {
        for (dst, &b) in buf.iter_mut().skip(len).zip(part.as_bytes()) {
            *dst = b;
        }
        len += part.len();
    }
    if let Some(nul) = buf.get_mut(len) {
        *nul = 0;
    }
    len
}

unsafe extern "C" {
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: u64) -> ([u8; REJECT_MSG_CAP], usize) {
        let mut buf = [0u8; REJECT_MSG_CAP];
        let len = reject_message(id, &mut buf);
        assert_eq!(buf[len], 0, "NUL-terminated");
        (buf, len)
    }

    #[track_caller]
    fn assert_line(id: u64, expected: &str) {
        let (buf, len) = line(id);
        assert_eq!(&buf[..len], expected.as_bytes(), "fam {id}");
    }

    /// The composed lines are byte-for-byte the per-id literals they
    /// replaced — a failure line still names the family it came from.
    #[test]
    fn composed_lines_match_the_former_literals() {
        assert_line(0, "str method family stripped (dispatch judgment bug)");
        assert_line(2, "dynobj method family stripped (dispatch judgment bug)");
        assert_line(14, "num method family stripped (dispatch judgment bug)");
        assert_line(
            15,
            "namespace-static world stripped (dispatch judgment bug)",
        );
        assert_line(16, "printer kernel stripped (dispatch judgment bug)");
        assert_line(39, "printer kernel stripped (dispatch judgment bug)");
        assert_line(40, "exotic-array join stripped (link judgment bug)");
        assert_line(
            43,
            "scalar-array subclass drop stripped (link judgment bug)",
        );
        assert_line(50, "struct field-write guard stripped (link judgment bug)");
        assert_line(51, "typed-array exotic sort stripped (link judgment bug)");
        assert_line(52, "typed-array keyed read stripped (link judgment bug)");
        assert_line(
            53,
            "typed-array keyed str read stripped (link judgment bug)",
        );
        assert_line(54, "array map hole mark stripped (link judgment bug)");
        assert_line(55, "unsized cell free stripped (link judgment bug)");
        assert_line(
            56,
            "typed-array keyed any read stripped (link judgment bug)",
        );
        assert_line(57, "struct field-read guard stripped (link judgment bug)");
        assert_line(58, "struct absent set stripped (link judgment bug)");
        assert_line(59, "class method twin stripped (dispatch judgment bug)");
        assert_line(60, "boxed lastIndex read stripped (link judgment bug)");
        assert_line(61, "shift on ro length stripped (link judgment bug)");
        assert_line(62, "unhandled error render stripped (link judgment bug)");
        assert_line(63, "thenable probe stripped (link judgment bug)");
        assert_line(64, "re-parented struct in stripped (link judgment bug)");
        assert_line(65, "exotic struct json walk stripped (link judgment bug)");
        assert_line(u64::MAX, "method family stripped (dispatch judgment bug)");
    }

    /// No id composes past the buffer — the cap is sized off the
    /// widest subject plus the widest tail plus the NUL.
    #[test]
    fn every_line_fits_the_buffer() {
        for id in 0..66u64 {
            assert!(line(id).1 < REJECT_MSG_CAP, "id {id}");
        }
    }
}
