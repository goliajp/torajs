//! User [[Prototype]]-chain method dispatch for the dynobj receiver
//! arm (RFC 20260717-user-proto-chain knife 2) — §13.3.6
//! EvaluateCall: the METHOD LOOKUP walks the chain (the member-get
//! recursion covers grandparents), but `this` stays the ORIGINAL
//! receiver — `child.greet()` with `greet` inherited from
//! `Object.create(parent)` runs against `child`.
//!
//! The resolved cell dispatches exactly like an own `ANY_HEAP`
//! entry: a reified builtin cell re-dispatches its carried mid with
//! the child receiver; a closure invokes through the recv-first
//! channel when its env carries `FLAG_CLOSURE_RECV_FIRST`; an
//! accessor entry runs its getter (this = child) and the answer
//! becomes the callee. A chain-resolved non-callable is the
//! resolved-not-callable TypeError (spec: it must NOT fall through
//! to the builtin surface).
//!
//! Recorded boundary: a chain entry STORING undefined reads as a
//! member-get miss (the borrow pair conflates it with full-chain
//! absence), so it falls to the Object.prototype fallback instead
//! of the resolved-not-callable TypeError.

use core::ffi::c_void;

use crate::member_get::{__torajs_any_member_get_tag, __torajs_any_member_get_value};
use crate::member_get_own::user_proto_cell;
use crate::method_call::{closure_boxed_entry, not_callable};
use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// torajs-meta — accessor-pair getter invoke (owned answer).
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv: u64) -> u64;
    /// torajs-throw — pending-throw flag probe.
    fn __torajs_throw_check() -> i64;
}

/// `ANY_ACCESSOR_TAG` mirror (torajs-core `ssa_lower_accessor.rs`).
const ANY_ACCESSOR_TAG: u64 = 6;

