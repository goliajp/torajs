//! §20.5.7 / §20.5.8 data-carrying Error subclass synthesis —
//! sibling of `inject_builtin_classes` (the parent file sat at 496
//! lines; the shared one-shape builder stays there, the data-param
//! variant lives here).

use super::inject_builtin_classes::{build_msg_default, build_stack_concat};
use super::{Ast, ClassCtor, Expr, Param, Stmt};

/// §20.5.7 AggregateError / §20.5.8 SuppressedError — the two
/// standard Error subclasses whose constructors carry own DATA
/// params ahead of the shared optional message:
///
/// ```ts
/// class AggregateError extends Error {
///   errors: any;
///   constructor(errors: any, message: string = <undef sentinel>) {
///     super(message);
///     this.name = "AggregateError";
///     this.errors = errors;
///     this.stack = __torajs_error_stack(this);
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
pub(super) fn build_error_data_subclass(
    ast: &mut Ast,
    sub_name: &str,
    data_params: &[&str],
) -> Stmt {
    let msg_ident = ast.add_expr(Expr::Ident("message".to_string()));
    // §20.5.8.1 — forward `options` to Error's ctor, which owns the
    // single copy of the InstallErrorCause test.
    let opts_ident = ast.add_expr(Expr::Ident("options".to_string()));
    let super_call = ast.add_expr(Expr::Super {
        args: vec![msg_ident, opts_ident],
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

    let mut body = vec![Stmt::Expr(super_call), Stmt::Expr(assign_name)];
    for p in data_params {
        let this_ref = ast.add_expr(Expr::This);
        let field = ast.add_expr(Expr::Member {
            obj: this_ref,
            name: (*p).to_string(),
        });
        let value = ast.add_expr(Expr::Ident((*p).to_string()));
        let assign = ast.add_expr(Expr::Assign {
            target: field,
            value,
        });
        body.push(Stmt::Expr(assign));
    }
    // Same P7.3 posture as build_error_subclass: re-set the stack
    // AFTER the name override so it reads "<SubName>: msg".
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
    body.push(Stmt::Expr(assign_stack));

    let msg_default = build_msg_default(ast);
    let mut params: Vec<Param> = data_params
        .iter()
        .map(|p| Param {
            name: (*p).to_string(),
            type_ann: Some("any".to_string()),
            default: None,
            is_rest: false,
        })
        .collect();
    params.push(Param {
        name: "message".to_string(),
        type_ann: Some("string".to_string()),
        default: Some(msg_default),
        is_rest: false,
    });
    params.push(super::inject_builtin_classes_cause::build_options_param(
        ast,
    ));

    Stmt::ClassDecl {
        name: sub_name.to_string(),
        type_params: Vec::new(),
        parent: Some("Error".to_string()),
        is_abstract: false,
        fields: data_params
            .iter()
            .map(|p| ((*p).to_string(), "any".to_string()))
            .collect(),
        static_init: Vec::new(),
        ctor: Some(ClassCtor { params, body }),
        methods: Vec::new(),
        static_methods: Vec::new(),
    }
}
