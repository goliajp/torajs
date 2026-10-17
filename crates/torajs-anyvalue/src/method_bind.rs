//! `Function.prototype.bind` (chunk 714) — mints a bound-function
//! cell off any callable that travels the `any` world.
//!
//! ES semantics projected:
//!
//! - Binding a reified builtin method (`s.toUpperCase.bind(s)`)
//!   captures the thisArg as the future RECEIVER — calling the
//!   bound function re-dispatches the original method id on it.
//! - Binding a plain closure captures nothing but the partial
//!   arguments (a torajs closure body cannot reference `this`).
//! - Binding a bound function nests: the outer thisArg is ignored
//!   (a bound this cannot be re-bound, ES §20.2.3.2) and the
//!   partial argument lists concatenate outside-in.
//! - Binding a CLASS constructor (a dynobj carrying
//!   `FLAG_DYNOBJ_CLASS_CTOR`, RFC 20260915) holds the class object
//!   as a kind-1 target: `new` on the bound cell constructs the
//!   class with boundArgs ++ args (§10.4.1.2, `__torajs_anyv_construct`
//!   recurses on the target), while a plain call is the class's own
//!   "cannot be invoked without 'new'" TypeError.
//! - The bound function is an ordinary callable — `typeof` answers
//!   "function", it rides HOF callbacks and `.call` / `.apply`
//!   (whose thisArg is likewise ignored), and expandos work.
//!
//! Cell layout — a closure-env shape (every callable probe keeps
//! working) with the bound state in the capture region:
//!
//!   offset  0 — universal header (rc=1, Tag::Closure, flags=0)
//!   offset  8 — fn_addr = the throwing native entry (method_value)
//!   offset 16 — drop_fn = [`bound_drop`]
//!   offset 24 — props (lazy expando bag, NULL)
//!   offset 32 — boxed_entry = [`bound_entry`]
//!   offset 40 — trace_fn = [`bound_trace`] (RFC 20260717
//!               closure-env-cycle — enumerates the held
//!               target/this/args children for the cycle collector)
//!   offset 48 — kind: 0 = builtin method id / 1 = closure-shaped
//!               target cell (held +1)
//!   offset 56 — target (method id or cell pointer)
//!   offset 64 — bound this (AnyValue, held ref on cells)
//!   offset 72 — bound argc
//!   offset 80 — bound length (f64 bits) — §20.2.3.2 steps 5-7, read
//!               off the TARGET at bind time
//!   offset 88 — bound name (owned Str cell, `bound <target>`) —
//!               steps 8-14, likewise read at bind time
//!   offset 96 — extra `bound ` prefixes owed in front of the name
//!               (see [`BOUND_NAME_PREFIXES_OFF`])
//!   offset 104+ — bound args (AnyValue each, held ref on cells)
//!
//! `length` and `name` are resolved ONCE, when the cell is minted,
//! because that is when the spec reads them: a target whose `name`
//! getter throws makes `.bind()` itself throw (§20.2.3.2 step 13),
//! and a `defineProperty` on the target AFTER the bind changes
//! nothing about the bound function. Resolving them lazily got both
//! backwards, and the second one silently. Only the spelling of a
//! name shared with an unrenamed bound target waits for a read: the
//! value is fixed at mint, the joined string is built on demand.
//!
//! Unlike the interned method cells this cell is OWNED — rc-managed,
//! dropped through `drop_fn` like any synthesized closure env.

use core::ffi::c_void;

use crate::STR_HDR_SIZE;
use crate::method_call::not_callable;
use crate::method_value::{builtin_method_family, builtin_method_mid, native_entry};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// torajs-rc / torajs-value-drop — NaN-box-safe release + the
    /// universal heap dropper (props bag, held target cell).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-cycle — scrub a normal-dropping cell from the cycle
    /// root buffer (RFC 20260717 knife 3; no-op when never buffered).
    fn __torajs_cycle_unbuffer(p: *mut c_void);
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — is a catchable throw already recorded? The
    /// bind-time property reads run user getters, and step 6.a / 13
    /// both propagate an abrupt completion instead of minting a cell.
    fn __torajs_throw_check() -> i64;
    /// torajs-str — the `bound ` prefix join and its release.
    fn __torajs_str_alloc(p: *const u8, len: i64) -> *mut u8;
    /// torajs-str — the RangeError for a Str past the length limit,
    /// answering an empty Str.
    fn __torajs_str_too_long() -> *mut u8;
    fn __torajs_str_drop(p: *mut c_void);
}

