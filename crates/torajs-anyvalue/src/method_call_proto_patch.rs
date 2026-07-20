//! Builtin-prototype monkey-patch consult — the proto-chain step
//! for primitive / builtin-shaped receivers. When every dispatch
//! arm misses, a patch installed on the receiver's builtin
//! prototype singleton (`Number.prototype.split =
//! String.prototype.split`, `String.prototype.myFn = () => …`)
//! resolves before the final not-a-function TypeError (ES
//! §10.1.9.2 OrdinaryGet step 2 — the receiver's own faces are the
//! per-tag arms, the singleton's own entries are its prototype
//! level). RFC 20260721-string-proto-cluster 刀 3.
//!
//! Cycle posture: the consult runs only on first-level dispatch
//! (`skip_wrapper_expando == false` — a reified-builtin
//! re-dispatch is method-body execution, lookup is over), and a
//! borrowed builtin cell only proceeds through the String-family
//! generic coerce; a non-string-family borrow answers `None`
//! (recorded L3b — an array-generic lane would re-enter the
//! dispatcher and re-consult this table).

use core::ffi::c_void;

use crate::method_call::{closure_cell_entry, invoke_with_this, not_callable};
use crate::method_value::{
    STR_PROTO_FAMILY, builtin_method_family, builtin_method_mid, recv_proto_family,
};
use crate::nanbox::AnyValue;

/// Probe the receiver's builtin prototype singleton for a live own
/// entry under the method name — `Some(result)` when a patch
/// resolved (invoked / coerced / not-callable throw), `None` when
/// there is no patch and the caller keeps its miss exit.
pub(crate) unsafe fn builtin_proto_patch_method(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let fam = recv_proto_family(recv);
        if fam < 0 {
            return None;
        }
        // Peek only — a singleton nobody minted cannot carry a
        // patch, and the miss exit must stay alloc-free.
        let proto = torajs_rc::builtin_proto::__torajs_peek_builtin_prototype(fam);
        if proto.is_null() {
            return None;
        }
        // A known-mid call site carries no name bytes — mint the
        // canonical name from the meta row for the own probe (cold
        // path; the patch table is a monkey-patch surface).
        let mut minted: *mut u8 = core::ptr::null_mut();
        let key: *const u8 = if !name_str.is_null() {
            name_str
        } else if let Some((nm, _)) = torajs_rc::any_method_meta(mid) {
            let s = crate::__torajs_str_alloc_pooled(nm.len() as u64);
            core::ptr::copy_nonoverlapping(nm.as_ptr(), s.add(16), nm.len());
            minted = s;
            s
        } else {
            return None;
        };
        let (tag, value) =
            crate::method_support_proto::proto_own_probe(proto, key as *const c_void);
        if !minted.is_null() {
            crate::__torajs_str_drop(minted as *mut c_void);
        }
        if tag == 5 {
            return None;
        }
        let cell = value as *mut c_void;
        // A borrowed builtin cell (`Number.prototype.split =
        // String.prototype.split`) — String-family cells run the
        // §22.1.3 generic ToString(this) coerce; any other family
        // stays on the miss exit (see module doc).
        if let Some(patch_mid) = builtin_method_mid(cell) {
            let str_family = builtin_method_family(cell) == STR_PROTO_FAMILY;
            return crate::method_call_closure::generic_str_this(
                patch_mid, recv, argv, argc, str_family,
            );
        }
        if let Some((env, entry)) = closure_cell_entry(cell) {
            return Some(invoke_with_this(env, entry, recv, argv, argc));
        }
        Some(not_callable())
    }
}
