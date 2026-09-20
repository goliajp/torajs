//! The synthesized `Error` hierarchy's class bodies — the root
//! §20.5.1 `class Error` and the §20.5.5 NativeError subclass shape,
//! plus the two expression mints their ctors share.
//!
//! Split out of [`super::inject_builtin_classes`] verbatim in
//! rotation 604. That file answers *which* classes a program gets and
//! where they are spliced; this one answers *what one of them is
//! made of*. It was 211 of the file's 500 lines, and the file was at
//! the size limit exactly — the shadow test the same rotation had to
//! widen had nowhere to go.

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

pub(super) fn build_error_class(ast: &mut Ast) -> Stmt {
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
        name: "isError".into(),
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

    let decl_id = ast.mint_class_decl_id();
    Stmt::ClassDecl {
        name: "Error".to_string(),
        decl_id,
        type_params: Vec::new(),
        parent: None,
        is_abstract: false,
        fields: vec![
            ("message".into(), "string".to_string()),
            ("name".into(), "string".to_string()),
            ("stack".into(), "string".to_string()),
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
pub(super) fn build_error_subclass(ast: &mut Ast, sub_name: &str) -> Stmt {
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
    let decl_id = ast.mint_class_decl_id();
    Stmt::ClassDecl {
        name: sub_name.to_string(),
        decl_id,
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
