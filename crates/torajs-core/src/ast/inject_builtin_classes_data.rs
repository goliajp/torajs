//! §20.5.7 / §20.5.8 data-carrying Error subclass synthesis —
//! sibling of `inject_builtin_classes` (the parent file sat at 496
//! lines; the shared one-shape builder stays there, the data-param
//! variant lives here).

use super::{Ast, ClassCtor, Expr, Param, PropKey, Stmt};

/// §20.5.7 AggregateError / §20.5.8 SuppressedError — the two
/// standard Error subclasses whose constructors carry own DATA
/// params ahead of the shared optional message:
///
/// ```ts
/// class AggregateError extends Error {
///   errors: any;
///   constructor(errors: any, message: string = <undef sentinel>) {
///     super(message);
///     this.errors = errors;
///   }
/// }
/// ```
///
/// (SuppressedError is the same shape with `error` + `suppressed`.)
/// Each data param lands as an own `any` field — the values are
/// arbitrary (an errors list, a pair of exception values) and the
/// spec installs them as data properties, not typed slots. MVP
/// boundary, loud where visible: AggregateError stores the errors
/// argument itself rather than an IterableToList snapshot.
///
/// The trailing `options` param is the §20.5.8.1 error-cause face.
/// It is only forwarded from here — `Error`'s ctor holds the single
/// copy of the InstallErrorCause test.
/// RFC 20260809 B6 — whether a data-carrying subclass's own params
/// are OPTIONAL. §20.5.8 SuppressedError treats all three of
/// (error, suppressed, message) as plain arguments — `new
/// SuppressedError()` constructs with both own fields undefined.
/// §20.5.7 AggregateError is different: step 3 iterates `errors`, so
/// a missing argument is a RUNTIME TypeError (undefined is not
/// iterable), and the checker's required-param face stays the loud
/// stand-in until IterableToList lands.
fn data_params_optional(sub_name: &str) -> bool {
    sub_name == "SuppressedError"
}

/// `Object.defineProperty(<class>, "length", { value: <len> })` —
/// §20.5.8 pins SuppressedError's ctor `length` at 3 even though the
/// TS-source params carry defaults (a default-carrying param drops
/// out of the natural length count, and `new SuppressedError()` must
/// stay legal, so the descriptor override is the only spelling that
/// satisfies both faces). Runs after the class decl in the injected
/// prefix.
pub(super) fn build_length_override(ast: &mut Ast, class_name: &str, len: f64) -> Stmt {
    let obj = ast.add_expr(Expr::Ident("Object".to_string()));
    let dp = ast.add_expr(Expr::Member {
        obj,
        name: "defineProperty".to_string(),
    });
    let target = ast.add_expr(Expr::Ident(class_name.to_string()));
    let key = ast.add_expr(Expr::String("length".to_string().into()));
    let value = ast.add_expr(Expr::Number(len));
    let desc = ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), value)],
    });
    let call = ast.add_expr(Expr::Call {
        callee: dp,
        args: vec![target, key, desc],
    });
    Stmt::Expr(call)
}

pub(super) fn build_error_data_subclass(
    ast: &mut Ast,
    sub_name: &str,
    data_params: &[&str],
) -> Stmt {
    let msg_ident = ast.add_expr(Expr::Ident("__bi_message".to_string()));
    // §20.5.8.1 — forward `options` to Error's ctor, which owns the
    // single copy of the InstallErrorCause test.
    let opts_ident = ast.add_expr(Expr::Ident("__bi_options".to_string()));
    let super_call = ast.add_expr(Expr::Super {
        args: vec![msg_ident, opts_ident],
    });
    // §20.5.3.2 — the class name belongs to `<N>.prototype.name`,
    // where `__torajs_error_proto_install` already put it; writing it
    // into the instance field shadowed that with an enumerable own
    // copy. `build_error_subclass` retired the same pair of statements
    // for the same reason (the `stack` re-run below went with it —
    // Error's ctor resolves the name off the receiver's prototype
    // chain, so its header line already reads `<N>: msg`).
    let mut body = vec![Stmt::Expr(super_call)];
    for p in data_params {
        let this_ref = ast.add_expr(Expr::This);
        let field = ast.add_expr(Expr::Member {
            obj: this_ref,
            name: (*p).to_string(),
        });
        let value = ast.add_expr(Expr::Ident(format!("__bi_{p}")));
        let assign = ast.add_expr(Expr::Assign {
            target: field,
            value,
        });
        body.push(Stmt::Expr(assign));
    }
    // Plain `undefined` default (Error-root message install resolves
    // absence; the sentinel must not ride the `any` param).
    let msg_default = ast.add_expr(Expr::Ident("undefined".to_string()));
    let optional = data_params_optional(sub_name);
    let mut params: Vec<Param> = data_params
        .iter()
        .map(|p| Param {
            name: format!("__bi_{p}"),
            type_ann: Some("any".to_string()),
            default: optional.then(|| ast.add_expr(Expr::Ident("undefined".to_string()))),
            is_rest: false,
        })
        .collect();
    params.push(Param {
        name: "__bi_message".to_string(),
        type_ann: Some("any".to_string()),
        default: Some(msg_default),
        is_rest: false,
    });
    params.push(super::inject_builtin_classes_cause::build_options_param(
        ast,
    ));

    let parent_ident = ast.add_expr(Expr::Ident("Error".to_string()));
    Stmt::ClassDecl {
        name: sub_name.to_string(),
        type_params: Vec::new(),
        parent: Some(parent_ident),
        is_abstract: false,
        fields: data_params
            .iter()
            .map(|p| (PropKey::from(*p), "any".to_string()))
            .collect(),
        static_init: Vec::new(),
        ctor: Some(ClassCtor { params, body }),
        methods: Vec::new(),
        static_methods: Vec::new(),
    }
}
