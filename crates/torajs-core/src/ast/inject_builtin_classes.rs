//! `Error` + native-error subclass injection pass — chunk 362,
//! extracted from ast.rs.
//!
//! Pub entry `inject_builtin_classes` (P4.6 / P7.1) synthesizes
//! `class Error { message; name; stack; }` + the four spec §20.5.5
//! subclasses (`TypeError` / `RangeError` / `SyntaxError` /
//! `ReferenceError`) as `Stmt::ClassDecl` and splices them at
//! `ast.stmts[0..0]` so the rest of the desugar pipeline processes
//! them as ordinary user classes. Injection is idempotent +
//! usage-gated (user `class Error` overrides; each subclass gated
//! on reference/implied/runtime-thrown minus user-shadowing).
//!
//! Three private helpers cooperate:
//!   * `build_stack_concat` — synth `.stack` init
//!     (`__torajs_error_stack(this)` → the §20.5.3.4 runtime
//!     formatter; empty/absent message → name only).
//!   * `build_error_class` — synth root `class Error`.
//!   * `build_error_subclass` — synth `class <N> extends Error` for
//!     each requested NativeError subclass.
//!
//! Three siblings carry the pieces that would push this file past
//! the 500-line limit: `inject_builtin_classes_cause` (§20.5.8.1,
//! the `options` / `cause` face every ctor accepts),
//! `inject_builtin_classes_data` (§20.5.7 / §20.5.8, the subclasses
//! whose ctors carry own data params ahead of `message`) and
//! `inject_builtin_classes_message` (§20.5.1.1 step 3, the root
//! ctor's message coercion + own-absence install).

use super::inject_builtin_classes_cause::{build_install_cause, build_options_param};
use super::inject_builtin_classes_message::build_message_install;
use super::{Ast, ClassCtor, ClassMethod, Expr, ExprId, Param, Stmt, Visibility};

/// Synthetic root `class Error { message: string; name: string;
/// constructor(message: string) { this.message = message;
/// this.name = "Error"; } }` (spec §20.5.1). Field order matters:
/// it must match the ctor-body assignment order since check.rs's
/// affine-flow analysis walks declarations in order.
/// P7.3 — build the header expression shared by Error / subclass
/// ctors as the minimal `.stack` value: `__torajs_error_stack(this)`,
/// lowered to the `__torajs_error_to_string` runtime helper
/// (ECMAScript §20.5.3.4 — the stack's first line uses the same
/// format in every engine; an empty OR absent `message` yields just
/// the name, which the old AST-level `=== ""` ternary couldn't see
/// once absence became the sentinel — RFC
/// 20260718-error-message-own-prop 刀 2).
pub(super) fn build_stack_concat(ast: &mut Ast) -> ExprId {
    let this = ast.add_expr(Expr::This);
    let callee = ast.add_expr(Expr::Ident("__torajs_error_stack".to_string()));
    ast.add_expr(Expr::Call {
        callee,
        args: vec![this],
    })
}

/// The own-absence Str sentinel (`__torajs_undef_str()`, GlobalRef
/// mint). A slot holding it carries no own property at all, which is
/// how two of the injected layout's fields express a spec shape a
/// declared field cannot:
///
/// - the `message` param's default (§20.5.1.1 — a missing message
///   defines no own `message`; pre-fix the default was `""`, which
///   owned the property on every no-arg construction).
/// - the `name` slot unconditionally (§20.5.3.2 — `name` lives on
///   `Error.prototype`, so an instance owns one only when user code
///   assigns it and overwrites the sentinel).
pub(super) fn build_absent_sentinel(ast: &mut Ast) -> ExprId {
    let callee = ast.add_expr(Expr::Ident("__torajs_undef_str".to_string()));
    ast.add_expr(Expr::Call {
        callee,
        args: Vec::new(),
    })
}

