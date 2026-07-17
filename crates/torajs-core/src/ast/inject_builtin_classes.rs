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
//!   * `build_stack_concat` — synth `.stack` init Ternary expr per
//!     ECMAScript §20.5.3.4 (empty-message → name only; else
//!     `name + ": " + message`).
//!   * `build_error_class` — synth root `class Error`.
//!   * `build_error_subclass` — synth `class <N> extends Error` for
//!     each requested NativeError subclass.

use super::{Ast, BinOp, ClassCtor, Expr, ExprId, Param, Stmt};

/// Synthetic root `class Error { message: string; name: string;
/// constructor(message: string) { this.message = message;
/// this.name = "Error"; } }` (spec §20.5.1). Field order matters:
/// it must match the ctor-body assignment order since check.rs's
/// affine-flow analysis walks declarations in order.
/// P7.3 — build the header expression shared by Error / subclass
/// ctors as the minimal `.stack` value. Follows ECMAScript
/// §20.5.3.4 `Error.prototype.toString` (the stack's first line uses
/// the same format in every engine): an empty `message` yields just
/// the name — `new Error("").stack` is "Error", not "Error: " (bun /
/// V8 / JSC all do this). The §20.5.3.4 empty-`name` branch is
/// unreachable for the injected classes (name is always a non-empty
/// literal), so only the empty-message case is special-cased:
///
///   this.message === "" ? this.name : this.name + ": " + this.message
fn build_stack_concat(ast: &mut Ast) -> ExprId {
    // cond: this.message === ""
    let cm_obj = ast.add_expr(Expr::This);
    let cm = ast.add_expr(Expr::Member {
        obj: cm_obj,
        name: "message".to_string(),
    });
    let empty = ast.add_expr(Expr::String(String::new()));
    let cond = ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: cm,
        right: empty,
    });
    // then: this.name
    let tn_obj = ast.add_expr(Expr::This);
    let then_branch = ast.add_expr(Expr::Member {
        obj: tn_obj,
        name: "name".to_string(),
    });
    // else: this.name + ": " + this.message
    let n_obj = ast.add_expr(Expr::This);
    let n = ast.add_expr(Expr::Member {
        obj: n_obj,
        name: "name".to_string(),
    });
    let colon = ast.add_expr(Expr::String(": ".to_string()));
    let n_colon = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: n,
        right: colon,
    });
    let m_obj = ast.add_expr(Expr::This);
    let m = ast.add_expr(Expr::Member {
        obj: m_obj,
        name: "message".to_string(),
    });
    let else_branch = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: n_colon,
        right: m,
    });
    ast.add_expr(Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    })
}

fn build_error_class(ast: &mut Ast) -> Stmt {
    let this0 = ast.add_expr(Expr::This);
    let msg_member = ast.add_expr(Expr::Member {
        obj: this0,
        name: "message".to_string(),
    });
    let msg_value = ast.add_expr(Expr::Ident("message".to_string()));
    let assign1 = ast.add_expr(Expr::Assign {
        target: msg_member,
        value: msg_value,
    });
    let this1 = ast.add_expr(Expr::This);
    let name_member = ast.add_expr(Expr::Member {
        obj: this1,
        name: "name".to_string(),
    });
    let name_value = ast.add_expr(Expr::String("Error".to_string()));
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

    let ctor = ClassCtor {
        params: vec![Param {
            name: "message".to_string(),
            type_ann: Some("string".to_string()),
            default: None,
            is_rest: false,
        }],
        body: vec![
            Stmt::Expr(assign1),
            Stmt::Expr(assign2),
            Stmt::Expr(assign3),
        ],
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
        static_methods: Vec::new(),
    }
}

