//! `Error` + native-error subclass injection pass — chunk 362,
//! extracted from ast.rs.
//!
//! Pub entry `inject_builtin_classes` (P4.6 / P7.1) synthesizes
//! `class Error { message; name; stack; }` + the four spec §20.5.5
//! subclasses (`TypeError` / `RangeError` / `SyntaxError` /
//! `ReferenceError`) as `Stmt::ClassDecl` and splices them at
//! `ast.stmts[0..0]` so the rest of the desugar pipeline processes
//! them as ordinary user classes. Injection is idempotent +
//! usage-gated (a user declaration of `Error` overrides the whole
//! hierarchy; each subclass gated on reference/implied/runtime-thrown
//! minus user-shadowing).
//!
//! Four siblings carry the pieces that would push this file past
//! the 500-line limit: `inject_builtin_classes_build` (the root
//! `class Error` and the NativeError subclass shape, plus the two
//! expression mints their ctors share),
//! `inject_builtin_classes_cause` (§20.5.8.1, the `options` /
//! `cause` face every ctor accepts), `inject_builtin_classes_data`
//! (§20.5.7 / §20.5.8, the subclasses whose ctors carry own data
//! params ahead of `message`) and `inject_builtin_classes_message`
//! (§20.5.1.1 step 3, the root ctor's message coercion +
//! own-absence install).

use super::inject_builtin_classes_build::{build_error_class, build_error_subclass};
use super::inject_builtin_classes_reach::instance_shape_observable;
use super::{Ast, Expr, Stmt};

/// P4.6 / P7.1 — inject built-in class declarations for `Error` and
/// its four standard subclasses (`TypeError` / `RangeError` /
/// `SyntaxError` / `ReferenceError`, spec §20.5.5) so user code can
/// `new TypeError(msg)` directly AND `class MyError extends Error`
/// flows through the existing user-class ClassDecl machinery in
/// `desugar_classes`. Pre-fix `new Error("oops")` panicked at
/// check.rs with "internal: `new Error` reached check.rs (desugar
/// didn't run?)" because no factory FnDecl was synthesized.
///
/// Shapes (spec §20.5): `Error` has `message` / `name` string fields
/// and a one-arg ctor; each subclass `extends Error`, forwards via
/// `super(message)`, and sets `this.name` to its own name. Other
/// Error surface area (.stack DWARF capture, .toString format) lands
/// incrementally as follow-up substrate.
///
/// Runs BEFORE `desugar_classes` so the synth ClassDecls get
/// processed normally — synthesizes `__new_<C>` factory /
/// `__cm_<C>__ctor` / `__class_<C>` (via synthesize_class_globals
/// later) and registers each in the class_name_to_tag map so
/// instanceof chain walks reach them.
///
/// Idempotent + usage-gated:
/// - If the user declares their own `class Error`, the whole error
///   hierarchy is theirs — skip all injection (preserves the P4.6
///   stdlib-override contract, and avoids the desugar
///   declaration-order panic that an injected `Sub extends Error`
///   prepended ahead of a user `class Error` would trigger).
/// - Each subclass is injected only when referenced and not
///   user-shadowed by a same-named top-level declaration of any kind.
/// - `Error` is injected when referenced directly OR implied by any
///   wanted subclass (subclasses extend it). Nothing is injected for
///   programs that never mention the error hierarchy (compile-time
///   neutral).
/// Injection-reachability gate override — `true` keeps every program
/// on today's full injection (see the 刀 B comment at the use site).
/// Held open through rotation 496 while the RFC's dependency list
/// (seven latent bugs the injected hierarchy had been masking, plus a
/// gate-open sweep) was driven to zero; released in rotation 497.
const GATE_HELD_OPEN: bool = false;

/// Does the program declare `name` at module top level itself? The
/// injection has always stepped aside for a user `class Error`, but
/// it asked only about `ClassDecl` — so a program binding the name
/// with `function` / `let` / `const` / `var` got the injected class
/// spliced in AHEAD of its own declaration, and every `new <name>()`
/// in the file built the intrinsic instead of the thing the program
/// wrote. Silently, because the two answer to the same name: only an
/// identity comparison tells them apart, which is precisely what
/// test262's `harness/assert-throws-custom-typeerror` does.
///
/// Module top level is the whole question. A declaration nested in a
/// function body shadows only inside that body, and the injected
/// class is still what the outer scope must resolve to.
fn user_declares_top_level(ast: &Ast, name: &str) -> bool {
    ast.stmts.iter().any(|s| match s {
        Stmt::ClassDecl { name: n, .. }
        | Stmt::FnDecl { name: n, .. }
        | Stmt::LetDecl { name: n, .. } => n == name,
        _ => false,
    })
}

