//! §7.2.3 IsCallable's other two shapes, for a site that resolved a
//! method value and found no closure pair.
//!
//! Every dispatcher that ends in `not_callable()` reaches it the same
//! way: it probed the value for a `(env, entry)` pair, missed, and
//! read that miss as "this is not callable". The pair probe is a
//! CLOSURE-CELL test, and §7.2.3 names two more shapes that have no
//! pair of their own — a callable Proxy and a class object. So
//! `({ valueOf: new Proxy(fn, {}) }) * 1` said "value is not a
//! function" about a value `typeof` calls a function, and every
//! coercion that walks `valueOf` / `toString` / `@@toPrimitive`
//! inherited that answer.
//!
//! Neither shape can be given a pair to jump to, so neither can be
//! folded into the fast path. What they have instead is a call
//! protocol of their own: §10.5.12 runs the proxy's `apply` trap
//! (with the receiver the caller was going to pass), and §10.2.1
//! step 2 decides what a class object does in a call position. Both
//! already exist; what was missing is any site asking for them.
//!
//! The fast path stays first and untouched at every call site: a
//! real closure never reaches here, so this costs one predicate on
//! the path that was about to throw anyway.

use crate::nanbox::AnyValue;

/// Call `callee` with `this_arg` when it is callable but carries no
/// closure pair. `None` means genuinely not callable, and the caller
/// keeps its own TypeError — the spec spells that message
/// differently per operation, so it is not this module's to raise.
///
/// # Safety
/// `callee` and `this_arg` are live AnyValues; `argv` points at
/// `argc` slots the caller keeps alive across the call.
pub(crate) unsafe fn call_with_this(
    callee: AnyValue,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        // §10.5.12 — the handler decides the call, not the target,
        // and the receiver rides through to the `apply` trap's
        // second argument.
        if crate::proxy::is_proxy(callee) {
            return Some(crate::proxy_callable::__torajs_proxy_apply(
                callee, this_arg, argv, argc,
            ));
        }
        // §10.2.1 step 2 — a class object IS callable; what its
        // [[Call]] does (construct, or refuse) is that function's
        // question, and either answer is the spec's, not a miss.
        crate::construct::call_class_ctor(callee, argv, argc)
    }
}