const CLOSURE_FN_ADDR_OFF: usize = 8;
const CLOSURE_DROP_FN_OFF: usize = 16;
const CLOSURE_PROPS_OFF: usize = 24;
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;
const CLOSURE_TRACE_FN_OFF: usize = 40;
// r682 file-size split — "how a bound cell is made" moved to the
// child module (the parent crossed 500 when the callback thunk
// joined it). Same-crate child: every private item here stays
// reachable through `super::`.
pub(crate) mod mint;
// The call path, split out the same way when walking the chain made
// it outgrow the parent.
mod entry;
use entry::bound_entry;
// `name`: made at bind time, spelled on read.
mod name;
use name::bind_time_name;
pub(crate) use name::bound_cell_name;

const BOUND_KIND_OFF: usize = 48;
const BOUND_TARGET_OFF: usize = 56;
const BOUND_THIS_OFF: usize = 64;
const BOUND_ARGC_OFF: usize = 72;
const BOUND_LEN_OFF: usize = 80;
const BOUND_NAME_OFF: usize = 88;
/// Binding a bound function whose `name` is still its intrinsic one
/// shares the target's name cell and counts one more `bound ` prefix
/// here instead of materializing the joined string. A chain of N
/// binds would otherwise hold N names of growing length — O(N^2)
/// bytes for an O(N) chain. The full string is spelled on read.
const BOUND_NAME_PREFIXES_OFF: usize = 96;
const BOUND_ARGS_OFF: usize = 104;
/// The longest Str the runtime can hold, in code units; the same
/// limit as `torajs_str::max_len::STR_MAX_LEN`.
const STR_MAX_LEN: usize = (1 << 31) - 1;

fn bound_layout(argc: usize) -> core::alloc::Layout {
    core::alloc::Layout::from_size_align(BOUND_ARGS_OFF + argc * 8, 8).unwrap()
}

/// The `bind` arm — `recv.bind(thisArg, ...partial)` where `recv`
/// is the closure-tagged cell being bound. Returns the fresh bound
/// cell as an owned box, or the not-callable TypeError.
///
/// # Safety
/// `ptr` is a valid `Tag::Closure` heap pointer; `argv` points at
/// `argc` BORROWED AnyValue slots.
pub(crate) unsafe fn bind_cell(ptr: *mut c_void, argv: *const u64, argc: i64) -> AnyValue {
    unsafe {
        // Target classification: a reified builtin method re-binds
        // its receiver; anything with a boxed dual entry (plain
        // closures, other bound cells) dispatches through it.
        let (kind, target) = if let Some(mid) = builtin_method_mid(ptr) {
            (0u64, pack_builtin_target(mid, builtin_method_family(ptr)))
        } else if crate::construct::is_callable(crate::nanbox::box_void_ptr(ptr)) {
            // §20.2.3.2 step 2 is IsCallable, so the gate is §7.2.3
            // — listing the shapes here admitted the closure and the
            // class and turned a callable Proxy away.
            torajs_rc::__torajs_rc_inc(ptr);
            (1u64, ptr as u64)
        } else {
            return not_callable();
        };
        let this_arg = if argc >= 1 { *argv } else { VALUE_UNDEFINED };
        let n = (argc - 1).max(0) as usize;

        // §20.2.3.2 steps 5-7 then 8-14, in that order and both
        // before anything is handed back: each one can run a user
        // getter, and an abrupt completion from either leaves no
        // bound function at all. Done BEFORE the allocation so the
        // abort path has nothing to unwind.
        let len = bind_time_length(kind, target, n);
        if __torajs_throw_check() != 0 {
            if kind == 1 {
                __torajs_value_drop_heap(target as *mut c_void);
            }
            return VALUE_UNDEFINED;
        }
        let name = bind_time_name(kind, target);
        if __torajs_throw_check() != 0 {
            __torajs_str_drop(name.0 as *mut c_void);
            if kind == 1 {
                __torajs_value_drop_heap(target as *mut c_void);
            }
            return VALUE_UNDEFINED;
        }

        mint::mint_bound(kind, target, this_arg, argv.add(1), n, len, name)
    }
}

