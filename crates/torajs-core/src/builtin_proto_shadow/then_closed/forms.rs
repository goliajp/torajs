//! The name tables of the `then` closure proof. Each list says what is
//! known to be harmless; a name outside it opens the proof.

use crate::ast::{Ast, Expr, ExprId};
use crate::ssa::id_hash::HashSet;

/// Globals that answer any key, run code from a string, hand out the
/// global object or a module the program does not hold, or install a
/// property by name: any mention opens the proof, wherever it stands.
/// `then` is here too: a binding of that name is a global object
/// property at a script's top level.
pub(super) fn ident_opens(n: &str) -> bool {
    matches!(
        n,
        "Reflect"
            | "Proxy"
            | "eval"
            | "Function"
            | "globalThis"
            | "global"
            | "self"
            | "window"
            | "require"
            | "module"
            | "exports"
            | "then"
            | "__proto__"
            | "__defineGetter__"
            | "__defineSetter__"
            | "__lookupGetter__"
            | "__lookupSetter__"
    )
}

/// Globals with statics that define properties or prototypes next to
/// statics that do not: a mention clears only as the object of a
/// member read of one of [`gated_member`]'s names, or as the right of
/// an `instanceof`.
pub(super) fn gated_root(n: &str) -> bool {
    matches!(n, "Object" | "JSON" | "Promise")
}

/// The statics of a [`gated_root`] that define no property and set no
/// prototype. `JSON.parse` runs a reviver given as a second argument,
/// so it clears only where it is called without one ([`vetted_roots`]).
pub(super) fn gated_member(root: &str, name: &str) -> bool {
    match root {
        "Object" => matches!(
            name,
            "keys"
                | "values"
                | "entries"
                | "freeze"
                | "isFrozen"
                | "seal"
                | "isSealed"
                | "preventExtensions"
                | "isExtensible"
                | "getPrototypeOf"
                | "is"
                | "hasOwn"
                | "getOwnPropertyNames"
                | "getOwnPropertySymbols"
        ),
        "JSON" => name == "stringify",
        "Promise" => matches!(
            name,
            "resolve" | "reject" | "all" | "allSettled" | "any" | "race" | "withResolvers" | "try"
        ),
        _ => false,
    }
}

/// The mentions of a [`gated_root`] that clear.
pub(super) fn vetted_roots(ast: &Ast) -> HashSet<ExprId> {
    let mut vetted = HashSet::default();
    for e in &ast.exprs {
        match e {
            Expr::Member { obj, name } | Expr::OptChain { obj, name } => {
                if let Expr::Ident(root) = ast.get_expr(*obj)
                    && gated_member(root, name)
                {
                    vetted.insert(*obj);
                }
            }
            Expr::InstanceOf { rhs, .. } => {
                vetted.insert(*rhs);
            }
            Expr::Call { callee, args } | Expr::OptCall { callee, args } if args.len() <= 1 => {
                if let Expr::Member { obj, name } = ast.get_expr(*callee)
                    && name == "parse"
                    && matches!(ast.get_expr(*obj), Expr::Ident(r) if r == "JSON")
                {
                    vetted.insert(*obj);
                }
            }
            _ => {}
        }
    }
    vetted
}

/// A builtin global whose `.prototype` is a builtin prototype.
pub(super) fn builtin_global(n: &str) -> bool {
    crate::ssa_lower_typeof_ns::builtin_global_typeof(n) == Some("function") || n == "Iterator"
}

/// Member names that reach %Object.prototype%'s `__proto__` accessor or
/// its Annex B definers (B.2.2), or %Function% through a function's
/// `constructor`.
pub(super) fn danger_member(n: &str) -> bool {
    matches!(
        n,
        "__proto__"
            | "__defineGetter__"
            | "__defineSetter__"
            | "__lookupGetter__"
            | "__lookupSetter__"
            | "constructor"
    )
}

/// Keys a computed access may not name: `then`, a [`danger_member`],
/// or `prototype`.
pub(super) fn danger_key(k: &str) -> bool {
    k == "then" || k == "prototype" || danger_member(k)
}

/// String literals that open the proof wherever they stand: a key that
/// names `then` or the `__proto__` accessor, or a definer. `constructor`
/// and `prototype` are left to the key and member walks: the class
/// lowering spells them as strings.
pub(super) fn danger_string(k: &str) -> bool {
    k == "then" || (danger_member(k) && k != "constructor")
}

/// The compiler's own helpers (`__torajs_<h>`) known to define no
/// property under a key the program does not spell and to set no
/// prototype outside the ordinary chain (`proto_link_fresh` links the
/// class prototypes the class tree declares). The `super` property kernels
/// take their key as an argument, which the call walk checks. Any
/// other helper opens the proof.
pub(super) fn helper_defines_nothing(h: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "arguments",
        "arr_subclass_",
        "boolean_wrapper_subclass_",
        "ctor_ret_",
        "date_",
        "dstr_",
        "error_",
        "finreg_subclass_",
        "iife_extra_",
        "map_subclass_",
        "math_",
        "number_wrapper_subclass_",
        "priv_",
        "regex_subclass_",
        "script_preflight_",
        "set_subclass_",
        "string_wrapper_subclass_",
        "super_prop_",
        "typedarray_subclass_",
        "unit_static_",
        "using_",
        "weakmap_subclass_",
        "weakref_subclass_",
        "weakset_subclass_",
    ];
    matches!(
        h,
        "argc"
            | "argc_len"
            | "argv"
            | "body_arguments"
            | "class_accessor_reify"
            | "class_call_throw"
            | "class_register"
            | "class_static_accessor_reify"
            | "class_static_own_move_to_end"
            | "class_unit_wire"
            | "gen_argv"
            | "genfn_chain"
            | "global_fn_declare"
            | "global_var_declare"
            | "in_op"
            | "my_class_ref"
            | "new_target_register"
            | "obj_rest"
            | "proto_link_fresh"
            | "proto_register"
            | "register_native_error"
            | "retv"
            | "static_field_reify"
            | "static_method_reify"
            | "template_object"
            | "this_binding_throw"
            | "undef_str"
            | "unit_home"
    ) || PREFIXES.iter().any(|p| h.starts_with(p))
}