pub fn inject_builtin_classes(ast: &mut Ast) {
    if user_declares_top_level(ast, "Error") {
        return;
    }

    // The standard NativeError subclasses (spec §20.5.5). The two
    // data-carrying subclasses whose ctors do NOT share the (message)
    // shape — AggregateError (errors, message) §20.5.7 and
    // SuppressedError (error, suppressed, message) §20.5.8 — ride
    // `build_error_data_subclass` below instead (rotation 234; the
    // RFC 20260718 boundary that excluded them was about THIS array's
    // one-shape builder, not about the classes).
    const ERROR_SUBCLASSES: [&str; 6] = [
        "TypeError",
        "RangeError",
        "SyntaxError",
        "ReferenceError",
        "EvalError",
        "URIError",
    ];

    // A name is "referenced" if it appears as a bare Ident, a
    // `new <N>(...)`, a `.<N>` member, or an `extends <N>` parent.
    let referenced = |n: &str| -> bool {
        ast.exprs.iter().any(|e| {
            matches!(e, Expr::Ident(x) | Expr::New { class_name: x, .. } if x == n)
            // A raise a desugar synthesized (`dflt_param_tdz::
            // tdz_throw_iife`) calls the factory by its own name, which
            // names the class as surely as the program writing it would.
                || matches!(e, Expr::Ident(x) if x.strip_prefix("__new_") == Some(n))
                || matches!(e, Expr::Member { name, .. } if name == n)
            // P7.4-a-2 — `x instanceof <N>` is a genuine reference to
            // <N> (without it `catch (e) { e instanceof TypeError }`
            // would not inject TypeError and the runtime native-error
            // throw could not build a real instance). It needs no arm
            // of its own: the target is an expression, so the name
            // arrives as the `Expr::Ident` the first line matches.
            // RFC 20260815 — `extends <N>` is covered the same way:
            // the heritage is an arena expression now, so its bare
            // name is the `Expr::Ident` the first line matches.
        }) || ast.stmts.iter().any(|s| {
            // P7.4-a-2 — `catch (e: <N>)` annotates the class and
            // expects it to resolve; treat the annotation as a
            // reference so the typed catch + runtime real-instance
            // path both work.
            matches!(s, Stmt::Try { catch_type: Some(t), .. } if t == n)
        })
    };

    // P7.4-a-b — bigint `/ % ** << >>` and `BigInt(x)` can throw a
    // real RangeError at runtime (divide-by-zero / negative exponent /
    // shift too large / non-integer). If the program uses bigint at
    // all, imply RangeError so its `__new_RangeError` factory is
    // registered into the native-error registry (slot 2); otherwise
    // the runtime throw degrades to a bare string instead of a
    // catchable RangeError instance.
    let uses_bigint =
        ast.exprs.iter().any(|e| matches!(e, Expr::BigInt { .. })) || referenced("BigInt");

    // §27.2.4.2 — an all-rejected `Promise.any` answers a freshly
    // built AggregateError, so the combinator is a producer of one
    // exactly the way bigint division is a producer of RangeError.
    // Nothing in `Promise.any([a, b])` names the class, so without
    // this the registry slot stays empty and the runtime falls back
    // to the pre-spec posture of forwarding the last rejection.
    // Imply-only: a program that never mentions `Promise.any` pays
    // nothing, and a user class of the same name still shadows.
    let uses_promise_any = ast.exprs.iter().any(|e| {
        matches!(e, Expr::Member { obj, name } if name == "any"
            && matches!(ast.get_expr(*obj), Expr::Ident(o) if o == "Promise"))
    });

    // §19.2.6 — the four URI globals raise a real URIError on a
    // malformed input, so calling any of them implies the class the
    // same way bigint implies RangeError. Imply-only: programs that
    // never touch a URI global pay nothing.
    let uses_uri_global = [
        "encodeURI",
        "encodeURIComponent",
        "decodeURI",
        "decodeURIComponent",
    ]
    .iter()
    .any(|n| referenced(n));

    // RFC 20260825-injection-reachability 刀 B — the runtime-thrown
    // force-inject applies only when the program can OBSERVE a
    // native raise's instance shape (`instance_shape_observable`,
    // the `_reach` sibling). The first gated build surfaced that
    // "the injected hierarchy exists" had been a LOAD-BEARING
    // invariant far beyond the throw path: the zero-class desugar
    // bail, the iter-result struct-probe's unprotected class-tag
    // read, the struct-intern false match on a not-yet-finalized
    // slot, and the escape-capture census walking past `As` all
    // leaned on it (each fixed on its own merits — they were latent
    // in every program the injection merely reshaped). The gate
    // constant stays as the documented override should a further
    // dependency surface.
    let observable = instance_shape_observable(ast) | GATE_HELD_OPEN;

    // Subclasses to inject: (referenced OR implied OR runtime-thrown)
    // AND not user-shadowed.
    //
    // `runtime_thrown` covers TypeError / RangeError — emitted by
    // runtime helpers (Object.defineProperty on sealed / writes to
    // frozen / `__torajs_throw_type_error` / bigint `/ %` / Array
    // length range / numeric radix range / ...) regardless of user
    // reference. Without auto-inject the native-error registry slot
    // for that class stays unregistered → the helper falls through
    // to a bare-Str throw, breaking `e.message` and
    // `e instanceof TypeError` on the caught value. The fixture
    // workaround "add `const _t = TypeError;` to the program"
    // (check-throw-msg-001.ts, ba8f4ef4) is the same gap surfaced
    // case-by-case; force-inject closes it module-wide. SyntaxError
    // joined the runtime-thrown set with RFC 20260720 刀 5b (the
    // StringToBigInt parse-failure raise); EvalError / URIError are
    // never emitted by runtime helpers, so their reference-gated
    // path stays — programs that never mention them pay no cost.
    let want_sub: Vec<&str> = ERROR_SUBCLASSES
        .iter()
        .copied()
        .filter(|n| {
            let shadowed = user_declares_top_level(ast, n);
            let implied =
                (*n == "RangeError" && uses_bigint) || (*n == "URIError" && uses_uri_global);
            let runtime_thrown = observable
                && matches!(
                    *n,
                    "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
                );
            !shadowed && (runtime_thrown || referenced(n) || implied)
        })
        .collect();

    // The data-carrying subclasses (§20.5.7 / §20.5.8): own params
    // land as own `any` fields ahead of the shared optional message.
    // Reference-gated, plus AggregateError's one implication above —
    // no runtime helper THROWS either of these, but `Promise.any`
    // builds one.
    const DATA_SUBCLASSES: [(&str, &[&str]); 2] = [
        ("AggregateError", &["errors"]),
        ("SuppressedError", &["error", "suppressed"]),
    ];
    let want_data: Vec<(&str, &[&str])> = DATA_SUBCLASSES
        .iter()
        .copied()
        .filter(|(n, _)| {
            let shadowed = user_declares_top_level(ast, n);
            let implied = *n == "AggregateError" && uses_promise_any;
            !shadowed && (referenced(n) || implied)
        })
        .collect();

    // Subclasses extend Error, so any wanted subclass implies Error.
    let want_error = referenced("Error") || !want_sub.is_empty() || !want_data.is_empty();
    if !want_error {
        return;
    }

    // Build Error first, then each wanted subclass, and splice at the
    // front so the parent (`Error`) precedes its children — the
    // field-flattening + declaration-order check in `desugar_classes`
    // requires every ancestor declared before its descendants — and
    // all user references (forward + downstream) resolve here.
    let mut injected: Vec<Stmt> = Vec::with_capacity(1 + want_sub.len() + want_data.len());
    injected.push(build_error_class(ast));
    ast.injected_error_classes.insert("Error".to_string());
    for n in &want_sub {
        injected.push(build_error_subclass(ast, n));
        ast.injected_error_classes.insert((*n).to_string());
    }
    for (n, data_params) in &want_data {
        injected.push(
            super::inject_builtin_classes_data::build_error_data_subclass(ast, n, data_params),
        );
        // §20.5.8 — SuppressedError's ctor length is 3 despite the
        // all-optional params (RFC 20260809 B6 residual); the
        // override statement runs after its class decl.
        if *n == "SuppressedError" {
            injected.push(super::inject_builtin_classes_data::build_length_override(
                ast, n, 3.0,
            ));
        }
        ast.injected_error_classes.insert((*n).to_string());
    }
    ast.intrinsic_classes
        .extend(ast.injected_error_classes.iter().cloned());
    ast.stmts.splice(0..0, injected);
}

/// The named class is `Error`, a NativeError, or reaches one through
/// its `extends` chain — the compile-time face of the runtime's
/// inherited FLAG_ERROR. Instances of these own `message` / `stack`
/// with [[Enumerable]]: false (§20.5.6.1.1), which the static layout
/// cannot express, so checker and lowering both use this to route
/// their enumerable-only surfaces through the runtime own-walk.
pub(crate) fn class_reaches_error(ast: &Ast, name: &str) -> bool {
    let mut name = name;
    // The bound guards against a cycle in a malformed parents map; any
    // real chain is a handful deep.
    for _ in 0..64 {
        if ast.injected_error_classes.contains(name) {
            return true;
        }
        match ast.class_parents.get(name) {
            Some(Some(parent)) => name = parent,
            _ => return false,
        }
    }
    false
}