/// A kind-0 bound target packs `mid | (family + 1) << 32`, the same
/// encoding the interned method cell uses for its capture slot.
/// Carrying the family (rather than the bare mid) is what keeps a
/// bound cell's dispatch identical to the `.call` path's: the
/// family-generic gate — §21.1.3 thisNumberValue / §20.3.3
/// thisBooleanValue brand checks, the §22.1.3 ToString(this) lane —
/// is selected by family, so dropping it made
/// `Boolean.prototype.toString.bind({})()` answer silently instead of
/// throwing. It also keeps the `.length` reflection family-aware.
fn pack_builtin_target(mid: i64, fam: i64) -> u64 {
    (mid as u32 as u64) | (((fam + 1) as u32 as u64) << 32)
}

/// Inverse of [`pack_builtin_target`].
pub(crate) fn unpack_builtin_target(target: u64) -> (i64, i64) {
    ((target as u32) as i64, ((target >> 32) as i64) - 1)
}

/// Bound-cell metadata `(kind, target, bound_argc)` — `None` for
/// every other closure shape. Chunk 719 — the `.name` / `.length`
/// reflection reads key off this.
///
/// Discriminated by the DROP fn's address — the destructor is the
/// cell-type brand (mint and drop are minted as a pair, nothing
/// else ever writes [`bound_drop`] into a drop slot). It used to
/// compare the boxed dual entry against [`bound_entry`], but that
/// address compare link-rooted the whole method-dispatch universe
/// from every generic kernel that merely asked "is this bound"
/// (bound_entry -> closure dispatch -> ns_static_cell -> the
/// dispatch clique; anyvalue WHO census, rotation 494). Same cure
/// as the `FLAG_CLOSURE_NS_STATIC` precedent, except the Closure
/// flag byte is full and bits 13-14 are the collector's color
/// field — so the brand rides the drop slot instead. `bound_drop`
/// only reaches the universal drop machinery, which is live in
/// every program anyway.
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell.
pub(crate) unsafe fn bound_cell_meta(ptr: *mut c_void) -> Option<(u64, u64, usize)> {
    unsafe {
        let cell = ptr.cast::<u8>();
        let dropf = *(cell.add(CLOSURE_DROP_FN_OFF) as *const u64);
        if dropf != bound_drop as *const () as u64 {
            return None;
        }
        Some((
            *(cell.add(BOUND_KIND_OFF) as *const u64),
            *(cell.add(BOUND_TARGET_OFF) as *const u64),
            *(cell.add(BOUND_ARGC_OFF) as *const u64) as usize,
        ))
    }
}

/// The bound cell a kind-1 `target` points at, when it is one — a
/// class object or a Proxy target is a different cell shape, and
/// [`bound_cell_meta`] may only look at a closure.
///
/// # Safety
/// `target` is a live kind-1 target pointer.
unsafe fn bound_target_cell(target: u64) -> Option<*mut u8> {
    unsafe {
        let p = target as *mut c_void;
        if (p.cast::<u8>().add(4) as *const u16).read() != torajs_rc::Tag::Closure as u16 {
            return None;
        }
        bound_cell_meta(p).map(|_| p.cast::<u8>())
    }
}

/// §20.2.3.2 steps 5-7 resolved against the target, with the partial
/// count already subtracted — the whole of what the bound function's
/// `length` will ever answer.
///
/// # Safety
/// `target` is a live packed builtin id (kind 0) or closure-shaped
/// cell pointer (kind 1).
unsafe fn bind_time_length(kind: u64, target: u64, partials: usize) -> f64 {
    unsafe {
        let tlen = if kind == 0 {
            let (mid, fam) = unpack_builtin_target(target);
            crate::method_value::builtin_method_arity_for(mid, fam).map_or(0.0, |a| a as f64)
        } else {
            crate::len_get::bound_target_length(target)
        };
        if tlen.is_infinite() {
            // Step 6.b.i / 6.b.ii — +infinity stays, -infinity is 0;
            // neither subtracts the bound arguments.
            return if tlen > 0.0 { tlen } else { 0.0 };
        }
        (tlen - partials as f64).max(0.0)
    }
}