/// Synthetic `class <N> extends Error { constructor(message: string)
/// { super(message); this.name = "<N>"; } }` for the four standard
/// NativeError subclasses (spec §20.5.5). No own fields: `message` /
/// `name` are inherited from Error via desugar field-flattening
/// (which panics on parent-field redeclaration), so the ctor just
/// forwards to Error's ctor through `super` and overrides `name`.
/// The `super(message)` site is rewritten to `__cm_Error__ctor(...)`
/// by the existing Pass 1.5 super-rewrite in `desugar_classes` —
/// identical to a user-written subclass.
fn build_error_subclass(ast: &mut Ast, sub_name: &str) -> Stmt {
    let msg_ident = ast.add_expr(Expr::Ident("message".to_string()));
    let super_call = ast.add_expr(Expr::Super {
        args: vec![msg_ident],
    });
    let this0 = ast.add_expr(Expr::This);
    let name_member = ast.add_expr(Expr::Member {
        obj: this0,
        name: "name".to_string(),
    });
    let name_value = ast.add_expr(Expr::String(sub_name.to_string()));
    let assign_name = ast.add_expr(Expr::Assign {
        target: name_member,
        value: name_value,
    });
    // P7.3 — re-set this.stack AFTER overriding name so it reflects
    // the subclass name ("TypeError: msg"), not Error's. The Error
    // ctor reached via super() already set a "Error: msg" stack;
    // this overwrites it on the same inherited field.
    let stack_expr = build_stack_concat(ast);
    let stack_obj = ast.add_expr(Expr::This);
    let stack_member = ast.add_expr(Expr::Member {
        obj: stack_obj,
        name: "stack".to_string(),
    });
    let assign_stack = ast.add_expr(Expr::Assign {
        target: stack_member,
        value: stack_expr,
    });

    let ctor = ClassCtor {
        params: vec![Param {
            name: "message".to_string(),
            type_ann: Some("string".to_string()),
            default: None,
            is_rest: false,
        }],
        body: vec![
            Stmt::Expr(super_call),
            Stmt::Expr(assign_name),
            Stmt::Expr(assign_stack),
        ],
    };

    Stmt::ClassDecl {
        name: sub_name.to_string(),
        type_params: Vec::new(),
        parent: Some("Error".to_string()),
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

    // The four standard NativeError subclasses (spec §20.5.5).
    const ERROR_SUBCLASSES: [&str; 4] =
        ["TypeError", "RangeError", "SyntaxError", "ReferenceError"];

    // A name is "referenced" if it appears as a bare Ident, a
    // `new <N>(...)`, a `.<N>` member, or an `extends <N>` parent.
    let referenced = |n: &str| -> bool {
        ast.exprs.iter().any(|e| {
            matches!(e, Expr::Ident(x) | Expr::New { class_name: x, .. } if x == n)
                || matches!(e, Expr::Member { name, .. } if name == n)
                // P7.4-a-2 — `x instanceof <N>` is a genuine reference
                // to <N>; without this `catch (e) { e instanceof
                // TypeError }` wouldn't inject TypeError and the
                // runtime native-error throw couldn't build a real
                // instance for it.
                || matches!(e, Expr::InstanceOf { class_name, .. } if class_name == n)
        }) || ast.stmts.iter().any(|s| {
            matches!(s, Stmt::ClassDecl { parent: Some(p), .. } if p == n)
                // P7.4-a-2 — `catch (e: <N>)` annotates the class and
                // expects it to resolve; treat the annotation as a
                // reference so the typed catch + runtime real-instance
                // path both work.
                || matches!(s, Stmt::Try { catch_type: Some(t), .. } if t == n)
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
    // / ReferenceError are never emitted by runtime helpers (parse-
    // time / type-check-time only), so their reference-gated path
    // stays — programs that never mention them pay no cost.
    let want_sub: Vec<&str> = ERROR_SUBCLASSES
        .iter()
        .copied()
        .filter(|n| {
            let shadowed = ast
                .stmts
                .iter()
                .any(|s| matches!(s, Stmt::ClassDecl { name, .. } if name == *n));
            let implied = *n == "RangeError" && uses_bigint;
            let runtime_thrown = matches!(*n, "TypeError" | "RangeError");
            !shadowed && (runtime_thrown || referenced(n) || implied)
        })
        .collect();

    // Subclasses extend Error, so any wanted subclass implies Error.
    let want_error = referenced("Error") || !want_sub.is_empty();
    if !want_error {
        return;
    }

    // Build Error first, then each wanted subclass, and splice at the
    // front so the parent (`Error`) precedes its children — the
    // field-flattening + declaration-order check in `desugar_classes`
    // requires every ancestor declared before its descendants — and
    // all user references (forward + downstream) resolve here.
    let mut injected: Vec<Stmt> = Vec::with_capacity(1 + want_sub.len());
    injected.push(build_error_class(ast));
    ast.injected_error_classes.insert("Error".to_string());
    for n in &want_sub {
        injected.push(build_error_subclass(ast, n));
        ast.injected_error_classes.insert((*n).to_string());
    }
    ast.stmts.splice(0..0, injected);
}
