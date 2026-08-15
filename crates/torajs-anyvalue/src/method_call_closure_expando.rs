//! What an own expando entry on a FUNCTION value resolves to —
//! split from `method_call_closure.rs` at the file-size cap.
//!
//! Its host answers "what does calling a method on a function value
//! mean"; this answers the narrower question the expando arm has to
//! settle first, and the two only ever shared a line.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

unsafe extern "C" {
    /// torajs-dynobj — own-property probe (5 = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — own-property value read (borrowed).
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> AnyValue;
    /// Borrowed heap-pointer box — no `rc_inc`, same use as the
    /// dynobj arm's receiver.
    fn __torajs_anyv_box_pointer(p: *mut c_void) -> AnyValue;
}

/// A plain closure stored as an expando ON A FUNCTION VALUE, invoked
/// with the function as its receiver. `None` leaves the call to the
/// props-bag walk exactly as before.
///
/// The expando arm delegates the whole dispatch to
/// [`crate::method_call_dynobj::dynobj_method`] against the props
/// bag, and every `this` in there is that bag. For a property READ
/// that is indistinguishable — the properties are in the bag — which
/// is why `K.s = function () { return this.tag }` has always
/// answered. It is not indistinguishable for the function itself:
///
/// ```text
/// const K: any = function () {};
/// K.self = function () { return this };
/// K.self() === K        // bun true, tr false (it was the bag)
/// typeof K.self()       // bun "function", tr "object"
/// ```
///
/// Only a receiver-first closure is intercepted: without that flag
/// `invoke_boxed` never reads a receiver, so the bag walk and this
/// path are the same call. A reified builtin cell and a class-method
/// adapter carry their own dispatch and are left where they were.
pub(crate) unsafe fn expando_this_is_the_function(
    ptr: *mut c_void,
    props: *const c_void,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let key = name_str as *const c_void;
        // 4 = ANY_HEAP, a plain cell-valued own entry.
        if __torajs_dynobj_get_tag(props, key) != 4 {
            return None;
        }
        let cell = __torajs_dynobj_get_value(props, key);
        if !is_cell(cell)
            || crate::method_value::builtin_method_mid(as_void_ptr(cell)).is_some()
            || crate::method_value_class::class_method_adapter(as_void_ptr(cell)).is_some()
        {
            return None;
        }
        let (env, entry) = crate::method_call::closure_boxed_entry(cell)?;
        let flags = (env as *const u8).add(6).cast::<u16>().read();
        if flags & torajs_rc::FLAG_CLOSURE_RECV_FIRST == 0 {
            return None;
        }
        Some(crate::method_call::invoke_boxed_recv_first(
            env,
            entry,
            __torajs_anyv_box_pointer(ptr),
            argv,
            argc,
        ))
    }
}

/// A method resolved through a re-parented function value's user
/// [[Prototype]] chain (405-01 substrate): `Object.setPrototypeOf(D,
/// P)` puts `P` on `D`'s chain, so `D.s()` walks up until some
/// ancestor's expando bag owns the name — and invokes it with `D` as
/// the receiver, which is what §10.1.8.1 GetV + §13.3.6 EvaluateCall
/// compose to. Per hop the entry resolves exactly like an own
/// expando: a receiver-first closure takes the ORIGINAL receiver,
/// anything else goes through the props-bag dispatch unchanged (the
/// same approximation the own-entry path makes).
///
/// A REAL class object on the chain answers through its member-get
/// face (405-01 face 2): the extends lane links `Object.
/// setPrototypeOf(D, P)` where P may be a hoisted or top-level
/// class — a DynObj carrying the class-ctor flag — and `D.s()` must
/// resolve P's statics (own and inherited; the member-get face
/// covers P's parent chain) with the ORIGINAL receiver, per
/// §10.1.8.1 GetV + §13.3.6 EvaluateCall. Any other non-closure
/// ancestor still ends the walk (recorded boundary); so does an
/// explicit-null or never-re-parented link. The set path refuses
/// cycles, so the walk terminates.
pub(crate) unsafe fn proto_chain_method(
    recv: *mut c_void,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let mut cur = recv;
        loop {
            let parent = crate::member_get_own::closure_user_proto(cur)??;
            if !is_cell(parent) {
                return None;
            }
            let pp = as_void_ptr(parent);
            let (_, t) = crate::member_get::recv_cell(parent)?;
            if t == Tag::DynObj as u16 {
                return class_object_hop(recv, parent, name_str, argv, argc);
            }
            if t != Tag::Closure as u16 {
                return None;
            }
            let props = crate::member_get_layout::closure_props(pp);
            if !props.is_null() && __torajs_dynobj_get_tag(props, name_str as *const c_void) != 5 {
                if let Some(r) = expando_this_is_the_function(recv, props, name_str, argv, argc) {
                    return Some(r);
                }
                return Some(crate::method_call_dynobj::dynobj_method(
                    props as *mut c_void,
                    mid,
                    name_str,
                    core::ptr::null_mut(),
                    argv,
                    argc,
                ));
            }
            cur = pp;
        }
    }
}

/// Resolve `name` against a class object the chain walk reached and
/// invoke with the original function-value receiver (405-01 face 2).
///
/// The member-get face covers the class's own statics and its parent
/// chain (§15.7.14 links class ctors), so one read answers inherited
/// statics too. The resolved cell dispatches through
/// [`crate::method_call::invoke_with_this`] with the original
/// receiver — the same verdict every `.call` / detached-value
/// spelling gets (static faces ride their `__smany_` twin,
/// this-free faces run bare, plain closures honor their
/// receiver-first flag), so the two spellings cannot disagree. A
/// resolved non-callable is the §13.3.6 TypeError; an accessor
/// entry is the recorded residue (`None` keeps the caller's
/// no-such-method exit).
unsafe fn class_object_hop(
    recv: *mut c_void,
    parent: AnyValue,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let key = name_str as *const c_void;
        let ctag = crate::member_get::__torajs_any_member_get_tag(parent, key);
        // 5 = full-chain miss; 6 = accessor (residue).
        if ctag != 4 {
            return None;
        }
        let cell = crate::member_get::__torajs_any_member_get_value(parent, key);
        if let Some((env, entry)) = crate::method_call::closure_boxed_entry(cell) {
            return Some(crate::method_call::invoke_with_this(
                env,
                entry,
                __torajs_anyv_box_pointer(recv),
                argv,
                argc,
            ));
        }
        Some(crate::method_call::not_callable())
    }
}