/// See the module doc. `Some` = the chain resolved the name (invoked,
/// or the resolved-not-callable TypeError fired); `None` = no user
/// chain or a full-chain miss — the caller proceeds to the inherited
/// Object.prototype fallback.
///
/// # Safety
/// `obj` is a live `Tag::DynObj` heap pointer; `name_str` is a live
/// Str cell; `argv` points at `argc` AnyValue slots the caller keeps
/// alive across the call.
pub(crate) unsafe fn proto_chain_method(
    obj: *mut c_void,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let parent = user_proto_cell(obj)?;
        let key = name_str as *const c_void;
        let ctag = __torajs_any_member_get_tag(parent, key);
        if ctag == 5 {
            return None;
        }
        if ctag == ANY_ACCESSOR_TAG {
            // Getter-as-callee with this = child (mirror of the own
            // accessor arm; the getter's owned answer releases after
            // the invoke).
            let pair = __torajs_any_member_get_value(parent, key) as *const c_void;
            let recv = __torajs_anyv_box_pointer(obj);
            let got = __torajs_accessor_invoke_getter(pair, recv);
            if __torajs_throw_check() != 0 {
                return Some(got);
            }
            if let Some((env, entry)) = closure_boxed_entry(got) {
                let r = crate::method_call::invoke_with_this(env, entry, recv, argv, argc);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
                return Some(r);
            }
            crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
            return Some(not_callable());
        }
        if ctag == 4 {
            let cell = __torajs_any_member_get_value(parent, key);
            // A reified builtin method cell re-dispatches its
            // CARRIED mid with the child receiver.
            if crate::nanbox::is_cell(cell)
                && let Some(mid2) =
                    crate::method_value::builtin_method_mid(crate::nanbox::as_void_ptr(cell))
            {
                // Rotation 204's family-generic gate, chain leg
                // (420-06): the mint family picks its lane BEFORE the
                // shared-mid array-like shortcut. A class object's
                // chain reaches %Function.prototype%'s `toString`,
                // and the shortcut ran §23.1.3.31 array-like join on
                // the ctor dynobj — an empty string, silently — where
                // the function-family lane answers the recorded class
                // source (§20.2.3.5).
                let fam =
                    crate::method_value::builtin_method_family(crate::nanbox::as_void_ptr(cell));
                if let Some(out) = crate::method_call_closure::generic_builtin_this(
                    mid2,
                    __torajs_anyv_box_pointer(obj),
                    argv,
                    argc,
                    fam,
                ) {
                    return Some(out);
                }
                if crate::method_call_arraylike_concat::obj_supported(mid2) {
                    return Some(crate::method_call_arraylike_concat::obj_method(
                        obj, mid2, recv_slot, argv, argc,
                    ));
                }
                return Some(crate::method_call::any_method_call_inner(
                    __torajs_anyv_box_pointer(obj),
                    mid2,
                    core::ptr::null(),
                    recv_slot,
                    argv,
                    argc,
                ));
            }
            // A reified class-method cell resolved through the chain
            // (knife B cut 1) — the child rides the env slot.
            if crate::nanbox::is_cell(cell)
                && let Some(adapter) = crate::method_value_class::class_method_adapter(
                    crate::nanbox::as_void_ptr(cell),
                )
            {
                // 398-02 — a STATIC face (`(tag 0, twin ≠ 0)`) has no
                // receiver channel in its mono body (a static `this`
                // resolved to the DECLARING class at compile time),
                // and the chain is exactly where an INHERITED static
                // resolves: `X.make(6)` on `const X: any = Sub` found
                // Base's cell here and silently constructed Base. The
                // child (§13.3.6.2's receiver) rides the `__smany_`
                // twin instead — the verdict `invoke_with_this`
                // already applies to the `.call` / `.apply`
                // spellings. Instance faces keep the mono env path.
                let cellp = crate::nanbox::as_void_ptr(cell);
                let (face_tag, twin) = crate::method_value_class::class_method_face_guard(cellp);
                if face_tag == 0 && twin != 0 {
                    let recv = __torajs_anyv_box_pointer(obj);
                    return Some(
                        crate::method_call_closure_dispatch::invoke_boxed_recv_first(
                            cellp, twin, recv, argv, argc,
                        ),
                    );
                }
                // Blade 3 guard, chain edition — the mono body reads
                // the receiver at baked class offsets, and THIS arm's
                // receiver is always a dynobj (`Object.create(
                // C.prototype)` instances), never the owning class's
                // struct layout, so a guarded instance face always
                // rides the `__cmany_` twin. Pre-fix the mono ran
                // against the dynobj and read nanbox bits as field
                // values. twin 0 keeps the mono path (this-free
                // bodies never read the receiver; super-route bodies
                // are the recorded residue, same as invoke_with_this).
                if face_tag != 0 && twin != 0 {
                    let recv = __torajs_anyv_box_pointer(obj);
                    return Some(
                        crate::method_call_closure_dispatch::invoke_boxed_recv_first(
                            cellp, twin, recv, argv, argc,
                        ),
                    );
                }
                return Some(crate::method_call_closure_dispatch::invoke_boxed(
                    obj, adapter, argv, argc,
                ));
            }
            if let Some((env, entry)) = closure_boxed_entry(cell) {
                // Receiver-first closures bind the child as `this`
                // (RFC 20260717-objlit-anylane-recv knife 1 flag).
                let flags = (env as *const u8).add(6).cast::<u16>().read();
                if flags & torajs_rc::FLAG_CLOSURE_RECV_FIRST != 0 {
                    let recv = __torajs_anyv_box_pointer(obj);
                    return Some(crate::method_call::invoke_boxed_recv_first(
                        env, entry, recv, argv, argc,
                    ));
                }
                return Some(crate::method_call::invoke_boxed(env, entry, argv, argc));
            }
        }
        // Chain-resolved non-callable data — the TypeError, never
        // the builtin fallthrough.
        Some(not_callable())
    }
}
