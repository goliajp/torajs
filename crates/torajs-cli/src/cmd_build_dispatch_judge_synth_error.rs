//! The prologue-synthesized native-error world the dispatch judgment
//! ([`crate::cmd_build_dispatch_judge`]) exempts from its scan.

/// The prologue-synthesized native-error constructors
/// (`inject_builtin_classes` — Error plus the §20.5.5/.7/.8
/// family). Their bodies carry the spec ToString(message) coercion
/// in EVERY program, so scanning them would tax the empty program
/// with the obj-world keep. They only run when something CALLS
/// them: a runtime throw passes a constant str message (no arm
/// entry), and a user-code call site is a visible `Call` to one of
/// these names — which re-applies the coercion keep. A user class
/// SHADOWING one of these names makes the ctor user-authored; its
/// call sites still hit the Call rule, so the exemption stays
/// sound (only its internal coercion observation is skipped).
const SYNTH_ERROR_FNS: [&str; 16] = [
    "__cm_Error__ctor",
    "__cm_TypeError__ctor",
    "__cm_RangeError__ctor",
    "__cm_ReferenceError__ctor",
    "__cm_SyntaxError__ctor",
    "__cm_EvalError__ctor",
    "__cm_URIError__ctor",
    "__cm_AggregateError__ctor",
    "__new_Error",
    "__new_TypeError",
    "__new_RangeError",
    "__new_ReferenceError",
    "__new_SyntaxError",
    "__new_EvalError",
    "__new_URIError",
    "__new_AggregateError",
];

/// A prologue-synthesized error-world fn: one of the base names or
/// a synthesized wrapper of one (`__boxed___cm_Error__ctor` — the
/// boxed-entry adapters end with the wrapped name). A user class
/// whose NAME ends in a base spelling (`class Foo__cm_Error`) would
/// be mis-exempted here; the cost is a loud stub TypeError caught
/// by the gate, never a silent wrong answer.
pub(crate) fn is_synth_error_fn(name: &str) -> bool {
    SYNTH_ERROR_FNS
        .iter()
        .any(|b| name == *b || (b.starts_with("__cm_") && name.ends_with(b)))
}