fn build_error_class(ast: &mut Ast) -> Stmt {
    let install_message = build_message_install(ast);
    // §20.5.3.2 — `name` is `Error.prototype`'s own property, not the
    // instance's; `__torajs_error_proto_install` already puts it there
    // with the spec `{W:1, E:0, C:1}` attributes. Writing the class
    // name into the field too shadowed that entry with a second,
    // enumerable copy, so `Object.keys(new Error("m"))` listed `name`
    // where every engine lists nothing. The slot instead carries the
    // own-absence sentinel: reads fall through to the prototype, and
    // a user's `this.name = "..."` overwrites it into a real own
    // property — which IS enumerable, exactly as bun reports.
    let this1 = ast.add_expr(Expr::This);
    let name_member = ast.add_expr(Expr::Member {
        obj: this1,
        name: "name".to_string(),
    });
    let name_value = build_absent_sentinel(ast);
    let assign2 = ast.add_expr(Expr::Assign {
        target: name_member,
        value: name_value,
    });
    // P7.3 — this.stack = this.name + ": " + this.message. Minimal
    // header-only stack (the Node `Error.stackTraceLimit = 0` shape,
    // a real-engine-legal value): `.stack` exists and is a string so
    // code reading it doesn't fault. No synthesized frames — fake
    // "at file:line" would be silent-wrong; real frame capture is a
    // separate perf-sensitive substrate (P7.3-frames). Set last so
    // it reflects the final name/message; subclasses re-run this
    // after overriding `name`.
    let stack_expr = build_stack_concat(ast);
    let stack_obj = ast.add_expr(Expr::This);
    let stack_member = ast.add_expr(Expr::Member {
        obj: stack_obj,
        name: "stack".to_string(),
    });
    let assign3 = ast.add_expr(Expr::Assign {
        target: stack_member,
        value: stack_expr,
    });

    // §20.5.1.1 — `message` is optional (`new Error()` is legal).
    // The default is plain `undefined`, folding the absent case into
    // the explicit-undefined arm of `build_message_install` — the
    // sentinel itself must NOT ride the `any` param (see that
    // builder's doc for the identity-stripping rationale).
    let msg_default = ast.add_expr(Expr::Ident("undefined".to_string()));
    // §20.5.8.1 runs last: `cause` is installed after `stack`, so a
    // reader walking own keys sees the declared fields first and the
    // conditional one behind them.
    let install_cause = build_install_cause(ast);
    let options_param = build_options_param(ast);
    let ctor = ClassCtor {
        params: vec![
            Param {
                name: "__bi_message".to_string(),
                type_ann: Some("any".to_string()),
                default: Some(msg_default),
                is_rest: false,
            },
            options_param,
        ],
        body: vec![
            install_message,
            Stmt::Expr(assign2),
            Stmt::Expr(assign3),
            install_cause,
        ],
    };

    // RFC 20260718 刀 3 — `static isError(x)` (ES2025 §20.5.2.1)
    // rides the ordinary static-method pipeline (desugar emits
    // `__sm_Error__isError`, class_globals reifies the own function
    // entry on `__class_Error`); the body is the runtime
    // [[ErrorData]] probe (`FLAG_ERROR` header bit).
    let probe_arg = ast.add_expr(Expr::Ident("__bi_x".to_string()));
    let probe_callee = ast.add_expr(Expr::Ident("__torajs_error_is_error".to_string()));
    let probe_call = ast.add_expr(Expr::Call {
        callee: probe_callee,
        args: vec![probe_arg],
    });
    let is_error = ClassMethod {
        name: "isError".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            name: "__bi_x".to_string(),
            type_ann: Some("any".to_string()),
            default: None,
            is_rest: false,
        }],
        return_type: Some("boolean".to_string()),
        body: vec![Stmt::Return(Some(probe_call))],
        is_abstract: false,
        visibility: Visibility::Public,
        accessor_kind: None,
        span: crate::lexer::Span { start: 0, end: 0 },
    };

    Stmt::ClassDecl {
        name: "Error".to_string(),
        type_params: Vec::new(),
        parent: None,
        is_abstract: false,
        fields: vec![
            ("message".to_string(), "string".to_string()),
            ("name".to_string(), "string".to_string()),
            ("stack".to_string(), "string".to_string()),
        ],
        static_init: Vec::new(),
        ctor: Some(ctor),
        methods: Vec::new(),
        static_methods: vec![is_error],
    }
}