/// The bound function's resolved `length` / `name`, read at mint.
///
/// # Safety
/// `ptr` is a live bound cell (`bound_cell_meta` answered `Some`).
pub(crate) unsafe fn bound_cell_length(ptr: *mut c_void) -> f64 {
    unsafe { *((ptr as *const u8).add(BOUND_LEN_OFF) as *const f64) }
}

/// Raw view of a bound cell's partial-argument block — the construct
/// kernel's §10.4.1.2 concatenation reads the slots in place (they
/// stay the cell's own BORROWED references).
///
/// # Safety
/// `ptr` is a live bound cell (`bound_cell_meta` answered `Some`).
pub(crate) unsafe fn bound_args_ptr(ptr: *mut c_void) -> *const u64 {
    unsafe { (ptr as *const u8).add(BOUND_ARGS_OFF) as *const u64 }
}

/// drop_fn of a bound cell — the universal Closure drop arm calls
/// this on hit-zero; release every captured reference, the lazy
/// props bag, then the block itself.
/// The bound cell's trace fn (RFC 20260717 closure-env-cycle knife
/// 2) — enumerates the same held children [`bound_drop`] releases,
/// minus the +24 props bag (fixed shared offset; the collector's
/// Closure arm reads it directly). Children are raw slot payloads —
/// AnyValue bits for this/args — and the collector's visit callback
/// owns the cell-like gate, mirroring the SSA-emitted
/// `__env_trace_<fn>` contract.
unsafe extern "C" fn bound_trace(
    env: *mut c_void,
    visit: unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void),
    ctx: *mut c_void,
) {
    unsafe {
        let cell = env.cast::<u8>();
        if *(cell.add(BOUND_KIND_OFF) as *const u64) == 1 {
            let slot = cell.add(BOUND_TARGET_OFF);
            visit(
                0,
                *(slot as *const u64) as *mut c_void,
                slot as *mut c_void,
                ctx,
            );
        }
        let this_slot = cell.add(BOUND_THIS_OFF);
        visit(
            1,
            *(this_slot as *const u64) as *mut c_void,
            this_slot as *mut c_void,
            ctx,
        );
        let bn = *(cell.add(BOUND_ARGC_OFF) as *const u64) as usize;
        for i in 0..bn {
            let slot = cell.add(BOUND_ARGS_OFF + i * 8);
            visit(
                2 + i as i64,
                *(slot as *const u64) as *mut c_void,
                slot as *mut c_void,
                ctx,
            );
        }
    }
}

unsafe extern "C" fn bound_drop(env: *mut c_void) {
    unsafe {
        // Scrub the cycle root buffer first — a buffered bound cell
        // that normal-drops here would leave a dangling entry (RFC
        // 20260717 knife 3; cheap no-op when FLAG_BUFFERED is clear).
        __torajs_cycle_unbuffer(env);
        let cell = env.cast::<u8>();
        let props = *(cell.add(CLOSURE_PROPS_OFF) as *const u64);
        if props != 0 {
            __torajs_value_drop_heap(props as *mut c_void);
        }
        let this_v = *(cell.add(BOUND_THIS_OFF) as *const u64);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(this_v);
        let bn = *(cell.add(BOUND_ARGC_OFF) as *const u64) as usize;
        for i in 0..bn {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(
                *(cell.add(BOUND_ARGS_OFF + i * 8) as *const u64),
            );
        }
        __torajs_str_drop(*(cell.add(BOUND_NAME_OFF) as *const u64) as *mut c_void);
        if *(cell.add(BOUND_KIND_OFF) as *const u64) == 1 {
            let target = *(cell.add(BOUND_TARGET_OFF) as *const u64);
            __torajs_value_drop_heap(target as *mut c_void);
        }
        std::alloc::dealloc(cell, bound_layout(bn));
    }
}