/// Synthetic `class <N> extends Error { constructor(message: string)
/// { super(message); } }` for the four standard NativeError
/// subclasses (spec §20.5.5). No own fields: `message` / `name` are
/// inherited from Error via desugar field-flattening (which panics on
/// parent-field redeclaration), so the ctor only forwards to Error's
/// ctor through `super`. The `super(message)` site is rewritten to
/// `__cm_Error__ctor(...)` by the existing Pass 1.5 super-rewrite in
/// `desugar_classes` — identical to a user-written subclass.
///
/// The body used to carry two more statements, and the `name` fix
/// (§20.5.3.2) retires both: `this.name = "<N>"` is what shadowed
/// `<N>.prototype.name` with an enumerable own copy, and the `stack`
/// re-run existed only to pick the overridden name back up. Error's
/// ctor now resolves the name off the receiver's own prototype chain,
/// so the header line it writes already reads `<N>: msg`.
fn build_error_subclass(ast: &mut Ast, sub_name: &str) -> Stmt {
    let msg_ident = ast.add_expr(Expr::Ident("__bi_message".to_string()));
    // §20.5.8.1 is installed once, in Error's ctor; every subclass
    // only has to forward `options` to it rather than repeat the test.
    let opts_ident = ast.add_expr(Expr::Ident("__bi_options".to_string()));
    let super_call = ast.add_expr(Expr::Super {
        args: vec![msg_ident, opts_ident],
    });

    // Same §20.5.1.1 optional-message face as the Error root ctor,
    // and the same §20.5.8.1 options tail it forwards to. Plain
    // `undefined` default — absence resolves in the root ctor's
    // message install; the sentinel must not ride the `any` param.
    let msg_default = ast.add_expr(Expr::Ident("undefined".to_string()));
    let options_param = build_options_param(ast);
    let ctor = ClassCtor {
        params: vec![
            Param {
                name: "__bi_message".to_string(),
                type_ann: Some("any".to_string()),
                default: Some(msg_default),
                is_rest: false,
            },
            options_param,
        ],
        body: vec![Stmt::Expr(super_call)],
    };

    let parent_ident = ast.add_expr(Expr::Ident("Error".to_string()));
    Stmt::ClassDecl {
        name: sub_name.to_string(),
        type_params: Vec::new(),
        parent: Some(parent_ident),
        is_abstract: false,
        fields: Vec::new(),
        static_init: Vec::new(),
        ctor: Some(ctor),
        methods: Vec::new(),
        static_methods: Vec::new(),
    }
}

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
///   user-shadowed by a same-named ClassDecl.
/// - `Error` is injected when referenced directly OR implied by any
///   wanted subclass (subclasses extend it). Nothing is injected for
///   programs that never mention the error hierarchy (compile-time
///   neutral).
pub fn inject_builtin_classes(ast: &mut Ast) {
    let user_has_error = ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::ClassDecl { name, .. } if name == "Error"));
    if user_has_error {
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
            let shadowed = ast
                .stmts
                .iter()
                .any(|s| matches!(s, Stmt::ClassDecl { name, .. } if name == *n));
            let implied =
                (*n == "RangeError" && uses_bigint) || (*n == "URIError" && uses_uri_global);
            let runtime_thrown = matches!(
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
            let shadowed = ast
                .stmts
                .iter()
                .any(|s| matches!(s, Stmt::ClassDecl { name, .. } if name == n));
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
